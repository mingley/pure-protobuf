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
}

impl CallGuard {
    pub(crate) fn new(
        observer: Option<Arc<dyn LifecycleObserver>>,
        labels: Option<OwnedCallLabels>,
        start: std::time::Instant,
    ) -> Self {
        Self {
            observer,
            labels,
            start,
            completed: false,
            cancelled: false,
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
/// By default, all credential-sensitive metadata (such as `authorization`,
/// `cookie`, `set-cookie`, `proxy-authorization`, binary metadata `-bin`,
/// and token/secret headers) and request/response payloads are redacted.
///
/// Diagnostic detail requires explicit consent (`consent: true`). Even when
/// consent is provided, strict cardinality limits apply to prevent log and
/// telemetry pollution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticConfig {
    consent: bool,
    allow_payload: bool,
    allow_sensitive_headers: bool,
    allow_binary_metadata: bool,
    max_metadata_entries: usize,
    max_value_length: usize,
    custom_sensitive_headers: BTreeSet<String>,
}

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            consent: false,
            allow_payload: false,
            allow_sensitive_headers: false,
            allow_binary_metadata: false,
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

    /// Permit unredacted sensitive headers (requires explicit consent).
    #[must_use]
    pub fn with_sensitive_headers(mut self, allow: bool) -> Self {
        self.allow_sensitive_headers = allow;
        self
    }

    /// Permit binary metadata display (requires explicit consent).
    #[must_use]
    pub fn with_binary_metadata(mut self, allow: bool) -> Self {
        self.allow_binary_metadata = allow;
        self
    }

    /// Set a cardinality limit on the number of metadata entries formatted.
    #[must_use]
    pub fn with_max_metadata_entries(mut self, max: usize) -> Self {
        self.max_metadata_entries = max;
        self
    }

    /// Set a limit on the maximum length of a header value before truncation.
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

    /// Check whether sensitive headers can be displayed unredacted (both `allow_sensitive_headers` AND `consent` must be true).
    #[must_use]
    pub fn are_sensitive_headers_allowed(&self) -> bool {
        self.consent && self.allow_sensitive_headers
    }

    /// Check whether binary metadata can be displayed unredacted (both `allow_binary_metadata` AND `consent` must be true).
    #[must_use]
    pub fn is_binary_metadata_allowed(&self) -> bool {
        self.consent && self.allow_binary_metadata
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
/// credentials, cookies, binary metadata, or payloads by default.
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
        let mut s = f.debug_struct("TelemetryContext");
        s.field("service", &self.call.service())
            .field("method", &self.call.method())
            .field("path", &self.call.path())
            .field("role", &self.call.role())
            .field("authority", &self.call.authority());

        if let Some(status) = self.status {
            s.field("status_code", &status.code());
            s.field("status_message", &status.message());
        }

        if let Some(metadata) = self.metadata {
            s.field("metadata", &metadata.safe_debug(&self.config));
        }

        s.finish_non_exhaustive()
    }
}
