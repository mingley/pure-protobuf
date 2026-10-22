//! Outbound writes: headers, trailers, rejects, request pump.

use super::encode::SegFrame;
use super::headers::{
    APPLICATION_GRPC, GRPC_ACCEPT_ENCODING, GRPC_ENCODING, GRPC_MESSAGE, GRPC_RETRY_PUSHBACK_MS,
    GRPC_STATUS, GRPC_STATUS_DETAILS_BIN, RequestReject, STATUS_OK, accept_encoding_value,
    encoding_value,
};
use super::out_batch::{OutBatch, let_producer_catch_up};
use crate::codec::CodecMessage;
use crate::compression::CompressionAlgorithm;
use crate::config::Wire;
use crate::metadata::Metadata;
use crate::status::{Code, Pushback, Status};
use crate::stream::Streaming;
use crate::transport::{Reason, SendResponse, SendStream, h2 as backend};
use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use bytes::Bytes;
use futures_util::future::Either;
use http::{HeaderMap, HeaderValue, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

pub(crate) async fn wait_capacity(send: &mut backend::SendStream, n: usize) -> Result<(), Status> {
    if send.capacity() >= n {
        return Ok(());
    }
    send.reserve_capacity(n);
    while send.capacity() == 0 {
        match std::future::poll_fn(|cx| send.poll_capacity(cx)).await {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Status::from_h2_send(e)),
            None => return Err(Status::stream_closed()),
        }
    }
    Ok(())
}

/// Queue one gRPC frame in chunks no larger than the HTTP/2 send budget.
///
/// Small frames that fit in the bounded send buffer avoid a capacity poll. Large frames cannot wait
/// for full-frame credit when the configured send buffer or peer window is
/// smaller than the frame, so each chunk waits for only one byte of credit and
/// uses whatever is available. `Bytes` slices share the original allocation.
pub(crate) async fn send_bytes(
    send: &mut backend::SendStream,
    mut frame: Bytes,
    end: bool,
    send_buffer: usize,
) -> Result<(), Status> {
    if send_buffer == 0 {
        return Err(Status::invalid_argument(
            "HTTP/2 send buffer size must be nonzero",
        ));
    }
    if frame.is_empty() {
        return send.send_data(frame, end).map_err(Status::from_h2_send);
    }
    while !frame.is_empty() {
        // Buffer space and peer credit are different limits. Keep the small
        // write synchronous while space remains; a paused peer must still
        // stop production when the configured buffer is full.
        if frame.len() <= send_buffer {
            match send.try_send_data(frame, end) {
                Ok(result) => return result.map_err(Status::from_h2_send),
                Err(unsent) => frame = unsent,
            }
        }
        wait_capacity(send, frame.len().min(send_buffer)).await?;
        let n = frame.len().min(send.capacity()).min(send_buffer);
        let last = n == frame.len();
        send.send_data(frame.slice(..n), end && last)
            .map_err(Status::from_h2_send)?;
        frame = frame.slice(n..);
    }
    Ok(())
}

/// A frame that fit in the send buffer, or one that still has to wait.
pub(crate) enum ImmediateSend {
    /// `try_send_data` accepted the only segment, or the frame was empty.
    Done(Result<(), Status>),
    /// More than one segment, or the buffer could not take the bytes.
    Blocked(SegFrame),
}

/// Send a one-segment frame without building the async send future.
///
/// The async path polls that future even when the peer already has window.
/// A frame that fits is the steady case; anything else is returned untouched
/// so the caller can wait.
pub(crate) fn send_frame_if_ready(
    send: &mut backend::SendStream,
    frame: SegFrame,
    end: bool,
    send_buffer: usize,
) -> ImmediateSend {
    if send_buffer == 0 {
        return ImmediateSend::Done(Err(Status::invalid_argument(
            "HTTP/2 send buffer size must be nonzero",
        )));
    }
    if frame.seg_count() != 1 || frame.total_len() > send_buffer {
        return ImmediateSend::Blocked(frame);
    }
    // One segment has no tail iterator. `into_segments().next()` built one
    // anyway.
    let bytes = frame.into_first();
    if bytes.is_empty() {
        return ImmediateSend::Done(send.send_data(bytes, end).map_err(Status::from_h2_send));
    }
    match send.try_send_data(bytes, end) {
        Ok(result) => ImmediateSend::Done(result.map_err(Status::from_h2_send)),
        Err(unsent) => ImmediateSend::Blocked(SegFrame::single(unsent)),
    }
}

