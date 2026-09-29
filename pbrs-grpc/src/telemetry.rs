//! Lifecycle telemetry hooks and explicitly bounded metric labels.
//!
//! Exposes [`LifecycleObserver`] for monitoring gRPC call attempts, calls,
//! status codes, latencies, queue wait times, payload bytes, transport reconnects,
//! rejections, and cancellations. Observer events retain raw RPC identity;
//! register a [`BoundedMetricObserver`] with a [`MetricLabelPolicy`] for metrics.
//!
//! The disabled observer path does not allocate telemetry labels. Metric
//! labels are drawn only from a capped static allowlist or fallback bucket.

use crate::metadata::Metadata;
use crate::status::{Code, Status};
#[cfg(feature = "otel")]
use opentelemetry::metrics::{Counter, Histogram, Meter};
#[cfg(feature = "otel")]
use opentelemetry::{KeyValue, global};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

pub use crate::metadata::{MetadataMap, SafeMetadataDebug, is_sensitive_key};

/// The endpoint perspective of a call (client or server).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallRole {
    /// Outbound client call.
    Client,
    /// Inbound server call.
    Server,
}

/// Split `/service/method` without allocating. Unparseable paths yield empty
/// halves, which route to `UNIMPLEMENTED`.
#[must_use]
pub fn split_path(path: &str) -> (&str, &str) {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match rest.rsplit_once('/') {
        Some((service, method)) => (service, method),
        None => ("", ""),
    }
}

/// Raw identity of an RPC call passed to lifecycle observers.
///
/// A peer can supply an arbitrary path or authority, so these fields must not
/// be used directly as metric labels. Use [`MetricLabelPolicy::call`] to bound
/// cardinality and omit untrusted authority values from metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallLabels<'a> {
    /// Unverified service half of the path (e.g. `helloworld.Greeter`).
    pub service: &'a str,
    /// Unverified method half of the path (e.g. `SayHello`).
    pub method: &'a str,
    /// Unverified full gRPC path (e.g. `/helloworld.Greeter/SayHello`).
    pub path: &'a str,
    /// Unverified authority or host:port destination, if known.
    pub authority: Option<&'a str>,
    /// Whether this is a client or server call.
    pub role: CallRole,
}

impl<'a> CallLabels<'a> {
    /// Construct call labels from a gRPC path, optional authority, and call role.
    #[must_use]
    pub fn new(path: &'a str, authority: Option<&'a str>, role: CallRole) -> Self {
        let (service, method) = split_path(path);
        Self {
            service,
            method,
            path,
            authority,
            role,
        }
    }

    /// The service portion of the path (e.g. `helloworld.Greeter`).
    #[must_use]
    pub fn service(&self) -> &'a str {
        self.service
    }

    /// The method portion of the path (e.g. `SayHello`).
    #[must_use]
    pub fn method(&self) -> &'a str {
        self.method
    }

    /// The full gRPC path (e.g. `/helloworld.Greeter/SayHello`).
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The authority or host:port destination, if known.
    #[must_use]
    pub fn authority(&self) -> Option<&'a str> {
        self.authority
    }

    /// Whether this is a client or server call.
    #[must_use]
    pub fn role(&self) -> CallRole {
        self.role
    }

    /// Convert into owned strings when labels must be stored across async boundaries.
    #[must_use]
    pub fn to_owned(&self) -> OwnedCallLabels {
        OwnedCallLabels {
            service: self.service.to_owned(),
            method: self.method.to_owned(),
            path: self.path.to_owned(),
            authority: self.authority.map(str::to_owned),
            role: self.role,
        }
    }
}

/// Owned counterpart of [`CallLabels`] when labels must be stored across async tasks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OwnedCallLabels {
    /// Service half of the path.
    pub service: String,
    /// Method half of the path.
    pub method: String,
    /// Full gRPC path.
    pub path: String,
    /// Authority or host:port destination.
    pub authority: Option<String>,
    /// Whether this is a client or server call.
    pub role: CallRole,
}

impl OwnedCallLabels {
    /// Borrow as [`CallLabels`].
    #[must_use]
    pub fn as_borrowed(&self) -> CallLabels<'_> {
        CallLabels {
            service: &self.service,
            method: &self.method,
            path: &self.path,
            authority: self.authority.as_deref(),
            role: self.role,
        }
    }
}

/// Maximum number of explicit RPC paths accepted by a metric policy.
pub const MAX_METRIC_RPCS: usize = 256;
/// Maximum number of explicit reconnect targets accepted by a metric policy.
pub const MAX_METRIC_TARGETS: usize = 16;
/// Maximum byte length of one explicitly configured metric label.
pub const MAX_METRIC_LABEL_BYTES: usize = 256;
/// Fallback label shared by all unregistered RPC paths and reconnect targets.
pub const OTHER_METRIC_LABEL: &str = "_other";

/// Bounded RPC labels safe to use as metric dimensions.
///
/// The RPC name comes only from an explicitly configured static allowlist.
/// Peer-supplied authority, metadata, and payloads are never included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MetricCallLabels {
    rpc: &'static str,
    role: CallRole,
}

impl MetricCallLabels {
    /// Canonical registered RPC path or [`OTHER_METRIC_LABEL`].
    #[must_use]
    pub fn rpc(&self) -> &'static str {
        self.rpc
    }

    /// Client or server perspective.
    #[must_use]
    pub fn role(&self) -> CallRole {
        self.role
    }
}

/// Explicit, allocation-free classifier for bounded lifecycle metric labels.
///
/// Register only reviewed static RPC paths and reconnect targets. Raw
/// [`CallLabels`] and [`ReconnectEvent`] remain available for controlled
/// diagnostics, but cannot create new metric label values through this policy.
#[derive(Clone, Copy, Debug)]
pub struct MetricLabelPolicy {
    rpc_paths: &'static [&'static str],
    reconnect_targets: &'static [&'static str],
}

impl MetricLabelPolicy {
    /// Validate a static allowlist before an observer uses it for metrics.
    ///
    /// At most [`MAX_METRIC_RPCS`] RPC values plus one fallback and
    /// [`MAX_METRIC_TARGETS`] reconnect targets plus one fallback are emitted.
    ///
    /// ```
    /// use pbrs_grpc::{CallLabels, CallRole, MetricLabelPolicy, OTHER_METRIC_LABEL};
    ///
    /// let policy = MetricLabelPolicy::new(&["/helloworld.Greeter/SayHello"], &[])
    ///     .expect("valid static RPC path");
    /// let unregistered = CallLabels::new(
    ///     "/unknown.Service/Method123",
    ///     Some("tenant-secret.example"),
    ///     CallRole::Server,
    /// );
    /// assert_eq!(policy.call(&unregistered).rpc(), OTHER_METRIC_LABEL);
    /// ```
    pub fn new(
        rpc_paths: &'static [&'static str],
        reconnect_targets: &'static [&'static str],
    ) -> Result<Self, Status> {
        if rpc_paths.len() > MAX_METRIC_RPCS {
            return Err(Status::invalid_argument(format!(
                "metric RPC paths exceed the cap of {MAX_METRIC_RPCS}"
            )));
        }
        if reconnect_targets.len() > MAX_METRIC_TARGETS {
            return Err(Status::invalid_argument(format!(
                "metric reconnect targets exceed the cap of {MAX_METRIC_TARGETS}"
            )));
        }
        for (index, path) in rpc_paths.iter().enumerate() {
            let (service, method) = split_path(path);
            if path.len() > MAX_METRIC_LABEL_BYTES
                || !path.starts_with('/')
                || service.is_empty()
                || method.is_empty()
                || !service
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.')
                || !method
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(Status::invalid_argument("invalid metric RPC path"));
            }
            if rpc_paths.iter().take(index).any(|prior| prior == path) {
                return Err(Status::invalid_argument("duplicate metric RPC path"));
            }
        }
        for (index, target) in reconnect_targets.iter().enumerate() {
            if target.is_empty()
                || target.len() > MAX_METRIC_LABEL_BYTES
                || !target.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(Status::invalid_argument("invalid metric reconnect target"));
            }
            if reconnect_targets
                .iter()
                .take(index)
                .any(|prior| prior == target)
            {
                return Err(Status::invalid_argument(
                    "duplicate metric reconnect target",
                ));
            }
        }
        Ok(Self {
            rpc_paths,
            reconnect_targets,
        })
    }

    /// Classify an RPC without reflecting raw peer-supplied path or authority.
    #[must_use]
    pub fn call(&self, call: &CallLabels<'_>) -> MetricCallLabels {
        let rpc = self
            .rpc_paths
            .iter()
            .copied()
            .find(|path| *path == call.path)
            .unwrap_or(OTHER_METRIC_LABEL);
        MetricCallLabels {
            rpc,
            role: call.role,
        }
    }

    /// Classify a reconnect using only a configured static target name.
    #[must_use]
    pub fn reconnect_target(&self, event: &ReconnectEvent<'_>) -> &'static str {
        self.reconnect_targets
            .iter()
            .copied()
            .find(|target| *target == event.target)
            .unwrap_or(OTHER_METRIC_LABEL)
    }
}

