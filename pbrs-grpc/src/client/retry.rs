//! Policy retry and hedging execution.

use super::Channel;
use super::unary::run_unary;
use crate::config::Wire;
use crate::request::Response;
use crate::service_config::{HedgingPolicy, RetryPolicy, retry_backoff};
use crate::status::Status;
use crate::telemetry::{
    AttemptGuard, AttemptLabels, CallGuard, CancellationReason, LifecycleObserver, OwnedCallLabels,
};
use bytes::Bytes;
use http::HeaderValue;
use pbrs::Parse;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, mpsc, watch};

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
    /// Policy retries actually sent (per-attempt timeouts included).
    pub policy_retries: u64,
    /// Transparent redials on raced connection deaths (never policy-gated).
    pub transparent_retries: u64,
    /// Hedged sends past the first attempt.
    pub hedged_sends: u64,
    /// Retries or hedged sends refused by the throttling bucket.
    pub throttled: u64,
    /// Retries honoring a server pushback delay.
    pub pushback_delays: u64,
    /// Retries refused by a server `DoNotRetry` pushback.
    pub pushback_refusals: u64,
    /// Per-attempt recv timeouts that triggered a retry.
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
    pub(crate) frame: Bytes,
    pub(crate) cancel_rx: watch::Receiver<bool>,
    pub(crate) wire: Wire,
    pub(crate) ua: HeaderValue,
    pub(crate) https: bool,
    pub(crate) policy: HedgingPolicy,
    pub(crate) call_guard: CallGuard,
    pub(crate) owned_labels: Option<OwnedCallLabels>,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) permit: Option<OwnedSemaphorePermit>,
}

/// A finished hedged attempt: its 1-based index plus the outcome.
type HedgeOutcome<Resp> = (u32, Result<Response<Resp>, Status>);

