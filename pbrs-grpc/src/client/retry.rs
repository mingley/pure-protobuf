//! Policy retry and hedging execution.

use super::Channel;
use super::unary::{UnaryFailure, run_unary_frame};
use crate::codec::CodecMessage;
use crate::config::Wire;
use crate::request::Response;
use crate::service_config::{HedgingPolicy, RetryPolicy, retry_backoff};
use crate::status::Status;
use crate::telemetry::{
    AttemptGuard, AttemptLabels, CallGuard, CancellationReason, LifecycleObserver, OwnedCallLabels,
};
use http::HeaderValue;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;
use tokio::task::JoinSet;

/// Retry statistics for one [`Channel`](super::Channel), A45-style.
///
/// Every counter covers the channel's unary and server-streaming calls,
/// whether or not a retry policy is attached: calls without a policy still
/// record `calls` and their terminal outcome, so exporters can compute retry
/// rates. Recording is a few atomic adds per call outcome plus one per retry
/// decision, always on; there is no sampling flag. GF-01 exports these to
/// OpenTelemetry (A96); until then read them with
/// [`Channel::retry_stats`](super::Channel::retry_stats).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetryStats {
    /// Calls entering the unary or server-streaming executors.
    pub calls: u64,
    /// Additional policy attempts whose request HEADERS were opened.
    /// A retry decision or interrupted backoff alone does not increment this.
    pub policy_retries: u64,
    /// Transparent redials on raced connection deaths (never policy-gated).
    pub transparent_retries: u64,
    /// Hedged sends past the first attempt.
    pub hedged_sends: u64,
    /// Retries or hedged sends refused by the throttling bucket.
    pub throttled: u64,
    /// Dispatched retries that completed a server pushback delay.
    pub pushback_delays: u64,
    /// Retries refused by a server `DoNotRetry` pushback.
    pub pushback_refusals: u64,
    /// Per-attempt recv timeouts that led to a dispatched policy retry.
    pub per_attempt_timeouts: u64,
    /// Calls failing with a retryable outcome after attempts ran out.
    pub exhausted: u64,
    /// Calls committed `OK`, including hedged and retried calls.
    pub committed_ok: u64,
    /// Calls committed non-OK, including exhausted and throttled calls.
    pub committed_err: u64,
}

/// Channel-scoped atomic recorder behind [`RetryStats`].
///
/// One per [`Channel`](super::Channel), shared by clones. All methods are
/// lock-free; contention is one atomic add per recorded event.
#[derive(Debug, Default)]
pub(crate) struct RetryStatsRecorder {
    calls: AtomicU64,
    policy_retries: AtomicU64,
    transparent_retries: AtomicU64,
    hedged_sends: AtomicU64,
    throttled: AtomicU64,
    pushback_delays: AtomicU64,
    pushback_refusals: AtomicU64,
    per_attempt_timeouts: AtomicU64,
    exhausted: AtomicU64,
    committed_ok: AtomicU64,
    committed_err: AtomicU64,
}

impl RetryStatsRecorder {
    /// A zeroed recorder. Distinct from [`Self::snapshot`]: that reads.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A point-in-time copy. Counters keep moving under concurrent calls.
    pub(crate) fn snapshot(&self) -> RetryStats {
        RetryStats {
            calls: self.calls.load(Ordering::Relaxed),
            policy_retries: self.policy_retries.load(Ordering::Relaxed),
            transparent_retries: self.transparent_retries.load(Ordering::Relaxed),
            hedged_sends: self.hedged_sends.load(Ordering::Relaxed),
            throttled: self.throttled.load(Ordering::Relaxed),
            pushback_delays: self.pushback_delays.load(Ordering::Relaxed),
            pushback_refusals: self.pushback_refusals.load(Ordering::Relaxed),
            per_attempt_timeouts: self.per_attempt_timeouts.load(Ordering::Relaxed),
            exhausted: self.exhausted.load(Ordering::Relaxed),
            committed_ok: self.committed_ok.load(Ordering::Relaxed),
            committed_err: self.committed_err.load(Ordering::Relaxed),
        }
    }