/// Raw identity of an RPC attempt within a call.
///
/// An RPC call may have one or more attempts (initial attempt + transparent retries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AttemptLabels<'a> {
    /// Underlying call labels.
    pub call: CallLabels<'a>,
    /// 1-based attempt counter (1 for initial attempt, 2 for first transparent retry, etc.).
    pub attempt: u32,
}

impl<'a> AttemptLabels<'a> {
    /// Create new attempt labels for `call` at `attempt`.
    #[must_use]
    pub fn new(call: CallLabels<'a>, attempt: u32) -> Self {
        Self { call, attempt }
    }

    /// Attempt index (1-based).
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Underlying call labels.
    #[must_use]
    pub fn call(&self) -> &CallLabels<'a> {
        &self.call
    }
}

impl<'a> Deref for AttemptLabels<'a> {
    type Target = CallLabels<'a>;

    fn deref(&self) -> &Self::Target {
        &self.call
    }
}

/// Reason why an RPC was rejected before normal handler execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RejectionReason {
    /// Rejected by a client-side interceptor before opening the stream.
    ClientInterceptor,
    /// Rejected by a server-side interceptor before invoking the handler.
    ServerInterceptor,
    /// Concurrency limit reached (e.g. `max_concurrent_rpcs`).
    ConcurrencyLimit,
    /// Connection limit reached (e.g. `max_concurrent_connections`).
    ConnectionLimit,
    /// Invalid request headers, method, or framing (e.g. bad content-type or unsupported encoding).
    InvalidRequest,
    /// Service or method not registered on the server.
    Unimplemented,
    /// Transport byte budget exceeded.
    ByteBudgetExceeded,
    /// Request message encoding/serialization failure.
    MessageEncode,
    /// Dial or connection setup failure.
    SetupFailed,
    /// Other pre-handler rejection.
    Other,
}

/// Reason why an in-flight RPC was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CancellationReason {
    /// Request deadline / timeout expired.
    DeadlineExceeded,
    /// Caller cancelled explicitly (e.g. dropped future or triggered `CallHandle::cancel`).
    CallerCancelled,
    /// Peer reset the stream (e.g. HTTP/2 `RST_STREAM` CANCEL).
    PeerReset,
    /// Transport failure or broken connection during call.
    TransportReset,
    /// Other cancellation.
    Other,
}

/// Event emitted when an RPC is rejected before handler execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectionEvent<'a> {
    /// Labels of the rejected call.
    pub call: CallLabels<'a>,
    /// High-level rejection reason.
    pub reason: RejectionReason,
    /// Associated gRPC status code.
    pub code: Code,
}

/// Event emitted when an RPC is cancelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancellationEvent<'a> {
    /// Labels of the cancelled call.
    pub call: CallLabels<'a>,
    /// High-level cancellation reason.
    pub reason: CancellationReason,
}

/// Event emitted when a client transport reconnect/redial occurs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconnectEvent<'a> {
    /// Target endpoint or authority being reconnected.
    pub target: &'a str,
    /// Reconnect attempt index (1-based).
    pub attempt: u32,
    /// Duration of the reconnect / dial handshake.
    pub duration: Duration,
    /// Resulting status code (`None` on successful reconnect, `Some(code)` on failure).
    pub status: Option<Code>,
}