/// Send one framed message segment by segment (PK-11).
///
/// The concatenated bytes are exactly one gRPC frame; `end` applies to
/// the last segment only, so trailers (or stream end) still follow the
/// whole message.
pub(crate) async fn send_frame(
    send: &mut backend::SendStream,
    frame: SegFrame,
    end: bool,
    send_buffer: usize,
) -> Result<(), Status> {
    let total = frame.seg_count();
    for (i, seg) in frame.into_segments().enumerate() {
        let last = i + 1 == total;
        send_bytes(send, seg, end && last, send_buffer).await?;
    }
    Ok(())
}

/// `grpc-status: 0` and nothing else, from the trailer pool.
pub(crate) fn bare_ok_trailer_map() -> HeaderMap {
    // The queued frame owns these fields across awaits and worker migration.
    crate::h2_backend::header_pool::take(crate::h2_backend::header_pool::Slot::Trailers, || {
        let mut map = HeaderMap::with_capacity(1);
        map.insert(GRPC_STATUS, STATUS_OK);
        map
    })
}

pub(crate) fn grpc_trailers(status: &Status) -> Result<HeaderMap, Status> {
    if status.code() == Code::Ok
        && status.message().is_empty()
        && status.metadata().is_empty()
        && status.details().is_empty()
        && status.retry_pushback().is_none()
    {
        // The overwhelmingly common case: one static header, no formatting.
        return Ok(bare_ok_trailer_map());
    }
    let mut map = HeaderMap::with_capacity(6);
    let code = HeaderValue::from_str(&status.code().to_i32().to_string())
        .map_err(|e| Status::internal(e.to_string()))?;
    map.insert(GRPC_STATUS, code);
    if !status.message().is_empty() {
        let encoded = percent_encode(status.message());
        let val = HeaderValue::from_str(&encoded).map_err(|e| Status::internal(e.to_string()))?;
        map.insert(GRPC_MESSAGE, val);
    }
    match status.retry_pushback() {
        Some(Pushback::Delay(delay)) => {
            let val = HeaderValue::from_str(&delay.as_millis().to_string())
                .map_err(|e| Status::internal(e.to_string()))?;
            map.insert(GRPC_RETRY_PUSHBACK_MS, val);
        }
        Some(Pushback::DoNotRetry) => {
            let val = HeaderValue::from_static("-1");
            map.insert(GRPC_RETRY_PUSHBACK_MS, val);
        }
        None => {}
    }
    if !status.details().is_empty() {
        let encoded = STANDARD_NO_PAD.encode(status.details());
        let val = HeaderValue::from_str(&encoded).map_err(|e| Status::internal(e.to_string()))?;
        map.insert(GRPC_STATUS_DETAILS_BIN, val);
    }
    status.metadata().write_to(&mut map)?;
    Ok(map)
}

/// Answer with headers only, folding `grpc-status` into them.
///
/// This is the "Trailers-Only" response of the gRPC spec, used for errors
/// raised before any message could be produced.
pub(crate) fn send_trailers_only(
    respond: &mut backend::SendResponse,
    status: Status,
    extra_headers: &Metadata,
) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, APPLICATION_GRPC)
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    extra_headers.write_to(res.headers_mut()).ok();
    if let Ok(trailers) = grpc_trailers(&status) {
        for (k, v) in &trailers {
            res.headers_mut().append(k, v.clone());
        }
    }
    respond.send_response(res, true).ok();
}

/// Answer a request this server refuses to process, advertising what it does
/// accept.
///
/// The gRPC spec requires `grpc-accept-encoding` on a rejection caused by an
/// unsupported `grpc-encoding`, so the client knows what to retry with. Sending
/// it on every rejection costs one header and keeps the logic in one place.
pub(crate) fn reject(respond: &mut backend::SendResponse, status: Status, accept_gzip: bool) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, APPLICATION_GRPC)
        .header(GRPC_ACCEPT_ENCODING, accept_encoding_value(accept_gzip))
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    if let Ok(trailers) = grpc_trailers(&status) {
        for (k, v) in &trailers {
            res.headers_mut().append(k, v.clone());
        }
    }
    respond.send_response(res, true).ok();
}