    /// Record a call entering a policy-aware executor.
    pub(crate) fn record_call(&self) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a policy retry actually sent.
    pub(crate) fn record_policy_retry(&self) {
        self.policy_retries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a transparent redial on a raced connection death.
    pub(crate) fn record_transparent_retry(&self) {
        self.transparent_retries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a hedged send past the first attempt.
    pub(crate) fn record_hedged_send(&self) {
        self.hedged_sends.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a retry or hedged send refused by the throttling bucket.
    pub(crate) fn record_throttled(&self) {
        self.throttled.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a retry honoring a server pushback delay.
    pub(crate) fn record_pushback_delay(&self) {
        self.pushback_delays.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a retry refused by a server `DoNotRetry` pushback.
    pub(crate) fn record_pushback_refusal(&self) {
        self.pushback_refusals.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a per-attempt recv timeout that triggered a retry.
    pub(crate) fn record_per_attempt_timeout(&self) {
        self.per_attempt_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a call failing retryably after attempts ran out.
    pub(crate) fn record_exhausted(&self) {
        self.exhausted.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a terminal call outcome.
    pub(crate) fn record_committed(&self, ok: bool) {
        if ok {
            self.committed_ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.committed_err.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Pending policy-attempt accounting, consumed only after request HEADERS
/// open. A transparent replacement of that attempt must not count it again.
pub(crate) struct PolicyRetryDispatch<'a> {
    stats: &'a RetryStatsRecorder,
    pending: Option<(bool, bool)>,
}

impl<'a> PolicyRetryDispatch<'a> {
    pub(crate) fn new(stats: &'a RetryStatsRecorder) -> Self {
        Self {
            stats,
            pending: None,
        }
    }

    pub(crate) fn arm(&mut self, via_pushback: bool, per_attempt_timeout: bool) {
        self.pending = Some((via_pushback, per_attempt_timeout));
    }

    pub(crate) fn on_headers_opened(&mut self) {
        if let Some((via_pushback, per_attempt_timeout)) = self.pending.take() {
            self.stats.record_policy_retry();
            if via_pushback {
                self.stats.record_pushback_delay();
            }
            if per_attempt_timeout {
                self.stats.record_per_attempt_timeout();
            }
        }
    }
}

/// Policy replay's existing service-config §6.3 fallback. This does not
/// change the unlimited outbound message-size default.
const POLICY_REPLAY_BYTES: usize = 4 * 1024 * 1024;

/// One encoded request, retained only while replay is authorized. Oversized
/// policy requests move into their first attempt and commit against any
/// replacement; the transport can then release storage during response wait.
pub(crate) struct RequestReplay {
    frame: crate::wire::SegFrame,
    allowed: bool,
}

impl RequestReplay {
    pub(crate) fn new(
        frame: crate::wire::SegFrame,
        policy: bool,
        limits: crate::limits::MessageLimits,
    ) -> Self {
        let payload = frame.total_len().saturating_sub(crate::codec::HEADER_LEN);
        let budget = limits.max_encoding().unwrap_or(POLICY_REPLAY_BYTES);
        Self {
            frame,
            allowed: !policy || payload <= budget,
        }
    }

    pub(crate) fn allowed(&self) -> bool {
        self.allowed
    }

    pub(crate) fn total_len(&self) -> usize {
        self.frame.total_len()
    }

    /// Called once for a committed request, or once per eligible attempt.
    pub(crate) fn next_attempt(&mut self) -> crate::wire::SegFrame {
        if self.allowed {
            self.frame.clone()
        } else {
            std::mem::replace(
                &mut self.frame,
                crate::wire::SegFrame::single(bytes::Bytes::new()),
            )
        }
    }
}

/// A6 policy-retry decision: whether and how long to wait for the next attempt.
///
/// Distinct from a bare delay: the variants tell the caller which
/// [`RetryStats`] counter to record. `Declined` covers no policy, exhausted
/// attempts, cancellation, and non-retryable codes.
pub(crate) enum PolicyDecision {
    /// Retry after this delay; `via_pushback` names a server pushback delay
    /// rather than the computed jittered backoff for retry number
    /// `attempts_made` (1-based, so the first retry uses index 0).
    Proceed { delay: Duration, via_pushback: bool },
    /// The throttling bucket refused another send.
    Throttled,
    /// The server sent `DoNotRetry` pushback.
    PushbackRefused,
    /// No policy, attempts exhausted, cancelled, or a non-retryable code (a
    /// per-attempt timeout counts as retryable on its own).
    Declined,
}

/// Decide whether `status` earns another attempt under `policy`.
pub(crate) async fn policy_retry_delay(
    channel: &Channel,
    policy: Option<&RetryPolicy>,
    status: &Status,
    attempts_made: u32,
    per_attempt_timeout: bool,
    cancelled: bool,
) -> PolicyDecision {
    let Some(policy) = policy else {
        return PolicyDecision::Declined;
    };
    if cancelled || attempts_made >= policy.max_attempts {
        return PolicyDecision::Declined;
    }
    if matches!(
        status.retry_pushback(),
        Some(crate::status::Pushback::DoNotRetry)
    ) {
        return PolicyDecision::PushbackRefused;
    }
    let retryable = per_attempt_timeout || policy.retryable_status_codes.contains(&status.code());
    if !retryable {
        return PolicyDecision::Declined;
    }
    if !channel.retry_allowed().await {
        return PolicyDecision::Throttled;
    }
    if let Some(crate::status::Pushback::Delay(delay)) = status.retry_pushback() {
        return PolicyDecision::Proceed {
            delay,
            via_pushback: true,
        };
    }
    PolicyDecision::Proceed {
        delay: retry_backoff(
            policy.initial_backoff,
            policy.max_backoff,
            policy.backoff_multiplier,
            attempts_made.saturating_sub(1),
        ),
        via_pushback: false,
    }
}

/// Whether `status` fails a call whose policy attempts ran out.
pub(crate) fn retry_exhausted(
    policy: Option<&RetryPolicy>,
    status: &Status,
    attempts_made: u32,
    per_attempt_timeout: bool,
    cancelled: bool,
) -> bool {
    match policy {
        None => false,
        Some(policy) => {
            !cancelled
                && attempts_made >= policy.max_attempts
                && (per_attempt_timeout || policy.retryable_status_codes.contains(&status.code()))
                && !matches!(
                    status.retry_pushback(),
                    Some(crate::status::Pushback::DoNotRetry)
                )
        }
    }
}

/// One hedged unary call: everything an attempt task needs, all owned.
///
/// The concurrency semaphore permit travels here so the call keeps holding its
/// slot while hedges are in flight.
pub(crate) struct HedgeUnary {
    pub(crate) path: &'static str,
    pub(crate) md: crate::metadata::Metadata,
    pub(crate) req_timeout: Option<Duration>,
    pub(crate) deadline: Option<tokio::time::Instant>,
    pub(crate) wait: bool,
    pub(crate) compress: bool,
    pub(crate) frame: crate::wire::SegFrame,
    pub(crate) cancel_rx: crate::rpc_cancel::RpcCancel,
    pub(crate) wire: Wire,
    pub(crate) ua: HeaderValue,
    pub(crate) policy: HedgingPolicy,
    pub(crate) call_guard: CallGuard,
    pub(crate) owned_labels: Option<Box<OwnedCallLabels>>,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) permit: Option<OwnedSemaphorePermit>,
    pub(crate) binlog: Option<crate::binlog::CallLogger>,
}

/// A finished hedged attempt: its 1-based index plus the outcome.
type HedgeOutcome<Resp> = (u32, Result<Response<Resp>, Status>);

// A started stream is a separate channelz event even when a task is aborted
// or unwinds before run_unary_frame returns. A transparent redial creates
// a new guard for the replacement stream.
struct HedgeStreamAccounting {
    socket: Option<crate::channelz::SocketId>,
}

impl HedgeStreamAccounting {
    fn start(socket: Option<crate::channelz::SocketId>) -> Self {
        if let Some(socket) = socket {
            crate::channelz::Registry::global().note_stream_started(socket, true);
        }
        Self { socket }
    }

    fn finish(&mut self, ok: bool) {
        if let Some(socket) = self.socket.take() {
            let registry = crate::channelz::Registry::global();
            registry.note_stream_end(socket, ok);
            if ok {
                registry.note_messages(socket, true, 1);
                registry.note_messages(socket, false, 1);
            }
        }
    }
}

impl Drop for HedgeStreamAccounting {
    fn drop(&mut self) {
        self.finish(false);
    }
}

/// Run one hedged attempt: grab, send, transparent-redial once on a raced
/// connection death, then report exactly one outcome.
fn spawn_hedge_attempt<Resp>(
    handles: &mut JoinSet<HedgeOutcome<Resp>>,
    channel: Channel,
    req: &HedgeUnary,
    attempt: u32,
) where
    Resp: CodecMessage + Send + 'static,
{
    let path = req.path;
    let md = req.md.clone();
    let req_timeout = req.req_timeout;
    let deadline = req.deadline;
    let wait = req.wait;
    let compress = req.compress;
    let frame = req.frame.clone();
    let cancel_rx = req.cancel_rx.clone();
    let wire = req.wire;
    let ua = req.ua.clone();
    let observer = req.observer.clone();
    let owned_labels = req.owned_labels.clone();
    let tap = req.binlog.clone();
    drop(handles.spawn(async move {
        let mut attempt_guard = AttemptGuard::new(
            observer.clone(),
            owned_labels.clone(),
            attempt,
            std::time::Instant::now(),
        );
        if let (Some(obs), Some(call)) = (&observer, &owned_labels) {
            let borrowed = call.as_borrowed();
            obs.on_attempt_start(&AttemptLabels::new(borrowed, attempt));
        }
        let mut redialed = false;
        let outcome: Result<Response<Resp>, Status> = loop {
            let queue_start = tokio::time::Instant::now();
            let live = match channel.grab(&cancel_rx, deadline, wait, Some(&md)).await {
                Ok(live) => {
                    if let (Some(obs), Some(call)) = (&observer, &owned_labels) {
                        obs.on_queue_wait(&call.as_borrowed(), queue_start.elapsed());
                    }
                    live
                }
                Err(status) => break Err(status),
            };
            let (slot, r#gen, route) = (live.slot, live.r#gen, live.route);
            let live_socket = live.channelz_socket;
            let _load = live.load;
            // A48 least-request: RAII in-flight count for this
            // attempt; drops (releasing) on every exit path and
            // on hedged-task abort.
            let _lr =
                super::pool::track_least_request(&channel.inner.endpoint, route.resolved_address())
                    .await;
            let byte_permit = match channel.local.byte_budget.acquire(frame.total_len()) {
                Ok(permit) => permit,
                Err(status) => break Err(status),
            };
            if let (Some(obs), Some(call)) = (&observer, &owned_labels) {
                obs.on_bytes_sent(&call.as_borrowed(), frame.total_len());
            }
            // Channelz: the hedged attempt's stream starts here
            // (past setup rejects, so every start pairs with an end).
            let mut stream_accounting = HedgeStreamAccounting::start(live_socket);
            let routing = channel.local.for_attempt(route.authority());
            match run_unary_frame(
                live.send,
                &routing,
                path,
                &md,
                req_timeout,
                deadline,
                compress,
                frame.clone(),
                cancel_rx.clone(),
                wire,
                ua.clone(),
                byte_permit,
                tap.as_ref(),
                None,
            )
            .await
            {
                Err(UnaryFailure {
                    status,
                    response_committed: false,
                }) if !redialed
                    && status.is_transparent_retryable()
                    && channel.inner.endpoint.can_redial() =>
                {
                    redialed = true;
                    channel.local.retry_stats.record_transparent_retry();
                    stream_accounting.finish(false);
                    channel
                        .inner
                        .discard_conn(slot, r#gen, route.resolved_address())
                        .await;
                }
                result => {
                    let result = result.map_err(|failure| failure.status);
                    // Channelz: the hedged attempt's stream ends here
                    // (completed unaries claim one message each way).
                    stream_accounting.finish(result.is_ok());
                    // A58 per-call ORCA: every hedged attempt's trailers
                    // feed weights, including failed attempts.
                    let trailers = match &result {
                        Ok(response) => response.trailers(),
                        Err(status) => status.metadata(),
                    };
                    super::pool::ingest_orca_report(
                        &channel.inner.endpoint,
                        route.resolved_address(),
                        trailers,
                    )
                    .await;
                    // A50 outlier detection: every hedged attempt's
                    // outcome feeds the detectors.
                    match &result {
                        Ok(_) => super::pool::ingest_call_status(
                            &channel.inner.endpoint,
                            route.resolved_address(),
                            &Status::ok(),
                        ),
                        Err(status) => super::pool::ingest_call_status(
                            &channel.inner.endpoint,
                            route.resolved_address(),
                            status,
                        ),
                    }
                    if let Err(status) = &result {
                        if status.is_transport() {
                            channel
                                .inner
                                .discard_conn(slot, r#gen, route.resolved_address())
                                .await;
                        }
                    }
                    break result;
                }
            }
        };
        match &outcome {
            Ok(_) => attempt_guard.finish(&Status::ok()),
            Err(status) => attempt_guard.finish(status),
        }
        // On a current-thread test runtime this synchronous notification is
        // immediately followed by task completion, before the waiting test
        // can resume and release its held throttle lock.
        #[cfg(test)]
        super::hedge_contention_tests::record_enqueue(attempt);
        (attempt, outcome)
    }));
}

/// Abort every outstanding hedged attempt. Aborted attempts report
/// cancellation through their guard's `Drop`, matching a dropped [`Call`].
fn abort_hedges<Resp: 'static>(handles: &mut JoinSet<HedgeOutcome<Resp>>) {
    handles.abort_all();
}

/// Sleep until `at`, or forever when there is no deadline.
async fn sleep_until_or_pending(at: Option<tokio::time::Instant>) {
    match at {
        Some(instant) => tokio::time::sleep_until(instant).await,
        None => std::future::pending().await,
    }
}

/// Complete when the call's cancel flag is set.
async fn cancel_fired(cancel_rx: crate::rpc_cancel::RpcCancel) {
    cancel_rx.wait().await;
}

/// Pending sends and their timer for a single hedged call.
struct HedgeSchedule {
    at: Option<tokio::time::Instant>,
    pending: u32,
    stopped: bool,
}

impl HedgeSchedule {
    fn new(max: u32, delay: Option<Duration>, now: tokio::time::Instant) -> Self {
        match delay {
            Some(delay) => Self {
                at: (max > 1).then_some(now + delay),
                pending: 0,
                stopped: false,
            },
            None => Self {
                at: None,
                pending: max.saturating_sub(1),
                stopped: false,
            },
        }
    }

    fn stop(&mut self) {
        self.at = None;
        self.pending = 0;
        self.stopped = true;
    }

    fn nonfatal(
        &mut self,
        pushback: Option<crate::status::Pushback>,
        remaining: u32,
        now: tokio::time::Instant,
    ) {
        if self.stopped {
            return;
        }
        match pushback {
            Some(crate::status::Pushback::DoNotRetry) => self.stop(),
            Some(crate::status::Pushback::Delay(delay)) => {
                self.at = None;
                self.pending = 0;
                if remaining > 0 {
                    if delay.is_zero() {
                        self.pending = 1;
                    } else {
                        self.at = Some(now + delay);
                    }
                }
            }
            None => {
                self.at = None;
                self.pending = self.pending.saturating_add(1).min(remaining);
            }
        }
    }

    fn timer_elapsed(&mut self) {
        self.at = None;
        self.pending = 1;
    }

    fn sent(&mut self, remaining: u32, delay: Option<Duration>, now: tokio::time::Instant) {
        self.pending -= 1;
        if self.pending == 0 && remaining > 0 {
            match delay {
                Some(delay) => self.at = Some(now + delay),
                None => self.pending = remaining,
            }
        }
    }
}

enum HedgeEvent<T> {
    Cancelled,
    Deadline,
    Outcome(Option<T>),
    Permission(bool),
    Timer,
}

struct HedgeReadiness {
    attempts: bool,
    pending: bool,
    timer: bool,
}

/// A completed attempt takes precedence over a send whose throttle check is
/// simultaneously ready. Cancellation and the deadline take precedence over
/// both. Disabled attempt, permit, and timer futures are not polled.
async fn next_hedge_event<T>(
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
    hedge_at: Option<tokio::time::Instant>,
    outcome: impl std::future::Future<Output = Option<T>>,
    ready: HedgeReadiness,
    allowed: impl std::future::Future<Output = bool>,
) -> HedgeEvent<T> {
    tokio::select! {
        biased;
        () = cancel_fired(cancel_rx) => HedgeEvent::Cancelled,
        () = sleep_until_or_pending(deadline) => HedgeEvent::Deadline,
        result = outcome, if ready.attempts => HedgeEvent::Outcome(result),
        permission = allowed, if ready.pending => HedgeEvent::Permission(permission),
        () = sleep_until_or_pending(hedge_at), if ready.timer => HedgeEvent::Timer,
    }
}

impl super::Channel {
    /// Run a unary call under an A6 hedging policy.
    ///
    /// The first attempt goes out immediately and is never throttled. When it
    /// has not finished after `hedgingDelay`, the next attempt goes out, up to
    /// `maxAttempts`; an unset delay makes all remaining sends immediately
    /// eligible. Sends past the first need a throttling token and stop on a
    /// `DoNotRetry` pushback. The first `OK` wins; a non-OK status outside
    /// `nonFatalStatusCodes` commits immediately. A non-fatal status advances
    /// the next send, unless pushback specifies a delay or stops future sends.
    /// When attempts are exhausted the last non-fatal error fails the call.
    /// Every attempt may transparently redial once on a safe pre-commit
    /// transport failure.
    #[allow(
        clippy::too_many_lines,
        reason = "one select loop over cancel, deadline, hedge timer, and completions"
    )]
    pub(crate) async fn execute_hedged<Resp>(
        &self,
        mut req: HedgeUnary,
    ) -> Result<Response<Resp>, Status>
    where
        Resp: CodecMessage + Send + 'static,
    {
        let _held = req.permit.take();
        let max = req.policy.max_attempts.max(1);
        let mut handles = JoinSet::new();
        let mut sent = 1u32;
        let mut outstanding = 1u32;
        let mut last_err: Option<Status> = None;
        let mut schedule =
            HedgeSchedule::new(max, req.policy.hedging_delay, tokio::time::Instant::now());
        spawn_hedge_attempt(&mut handles, self.clone(), &req, 1);
        loop {
            let pending = schedule.pending > 0 && !schedule.stopped && sent < max;
            let timer =
                schedule.at.is_some() && schedule.pending == 0 && !schedule.stopped && sent < max;
            let has_attempts = !handles.is_empty();
            let event = next_hedge_event(
                req.cancel_rx.clone(),
                req.deadline,
                schedule.at,
                handles.join_next(),
                HedgeReadiness {
                    attempts: has_attempts,
                    pending,
                    timer,
                },
                self.retry_allowed(),
            )
            .await;
            match event {
                HedgeEvent::Cancelled => {
                    abort_hedges(&mut handles);
                    req.call_guard.cancel(CancellationReason::CallerCancelled);
                    let status = Status::cancelled();
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    self.local.retry_stats.record_committed(false);
                    return Err(status);
                }
                HedgeEvent::Deadline => {
                    abort_hedges(&mut handles);
                    req.call_guard.cancel(CancellationReason::DeadlineExceeded);
                    let status = Status::deadline_exceeded();
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    self.local.retry_stats.record_committed(false);
                    return Err(status);
                }
                HedgeEvent::Outcome(Some(Err(error))) if error.is_panic() => {
                    std::panic::resume_unwind(error.into_panic());
                }
                HedgeEvent::Outcome(Some(Err(_)) | None) => {
                    abort_hedges(&mut handles);
                    let status = Status::internal("hedged attempt ended unexpectedly");
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    self.local.retry_stats.record_committed(false);
                    return Err(status);
                }
                HedgeEvent::Outcome(Some(Ok((_attempt, result)))) => {
                    outstanding = outstanding.saturating_sub(1);
                    match result {
                        Ok(response) => {
                            abort_hedges(&mut handles);
                            let final_result: Result<Response<Resp>, Status> =
                                self.apply_response_hooks(req.path, response);
                            match &final_result {
                                Ok(_) => {
                                    req.call_guard.finish(&Status::ok());
                                    self.note_call_outcome(true).await;
                                }
                                Err(status) => {
                                    req.call_guard.finish(status);
                                    self.note_call_outcome(false).await;
                                }
                            }
                            self.local
                                .retry_stats
                                .record_committed(final_result.is_ok());
                            return final_result;
                        }
                        Err(status) => {
                            let fatal = !req.policy.non_fatal_status_codes.contains(&status.code());
                            if fatal {
                                abort_hedges(&mut handles);
                                req.call_guard.finish(&status);
                                self.note_call_outcome(false).await;
                                self.local.retry_stats.record_committed(false);
                                return Err(status);
                            }
                            schedule.nonfatal(
                                status.retry_pushback(),
                                max.saturating_sub(sent),
                                tokio::time::Instant::now(),
                            );
                            last_err = Some(status);
                        }
                    }
                }
                HedgeEvent::Timer => schedule.timer_elapsed(),
                HedgeEvent::Permission(allowed) => {
                    if allowed {
                        sent += 1;
                        self.local.retry_stats.record_hedged_send();
                        spawn_hedge_attempt(&mut handles, self.clone(), &req, sent);
                        outstanding += 1;
                        schedule.sent(
                            max.saturating_sub(sent),
                            req.policy.hedging_delay,
                            tokio::time::Instant::now(),
                        );
                    } else {
                        self.local.retry_stats.record_throttled();
                        schedule.stop();
                    }
                }
            }
            if outstanding == 0 && (schedule.stopped || sent >= max) {
                let status = last_err
                    .take()
                    .unwrap_or_else(|| Status::unknown("hedging ended without an outcome"));
                req.call_guard.finish(&status);
                self.note_call_outcome(false).await;
                self.local.retry_stats.record_committed(false);
                return Err(status);
            }
        }
    }
}

#[cfg(test)]
mod hedge_schedule_tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "hedge test assertions")]

    use super::{HedgeEvent, HedgeReadiness, HedgeSchedule, next_hedge_event};
    use crate::rpc_cancel::RpcCancel;
    use crate::status::Pushback;
    use std::time::Duration;
    use tokio::sync::{Mutex, mpsc};

    #[test]
    fn queued_nonfatal_responses_keep_separate_sends() {
        let now = tokio::time::Instant::now();
        let mut schedule = HedgeSchedule::new(5, Some(Duration::from_secs(10)), now);
        schedule.nonfatal(None, 2, now);
        schedule.nonfatal(None, 2, now);
        schedule.nonfatal(None, 2, now);
        assert_eq!(schedule.pending, 2);
        assert_eq!(schedule.at, None);
        schedule.sent(1, Some(Duration::from_secs(10)), now);
        assert_eq!(schedule.pending, 1);
        assert_eq!(schedule.at, None);
        schedule.sent(0, Some(Duration::from_secs(10)), now);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, None);
    }

    #[test]
    fn delay_replaces_pending_and_refusal_stays_sticky() {
        let now = tokio::time::Instant::now();
        let mut schedule = HedgeSchedule::new(5, Some(Duration::from_secs(10)), now);
        schedule.nonfatal(None, 3, now);
        schedule.nonfatal(None, 3, now);
        assert_eq!(schedule.pending, 2);
        schedule.nonfatal(Some(Pushback::Delay(Duration::from_secs(3))), 3, now);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, Some(now + Duration::from_secs(3)));
        schedule.nonfatal(Some(Pushback::DoNotRetry), 3, now);
        assert!(schedule.stopped);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, None);
        schedule.nonfatal(None, 3, now);
        assert!(schedule.stopped);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, None);
    }

    #[test]
    fn delayed_pushback_replaces_pending_then_restores_normal_delay() {
        let now = tokio::time::Instant::now();
        let mut schedule = HedgeSchedule::new(5, Some(Duration::from_secs(10)), now);
        schedule.nonfatal(None, 3, now);
        schedule.nonfatal(None, 3, now);
        assert_eq!(schedule.pending, 2);
        schedule.nonfatal(Some(Pushback::Delay(Duration::from_secs(3))), 3, now);
        assert_eq!(schedule.pending, 0, "pushback must replace pending sends");
        assert_eq!(schedule.at, Some(now + Duration::from_secs(3)));
        schedule.timer_elapsed();
        assert_eq!(schedule.pending, 1);
        schedule.sent(
            2,
            Some(Duration::from_secs(10)),
            now + Duration::from_secs(3),
        );
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, Some(now + Duration::from_secs(13)));
    }

    #[test]
    fn zero_pushback_and_unset_delay_resume_immediately() {
        let now = tokio::time::Instant::now();
        let mut schedule = HedgeSchedule::new(5, None, now);
        assert_eq!(schedule.pending, 4);
        schedule.nonfatal(Some(Pushback::Delay(Duration::ZERO)), 4, now);
        assert_eq!(schedule.pending, 1);
        schedule.sent(3, None, now);
        assert_eq!(schedule.pending, 3);
        assert_eq!(schedule.at, None);
    }

    #[tokio::test(start_paused = true)]
    async fn held_throttle_does_not_hide_a_completed_outcome() {
        let gate = Mutex::new(());
        let held = gate.lock().await;
        let (tx, mut rx) = mpsc::channel::<u8>(2);
        let permit = async {
            let _held = gate.lock().await;
            true
        };
        let cancel = RpcCancel::new();
        let wait = next_hedge_event(
            cancel,
            None,
            None,
            rx.recv(),
            HedgeReadiness {
                attempts: true,
                pending: true,
                timer: false,
            },
            permit,
        );
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut wait)
                .await
                .is_err()
        );
        tx.send(7).await.expect("queue outcome");
        let event = tokio::time::timeout(Duration::from_secs(1), &mut wait)
            .await
            .expect("outcome must interrupt the held throttle");
        assert!(matches!(event, HedgeEvent::Outcome(Some(7))));
        drop(held);
    }

    #[tokio::test]
    async fn queued_outcome_precedes_a_ready_throttle() {
        let gate = Mutex::new(());
        let held = gate.lock().await;
        let (tx, mut rx) = mpsc::channel::<u8>(2);
        tx.send(7).await.expect("queue outcome");
        drop(held);
        let permit = async {
            let _held = gate.lock().await;
            true
        };
        let event = next_hedge_event(
            RpcCancel::new(),
            None,
            None,
            rx.recv(),
            HedgeReadiness {
                attempts: true,
                pending: true,
                timer: false,
            },
            permit,
        )
        .await;
        assert!(
            matches!(event, HedgeEvent::Outcome(Some(7))),
            "queued outcome must precede ready permission"
        );
    }

    #[tokio::test]
    async fn held_throttle_processes_refusal_before_dispatch() {
        let now = tokio::time::Instant::now();
        let gate = Mutex::new(());
        let held = gate.lock().await;
        let (tx, mut rx) = mpsc::channel(2);
        let mut schedule = HedgeSchedule::new(3, Some(Duration::from_secs(10)), now);
        schedule.nonfatal(None, 1, now);
        assert_eq!(schedule.pending, 1);
        tx.send(Pushback::DoNotRetry).await.expect("queue refusal");
        let permit = async {
            let _held = gate.lock().await;
            true
        };
        let event = next_hedge_event(
            RpcCancel::new(),
            None,
            None,
            rx.recv(),
            HedgeReadiness {
                attempts: true,
                pending: true,
                timer: false,
            },
            permit,
        )
        .await;
        let HedgeEvent::Outcome(Some(pushback)) = event else {
            panic!("refusal must precede a blocked send");
        };
        schedule.nonfatal(Some(pushback), 1, now);
        assert!(schedule.stopped);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, None);
        drop(held);
    }

    #[tokio::test]
    async fn held_throttle_processes_delay_before_dispatch() {
        let now = tokio::time::Instant::now();
        let gate = Mutex::new(());
        let held = gate.lock().await;
        let (tx, mut rx) = mpsc::channel(2);
        let mut schedule = HedgeSchedule::new(4, Some(Duration::from_secs(10)), now);
        schedule.nonfatal(None, 2, now);
        schedule.nonfatal(None, 2, now);
        assert_eq!(schedule.pending, 2);
        tx.send(Pushback::Delay(Duration::from_secs(3)))
            .await
            .expect("queue delay");
        let permit = async {
            let _held = gate.lock().await;
            true
        };
        let event = next_hedge_event(
            RpcCancel::new(),
            None,
            None,
            rx.recv(),
            HedgeReadiness {
                attempts: true,
                pending: true,
                timer: false,
            },
            permit,
        )
        .await;
        let HedgeEvent::Outcome(Some(pushback)) = event else {
            panic!("delay must precede a blocked send");
        };
        schedule.nonfatal(Some(pushback), 2, now);
        assert_eq!(schedule.pending, 0);
        assert_eq!(schedule.at, Some(now + Duration::from_secs(3)));
        drop(held);
    }

    #[tokio::test]
    async fn held_throttle_keeps_multiple_nonfatal_sends_separate() {
        let now = tokio::time::Instant::now();
        let gate = Mutex::new(());
        let held = gate.lock().await;
        let (tx, mut rx) = mpsc::channel::<()>(2);
        let mut schedule = HedgeSchedule::new(5, Some(Duration::from_secs(10)), now);
        tx.send(()).await.expect("first outcome");
        tx.send(()).await.expect("second outcome");
        for _ in 0..2 {
            let permit = async {
                let _held = gate.lock().await;
                true
            };
            let event = next_hedge_event(
                RpcCancel::new(),
                None,
                None,
                rx.recv(),
                HedgeReadiness {
                    attempts: true,
                    pending: true,
                    timer: false,
                },
                permit,
            )
            .await;
            assert!(matches!(event, HedgeEvent::Outcome(Some(()))));
            schedule.nonfatal(None, 2, now);
        }
        assert_eq!(schedule.pending, 2);
        drop(held);
        for remaining in [1, 0] {
            let permit = async {
                let _held = gate.lock().await;
                true
            };
            let event = next_hedge_event(
                RpcCancel::new(),
                None,
                None,
                rx.recv(),
                HedgeReadiness {
                    attempts: true,
                    pending: true,
                    timer: false,
                },
                permit,
            )
            .await;
            assert!(matches!(event, HedgeEvent::Permission(true)));
            schedule.sent(remaining, Some(Duration::from_secs(10)), now);
        }
        assert_eq!(schedule.pending, 0);
    }

    #[tokio::test]
    async fn cancellation_and_deadline_precede_a_held_throttle() {
        for cancelled in [true, false] {
            let gate = Mutex::new(());
            let held = gate.lock().await;
            let (_tx, mut rx) = mpsc::channel::<u8>(1);
            let cancel = RpcCancel::new();
            if cancelled {
                cancel.signal();
            }
            let deadline = (!cancelled).then(tokio::time::Instant::now);
            let permit = async {
                let _held = gate.lock().await;
                true
            };
            let event = next_hedge_event(
                cancel,
                deadline,
                None,
                rx.recv(),
                HedgeReadiness {
                    attempts: true,
                    pending: true,
                    timer: false,
                },
                permit,
            )
            .await;
            assert!(if cancelled {
                matches!(event, HedgeEvent::Cancelled)
            } else {
                matches!(event, HedgeEvent::Deadline)
            });
            drop(held);
        }
    }
}