/// Optional observer for gRPC call lifecycles.
///
/// Implement this trait to receive structured notifications for:
/// - Client call start and completion
/// - Client attempt start and completion (including transparent retries)
/// - Server call start and completion
/// - Client and server local queue wait times
/// - Bytes sent and received
/// - Client transport reconnects
/// - Pre-handler rejections
/// - Cancellations (caller drop, deadline, peer reset)
///
/// All methods have default no-op implementations, so listeners only implement
/// the events they require. Methods accept borrowed labels to ensure zero heap
/// allocations during telemetry dispatch. These callbacks may include raw
/// peer-controlled identity or status text; use [`BoundedMetricObserver`]
/// instead when exporting metrics.
pub trait LifecycleObserver: Send + Sync + 'static {
    /// Invoked when a client-side RPC call is initiated.
    fn on_call_start(&self, _call: &CallLabels<'_>) {}

    /// Invoked when a client-side RPC call finishes.
    fn on_call_end(&self, _call: &CallLabels<'_>, _status: &Status, _latency: Duration) {}

    /// Invoked when a client-side attempt is started.
    fn on_attempt_start(&self, _attempt: &AttemptLabels<'_>) {}

    /// Invoked when a client-side attempt finishes.
    fn on_attempt_end(&self, _attempt: &AttemptLabels<'_>, _status: &Status, _latency: Duration) {}

    /// Invoked when a server-side RPC is received.
    fn on_server_call_start(&self, _call: &CallLabels<'_>) {}

    /// Invoked when a server-side RPC finishes.
    fn on_server_call_end(&self, _call: &CallLabels<'_>, _status: &Status, _latency: Duration) {}

    /// Invoked when a client-side call experiences queue wait (e.g. pool acquisition or wait-for-ready).
    fn on_queue_wait(&self, _call: &CallLabels<'_>, _wait: Duration) {}

    /// Invoked after server validation/admission for delay before the dispatch task starts.
    ///
    /// This is scheduling delay, not the complete listener/transport queue time.
    /// Calls rejected before admission do not emit this event.
    fn on_server_queue_wait(&self, _call: &CallLabels<'_>, _wait: Duration) {}

    /// Invoked when payload/frame bytes are sent on an RPC.
    fn on_bytes_sent(&self, _call: &CallLabels<'_>, _bytes: usize) {}

    /// Invoked when payload/frame bytes are received on an RPC.
    fn on_bytes_received(&self, _call: &CallLabels<'_>, _bytes: usize) {}

    /// Invoked when a transport reconnect/redial finishes (success or failure).
    fn on_reconnect(&self, _event: &ReconnectEvent<'_>) {}

    /// Invoked when an RPC is rejected before handler execution.
    fn on_rejection(&self, _event: &RejectionEvent<'_>) {}

    /// Invoked when an RPC is cancelled (by deadline, caller cancel, or peer reset).
    fn on_cancellation(&self, _event: &CancellationEvent<'_>) {}

    /// Invoked when diagnostic telemetry context is captured for an RPC.
    fn on_telemetry_context(&self, _ctx: &TelemetryContext<'_>) {}
}

/// Bounded classification of a raw attempt index for metric dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MetricAttemptClass {
    /// First attempt of a call.
    Initial,
    /// Transparent retry after the first attempt.
    Retry,
    /// Invalid zero attempt index supplied by a caller.
    Invalid,
}

impl MetricAttemptClass {
    fn from_index(index: u32) -> Self {
        match index {
            0 => Self::Invalid,
            1 => Self::Initial,
            _ => Self::Retry,
        }
    }
}

/// Lifecycle event whose identity fields cannot contain peer-controlled strings.
///
/// Durations, byte counts, status codes, and rejection/cancellation reasons are
/// observations, not new metric label values. The raw status message, metadata,
/// payload, authority, and numeric attempt index are never forwarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetricEvent {
    /// A client call started.
    CallStart {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
    },
    /// A client call finished.
    CallEnd {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Bounded status code.
        code: Code,
        /// Total call duration.
        latency: Duration,
    },
    /// A client attempt started.
    AttemptStart {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// First, retry, or invalid attempt class.
        class: MetricAttemptClass,
    },
    /// A client attempt finished.
    AttemptEnd {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// First, retry, or invalid attempt class.
        class: MetricAttemptClass,
        /// Bounded status code.
        code: Code,
        /// Attempt duration.
        latency: Duration,
    },
    /// A server call started.
    ServerCallStart {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
    },
    /// A server call finished.
    ServerCallEnd {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Bounded status code.
        code: Code,
        /// Total call duration.
        latency: Duration,
    },
    /// A client waited to acquire a connection or slot.
    ClientQueueWait {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Observed wait duration.
        wait: Duration,
    },
    /// A server waited between admission and dispatch.
    ServerQueueWait {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Post-admission scheduling delay, not full transport queue time.
        wait: Duration,
    },
    /// Bytes sent for one call.
    BytesSent {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Byte count, used as a value rather than a label.
        bytes: usize,
    },
    /// Bytes received for one call.
    BytesReceived {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Byte count, used as a value rather than a label.
        bytes: usize,
    },
    /// A reconnect/redial finished.
    Reconnect {
        /// Static allowlisted or fallback target.
        target: &'static str,
        /// First, retry, or invalid attempt class.
        class: MetricAttemptClass,
        /// Resulting bounded status code, or None on success.
        status: Option<Code>,
        /// Reconnect duration.
        duration: Duration,
    },
    /// A call was rejected before handler execution.
    Rejection {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Bounded rejection reason.
        reason: RejectionReason,
        /// Bounded status code.
        code: Code,
    },
    /// A call was cancelled.
    Cancellation {
        /// Static or fallback RPC identity.
        call: MetricCallLabels,
        /// Bounded cancellation reason.
        reason: CancellationReason,
    },
}

/// Receiver for lifecycle events with bounded identity and no raw diagnostics.
pub trait MetricSink: Send + Sync + 'static {
    /// Record one event without receiving raw peer data or credential-bearing status messages.
    fn record(&self, event: MetricEvent);
}

impl<S: MetricSink + ?Sized> MetricSink for Arc<S> {
    fn record(&self, event: MetricEvent) {
        (**self).record(event);
    }
}

/// Adapts raw lifecycle hooks into events safe for bounded metric dimensions.
///
/// Unlike a directly registered [`LifecycleObserver`], this adapter cannot
/// forward raw paths, authorities, payloads, metadata, or status messages
/// to the sink. Raw diagnostic telemetry contexts are intentionally ignored.
///
/// ```
/// use pbrs_grpc::{
///     BoundedMetricObserver, CallLabels, CallRole, LifecycleObserver, MetricEvent,
///     MetricLabelPolicy, MetricSink, OTHER_METRIC_LABEL,
/// };
///
/// struct Check;
/// impl MetricSink for Check {
///     fn record(&self, event: MetricEvent) {
///         if let MetricEvent::ServerCallStart { call } = event {
///             assert_eq!(call.rpc(), OTHER_METRIC_LABEL);
///         }
///     }
/// }
/// let policy = MetricLabelPolicy::new(&["/helloworld.Greeter/SayHello"], &[])
///     .expect("valid allowlist");
/// let observer = BoundedMetricObserver::new(policy, Check);
/// let raw = CallLabels::new("/unregistered.Service/Call", Some("peer-secret"), CallRole::Server);
/// observer.on_server_call_start(&raw);
/// ```
pub struct BoundedMetricObserver<S: MetricSink> {
    policy: MetricLabelPolicy,
    sink: S,
}

impl<S: MetricSink> BoundedMetricObserver<S> {
    /// Bind a validated static label policy to a metric event receiver.
    #[must_use]
    pub fn new(policy: MetricLabelPolicy, sink: S) -> Self {
        Self { policy, sink }
    }
}

impl<S: MetricSink> LifecycleObserver for BoundedMetricObserver<S> {
    fn on_call_start(&self, call: &CallLabels<'_>) {
        self.sink.record(MetricEvent::CallStart {
            call: self.policy.call(call),
        });
    }

    fn on_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        self.sink.record(MetricEvent::CallEnd {
            call: self.policy.call(call),
            code: status.code(),
            latency,
        });
    }

    fn on_attempt_start(&self, attempt: &AttemptLabels<'_>) {
        self.sink.record(MetricEvent::AttemptStart {
            call: self.policy.call(attempt.call()),
            class: MetricAttemptClass::from_index(attempt.attempt()),
        });
    }

    fn on_attempt_end(&self, attempt: &AttemptLabels<'_>, status: &Status, latency: Duration) {
        self.sink.record(MetricEvent::AttemptEnd {
            call: self.policy.call(attempt.call()),
            class: MetricAttemptClass::from_index(attempt.attempt()),
            code: status.code(),
            latency,
        });
    }

    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        self.sink.record(MetricEvent::ServerCallStart {
            call: self.policy.call(call),
        });
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        self.sink.record(MetricEvent::ServerCallEnd {
            call: self.policy.call(call),
            code: status.code(),
            latency,
        });
    }

    fn on_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        self.sink.record(MetricEvent::ClientQueueWait {
            call: self.policy.call(call),
            wait,
        });
    }

    fn on_server_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        self.sink.record(MetricEvent::ServerQueueWait {
            call: self.policy.call(call),
            wait,
        });
    }

    fn on_bytes_sent(&self, call: &CallLabels<'_>, bytes: usize) {
        self.sink.record(MetricEvent::BytesSent {
            call: self.policy.call(call),
            bytes,
        });
    }

    fn on_bytes_received(&self, call: &CallLabels<'_>, bytes: usize) {
        self.sink.record(MetricEvent::BytesReceived {
            call: self.policy.call(call),
            bytes,
        });
    }

    fn on_reconnect(&self, event: &ReconnectEvent<'_>) {
        self.sink.record(MetricEvent::Reconnect {
            target: self.policy.reconnect_target(event),
            class: MetricAttemptClass::from_index(event.attempt),
            status: event.status,
            duration: event.duration,
        });
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        self.sink.record(MetricEvent::Rejection {
            call: self.policy.call(&event.call),
            reason: event.reason,
            code: event.code,
        });
    }

    fn on_cancellation(&self, event: &CancellationEvent<'_>) {
        self.sink.record(MetricEvent::Cancellation {
            call: self.policy.call(&event.call),
            reason: event.reason,
        });
    }
}

/// Chained observer running `first` then `second`.
pub struct ObserverChain {
    first: Arc<dyn LifecycleObserver>,
    second: Arc<dyn LifecycleObserver>,
}

impl ObserverChain {
    /// Create a new chain of two observers.
    #[must_use]
    pub fn new(first: Arc<dyn LifecycleObserver>, second: Arc<dyn LifecycleObserver>) -> Self {
        Self { first, second }
    }
}

impl fmt::Debug for ObserverChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverChain").finish_non_exhaustive()
    }
}

impl LifecycleObserver for ObserverChain {
    fn on_call_start(&self, call: &CallLabels<'_>) {
        self.first.on_call_start(call);
        self.second.on_call_start(call);
    }

    fn on_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        self.first.on_call_end(call, status, latency);
        self.second.on_call_end(call, status, latency);
    }

    fn on_attempt_start(&self, attempt: &AttemptLabels<'_>) {
        self.first.on_attempt_start(attempt);
        self.second.on_attempt_start(attempt);
    }

    fn on_attempt_end(&self, attempt: &AttemptLabels<'_>, status: &Status, latency: Duration) {
        self.first.on_attempt_end(attempt, status, latency);
        self.second.on_attempt_end(attempt, status, latency);
    }

    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        self.first.on_server_call_start(call);
        self.second.on_server_call_start(call);
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        self.first.on_server_call_end(call, status, latency);
        self.second.on_server_call_end(call, status, latency);
    }

    fn on_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        self.first.on_queue_wait(call, wait);
        self.second.on_queue_wait(call, wait);
    }

    fn on_server_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        self.first.on_server_queue_wait(call, wait);
        self.second.on_server_queue_wait(call, wait);
    }

    fn on_bytes_sent(&self, call: &CallLabels<'_>, bytes: usize) {
        self.first.on_bytes_sent(call, bytes);
        self.second.on_bytes_sent(call, bytes);
    }

    fn on_bytes_received(&self, call: &CallLabels<'_>, bytes: usize) {
        self.first.on_bytes_received(call, bytes);
        self.second.on_bytes_received(call, bytes);
    }

    fn on_reconnect(&self, event: &ReconnectEvent<'_>) {
        self.first.on_reconnect(event);
        self.second.on_reconnect(event);
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        self.first.on_rejection(event);
        self.second.on_rejection(event);
    }

    fn on_cancellation(&self, event: &CancellationEvent<'_>) {
        self.first.on_cancellation(event);
        self.second.on_cancellation(event);
    }

    fn on_telemetry_context(&self, ctx: &TelemetryContext<'_>) {
        self.first.on_telemetry_context(ctx);
        self.second.on_telemetry_context(ctx);
    }
}

impl<O: LifecycleObserver + ?Sized> LifecycleObserver for Arc<O> {
    fn on_call_start(&self, call: &CallLabels<'_>) {
        (**self).on_call_start(call);
    }

    fn on_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        (**self).on_call_end(call, status, latency);
    }

    fn on_attempt_start(&self, attempt: &AttemptLabels<'_>) {
        (**self).on_attempt_start(attempt);
    }

    fn on_attempt_end(&self, attempt: &AttemptLabels<'_>, status: &Status, latency: Duration) {
        (**self).on_attempt_end(attempt, status, latency);
    }

    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        (**self).on_server_call_start(call);
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, latency: Duration) {
        (**self).on_server_call_end(call, status, latency);
    }

    fn on_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        (**self).on_queue_wait(call, wait);
    }

    fn on_server_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        (**self).on_server_queue_wait(call, wait);
    }

    fn on_bytes_sent(&self, call: &CallLabels<'_>, bytes: usize) {
        (**self).on_bytes_sent(call, bytes);
    }

    fn on_bytes_received(&self, call: &CallLabels<'_>, bytes: usize) {
        (**self).on_bytes_received(call, bytes);
    }

    fn on_reconnect(&self, event: &ReconnectEvent<'_>) {
        (**self).on_reconnect(event);
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        (**self).on_rejection(event);
    }

    fn on_cancellation(&self, event: &CancellationEvent<'_>) {
        (**self).on_cancellation(event);
    }

    fn on_telemetry_context(&self, ctx: &TelemetryContext<'_>) {
        (**self).on_telemetry_context(ctx);
    }
}

pub(crate) struct CallGuard {
    observer: Option<Arc<dyn LifecycleObserver>>,
    labels: Option<OwnedCallLabels>,
    start: std::time::Instant,
    completed: bool,
    cancelled: bool,
    channelz: crate::channelz::ChannelId,
}

impl CallGuard {
    pub(crate) fn new(
        observer: Option<Arc<dyn LifecycleObserver>>,
        labels: Option<OwnedCallLabels>,
        start: std::time::Instant,
        channelz: crate::channelz::ChannelId,
    ) -> Self {
        crate::channelz::Registry::global().note_call_started(channelz);
        Self {
            observer,
            labels,
            start,
            completed: false,
            cancelled: false,
            channelz,
        }
    }

