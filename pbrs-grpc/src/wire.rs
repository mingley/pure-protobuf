//! gRPC-over-HTTP/2 protocol: request/response headers, data frames, status
//! trailers, and the stream pumps that connect them to [`Streaming`].

use crate::codec::{self, Frame};
use crate::config::Wire;
use crate::gzip;
use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::status::{Code, Status};
use crate::stream::{Framed, Streaming};
use bytes::{BufMut, Bytes, BytesMut};
use h2::{Reason, RecvStream, SendStream};
use http::uri::{Authority, PathAndQuery, Scheme};
use http::{HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode};
use pbrs::{Parse, Serialize};
use std::time::Duration;

const GRPC_STATUS: HeaderName = HeaderName::from_static("grpc-status");
const GRPC_MESSAGE: HeaderName = HeaderName::from_static("grpc-message");
const GRPC_TIMEOUT: HeaderName = HeaderName::from_static("grpc-timeout");
const GRPC_ENCODING: HeaderName = HeaderName::from_static("grpc-encoding");
const GRPC_ACCEPT_ENCODING: HeaderName = HeaderName::from_static("grpc-accept-encoding");
const APPLICATION_GRPC: HeaderValue = HeaderValue::from_static("application/grpc");
const TRAILERS: HeaderValue = HeaderValue::from_static("trailers");
const IDENTITY_GZIP: HeaderValue = HeaderValue::from_static("identity,gzip");
const GZIP: HeaderValue = HeaderValue::from_static("gzip");
const STATUS_OK: HeaderValue = HeaderValue::from_static("0");

/// Headers a gRPC request or response carries before user metadata, rounded to
/// what `HeaderMap` will actually allocate. Sizing up front avoids a rehash.
const HEADER_CAPACITY: usize = 8;

pub(crate) fn grpc_request(
    authority: &Authority,
    path: &'static str,
    md: &Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
) -> Result<Request<()>, Status> {
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(Scheme::HTTP);
    parts.authority = Some(authority.clone());
    parts.path_and_query = Some(PathAndQuery::from_static(path));
    let uri = http::Uri::from_parts(parts).map_err(|e| Status::internal(e.to_string()))?;
    let mut req = Request::new(());
    *req.method_mut() = http::Method::POST;
    *req.uri_mut() = uri;
    // Pre-sized so the fixed gRPC headers do not force a rehash.
    *req.headers_mut() = HeaderMap::with_capacity(HEADER_CAPACITY);
    let headers = req.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
    headers.insert(http::header::TE, TRAILERS);
    headers.insert(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP);
    if send_gzip {
        headers.insert(GRPC_ENCODING, GZIP);
    }
    if let Some(d) = timeout {
        let val = HeaderValue::from_str(&crate::timeout::encode_timeout(d))
            .map_err(|e| Status::internal(e.to_string()))?;
        headers.insert(GRPC_TIMEOUT, val);
    }
    md.write_to(headers)?;
    Ok(req)
}

/// Reject anything that is not a gRPC request we can answer.
///
/// Runs before any body is read, so a malformed or unsupported request costs
/// one trailers-only response and nothing else.
pub(crate) fn check_request(request: &Request<RecvStream>) -> Result<(), Status> {
    if request.method() != http::Method::POST {
        return Err(Status::unimplemented("gRPC requires POST"));
    }
    let Some(ct) = request.headers().get(http::header::CONTENT_TYPE) else {
        return Err(Status::invalid_argument("missing content-type"));
    };
    let Ok(ct) = ct.to_str() else {
        return Err(Status::invalid_argument("invalid content-type"));
    };
    // `application/grpc`, `application/grpc+proto`, `application/grpc;charset=..`.
    let subtype = ct
        .strip_prefix("application/grpc")
        .ok_or_else(|| Status::invalid_argument("content-type must begin with application/grpc"))?;
    if !(subtype.is_empty() || subtype.starts_with('+') || subtype.starts_with(';')) {
        return Err(Status::invalid_argument(
            "content-type must begin with application/grpc",
        ));
    }
    if let Some(enc) = request.headers().get(GRPC_ENCODING) {
        let supported = matches!(enc.to_str(), Ok("identity" | "gzip"));
        if !supported {
            return Err(Status::unimplemented(
                "grpc-encoding not supported; this server accepts identity and gzip",
            ));
        }
    }
    Ok(())
}