/// Answer [`RequestReject`]: gRPC trailers-only, or a bare HTTP status.
pub(crate) fn reject_request(
    respond: &mut backend::SendResponse,
    err: RequestReject,
    accept_gzip: bool,
) {
    match err {
        RequestReject::Grpc(status) => reject(respond, status, accept_gzip),
        RequestReject::Http(code) => send_http(respond, code),
    }
}

/// HTTP 405/415 for a request that is not gRPC. No `grpc-status`, so an HTTP/2
/// client cannot take this as a successful RPC.
pub(crate) fn send_http(respond: &mut backend::SendResponse, status: StatusCode) {
    let mut builder = Response::builder().status(status);
    if status == StatusCode::METHOD_NOT_ALLOWED {
        builder = builder.header(http::header::ALLOW, "POST");
    }
    let Ok(res) = builder.body(()) else {
        return;
    };
    respond.send_response(res, true).ok();
}

pub(crate) fn send_ok_headers(
    respond: &mut backend::SendResponse,
    md: &Metadata,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
) -> Result<backend::SendStream, Status> {
    // Default responses repeat the same two headers. Clone that block
    // instead of hashing the names again.
    if md.is_empty() && send_codec.is_none() && accept_gzip {
        if crate::h2_backend::header_pool::is_parked(crate::h2_backend::header_pool::Slot::Response)
        {
            return respond
                .send_stock_response(false)
                .map_err(|e| Status::internal(e.to_string()));
        }
        let mut res = crate::h2_backend::server::copy_stock_ok_response();
        // A parked stock response is replayed from the encoder cache. The
        // frame does not need its own copy of the map.
        if !crate::h2_backend::header_pool::is_parked(
            crate::h2_backend::header_pool::Slot::Response,
        ) {
            crate::h2_backend::header_pool::replace_map(
                res.headers_mut(),
                crate::h2_backend::header_pool::take(
                    crate::h2_backend::header_pool::Slot::Response,
                    || crate::wire::headers::standard_response_headers().clone(),
                ),
            );
        }
        crate::h2_backend::header_pool::mark_next_stock(
            crate::h2_backend::header_pool::STOCK_RESPONSE,
        );
        return respond
            .send_response(res, false)
            .map_err(|e| Status::internal(e.to_string()));
    }
    let mut res = Response::new(());
    *res.status_mut() = StatusCode::OK;
    send_ok_headers_slow(respond, res, md, send_codec, accept_gzip)
}

/// Metadata or a compressed response. Out of line so the stock response
/// does not reserve this header table.
#[inline(never)]
fn send_ok_headers_slow(
    respond: &mut backend::SendResponse,
    mut res: http::Response<()>,
    md: &Metadata,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
) -> Result<backend::SendStream, Status> {
    // Exact sizing: the fixed headers plus response metadata. A minimal
    // table for the common no-metadata call, and never a rehash past it.
    let capacity = 2 + md.len() + usize::from(send_codec.is_some());
    crate::h2_backend::header_pool::replace_map(
        res.headers_mut(),
        HeaderMap::with_capacity(capacity),
    );
    let headers = res.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
    headers.insert(GRPC_ACCEPT_ENCODING, accept_encoding_value(accept_gzip));
    if let Some(codec) = send_codec {
        headers.insert(GRPC_ENCODING, encoding_value(codec));
    }
    md.write_to(headers)?;
    respond
        .send_response(res, false)
        .map_err(|e| Status::internal(e.to_string()))
}

/// How [`pump_outbound`] stopped.
pub(crate) enum PumpEnd {
    /// Request half-closed with `end_stream`. Park `SendStream` on cancel.
    HalfClosed,
    /// Already `RST_STREAM` (Call cancel, or a write failed). Do not park.
    Reset,
    /// [`crate::StreamSender::fail`]: the caller `RST_STREAM`s CANCEL after
    /// delivering this status. Bidi holds that RST until the [`crate::Call`]
    /// takes the status (or is dropped), so the Call sees this rather than
    /// `UNAVAILABLE` from h2. A client-streaming [`crate::Call`], or a bidi
    /// [`crate::Call`] that has not yet seen headers, resolves with this
    /// status.
    Failed(Status),
}