    pub(crate) fn reject(&mut self, reason: RejectionReason, status: &Status) {
        if let (Some(obs), Some(labels)) = (&self.observer, &self.labels) {
            obs.on_rejection(&RejectionEvent {
                call: labels.as_borrowed(),
                reason,
                code: status.code(),
            });
        }
        self.finish(status);
    }

    pub(crate) fn cancel(&mut self, reason: CancellationReason) {
        if !self.cancelled {
            self.cancelled = true;
            if let (Some(obs), Some(labels)) = (&self.observer, &self.labels) {
                obs.on_cancellation(&CancellationEvent {
                    call: labels.as_borrowed(),
                    reason,
                });
            }
        }
    }

    pub(crate) fn finish(&mut self, status: &Status) {
        if !self.completed {
            self.completed = true;
            crate::channelz::Registry::global().note_call_end(self.channelz, status.is_ok());
            if let (Some(obs), Some(labels)) = (&self.observer, &self.labels) {
                obs.on_call_end(&labels.as_borrowed(), status, self.start.elapsed());
            }
        }
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel(CancellationReason::CallerCancelled);
            self.finish(&Status::cancelled());
        }
    }
}

pub(crate) struct AttemptGuard {
    observer: Option<Arc<dyn LifecycleObserver>>,
    call: Option<OwnedCallLabels>,
    attempt: u32,
    start: std::time::Instant,
    completed: bool,
    cancelled: bool,
}

impl AttemptGuard {
    pub(crate) fn new(
        observer: Option<Arc<dyn LifecycleObserver>>,
        call: Option<OwnedCallLabels>,
        attempt: u32,
        start: std::time::Instant,
    ) -> Self {
        Self {
            observer,
            call,
            attempt,
            start,
            completed: false,
            cancelled: false,
        }
    }

    pub(crate) fn reject(&mut self, reason: RejectionReason, status: &Status) {
        if let (Some(obs), Some(call)) = (&self.observer, &self.call) {
            obs.on_rejection(&RejectionEvent {
                call: call.as_borrowed(),
                reason,
                code: status.code(),
            });
        }
        self.finish(status);
    }

    pub(crate) fn cancel(&mut self, reason: CancellationReason) {
        if !self.cancelled {
            self.cancelled = true;
            if let (Some(obs), Some(call)) = (&self.observer, &self.call) {
                obs.on_cancellation(&CancellationEvent {
                    call: call.as_borrowed(),
                    reason,
                });
            }
        }
    }

    pub(crate) fn finish(&mut self, status: &Status) {
        if !self.completed {
            self.completed = true;
            if let (Some(obs), Some(call)) = (&self.observer, &self.call) {
                let attempt_labels = AttemptLabels::new(call.as_borrowed(), self.attempt);
                obs.on_attempt_end(&attempt_labels, status, self.start.elapsed());
            }
        }
    }
}

impl Drop for AttemptGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel(CancellationReason::CallerCancelled);
            self.finish(&Status::cancelled());
        }
    }
}

/// Configuration for diagnostic formatting and telemetry observation.
///
/// By default, all metadata values (including unknown custom headers),
/// raw RPC identity, status messages, and request/response payloads are
/// redacted in diagnostic formatting. Keys and status codes remain visible.
///
/// Diagnostic detail requires explicit consent (`consent: true`). Even when
/// consent is provided, strict cardinality limits apply to prevent log and
/// telemetry pollution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticConfig {
    consent: bool,
    allow_payload: bool,
    allow_metadata_values: bool,
    allow_sensitive_headers: bool,
    allow_binary_metadata: bool,
    allow_raw_identity: bool,
    allow_status_message: bool,
    max_metadata_entries: usize,
    max_value_length: usize,
    custom_sensitive_headers: BTreeSet<String>,
}

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            consent: false,
            allow_payload: false,
            allow_metadata_values: false,
            allow_sensitive_headers: false,
            allow_binary_metadata: false,
            allow_raw_identity: false,
            allow_status_message: false,
            max_metadata_entries: 64,
            max_value_length: 256,
            custom_sensitive_headers: BTreeSet::new(),
        }
    }
}

impl DiagnosticConfig {
    /// Create default safe diagnostic configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Provide explicit consent for diagnostic detail formatting.
    #[must_use]
    pub fn with_consent(mut self, consent: bool) -> Self {
        self.consent = consent;
        self
    }

    /// Permit payload display in diagnostics (requires explicit consent).
    #[must_use]
    pub fn with_payload(mut self, allow: bool) -> Self {
        self.allow_payload = allow;
        self
    }

    /// Permit bounded values for unclassified ASCII metadata (requires consent).
    ///
    /// A peer can put sensitive content under any custom key; use only for
    /// controlled diagnostics. Known sensitive and binary values require
    /// their separate switches even when this is enabled.
    #[must_use]
    pub fn with_metadata_values(mut self, allow: bool) -> Self {
        self.allow_metadata_values = allow;
        self
    }

    /// Permit unredacted sensitive headers (requires explicit consent).
    #[must_use]
    pub fn with_sensitive_headers(mut self, allow: bool) -> Self {
        self.allow_sensitive_headers = allow;
        self
    }

    /// Permit binary metadata display (requires consent and sensitive-header permission).
    #[must_use]
    pub fn with_binary_metadata(mut self, allow: bool) -> Self {
        self.allow_binary_metadata = allow;
        self
    }

    /// Permit bounded path, authority, peer facts, and other untrusted identity
    /// fields in envelope and RPC Debug output (requires explicit consent).
    #[must_use]
    pub fn with_raw_identity(mut self, allow: bool) -> Self {
        self.allow_raw_identity = allow;
        self
    }

    /// Permit raw status message display (requires explicit consent).
    #[must_use]
    pub fn with_status_message(mut self, allow: bool) -> Self {
        self.allow_status_message = allow;
        self
    }

    /// Set a cardinality limit on the number of metadata entries formatted.
    #[must_use]
    pub fn with_max_metadata_entries(mut self, max: usize) -> Self {
        self.max_metadata_entries = max;
        self
    }

    /// Set a byte limit for opted-in identity/status text and metadata value
    /// diagnostics before truncation.
    #[must_use]
    pub fn with_max_value_length(mut self, max: usize) -> Self {
        self.max_value_length = max;
        self
    }

    /// Register a custom header name as sensitive (will be redacted with `[REDACTED]`).
    #[must_use]
    pub fn with_sensitive_header(mut self, header: impl Into<String>) -> Self {
        self.custom_sensitive_headers
            .insert(header.into().to_ascii_lowercase());
        self
    }

    /// Whether explicit consent was granted.
    #[must_use]
    pub fn has_consent(&self) -> bool {
        self.consent
    }

    /// Check whether payload display is permitted (both `allow_payload` AND `consent` must be true).
    #[must_use]
    pub fn is_payload_allowed(&self) -> bool {
        self.consent && self.allow_payload
    }

    /// Whether unclassified metadata values were explicitly permitted.
    #[must_use]
    pub fn are_metadata_values_allowed(&self) -> bool {
        self.consent && self.allow_metadata_values
    }

    /// Check whether sensitive headers can be displayed unredacted (both `allow_sensitive_headers` AND `consent` must be true).
    #[must_use]
    pub fn are_sensitive_headers_allowed(&self) -> bool {
        self.consent && self.allow_sensitive_headers
    }

    /// Check binary permission; formatting also requires sensitive-header permission.
    #[must_use]
    pub fn is_binary_metadata_allowed(&self) -> bool {
        self.consent && self.allow_binary_metadata
    }

    /// Whether raw identity diagnostics were explicitly permitted.
    #[must_use]
    pub fn is_raw_identity_allowed(&self) -> bool {
        self.consent && self.allow_raw_identity
    }

    /// Whether raw status message display was explicitly permitted.
    #[must_use]
    pub fn is_status_message_allowed(&self) -> bool {
        self.consent && self.allow_status_message
    }

    /// Maximum metadata entries allowed by cardinality limits.
    #[must_use]
    pub fn max_metadata_entries(&self) -> usize {
        self.max_metadata_entries
    }

    /// Maximum header value length allowed.
    #[must_use]
    pub fn max_value_length(&self) -> usize {
        self.max_value_length
    }

    /// Returns true if the key is configured as custom sensitive.
    #[must_use]
    pub fn is_custom_sensitive(&self, key: &str) -> bool {
        self.custom_sensitive_headers
            .contains(&key.to_ascii_lowercase())
    }
}

/// Safe diagnostic telemetry context representing an RPC call.
///
/// Designed for structured logging and telemetry formatting without leaking
/// raw paths, authorities, status messages, credentials, binary metadata,
/// or payloads by default. Accessors remain raw for controlled diagnostics.
#[derive(Clone)]
pub struct TelemetryContext<'a> {
    /// Underlying call labels.
    pub call: CallLabels<'a>,
    /// Associated request or response metadata, if known.
    pub metadata: Option<&'a Metadata>,
    /// Resulting gRPC status, if call has completed.
    pub status: Option<&'a Status>,
    /// Diagnostic configuration governing redaction and limits.
    pub config: DiagnosticConfig,
}