pub(crate) fn timeout_from_headers(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(GRPC_TIMEOUT)
        .and_then(|v| v.to_str().ok())
        .and_then(crate::timeout::parse_timeout)
}

/// Serialize straight into the framed buffer: one allocation, no intermediate
/// `Vec`, and the length prefix is known before encoding starts.
fn frame_from_msg<T: Serialize>(msg: &T, len: usize) -> Result<Bytes, Status> {
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    let mut buf = BytesMut::with_capacity(codec::HEADER_LEN + len);
    buf.put_u8(0);
    buf.put_u32(prefix);
    T::encode(msg, &mut buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(buf.freeze())
}

pub(crate) fn encode_msg<T: Serialize>(
    msg: &T,
    compress: bool,
    limits: MessageLimits,
) -> Result<Bytes, Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if !compress {
        return frame_from_msg(msg, len);
    }
    let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
    let gz = gzip::encode(&body)?;
    codec::encode(&gz, true)
}

/// How many bytes of stream output to accumulate before handing them to HTTP/2.
///
/// gRPC messages are length-prefixed, so a DATA frame may carry any number of
/// them. Writing one frame per message costs a wakeup and often a syscall per
/// message, which dominates the cost of a small-message stream; batching to
/// 32 KiB amortises that without adding meaningful latency, because a batch is
/// flushed as soon as the producer has nothing more ready.
const STREAM_BATCH_BYTES: usize = 32 * 1024;

/// Append one length-prefixed message to `buf`.
///
/// The uncompressed path serializes straight into `buf`, so a batch of `n`
/// messages costs one buffer rather than `n`.
fn append_frame<T: Serialize>(
    buf: &mut BytesMut,
    msg: &T,
    compress: bool,
    limits: MessageLimits,
) -> Result<(), Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if compress {
        let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
        let gz = gzip::encode(&body)?;
        let prefix = u32::try_from(gz.len()).map_err(|_| Status::internal("message too large"))?;
        buf.reserve(codec::HEADER_LEN + gz.len());
        buf.put_u8(1);
        buf.put_u32(prefix);
        buf.extend_from_slice(&gz);
        return Ok(());
    }
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    buf.reserve(codec::HEADER_LEN + len);
    buf.put_u8(0);
    buf.put_u32(prefix);
    T::encode(msg, buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}

/// Accumulates encoded stream output and hands it to HTTP/2 in batches.
pub(crate) struct OutBatch {
    buf: BytesMut,
    wire: Wire,
}

impl OutBatch {
    /// Most messages to take from a producer in one go.
    ///
    /// Bounded so a fast producer cannot make one batch unboundedly large; the
    /// byte threshold usually fires first.
    pub(crate) const BURST: usize = 64;

    pub(crate) fn new(wire: Wire) -> Self {
        Self {
            buf: BytesMut::new(),
            wire,
        }
    }

    /// Encode one message into the batch, flushing if the batch is now full.
    ///
    /// Takes the message by value so the returned future owns it: holding a
    /// borrow across the flush would demand `T: Sync` of every streamed
    /// message type.
    pub(crate) async fn push<T: Serialize>(
        &mut self,
        send: &mut SendStream<Bytes>,
        item: Framed<T>,
    ) -> Result<(), Status> {
        append_frame(
            &mut self.buf,
            &item.message,
            item.compressed,
            self.wire.limits,
        )?;
        if self.buf.len() >= STREAM_BATCH_BYTES {
            self.flush(send).await?;
        }
        Ok(())
    }