/// Run one hedged attempt: grab, send, transparent-redial once on a raced
/// connection death, then report exactly one outcome.
fn spawn_hedge_attempt<Resp>(
    channel: Channel,
    req: &HedgeUnary,
    attempt: u32,
    tx: mpsc::Sender<HedgeOutcome<Resp>>,
) -> tokio::task::JoinHandle<()>
where
    Resp: Parse + Default + Send + 'static,
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
    let https = req.https;
    let observer = req.observer.clone();
    let owned_labels = req.owned_labels.clone();
    tokio::spawn(async move {
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
            let live = match channel.grab(cancel_rx.clone(), deadline, wait).await {
                Ok(live) => {
                    if let (Some(obs), Some(call)) = (&observer, &owned_labels) {
                        obs.on_queue_wait(&call.as_borrowed(), queue_start.elapsed());
                    }
                    live
                }
                Err(status) => break Err(status),
            };
            let (slot, r#gen) = (live.slot, live.r#gen);
            let byte_permit = match channel.byte_budget.acquire(frame.len()) {
                Ok(permit) => permit,
                Err(status) => break Err(status),
            };
            if let (Some(obs), Some(call)) = (&observer, &owned_labels) {
                obs.on_bytes_sent(&call.as_borrowed(), frame.len());
            }
            match run_unary(
                live.send,
                &channel.authority,
                path,
                &md,
                req_timeout,
                deadline,
                compress,
                frame.clone(),
                cancel_rx.clone(),
                wire,
                ua.clone(),
                https,
                byte_permit,
            )
            .await
            {
                Err(status)
                    if !redialed
                        && status.is_transparent_retryable()
                        && channel.inner.endpoint.can_redial() =>
                {
                    redialed = true;
                    channel.retry_stats.record_transparent_retry();
                    channel.inner.discard(slot, r#gen).await;
                }
                result => {
                    if let Err(status) = &result {
                        if status.is_transport() {
                            channel.inner.discard(slot, r#gen).await;
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
        drop(tx.send((attempt, outcome)).await);
    })
}

/// Abort every outstanding hedged attempt. Aborted attempts report
/// cancellation through their guard's `Drop`, matching a dropped [`Call`].
fn abort_hedges(handles: &[tokio::task::JoinHandle<()>]) {
    for handle in handles {
        handle.abort();
    }
}

/// Sleep until `at`, or forever when there is no deadline.
async fn sleep_until_or_pending(at: Option<tokio::time::Instant>) {
    match at {
        Some(instant) => tokio::time::sleep_until(instant).await,
        None => std::future::pending().await,
    }
}

/// Complete when the call's cancel flag is set.
async fn cancel_fired(cancel_rx: watch::Receiver<bool>) {
    let mut cancel_rx = cancel_rx;
    drop(cancel_rx.wait_for(|flag| *flag).await);
}

impl super::Channel {
    /// Run a unary call under an A6 hedging policy.
    ///
    /// The first attempt goes out immediately and is never throttled. When it
    /// has not finished after `hedgingDelay`, the next attempt goes out, up to
    /// `maxAttempts`; an unset delay fans all attempts out at once. Sends past
    /// the first need a throttling token and stop on a `DoNotRetry` pushback.
    /// The first `OK` wins; a non-OK status outside `nonFatalStatusCodes`
    /// commits immediately; non-fatal statuses keep waiting, and when nothing
    /// is outstanding the next hedge goes out at once instead of waiting for
    /// the timer. When attempts are exhausted the last non-fatal error fails
    /// the call. Every attempt transparent-redials once on a raced connection
    /// death, exactly like [`Self::unary`].
    #[allow(
        clippy::too_many_lines,
        reason = "one select loop over cancel, deadline, hedge timer, and completions"
    )]
    pub(crate) async fn execute_hedged<Resp>(
        &self,
        mut req: HedgeUnary,
    ) -> Result<Response<Resp>, Status>
    where
        Resp: Parse + Default + Send + 'static,
    {
        let _held = req.permit.take();
        let max = req.policy.max_attempts.max(1);
        let bound = usize::try_from(max).unwrap_or(usize::MAX).max(1);
        let (tx, mut rx) = mpsc::channel(bound);
        let mut handles = Vec::new();
        let mut sent = 1u32;
        let mut outstanding = 1u32;
        let mut last_err: Option<Status> = None;
        let mut stop_sending = false;
        let mut hedge_at: Option<tokio::time::Instant> = None;
        handles.push(spawn_hedge_attempt(self.clone(), &req, 1, tx.clone()));
        match req.policy.hedging_delay {
            Some(delay) => {
                hedge_at = Some(tokio::time::Instant::now() + delay);
            }
            None => {
                while sent < max {
                    if !self.retry_allowed().await {
                        self.retry_stats.record_throttled();
                        stop_sending = true;
                        break;
                    }
                    sent += 1;
                    self.retry_stats.record_hedged_send();
                    handles.push(spawn_hedge_attempt(self.clone(), &req, sent, tx.clone()));
                    outstanding += 1;
                }
            }
        }
        loop {
            tokio::select! {
                biased;
                () = cancel_fired(req.cancel_rx.clone()) => {
                    abort_hedges(&handles);
                    req.call_guard.cancel(CancellationReason::CallerCancelled);
                    let status = Status::cancelled();
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    self.retry_stats.record_committed(false);
                    return Err(status);
                }
                () = sleep_until_or_pending(req.deadline) => {
                    abort_hedges(&handles);
                    req.call_guard.cancel(CancellationReason::DeadlineExceeded);
                    let status = Status::deadline_exceeded();
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    self.retry_stats.record_committed(false);
                    return Err(status);
                }
                () = sleep_until_or_pending(hedge_at),
                    if hedge_at.is_some() && sent < max && !stop_sending =>
                {
                    hedge_at = None;
                    if self.retry_allowed().await {
                        sent += 1;
                        self.retry_stats.record_hedged_send();
                        handles.push(spawn_hedge_attempt(self.clone(), &req, sent, tx.clone()));
                        outstanding += 1;
                        if let Some(delay) = req.policy.hedging_delay {
                            hedge_at = Some(tokio::time::Instant::now() + delay);
                        }
                    } else {
                        self.retry_stats.record_throttled();
                        stop_sending = true;
                    }
                }
                outcome = rx.recv() => {
                    let Some((_attempt, result)) = outcome else {
                        // Defensive: every task sends exactly one outcome, so
                        // the channel only closes after a commit aborts the
                        // tasks. Never return without an outcome.
                        abort_hedges(&handles);
                        let status = last_err.take().unwrap_or_else(|| {
                            Status::unknown("hedging ended without an outcome")
                        });
                        req.call_guard.finish(&status);
                        self.note_call_outcome(false).await;
                        self.retry_stats.record_committed(false);
                        return Err(status);
                    };
                    outstanding = outstanding.saturating_sub(1);
                    match result {
                        Ok(response) => {
                            abort_hedges(&handles);
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
                            self.retry_stats.record_committed(final_result.is_ok());
                            return final_result;
                        }
                        Err(status) => {
                            if matches!(
                                status.retry_pushback(),
                                Some(crate::status::Pushback::DoNotRetry)
                            ) {
                                stop_sending = true;
                            }
                            let fatal = !req
                                .policy
                                .non_fatal_status_codes
                                .contains(&status.code());
                            if fatal {
                                abort_hedges(&handles);
                                req.call_guard.finish(&status);
                                self.note_call_outcome(false).await;
                                self.retry_stats.record_committed(false);
                                return Err(status);
                            }
                            last_err = Some(status);
                            if outstanding == 0 {
                                if !stop_sending && sent < max && self.retry_allowed().await {
                                    sent += 1;
                                    self.retry_stats.record_hedged_send();
                                    handles.push(spawn_hedge_attempt(
                                        self.clone(),
                                        &req,
                                        sent,
                                        tx.clone(),
                                    ));
                                    outstanding += 1;
                                    if let Some(delay) = req.policy.hedging_delay {
                                        hedge_at = Some(tokio::time::Instant::now() + delay);
                                    }
                                } else {
                                    if !stop_sending && sent < max {
                                        self.retry_stats.record_throttled();
                                    }
                                    if let Some(status) = last_err.take() {
                                        req.call_guard.finish(&status);
                                        self.note_call_outcome(false).await;
                                        self.retry_stats.record_committed(false);
                                        return Err(status);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