impl<'a> TelemetryContext<'a> {
    /// Create new telemetry context for `call`.
    #[must_use]
    pub fn new(call: CallLabels<'a>) -> Self {
        Self {
            call,
            metadata: None,
            status: None,
            config: DiagnosticConfig::default(),
        }
    }

    /// Attach metadata to the telemetry context.
    #[must_use]
    pub fn with_metadata(mut self, metadata: &'a Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Attach resulting status to the telemetry context.
    #[must_use]
    pub fn with_status(mut self, status: &'a Status) -> Self {
        self.status = Some(status);
        self
    }

    /// Attach custom diagnostic configuration.
    #[must_use]
    pub fn with_config(mut self, config: DiagnosticConfig) -> Self {
        self.config = config;
        self
    }

    /// Borrow call labels.
    #[must_use]
    pub fn call(&self) -> &CallLabels<'a> {
        &self.call
    }

    /// Borrow metadata if present.
    #[must_use]
    pub fn metadata(&self) -> Option<&'a Metadata> {
        self.metadata
    }

    /// Borrow status if present.
    #[must_use]
    pub fn status(&self) -> Option<&'a Status> {
        self.status
    }

    /// Borrow diagnostic configuration.
    #[must_use]
    pub fn config(&self) -> &DiagnosticConfig {
        &self.config
    }

    /// Service portion of the call.
    #[must_use]
    pub fn service(&self) -> &'a str {
        self.call.service()
    }

    /// Method portion of the call.
    #[must_use]
    pub fn method(&self) -> &'a str {
        self.call.method()
    }

    /// Full gRPC path.
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.call.path()
    }

    /// Authority or destination host:port.
    #[must_use]
    pub fn authority(&self) -> Option<&'a str> {
        self.call.authority()
    }

    /// Call role (Client or Server).
    #[must_use]
    pub fn role(&self) -> CallRole {
        self.call.role()
    }

    /// Status code if completed.
    #[must_use]
    pub fn status_code(&self) -> Option<Code> {
        self.status.map(Status::code)
    }
}

impl fmt::Debug for TelemetryContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let identity = |value| diagnostic_identity(value, Some(&self.config));
        let mut s = f.debug_struct("TelemetryContext");
        s.field("service", &identity(self.call.service()))
            .field("method", &identity(self.call.method()))
            .field("path", &identity(self.call.path()))
            .field("role", &self.call.role())
            .field("authority", &self.call.authority().map(identity));

        if let Some(status) = self.status {
            s.field("status_code", &status.code());
            let message = if self.config.is_status_message_allowed() {
                diagnostic_value(status.message(), self.config.max_value_length())
            } else {
                Cow::Borrowed("[REDACTED]")
            };
            s.field("status_message", &message);
        }

        if let Some(metadata) = self.metadata {
            s.field("metadata", &metadata.safe_debug(&self.config));
        }

        s.finish_non_exhaustive()
    }
}

pub(crate) fn diagnostic_identity<'a>(
    value: &'a str,
    config: Option<&DiagnosticConfig>,
) -> Cow<'a, str> {
    match config.filter(|config| config.is_raw_identity_allowed()) {
        Some(config) => diagnostic_value(value, config.max_value_length()),
        None => Cow::Borrowed("[REDACTED]"),
    }
}

pub(crate) fn diagnostic_debug_value<T: fmt::Debug>(
    value: &T,
    config: Option<&DiagnosticConfig>,
) -> Cow<'static, str> {
    match config.filter(|config| config.is_raw_identity_allowed()) {
        Some(config) => Cow::Owned(
            diagnostic_value(&format!("{value:?}"), config.max_value_length()).into_owned(),
        ),
        None => Cow::Borrowed("[REDACTED]"),
    }
}

pub(crate) fn diagnostic_value(value: &str, max_bytes: usize) -> Cow<'_, str> {
    if value.len() <= max_bytes {
        return Cow::Borrowed(value);
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    Cow::Owned(format!("{}... [TRUNCATED]", &value[..boundary]))
}

/// Zero-sized [`LifecycleObserver`] that records nothing.
///
/// Use it as the enabled-but-idle endpoint of [`measure_dispatch_overhead`],
/// or anywhere an observer slot must be filled without affecting behavior.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopObserver;

impl LifecycleObserver for NoopObserver {}

/// Thread-shared in-memory [`MetricSink`] for tests and local diagnosis.
///
/// Pair with [`BoundedMetricObserver`] so every recorded event already carries
/// only static allowlisted (or fallback) labels; this sink adds no labels of
/// its own. Clone shares the recording. [`InMemoryMetricSink::diagnose`] folds
/// the recording into timeout, retry, and saturation counts.
///
/// ```
/// use pbrs_grpc::telemetry::InMemoryMetricSink;
/// use pbrs_grpc::{
///     BoundedMetricObserver, CallLabels, CallRole, CancellationEvent, CancellationReason,
///     LifecycleObserver, MetricLabelPolicy,
/// };
///
/// static RPCS: &[&str] = &["/helloworld.Greeter/SayHello"];
/// static TARGETS: &[&str] = &[];
/// let policy = MetricLabelPolicy::new(RPCS, TARGETS).expect("static allowlist");
/// let sink = InMemoryMetricSink::new();
/// let observer = BoundedMetricObserver::new(policy, sink.clone());
/// let call = CallLabels::new("/helloworld.Greeter/SayHello", None, CallRole::Client);
/// observer.on_cancellation(&CancellationEvent {
///     call,
///     reason: CancellationReason::DeadlineExceeded,
/// });
/// assert_eq!(sink.diagnose().timeouts, 1);
/// ```
#[allow(
    clippy::disallowed_types,
    reason = "sync MetricSink::record never awaits; the critical section pushes one small enum"
)]
#[derive(Clone, Debug, Default)]
pub struct InMemoryMetricSink {
    events: Arc<std::sync::Mutex<Vec<MetricEvent>>>,
}

impl InMemoryMetricSink {
    /// Create an empty recording sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot every recorded event in arrival order.
    #[must_use]
    pub fn events(&self) -> Vec<MetricEvent> {
        self.lock_events().clone()
    }

    /// Number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock_events().len()
    }

    /// Whether no event has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock_events().is_empty()
    }

    /// Drop every recorded event.
    pub fn clear(&self) {
        self.lock_events().clear();
    }

    /// Fold the recording into timeout, retry, and saturation counts.
    ///
    /// Timeout and saturation signals are observations, not deduplicated
    /// calls: one timed-out call may contribute both a cancellation and a
    /// terminal `DEADLINE_EXCEEDED` status.
    #[must_use]
    pub fn diagnose(&self) -> MetricDiagnosis {
        let mut diagnosis = MetricDiagnosis::default();
        for event in self.events() {
            diagnosis.events += 1;
            match event {
                MetricEvent::CallStart { call } | MetricEvent::ServerCallStart { call } => {
                    diagnosis.count_rpc(call);
                }
                MetricEvent::CallEnd { call, code, .. }
                | MetricEvent::ServerCallEnd { call, code, .. } => {
                    diagnosis.count_rpc(call);
                    diagnosis.count_terminal(code);
                }
                MetricEvent::AttemptStart { call, .. } => {
                    diagnosis.count_rpc(call);
                }
                MetricEvent::AttemptEnd {
                    call, class, code, ..
                } => {
                    diagnosis.count_rpc(call);
                    diagnosis.count_terminal(code);
                    if class == MetricAttemptClass::Retry {
                        diagnosis.retries += 1;
                    }
                }
                MetricEvent::ClientQueueWait { call, wait } => {
                    diagnosis.count_rpc(call);
                    diagnosis.client_queue_waits += 1;
                    diagnosis.add_queue_wait(wait);
                }
                MetricEvent::ServerQueueWait { call, wait } => {
                    diagnosis.count_rpc(call);
                    diagnosis.server_queue_waits += 1;
                    diagnosis.add_queue_wait(wait);
                }
                MetricEvent::BytesSent { call, .. } | MetricEvent::BytesReceived { call, .. } => {
                    diagnosis.count_rpc(call);
                }
                MetricEvent::Reconnect { target, .. } => {
                    diagnosis.reconnects += 1;
                    *diagnosis.reconnect_targets.entry(target).or_default() += 1;
                }
                MetricEvent::Rejection { call, reason, .. } => {
                    diagnosis.count_rpc(call);
                    match reason {
                        RejectionReason::ConcurrencyLimit => {
                            diagnosis.rejected_concurrency += 1;
                        }
                        RejectionReason::ConnectionLimit => {
                            diagnosis.rejected_connection += 1;
                        }
                        _ => {}
                    }
                }
                MetricEvent::Cancellation { call, reason } => {
                    diagnosis.count_rpc(call);
                    if reason == CancellationReason::DeadlineExceeded {
                        diagnosis.timeouts += 1;
                    }
                }
            }
        }
        diagnosis
    }

    fn lock_events(&self) -> std::sync::MutexGuard<'_, Vec<MetricEvent>> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl MetricSink for InMemoryMetricSink {
    fn record(&self, event: MetricEvent) {
        self.lock_events().push(event);
    }
}