    /// Whether the batch has reached the size worth writing on its own.
    pub(crate) fn is_full(&self) -> bool {
        self.buf.len() >= STREAM_BATCH_BYTES
    }

    /// Hand whatever has accumulated to HTTP/2.
    pub(crate) async fn flush(&mut self, send: &mut SendStream<Bytes>) -> Result<(), Status> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let frame = std::mem::take(&mut self.buf).freeze();
        send_bytes(send, frame, false, self.wire.send_buffer).await
    }
}

/// Give a producer one scheduling turn to refill the queue before writing.
///
/// A producer running ahead of the network is bounded by its channel depth, so
/// draining it yields only that many messages and the write is smaller than it
/// could be. One `yield_now` lets it top the queue up, which halves the writes
/// and the task wakeups for a bulk stream. It costs one re-queue on the same
/// worker when the producer has nothing more, so an interactive stream pays a
/// scheduler turn rather than a timer.
pub(crate) async fn let_producer_catch_up() {
    tokio::task::yield_now().await;
}

async fn wait_capacity(send: &mut SendStream<Bytes>, n: usize) -> Result<(), Status> {
    if send.capacity() >= n {
        return Ok(());
    }
    send.reserve_capacity(n);
    while send.capacity() < n {
        match std::future::poll_fn(|cx| send.poll_capacity(cx)).await {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Status::internal(e.to_string())),
            None => return Err(Status::internal("stream closed")),
        }
    }
    Ok(())
}