/// Encode a client's outbound stream, watching for cancellation.
///
/// The caller keeps `send` so a client-streaming [`crate::CallHandle`] can
/// still `RST_STREAM` after a clean half-close. [`PumpEnd::HalfClosed`]
/// means this pump half-closed; [`PumpEnd::Reset`] means it already reset;
/// [`PumpEnd::Failed`] is [`crate::StreamSender::fail`]. The caller RSTs
/// CANCEL; bidi holds that RST until the Call takes the status.
pub(crate) async fn pump_outbound<T: CodecMessage>(
    send: &mut backend::SendStream,
    mut rx: Streaming<T>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    wire: Wire,
) -> PumpEnd {
    let mut batch = OutBatch::new(wire);
    let mut items = Vec::with_capacity(OutBatch::BURST);
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
                // The Call finished and dropped its sender, and no received
                // stream is holding a clone. Stop watching.
                watch_cancel = false;
                continue;
            }
            taken = rx.recv_many(&mut items, OutBatch::BURST) => taken,
        };
        if taken == 0 {
            // Half-close, carrying whatever is still batched.
            if batch.flush(send).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return PumpEnd::Reset;
            }
            send.send_data(Bytes::new(), true).ok();
            return PumpEnd::HalfClosed;
        }
        // See the note in the server's drain loop: yield only when the caller
        // is demonstrably ahead of the network.
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 && !rx.producer_finished() {
            let_producer_catch_up().await;
            rx.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let item = match item {
                Ok(item) => item,
                Err(status) => {
                    // Do not RST here: the caller delivers this status first
                    // (in-task on client-streaming, oneshot on bidi) so the
                    // Call does not lose it to h2 UNAVAILABLE from the reset.
                    return PumpEnd::Failed(status);
                }
            };
            if let Err(status) = batch.encode(item) {
                return PumpEnd::Failed(status);
            }
            if batch.is_full() && batch.flush(send).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return PumpEnd::Reset;
            }
        }
        if !batch.is_full() && batch.flush(send).await.is_err() {
            send.send_reset(Reason::INTERNAL_ERROR);
            return PumpEnd::Reset;
        }
    }
}

/// After a server-streaming request or a bidi sender is half-closed, keep
/// `send` so a [`crate::CallHandle`] (or dropping the received [`Streaming`]
/// before the end) can still `RST_STREAM`. RecvStream drop is not a last-ref
/// reset while this handle lives. The deadline RSTs too: the [`crate::Call`]
/// is already Ready, so it will not set `cancel_rx`. Client-streaming keeps
/// the send half on the Call task instead.
/// Half-closed bidi upload: `finished` fires when the response stream ends
/// or is dropped, and that must not RST. Cancel and the deadline still do.
pub(crate) fn hold_half_closed_send(
    send: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
    finished: tokio::sync::oneshot::Receiver<()>,
) {
    park_cancel(CancelJob::new(
        send,
        cancel_rx,
        deadline,
        Some(finished),
        true,
    ));
}

pub(crate) fn reset_on_cancel(
    send: backend::SendStream,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
) {
    // Wait keeps the flag alive without keeping a sender alive. Closing every
    // sender releases the parked send half without issuing a reset.
    park_cancel(CancelJob::new(send, cancel_rx, deadline, None, false));
}

pin_project_lite::pin_project! {
    // FuturesUnordered owns this value in its wake-indexed node. A separate
    // boxed future would add another allocation for every parked stream.
    struct CancelJob {
        send: backend::SendStream,
        cancel: crate::rpc_cancel::Wait,
        _sender: Option<crate::rpc_cancel::RpcCancel>,
        finished: Option<tokio::sync::oneshot::Receiver<()>>,
        #[pin]
        deadline: Option<tokio::time::Sleep>,
    }
}

impl CancelJob {
    fn new(
        send: backend::SendStream,
        cancel: crate::rpc_cancel::RpcCancel,
        deadline: Option<tokio::time::Instant>,
        finished: Option<tokio::sync::oneshot::Receiver<()>>,
        keep_sender: bool,
    ) -> Self {
        let wait = cancel.wait();
        Self {
            send,
            cancel: wait,
            _sender: keep_sender.then_some(cancel),
            finished,
            deadline: deadline.map(tokio::time::sleep_until),
        }
    }
}