/// Timeout, retry, and saturation counts folded from one [`InMemoryMetricSink`].
///
/// `by_rpc` keys prove label cardinality: behind a [`BoundedMetricObserver`]
/// every key is a registered static path or [`OTHER_METRIC_LABEL`], no matter
/// how many distinct peer paths arrived. `reconnect_targets` gives the same
/// proof for reconnect target labels.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetricDiagnosis {
    /// Total metric events folded.
    pub events: u64,
    /// Timeout signals: `DeadlineExceeded` cancellations plus completed
    /// calls and attempts carrying [`Code::DeadlineExceeded`].
    pub timeouts: u64,
    /// Transparent-retry signals: attempt completions with
    /// [`MetricAttemptClass::Retry`].
    pub retries: u64,
    /// Client queue waits (pool acquisition or wait-for-ready).
    pub client_queue_waits: u64,
    /// Server post-admission scheduling delays.
    pub server_queue_waits: u64,
    /// Total queue wait observed (client plus server), saturating at
    /// [`Duration::MAX`].
    pub queue_wait_total: Duration,
    /// Rejections naming [`RejectionReason::ConcurrencyLimit`].
    pub rejected_concurrency: u64,
    /// Rejections naming [`RejectionReason::ConnectionLimit`].
    pub rejected_connection: u64,
    /// Completed calls and attempts carrying [`Code::ResourceExhausted`].
    pub resource_exhausted: u64,
    /// Completed reconnects observed.
    pub reconnects: u64,
    /// Event counts keyed by bounded rpc label.
    pub by_rpc: BTreeMap<&'static str, u64>,
    /// Reconnect counts keyed by bounded static target label.
    pub reconnect_targets: BTreeMap<&'static str, u64>,
}

impl MetricDiagnosis {
    /// Total saturation signals: rejected concurrency/connection budgets and
    /// resource-exhausted completions.
    ///
    /// Queue waits are counted separately (see `client_queue_waits`,
    /// `server_queue_waits`, and `queue_wait_total`) because every RPC waits
    /// briefly to acquire a connection or dispatch slot; only waits over a
    /// caller-chosen threshold indicate saturation.
    #[must_use]
    pub fn saturation_signals(&self) -> u64 {
        self.rejected_concurrency + self.rejected_connection + self.resource_exhausted
    }

    fn count_rpc(&mut self, call: MetricCallLabels) {
        *self.by_rpc.entry(call.rpc()).or_default() += 1;
    }

    fn count_terminal(&mut self, code: Code) {
        if code == Code::DeadlineExceeded {
            self.timeouts += 1;
        }
        if code == Code::ResourceExhausted {
            self.resource_exhausted += 1;
        }
    }

    fn add_queue_wait(&mut self, wait: Duration) {
        self.queue_wait_total = self
            .queue_wait_total
            .checked_add(wait)
            .unwrap_or(Duration::MAX);
    }
}

/// Explicit W3C Trace Context header contract, without the `otel` feature.
///
/// The recording propagator is `otel::trace::W3CPropagator` (feature `otel`);
/// this module pins the header names and the strict version-00 `traceparent`
/// shape so fixtures, examples, and redaction tests can assert propagation
/// bytes without linking OpenTelemetry.
pub mod propagation {
    /// `traceparent` header carrying version, trace id, parent span id, flags.
    pub const TRACEPARENT: &str = "traceparent";
    /// `tracestate` header carrying the vendor list.
    pub const TRACESTATE: &str = "tracestate";

    /// Whether `value` is a strict version-00 W3C `traceparent`.
    ///
    /// Accepts exactly `00-<32 lowercase-or-upper hex>-<16 hex>-<2 hex>` with
    /// non-zero trace and span ids. Future versions (extra trailing fields)
    /// and `tracestate` parsing belong to the real propagator; this predicate
    /// only certifies fixture bytes.
    #[must_use]
    pub fn valid_traceparent(value: &str) -> bool {
        let mut parts = value.split('-');
        let (Some(version), Some(trace), Some(span), Some(flags)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        if parts.next().is_some() || version != "00" {
            return false;
        }
        is_hex_len(trace, 32)
            && is_hex_len(span, 16)
            && is_hex_len(flags, 2)
            && !is_all_zero(trace)
            && !is_all_zero(span)
    }

    fn is_hex_len(text: &str, len: usize) -> bool {
        text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn is_all_zero(text: &str) -> bool {
        text.bytes().all(|byte| byte == b'0')
    }
}

/// Cost of lifecycle dispatch measured over `iters` iterations per path.
///
/// The baseline loop performs the same iteration shape with no dispatch; the
/// disabled path installs `None` (the production shape without an observer);
/// the enabled path installs [`NoopObserver`] behind an opaque `dyn` call.
/// Totals are wall-clock nanoseconds and only comparable within one run.
///
/// Per-iteration figures include the loop and [`std::hint::black_box`]
/// harness, so they overstate the dispatch itself; that makes them a
/// conservative input to the 2% disabled-CPU budget below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchOverhead {
    /// Loop iterations per path.
    pub iters: u32,
    /// Total nanoseconds of the loop with no dispatch (comparator).
    pub baseline_ns: u64,
    /// Total nanoseconds with `None` installed (disabled path).
    pub disabled_ns: u64,
    /// Total nanoseconds with [`NoopObserver`] installed (enabled path).
    pub enabled_ns: u64,
}

impl DispatchOverhead {
    /// Harness floor in picoseconds per iteration (0 when `iters` is 0).
    #[must_use]
    pub fn baseline_ps_per_iter(&self) -> u128 {
        Self::ps_per_iter(self.baseline_ns, self.iters)
    }

    /// Disabled-path cost in picoseconds per iteration (0 when `iters` is 0).
    ///
    /// Divide by the measured per-RPC CPU cost of the workload (for example
    /// the `rpc-bench` unary ping-pong CPU per RPC) to check the proposed 2%
    /// disabled-CPU budget: a 7,500 ps dispatch against a 20,000,000 ps RPC
    /// is 0.04%.
    #[must_use]
    pub fn disabled_ps_per_iter(&self) -> u128 {
        Self::ps_per_iter(self.disabled_ns, self.iters)
    }

    /// Enabled-path cost in picoseconds per iteration (0 when `iters` is 0).
    #[must_use]
    pub fn enabled_ps_per_iter(&self) -> u128 {
        Self::ps_per_iter(self.enabled_ns, self.iters)
    }

