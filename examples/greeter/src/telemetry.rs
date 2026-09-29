//! Optional telemetry wiring for the greeter example (OB-02).
//!
//! This module shows the OB-02 integration shape without linking an exporter:
//!
//! - a reviewed [`MetricLabelPolicy`] allowlist over the four greeter RPCs,
//! - one shared [`InMemoryMetricSink`] behind client and server
//!   [`BoundedMetricObserver`]s, folded by [`MetricDiagnosis`] into timeout,
//!   retry, and saturation counts,
//! - explicit W3C `traceparent` propagation through a client/server
//!   interceptor pair, validated by [`propagation::valid_traceparent`],
//! - redaction of sensitive metadata through [`TelemetryContext`] debug
//!   formatting (keys visible, values and status text redacted by default).
//!
//! Wiring note: OB-02's write scope excludes `lib.rs`, so this module is not
//!! yet declared there. The coordinator wires it with `pub mod telemetry;` in
//! `lib.rs`; until then this file is verified by temporary wiring (see the
//!! OB-02 report) and compiles as ordinary example code.
//!
//! [`BoundedMetricObserver`]: pbrs_grpc::BoundedMetricObserver
//! [`InMemoryMetricSink`]: pbrs_grpc::telemetry::InMemoryMetricSink
//! [`MetricDiagnosis`]: pbrs_grpc::telemetry::MetricDiagnosis
//! [`MetricLabelPolicy`]: pbrs_grpc::MetricLabelPolicy
//! [`TelemetryContext`]: pbrs_grpc::telemetry::TelemetryContext

use super::{GreeterClient, GreeterServer, HelloRequest, MyGreeter, Request, Router, Status};
use pbrs_grpc::telemetry::{InMemoryMetricSink, MetricDiagnosis, propagation};
use pbrs_grpc::{
    BoundedMetricObserver, Channel, ClientInterceptor, Interceptor, MetricLabelPolicy, Outgoing,
    Rpc,
};
use std::sync::Arc;
use tokio::net::TcpListener;

/// Reviewed greeter RPC paths; the only `grpc.method` values this example
/// ever exports. Anything else collapses to `_other`.
pub static GREETER_RPCS: &[&str] = &[
    "/helloworld.Greeter/SayHello",
    "/helloworld.Greeter/ClientHello",
    "/helloworld.Greeter/ServerHello",
    "/helloworld.Greeter/StreamHello",
];

/// Reviewed reconnect targets. The loopback example dials ephemeral ports, so
/// there is no static target to allowlist and every reconnect counts as
/// `_other`.
pub static GREETER_RECONNECT_TARGETS: &[&str] = &[];

/// Build the reviewed label policy for the greeter RPCs.
pub fn metric_policy() -> Result<MetricLabelPolicy, Status> {
    MetricLabelPolicy::new(GREETER_RPCS, GREETER_RECONNECT_TARGETS)
}

/// One shared in-memory sink plus the policy it is bounded by.
///
/// Hand out a fresh [`BoundedMetricObserver`] per endpoint
/// ([`Telemetry::client_observer`], [`Telemetry::server_observer`]); every
/// observer records into the same sink, so [`Telemetry::diagnose`] sees both
/// sides of each RPC.
#[derive(Clone, Debug)]
pub struct Telemetry {
    policy: MetricLabelPolicy,
    sink: InMemoryMetricSink,
}

impl Telemetry {
    /// Bind a label policy to a fresh shared sink.
    #[must_use]
    pub fn new(policy: MetricLabelPolicy) -> Self {
        Self {
            policy,
            sink: InMemoryMetricSink::new(),
        }
    }

    /// Bind the reviewed [`metric_policy`] to a fresh shared sink.
    pub fn with_greeter_policy() -> Result<Self, Status> {
        Ok(Self::new(metric_policy()?))
    }

    /// Observer for the client [`Channel`].
    #[must_use]
    pub fn client_observer(&self) -> BoundedMetricObserver<InMemoryMetricSink> {
        BoundedMetricObserver::new(self.policy, self.sink.clone())
    }

    /// Observer for the server [`Router`].
    #[must_use]
    pub fn server_observer(&self) -> BoundedMetricObserver<InMemoryMetricSink> {
        BoundedMetricObserver::new(self.policy, self.sink.clone())
    }

    /// The shared recording sink.
    #[must_use]
    pub fn sink(&self) -> &InMemoryMetricSink {
        &self.sink
    }

    /// Fold the recording into timeout, retry, and saturation counts.
    #[must_use]
    pub fn diagnose(&self) -> MetricDiagnosis {
        self.sink.diagnose()
    }
}

/// Client interceptor injecting one explicit `traceparent` value.
///
/// Unlike the recording `otel::trace` propagator (which never fails the RPC),
/// this example surfaces misconfiguration: metadata that rejects the value
/// fails the call, so a bad fixture cannot silently run unpropagated.
#[derive(Clone, Debug)]
pub struct TraceparentInjector {
    value: String,
}

impl TraceparentInjector {
    /// Inject `value` as `traceparent` on every outbound call.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// The injected header value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl ClientInterceptor for TraceparentInjector {
    fn intercept(&self, call: &mut Outgoing<'_>) -> Result<(), Status> {
        call.metadata_mut()
            .set(propagation::TRACEPARENT, self.value.as_str())
    }
}

