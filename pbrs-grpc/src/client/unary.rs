//! Unary call shape.

use super::call::{
    AttemptCommitment, first_of, open, prefer_peer_rejection_after_send, race, send_request_frame,
};
use super::retry::{HedgeUnary, policy_retry_delay};
use crate::config::Wire;
use crate::limits::BytePermit;
use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::telemetry::{
    AttemptGuard, AttemptLabels, CallGuard, CallLabels, CallRole, CancellationReason,
    LifecycleObserver, RejectionReason,
};
use crate::timeout::{deadline_from, remaining_timeout};
use crate::wire::{encode_msg, finish_unary};
use bytes::Bytes;
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
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: Bytes,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
) -> Result<Response<Resp>, Status>
where
    Resp: Parse + Default,
{
    let mut commitment = AttemptCommitment::Uncommitted;
    let (resp_fut, mut send_stream) = open(
        send_req,
        authority,
        path,
        md,
        timeout,
        deadline,
        cancel_rx.clone(),
        compress,
        wire.accept_gzip,
        &user_agent,
        https,
    )
    .await
    .map_err(|e| commitment.classify(e))?;
    commitment = AttemptCommitment::BodyStarted;
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
    race(
        async {
            let response = resp_fut.await.map_err(|e| commitment.classify_h2(e))?;
            commitment = AttemptCommitment::ResponseCommitted;
            finish_unary::<Resp>(response, wire.limits, wire.accept_gzip)
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
                let mut call_guard =
                    CallGuard::new(observer.clone(), owned_labels.clone(), call_start);

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
                let frame = match encode_msg(&msg, compress, wire.limits, wire.gzip_level) {
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
                let (retry_policy, hedging_policy) = channel
                    .method_config_for(path)
                    .map(|method| (method.retry_policy.clone(), method.hedging_policy.clone()))
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
                    let live = match channel.grab(cancel_rx.clone(), deadline, wait).await {
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
                            attempt_guard.finish(&status);
                            return Err(status);
                        }
                    };
                    let (slot, r#gen) = (live.slot, live.r#gen);
                    let byte_permit = match channel.byte_budget.acquire(frame.len()) {
                        Ok(p) => p,
                        Err(status) => {
                            attempt_guard.reject(RejectionReason::ByteBudgetExceeded, &status);
                            call_guard.reject(RejectionReason::ByteBudgetExceeded, &status);
                            if policy_attempts > 1 {
                                channel.note_call_outcome(false).await;
                            }
                            attempt_guard.finish(&status);
                            return Err(status);
                        }
                    };
                    if let Some(obs) = &observer {
                        obs.on_bytes_sent(&call_labels, frame.len());
                    }
                    let attempt_deadline = retry_policy
                        .as_ref()
                        .and_then(|policy| policy.per_attempt_recv_timeout)
                        .map(|budget| {
                            let capped = tokio::time::Instant::now() + budget;
                            deadline.map_or(capped, |overall| overall.min(capped))
                        })
                        .or(deadline);
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
                    )
                    .await
                    {
                        Err(status)
                            if !retried
                                && status.is_transparent_retryable()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            retried = true;
                            attempt_guard.finish(&status);
                            channel.inner.discard(slot, r#gen).await;
                            attempt_idx += 1;
                        }
                        result => {
                            if let Err(status) = &result {
                                let cancelled = *cancel_rx.borrow();
                                let per_attempt_timeout = status.code() == Code::DeadlineExceeded
                                    && retry_policy.as_ref().is_some_and(|policy| {
                                        policy.per_attempt_recv_timeout.is_some()
                                    })
                                    && remaining_timeout(deadline).is_ok()
                                    && !cancelled;
                                if let Some(delay) = policy_retry_delay(
                                    &channel,
                                    retry_policy.as_ref(),
                                    status,
                                    policy_attempts,
                                    per_attempt_timeout,
                                    cancelled,
                                )
                                .await
                                {
                                    if status.is_transport() {
                                        channel.inner.discard(slot, r#gen).await;
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
                                            call_guard.cancel(CancellationReason::CallerCancelled);
                                        } else {
                                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                                        }
                                        channel.note_call_outcome(false).await;
                                        call_guard.finish(&sleep_status);
                                        return Err(sleep_status);
                                    }
                                    policy_attempts += 1;
                                    attempt_idx += 1;
                                    continue;
                                }
                            }
                            if let Err(status) = &result {
                                if status.is_transport() {
                                    channel.inner.discard(slot, r#gen).await;
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
                            return final_result;
                        }
                    }
                }
            }),
        )
    }
}
