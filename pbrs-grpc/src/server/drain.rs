//! Response draining: handler race, cancel guards, and wire writers.

use crate::config::Wire;
use crate::limits::ByteBudgetTracker;
use crate::metadata::Metadata;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{CallLabels, LifecycleObserver};
use crate::wire::{
    OutBatch, encode_msg, grpc_trailers, gzip_outbound, gzip_stream_frame, let_producer_catch_up,
    send_bytes, send_ok_headers, send_trailers_only,
};
use bytes::Bytes;
use pbrs::Serialize;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::watch;

/// Wake spawned work as soon as the server deadline wins, not after trailers.
pub(crate) fn notify_deadline<T>(outcome: &Result<T, Status>, cancel: &watch::Sender<bool>) {
    if matches!(outcome, Err(s) if s.code() == Code::DeadlineExceeded) {
        cancel.send(true).ok();
    }
}

/// Resolve when the client `RST_STREAM`s this RPC.
///
/// Unary handlers that have already read the request (and streaming handlers
/// that are not currently reading) would otherwise run to completion after
/// the caller has gone. `SendResponse::poll_reset` sees the reset without
/// needing the request body.
pub(crate) async fn wait_client_reset(respond: &mut h2::server::SendResponse<Bytes>) -> Status {
    drop(std::future::poll_fn(|cx| respond.poll_reset(cx)).await);
    Status::cancelled()
}

/// Race the handler against a client reset, signalling spawned work on RST.
///
/// After signalling, poll the handler once so a body awaiting
/// [`Request::cancelled`] can finish. A handler that ignores cancel stays
/// `Pending` and is dropped, the same as before.
pub(crate) async fn run_handler<T>(
    respond: &mut h2::server::SendResponse<Bytes>,
    on_reset: watch::Sender<bool>,
    handler: impl Future<Output = Result<T, Status>>,
    tap: Option<&crate::binlog::CallLogger>,
) -> Result<T, Status> {
    tokio::pin!(handler);
    tokio::select! {
        biased;
        result = &mut handler => result,
        gone = wait_client_reset(respond) => {
            on_reset.send(true).ok();
            if let Some(tap) = tap {
                tap.log_cancel();
            }
            match poll_fn(|cx| Poll::Ready(handler.as_mut().poll(cx))).await {
                Poll::Ready(result) => result,
                Poll::Pending => Err(gone),
            }
        }
    }
}

/// Marks [`Request::cancelled`] when this RPC is fully written or rejected.
///
/// Lives until the response is on the wire so a streaming producer spawned
/// before the handler returns is not cancelled at return — only when the
/// stream drains, the client resets, or the deadline fires.
pub(crate) struct CancelOnDrop(pub(crate) watch::Sender<bool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.send(true).ok();
    }
}

/// Keep [`CancelOnDrop`] alive across `write`.
///
/// Locals not named in the write future can be dropped at `.await` (NLL).
/// A manual future holds the guard as a field so it cannot drop until `write`
/// completes — `write.await; drop(cancel)` is not enough.
pub(crate) fn hold_cancel<F: Future<Output = ()>>(cancel: CancelOnDrop, write: F) -> HoldCancel<F> {
    HoldCancel {
        write: Box::pin(write),
        cancel: Some(cancel),
    }
}

/// [`hold_cancel`]'s state: poll `write`, drop the guard only when it finishes.
pub(crate) struct HoldCancel<F> {
    cancel: Option<CancelOnDrop>,
    write: Pin<Box<F>>,
}

