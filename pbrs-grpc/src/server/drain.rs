//! Response draining: handler race, cancel guards, and wire writers.

use crate::codec::CodecMessage;
use crate::config::Wire;
use crate::limits::ByteBudgetTracker;
use crate::metadata::Metadata;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{CallLabels, LifecycleObserver};
use crate::transport::{SendResponse, SendStream, h2 as backend};
use crate::wire::{
    OutBatch, encode_msg, grpc_trailers, let_producer_catch_up, preferred_codec,
    select_outbound_codec, select_stream_codec, send_frame, send_ok_headers, send_trailers_only,
};
use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;
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
pub(crate) async fn wait_client_reset(respond: &mut backend::SendResponse) -> Status {
    drop(std::future::poll_fn(|cx| respond.poll_reset(cx)).await);
    Status::cancelled()
}

/// Race the handler against a client reset, signalling spawned work on RST.
///
/// After signalling, poll the handler once so a body awaiting
/// [`Request::cancelled`] can finish. A handler that ignores cancel stays
/// `Pending` and is dropped, the same as before.
pub(crate) async fn run_handler<T>(
    respond: &mut backend::SendResponse,
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
/// The polling closure owns the guard while borrowing the pinned writer.
/// Taking the guard on completion signals cancellation before the writer is
/// dropped. Capturing the initial state whole also preserves that order if
/// the returned future is dropped before its first poll.
pub(crate) fn hold_cancel<F: Future<Output = ()>>(
    cancel: CancelOnDrop,
    write: F,
) -> impl Future<Output = ()> {
    let held = HoldCancel {
        cancel: Some(cancel),
        write,
    };
    async move {
        let (cancel, write) = held.into_parts();
        tokio::pin!(write);
        let mut cancel = cancel;
        poll_fn(move |cx| match write.as_mut().poll(cx) {
            Poll::Ready(()) => {
                cancel.take();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        })
        .await;
    }
}

/// Initial owned state; declaration order keeps cancellation before writer drop.
struct HoldCancel<F> {
    cancel: Option<CancelOnDrop>,
    write: F,
}

impl<F> HoldCancel<F> {
    fn into_parts(self) -> (Option<CancelOnDrop>, F) {
        (self.cancel, self.write)
    }
}

/// A handler result plus the response channel it still has to be written to.
pub(crate) struct Prepared<T> {
    pub(crate) respond: backend::SendResponse,
    pub(crate) wire: Wire,
    /// The RPC's deadline, shared by the handler, the inbound stream, and the
    /// response writer, so no stage can outlive it.
    pub(crate) deadline: Option<tokio::time::Instant>,
    pub(crate) outcome: Result<T, Status>,
    pub(crate) prefer_gzip: bool,
    pub(crate) peer_accepts_gzip: bool,
    pub(crate) peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")]
    pub(crate) peer_accepts_zstd: bool,
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
    #[cfg(feature = "grpc-web")]
    pub(crate) web: Option<crate::web::Mode>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal write helper with observer and options"
)]
pub(crate) async fn send_unary_response<Resp: CodecMessage>(
    response: Response<Resp>,
    mut respond: backend::SendResponse,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) {
    let (msg, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let frame = match encode_msg(&msg, codec, wire.limits, wire.gzip_level) {
        Ok(frame) => frame,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let permit = match budget.acquire(frame.total_len()) {
        Ok(p) => p,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, codec, wire.accept_gzip) else {
        return;
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
        for seg in frame.segments() {
            tap.log_written(seg);
        }
    }

    if let Some(obs) = observer {
        obs.on_bytes_sent(call_labels, frame.total_len());
    }
    if let Some(socket) = channelz_socket {
        crate::channelz::Registry::global().note_messages(socket, true, 1);
    }
    send_frame(&mut send, frame, false, wire.send_buffer)
        .await
        .ok();
    drop(permit);
    let mut status = Status::ok();
    if !trailers.is_empty() {
        *status.metadata_mut() = trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal write helper with observer and options"
)]
pub(crate) async fn send_web_unary_response<Resp: CodecMessage>(
    response: Response<Resp>,
    mut respond: backend::SendResponse,
    web: crate::web::Mode,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) {
    let (msg, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let frame = match encode_msg(&msg, codec, wire.limits, wire.gzip_level) {
        Ok(frame) => frame,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            crate::web::send_trailers_only(&mut respond, web, status, &Metadata::new());
            return;
        }
    };
    let permit = match budget.acquire(frame.total_len()) {
        Ok(p) => p,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            crate::web::send_trailers_only(&mut respond, web, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) =
        crate::web::send_ok_headers(&mut respond, web, &headers, codec, wire.accept_gzip)
    else {
        return;
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
        for seg in frame.segments() {
            tap.log_written(seg);
        }
    }
    if let Some(obs) = observer {
        obs.on_bytes_sent(call_labels, frame.total_len());
    }
    if let Some(socket) = channelz_socket {
        crate::channelz::Registry::global().note_messages(socket, true, 1);
    }
    let mut text = crate::web::TextEncoder::new();
    crate::web::send_frame(&mut send, web, &mut text, frame, wire.send_buffer)
        .await
        .ok();
    drop(permit);
    let mut status = Status::ok();
    if !trailers.is_empty() {
        *status.metadata_mut() = trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    crate::web::send_trailers(&mut send, web, &mut text, &status, wire.send_buffer)
        .await
        .ok();
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn send_stream_response<Resp: CodecMessage + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: backend::SendResponse,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Status {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, codec, wire.accept_gzip) else {
        return Status::unavailable("failed to send response headers");
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
    }
    let ok_trailers = trailers;
    let mut status = Status::ok();
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
                peer_accepts_deflate,
                #[cfg(feature = "zstd")]
                peer_accepts_zstd,
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
                peer_accepts_deflate,
                #[cfg(feature = "zstd")]
                peer_accepts_zstd,
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
    if status.is_ok() && !ok_trailers.is_empty() {
        *status.metadata_mut() = ok_trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
    status
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn send_web_stream_response<Resp: CodecMessage + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: backend::SendResponse,
    web: crate::web::Mode,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Status {
    let (mut stream, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let Ok(mut send) =
        crate::web::send_ok_headers(&mut respond, web, &headers, codec, wire.accept_gzip)
    else {
        return Status::unavailable("failed to send response headers");
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
    }
    let ok_trailers = trailers;
    let mut status = Status::ok();
    let mut text = crate::web::TextEncoder::new();
    let drained = if web.is_text() {
        match deadline {
            None => {
                drain_to_web_text(
                    &mut stream,
                    &mut send,
                    web,
                    &mut text,
                    wire,
                    compress,
                    prefer_gzip,
                    peer_accepts_gzip,
                    peer_accepts_deflate,
                    #[cfg(feature = "zstd")]
                    peer_accepts_zstd,
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
                drain_to_web_text(
                    &mut stream,
                    &mut send,
                    web,
                    &mut text,
                    wire,
                    compress,
                    prefer_gzip,
                    peer_accepts_gzip,
                    peer_accepts_deflate,
                    #[cfg(feature = "zstd")]
                    peer_accepts_zstd,
                    budget,
                    observer,
                    call_labels,
                    tap.cloned(),
                    channelz_socket,
                ),
            )
            .await
            .unwrap_or_else(|_| Err(DrainError::Producer(Status::deadline_exceeded()))),
        }
    } else {
        match deadline {
            None => {
                drain_to_wire(
                    &mut stream,
                    &mut send,
                    wire,
                    compress,
                    prefer_gzip,
                    peer_accepts_gzip,
                    peer_accepts_deflate,
                    #[cfg(feature = "zstd")]
                    peer_accepts_zstd,
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
                    peer_accepts_deflate,
                    #[cfg(feature = "zstd")]
                    peer_accepts_zstd,
                    budget,
                    observer,
                    call_labels,
                    tap.cloned(),
                    channelz_socket,
                ),
            )
            .await
            .unwrap_or_else(|_| Err(DrainError::Producer(Status::deadline_exceeded()))),
        }
    };
    if let Err(err) = drained {
        match err {
            DrainError::Transport => return Status::unavailable("transport closed during stream"),
            DrainError::Producer(producer) => status = producer,
        }
    }
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    if status.is_ok() && !ok_trailers.is_empty() {
        *status.metadata_mut() = ok_trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    crate::web::send_trailers(&mut send, web, &mut text, &status, wire.send_buffer)
        .await
        .ok();
    status
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn drain_to_web_text<Resp: CodecMessage + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut backend::SendStream,
    web: crate::web::Mode,
    text: &mut crate::web::TextEncoder,
    wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Result<(), DrainError> {
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    loop {
        let item = tokio::select! {
            biased;
            reset = poll_fn(|cx| send.poll_reset(cx)) => {
                drop(reset);
                return Err(DrainError::Transport);
            }
            item = stream.next_framed() => item,
        };
        let item = match item {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(status) => return Err(DrainError::Producer(status)),
        };
        let codec = select_stream_codec(item.compressed, envelope, prefer_gzip, negotiated);
        let frame = encode_msg(&item.message, codec, wire.limits, wire.gzip_level)
            .map_err(DrainError::Producer)?;
        let permit = budget
            .acquire(frame.total_len())
            .map_err(DrainError::Producer)?;
        if let Some(tap) = &tap {
            for seg in frame.segments() {
                tap.log_written(seg);
            }
        }
        if let Some(obs) = observer {
            obs.on_bytes_sent(call_labels, frame.total_len());
        }
        if let Some(socket) = channelz_socket {
            crate::channelz::Registry::global().note_messages(socket, true, 1);
        }
        crate::web::send_frame(send, web, text, frame, wire.send_buffer)
            .await
            .map_err(|_| DrainError::Transport)?;
        drop(permit);
    }
    Ok(())
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
    send: &mut backend::SendStream,
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
pub(crate) async fn drain_to_wire<Resp: CodecMessage + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut backend::SendStream,
    mut wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Result<(), DrainError> {
    // Negotiated once per response: every frame shares the `grpc-encoding`
    // the headers advertised, so per-message selection only decides the
    // Compressed-Flag.
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    wire.send_codec = negotiated.unwrap_or_default();
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
                select_stream_codec(item.compressed, envelope, prefer_gzip, negotiated).is_some();
            let frame_len = 5 + item.message.encoded_len();
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

#[cfg(test)]
#[path = "sv09_cancel_tests.rs"]
mod sv09_cancel_tests;
