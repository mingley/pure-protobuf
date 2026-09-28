//! Optional OpenTelemetry metrics bridge (A66/A79/A94/A108 subset).
//!
//! [`Metrics`] is a [`LifecycleObserver`](crate::LifecycleObserver) that
//! records gRFC-named instruments. Install it with the existing
//! observer builders ([`Channel::observer`](crate::Channel::observer),
//! [`Server::observer`](crate::server::Server::observer),
//! [`Router::observer`](crate::Router::observer)); observers stack, so
//! it composes with logging or channelz-side observers.
//!
//! The module compiles out without the `otel` feature. With the
//! feature enabled but no observer installed, the pre-existing
//! `Option` checks keep the overhead at zero (see the dev-loop
//! evidence in GF-01).
//!
//! ## Implemented instruments (names per the gRFCs)
//!
//! | Instrument | Type | Unit | Labels |
//! |---|---|---|---|
//! | `grpc.client.attempt.started` (A66) | Counter | `{attempt}` | method, target |
//! | `grpc.client.attempt.duration` (A66) | Histogram | `s` | method, target, status |
//! | `grpc.client.call.duration` (A66) | Histogram | `s` | method, target, status |
//! | `grpc.server.call.started` (A66) | Counter | `{call}` | method |
//! | `grpc.server.call.duration` (A66) | Histogram | `s` | method, status |
//! | `grpc.subchannel.connection_attempts_succeeded` (A94) | Counter | `{attempt}` | target |
//! | `grpc.subchannel.connection_attempts_failed` (A94) | Counter | `{attempt}` | target |
//!
//! `grpc.method` is the full `service/Method` path; `grpc.target` is
//! the channel authority or dial target; `grpc.status` is the
//! canonical code name (`OK`, `CANCELLED`, ...). Static custom
//! attributes ([`Metrics::with_custom_attributes`]) ride every
//! per-call instrument (A108, channel-level values).
//!
//! ## Deferred to GF-01b (needs hook/plumbing changes outside GF-01)
//!
//! - A66 message-size histograms: per-attempt byte attribution needs
//!   call/attempt ids on the byte hooks.
//! - A96 retry instruments: per-call attempt accounting needs call
//!   ids plus transparent-retry flags on the attempt guards.
//! - A94 `disconnections` / `open_connections`: need transport
//!   close/disconnect hooks in the pool.
//! - A78 WRR gauges and xDS client instruments: need LB-policy and
//!   xDS-client hooks (XD lane for the latter).
//! - A108 per-RPC dynamic label values: need a tags channel from the
//!   call site to the observer.
//! - Server `grpc.method` collapsing unregistered methods to
//!   `other`: needs dispatch registration knowledge at the hook.
//! - A66 recommended histogram bucket boundaries: use SDK Views; the
//!   bridge keeps SDK defaults.

use crate::Status;
use crate::telemetry::{AttemptLabels, CallLabels, LifecycleObserver, ReconnectEvent};
use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::{KeyValue, global};
use std::sync::Arc;

/// W3C propagation plus server spans (GF-02, A72).
pub mod trace;

/// Attribute keys per A66/A79/A94.
pub mod label {
    /// Full `service/Method` name.
    pub const METHOD: &str = "grpc.method";
    /// Canonical dial target / authority.
    pub const TARGET: &str = "grpc.target";
    /// Canonical status code name.
    pub const STATUS: &str = "grpc.status";
}

/// gRFC instrument names (A66/A94).
pub mod instrument {
    /// Total number of client RPC attempts started (A66 Counter).
    pub const CLIENT_ATTEMPT_STARTED: &str = "grpc.client.attempt.started";
    /// End-to-end time to complete a client RPC attempt (A66 Histogram).
    pub const CLIENT_ATTEMPT_DURATION: &str = "grpc.client.attempt.duration";
    /// End-to-end time to complete a client RPC (A66 Histogram).
    pub const CLIENT_CALL_DURATION: &str = "grpc.client.call.duration";
    /// Total number of server RPCs started (A66 Counter).
    pub const SERVER_CALL_STARTED: &str = "grpc.server.call.started";
    /// End-to-end time of a server RPC (A66 Histogram).
    pub const SERVER_CALL_DURATION: &str = "grpc.server.call.duration";
    /// Successful subchannel connection attempts (A94 Counter).
    pub const SUBCHANNEL_CONN_SUCCEEDED: &str = "grpc.subchannel.connection_attempts_succeeded";
    /// Failed subchannel connection attempts (A94 Counter).
    pub const SUBCHANNEL_CONN_FAILED: &str = "grpc.subchannel.connection_attempts_failed";
}

#[derive(Clone)]
struct Instruments {
    attempt_started: Counter<u64>,
    attempt_duration: Histogram<f64>,
    call_duration: Histogram<f64>,
    server_call_started: Counter<u64>,
    server_call_duration: Histogram<f64>,
    conn_succeeded: Counter<u64>,
    conn_failed: Counter<u64>,
}