impl Future for CancelJob {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.project();
        // A clean response completion releases a half-closed send stream even
        // if cancellation becomes ready in the same poll.
        if let Some(finished) = this.finished.as_mut() {
            if Pin::new(finished).poll(cx).is_ready() {
                return Poll::Ready(());
            }
        }
        if Pin::new(&mut *this.cancel).poll(cx).is_ready() {
            if this.cancel.is_cancelled() {
                this.send.send_reset(Reason::CANCEL);
            }
            return Poll::Ready(());
        }
        if let Some(deadline) = this.deadline.as_mut().as_pin_mut() {
            if deadline.poll(cx).is_ready() {
                this.send.send_reset(Reason::CANCEL);
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }
}

const PARKER_CACHE_SIZE: usize = 4;
const PARKER_QUEUE_SIZE: usize = 256;

struct CachedParker {
    runtime: tokio::runtime::Id,
    sender: tokio::sync::mpsc::Sender<CancelJob>,
}

thread_local! {
    // A bounded cache per calling thread avoids a process-global runtime
    // registry and keeps registrations on the runtime that owns their timers.
    static PARKERS: std::cell::RefCell<Vec<CachedParker>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

fn park_cancel(job: CancelJob) {
    let runtime = tokio::runtime::Handle::current();
    let id = runtime.id();
    let sender = PARKERS.try_with(|slot| {
        let mut cached = slot.borrow_mut();
        cached.retain(|entry| !entry.sender.is_closed());
        if let Some(index) = cached.iter().position(|entry| entry.runtime == id) {
            let entry = cached.remove(index);
            let sender = entry.sender.clone();
            cached.push(entry);
            return sender;
        }
        if cached.len() == PARKER_CACHE_SIZE {
            // Closing registration retires this dispatcher only after its
            // already queued and active jobs have completed.
            cached.remove(0);
        }
        let (sender, receiver) = tokio::sync::mpsc::channel(PARKER_QUEUE_SIZE);
        drop(runtime.spawn(run_parker(receiver)));
        cached.push(CachedParker {
            runtime: id,
            sender: sender.clone(),
        });
        sender
    });
    let job = match sender {
        Ok(sender) => match sender.try_send(job) {
            Ok(()) => return,
            Err(error) => error.into_inner(),
        },
        Err(_) => job,
    };
    // Registration is synchronous: a full queue cannot await capacity. The
    // exceptional path gives ownership to this runtime rather than dropping
    // cancellation or blocking its worker. Normal registrations share a task.
    drop(runtime.spawn(job));
}

async fn run_parker(mut receiver: tokio::sync::mpsc::Receiver<CancelJob>) {
    let mut parked = futures_util::stream::FuturesUnordered::new();
    let mut receiving = true;
    loop {
        tokio::select! {
            biased;
            _ = std::future::poll_fn(|cx| poll_parked(&mut parked, cx)), if !parked.is_empty() => {},
            incoming = receiver.recv(), if receiving => {
                match incoming {
                    Some(job) => parked.push(job),
                    None => receiving = false,
                }
            }
            else => break,
        }
    }
}

fn poll_parked<F: Future<Output = ()>>(
    parked: &mut futures_util::stream::FuturesUnordered<F>,
    cx: &mut Context<'_>,
) -> Poll<()> {
    match futures_core::Stream::poll_next(Pin::new(parked), cx) {
        Poll::Ready(Some(())) => Poll::Ready(()),
        Poll::Ready(None) | Poll::Pending => Poll::Pending,
    }
}

#[cfg(test)]
mod park_tests;

/// No deadline polls `fut` directly. The sleep is boxed in the other arm, so a
/// call without a timeout does not carry that state machine.
pub(crate) fn wrap_timeout<T>(
    timeout: Option<Duration>,
    fut: impl Future<Output = Result<T, Status>>,
) -> impl Future<Output = Result<T, Status>> {
    match timeout {
        None => Either::Left(fut),
        Some(d) => Either::Right(Box::pin(expire_after(d, fut))),
    }
}

async fn expire_after<T>(
    d: Duration,
    fut: impl Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    match tokio::time::timeout(d, fut).await {
        Ok(r) => r,
        Err(_) => Err(Status::deadline_exceeded()),
    }
}

/// Percent-encode a `grpc-message` value.
///
/// The gRPC spec passes `0x20..=0x7E` through literally and escapes
/// everything else plus `%`.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if (0x20..=0x7e).contains(&b) && b != b'%' {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(hex_upper(b >> 4));
            out.push(hex_upper(b & 0x0f));
        }
    }
    out
}

pub(crate) fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        _ => char::from(b'A' + (nibble - 10)),
    }
}