/// Server interceptor capturing inbound `traceparent` values.
///
/// Install on the [`Router`] opposite a [`TraceparentInjector`] to prove the
/// propagation bytes survived the hop. Clone shares the capture.
#[allow(
    clippy::disallowed_types,
    reason = "sync Interceptor::intercept never awaits; the critical section pushes one header value"
)]
#[derive(Clone, Debug, Default)]
pub struct TraceparentCapture {
    seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl TraceparentCapture {
    /// Capture into a fresh shared slot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `traceparent` value observed so far, in arrival order.
    #[must_use]
    pub fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Interceptor for TraceparentCapture {
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status> {
        if let Some(value) = rpc.metadata().get(propagation::TRACEPARENT) {
            self.seen
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(value.to_owned());
        }
        Ok(())
    }
}

/// Outcome of one instrumented loopback RPC.
#[derive(Clone, Debug)]
pub struct InstrumentedLoopback {
    /// Timeout, retry, and saturation counts from both endpoints.
    pub diagnosis: MetricDiagnosis,
    /// `traceparent` values the server interceptor observed.
    pub propagated: Vec<String>,
}

/// Serve the greeter on loopback with telemetry installed, run one unary
/// `SayHello`, and return the diagnosis plus the propagated traceparent.
///
/// The client channel carries a [`TraceparentInjector`] and the client
/// observer; the router carries a [`TraceparentCapture`] and the server
/// observer. `traceparent` must already satisfy
/// [`propagation::valid_traceparent`]; anything else is rejected before any
/// socket opens.
pub async fn run_instrumented(traceparent: &str) -> Result<InstrumentedLoopback, Status> {
    if !propagation::valid_traceparent(traceparent) {
        return Err(Status::invalid_argument(
            "traceparent must be a strict W3C version-00 header",
        ));
    }
    let telemetry = Telemetry::with_greeter_policy()?;
    let capture = TraceparentCapture::new();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn({
        let telemetry = telemetry.clone();
        let capture = capture.clone();
        async move {
            Router::new()
                .add_service(GreeterServer::new(MyGreeter))
                .intercept(capture)
                .observer(telemetry.server_observer())
                .serve_listener(listener)
                .await
        }
    });

    let channel = Channel::connect(addr)
        .await?
        .intercept(TraceparentInjector::new(traceparent))
        .observer(telemetry.client_observer());
    let client = GreeterClient::new(channel);
    let mut request = HelloRequest::new();
    request.set_name("ada");
    let reply = client.say_hello(Request::new(request)).await?;
    let text = reply
        .get_ref()
        .message()
        .to_str()
        .map_err(|_| Status::internal("reply was not valid UTF-8"))?;
    if text != "hello ada" {
        server.abort();
        return Err(Status::internal(format!("unexpected reply: {text}")));
    }
    server.abort();

    Ok(InstrumentedLoopback {
        diagnosis: telemetry.diagnose(),
        propagated: capture.seen(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pbrs_grpc::telemetry::TelemetryContext;
    use pbrs_grpc::{CallLabels, CallRole, LifecycleObserver, Metadata};

    const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn greeter_policy_collapses_unregistered_paths() {
        let telemetry = Telemetry::with_greeter_policy().expect("static allowlist");
        let observer = telemetry.server_observer();
        for index in 0..1024u32 {
            let path = format!("/evil.Service/Method{index}");
            let evil = CallLabels::new(path.as_str(), Some("tenant-secret"), CallRole::Server);
            observer.on_call_start(&evil);
        }
        let diagnosis = telemetry.diagnose();
        assert_eq!(diagnosis.by_rpc.len(), 1);
        assert_eq!(
            diagnosis.by_rpc.get(pbrs_grpc::OTHER_METRIC_LABEL),
            Some(&1024)
        );
        assert!(propagation::valid_traceparent(TRACEPARENT));
    }

    #[test]
    fn redaction_keeps_traceparent_key_but_hides_values() {
        let mut metadata = Metadata::new();
        metadata
            .set("authorization", "Bearer test-token-abc")
            .expect("ascii value");
        metadata
            .set(propagation::TRACEPARENT, TRACEPARENT)
            .expect("ascii value");
        let status = Status::unavailable("token=test-token-abc");
        let call = CallLabels::new(
            "/helloworld.Greeter/SayHello",
            Some("tenant-a.internal"),
            CallRole::Client,
        );
        let dumped = format!(
            "{:?}",
            TelemetryContext::new(call)
                .with_metadata(&metadata)
                .with_status(&status)
        );
        assert!(dumped.contains("authorization"), "key visible: {dumped}");
        assert!(dumped.contains("traceparent"), "key visible: {dumped}");
        for secret in ["test-token-abc", "Bearer", "4bf92f35", "tenant-a.internal"] {
            assert!(!dumped.contains(secret), "leaked {secret}: {dumped}");
        }
    }

    #[tokio::test]
    async fn instrumented_loopback_propagates_and_records() {
        let outcome = run_instrumented(TRACEPARENT)
            .await
            .expect("instrumented loopback");
        assert_eq!(outcome.propagated, vec![TRACEPARENT.to_owned()]);
        assert_eq!(outcome.diagnosis.timeouts, 0);
        assert_eq!(outcome.diagnosis.retries, 0);
        assert_eq!(outcome.diagnosis.saturation_signals(), 0);
        assert!(
            outcome.diagnosis.events >= 4,
            "client + server start/end: {:?}",
            outcome.diagnosis
        );
        assert!(
            outcome
                .diagnosis
                .by_rpc
                .contains_key("/helloworld.Greeter/SayHello"),
            "registered path recorded: {:?}",
            outcome.diagnosis.by_rpc
        );
    }

    #[tokio::test]
    async fn instrumented_loopback_rejects_a_bad_traceparent() {
        let err = run_instrumented("not-a-traceparent")
            .await
            .expect_err("invalid traceparent rejected");
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);
    }
}
