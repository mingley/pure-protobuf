//! Streaming call shapes.

use super::call::{
    AttemptCommitment, first_of, open, prefer_deadline, prefer_peer_rejection_after_send, race,
    send_request_frame,
};
use super::pool;
use super::retry::{PolicyDecision, policy_retry_delay, retry_exhausted};
use crate::binlog::CallLogger;
use crate::config::Wire;
use crate::limits::{ByteBudgetTracker, BytePermit};
use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::stream::{StreamSender, Streaming};
use crate::telemetry::{
    AttemptGuard, AttemptLabels, CallGuard, CallLabels, CallRole, CancellationReason,
    LifecycleObserver, RejectionReason,
};
use crate::timeout::{deadline_from, remaining_timeout};
use crate::wire::{OutBatch, PumpEnd, encode_msg, finish_stream, finish_unary, reset_on_cancel};
use bytes::Bytes;
use h2::Reason;
use http::HeaderValue;
use http::uri::Authority;
use pbrs::{Parse, Serialize};
use std::time::Duration;
use tokio::sync::watch;

#[allow(
    clippy::too_many_arguments,
    reason = "thin cancel-logging wrapper over run_server_stream_inner"
)]
pub(crate) async fn run_server_stream<Resp>(
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
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: Parse + Default + Send + 'static,
{
    let outcome = run_server_stream_inner(
        send_req, authority, path, md, timeout, deadline, compress, frame, cancel_rx, wire,
        user_agent, https, permit, tap, socket,
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
    reason = "one transport handle plus request, cancel, limits, buffer, and tap"
)]
async fn run_server_stream_inner<Resp>(
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
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: Parse + Default + Send + 'static,
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
    // Channelz: the single request message went out (failed sends claim
    // none, like unary).
    if let Some(sock) = socket {
        crate::channelz::Registry::global().note_messages(sock, true, 1);
    }
    if let (Some(tap), Some(log_frame)) = (tap, &log_frame) {
        // Server-streaming sends one request with end-of-stream set.
        tap.log_written(log_frame);
        tap.log_half_close();
    }
    let response = race(
        async {
            let response = resp_fut.await.map_err(|e| commitment.classify_h2(e))?;
            commitment = AttemptCommitment::ResponseCommitted;
            finish_stream::<Resp>(
                response,
                wire.limits,
                deadline,
                wire.accept_gzip,
                tap.cloned(),
            )
            .await
            .map_err(|e| commitment.classify(e))
        },
        cancel_rx.clone(),
        deadline,
        Some(&mut send_stream),
    )
    .await;
    // Channelz: count the attempt's stream like unary (started on every
    // post-setup attempt, end on failure now and on success via the bound
    // Streaming's polls/drop). Early `?` exits above never started.
    match response {
        Ok(response) => {
            if let Some(sock) = socket {
                crate::channelz::Registry::global().note_stream_started(sock, true);
            }
            // Half-closed send would otherwise drop here, so
            // RecvStream-last-ref was the only RST after headers and
            // CallHandle was a no-op.
            reset_on_cancel(send_stream, cancel_rx, deadline);
            Ok(response.map(|stream| stream.bind_channelz_socket(socket)))
        }
        Err(status) => {
            if let Some(sock) = socket {
                let global = crate::channelz::Registry::global();
                global.note_stream_started(sock, true);
                global.note_stream_end(sock, false);
            }
            Err(status)
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "response and send halves plus cancel, limits, deadline, and tap"
)]
async fn run_client_stream<Req, Resp>(
    resp_fut: h2::client::ResponseFuture,
    send_stream: h2::SendStream<Bytes>,
    rx: Streaming<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    budget: ByteBudgetTracker,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Resp>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default,
{
    // Keep the send half on this stack and RST it if the Call is dropped
    // mid-wait. Harvesting it from a spawned pump lost the RST: cancel can
    // win the same `select!` as JoinHandle Ready, and RecvStream drop is
    // not a last-ref reset while that task holds SendStream.
    let mut send = ResetSend {
        stream: send_stream,
        live: true,
    };
    let result = {
        let mut failed = false;
        let result = {
            let pump = pump_outbound_budget(
                &mut send.stream,
                rx,
                cancel_rx.clone(),
                wire,
                &budget,
                tap.cloned(),
                socket,
            );
            tokio::pin!(pump);
            let fut = async {
                let response = resp_fut.await.map_err(Status::from_h2_post_dispatch)?;
                finish_unary::<Resp>(response, wire.limits, wire.accept_gzip, tap).await
            };
            tokio::pin!(fut);
            let until_deadline = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::pin!(until_deadline);
            let mut cancelled = cancel_rx;
            let mut half_closed = false;
            loop {
                tokio::select! {
                    biased;
                    () = &mut until_deadline => break Err(Status::deadline_exceeded()),
                    _ = cancelled.wait_for(|v| *v) => break Err(Status::cancelled()),
                    end = &mut pump, if !half_closed => {
                        match end {
                            PumpEnd::Failed(status) => {
                                failed = true;
                                break Err(status);
                            }
                            PumpEnd::HalfClosed | PumpEnd::Reset => half_closed = true,
                        }
                    }
                    result = &mut fut => break result,
                }
            }
        };
        if failed
            || matches!(
                &result,
                Err(s) if s.code() == Code::Cancelled || s.code() == Code::DeadlineExceeded
            )
        {
            send.stream.send_reset(Reason::CANCEL);
        }
        result
    };
    send.live = false;
    let result = prefer_deadline(result, deadline);
    if let (Some(tap), Err(status)) = (tap, &result) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    // Channelz: the attempt's stream ends here, like unary. Sent messages
    // were claimed by the pump as written; the single response claims one
    // received message on success.
    if let Some(sock) = socket {
        let global = crate::channelz::Registry::global();
        global.note_stream_started(sock, true);
        global.note_stream_end(sock, result.is_ok());
        if result.is_ok() {
            global.note_messages(sock, false, 1);
        }
    }
    result
}

async fn pump_outbound_budget<T: Serialize>(
    send: &mut h2::SendStream<Bytes>,
    mut rx: Streaming<T>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    wire: Wire,
    budget: &ByteBudgetTracker,
    tap: Option<CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> PumpEnd {
    let mut batch = OutBatch::new(wire);
    if let Some(tap) = &tap {
        batch.set_tap(tap.clone());
    }
    let mut items = Vec::with_capacity(OutBatch::BURST);
    let mut permits = Vec::with_capacity(OutBatch::BURST);
    let mut watch_cancel = true;
    loop {
        items.clear();
        let taken = tokio::select! {
            cancelled = async {
                cancel_rx.wait_for(|v| *v).await.is_ok()
            }, if watch_cancel => {
                if cancelled {
                    send.send_reset(Reason::CANCEL);
                    return PumpEnd::Reset;
                }
                watch_cancel = false;
                continue;
            }
            taken = rx.recv_many(&mut items, OutBatch::BURST) => taken,
        };
        if taken == 0 {
            if batch.flush(send).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return PumpEnd::Reset;
            }
            permits.clear();
            send.send_data(Bytes::new(), true).ok();
            if let Some(tap) = &tap {
                tap.log_half_close();
            }
            return PumpEnd::HalfClosed;
        }
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 {
            crate::wire::let_producer_catch_up().await;
            rx.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let item = match item {
                Ok(item) => item,
                Err(status) => return PumpEnd::Failed(status),
            };
            let frame_len = 5 + item.message.serialized_len();
            match budget.acquire(frame_len) {
                Ok(permit) => permits.push(permit),
                Err(status) => return PumpEnd::Failed(status),
            }
            if let Err(status) = batch.encode(item) {
                return PumpEnd::Failed(status);
            }
            // Channelz: each encoded message counts as sent (a later
            // flush failure can only overcount a doomed stream).
            if let Some(socket) = channelz_socket {
                crate::channelz::Registry::global().note_messages(socket, true, 1);
            }
            if batch.is_full() {
                if batch.flush(send).await.is_err() {
                    send.send_reset(Reason::INTERNAL_ERROR);
                    return PumpEnd::Reset;
                }
                permits.clear();
            }
        }
        if !batch.is_full() {
            if batch.flush(send).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return PumpEnd::Reset;
            }
            permits.clear();
        }
    }
}

/// `RST_STREAM` a client-streaming send half if the Call is dropped while
/// still waiting for the unary response (including after a clean half-close).
struct ResetSend {
    stream: h2::SendStream<Bytes>,
    live: bool,
}

impl Drop for ResetSend {
    fn drop(&mut self) {
        if self.live {
            self.stream.send_reset(Reason::CANCEL);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "response and send halves plus cancel, limits, deadline, and tap"
)]
async fn run_bidi<Req, Resp>(
    resp_fut: h2::client::ResponseFuture,
    send_stream: h2::SendStream<Bytes>,
    rx: Streaming<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    budget: ByteBudgetTracker,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default + Send + 'static,
{
    // A spawned pump can RST before headers; without this channel the Call
    // would see UNAVAILABLE from h2 instead of StreamSender::fail's status.
    // The Call deadline does not set cancel_rx (`is_cancelled` is not
    // deadline). Watch the same Instant here so a Ready DEADLINE_EXCEEDED
    // Call does not leave SendStream parked on a watch that never fires.
    let (fail_tx, mut fail_rx) = tokio::sync::oneshot::channel();
    let (hold_tx, hold_rx) = tokio::sync::oneshot::channel::<()>();
    let pump_tap = tap.clone();
    drop(tokio::spawn({
        let cancel_rx = cancel_rx.clone();
        async move {
            let mut send = send_stream;
            let tap = pump_tap;
            let end = {
                let pump = pump_outbound_budget(
                    &mut send,
                    rx,
                    cancel_rx.clone(),
                    wire,
                    &budget,
                    tap,
                    socket,
                );
                tokio::pin!(pump);
                let until_deadline = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::pin!(until_deadline);
                tokio::select! {
                    biased;
                    () = &mut until_deadline => None,
                    end = &mut pump => Some(end),
                }
            };
            match end {
                None => send.send_reset(Reason::CANCEL),
                Some(PumpEnd::Failed(status)) => {
                    // Hold RST until the Call takes this status. RST first
                    // and resp_fut surfaces CANCEL as UNAVAILABLE
                    // ("stream no longer needed") on the same poll.
                    fail_tx.send(status).ok();
                    hold_rx.await.ok();
                    send.send_reset(Reason::CANCEL);
                }
                Some(PumpEnd::HalfClosed) => reset_on_cancel(send, cancel_rx, deadline),
                Some(PumpEnd::Reset) => {}
            }
        }
    }));
    let result = {
        let fut = async {
            let response = resp_fut.await.map_err(Status::from_h2_post_dispatch)?;
            finish_stream::<Resp>(
                response,
                wire.limits,
                deadline,
                wire.accept_gzip,
                tap.clone(),
            )
            .await
        };
        tokio::pin!(fut);
        let until_deadline = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(until_deadline);
        let mut cancelled = cancel_rx;
        let mut fail_done = false;
        let mut hold_tx = Some(hold_tx);
        loop {
            tokio::select! {
                biased;
                () = &mut until_deadline => break Err(Status::deadline_exceeded()),
                _ = cancelled.wait_for(|v| *v) => break Err(Status::cancelled()),
                status = &mut fail_rx, if !fail_done => {
                    match status {
                        Ok(status) => {
                            hold_tx.take();
                            break Err(status);
                        }
                        Err(_) => fail_done = true,
                    }
                }
                result = &mut fut => break result,
            }
        }
    };
    let result = prefer_deadline(result, deadline);
    if let (Some(tap), Err(status)) = (&tap, &result) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    // Channelz: count the attempt's stream like unary (started on every
    // post-setup attempt, end on failure now and on success via the bound
    // Streaming's polls/drop). Sent messages were claimed by the pump.
    match result {
        Ok(response) => {
            if let Some(sock) = socket {
                crate::channelz::Registry::global().note_stream_started(sock, true);
            }
            Ok(response.map(|stream| stream.bind_channelz_socket(socket)))
        }
        Err(status) => {
            if let Some(sock) = socket {
                let global = crate::channelz::Registry::global();
                global.note_stream_started(sock, true);
                global.note_stream_end(sock, false);
            }
            Err(status)
        }
    }
}

impl super::Channel {
    /// Issue a server-streaming RPC: one request message, many responses.
    ///
    /// `path` is the full gRPC path, `/<package>.<Service>/<Method>`.
    /// A hand-written [`crate::Service`] is first-class on this path;
    /// generated clients call this for you.
    ///
    /// Await the [`Call`] for headers and the response [`Streaming`]. Dropping
    /// the [`Call`] without awaiting resets the stream, the same as dropping a
    /// unary [`Call`]. A [`crate::CallHandle`] taken before await still cancels
    /// while waiting for headers, and still cancels that live stream after
    /// headers. Dropping the received [`Streaming`] before the end does the
    /// same. Letting the deadline fire RSTs the send half before headers and
    /// after headers, matching [`Self::bidi`].
    /// [`crate::Streaming::trailers`] waits for end-of-stream, including when
    /// called before draining messages. A non-OK trailing `grpc-status` is
    /// `Err`. A `-bin` trailer must not appear as a header, including over
    /// TLS, mTLS, Unix, and [`Self::from_io`].
    /// [`Self::max_encoding_message_size`] / [`Self::max_decoding_message_size`]
    /// fail this path as [`Code::ResourceExhausted`], including over TLS, mTLS,
    /// Unix, and [`Self::from_io`]. Distinct from generated client wrappers.
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let mut req = HelloRequest::new();
    /// req.set_name("world");
    /// let mut stream = channel
    ///     .server_streaming::<HelloRequest, HelloReply>(
    ///         "/helloworld.Greeter/ServerHello",
    ///         Request::new(req),
    ///     )
    ///     .await?
    ///     .into_inner();
    /// while let Some(reply) = stream.message().await? {
    ///     let _ = reply;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn server_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<Req>,
    ) -> Call<Response<Streaming<Resp>>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let channel = self.clone();
        let mut req = req;
        let prepared = channel.prepare_outbound(path, &mut req);
        let (cancel, cancel_rx) = watch::channel(false);
        let reset = cancel.clone();
        let wire = channel.wire_for(path);
        let observer = channel.observer.clone();
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
                let permit = match channel.take_rpc_slot() {
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
                let retry_policy = channel
                    .method_config_for(path)
                    .and_then(|method| method.retry_policy);
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
                    let (slot, r#gen, lease, driver, rr_addr, live_socket) = (
                        live.slot,
                        live.r#gen,
                        live.lease,
                        live.driver,
                        live.rr_addr,
                        live.channelz_socket,
                    );
                    let byte_permit = match channel.byte_budget.acquire(frame.len()) {
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
                    match run_server_stream(
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
                        live_socket,
                    )
                    .await
                    {
                        Ok(response) => {
                            let hooked = channel.apply_response_hooks(path, response);
                            match hooked {
                                Ok(response) => {
                                    channel.note_call_outcome(true).await;
                                    attempt_guard.finish(&Status::ok());
                                    call_guard.finish(&Status::ok());
                                    channel.retry_stats.record_committed(true);
                                    return Ok(pool::attach_conn(
                                        response,
                                        lease,
                                        driver,
                                        Some(reset),
                                        permit,
                                        live_socket,
                                    ));
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
                                    channel.retry_stats.record_committed(false);
                                    attempt_guard.finish(&status);
                                    call_guard.finish(&status);
                                    return Err(status);
                                }
                            }
                        }
                        Err(status)
                            if !retried
                                && status.is_transparent_retryable()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            retried = true;
                            channel.retry_stats.record_transparent_retry();
                            attempt_guard.finish(&status);
                            channel
                                .inner
                                .discard_conn(slot, r#gen, rr_addr.as_ref())
                                .await;
                            attempt_idx += 1;
                        }
                        Err(status) => {
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
                                &status,
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
                                    attempt_guard.finish(&status);
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
                                        &status,
                                        policy_attempts,
                                        per_attempt_timeout,
                                        cancelled,
                                    ) {
                                        channel.retry_stats.record_exhausted();
                                    }
                                }
                            }
                            if status.is_transport() {
                                channel
                                    .inner
                                    .discard_conn(slot, r#gen, rr_addr.as_ref())
                                    .await;
                            }
                            if *cancel_rx.borrow() {
                                attempt_guard.cancel(CancellationReason::CallerCancelled);
                                call_guard.cancel(CancellationReason::CallerCancelled);
                            } else if status.code() == Code::DeadlineExceeded {
                                attempt_guard.cancel(CancellationReason::DeadlineExceeded);
                                call_guard.cancel(CancellationReason::DeadlineExceeded);
                            }
                            channel.note_call_outcome(false).await;
                            channel.retry_stats.record_committed(false);
                            attempt_guard.finish(&status);
                            call_guard.finish(&status);
                            return Err(status);
                        }
                    }
                }
            }),
        )
    }

    /// Issue a client-streaming RPC: many request messages, one response.
    ///
    /// A hand-written [`crate::Service`] is first-class on this path;
    /// generated clients call this for you.
    ///
    /// Send on the returned [`StreamSender`], drop it to half-close, then
    /// await the [`Call`]. Dropping the pair without awaiting resets the
    /// stream, the same as dropping a unary [`Call`]. A [`crate::CallHandle`]
    /// taken before await still cancels after the sender is closed, while the
    /// unary response is pending. Cancelling before any request message
    /// (`cancel_after_begin`) is [`crate::Code::Cancelled`], not OK from a
    /// half-close: hold the [`StreamSender`] until the [`Call`] settles,
    /// including over TLS, mTLS, Unix, and [`Self::from_io`]. Dropping the
    /// [`Call`] or letting its deadline fire after that half-close resets the
    /// same way.
    /// OK-path custom trailers land on [`crate::Response::trailers`]; a `-bin`
    /// trailer must not appear as a header, including over TLS, mTLS, Unix,
    /// and [`Self::from_io`].
    /// [`Self::max_encoding_message_size`] / [`Self::max_decoding_message_size`]
    /// fail this path as [`Code::ResourceExhausted`], including over TLS, mTLS,
    /// Unix, and [`Self::from_io`]. Distinct from generated client wrappers.
    ///
    /// [`crate::StreamSender::fail`] resolves the [`Call`] with that status
    /// (no request-side `grpc-status`; the stream is reset with CANCEL).
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(
    ///     "/helloworld.Greeter/ClientHello",
    ///     Request::new(()),
    /// );
    /// for name in ["ada", "grace"] {
    ///     let mut req = HelloRequest::new();
    ///     req.set_name(name);
    ///     tx.send(req).await?;
    /// }
    /// tx.close();
    /// let reply = call.await?.into_inner();
    /// # let _ = reply;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "dropping a client-streaming Call resets the stream"]
    pub fn client_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamSender<Req>, Call<Response<Resp>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let mut req = req;
        let prepared = self.prepare_outbound(path, &mut req);
        let wire = self.wire_for(path);
        let (tx, rx) = Streaming::channel(self.config.stream_buffer_size());
        let tx = tx.with_limits(wire.limits).with_compress(req.compress());
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let observer = self.observer.clone();
        let call = Call::new(
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
                let (_, md, _, compress, ua) = req.into_parts();
                let user_agent = ua.unwrap_or_else(|| channel.user_agent.clone());
                let _permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                let attempt_labels = AttemptLabels::new(call_labels, 1);
                let attempt_start = std::time::Instant::now();
                if let Some(obs) = &observer {
                    obs.on_attempt_start(&attempt_labels);
                }
                let mut attempt_guard =
                    AttemptGuard::new(observer.clone(), owned_labels.clone(), 1, attempt_start);
                let queue_start = tokio::time::Instant::now();
                let tap = channel
                    .binlog
                    .as_ref()
                    .and_then(|binlog| binlog.start_call(path, crate::binlog::Logger::Client));
                let opened = match channel
                    .open_retrying(
                        cancel_rx.clone(),
                        req_timeout,
                        deadline,
                        wait,
                        path,
                        &md,
                        compress,
                        &user_agent,
                    )
                    .await
                {
                    Ok(opened) => {
                        if let Some(obs) = &observer {
                            obs.on_queue_wait(&call_labels, queue_start.elapsed());
                        }
                        if let Some(tap) = &tap {
                            tap.log_client_header(
                                &md,
                                path,
                                channel.authority.as_str(),
                                req_timeout,
                            );
                        }
                        opened
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
                        attempt_guard.finish(&status);
                        return Err(status);
                    }
                };
                let budget = channel.byte_budget.clone();
                let response: Response<Resp> = match run_client_stream(
                    opened.resp_fut,
                    opened.send,
                    rx,
                    cancel_rx.clone(),
                    wire,
                    deadline,
                    budget,
                    tap.as_ref(),
                    opened.channelz_socket,
                )
                .await
                {
                    Ok(r) => r,
                    Err(status) => {
                        if *cancel_rx.borrow() {
                            attempt_guard.cancel(CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            attempt_guard.cancel(CancellationReason::DeadlineExceeded);
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        }
                        attempt_guard.finish(&status);
                        call_guard.finish(&status);
                        return Err(status);
                    }
                };
                let final_res = channel.apply_response_hooks(path, response);
                match &final_res {
                    Ok(_) => {
                        if let Some(obs) = &observer {
                            obs.on_bytes_received(&call_labels, 0);
                        }
                        attempt_guard.finish(&Status::ok());
                        call_guard.finish(&Status::ok());
                    }
                    Err(status) => {
                        attempt_guard.finish(status);
                        call_guard.finish(status);
                    }
                }
                final_res
            }),
        );
        (tx, call)
    }

    /// Issue a bidirectional-streaming RPC.
    ///
    /// A hand-written [`crate::Service`] is first-class on this path;
    /// generated clients call this for you.
    ///
    /// Send on the returned [`StreamSender`] and await the [`Call`] for
    /// responses. Dropping the pair without awaiting resets the stream,
    /// the same as dropping a unary [`Call`]. A [`crate::CallHandle`] taken
    /// before await still cancels while waiting for headers, and still
    /// cancels that live stream after headers, including after the sender is
    /// closed. Dropping the received [`Streaming`] before the end does the
    /// same. Letting the deadline fire RSTs the send half before headers and
    /// after a half-close, so a Ready [`Call`] does not leave the stream
    /// parked.
    ///
    /// [`crate::StreamSender::fail`] before headers resolves the [`Call`] with
    /// that status, not `UNAVAILABLE` from the reset; after headers the
    /// received [`Streaming`] sees [`crate::Code::Cancelled`], not that status.
    /// [`crate::Streaming::trailers`] waits for end-of-stream, including when
    /// called before draining messages. A non-OK trailing `grpc-status` is
    /// `Err`. A `-bin` trailer must not appear as a header, including over
    /// TLS, mTLS, Unix, and [`Self::from_io`].
    /// [`Self::max_encoding_message_size`] / [`Self::max_decoding_message_size`]
    /// fail this path as [`Code::ResourceExhausted`], including over TLS, mTLS,
    /// Unix, and [`Self::from_io`]. Distinct from generated client wrappers.
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let (tx, call) = channel.bidi::<HelloRequest, HelloReply>(
    ///     "/helloworld.Greeter/StreamHello",
    ///     Request::new(()),
    /// );
    /// let mut inbound = call.await?.into_inner();
    /// let mut ping = HelloRequest::new();
    /// ping.set_name("ping");
    /// tx.send(ping).await?;
    /// tx.close();
    /// let _ = inbound.message().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "dropping a bidi Call resets the stream"]
    pub fn bidi<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamSender<Req>, Call<Response<Streaming<Resp>>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let channel = self.clone();
        let mut req = req;
        let prepared = channel.prepare_outbound(path, &mut req);
        let wire = channel.wire_for(path);
        let buffer = channel.config.stream_buffer_size();
        let (tx, rx) = Streaming::channel(buffer);
        let tx = tx.with_limits(wire.limits).with_compress(req.compress());
        let (cancel, cancel_rx) = watch::channel(false);
        let reset = cancel.clone();
        let observer = channel.observer.clone();
        let call = Call::new(
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
                let (_, md, _, compress, ua) = req.into_parts();
                let user_agent = ua.unwrap_or_else(|| channel.user_agent.clone());
                let permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                let attempt_labels = AttemptLabels::new(call_labels, 1);
                let attempt_start = std::time::Instant::now();
                if let Some(obs) = &observer {
                    obs.on_attempt_start(&attempt_labels);
                }
                let mut attempt_guard =
                    AttemptGuard::new(observer.clone(), owned_labels.clone(), 1, attempt_start);
                let queue_start = tokio::time::Instant::now();
                let tap = channel
                    .binlog
                    .as_ref()
                    .and_then(|binlog| binlog.start_call(path, crate::binlog::Logger::Client));
                let opened = match channel
                    .open_retrying(
                        cancel_rx.clone(),
                        req_timeout,
                        deadline,
                        wait,
                        path,
                        &md,
                        compress,
                        &user_agent,
                    )
                    .await
                {
                    Ok(opened) => {
                        if let Some(obs) = &observer {
                            obs.on_queue_wait(&call_labels, queue_start.elapsed());
                        }
                        if let Some(tap) = &tap {
                            tap.log_client_header(
                                &md,
                                path,
                                channel.authority.as_str(),
                                req_timeout,
                            );
                        }
                        opened
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
                        attempt_guard.finish(&status);
                        return Err(status);
                    }
                };
                let budget = channel.byte_budget.clone();
                let response = match run_bidi(
                    opened.resp_fut,
                    opened.send,
                    rx,
                    cancel_rx.clone(),
                    wire,
                    tap,
                    deadline,
                    budget,
                    opened.channelz_socket,
                )
                .await
                {
                    Ok(resp) => resp,
                    Err(status) => {
                        if *cancel_rx.borrow() {
                            attempt_guard.cancel(CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            attempt_guard.cancel(CancellationReason::DeadlineExceeded);
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        }
                        attempt_guard.finish(&status);
                        call_guard.finish(&status);
                        return Err(status);
                    }
                };
                let response = channel.apply_response_hooks(path, response)?;
                attempt_guard.finish(&Status::ok());
                call_guard.finish(&Status::ok());
                Ok(pool::attach_conn(
                    response,
                    opened.lease,
                    opened.driver,
                    Some(reset),
                    permit,
                    opened.channelz_socket,
                ))
            }),
        );
        (tx, call)
    }
}
