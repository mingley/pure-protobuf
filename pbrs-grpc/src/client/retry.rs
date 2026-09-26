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
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, mpsc, watch};

/// A6 policy-retry decision: how long to wait before the next attempt.
///
/// Returns `None` when the call must finish with `status`: no policy, attempts
/// exhausted, cancelled, a non-retryable code (a per-attempt timeout counts as
/// retryable on its own), a `DoNotRetry` pushback, or a throttled bucket.
/// Otherwise returns the pushback delay when the server sent one, else the
/// jittered exponential backoff for retry number `attempts_made` (1-based, so
/// the first retry uses index 0).
pub(crate) async fn policy_retry_delay(
    channel: &Channel,
    policy: Option<&RetryPolicy>,
    status: &Status,
    attempts_made: u32,
    per_attempt_timeout: bool,
    cancelled: bool,
) -> Option<Duration> {
    let policy = policy?;
    if cancelled || attempts_made >= policy.max_attempts {
        return None;
    }
    if matches!(
        status.retry_pushback(),
        Some(crate::status::Pushback::DoNotRetry)
    ) {
        return None;
    }
    let retryable = per_attempt_timeout || policy.retryable_status_codes.contains(&status.code());
    if !retryable {
        return None;
    }
    if !channel.retry_allowed().await {
        return None;
    }
    if let Some(crate::status::Pushback::Delay(delay)) = status.retry_pushback() {
        return Some(delay);
    }
    Some(retry_backoff(
        policy.initial_backoff,
        policy.max_backoff,
        policy.backoff_multiplier,
        attempts_made.saturating_sub(1),
    ))
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
                        stop_sending = true;
                        break;
                    }
                    sent += 1;
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
                    return Err(status);
                }
                () = sleep_until_or_pending(req.deadline) => {
                    abort_hedges(&handles);
                    req.call_guard.cancel(CancellationReason::DeadlineExceeded);
                    let status = Status::deadline_exceeded();
                    req.call_guard.finish(&status);
                    self.note_call_outcome(false).await;
                    return Err(status);
                }
                () = sleep_until_or_pending(hedge_at),
                    if hedge_at.is_some() && sent < max && !stop_sending =>
                {
                    hedge_at = None;
                    if self.retry_allowed().await {
                        sent += 1;
                        handles.push(spawn_hedge_attempt(self.clone(), &req, sent, tx.clone()));
                        outstanding += 1;
                        if let Some(delay) = req.policy.hedging_delay {
                            hedge_at = Some(tokio::time::Instant::now() + delay);
                        }
                    } else {
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
                                return Err(status);
                            }
                            last_err = Some(status);
                            if outstanding == 0 {
                                if !stop_sending && sent < max && self.retry_allowed().await {
                                    sent += 1;
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
                                } else if let Some(status) = last_err.take() {
                                    req.call_guard.finish(&status);
                                    self.note_call_outcome(false).await;
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