impl Instruments {
    fn new(meter: Meter) -> Self {
        Self {
            attempt_started: meter
                .u64_counter(instrument::CLIENT_ATTEMPT_STARTED)
                .with_unit("{attempt}")
                .with_description("Total number of RPC attempts started (A66).")
                .build(),
            attempt_duration: meter
                .f64_histogram(instrument::CLIENT_ATTEMPT_DURATION)
                .with_unit("s")
                .with_description("End-to-end time to complete an RPC attempt (A66).")
                .build(),
            call_duration: meter
                .f64_histogram(instrument::CLIENT_CALL_DURATION)
                .with_unit("s")
                .with_description("End-to-end time to complete an RPC from the app view (A66).")
                .build(),
            server_call_started: meter
                .u64_counter(instrument::SERVER_CALL_STARTED)
                .with_unit("{call}")
                .with_description("Total number of server RPCs started (A66).")
                .build(),
            server_call_duration: meter
                .f64_histogram(instrument::SERVER_CALL_DURATION)
                .with_unit("s")
                .with_description("End-to-end time of a server RPC (A66).")
                .build(),
            conn_succeeded: meter
                .u64_counter(instrument::SUBCHANNEL_CONN_SUCCEEDED)
                .with_unit("{attempt}")
                .with_description("Successful subchannel connection attempts (A94).")
                .build(),
            conn_failed: meter
                .u64_counter(instrument::SUBCHANNEL_CONN_FAILED)
                .with_unit("{attempt}")
                .with_description("Failed subchannel connection attempts (A94).")
                .build(),
        }
    }
}

/// OpenTelemetry metrics recorder. Cheap to clone; install with
/// `.observer(Metrics::new())` on channels and servers.
#[derive(Clone)]
pub struct Metrics {
    inner: Arc<Inner>,
}

#[derive(Clone)]
struct Inner {
    instruments: Instruments,
    /// Static custom attributes (A108, channel level) attached to
    /// every per-call measurement.
    custom: Vec<KeyValue>,
}

impl Metrics {
    /// Record through the global meter provider.
    #[must_use]
    pub fn new() -> Self {
        Self::with_meter(global::meter_provider().meter("pbrs-grpc"))
    }

    /// Record through an explicit meter (tests, custom providers).
    #[must_use]
    pub fn with_meter(meter: Meter) -> Self {
        Self {
            inner: Arc::new(Inner {
                instruments: Instruments::new(meter),
                custom: Vec::new(),
            }),
        }
    }

    /// Attach static custom attributes to every per-call measurement
    /// (A108, channel-level values; per-RPC dynamic values need a
    /// tags channel and ride GF-01b).
    #[must_use]
    pub fn with_custom_attributes(self, attrs: Vec<KeyValue>) -> Self {
        let mut inner = Arc::unwrap_or_clone(self.inner);
        inner.custom = attrs;
        Self {
            inner: Arc::new(inner),
        }
    }

    fn method_of(call: &CallLabels<'_>) -> KeyValue {
        KeyValue::new(
            label::METHOD,
            call.path.strip_prefix('/').unwrap_or(call.path).to_owned(),
        )
    }

    fn target_of(call: &CallLabels<'_>) -> Option<KeyValue> {
        call.authority
            .map(|a| KeyValue::new(label::TARGET, a.to_owned()))
    }

    fn status_of(status: &Status) -> KeyValue {
        KeyValue::new(label::STATUS, status.code().name())
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl LifecycleObserver for Metrics {
    fn on_attempt_start(&self, attempt: &AttemptLabels<'_>) {
        let mut attrs = Vec::with_capacity(3 + self.inner.custom.len());
        attrs.push(Self::method_of(&attempt.call));
        attrs.extend(Self::target_of(&attempt.call));
        attrs.extend(self.inner.custom.iter().cloned());
        self.inner.instruments.attempt_started.add(1, &attrs);
    }

    fn on_attempt_end(
        &self,
        attempt: &AttemptLabels<'_>,
        status: &Status,
        latency: std::time::Duration,
    ) {
        let mut attrs = Vec::with_capacity(4 + self.inner.custom.len());
        attrs.push(Self::method_of(&attempt.call));
        attrs.extend(Self::target_of(&attempt.call));
        attrs.push(Self::status_of(status));
        attrs.extend(self.inner.custom.iter().cloned());
        self.inner
            .instruments
            .attempt_duration
            .record(latency.as_secs_f64(), &attrs);
    }

    fn on_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: std::time::Duration) {
        let mut attrs = Vec::with_capacity(4 + self.inner.custom.len());
        attrs.push(Self::method_of(call));
        attrs.extend(Self::target_of(call));
        attrs.push(Self::status_of(status));
        attrs.extend(self.inner.custom.iter().cloned());
        self.inner
            .instruments
            .call_duration
            .record(latency.as_secs_f64(), &attrs);
    }

    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        let mut attrs = Vec::with_capacity(2 + self.inner.custom.len());
        attrs.push(Self::method_of(call));
        attrs.extend(self.inner.custom.iter().cloned());
        self.inner.instruments.server_call_started.add(1, &attrs);
    }

    fn on_server_call_end(
        &self,
        call: &CallLabels<'_>,
        status: &Status,
        latency: std::time::Duration,
    ) {
        let mut attrs = Vec::with_capacity(3 + self.inner.custom.len());
        attrs.push(Self::method_of(call));
        attrs.push(Self::status_of(status));
        attrs.extend(self.inner.custom.iter().cloned());
        self.inner
            .instruments
            .server_call_duration
            .record(latency.as_secs_f64(), &attrs);
    }

    fn on_reconnect(&self, event: &ReconnectEvent<'_>) {
        // A94 connection counters. Custom attributes stay off: A108
        // scopes custom labels to per-call instruments.
        let attrs = [KeyValue::new(label::TARGET, event.target.to_owned())];
        match event.status {
            None => self.inner.instruments.conn_succeeded.add(1, &attrs),
            Some(_) => self.inner.instruments.conn_failed.add(1, &attrs),
        }
    }
}