#[cfg(test)]
mod hedge_stream_accounting_tests {
    use super::HedgeStreamAccounting;
    use crate::channelz::{Registry, SocketParent, SocketSecurity};

    #[test]
    fn failed_stream_then_success_counts_each_stream_once() {
        let registry = Registry::global_shared();
        let channel = registry.register_channel("hedge accounting test", "hedge accounting");
        let socket = registry.register_socket(
            SocketParent::Channel(channel.id()),
            None,
            None,
            None,
            SocketSecurity::None,
            false,
        );
        let entry = registry.socket(socket.id()).expect("registered socket");
        {
            let mut first = HedgeStreamAccounting::start(Some(socket.id()));
            first.finish(false);
        }
        {
            let mut second = HedgeStreamAccounting::start(Some(socket.id()));
            second.finish(true);
        }
        assert_eq!(entry.streams_started(), 2);
        assert_eq!(entry.streams_failed(), 1);
        assert_eq!(entry.streams_succeeded(), 1);
        assert_eq!(entry.messages_sent(), 1);
        assert_eq!(entry.messages_received(), 1);
    }
}

#[cfg(test)]
mod hedge_panic_priority_tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "hedge priority tests")]

    use super::{HedgeEvent, HedgeReadiness, next_hedge_event};
    use crate::rpc_cancel::RpcCancel;
    use std::time::Duration;
    use tokio::task::JoinSet;

    async fn panicked_task() -> JoinSet<()> {
        let mut tasks = JoinSet::new();
        let handle = tasks.spawn(async { panic!("completed hedge panic") });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !handle.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("child task completed");
        tasks
    }

    fn ready() -> HedgeReadiness {
        HedgeReadiness {
            attempts: true,
            pending: true,
            timer: false,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ready_cancel_deadline_and_panic_follow_priority() {
        let mut tasks = panicked_task().await;
        let cancel = RpcCancel::new();
        cancel.signal();
        let event = next_hedge_event(
            cancel,
            None,
            None,
            tasks.join_next(),
            ready(),
            std::future::ready(true),
        )
        .await;
        assert!(
            matches!(event, HedgeEvent::Cancelled),
            "ready cancellation must beat child panic"
        );

        let mut tasks = panicked_task().await;
        let event = next_hedge_event(
            RpcCancel::new(),
            Some(tokio::time::Instant::now() - Duration::from_secs(1)),
            None,
            tasks.join_next(),
            ready(),
            std::future::ready(true),
        )
        .await;
        assert!(
            matches!(event, HedgeEvent::Deadline),
            "ready deadline must beat child panic"
        );

        let mut tasks = panicked_task().await;
        let event = next_hedge_event(
            RpcCancel::new(),
            None,
            None,
            tasks.join_next(),
            ready(),
            std::future::ready(true),
        )
        .await;
        assert!(
            matches!(event, HedgeEvent::Outcome(Some(Err(error))) if error.is_panic()),
            "ready child panic must beat throttle permission"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn selected_success_aborts_a_later_panicking_attempt() {
        let mut tasks = JoinSet::<u8>::new();
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let pending = tasks.spawn(async move {
            let _released = wait.await.is_ok();
            panic!("the later attempt must be aborted");
        });
        let winner = tasks.spawn(async { 7_u8 });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !winner.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("winner finished");
        let event = next_hedge_event(
            RpcCancel::new(),
            None,
            None,
            tasks.join_next(),
            HedgeReadiness {
                attempts: true,
                pending: false,
                timer: false,
            },
            std::future::ready(true),
        )
        .await;
        assert!(matches!(event, HedgeEvent::Outcome(Some(Ok(7)))));
        drop(tasks);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !pending.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("losing attempt aborted");
        assert!(release.send(()).is_err(), "losing future was dropped");
    }
}