impl<F: Future<Output = ()>> Future for HoldCancel<F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.write.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.cancel.take();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A handler result plus the response channel it still has to be written to.
pub(crate) struct Prepared<T> {
    pub(crate) respond: h2::server::SendResponse<Bytes>,
    pub(crate) wire: Wire,
    /// The RPC's deadline, shared by the handler, the inbound stream, and the
    /// response writer, so no stage can outlive it.
    pub(crate) deadline: Option<tokio::time::Instant>,
    pub(crate) outcome: Result<T, Status>,
    pub(crate) prefer_gzip: bool,
    pub(crate) peer_accepts_gzip: bool,
    pub(crate) cancel: CancelOnDrop,
    /// Kernel-stamped onto the handler [`Response`] before `on_response`.
    pub(crate) path: Option<String>,
    pub(crate) gzip_level: u32,
    pub(crate) timeout: Option<Duration>,
    pub(crate) peer_timeout: Option<Duration>,
    pub(crate) rpc_timeout: Option<Duration>,
    pub(crate) budget: ByteBudgetTracker,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) call_start: tokio::time::Instant,
    pub(crate) binlog: Option<crate::binlog::CallLogger>,
    /// Channelz server owning this RPC, for call counters.
    pub(crate) channelz_server: Option<crate::channelz::ServerId>,
    /// Channelz socket serving this RPC, for stream/message counters.
    pub(crate) channelz_socket: Option<crate::channelz::SocketId>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal write helper with observer and options"
)]
pub(crate) async fn send_unary_response<Resp: Serialize>(
    response: Response<Resp>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) {
    let (msg, headers, trailers, compress) = response.split();
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let frame = match encode_msg(&msg, gzip, wire.limits, wire.gzip_level) {
        Ok(frame) => frame,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let permit = match budget.acquire(frame.len()) {
        Ok(p) => p,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip, wire.accept_gzip) else {
        return;
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
        tap.log_written(&frame);
    }
    if let Some(obs) = observer {
        obs.on_bytes_sent(call_labels, frame.len());
    }
    if let Some(socket) = channelz_socket {
        crate::channelz::Registry::global().note_messages(socket, true, 1);
    }
    send_bytes(&mut send, frame, false, wire.send_buffer)
        .await
        .ok();
    drop(permit);
    let mut status = Status::new(Code::Ok, "");
    *status.metadata_mut() = trailers;
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn send_stream_response<Resp: Serialize + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Status {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip, wire.accept_gzip) else {
        return Status::unavailable("failed to send response headers");
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
    }
    let mut status = Status::from_code(Code::Ok);
    *status.metadata_mut() = trailers;
    // The deadline has to cover the whole response, not just the handler
    // future: a producer that stops early because *its* deadline expired must
    // not be reported as a clean end of stream.
    let drained = match deadline {
        None => {
            drain_to_wire(
                &mut stream,
                &mut send,
                wire,
                compress,
                prefer_gzip,
                peer_accepts_gzip,
                budget,
                observer,
                call_labels,
                tap.cloned(),
                channelz_socket,
            )
            .await
        }
        Some(at) => tokio::time::timeout_at(
            at,
            drain_to_wire(
                &mut stream,
                &mut send,
                wire,
                compress,
                prefer_gzip,
                peer_accepts_gzip,
                budget,
                observer,
                call_labels,
                tap.cloned(),
                channelz_socket,
            ),
        )
        .await
        .unwrap_or_else(|_| Err(DrainError::Producer(Status::deadline_exceeded()))),
    };
    if let Err(err) = drained {
        // A transport failure cannot be reported; a producer failure becomes
        // the stream's trailing status.
        match err {
            DrainError::Transport => return Status::unavailable("transport closed during stream"),
            DrainError::Producer(producer) => status = producer,
        }
    }
    // If the deadline elapsed, the RPC did not finish in time, however the
    // drain ended. A handler reading its request stream sees the deadline as an
    // error on the read and will usually just stop producing, which would
    // otherwise be indistinguishable from a clean end of stream.
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
    status
}

/// Why a stream stopped before its clean end.
pub(crate) enum DrainError {
    /// The wire is gone, so no status can be delivered.
    Transport,
    /// The handler ended the stream with a status.
    Producer(Status),
}

pub(crate) async fn flush_queued_before_error(
    batch: &mut OutBatch,
    send: &mut h2::SendStream<Bytes>,
    permits: &mut Vec<crate::limits::BytePermit>,
    status: Status,
) -> Result<(), DrainError> {
    // A burst can contain successful replies followed by a producer error.
    batch.flush(send).await.map_err(|_| DrainError::Transport)?;
    permits.clear();
    Err(DrainError::Producer(status))
}

/// Copy every message from `stream` onto `send`, batching each burst.
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream drain helper with observer and options"
)]
pub(crate) async fn drain_to_wire<Resp: Serialize + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut h2::SendStream<Bytes>,
    wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Result<(), DrainError> {
    let mut batch = OutBatch::new(wire);
    if let Some(tap) = tap {
        batch.set_tap(tap);
    }
    let mut items = Vec::with_capacity(OutBatch::BURST);
    let mut permits = Vec::with_capacity(OutBatch::BURST);
    loop {
        items.clear();
        // A client RST while we wait for the next message must abort: a
        // producer that is itself waiting (Health Watch, a timer) will not
        // send, so the write path would never see the reset.
        let n = tokio::select! {
            biased;
            reset = poll_fn(|cx| send.poll_reset(cx)) => {
                drop(reset);
                return Err(DrainError::Transport);
            }
            n = stream.recv_many(&mut items, OutBatch::BURST) => n,
        };
        if n == 0 {
            break;
        }
        // More than one message queued means the producer is running ahead of
        // the network and is bounded by its channel depth, so one scheduling
        // turn lets it top the queue up and doubles the write size. Exactly one
        // means it is not ahead — a request/response stream, say — and must not
        // pay a turn of latency for nothing.
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 {
            let_producer_catch_up().await;
            stream.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let mut item = match item {
                Ok(item) => item,
                Err(status) => {
                    return flush_queued_before_error(&mut batch, send, &mut permits, status).await;
                }
            };
            item.compressed =
                gzip_stream_frame(item.compressed, envelope, prefer_gzip, peer_accepts_gzip);
            let frame_len = 5 + item.message.serialized_len();
            let permit = match budget.acquire(frame_len) {
                Ok(permit) => permit,
                Err(status) => {
                    return flush_queued_before_error(&mut batch, send, &mut permits, status).await;
                }
            };
            permits.push(permit);
            if let Err(status) = batch.encode(item) {
                return flush_queued_before_error(&mut batch, send, &mut permits, status).await;
            }
            if let Some(obs) = observer {
                obs.on_bytes_sent(call_labels, frame_len);
            }
            if let Some(socket) = channelz_socket {
                crate::channelz::Registry::global().note_messages(socket, true, 1);
            }
            if batch.is_full() {
                batch.flush(send).await.map_err(|_| DrainError::Transport)?;
                permits.clear();
            }
        }
        if !batch.is_full() {
            batch.flush(send).await.map_err(|_| DrainError::Transport)?;
            permits.clear();
        }
    }
    batch.flush(send).await.map_err(|_| DrainError::Transport)?;
    permits.clear();
    Ok(())
}
