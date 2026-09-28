//! Unary call shape.

use super::call::{
    AttemptCommitment, first_of, open, prefer_peer_rejection_after_send, race, send_request_frame,
};
use super::retry::{HedgeUnary, PolicyDecision, policy_retry_delay, retry_exhausted};
use crate::binlog::CallLogger;
use crate::config::Wire;
use crate::limits::BytePermit;
use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::telemetry::{
    AttemptGuard, AttemptLabels, CallGuard, CallLabels, CallRole, CancellationReason,
    LifecycleObserver, RejectionReason,
};
use crate::timeout::{deadline_from, remaining_timeout};
use crate::transport::h2 as backend;
use crate::wire::{SegFrame, encode_msg, finish_unary};
use http::HeaderValue;
use http::uri::Authority;
use pbrs::{Parse, Serialize};
use std::time::Duration;
use tokio::sync::watch;

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, cancel, limits, and scheme"
)]
pub(crate) async fn run_unary<Resp>(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: SegFrame,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
    tap: Option<&CallLogger>,
) -> Result<Response<Resp>, Status>
where
    Resp: Parse + Default,
{
    let outcome = run_unary_inner(
        send_req, authority, path, md, timeout, deadline, compress, frame, cancel_rx, wire,
        user_agent, https, permit, tap,
    )
    .await;
    if let (Some(tap), Err(status)) = (tap, &outcome) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    outcome
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, cancel, limits, scheme, and tap"
)]
async fn run_unary_inner<Resp>(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: SegFrame,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
    tap: Option<&CallLogger>,
) -> Result<Response<Resp>, Status>
where
    Resp: Parse + Default,
{
    let mut commitment = AttemptCommitment::Uncommitted;
    if let Some(tap) = tap {
        tap.log_client_header(md, path, authority.as_str(), timeout);
    }
    let (resp_fut, mut send_stream) = open(
        send_req,
        authority,
        path,
        md,
        timeout,
        deadline,
        cancel_rx.clone(),
        compress.then_some(wire.send_codec),
        wire.accept_gzip,
        &user_agent,
        https,
    )
    .await
    .map_err(|e| commitment.classify(e))?;
    commitment = AttemptCommitment::BodyStarted;
    let log_frame = tap.is_some().then(|| frame.clone());
    let sent = send_request_frame(
        &mut send_stream,
        frame,
        wire.send_buffer,
        cancel_rx.clone(),
        deadline,
    )
    .await;
    drop(permit);
    if let Err(status) = sent {
        if matches!(status.code(), Code::Cancelled | Code::DeadlineExceeded) {
            return Err(status);
        }
        return race(
            prefer_peer_rejection_after_send(resp_fut, commitment.classify(status)),
            cancel_rx,
            deadline,
            Some(&mut send_stream),
        )
        .await;
    }
    if let (Some(tap), Some(log_frame)) = (tap, &log_frame) {
        // Unary sends with end-of-stream set: message and half-close together.
        for seg in log_frame.segments() {
            tap.log_written(seg);
        }
        tap.log_half_close();
    }
    race(
        async {
            let response = resp_fut.await.map_err(|e| commitment.classify_h2(e))?;
            commitment = AttemptCommitment::ResponseCommitted;
            finish_unary::<Resp>(response, wire.limits, wire.accept_gzip, tap)
                .await
                .map_err(|e| commitment.classify(e))
        },
        cancel_rx,
        deadline,
        Some(&mut send_stream),
    )
    .await
}