/// Queue one gRPC frame, reserving flow-control capacity only when we have to.
///
/// `h2` buffers anything up to the connection's send budget without waiting, so
/// reserving capacity first would cost a needless round trip through the
/// connection task on every message. A frame larger than the budget, or one
/// arriving while the budget is already spent, falls back to reserving capacity
/// and retrying. `Bytes` is reference-counted, so the retry does not copy.
pub(crate) async fn send_bytes(
    send: &mut SendStream<Bytes>,
    frame: Bytes,
    end: bool,
    send_buffer: usize,
) -> Result<(), Status> {
    if frame.len() <= send_buffer && send.send_data(frame.clone(), end).is_ok() {
        return Ok(());
    }
    wait_capacity(send, frame.len()).await?;
    send.send_data(frame, end)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn grpc_trailers(status: &Status) -> Result<HeaderMap, Status> {
    if status.code() == Code::Ok && status.message().is_empty() && status.metadata().is_empty() {
        // The overwhelmingly common case: one static header, no formatting.
        let mut map = HeaderMap::with_capacity(1);
        map.insert(GRPC_STATUS, STATUS_OK);
        return Ok(map);
    }
    let mut map = HeaderMap::with_capacity(4);
    let code = HeaderValue::from_str(&status.code().to_i32().to_string())
        .map_err(|e| Status::internal(e.to_string()))?;
    map.insert(GRPC_STATUS, code);
    if !status.message().is_empty() {
        let encoded = percent_encode(status.message());
        let val = HeaderValue::from_str(&encoded).map_err(|e| Status::internal(e.to_string()))?;
        map.insert(GRPC_MESSAGE, val);
    }
    status.metadata().write_to(&mut map)?;
    Ok(map)
}

/// Answer with headers only, folding `grpc-status` into them.
///
/// This is the "Trailers-Only" response of the gRPC spec, used for errors
/// raised before any message could be produced.
pub(crate) fn send_trailers_only(
    respond: &mut h2::server::SendResponse<Bytes>,
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
pub(crate) fn reject(respond: &mut h2::server::SendResponse<Bytes>, status: Status) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, APPLICATION_GRPC)
        .header(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP)
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

pub(crate) fn send_ok_headers(
    respond: &mut h2::server::SendResponse<Bytes>,
    md: &Metadata,
    send_gzip: bool,
) -> Result<SendStream<Bytes>, Status> {
    let mut res = Response::new(());
    *res.status_mut() = StatusCode::OK;
    *res.headers_mut() = HeaderMap::with_capacity(HEADER_CAPACITY);
    let headers = res.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
    if send_gzip {
        headers.insert(GRPC_ENCODING, GZIP);
    }
    md.write_to(headers)?;
    respond
        .send_response(res, false)
        .map_err(|e| Status::internal(e.to_string()))
}

/// Read `grpc-status` from trailers, falling back to headers for
/// Trailers-Only responses.
pub(crate) fn status_from(headers: &HeaderMap, trailers: Option<&HeaderMap>) -> Status {
    let pick = |map: &HeaderMap| {
        let code = map.get(GRPC_STATUS)?.to_str().ok()?.parse::<i32>().ok()?;
        let message = map
            .get(GRPC_MESSAGE)
            .and_then(|v| v.to_str().ok())
            .map(percent_decode)
            .unwrap_or_default();
        Some((Code::from_i32(code), message))
    };
    let found = trailers
        .and_then(|t| pick(t).map(|hit| (hit, t)))
        .or_else(|| pick(headers).map(|hit| (hit, headers)));
    match found {
        Some(((code, message), map)) => {
            let mut status = Status::new(code, message);
            if code != Code::Ok {
                *status.metadata_mut() = Metadata::from_headers(map);
            }
            status
        }
        None => Status::unknown("missing grpc-status"),
    }
}

/// Next HTTP/2 DATA chunk. Flow-control capacity is *not* released; the caller
/// releases it once the chunk has been handed on, which is what turns a slow
/// reader into peer backpressure.
async fn next_data(recv: &mut RecvStream) -> Result<Option<Bytes>, Status> {
    match recv.data().await {
        None => Ok(None),
        Some(Ok(bytes)) => Ok(Some(bytes)),
        Some(Err(e)) => Err(h2_error(e)),
    }
}

fn release(recv: &mut RecvStream, n: usize) -> Result<(), Status> {
    if n == 0 {
        return Ok(());
    }
    recv.flow_control()
        .release_capacity(n)
        .map_err(|e| Status::internal(e.to_string()))
}

fn h2_error(e: h2::Error) -> Status {
    if e.is_reset() {
        Status::cancelled()
    } else {
        Status::internal(e.to_string())
    }
}

/// Splits inbound DATA chunks into gRPC frames.
///
/// Invariant: `carry` is non-empty only while `chunk` is empty. A frame that
/// arrived whole inside one DATA chunk is sliced out of it and never copied;
/// only a frame straddling a chunk boundary passes through `carry`.
struct FrameReader {
    chunk: Bytes,
    carry: BytesMut,
    limits: MessageLimits,
}

impl FrameReader {
    fn new(limits: MessageLimits) -> Self {
        Self {
            chunk: Bytes::new(),
            carry: BytesMut::new(),
            limits,
        }
    }

    fn push(&mut self, next: Bytes) {
        if self.carry.is_empty() && self.chunk.is_empty() {
            self.chunk = next;
            return;
        }
        if !self.chunk.is_empty() {
            self.carry.extend_from_slice(&self.chunk);
            self.chunk = Bytes::new();
        }
        self.carry.extend_from_slice(&next);
    }

    fn next_frame(&mut self) -> Result<Option<Frame>, Status> {
        if !self.chunk.is_empty() {
            return codec::pop_from_chunk(&mut self.chunk, self.limits);
        }
        if !self.carry.is_empty() {
            return codec::pop_limited(&mut self.carry, self.limits);
        }
        Ok(None)
    }

    /// A stream that ended mid-frame is a protocol violation, not an
    /// empty message.
    fn finish(&self) -> Result<(), Status> {
        if self.chunk.is_empty() && self.carry.is_empty() {
            Ok(())
        } else {
            Err(Status::internal("truncated gRPC frame"))
        }
    }
}

fn decode_frame<T: Parse + Default>(
    frame: Frame,
    limits: MessageLimits,
) -> Result<Framed<T>, Status> {
    let message = if frame.compressed {
        let raw = gzip::decode_limited(&frame.payload, limits)?;
        T::parse(&raw).map_err(|e| Status::internal(e.to_string()))?
    } else {
        T::parse(frame.payload.as_ref()).map_err(|e| Status::internal(e.to_string()))?
    };
    Ok(Framed {
        message,
        compressed: frame.compressed,
    })
}

/// Read the single message of a unary request or response.
///
/// An empty body decodes to `T::default()`, matching gRPC's treatment of a
/// zero-field message. More than one message is a protocol violation.
pub(crate) async fn read_one_message<T: Parse + Default>(
    recv: &mut RecvStream,
    limits: MessageLimits,
) -> Result<Framed<T>, Status> {
    let mut reader = FrameReader::new(limits);
    let mut found: Option<Framed<T>> = None;
    while let Some(chunk) = next_data(recv).await? {
        let n = chunk.len();
        reader.push(chunk);
        release(recv, n)?;
        while let Some(frame) = reader.next_frame()? {
            if found.is_some() {
                return Err(Status::internal("unary rpc received more than one message"));
            }
            found = Some(decode_frame(frame, limits)?);
        }
    }
    reader.finish()?;
    Ok(found.unwrap_or_else(|| Framed::new(T::default())))
}

/// Drain trailers so the HTTP/2 stream closes cleanly.
pub(crate) async fn read_trailers(recv: &mut RecvStream) -> Result<Option<HeaderMap>, Status> {
    recv.trailers().await.map_err(h2_error)
}

/// An inbound message stream, decoded straight off its HTTP/2 stream.
///
/// There is no pump task and no intermediate queue: reading a message reads the
/// wire. That removes a task hop and a copy per message, and makes backpressure
/// exact, because a reader that stops reading stops releasing HTTP/2 capacity
/// and the peer stalls at the window.
pub(crate) struct WireStream<T> {
    recv: RecvStream,
    reader: FrameReader,
    limits: MessageLimits,
    /// Bound at construction, where `T: Parse` is known, so the public
    /// [`Streaming`] type needs no `Parse` bound of its own.
    decode: fn(Frame, MessageLimits) -> Result<Framed<T>, Status>,
    /// When the RPC's deadline expires. A deadline has to reach the reads, not
    /// just the call setup: a server that answers with headers and then goes
    /// quiet would otherwise hang the reader forever.
    deadline: Option<tokio::time::Instant>,
    ended: bool,
    trailers: Metadata,
}

impl<T: Parse + Default> WireStream<T> {
    pub(crate) fn new(
        recv: RecvStream,
        limits: MessageLimits,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        Self {
            recv,
            reader: FrameReader::new(limits),
            limits,
            decode: decode_frame::<T>,
            deadline,
            ended: false,
            trailers: Metadata::new(),
        }
    }
}

impl<T> WireStream<T> {
    /// The next message, or `Ok(None)` once the stream has ended cleanly.
    ///
    /// A non-OK `grpc-status` in the trailers surfaces here as `Err`, and so
    /// does an expired deadline.
    pub(crate) async fn next(&mut self) -> Result<Option<Framed<T>>, Status> {
        let Some(at) = self.deadline else {
            return self.next_inner().await;
        };
        match tokio::time::timeout_at(at, self.next_inner()).await {
            Err(_) => Err(Status::deadline_exceeded()),
            // A peer enforcing the same deadline resets the stream at it, and
            // that reset can arrive before our timer. Report the deadline
            // rather than a transport failure.
            Ok(Err(status))
                if matches!(status.code(), Code::Unavailable | Code::Cancelled)
                    && tokio::time::Instant::now() >= at =>
            {
                Err(Status::deadline_exceeded())
            }
            Ok(result) => result,
        }
    }

    async fn next_inner(&mut self) -> Result<Option<Framed<T>>, Status> {
        loop {
            if let Some(frame) = self.reader.next_frame()? {
                return (self.decode)(frame, self.limits).map(Some);
            }
            if self.ended {
                return Ok(None);
            }
            match next_data(&mut self.recv).await? {
                Some(chunk) => {
                    let n = chunk.len();
                    self.reader.push(chunk);
                    release(&mut self.recv, n)?;
                }
                None => {
                    self.ended = true;
                    self.reader.finish()?;
                    return self.finish_trailers().await.map(|()| None);
                }
            }
        }
    }

    async fn finish_trailers(&mut self) -> Result<(), Status> {
        let Some(map) = read_trailers(&mut self.recv).await? else {
            return Ok(());
        };
        let status = status_from(&map, Some(&map));
        self.trailers = Metadata::from_owned_headers(map);
        if status.code() == Code::Ok {
            Ok(())
        } else {
            Err(status)
        }
    }

    /// Trailing metadata, available once the stream has ended.
    pub(crate) fn trailers(&self) -> &Metadata {
        &self.trailers
    }
}

/// Encode a client's outbound stream, watching for cancellation.
pub(crate) async fn pump_outbound<T: Serialize>(
    mut send: SendStream<Bytes>,
    mut rx: Streaming<T>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    wire: Wire,
) {
    let mut batch = OutBatch::new(wire);
    let mut items = Vec::with_capacity(OutBatch::BURST);
    let mut watch_cancel = true;
    loop {
        items.clear();
        let taken = tokio::select! {
            cancelled = async {
                cancel_rx.wait_for(|v| *v).await.is_ok()
            }, if watch_cancel => {
                if cancelled {
                    send.send_reset(Reason::CANCEL);
                    return;
                }
                // The call finished and dropped its sender; stop watching.
                watch_cancel = false;
                continue;
            }
            taken = rx.recv_many(&mut items, OutBatch::BURST) => taken,
        };
        if taken == 0 {
            // Half-close, carrying whatever is still batched.
            if batch.flush(&mut send).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return;
            }
            send.send_data(Bytes::new(), true).ok();
            return;
        }
        // See the note in the server's drain loop: yield only when the caller
        // is demonstrably ahead of the network.
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 {
            let_producer_catch_up().await;
            rx.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let Ok(item) = item else {
                send.send_reset(Reason::INTERNAL_ERROR);
                return;
            };
            if batch.push(&mut send, item).await.is_err() {
                send.send_reset(Reason::INTERNAL_ERROR);
                return;
            }
        }
        if !batch.is_full() && batch.flush(&mut send).await.is_err() {
            send.send_reset(Reason::INTERNAL_ERROR);
            return;
        }
    }
}