    fn ps_per_iter(total_ns: u64, iters: u32) -> u128 {
        if iters == 0 {
            return 0;
        }
        u128::from(total_ns) * 1_000 / u128::from(iters)
    }
}

/// Measure disabled (`None`) and enabled ([`NoopObserver`]) dispatch cost.
///
/// The dispatch helper mirrors the `if let Some(observer)` guard used by the
/// call and attempt guards, and the observer slot passes through
/// [`std::hint::black_box`] every iteration so the loop cannot be folded away
/// and the `dyn` call cannot be devirtualized. Wall-clock totals vary by host
/// and load; committed tests assert only the probe shape, while benchmark
/// evidence comes from repeated local runs plus the `rpc-bench` check.
///
/// ```
/// use pbrs_grpc::telemetry::measure_dispatch_overhead;
///
/// let overhead = measure_dispatch_overhead(8);
/// assert_eq!(overhead.iters, 8);
/// ```
#[must_use]
pub fn measure_dispatch_overhead(iters: u32) -> DispatchOverhead {
    use std::hint::black_box;

    let call = CallLabels::new(
        "/helloworld.Greeter/SayHello",
        Some("127.0.0.1:1"),
        CallRole::Client,
    );
    let status = Status::ok();
    let latency = Duration::from_micros(7);
    let noop = NoopObserver;
    let disabled: Option<&dyn LifecycleObserver> = black_box(None);
    let enabled: Option<&dyn LifecycleObserver> = black_box(Some(&noop));

    let baseline_ns = time_observer_loop(iters, || {
        black_box(&call);
    });
    let disabled_ns = time_observer_loop(iters, || {
        dispatch_call_end(black_box(disabled), &call, &status, latency);
    });
    let enabled_ns = time_observer_loop(iters, || {
        dispatch_call_end(black_box(enabled), &call, &status, latency);
    });
    DispatchOverhead {
        iters,
        baseline_ns,
        disabled_ns,
        enabled_ns,
    }
}

/// The production dispatch shape: one predictable branch, no label work.
#[inline]
fn dispatch_call_end(
    observer: Option<&dyn LifecycleObserver>,
    call: &CallLabels<'_>,
    status: &Status,
    latency: Duration,
) {
    if let Some(observer) = observer {
        observer.on_call_end(call, status, latency);
    }
}

fn time_observer_loop(iters: u32, mut body: impl FnMut()) -> u64 {
    let start = std::time::Instant::now();
    for _ in 0..iters {
        body();
    }
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// [`MetricSink`] recording gRFC-named OpenTelemetry instruments with bounded labels.
///
/// Pair with [`BoundedMetricObserver`]: every `grpc.method` value is a
/// registered static path or [`OTHER_METRIC_LABEL`], and `grpc.target` is
/// either the statically configured client target
/// ([`OtelMetricSink::with_target`]) or a registered reconnect target — never
/// a peer-supplied authority. This is the export-safe counterpart to
/// [`crate::otel::Metrics`], which records raw paths and authorities for
/// diagnostics and therefore has unbounded cardinality.
///
/// Recorded instruments reuse the names in [`crate::otel::instrument`]:
/// client attempt started/duration, client call duration, server call
/// started/duration, and subchannel connection outcomes. Queue waits, byte
/// counts, rejections, and cancellations have no instrument in this bridge
/// subset and are ignored; diagnose them from [`InMemoryMetricSink`].
/// Attempt classes likewise carry no label: retries are visible as repeated
/// `grpc.client.attempt.*` measurements, not as a separate series.
///
/// ```text
/// let sink = OtelMetricSink::with_meter(meter).with_target("greeter-primary");
/// let observer = BoundedMetricObserver::new(policy, sink);
/// let channel = Channel::connect(addr).await?.observer(observer);
/// ```
#[cfg(feature = "otel")]
#[derive(Clone)]
pub struct OtelMetricSink {
    inner: Arc<OtelInner>,
}

#[cfg(feature = "otel")]
#[derive(Clone)]
struct OtelInner {
    attempt_started: Counter<u64>,
    attempt_duration: Histogram<f64>,
    call_duration: Histogram<f64>,
    server_call_started: Counter<u64>,
    server_call_duration: Histogram<f64>,
    conn_succeeded: Counter<u64>,
    conn_failed: Counter<u64>,
    target: Option<&'static str>,
}

#[cfg(feature = "otel")]
impl OtelMetricSink {
    /// Record through the global meter provider.
    #[must_use]
    pub fn new() -> Self {
        Self::with_meter(global::meter_provider().meter("pbrs-grpc"))
    }

    /// Record through an explicit meter (tests, custom providers).
    #[must_use]
    pub fn with_meter(meter: Meter) -> Self {
        Self {
            inner: Arc::new(OtelInner {
                attempt_started: meter
                    .u64_counter(crate::otel::instrument::CLIENT_ATTEMPT_STARTED)
                    .with_unit("{attempt}")
                    .with_description("Total number of RPC attempts started (A66).")
                    .build(),
                attempt_duration: meter
                    .f64_histogram(crate::otel::instrument::CLIENT_ATTEMPT_DURATION)
                    .with_unit("s")
                    .with_description("End-to-end time to complete an RPC attempt (A66).")
                    .build(),
                call_duration: meter
                    .f64_histogram(crate::otel::instrument::CLIENT_CALL_DURATION)
                    .with_unit("s")
                    .with_description("End-to-end time to complete an RPC from the app view (A66).")
                    .build(),
                server_call_started: meter
                    .u64_counter(crate::otel::instrument::SERVER_CALL_STARTED)
                    .with_unit("{call}")
                    .with_description("Total number of server RPCs started (A66).")
                    .build(),
                server_call_duration: meter
                    .f64_histogram(crate::otel::instrument::SERVER_CALL_DURATION)
                    .with_unit("s")
                    .with_description("End-to-end time of a server RPC (A66).")
                    .build(),
                conn_succeeded: meter
                    .u64_counter(crate::otel::instrument::SUBCHANNEL_CONN_SUCCEEDED)
                    .with_unit("{attempt}")
                    .with_description("Successful subchannel connection attempts (A94).")
                    .build(),
                conn_failed: meter
                    .u64_counter(crate::otel::instrument::SUBCHANNEL_CONN_FAILED)
                    .with_unit("{attempt}")
                    .with_description("Failed subchannel connection attempts (A94).")
                    .build(),
                target: None,
            }),
        }
    }

    /// Attach a static client target label to every per-call measurement.
    ///
    /// The value must be operator-configured (a deployment or pool name), not
    /// copied from a peer-supplied authority, or the bounded-label guarantee
    /// is void. Without a target the per-call instruments carry method and
    /// status only.
    #[must_use]
    pub fn with_target(self, target: &'static str) -> Self {
        let mut inner = Arc::unwrap_or_clone(self.inner);
        inner.target = Some(target);
        Self {
            inner: Arc::new(inner),
        }
    }

    fn method_of(call: &MetricCallLabels) -> &'static str {
        call.rpc().strip_prefix('/').unwrap_or(call.rpc())
    }

    fn call_attrs(&self, call: &MetricCallLabels, code: Option<Code>) -> Vec<KeyValue> {
        let mut attrs = Vec::with_capacity(3);
        attrs.push(KeyValue::new(
            crate::otel::label::METHOD,
            Self::method_of(call),
        ));
        if let Some(target) = self.inner.target {
            attrs.push(KeyValue::new(crate::otel::label::TARGET, target));
        }
        if let Some(code) = code {
            attrs.push(KeyValue::new(crate::otel::label::STATUS, code.name()));
        }
        attrs
    }
}

#[cfg(feature = "otel")]
impl Default for OtelMetricSink {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "otel")]
impl MetricSink for OtelMetricSink {
    fn record(&self, event: MetricEvent) {
        match event {
            MetricEvent::CallStart { .. } => {}
            MetricEvent::CallEnd {
                call,
                code,
                latency,
            } => {
                self.inner
                    .call_duration
                    .record(latency.as_secs_f64(), &self.call_attrs(&call, Some(code)));
            }
            MetricEvent::AttemptStart { call, .. } => {
                self.inner
                    .attempt_started
                    .add(1, &self.call_attrs(&call, None));
            }
            MetricEvent::AttemptEnd {
                call,
                code,
                latency,
                ..
            } => {
                self.inner
                    .attempt_duration
                    .record(latency.as_secs_f64(), &self.call_attrs(&call, Some(code)));
            }
            MetricEvent::ServerCallStart { call } => {
                // A66 server instruments carry method only; the configured
                // client target never leaks onto the server series.
                let attrs = [KeyValue::new(
                    crate::otel::label::METHOD,
                    Self::method_of(&call),
                )];
                self.inner.server_call_started.add(1, &attrs);
            }
            MetricEvent::ServerCallEnd {
                call,
                code,
                latency,
            } => {
                let attrs = [
                    KeyValue::new(crate::otel::label::METHOD, Self::method_of(&call)),
                    KeyValue::new(crate::otel::label::STATUS, code.name()),
                ];
                self.inner
                    .server_call_duration
                    .record(latency.as_secs_f64(), &attrs);
            }
            MetricEvent::Reconnect { target, status, .. } => {
                let attrs = [KeyValue::new(crate::otel::label::TARGET, target)];
                match status {
                    None => self.inner.conn_succeeded.add(1, &attrs),
                    Some(_) => self.inner.conn_failed.add(1, &attrs),
                }
            }
            MetricEvent::ClientQueueWait { .. }
            | MetricEvent::ServerQueueWait { .. }
            | MetricEvent::BytesSent { .. }
            | MetricEvent::BytesReceived { .. }
            | MetricEvent::Rejection { .. }
            | MetricEvent::Cancellation { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticConfig, diagnostic_debug_value};
    use std::cell::Cell;
    use std::fmt;

    struct Counted<'a>(&'a Cell<usize>);

    impl fmt::Debug for Counted<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get() + 1);
            f.write_str("éééé private")
        }
    }

    #[test]
    fn diagnostic_peer_debug_is_lazy_and_unicode_safe() {
        let calls = Cell::new(0);
        let value = Counted(&calls);
        let no_consent = DiagnosticConfig::new().with_raw_identity(true);
        assert_eq!(diagnostic_debug_value(&value, None), "[REDACTED]");
        assert_eq!(
            diagnostic_debug_value(&value, Some(&no_consent)),
            "[REDACTED]"
        );
        assert_eq!(calls.get(), 0);

        let config = no_consent.with_consent(true).with_max_value_length(7);
        assert_eq!(
            diagnostic_debug_value(&value, Some(&config)),
            "ééé... [TRUNCATED]"
        );
        assert_eq!(calls.get(), 1);
    }
}

#[cfg(test)]
mod ob02_tests {
    use super::*;
    use std::time::Duration;

    static RPCS: &[&str] = &["/helloworld.Greeter/SayHello"];
    static TARGETS: &[&str] = &["primary"];

    fn rig() -> (
        BoundedMetricObserver<InMemoryMetricSink>,
        InMemoryMetricSink,
    ) {
        let policy = MetricLabelPolicy::new(RPCS, TARGETS).expect("static allowlist");
        let sink = InMemoryMetricSink::new();
        let observer = BoundedMetricObserver::new(policy, sink.clone());
        (observer, sink)
    }