impl super::Channel {
    /// Issue a unary RPC: one request message, one response message.
    ///
    /// `path` is the full gRPC path, `/<package>.<Service>/<Method>`.
    /// A hand-written [`crate::Service`] is first-class on this path;
    /// generated clients call this for you.
    ///
    /// Dropping the [`Call`] without awaiting resets the stream. A
    /// [`crate::CallHandle`] taken before await still cancels it.
    /// OK-path custom trailers land on [`crate::Response::trailers`]; a `-bin`
    /// trailer must not appear as a header, including over TLS, mTLS, Unix,
    /// and [`Self::from_io`].
    /// [`Self::max_encoding_message_size`] / [`Self::max_decoding_message_size`]
    /// fail this path as [`Code::ResourceExhausted`], including over TLS, mTLS,
    /// Unix, and [`Self::from_io`]. Distinct from generated client wrappers.
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let mut req = HelloRequest::new();
    /// req.set_name("world");
    /// let reply: HelloReply = channel
    ///     .unary("/helloworld.Greeter/SayHello", Request::new(req))
    ///     .await?
    ///     .into_inner();
    /// # let _ = reply;
    /// # Ok(())
    /// # }
    /// ```
    pub fn unary<Req, Resp>(&self, path: &'static str, req: Request<Req>) -> Call<Response<Resp>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let mut req = req;
        let prepared = self.prepare_outbound(path, &mut req);
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let wire = self.wire_for(path);
        let observer = self.observer.clone();
        Call::new(
            cancel,
            Box::pin(async move {
                let call_labels =
                    CallLabels::new(path, Some(channel.authority.as_str()), CallRole::Client);
                let call_start = std::time::Instant::now();
                if let Some(obs) = &observer {
                    obs.on_call_start(&call_labels);
                }
                let owned_labels = observer.as_ref().map(|_| call_labels.to_owned());
                let mut call_guard = CallGuard::new(
                    observer.clone(),
                    owned_labels.clone(),
                    call_start,
                    channel.inner.channelz.id(),
                );

                if let Err(status) = prepared {
                    call_guard.reject(RejectionReason::ClientInterceptor, &status);
                    return Err(status);
                }
                let wait = req.wait_for_ready();
                let deadline = deadline_from(req.timeout());
                let _ = match remaining_timeout(deadline) {
                    Ok(t) => t,
                    Err(status) => {
                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                        return Err(status);
                    }
                };
                let req_timeout = req.timeout();
                let (msg, md, _, compress, ua) = req.into_parts();
                // Encode before opening so an oversize message never occupies a
                // stream slot, and a transparent retry does not re-serialize.
                let frame = match encode_msg(
                    &msg,
                    compress.then_some(wire.send_codec),
                    wire.limits,
                    wire.gzip_level,
                ) {
                    Ok(f) => f,
                    Err(status) => {
                        call_guard.reject(RejectionReason::MessageEncode, &status);
                        return Err(status);
                    }
                };
                let https = channel.https;
                let ua = ua.unwrap_or_else(|| channel.user_agent.clone());
                let _permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                channel.retry_stats.record_call();
                let tap = channel
                    .binlog
                    .as_ref()
                    .and_then(|binlog| binlog.start_call(path, crate::binlog::Logger::Client));
                let (retry_policy, hedging_policy) = channel
                    .method_config_for(path)
                    .map(|method| (method.retry_policy, method.hedging_policy))
                    .unwrap_or((None, None));
                if let Some(policy) = hedging_policy {
                    return channel
                        .execute_hedged(HedgeUnary {
                            path,
                            md,
                            req_timeout,
                            deadline,
                            wait,
                            compress,
                            frame,
                            cancel_rx,
                            wire,
                            ua,
                            https,
                            policy,
                            call_guard,
                            owned_labels,
                            observer,
                            permit: _permit,
                            binlog: tap,
                        })
                        .await;
                }
                let mut attempt_idx = 1u32;
                let mut policy_attempts = 1u32;
                let mut retried = false;
                loop {
                    let _ = match remaining_timeout(deadline) {
                        Ok(t) => t,
                        Err(status) => {
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                            channel.retry_stats.record_committed(false);
                            return Err(status);
                        }
                    };
                    let attempt_labels = AttemptLabels::new(call_labels, attempt_idx);
                    let attempt_start = std::time::Instant::now();
                    if let Some(obs) = &observer {
                        obs.on_attempt_start(&attempt_labels);
                    }
                    let mut attempt_guard = AttemptGuard::new(
                        observer.clone(),
                        owned_labels.clone(),
                        attempt_idx,
                        attempt_start,
                    );
                    let queue_start = tokio::time::Instant::now();
                    let live = match channel
                        .grab(cancel_rx.clone(), deadline, wait, Some(&md))
                        .await
                    {
                        Ok(live) => {
                            if let Some(obs) = &observer {
                                obs.on_queue_wait(&call_labels, queue_start.elapsed());
                            }
                            live
                        }
                        Err(status) => {
                            if *cancel_rx.borrow() {
                                attempt_guard.cancel(CancellationReason::CallerCancelled);
                                call_guard.cancel(CancellationReason::CallerCancelled);
                            } else if status.code() == Code::DeadlineExceeded {
                                attempt_guard.cancel(CancellationReason::DeadlineExceeded);
                                call_guard.cancel(CancellationReason::DeadlineExceeded);
                            } else {
                                attempt_guard.reject(RejectionReason::SetupFailed, &status);
                                call_guard.reject(RejectionReason::SetupFailed, &status);
                            }
                            if policy_attempts > 1 {
                                channel.note_call_outcome(false).await;
                            }
                            channel.retry_stats.record_committed(false);
                            attempt_guard.finish(&status);
                            return Err(status);
                        }
                    };
                    let (slot, r#gen, rr_addr) = (live.slot, live.r#gen, live.rr_addr);
                    let live_socket = live.channelz_socket;
                    // A48 least-request: RAII in-flight count for this
                    // attempt; drops (releasing) on every exit path.
                    let _lr =
                        super::pool::track_least_request(&channel.inner.endpoint, rr_addr.as_ref())
                            .await;
                    let byte_permit = match channel.byte_budget.acquire(frame.total_len()) {
                        Ok(p) => p,
                        Err(status) => {
                            attempt_guard.reject(RejectionReason::ByteBudgetExceeded, &status);
                            call_guard.reject(RejectionReason::ByteBudgetExceeded, &status);
                            if policy_attempts > 1 {
                                channel.note_call_outcome(false).await;
                            }
                            channel.retry_stats.record_committed(false);
                            attempt_guard.finish(&status);
                            return Err(status);
                        }
                    };
                    if let Some(obs) = &observer {
                        obs.on_bytes_sent(&call_labels, frame.total_len());
                    }
                    let attempt_deadline = retry_policy
                        .as_ref()
                        .and_then(|policy| policy.per_attempt_recv_timeout)
                        .map(|budget| {
                            let capped = tokio::time::Instant::now() + budget;
                            deadline.map_or(capped, |overall| overall.min(capped))
                        })
                        .or(deadline);
                    // Channelz: the attempt's stream starts here (past
                    // setup rejects, so every start pairs with an end
                    // below or a transparent retry).
                    if let Some(socket) = live_socket {
                        crate::channelz::Registry::global().note_stream_started(socket, true);
                    }
                    match run_unary(
                        live.send,
                        &channel.authority,
                        path,
                        &md,
                        req_timeout,
                        attempt_deadline,
                        compress,
                        frame.clone(),
                        cancel_rx.clone(),
                        wire,
                        ua.clone(),
                        https,
                        byte_permit,
                        tap.as_ref(),
                    )
                    .await
                    {
                        Err(status)
                            if !retried
                                && status.is_transparent_retryable()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            retried = true;
                            channel.retry_stats.record_transparent_retry();
                            attempt_guard.finish(&status);
                            if let Some(socket) = live_socket {
                                crate::channelz::Registry::global().note_stream_end(socket, false);
                            }
                            channel
                                .inner
                                .discard_conn(slot, r#gen, rr_addr.as_ref())
                                .await;
                            attempt_idx += 1;
                        }
                        result => {
                            // Channelz: the attempt's stream ends here. A
                            // completed unary claims one message each way;
                            // failed attempts claim none (the write may
                            // never have happened).
                            if let Some(socket) = live_socket {
                                let global = crate::channelz::Registry::global();
                                global.note_stream_end(socket, result.is_ok());
                                if result.is_ok() {
                                    global.note_messages(socket, true, 1);
                                    global.note_messages(socket, false, 1);
                                }
                            }
                            // A58 per-call ORCA: every attempt's trailers
                            // feed weights, including failed attempts.
                            let trailers = match &result {
                                Ok(response) => response.trailers(),
                                Err(status) => status.metadata(),
                            };
                            super::pool::ingest_orca_report(
                                &channel.inner.endpoint,
                                rr_addr.as_ref(),
                                trailers,
                            )
                            .await;
                            // A50 outlier detection: every attempt's
                            // outcome feeds the detectors.
                            match &result {
                                Ok(_) => super::pool::ingest_call_status(
                                    &channel.inner.endpoint,
                                    rr_addr.as_ref(),
                                    &Status::ok(),
                                ),
                                Err(status) => super::pool::ingest_call_status(
                                    &channel.inner.endpoint,
                                    rr_addr.as_ref(),
                                    status,
                                ),
                            }
                            if let Err(status) = &result {
                                let cancelled = *cancel_rx.borrow();
                                let per_attempt_timeout = status.code() == Code::DeadlineExceeded
                                    && retry_policy.as_ref().is_some_and(|policy| {
                                        policy.per_attempt_recv_timeout.is_some()
                                    })
                                    && remaining_timeout(deadline).is_ok()
                                    && !cancelled;
                                match policy_retry_delay(
                                    &channel,
                                    retry_policy.as_ref(),
                                    status,
                                    policy_attempts,
                                    per_attempt_timeout,
                                    cancelled,
                                )
                                .await
                                {
                                    PolicyDecision::Proceed {
                                        delay,
                                        via_pushback,
                                    } => {
                                        channel.retry_stats.record_policy_retry();
                                        if via_pushback {
                                            channel.retry_stats.record_pushback_delay();
                                        }
                                        if per_attempt_timeout {
                                            channel.retry_stats.record_per_attempt_timeout();
                                        }
                                        if status.is_transport() {
                                            channel
                                                .inner
                                                .discard_conn(slot, r#gen, rr_addr.as_ref())
                                                .await;
                                        }
                                        attempt_guard.finish(status);
                                        let slept = first_of(
                                            async {
                                                tokio::time::sleep(delay).await;
                                                Ok::<(), Status>(())
                                            },
                                            cancel_rx.clone(),
                                            deadline,
                                        )
                                        .await;
                                        if let Err(sleep_status) = slept {
                                            if *cancel_rx.borrow() {
                                                call_guard
                                                    .cancel(CancellationReason::CallerCancelled);
                                            } else {
                                                call_guard
                                                    .cancel(CancellationReason::DeadlineExceeded);
                                            }
                                            channel.note_call_outcome(false).await;
                                            channel.retry_stats.record_committed(false);
                                            call_guard.finish(&sleep_status);
                                            return Err(sleep_status);
                                        }
                                        policy_attempts += 1;
                                        attempt_idx += 1;
                                        continue;
                                    }
                                    PolicyDecision::Throttled => {
                                        channel.retry_stats.record_throttled();
                                    }
                                    PolicyDecision::PushbackRefused => {
                                        channel.retry_stats.record_pushback_refusal();
                                    }
                                    PolicyDecision::Declined => {
                                        if retry_exhausted(
                                            retry_policy.as_ref(),
                                            status,
                                            policy_attempts,
                                            per_attempt_timeout,
                                            cancelled,
                                        ) {
                                            channel.retry_stats.record_exhausted();
                                        }
                                    }
                                }
                            }
                            if let Err(status) = &result {
                                if status.is_transport() {
                                    channel
                                        .inner
                                        .discard_conn(slot, r#gen, rr_addr.as_ref())
                                        .await;
                                }
                            }
                            let final_result: Result<Response<Resp>, Status> = result
                                .and_then(|response| channel.apply_response_hooks(path, response));
                            match &final_result {
                                Ok(_) => {
                                    if let Some(obs) = &observer {
                                        obs.on_bytes_received(&call_labels, 0);
                                    }
                                    channel.note_call_outcome(true).await;
                                    attempt_guard.finish(&Status::ok());
                                    call_guard.finish(&Status::ok());
                                }
                                Err(status) => {
                                    if *cancel_rx.borrow() {
                                        attempt_guard.cancel(CancellationReason::CallerCancelled);
                                        call_guard.cancel(CancellationReason::CallerCancelled);
                                    } else if status.code() == Code::DeadlineExceeded {
                                        attempt_guard.cancel(CancellationReason::DeadlineExceeded);
                                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                                    }
                                    channel.note_call_outcome(false).await;
                                    attempt_guard.finish(status);
                                    call_guard.finish(status);
                                }
                            }
                            channel.retry_stats.record_committed(final_result.is_ok());
                            return final_result;
                        }
                    }
                }
            }),
        )
    }
}