pub(crate) async fn wrap_timeout<T>(
    timeout: Option<Duration>,
    fut: impl std::future::Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    match timeout {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(r) => r,
            Err(_) => Err(Status::deadline_exceeded()),
        },
        None => fut.await,
    }
}

pub(crate) async fn finish_unary<Resp: Parse + Default>(
    response: http::Response<RecvStream>,
    limits: MessageLimits,
) -> Result<crate::request::Response<Resp>, Status> {
    if response.status() != StatusCode::OK {
        return Err(Status::unknown(format!("http {}", response.status())));
    }
    let (parts, mut body) = response.into_parts();
    if body.is_end_stream() {
        // Trailers-Only: the status is in the headers and there is no message.
        let status = status_from(&parts.headers, None);
        if status.code() != Code::Ok {
            return Err(status);
        }
    }
    let framed = read_one_message::<Resp>(&mut body, limits).await?;
    let trailers = read_trailers(&mut body).await?;
    let status = status_from(&parts.headers, trailers.as_ref());
    if status.code() != Code::Ok {
        return Err(status);
    }
    let trailers_md = trailers
        .map(Metadata::from_owned_headers)
        .unwrap_or_default();
    Ok(crate::request::Response::from_parts_compress(
        framed.message,
        Metadata::from_owned_headers(parts.headers),
        trailers_md,
        framed.compressed,
    ))
}

