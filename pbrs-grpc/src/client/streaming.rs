//! Streaming call shapes.

use super::call::{
    AttemptCommitment, cancel_attempt, finish_attempt, first_of, open, prefer_deadline,
    prefer_peer_rejection_after_send, race, reject_attempt, send_request_frame,
};
use super::pool;
use super::retry::{
    PolicyDecision, PolicyRetryDispatch, RequestReplay, policy_retry_delay, retry_exhausted,
};
use crate::binlog::CallLogger;
use crate::codec::CodecMessage;
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
use crate::transport::{Reason, SendStream, h2 as backend};
use crate::wire::encode::checked_budget_frame_len;
use crate::wire::{
    OutBatch, PumpEnd, SegFrame, encode_msg, finish_stream, finish_unary, reset_on_cancel,
};
use bytes::Bytes;
use http::HeaderValue;
use http::uri::Authority;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

#[allow(
    clippy::too_many_arguments,
    reason = "thin cancel-logging wrapper over run_server_stream_inner"
)]
pub(crate) async fn run_server_stream<Resp>(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: SegFrame,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: CodecMessage + Send + 'static,
{
    run_server_stream_frame(
        send_req, authority, path, md, timeout, deadline, compress, frame, cancel_rx, wire,
        user_agent, https, permit, tap, socket, None,
    )
    .await
}

/// [`run_server_stream`] with cancellation logging around one attempt.
#[allow(
    clippy::too_many_arguments,
    reason = "thin cancel-logging wrapper over run_server_stream_inner"
)]
async fn run_server_stream_frame<Resp>(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: SegFrame,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
    policy_dispatch: Option<&mut PolicyRetryDispatch<'_>>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: CodecMessage + Send + 'static,
{
    let outcome = run_server_stream_inner(
        send_req,
        authority,
        path,
        md,
        timeout,
        deadline,
        compress,
        frame,
        cancel_rx,
        wire,
        user_agent,
        https,
        permit,
        tap,
        socket,
        policy_dispatch,
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
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    compress: bool,
    frame: SegFrame,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
    permit: BytePermit,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
    policy_dispatch: Option<&mut PolicyRetryDispatch<'_>>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: CodecMessage + Send + 'static,
{
    let mut commitment = AttemptCommitment::Uncommitted;
    if let Some(tap) = tap {
        tap.log_client_header(md, path, authority.as_str(), timeout);
    }
    let (mut resp_fut, mut send_stream) = match open(
        send_req,
        authority,
        path,
        md,
        timeout,
        deadline,
        &cancel_rx,
        compress.then_some(wire.send_codec),
        wire.accept_gzip,
        &user_agent,
        https,
    )
    .await
    {
        Ok(opened) => opened,
        // HEADERS never went out; the caller retains the encoded frame.
        Err(status) => return Err(commitment.classify(status)),
    };
    if let Some(dispatch) = policy_dispatch {
        dispatch.on_headers_opened();
    }
    commitment = AttemptCommitment::BodyStarted;
    let log_frame = tap.is_some().then(|| frame.clone());
    let mut received = None;
    let sent = send_request_frame(
        &mut send_stream,
        frame,
        wire.send_buffer,
        &cancel_rx,
        deadline,
        &mut resp_fut,
        &mut received,
    )
    .await;
    drop(permit);
    if sent.is_ok() {
        if let (Some(tap), Some(log_frame)) = (tap, &log_frame) {
            // Server-streaming sends one request with end-of-stream set.
            for seg in log_frame.segments() {
                tap.log_written(seg);
            }
            tap.log_half_close();
        }
    }
    drop(log_frame);
    if let Err(status) = sent {
        if matches!(status.code(), Code::Cancelled | Code::DeadlineExceeded) {
            return Err(status);
        }
        return race(
            prefer_peer_rejection_after_send(
                async move {
                    match received {
                        Some(head) => head,
                        None => resp_fut.await,
                    }
                },
                commitment.classify(status),
            ),
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
    let response = race(
        async {
            let response = match received {
                Some(head) => head,
                None => resp_fut.await,
            }
            .map_err(|e| commitment.classify_h2(e))?;
            commitment = AttemptCommitment::ResponseCommitted;
            finish_stream::<Resp>(
                response,
                wire.limits,
                deadline,
                wire.accept_gzip,
                tap.cloned(),
            )
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
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    rx: Streaming<Req>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    budget: ByteBudgetTracker,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Resp>, Status>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage,
{
    // Keep the send half on this stack and RST it if the Call is dropped
    // mid-wait. Harvesting it from a spawned pump lost the RST: cancel can
    // win the same `select!` as JoinHandle Ready, and RecvStream drop is
    // not a last-ref reset while that task holds SendStream.
    let mut send = ResetSend::new(send_stream);
    let result = {
        let mut failed = false;
        let result = {
            let pump = pump_outbound_budget(
                send.stream_mut(),
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
            let cancelled = cancel_rx;
            let mut half_closed = false;
            loop {
                tokio::select! {
                    biased;
                    () = &mut until_deadline => break Err(Status::deadline_exceeded()),
                    _ = cancelled.wait() => break Err(Status::cancelled()),
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
            send.stream_mut().send_reset(Reason::CANCEL);
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
    note_client_stream_end(socket, &result);
    result
}

async fn pump_outbound_budget<T: CodecMessage>(
    send: &mut backend::SendStream,
    mut rx: Streaming<T>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    budget: &ByteBudgetTracker,
    tap: Option<CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> PumpEnd {
    let mut batch = OutBatch::new(wire);
    if let Some(tap) = &tap {
        batch.set_tap(tap.clone());
    }
    // One map lookup for the whole upload. Each encoded message then
    // touches the entry's atomics.
    let channelz_entry = channelz_socket.and_then(crate::channelz::socket_entry);
    let mut items = Vec::with_capacity(OutBatch::BURST);
    let mut permits = Vec::with_capacity(OutBatch::BURST);
    let mut watch_cancel = true;
    loop {
        items.clear();
        let taken = tokio::select! {
            cancelled = async {
                cancel_rx.wait().await;
                cancel_rx.is_cancelled()
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
        if items.len() > 1 && room > 0 && !rx.producer_finished() {
            crate::wire::let_producer_catch_up().await;
            rx.try_recv_many(&mut items, room);
        }
        let mut ready = items.drain(..);
        while let Some(item) = ready.next() {
            let ready_messages = ready.len() + 1;
            let item = match item {
                Ok(item) => item,
                Err(status) => return PumpEnd::Failed(status),
            };
            let frame_len = match checked_budget_frame_len(item.message.encoded_len(), wire.limits)
            {
                Ok(len) => len,
                Err(status) => return PumpEnd::Failed(status),
            };
            match budget.acquire(frame_len) {
                Ok(permit) => permits.push(permit),
                Err(status) => return PumpEnd::Failed(status),
            }
            if let Err(status) = batch.encode_ready(item, ready_messages) {
                return PumpEnd::Failed(status);
            }
            // Channelz: each encoded message counts as sent (a later
            // flush failure can only overcount a doomed stream).
            if let Some(entry) = &channelz_entry {
                entry.add_messages(true, 1);
            }
            if batch.is_full() {
                if batch.flush(send).await.is_err() {
                    send.send_reset(Reason::INTERNAL_ERROR);
                    return PumpEnd::Reset;
                }
                permits.clear();
            }
        }
        drop(ready);
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
    stream: Option<backend::SendStream>,
    live: bool,
}

impl Drop for ResetSend {
    fn drop(&mut self) {
        if self.live {
            if let Some(stream) = self.stream.as_mut() {
                stream.send_reset(Reason::CANCEL);
            }
        }
    }
}

impl ResetSend {
    fn new(stream: backend::SendStream) -> Self {
        Self {
            stream: Some(stream),
            live: true,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "send access ends before the guard transfers its stream into the completion holder"
    )]
    fn stream_mut(&mut self) -> &mut backend::SendStream {
        self.stream.as_mut().expect("send half is still held")
    }
}

enum ClosedUpload {
    /// Every request byte and the half-close are queued.
    Sent,
    /// The peer completed its response and stopped accepting the upload with
    /// RST_STREAM(NO_ERROR). Read and validate that response before deciding
    /// the RPC result; a local CANCEL here would discard queued trailers.
    PeerFinished,
    /// A prefix may already be queued. `rest` is the unsent payload, in order.
    Rest(EncodedUpload),
}

/// Encoded transport-owned bytes keep their existing budget charge until
/// accepted by the HTTP/2 send queue or dropped with the RPC.
struct EncodedUpload {
    segments: Vec<Bytes>,
    permit: BytePermit,
}

/// Encode a closed request stream and queue it without waiting for window.
///
/// `Err` resets `send`. [`ClosedUpload::Rest`] means at least one segment did
/// not fit in the stream buffer; nothing in `rest` has been queued.
fn write_closed_upload<T: CodecMessage>(
    send: &mut backend::SendStream,
    items: Vec<Result<crate::stream::Framed<T>, Status>>,
    wire: Wire,
    budget: &ByteBudgetTracker,
    tap: &Option<CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<ClosedUpload, Status> {
    let channelz_entry = socket.and_then(crate::channelz::socket_entry);
    let mut batch = OutBatch::new(wire);
    if let Some(tap) = tap {
        batch.set_tap(tap.clone());
    }
    let mut permit = BytePermit::empty();
    let mut queued: Vec<Bytes> = Vec::new();
    let fail = |send: &mut backend::SendStream, status: Status| {
        send.send_reset(Reason::CANCEL);
        status
    };
    let mut ready = items.into_iter();
    while let Some(item) = ready.next() {
        let ready_messages = ready.len() + 1;
        let item = match item {
            Ok(item) => item,
            Err(status) => return Err(fail(send, status)),
        };
        let frame_len = match checked_budget_frame_len(item.message.encoded_len(), wire.limits) {
            Ok(len) => len,
            Err(status) => return Err(fail(send, status)),
        };
        match budget.acquire(frame_len) {
            Ok(acquired) => permit.merge(acquired),
            Err(status) => return Err(fail(send, status)),
        }
        if let Err(status) = batch.encode_ready(item, ready_messages) {
            return Err(fail(send, status));
        }
        if let Some(entry) = &channelz_entry {
            entry.add_messages(true, 1);
        }
        if batch.is_full() {
            if let Some(frame) = batch.take_frame() {
                queued.extend(frame.into_segments());
            }
        }
    }
    drop(ready);
    if let Some(frame) = batch.take_frame() {
        queued.extend(frame.into_segments());
    }
    let mut remaining = queued.into_iter();
    while let Some(seg) = remaining.next() {
        match send.try_send_data(seg, false) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                if err.is_remote() && err.is_reset() && err.reason() == Some(Reason::NO_ERROR) {
                    return Ok(ClosedUpload::PeerFinished);
                }
                return Err(fail(send, Status::from_h2_send(err)));
            }
            Err(seg) => {
                // Rebuild the unsent suffix once, only when the send buffer
                // blocks. Consuming the queued prefix never shifts segments.
                let mut rest = Vec::with_capacity(1 + remaining.len());
                rest.push(seg);
                rest.extend(remaining);
                return Ok(ClosedUpload::Rest(EncodedUpload {
                    segments: rest,
                    permit,
                }));
            }
        }
    }
    if let Err(err) = send.send_data(Bytes::new(), true) {
        if err.is_remote() && err.is_reset() && err.reason() == Some(Reason::NO_ERROR) {
            return Ok(ClosedUpload::PeerFinished);
        }
        return Err(fail(send, Status::from_h2_send(err)));
    }
    if let Some(tap) = tap {
        tap.log_half_close();
    }
    Ok(ClosedUpload::Sent)
}

/// Unary response after a closed client-streaming upload is fully queued.
///
/// No deadline arm: the caller only uses this when the deadline is absent, so
/// the wait does not embed a sleep timer.
async fn await_closed_client_response<Resp: CodecMessage>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<&CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Resp>, Status> {
    let mut send = ResetSend::new(send_stream);
    let cancelled = cancel_rx;
    let fut = async {
        let response = resp_fut.await.map_err(Status::from_h2_post_dispatch)?;
        finish_unary::<Resp>(response, wire.limits, wire.accept_gzip, tap).await
    };
    tokio::pin!(fut);
    let result = tokio::select! {
        biased;
        _ = cancelled.wait() => {
            send.stream_mut().send_reset(Reason::CANCEL);
            Err(Status::cancelled())
        }
        result = &mut fut => result,
    };
    send.live = false;
    if let (Some(tap), Err(status)) = (tap, &result) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    note_client_stream_end(socket, &result);
    result
}

/// Send a closed upload while reading its final unary response. A completed
/// response ends the RPC even when the peer stops accepting request bytes.
async fn send_client_rest<Resp: CodecMessage>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    rest: EncodedUpload,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Resp>, Status> {
    let mut send = ResetSend::new(send_stream);
    let mut send_done = false;
    let mut send_failed = false;
    let result = {
        let send_fut = pump_encoded_rest(send.stream_mut(), rest, &cancel_rx, wire, tap.as_ref());
        tokio::pin!(send_fut);
        let response_fut = async {
            let response = resp_fut.await.map_err(Status::from_h2_post_dispatch)?;
            finish_unary::<Resp>(response, wire.limits, wire.accept_gzip, tap.as_ref()).await
        };
        tokio::pin!(response_fut);
        loop {
            tokio::select! {
                biased;
                _ = cancel_rx.wait() => break Err(Status::cancelled()),
                result = &mut response_fut => break result,
                sent = &mut send_fut, if !send_done => {
                    send_done = true;
                    // Like the live upload pump, a wire send failure does not
                    // discard a response already queued by the peer.
                    send_failed = !matches!(sent, PumpEnd::HalfClosed);
                }
            }
        }
    };
    if !send_done
        || send_failed
        || matches!(&result, Err(status) if status.code() == Code::Cancelled)
    {
        send.stream_mut().send_reset(Reason::CANCEL);
    }
    send.live = false;
    if let (Some(tap), Err(status)) = (&tap, &result) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    note_client_stream_end(socket, &result);
    result
}

fn note_client_stream_end<T>(
    socket: Option<crate::channelz::SocketId>,
    result: &Result<T, Status>,
) {
    if let Some(sock) = socket {
        crate::channelz::Registry::global().note_local_stream_finished(
            sock,
            result.is_ok(),
            u64::from(result.is_ok()),
        );
    }
}

/// HTTP response after a closed upload with no deadline.
///
/// The gRPC envelope is built after this resolves so the wait future does not
/// carry it.
struct ReadyHttp {
    received: Result<http::Response<backend::RecvStream>, Status>,
    send: ResetSend,
    later_cancel: crate::rpc_cancel::RpcCancel,
}

async fn wait_ready_bidi_http(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    tap: Option<CallLogger>,
) -> ReadyHttp {
    let mut send = ResetSend::new(send_stream);
    let later_cancel = cancel_rx.clone();
    let cancelled = cancel_rx;
    let response_fut = async { resp_fut.await.map_err(Status::from_h2_post_dispatch) };
    tokio::pin!(response_fut);
    let received = tokio::select! {
        biased;
        _ = cancelled.wait() => {
            send.stream_mut().send_reset(Reason::CANCEL);
            Err(Status::cancelled())
        }
        result = &mut response_fut => result,
    };
    send.live = false;
    if let (Some(tap), Err(status)) = (&tap, &received) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    ReadyHttp {
        received,
        send,
        later_cancel,
    }
}

fn finish_ready_bidi<Resp>(
    ready: ReadyHttp,
    wire: Wire,
    tap: Option<CallLogger>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: CodecMessage + Send + 'static,
{
    let ReadyHttp {
        received,
        mut send,
        later_cancel,
    } = ready;
    let result = match received {
        Ok(response) => {
            finish_stream::<Resp>(response, wire.limits, None, wire.accept_gzip, tap.clone())
        }
        Err(status) => Err(status),
    };
    match result {
        Ok(response) => {
            if let Some(sock) = socket {
                crate::channelz::Registry::global().note_stream_started(sock, true);
            }
            let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
            send.live = false;
            if let Some(stream) = send.stream.take() {
                crate::wire::hold_half_closed_send(stream, later_cancel, None, completed_rx);
            }
            Ok(response.map(|stream| {
                stream
                    .bind_channelz_socket(socket)
                    .with_outbound_completion(completed_tx)
            }))
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

type BidiFut<Resp> =
    Pin<Box<dyn Future<Output = Result<Response<Streaming<Resp>>, Status>> + Send>>;

/// Build the deadline header wait inside this function so the caller stores a pointer.
#[inline(never)]
fn box_bidi_deadline<Resp>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    socket: Option<crate::channelz::SocketId>,
) -> BidiFut<Resp>
where
    Resp: CodecMessage + Send + 'static,
{
    Box::pin(await_flushed_bidi_headers(
        resp_fut,
        send_stream,
        cancel_rx,
        wire,
        tap,
        deadline,
        socket,
    ))
}

/// Build the partial-upload wait inside this function so the caller stores a pointer.
#[inline(never)]
#[allow(
    clippy::too_many_arguments,
    reason = "build the cold encoded-upload future from the existing transport and call state"
)]
fn box_bidi_rest<Req, Resp>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    rest: EncodedUpload,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    socket: Option<crate::channelz::SocketId>,
) -> BidiFut<Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    Box::pin(run_bidi_live(
        resp_fut,
        send_stream,
        BidiUpload::<Req>::Encoded(rest),
        cancel_rx,
        wire,
        tap,
        deadline,
        socket,
    ))
}

/// Build the live upload inside this function so the caller stores a pointer.
#[inline(never)]
#[allow(
    clippy::too_many_arguments,
    reason = "build the cold live-upload future from the existing transport and call state"
)]
fn box_bidi_live<Req, Resp>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    rx: Streaming<Req>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    budget: ByteBudgetTracker,
    socket: Option<crate::channelz::SocketId>,
) -> BidiFut<Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    Box::pin(run_bidi_live(
        resp_fut,
        send_stream,
        BidiUpload::Messages { rx, budget },
        cancel_rx,
        wire,
        tap,
        deadline,
        socket,
    ))
}

#[allow(
    clippy::too_many_arguments,
    reason = "response half plus cancel, deadline, and the already-queued upload"
)]
/// Headers, cancel, and deadline after a closed upload is fully queued.
async fn await_flushed_bidi_headers<Resp>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: CodecMessage + Send + 'static,
{
    let mut send = ResetSend::new(send_stream);
    let later_cancel = cancel_rx.clone();
    let cancelled = cancel_rx;
    let response_fut = async {
        let response = resp_fut.await.map_err(Status::from_h2_post_dispatch)?;
        finish_stream::<Resp>(
            response,
            wire.limits,
            deadline,
            wire.accept_gzip,
            tap.clone(),
        )
    };
    tokio::pin!(response_fut);
    let until_deadline = async {
        match deadline {
            Some(at) => tokio::time::sleep_until(at).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(until_deadline);
    let result = tokio::select! {
        biased;
        () = &mut until_deadline => {
            send.stream_mut().send_reset(Reason::CANCEL);
            Err(Status::deadline_exceeded())
        }
        _ = cancelled.wait() => {
            send.stream_mut().send_reset(Reason::CANCEL);
            Err(Status::cancelled())
        }
        result = &mut response_fut => result,
    };
    send.live = false;
    let result = prefer_deadline(result, deadline);
    if let (Some(tap), Err(status)) = (&tap, &result) {
        if status.code() == Code::Cancelled {
            tap.log_cancel();
        }
    }
    match result {
        Ok(response) => {
            if let Some(sock) = socket {
                crate::channelz::Registry::global().note_stream_started(sock, true);
            }
            let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
            send.live = false;
            if let Some(stream) = send.stream.take() {
                crate::wire::hold_half_closed_send(stream, later_cancel, deadline, completed_rx);
            }
            Ok(response.map(|stream| {
                stream
                    .bind_channelz_socket(socket)
                    .with_outbound_completion(completed_tx)
            }))
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
    reason = "dispatch the opened stream with its upload, cancellation, deadline, and resource state"
)]
async fn run_bidi<Req, Resp>(
    resp_fut: backend::ResponseFuture,
    mut send_stream: backend::SendStream,
    mut rx: Streaming<Req>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    budget: ByteBudgetTracker,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    // A closed producer can still be blocked on HTTP/2 request credit.
    // Bypass the independent upload pump only when every byte queues here.
    if rx.producer_finished() {
        // The messages are already queued. Writing them before the first
        // await keeps the upload state machine out of the call future when
        // they fit in the HTTP/2 send buffer.
        if let Some(items) = rx.drain_closed_channel() {
            match write_closed_upload(&mut send_stream, items, wire, &budget, &tap, socket) {
                Ok(ClosedUpload::Sent | ClosedUpload::PeerFinished) if deadline.is_none() => {
                    let ready =
                        wait_ready_bidi_http(resp_fut, send_stream, cancel_rx, tap.clone()).await;
                    return finish_ready_bidi(ready, wire, tap, socket);
                }
                Ok(ClosedUpload::Sent | ClosedUpload::PeerFinished) => {
                    return box_bidi_deadline(
                        resp_fut,
                        send_stream,
                        cancel_rx,
                        wire,
                        tap,
                        deadline,
                        socket,
                    )
                    .await;
                }
                Ok(ClosedUpload::Rest(rest)) => {
                    return box_bidi_rest::<Req, Resp>(
                        resp_fut,
                        send_stream,
                        rest,
                        cancel_rx,
                        wire,
                        tap,
                        deadline,
                        socket,
                    )
                    .await;
                }
                Err(status) => return Err(status),
            }
        }
    }
    box_bidi_live(
        resp_fut,
        send_stream,
        rx,
        cancel_rx,
        wire,
        tap,
        deadline,
        budget,
        socket,
    )
    .await
}

/// Closed uploads that exceed the send buffer need the same independent
/// response and upload lifetimes as a live producer. Fully queued uploads
/// continue to use the smaller, task-free header wait.
enum BidiUpload<Req> {
    Messages {
        rx: Streaming<Req>,
        budget: ByteBudgetTracker,
    },
    Encoded(EncodedUpload),
}

async fn pump_encoded_rest(
    send: &mut backend::SendStream,
    rest: EncodedUpload,
    cancel_rx: &crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<&CallLogger>,
) -> PumpEnd {
    let EncodedUpload { segments, permit } = rest;
    let sending = async {
        for segment in segments {
            crate::wire::send::send_bytes(send, segment, false, wire.send_buffer).await?;
        }
        send.send_data(Bytes::new(), true)
            .map_err(Status::from_h2_send)
    };
    let result = tokio::select! {
        biased;
        _ = cancel_rx.wait() => None,
        result = sending => Some(result),
    };
    drop(permit);
    match result {
        Some(Ok(())) => {
            if let Some(tap) = tap {
                tap.log_half_close();
            }
            PumpEnd::HalfClosed
        }
        Some(Err(_)) => {
            send.send_reset(Reason::INTERNAL_ERROR);
            PumpEnd::Reset
        }
        None => {
            send.send_reset(Reason::CANCEL);
            PumpEnd::Reset
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "own the opened stream and independent upload with call cancellation and deadline state"
)]
async fn run_bidi_live<Req, Resp>(
    resp_fut: backend::ResponseFuture,
    send_stream: backend::SendStream,
    upload: BidiUpload<Req>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
    tap: Option<CallLogger>,
    deadline: Option<tokio::time::Instant>,
    socket: Option<crate::channelz::SocketId>,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    // A spawned pump can RST before headers; without this channel the Call
    // would see UNAVAILABLE from h2 instead of StreamSender::fail's status.
    // The Call deadline does not set cancel_rx (`is_cancelled` is not
    // deadline). Watch the same Instant here so a Ready DEADLINE_EXCEEDED
    // Call does not leave SendStream parked on a watch that never fires.
    let (fail_tx, mut fail_rx) = tokio::sync::oneshot::channel();
    let (hold_tx, hold_rx) = tokio::sync::oneshot::channel::<()>();
    let (completed_tx, mut completed_rx) = tokio::sync::oneshot::channel::<()>();
    let pump_tap = tap.clone();
    #[cfg(feature = "copy-counts")]
    crate::copy_counts::note_spawn(crate::copy_counts::SpawnSite::ClientBidiUpload);
    // Capture an armed guard before spawning: task destruction before its
    // first poll, an upload panic, or runtime shutdown must still reset the
    // request while the response future/stream owns another HTTP/2 reference.
    let send = ResetSend::new(send_stream);
    let pump_task = tokio::spawn({
        let cancel_rx = cancel_rx.clone();
        async move {
            let mut send = send;
            let tap = pump_tap;
            let until_deadline = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::pin!(until_deadline);
            let end = {
                let pump = async {
                    match upload {
                        BidiUpload::Messages { rx, budget } => {
                            pump_outbound_budget(
                                send.stream_mut(),
                                rx,
                                cancel_rx.clone(),
                                wire,
                                &budget,
                                tap,
                                socket,
                            )
                            .await
                        }
                        BidiUpload::Encoded(rest) => {
                            pump_encoded_rest(
                                send.stream_mut(),
                                rest,
                                &cancel_rx,
                                wire,
                                tap.as_ref(),
                            )
                            .await
                        }
                    }
                };
                tokio::pin!(pump);
                tokio::select! {
                    biased;
                    _ = &mut completed_rx => None,
                    () = &mut until_deadline => None,
                    end = &mut pump => Some(end),
                }
            };
            match end {
                None => send.stream_mut().send_reset(Reason::CANCEL),
                Some(PumpEnd::Failed(status)) => {
                    // Hold RST until the Call takes this status. RST first
                    // and resp_fut surfaces CANCEL as UNAVAILABLE
                    // ("stream no longer needed") on the same poll.
                    fail_tx.send(status).ok();
                    hold_rx.await.ok();
                    send.stream_mut().send_reset(Reason::CANCEL);
                }
                Some(PumpEnd::HalfClosed) => {
                    // Preserve cancellation before response completion, but
                    // release the parked send half after terminal trailers.
                    tokio::select! {
                        biased;
                        _ = &mut completed_rx => {},
                        _ = cancel_rx.wait() => {
                            if cancel_rx.is_cancelled() {
                                send.stream_mut().send_reset(Reason::CANCEL);
                            }
                        }
                        () = &mut until_deadline => send.stream_mut().send_reset(Reason::CANCEL),
                    }
                }
                Some(PumpEnd::Reset) => {}
            }
            // Normal completion has already handled cancellation/reset, or
            // observed clean response completion after request half-close.
            send.live = false;
        }
    });
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
        };
        tokio::pin!(fut);
        let until_deadline = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(until_deadline);
        let cancelled = cancel_rx;
        let mut fail_done = false;
        let mut hold_tx = Some(hold_tx);
        loop {
            tokio::select! {
                biased;
                () = &mut until_deadline => break Err(Status::deadline_exceeded()),
                _ = cancelled.wait() => break Err(Status::cancelled()),
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
            Ok(response.map(|stream| {
                stream
                    .bind_channelz_socket(socket)
                    .with_outbound_completion(completed_tx)
            }))
        }
        Err(status) => {
            // A terminal response ends the request half as well. Wait for the
            // owned upload to observe completion before releasing the Call so
            // encoded bytes and their budget charge cannot outlive this error.
            drop(completed_tx);
            let _pump_result = pump_task.await;
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
        mut req: Request<Req>,
    ) -> Call<Response<Streaming<Resp>>>
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
    {
        let channel = self.clone();
        let state = channel.local.service_config.get();
        let prepared = channel.prepare_outbound(path, &mut req, state.as_deref());
        let cancel = crate::rpc_cancel::RpcCancel::new();
        let cancel_rx = cancel.clone();
        let reset = cancel.clone();
        let wire = channel.wire_for(path, state.as_deref());
        let observer = channel.local.observer.clone();
        // Destructure before the async block so the envelope is not stored
        // in the future; only the parts cross awaits.
        let wait = req.wait_for_ready();
        let req_timeout = req.timeout();
        // Field moves, not `into_parts`: passing the envelope by value copies it.
        let msg = req.message;
        let md = req.metadata;
        let compress = req.compress.unwrap_or(false);
        let ua = req.user_agent;
        Call::new(
            cancel,
            Box::pin(async move {
                // Labels are built per use, only when observed, so an
                // unobserved call neither constructs nor stores them.
                let labels = || {
                    CallLabels::new(
                        path,
                        Some(channel.local.authority.as_str()),
                        CallRole::Client,
                    )
                };
                let call_start = crate::telemetry::call_started_at(observer.is_some());
                if let Some(obs) = &observer {
                    obs.on_call_start(&labels());
                }
                let owned_labels = observer.as_ref().map(|_| Box::new(labels().to_owned()));
                let mut call_guard = CallGuard::new(
                    observer.clone(),
                    owned_labels.clone(),
                    call_start,
                    channel.inner.channelz.entry(),
                );

                if let Err(status) = prepared {
                    call_guard.reject(RejectionReason::ClientInterceptor, &status);
                    return Err(status);
                }
                let deadline = deadline_from(req_timeout);
                let _ = match remaining_timeout(deadline) {
                    Ok(t) => t,
                    Err(status) => {
                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                        return Err(status);
                    }
                };
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
                // The original message is no longer needed after encoding.
                drop(msg);
                let https = channel.local.https;
                let ua = ua.unwrap_or_else(|| channel.local.user_agent.clone());
                let permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                channel.local.retry_stats.record_call();
                let tap = channel
                    .local
                    .binlog
                    .as_ref()
                    .and_then(|binlog| binlog.start_call(path, crate::binlog::Logger::Client));
                let retry_policy = channel
                    .method_config_for(path)
                    .and_then(|method| method.retry_policy);
                let mut replay = RequestReplay::new(frame, retry_policy.is_some(), wire.limits);
                let mut attempt_idx = 1u32;
                let mut policy_attempts = 1u32;
                let mut policy_dispatch = retry_policy
                    .as_ref()
                    .map(|_| PolicyRetryDispatch::new(&channel.local.retry_stats));
                let mut retried = false;
                loop {
                    let _ = match remaining_timeout(deadline) {
                        Ok(t) => t,
                        Err(status) => {
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                            channel.local.retry_stats.record_committed(false);
                            return Err(status);
                        }
                    };
                    if let Some(obs) = &observer {
                        obs.on_attempt_start(&AttemptLabels::new(labels(), attempt_idx));
                    }
                    // The attempt guard has no effects without an observer,
                    // so it (and its start clock) only exists then.
                    let mut attempt_guard = observer.as_ref().map(|_| {
                        AttemptGuard::new(
                            observer.clone(),
                            owned_labels.clone(),
                            attempt_idx,
                            std::time::Instant::now(),
                        )
                    });
                    let queue_start = observer.as_ref().map(|_| tokio::time::Instant::now());
                    let live = match channel.grab(&cancel_rx, deadline, wait, Some(&md)).await {
                        Ok(live) => {
                            if let (Some(obs), Some(queue_start)) = (&observer, &queue_start) {
                                obs.on_queue_wait(&labels(), queue_start.elapsed());
                            }
                            live
                        }
                        Err(status) => {
                            if cancel_rx.is_cancelled() {
                                cancel_attempt(
                                    &mut attempt_guard,
                                    CancellationReason::CallerCancelled,
                                );
                                call_guard.cancel(CancellationReason::CallerCancelled);
                            } else if status.code() == Code::DeadlineExceeded {
                                cancel_attempt(
                                    &mut attempt_guard,
                                    CancellationReason::DeadlineExceeded,
                                );
                                call_guard.cancel(CancellationReason::DeadlineExceeded);
                            } else {
                                reject_attempt(
                                    &mut attempt_guard,
                                    RejectionReason::SetupFailed,
                                    &status,
                                );
                                call_guard.reject(RejectionReason::SetupFailed, &status);
                            }
                            if policy_attempts > 1 {
                                channel.note_call_outcome(false).await;
                            }
                            channel.local.retry_stats.record_committed(false);
                            finish_attempt(&mut attempt_guard, &status);
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
                    let _load = live.load;
                    let byte_permit = match channel.local.byte_budget.acquire(replay.total_len()) {
                        Ok(p) => p,
                        Err(status) => {
                            reject_attempt(
                                &mut attempt_guard,
                                RejectionReason::ByteBudgetExceeded,
                                &status,
                            );
                            call_guard.reject(RejectionReason::ByteBudgetExceeded, &status);
                            if policy_attempts > 1 {
                                channel.note_call_outcome(false).await;
                            }
                            channel.local.retry_stats.record_committed(false);
                            finish_attempt(&mut attempt_guard, &status);
                            return Err(status);
                        }
                    };
                    if let Some(obs) = &observer {
                        obs.on_bytes_sent(&labels(), replay.total_len());
                    }
                    let attempt_deadline = retry_policy
                        .as_ref()
                        .and_then(|policy| policy.per_attempt_recv_timeout)
                        .map(|budget| {
                            let capped = tokio::time::Instant::now() + budget;
                            deadline.map_or(capped, |overall| overall.min(capped))
                        })
                        .or(deadline);
                    match run_server_stream_frame(
                        live.send,
                        &channel.local.authority,
                        path,
                        &md,
                        req_timeout,
                        attempt_deadline,
                        compress,
                        replay.next_attempt(),
                        cancel_rx.clone(),
                        wire,
                        ua.clone(),
                        https,
                        byte_permit,
                        tap.as_ref(),
                        live_socket,
                        policy_dispatch.as_mut(),
                    )
                    .await
                    {
                        Ok(response) => {
                            let hooked = channel.apply_response_hooks(path, response);
                            match hooked {
                                Ok(response) => {
                                    channel.note_call_outcome(true).await;
                                    finish_attempt(&mut attempt_guard, &Status::ok());
                                    call_guard.finish(&Status::ok());
                                    channel.local.retry_stats.record_committed(true);
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
                                    if cancel_rx.is_cancelled() {
                                        cancel_attempt(
                                            &mut attempt_guard,
                                            CancellationReason::CallerCancelled,
                                        );
                                        call_guard.cancel(CancellationReason::CallerCancelled);
                                    } else if status.code() == Code::DeadlineExceeded {
                                        cancel_attempt(
                                            &mut attempt_guard,
                                            CancellationReason::DeadlineExceeded,
                                        );
                                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                                    }
                                    channel.note_call_outcome(false).await;
                                    channel.local.retry_stats.record_committed(false);
                                    finish_attempt(&mut attempt_guard, &status);
                                    call_guard.finish(&status);
                                    return Err(status);
                                }
                            }
                        }
                        Err(status)
                            if !retried
                                && replay.allowed()
                                && status.is_transparent_retryable()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            drop(_load);
                            drop(lease);
                            drop(driver);
                            finish_attempt(&mut attempt_guard, &status);
                            // The peer proved non-processing; replay the retained frame.
                            retried = true;
                            channel.local.retry_stats.record_transparent_retry();
                            channel
                                .inner
                                .discard_conn(slot, r#gen, rr_addr.as_ref())
                                .await;
                            attempt_idx += 1;
                        }
                        Err(status) => {
                            // Failed attempts own no live response stream.
                            // Release connection occupancy before any retry
                            // decision or sleep; success transfers its lease.
                            drop(_load);
                            drop(lease);
                            drop(driver);
                            let cancelled = cancel_rx.is_cancelled();
                            let per_attempt_timeout = status.code() == Code::DeadlineExceeded
                                && retry_policy.as_ref().is_some_and(|policy| {
                                    policy.per_attempt_recv_timeout.is_some()
                                })
                                && remaining_timeout(deadline).is_ok()
                                && !cancelled;
                            let decision = if replay.allowed() {
                                policy_retry_delay(
                                    &channel,
                                    retry_policy.as_ref(),
                                    &status,
                                    policy_attempts,
                                    per_attempt_timeout,
                                    cancelled,
                                )
                                .await
                            } else {
                                PolicyDecision::Declined
                            };
                            match decision {
                                PolicyDecision::Proceed {
                                    delay,
                                    via_pushback,
                                } => {
                                    if status.is_transport() {
                                        channel
                                            .inner
                                            .discard_conn(slot, r#gen, rr_addr.as_ref())
                                            .await;
                                    }
                                    finish_attempt(&mut attempt_guard, &status);
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
                                        if cancel_rx.is_cancelled() {
                                            call_guard.cancel(CancellationReason::CallerCancelled);
                                        } else {
                                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                                        }
                                        channel.note_call_outcome(false).await;
                                        channel.local.retry_stats.record_committed(false);
                                        call_guard.finish(&sleep_status);
                                        return Err(sleep_status);
                                    }
                                    if let Some(dispatch) = policy_dispatch.as_mut() {
                                        dispatch.arm(via_pushback, per_attempt_timeout);
                                    }
                                    policy_attempts += 1;
                                    attempt_idx += 1;
                                    continue;
                                }
                                PolicyDecision::Throttled => {
                                    channel.local.retry_stats.record_throttled();
                                }
                                PolicyDecision::PushbackRefused => {
                                    channel.local.retry_stats.record_pushback_refusal();
                                }
                                PolicyDecision::Declined => {
                                    if replay.allowed()
                                        && retry_exhausted(
                                            retry_policy.as_ref(),
                                            &status,
                                            policy_attempts,
                                            per_attempt_timeout,
                                            cancelled,
                                        )
                                    {
                                        channel.local.retry_stats.record_exhausted();
                                    }
                                }
                            }
                            if status.is_transport() {
                                channel
                                    .inner
                                    .discard_conn(slot, r#gen, rr_addr.as_ref())
                                    .await;
                            }
                            if cancel_rx.is_cancelled() {
                                cancel_attempt(
                                    &mut attempt_guard,
                                    CancellationReason::CallerCancelled,
                                );
                                call_guard.cancel(CancellationReason::CallerCancelled);
                            } else if status.code() == Code::DeadlineExceeded {
                                cancel_attempt(
                                    &mut attempt_guard,
                                    CancellationReason::DeadlineExceeded,
                                );
                                call_guard.cancel(CancellationReason::DeadlineExceeded);
                            }
                            channel.note_call_outcome(false).await;
                            channel.local.retry_stats.record_committed(false);
                            finish_attempt(&mut attempt_guard, &status);
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
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
    {
        self.client_streaming_with_input(path, req, |wire, compress| {
            let (tx, rx) = Streaming::channel(self.local.config.stream_buffer_size());
            (tx.with_limits(wire.limits).with_compress(compress), rx)
        })
    }

    #[cfg(feature = "tower")]
    pub(crate) fn client_streaming_from_stream<Req, Resp, S>(
        &self,
        path: &'static str,
        request: Request<S>,
    ) -> Call<Response<Resp>>
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
        S: futures_core::Stream<Item = Result<Req, Status>> + Send + 'static,
    {
        let (stream, parts) = request.into_message_and_parts();
        self.client_streaming_with_input(
            path,
            Request::<()>::from_message_and_parts((), parts),
            |wire, compress| {
                (
                    (),
                    Streaming::from_input(
                        stream,
                        wire.limits,
                        compress,
                        self.local.config.stream_buffer_size(),
                    ),
                )
            },
        )
        .1
    }

    fn client_streaming_with_input<Req, Resp, Input>(
        &self,
        path: &'static str,
        mut req: Request<()>,
        input: impl FnOnce(Wire, bool) -> (Input, Streaming<Req>),
    ) -> (Input, Call<Response<Resp>>)
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
    {
        let state = self.local.service_config.get();
        let prepared = self.prepare_outbound(path, &mut req, state.as_deref());
        let wire = self.wire_for(path, state.as_deref());
        let (tx, rx) = input(wire, req.compress());
        let cancel = crate::rpc_cancel::RpcCancel::new();
        let cancel_rx = cancel.clone();
        let channel = self.clone();
        let observer = self.local.observer.clone();
        // Destructure before the async block so the envelope is not stored
        // in the future; only the parts cross awaits.
        let wait = req.wait_for_ready();
        let req_timeout = req.timeout();
        // Field moves, not `into_parts`: passing the envelope by value copies it.
        let md = req.metadata;
        let compress = req.compress.unwrap_or(false);
        let ua = req.user_agent;
        let call = Call::new(
            cancel,
            Box::pin(async move {
                // Labels are built per use, only when observed, so an
                // unobserved call neither constructs nor stores them.
                let labels = || {
                    CallLabels::new(
                        path,
                        Some(channel.local.authority.as_str()),
                        CallRole::Client,
                    )
                };
                let call_start = crate::telemetry::call_started_at(observer.is_some());
                if let Some(obs) = &observer {
                    obs.on_call_start(&labels());
                }
                let owned_labels = observer.as_ref().map(|_| Box::new(labels().to_owned()));
                let mut call_guard = CallGuard::new(
                    observer.clone(),
                    owned_labels.clone(),
                    call_start,
                    channel.inner.channelz.entry(),
                );

                if let Err(status) = prepared {
                    call_guard.reject(RejectionReason::ClientInterceptor, &status);
                    return Err(status);
                }
                let deadline = deadline_from(req_timeout);
                let _ = match remaining_timeout(deadline) {
                    Ok(t) => t,
                    Err(status) => {
                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                        return Err(status);
                    }
                };
                let user_agent = ua.unwrap_or_else(|| channel.local.user_agent.clone());
                let _permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                if let Some(obs) = &observer {
                    obs.on_attempt_start(&AttemptLabels::new(labels(), 1));
                }
                // The attempt guard has no effects without an observer,
                // so it (and its start clock) only exists then.
                let mut attempt_guard = observer.as_ref().map(|_| {
                    AttemptGuard::new(
                        observer.clone(),
                        owned_labels.clone(),
                        1,
                        std::time::Instant::now(),
                    )
                });
                let queue_start = observer.as_ref().map(|_| tokio::time::Instant::now());
                let tap = channel
                    .local
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
                        if let (Some(obs), Some(queue_start)) = (&observer, &queue_start) {
                            obs.on_queue_wait(&labels(), queue_start.elapsed());
                        }
                        if let Some(tap) = &tap {
                            tap.log_client_header(
                                &md,
                                path,
                                channel.local.authority.as_str(),
                                req_timeout,
                            );
                        }
                        opened
                    }
                    Err(status) => {
                        if cancel_rx.is_cancelled() {
                            cancel_attempt(&mut attempt_guard, CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            cancel_attempt(
                                &mut attempt_guard,
                                CancellationReason::DeadlineExceeded,
                            );
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        } else {
                            reject_attempt(
                                &mut attempt_guard,
                                RejectionReason::SetupFailed,
                                &status,
                            );
                            call_guard.reject(RejectionReason::SetupFailed, &status);
                        }
                        finish_attempt(&mut attempt_guard, &status);
                        return Err(status);
                    }
                };
                let crate::client::call::Opened {
                    load: _load,
                    lease: _lease,
                    driver: _driver,
                    resp_fut,
                    mut send,
                    channelz_socket,
                } = opened;
                let budget = channel.local.byte_budget.clone();
                // Not read again after headers are sent. Ending them here keeps
                // them out of the state polled while the unary response arrives.
                drop(user_agent);
                drop(md);
                let mut rx = rx;
                // A closed upload with no deadline is the common case. Queue it
                // before waiting so this future does not carry the pump.
                let response: Result<Response<Resp>, Status> =
                    if deadline.is_none() && rx.producer_finished() {
                        if let Some(items) = rx.drain_closed_channel() {
                            match write_closed_upload(
                                &mut send,
                                items,
                                wire,
                                &budget,
                                &tap,
                                channelz_socket,
                            ) {
                                Ok(ClosedUpload::Sent | ClosedUpload::PeerFinished) => {
                                    await_closed_client_response(
                                        resp_fut,
                                        send,
                                        cancel_rx.clone(),
                                        wire,
                                        tap.as_ref(),
                                        channelz_socket,
                                    )
                                    .await
                                }
                                Ok(ClosedUpload::Rest(rest)) => {
                                    Box::pin(send_client_rest(
                                        resp_fut,
                                        send,
                                        rest,
                                        cancel_rx.clone(),
                                        wire,
                                        tap.clone(),
                                        channelz_socket,
                                    ))
                                    .await
                                }
                                Err(status) => Err(status),
                            }
                        } else {
                            Box::pin(run_client_stream(
                                resp_fut,
                                send,
                                rx,
                                cancel_rx.clone(),
                                wire,
                                deadline,
                                budget,
                                tap.as_ref(),
                                channelz_socket,
                            ))
                            .await
                        }
                    } else {
                        Box::pin(run_client_stream(
                            resp_fut,
                            send,
                            rx,
                            cancel_rx.clone(),
                            wire,
                            deadline,
                            budget,
                            tap.as_ref(),
                            channelz_socket,
                        ))
                        .await
                    };
                let response: Response<Resp> = match response {
                    Ok(r) => r,
                    Err(status) => {
                        if cancel_rx.is_cancelled() {
                            cancel_attempt(&mut attempt_guard, CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            cancel_attempt(
                                &mut attempt_guard,
                                CancellationReason::DeadlineExceeded,
                            );
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        }
                        finish_attempt(&mut attempt_guard, &status);
                        call_guard.finish(&status);
                        return Err(status);
                    }
                };
                let final_res = channel.apply_response_hooks(path, response);
                match &final_res {
                    Ok(_) => {
                        if let Some(obs) = &observer {
                            obs.on_bytes_received(&labels(), 0);
                        }
                        finish_attempt(&mut attempt_guard, &Status::ok());
                        call_guard.finish(&Status::ok());
                    }
                    Err(status) => {
                        finish_attempt(&mut attempt_guard, status);
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
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
    {
        self.bidi_with_input(path, req, |wire, compress| {
            let (tx, rx) = Streaming::channel(self.local.config.stream_buffer_size());
            (tx.with_limits(wire.limits).with_compress(compress), rx)
        })
    }

    #[cfg(feature = "tower")]
    pub(crate) fn bidi_from_stream<Req, Resp, S>(
        &self,
        path: &'static str,
        request: Request<S>,
    ) -> Call<Response<Streaming<Resp>>>
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
        S: futures_core::Stream<Item = Result<Req, Status>> + Send + 'static,
    {
        let (stream, parts) = request.into_message_and_parts();
        self.bidi_with_input(
            path,
            Request::<()>::from_message_and_parts((), parts),
            |wire, compress| {
                (
                    (),
                    Streaming::from_input(
                        stream,
                        wire.limits,
                        compress,
                        self.local.config.stream_buffer_size(),
                    ),
                )
            },
        )
        .1
    }

    fn bidi_with_input<Req, Resp, Input>(
        &self,
        path: &'static str,
        mut req: Request<()>,
        input: impl FnOnce(Wire, bool) -> (Input, Streaming<Req>),
    ) -> (Input, Call<Response<Streaming<Resp>>>)
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send + 'static,
    {
        let channel = self.clone();
        let state = channel.local.service_config.get();
        let prepared = channel.prepare_outbound(path, &mut req, state.as_deref());
        let wire = channel.wire_for(path, state.as_deref());
        let (tx, rx) = input(wire, req.compress());
        let cancel = crate::rpc_cancel::RpcCancel::new();
        let cancel_rx = cancel.clone();
        let reset = cancel.clone();
        let observer = channel.local.observer.clone();
        // Destructure before the async block so the envelope is not stored
        // in the future; only the parts cross awaits.
        let wait = req.wait_for_ready();
        let req_timeout = req.timeout();
        // Field moves, not `into_parts`: passing the envelope by value copies it.
        let md = req.metadata;
        let compress = req.compress.unwrap_or(false);
        let ua = req.user_agent;
        let call = Call::new(
            cancel,
            Box::pin(async move {
                // Labels are built per use, only when observed, so an
                // unobserved call neither constructs nor stores them.
                let labels = || {
                    CallLabels::new(
                        path,
                        Some(channel.local.authority.as_str()),
                        CallRole::Client,
                    )
                };
                let call_start = crate::telemetry::call_started_at(observer.is_some());
                if let Some(obs) = &observer {
                    obs.on_call_start(&labels());
                }
                let owned_labels = observer.as_ref().map(|_| Box::new(labels().to_owned()));
                let mut call_guard = CallGuard::new(
                    observer.clone(),
                    owned_labels.clone(),
                    call_start,
                    channel.inner.channelz.entry(),
                );

                if let Err(status) = prepared {
                    call_guard.reject(RejectionReason::ClientInterceptor, &status);
                    return Err(status);
                }
                let deadline = deadline_from(req_timeout);
                let _ = match remaining_timeout(deadline) {
                    Ok(t) => t,
                    Err(status) => {
                        call_guard.cancel(CancellationReason::DeadlineExceeded);
                        return Err(status);
                    }
                };
                let user_agent = ua.unwrap_or_else(|| channel.local.user_agent.clone());
                let permit = match channel.take_rpc_slot() {
                    Ok(p) => p,
                    Err(status) => {
                        call_guard.reject(RejectionReason::ConcurrencyLimit, &status);
                        return Err(status);
                    }
                };
                if let Some(obs) = &observer {
                    obs.on_attempt_start(&AttemptLabels::new(labels(), 1));
                }
                // The attempt guard has no effects without an observer,
                // so it (and its start clock) only exists then.
                let mut attempt_guard = observer.as_ref().map(|_| {
                    AttemptGuard::new(
                        observer.clone(),
                        owned_labels.clone(),
                        1,
                        std::time::Instant::now(),
                    )
                });
                let queue_start = observer.as_ref().map(|_| tokio::time::Instant::now());
                let tap = channel
                    .local
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
                        if let (Some(obs), Some(queue_start)) = (&observer, &queue_start) {
                            obs.on_queue_wait(&labels(), queue_start.elapsed());
                        }
                        if let Some(tap) = &tap {
                            tap.log_client_header(
                                &md,
                                path,
                                channel.local.authority.as_str(),
                                req_timeout,
                            );
                        }
                        opened
                    }
                    Err(status) => {
                        if cancel_rx.is_cancelled() {
                            cancel_attempt(&mut attempt_guard, CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            cancel_attempt(
                                &mut attempt_guard,
                                CancellationReason::DeadlineExceeded,
                            );
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        } else {
                            reject_attempt(
                                &mut attempt_guard,
                                RejectionReason::SetupFailed,
                                &status,
                            );
                            call_guard.reject(RejectionReason::SetupFailed, &status);
                        }
                        finish_attempt(&mut attempt_guard, &status);
                        return Err(status);
                    }
                };
                let crate::client::call::Opened {
                    load: _load,
                    lease,
                    driver,
                    resp_fut,
                    mut send,
                    channelz_socket,
                } = opened;
                let budget = channel.local.byte_budget.clone();
                // The header wait polls this future several times. Channel,
                // the request metadata, and the user-agent are not read again
                // after the stream is open, so drop them before that wait.
                let interceptors = std::sync::Arc::clone(&channel.local.response_interceptors);
                drop(channel);
                drop(user_agent);
                drop(md);
                // The common case is a closed upload and no deadline. Await
                // only that small wait here so the call future does not also
                // carry the colder upload state machines.
                let mut rx = rx;
                let bidi_result = if deadline.is_none() && rx.producer_finished() {
                    if let Some(items) = rx.drain_closed_channel() {
                        match write_closed_upload(
                            &mut send,
                            items,
                            wire,
                            &budget,
                            &tap,
                            channelz_socket,
                        ) {
                            Ok(ClosedUpload::Sent | ClosedUpload::PeerFinished) => {
                                let ready = wait_ready_bidi_http(
                                    resp_fut,
                                    send,
                                    cancel_rx.clone(),
                                    tap.clone(),
                                )
                                .await;
                                finish_ready_bidi(ready, wire, tap.clone(), channelz_socket)
                            }
                            Ok(ClosedUpload::Rest(rest)) => {
                                box_bidi_rest::<Req, Resp>(
                                    resp_fut,
                                    send,
                                    rest,
                                    cancel_rx.clone(),
                                    wire,
                                    tap.clone(),
                                    deadline,
                                    channelz_socket,
                                )
                                .await
                            }
                            Err(status) => Err(status),
                        }
                    } else {
                        // Cold: the upload is not a closed channel. Box it so
                        // the call future's hot state does not include it.
                        Box::pin(run_bidi(
                            resp_fut,
                            send,
                            rx,
                            cancel_rx.clone(),
                            wire,
                            tap.clone(),
                            deadline,
                            budget.clone(),
                            channelz_socket,
                        ))
                        .await
                    }
                } else {
                    Box::pin(run_bidi(
                        resp_fut,
                        send,
                        rx,
                        cancel_rx.clone(),
                        wire,
                        tap.clone(),
                        deadline,
                        budget,
                        channelz_socket,
                    ))
                    .await
                };
                let response = match bidi_result {
                    Ok(resp) => resp,
                    Err(status) => {
                        if cancel_rx.is_cancelled() {
                            cancel_attempt(&mut attempt_guard, CancellationReason::CallerCancelled);
                            call_guard.cancel(CancellationReason::CallerCancelled);
                        } else if status.code() == Code::DeadlineExceeded {
                            cancel_attempt(
                                &mut attempt_guard,
                                CancellationReason::DeadlineExceeded,
                            );
                            call_guard.cancel(CancellationReason::DeadlineExceeded);
                        }
                        finish_attempt(&mut attempt_guard, &status);
                        call_guard.finish(&status);
                        return Err(status);
                    }
                };
                let response = response.with_static_path(path);
                let response = if interceptors.is_empty() {
                    response
                } else {
                    crate::interceptor::intercept_response_all(response, &interceptors)?
                };
                finish_attempt(&mut attempt_guard, &Status::ok());
                call_guard.finish(&Status::ok());
                Ok(pool::attach_conn(
                    response,
                    lease,
                    driver,
                    Some(reset),
                    permit,
                    channelz_socket,
                ))
            }),
        );
        (tx, call)
    }
}