    #[test]
    fn in_memory_sink_diagnoses_timeout_retry_and_saturation() {
        let (observer, sink) = rig();
        let client = CallLabels::new(
            "/helloworld.Greeter/SayHello",
            Some("tenant-a.internal"),
            CallRole::Client,
        );
        let server = CallLabels::new("/helloworld.Greeter/SayHello", None, CallRole::Server);

        // Timeout: caller-visible cancellation plus a terminal deadline status.
        observer.on_cancellation(&CancellationEvent {
            call: client,
            reason: CancellationReason::DeadlineExceeded,
        });
        observer.on_call_end(
            &client,
            &Status::deadline_exceeded(),
            Duration::from_millis(5),
        );

        // Transparent retry: initial attempt fails retryable, second succeeds.
        observer.on_attempt_start(&AttemptLabels::new(client, 1));
        observer.on_attempt_end(
            &AttemptLabels::new(client, 1),
            &Status::unavailable("reset"),
            Duration::from_millis(1),
        );
        observer.on_attempt_start(&AttemptLabels::new(client, 2));
        observer.on_attempt_end(
            &AttemptLabels::new(client, 2),
            &Status::ok(),
            Duration::from_millis(2),
        );

        // Saturation: a concurrency rejection, waits on both sides, an
        // exhausted server call.
        observer.on_rejection(&RejectionEvent {
            call: server,
            reason: RejectionReason::ConcurrencyLimit,
            code: Code::ResourceExhausted,
        });
        observer.on_queue_wait(&client, Duration::from_millis(2));
        observer.on_server_queue_wait(&server, Duration::from_millis(3));
        observer.on_server_call_end(
            &server,
            &Status::resource_exhausted("quota"),
            Duration::from_millis(4),
        );

        // Hostile cardinality: 1024 distinct peer paths plus a secret
        // reconnect target collapse to the fallback bucket.
        for index in 0..1024u32 {
            let path = format!("/evil.Service/Method{index}");
            let evil = CallLabels::new(path.as_str(), Some("tenant-secret"), CallRole::Server);
            observer.on_call_start(&evil);
        }
        observer.on_reconnect(&ReconnectEvent {
            target: "user:pass@hidden",
            attempt: 1,
            duration: Duration::from_millis(1),
            status: None,
        });

        let diagnosis = sink.diagnose();
        assert_eq!(diagnosis.timeouts, 2);
        assert_eq!(diagnosis.retries, 1);
        assert_eq!(diagnosis.rejected_concurrency, 1);
        assert_eq!(diagnosis.rejected_connection, 0);
        assert_eq!(diagnosis.client_queue_waits, 1);
        assert_eq!(diagnosis.server_queue_waits, 1);
        assert_eq!(diagnosis.queue_wait_total, Duration::from_millis(5));
        assert_eq!(diagnosis.resource_exhausted, 1);
        assert_eq!(diagnosis.reconnects, 1);
        assert_eq!(diagnosis.saturation_signals(), 2);
        assert_eq!(diagnosis.events, 1035);
        assert_eq!(diagnosis.by_rpc.len(), 2);
        assert!(
            diagnosis
                .by_rpc
                .contains_key("/helloworld.Greeter/SayHello")
        );
        assert_eq!(
            diagnosis.by_rpc.get(OTHER_METRIC_LABEL),
            Some(&1024),
            "1024 hostile paths share one bucket"
        );
        assert_eq!(diagnosis.reconnect_targets.len(), 1);
        assert!(diagnosis.reconnect_targets.contains_key(OTHER_METRIC_LABEL));
    }

    #[test]
    fn bounded_metric_events_carry_no_peer_secrets() {
        let (observer, sink) = rig();
        let evil = CallLabels::new(
            "/evil.Service/Method",
            Some("tenant-secret"),
            CallRole::Server,
        );
        observer.on_server_call_start(&evil);
        observer.on_server_call_end(
            &evil,
            &Status::unavailable("token=hunter2"),
            Duration::from_millis(1),
        );
        observer.on_reconnect(&ReconnectEvent {
            target: "user:pass@hidden",
            attempt: 2,
            duration: Duration::from_millis(1),
            status: Some(Code::Unavailable),
        });

        let dumped = format!("{:?}", sink.events());
        for secret in [
            "tenant-secret",
            "hunter2",
            "user:pass@hidden",
            "/evil.Service/Method",
        ] {
            assert!(!dumped.contains(secret), "leaked {secret}: {dumped}");
        }
        assert!(dumped.contains(OTHER_METRIC_LABEL));
    }

    #[test]
    fn traceparent_validator_accepts_only_strict_v00() {
        let good = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert!(propagation::valid_traceparent(good));
        assert!(propagation::valid_traceparent(
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-00"
        ));
        for bad in [
            "",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-zzf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-011",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        ] {
            assert!(!propagation::valid_traceparent(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn telemetry_context_debug_redacts_values_but_keeps_keys() {
        let mut metadata = Metadata::new();
        metadata
            .set("authorization", "Bearer hunter2")
            .expect("ascii value");
        metadata
            .set(
                propagation::TRACEPARENT,
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            .expect("ascii value");
        let status = Status::unavailable("token=hunter2");
        let call = CallLabels::new(
            "/helloworld.Greeter/SayHello",
            Some("tenant-a.internal"),
            CallRole::Client,
        );
        let ctx = TelemetryContext::new(call)
            .with_metadata(&metadata)
            .with_status(&status);
        let dumped = format!("{ctx:?}");
        assert!(dumped.contains("authorization"), "key visible: {dumped}");
        assert!(dumped.contains("traceparent"), "key visible: {dumped}");
        for secret in ["hunter2", "Bearer", "4bf92f35", "tenant-a.internal"] {
            assert!(!dumped.contains(secret), "leaked {secret}: {dumped}");
        }
    }

    #[test]
    fn dispatch_overhead_probe_reports_disabled_and_enabled() {
        let overhead = measure_dispatch_overhead(200_000);
        eprintln!(
            "OB-02 dispatch overhead over {} iters (ps/iter): baseline={} disabled={} enabled={}",
            overhead.iters,
            overhead.baseline_ps_per_iter(),
            overhead.disabled_ps_per_iter(),
            overhead.enabled_ps_per_iter(),
        );
        assert_eq!(overhead.iters, 200_000);
    }

    #[test]
    fn metric_event_stays_stack_small() {
        assert!(
            std::mem::size_of::<MetricEvent>() <= 64,
            "MetricEvent grew: {} bytes",
            std::mem::size_of::<MetricEvent>()
        );
    }

    #[cfg(feature = "otel")]
    #[test]
    fn otel_sink_exports_only_bounded_labels() {
        use opentelemetry::metrics::MeterProvider as _;
        use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
        use opentelemetry_sdk::metrics::{
            InMemoryMetricExporterBuilder, PeriodicReader, SdkMeterProvider,
        };

        let exporter = InMemoryMetricExporterBuilder::new().build();
        let reader = PeriodicReader::builder(exporter.clone()).build();
        let provider = SdkMeterProvider::builder().with_reader(reader).build();
        let sink = OtelMetricSink::with_meter(provider.meter("ob02")).with_target("test-target");
        let policy = MetricLabelPolicy::new(RPCS, TARGETS).expect("static allowlist");
        let observer = BoundedMetricObserver::new(policy, sink);

        let registered = CallLabels::new(
            "/helloworld.Greeter/SayHello",
            Some("tenant-a.internal"),
            CallRole::Client,
        );
        let hostile = CallLabels::new(
            "/evil.Service/Method",
            Some("tenant-secret"),
            CallRole::Client,
        );
        observer.on_attempt_start(&AttemptLabels::new(registered, 1));
        observer.on_attempt_end(
            &AttemptLabels::new(registered, 1),
            &Status::ok(),
            Duration::from_micros(3),
        );
        observer.on_call_end(&registered, &Status::ok(), Duration::from_micros(5));
        observer.on_attempt_start(&AttemptLabels::new(hostile, 1));
        observer.on_call_end(
            &hostile,
            &Status::unavailable("token=hunter2"),
            Duration::from_micros(5),
        );
        observer.on_reconnect(&ReconnectEvent {
            target: "user:pass@hidden",
            attempt: 1,
            duration: Duration::from_millis(1),
            status: None,
        });
        observer.on_reconnect(&ReconnectEvent {
            target: "primary",
            attempt: 2,
            duration: Duration::from_millis(1),
            status: Some(Code::Unavailable),
        });

        provider.force_flush().expect("flush");
        let mut pairs: Vec<(String, String)> = Vec::new();
        for resource in exporter.get_finished_metrics().expect("metrics") {
            for scope in resource.scope_metrics() {
                for metric in scope.metrics() {
                    match metric.data() {
                        AggregatedMetrics::U64(MetricData::Sum(sum)) => {
                            for point in sum.data_points() {
                                pairs.extend(
                                    point
                                        .attributes()
                                        .map(|kv| (kv.key.to_string(), kv.value.to_string())),
                                );
                            }
                        }
                        AggregatedMetrics::F64(MetricData::Histogram(hist)) => {
                            for point in hist.data_points() {
                                pairs.extend(
                                    point
                                        .attributes()
                                        .map(|kv| (kv.key.to_string(), kv.value.to_string())),
                                );
                            }
                        }
                        other => panic!("unexpected aggregation: {other:?}"),
                    }
                }
            }
        }
        assert!(!pairs.is_empty());
        for secret in [
            "tenant-secret",
            "hunter2",
            "user:pass@hidden",
            "evil.Service",
            "tenant-a.internal",
        ] {
            assert!(
                !pairs.iter().any(|(_, value)| value.contains(secret)),
                "leaked {secret}: {pairs:?}"
            );
        }
        let values_of = |key: &str| -> Vec<&str> {
            pairs
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect()
        };
        for method in values_of(crate::otel::label::METHOD) {
            assert!(
                method == "helloworld.Greeter/SayHello" || method == OTHER_METRIC_LABEL,
                "unbounded method {method:?}"
            );
        }
        for target in values_of(crate::otel::label::TARGET) {
            assert!(
                target == "test-target" || target == "primary" || target == OTHER_METRIC_LABEL,
                "unbounded target {target:?}"
            );
        }
    }
}