pub(crate) async fn finish_stream<Resp: Parse + Default + Send + 'static>(
    response: http::Response<RecvStream>,
    limits: MessageLimits,
    deadline: Option<tokio::time::Instant>,
) -> Result<crate::request::Response<Streaming<Resp>>, Status> {
    if response.status() != StatusCode::OK {
        return Err(Status::unknown(format!("http {}", response.status())));
    }
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
        // Trailers-Only: the status is in the headers and there is no stream.
        let status = status_from(&parts.headers, None);
        if status.code() != Code::Ok {
            return Err(status);
        }
    }
    Ok(crate::request::Response::from_parts(
        Streaming::from_wire(WireStream::<Resp>::new(body, limits, deadline)),
        Metadata::from_owned_headers(parts.headers),
        Metadata::new(),
    ))
}

/// Percent-encode a `grpc-message` value.
///
/// The gRPC spec passes `0x20..=0x7E` through literally and escapes
/// everything else plus `%`.
fn percent_encode(s: &str) -> String {
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

fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        _ => char::from(b'A' + (nibble - 10)),
    }
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    if !bytes.contains(&b'%') {
        return s.to_owned();
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_value);
            let lo = bytes.get(i + 2).copied().and_then(hex_value);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{percent_decode, percent_encode, FrameReader};
    use crate::codec;
    use crate::gzip;
    use crate::limits::MessageLimits;
    use crate::status::Code;
    use bytes::{Bytes, BytesMut};

    #[test]
    fn message_encoding_matches_the_spec_set() {
        assert_eq!(percent_encode("plain text"), "plain text");
        assert_eq!(percent_encode("50%"), "50%25");
        assert_eq!(percent_encode("tab\there"), "tab%09here");
        assert_eq!(percent_encode("\u{00e9}"), "%C3%A9");
    }

    #[test]
    fn message_decoding_round_trips() {
        for original in ["plain text", "50%", "tab\there", "\u{00e9}\u{1f600}", ""] {
            assert_eq!(percent_decode(&percent_encode(original)), original);
        }
    }

    #[test]
    fn stray_percent_decodes_literally() {
        assert_eq!(percent_decode("100% sure"), "100% sure");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    fn frame(payload: &[u8]) -> Bytes {
        codec::encode(payload, false).expect("encode")
    }

    #[test]
    fn whole_frames_in_one_chunk_are_not_copied() {
        let mut joined = BytesMut::new();
        joined.extend_from_slice(&frame(b"one"));
        joined.extend_from_slice(&frame(b"two"));
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(joined.freeze());
        assert!(reader.carry.is_empty());
        let a = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&a.payload[..], b"one");
        let b = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&b.payload[..], b"two");
        assert!(reader.next_frame().expect("pop").is_none());
        reader.finish().expect("clean end");
        assert!(reader.carry.is_empty());
    }

    #[test]
    fn frames_split_across_chunks_are_rejoined() {
        let wire = frame(b"straddling");
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(wire.slice(..4));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(wire.slice(4..9));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(wire.slice(9..));
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"straddling");
        reader.finish().expect("clean end");
    }

    #[test]
    fn leftover_bytes_after_a_frame_carry_into_the_next_chunk() {
        let first = frame(b"a");
        let second = frame(b"bb");
        let mut joined = BytesMut::from(first.as_ref());
        joined.extend_from_slice(&second[..3]);
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(joined.freeze());
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"a");
        assert!(reader.next_frame().expect("pop").is_none());
        reader.push(second.slice(3..));
        let got = reader.next_frame().expect("pop").expect("frame");
        assert_eq!(&got.payload[..], b"bb");
        reader.finish().expect("clean end");
    }

    #[test]
    fn truncation_is_an_error() {
        let wire = frame(b"cut short");
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(wire.slice(..7));
        assert!(reader.next_frame().expect("pop").is_none());
        reader.finish().expect_err("truncated");
    }

    /// Deterministic xorshift, so a failure reproduces from the seed alone
    /// rather than needing a fuzzing dependency.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            if n == 0 {
                0
            } else {
                usize::try_from(self.next_u64() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
            }
        }

        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n)
                .map(|_| u8::try_from(self.next_u64() & 0xff).unwrap_or(0))
                .collect()
        }
    }

    /// Split `data` at random boundaries, so the reader sees every alignment of
    /// frames against chunks that HTTP/2 could produce.
    fn random_chunks(rng: &mut Rng, data: &Bytes) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let remaining = data.len() - offset;
            let take = 1 + rng.below(remaining.min(64));
            chunks.push(data.slice(offset..offset + take));
            offset += take;
        }
        chunks
    }

    /// Property: however the bytes are split, the frames come back intact and
    /// in order. This is the invariant the zero-copy fast path could break.
    #[test]
    fn arbitrary_chunk_boundaries_preserve_every_frame() {
        let mut rng = Rng(0x5eed_1234_abcd_0001);
        for _ in 0..2_000 {
            let count = 1 + rng.below(6);
            let payloads: Vec<Vec<u8>> = (0..count)
                .map(|_| {
                    let len = rng.below(200);
                    rng.bytes(len)
                })
                .collect();
            let mut wire = BytesMut::new();
            for payload in &payloads {
                wire.extend_from_slice(&codec::encode(payload, false).expect("encode"));
            }
            let wire = wire.freeze();

            let mut reader = FrameReader::new(MessageLimits::unlimited());
            let mut got: Vec<Vec<u8>> = Vec::new();
            for chunk in random_chunks(&mut rng, &wire) {
                reader.push(chunk);
                while let Some(frame) = reader.next_frame().expect("well-formed") {
                    got.push(frame.payload.to_vec());
                }
            }
            reader.finish().expect("clean end");
            assert_eq!(got, payloads, "chunking must not change the frames");
        }
    }

    /// Property: arbitrary bytes in arbitrary chunks produce frames or a
    /// `Status`, never a panic and never a frame longer than the cap.
    #[test]
    fn arbitrary_bytes_never_panic_and_never_exceed_the_cap() {
        const CAP: usize = 512;
        let limits = MessageLimits::unlimited().with_max_decoding(CAP);
        let mut rng = Rng(0xf00d_0bad_1dea_0002);
        for _ in 0..4_000 {
            let len = rng.below(600);
            let garbage = Bytes::from(rng.bytes(len));
            let mut reader = FrameReader::new(limits);
            for chunk in random_chunks(&mut rng, &garbage) {
                reader.push(chunk);
                loop {
                    match reader.next_frame() {
                        Ok(Some(frame)) => assert!(frame.payload.len() <= CAP),
                        Ok(None) => break,
                        // A `Status` is the correct answer for garbage.
                        Err(_) => break,
                    }
                }
            }
            // Truncation is a legitimate verdict on garbage; either arm is
            // fine, and neither may panic.
            match reader.finish() {
                Ok(()) | Err(_) => {}
            }
        }
    }

    /// Property: a compressed frame never inflates past the cap, whatever it
    /// claims. Random data barely compresses, so this also exercises the case
    /// where the inflated size is close to the input size.
    #[test]
    fn compressed_frames_respect_the_cap() {
        const CAP: usize = 256;
        let limits = MessageLimits::unlimited().with_max_decoding(CAP);
        let mut rng = Rng(0xdead_beef_cafe_0003);
        for _ in 0..300 {
            let len = rng.below(2_000);
            // Runs of zeros compress well; random bytes do not. Mix both.
            let payload: Vec<u8> = if rng.below(2) == 0 {
                vec![0u8; len]
            } else {
                rng.bytes(len)
            };
            let compressed = gzip::encode(&payload).expect("encode");
            match gzip::decode_limited(&compressed, limits) {
                Ok(inflated) => {
                    assert!(inflated.len() <= CAP);
                    assert_eq!(inflated, payload);
                }
                Err(status) => {
                    assert!(payload.len() > CAP, "only oversize payloads may fail");
                    assert_eq!(status.code(), Code::ResourceExhausted);
                }
            }
        }
    }
}
