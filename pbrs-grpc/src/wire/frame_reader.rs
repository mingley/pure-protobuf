//! Inbound reads: frame parsing, message reads, call completion.

use super::headers::{
    GRPC_MESSAGE, GRPC_RETRY_PUSHBACK_MS, GRPC_STATUS, GRPC_STATUS_DETAILS_BIN,
    encoding_not_supported, grpc_encoding,
};
use crate::binlog::{CallLogger, Logger};
use crate::codec::{self, Frame};
use crate::gzip;
use crate::limits::MessageLimits;
use crate::metadata::{self, Metadata};
use crate::status::{Code, Status, parse_pushback_value};
use crate::stream::{Framed, Streaming};
use bytes::{Bytes, BytesMut};
use h2::RecvStream;
use http::{HeaderMap, StatusCode};
use pbrs::Parse;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll};

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
            if let Some(pushback) = map
                .get(GRPC_RETRY_PUSHBACK_MS)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_pushback_value)
            {
                status.set_retry_pushback(pushback);
            }
            if let Some(raw) = map
                .get(GRPC_STATUS_DETAILS_BIN)
                .and_then(|v| v.to_str().ok())
            {
                if let Some(details) = metadata::decode_base64(raw) {
                    status.set_details(details);
                }
            }
            status
        }
        None => Status::unknown("missing grpc-status"),
    }
}

/// Next HTTP/2 DATA chunk. Flow-control capacity is *not* released; the caller
/// releases it once the chunk has been handed on, which is what turns a slow
/// reader into peer backpressure.
pub(crate) fn poll_data(
    recv: &mut RecvStream,
    cx: &mut Context<'_>,
) -> Poll<Result<Option<Bytes>, Status>> {
    match recv.poll_data(cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(None) => Poll::Ready(Ok(None)),
        Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Ok(Some(bytes))),
        Poll::Ready(Some(Err(e))) => Poll::Ready(Err(h2_error(e))),
    }
}

pub(crate) async fn next_data(recv: &mut RecvStream) -> Result<Option<Bytes>, Status> {
    poll_fn(|cx| poll_data(recv, cx)).await
}

pub(crate) fn release(recv: &mut RecvStream, n: usize) -> Result<(), Status> {
    if n == 0 {
        return Ok(());
    }
    recv.flow_control()
        .release_capacity(n)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn h2_error(e: h2::Error) -> Status {
    if e.is_reset() {
        Status::cancelled()
    } else {
        Status::from_h2(e)
    }
}

/// Splits inbound DATA chunks into gRPC frames.
///
/// Invariant: `carry` is non-empty only while `chunk` is empty. A frame that
/// arrived whole inside one DATA chunk is sliced out of it and never copied;
/// only a frame straddling a chunk boundary passes through `carry`.
pub(crate) struct FrameReader {
    pub(crate) chunk: Bytes,
    pub(crate) carry: BytesMut,
    pub(crate) limits: MessageLimits,
}

impl FrameReader {
    pub(crate) fn new(limits: MessageLimits) -> Self {
        Self {
            chunk: Bytes::new(),
            carry: BytesMut::new(),
            limits,
        }
    }

    pub(crate) fn push(&mut self, next: Bytes) {
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

    pub(crate) fn next_frame(&mut self) -> Result<Option<Frame>, Status> {
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
    pub(crate) fn finish(&self) -> Result<(), Status> {
        if self.chunk.is_empty() && self.carry.is_empty() {
            Ok(())
        } else {
            Err(Status::internal("truncated gRPC frame"))
        }
    }
}

pub(crate) fn decode_frame<T: Parse + Default>(
    frame: Frame,
    limits: MessageLimits,
    accept_gzip: bool,
) -> Result<Framed<T>, Status> {
    let message = if frame.compressed {
        if !accept_gzip {
            return Err(encoding_not_supported(false));
        }
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
    accept_gzip: bool,
    tap: Option<&CallLogger>,
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
            if let Some(tap) = tap {
                tap.log_read(&frame.payload);
            }
            found = Some(decode_frame(frame, limits, accept_gzip)?);
        }
    }
    reader.finish()?;
    // A clean unary-request end is the client's half-close. Response reads
    // end in a trailer, logged by the caller.
    if let Some(tap) = tap.filter(|tap| matches!(tap.role(), Logger::Server)) {
        tap.log_half_close();
    }
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
    decode: fn(Frame, MessageLimits, bool) -> Result<Framed<T>, Status>,
    accept_gzip: bool,
    /// When the RPC's deadline expires. A deadline has to reach the reads, not
    /// just the call setup: a server that answers with headers and then goes
    /// quiet would otherwise hang the reader forever.
    deadline: Option<tokio::time::Instant>,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    ended: bool,
    trailers_done: bool,
    trailers: Metadata,
    tap: Option<CallLogger>,
}

impl<T: Parse + Default> WireStream<T> {
    pub(crate) fn new(
        recv: RecvStream,
        limits: MessageLimits,
        deadline: Option<tokio::time::Instant>,
        accept_gzip: bool,
        tap: Option<CallLogger>,
    ) -> Self {
        Self {
            recv,
            reader: FrameReader::new(limits),
            limits,
            decode: decode_frame::<T>,
            accept_gzip,
            deadline,
            sleep: deadline.map(|at| Box::pin(tokio::time::sleep_until(at))),
            ended: false,
            trailers_done: false,
            trailers: Metadata::new(),
            tap,
        }
    }
}

impl<T> WireStream<T> {
    /// Whether DATA and trailers have both been consumed, so a Drop must
    /// not RST a finished RPC.
    pub(crate) fn finished(&self) -> bool {
        self.trailers_done
    }

    /// The next message, or `Ok(None)` once the stream has ended cleanly.
    ///
    /// A non-OK `grpc-status` in the trailers surfaces here as `Err`, and so
    /// does an expired deadline.
    pub(crate) async fn next(&mut self) -> Result<Option<Framed<T>>, Status> {
        poll_fn(|cx| self.poll_next(cx)).await
    }

    pub(crate) fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Framed<T>>, Status>> {
        if let Some(at) = self.deadline {
            if tokio::time::Instant::now() >= at {
                return Poll::Ready(Err(Status::deadline_exceeded()));
            }
            if let Some(sleep) = &mut self.sleep {
                if sleep.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(Err(Status::deadline_exceeded()));
                }
            }
        }
        match self.poll_next_inner(cx) {
            Poll::Ready(Err(status))
                if matches!(status.code(), Code::Unavailable | Code::Cancelled)
                    && self
                        .deadline
                        .is_some_and(|at| tokio::time::Instant::now() >= at) =>
            {
                Poll::Ready(Err(Status::deadline_exceeded()))
            }
            other => other,
        }
    }

    fn poll_next_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Framed<T>>, Status>> {
        loop {
            match self.reader.next_frame() {
                Err(e) => return Poll::Ready(Err(e)),
                Ok(Some(frame)) => {
                    if let Some(tap) = &self.tap {
                        tap.log_read(&frame.payload);
                    }
                    return Poll::Ready(
                        (self.decode)(frame, self.limits, self.accept_gzip).map(Some),
                    );
                }
                Ok(None) => {}
            }
            if self.ended {
                if self.trailers_done {
                    return Poll::Ready(Ok(None));
                }
                return match self.poll_finish_trailers(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok(())) => {
                        self.trailers_done = true;
                        Poll::Ready(Ok(None))
                    }
                    Poll::Ready(Err(e)) => {
                        self.trailers_done = true;
                        Poll::Ready(Err(e))
                    }
                };
            }
            match poll_data(&mut self.recv, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(Some(chunk))) => {
                    let n = chunk.len();
                    self.reader.push(chunk);
                    if let Err(e) = release(&mut self.recv, n) {
                        return Poll::Ready(Err(e));
                    }
                }
                Poll::Ready(Ok(None)) => {
                    self.ended = true;
                    if let Err(e) = self.reader.finish() {
                        return Poll::Ready(Err(e));
                    }
                    // A clean request-stream end is the client's half-close.
                    // Response streams end in a trailer instead.
                    if let Some(tap) = self
                        .tap
                        .as_ref()
                        .filter(|tap| matches!(tap.role(), Logger::Server))
                    {
                        tap.log_half_close();
                    }
                }
            }
        }
    }

    fn poll_finish_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Status>> {
        match self.recv.poll_trailers(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => {
                let status = h2_error(e);
                self.log_response_trailer(&status);
                Poll::Ready(Err(status))
            }
            Poll::Ready(Ok(None)) => {
                self.log_response_trailer(&Status::from_code(Code::Ok));
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(Some(map))) => {
                let status = status_from(&map, Some(&map));
                self.trailers = Metadata::from_owned_headers(map);
                self.log_response_trailer(&status);
                if status.code() == Code::Ok {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(status))
                }
            }
        }
    }

    /// Log the response trailer exactly once, when this stream reads
    /// responses. Request streams end in a half-close instead.
    fn log_response_trailer(&self, status: &Status) {
        if let Some(tap) = self
            .tap
            .as_ref()
            .filter(|tap| matches!(tap.role(), Logger::Client))
        {
            tap.log_trailer(&self.trailers, status);
        }
    }

    /// Trailing metadata, available once the stream has ended.
    pub(crate) fn trailers(&self) -> &Metadata {
        &self.trailers
    }
}

/// Refuse a gzip reply when this channel opted out of inbound gzip.
pub(crate) fn refuse_gzip_reply(headers: &HeaderMap, accept_gzip: bool) -> Result<(), Status> {
    if accept_gzip {
        return Ok(());
    }
    if grpc_encoding(headers).is_some_and(|token| token.eq_ignore_ascii_case("gzip")) {
        return Err(Status::unimplemented(
            "grpc-encoding not supported; this client accepts identity",
        ));
    }
    Ok(())
}

pub(crate) async fn finish_unary<Resp: Parse + Default>(
    response: http::Response<RecvStream>,
    limits: MessageLimits,
    accept_gzip: bool,
    tap: Option<&CallLogger>,
) -> Result<crate::request::Response<Resp>, Status> {
    if response.status() != StatusCode::OK {
        let status = Status::unknown(format!("http {}", response.status()));
        if let Some(tap) = tap {
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let (parts, mut body) = response.into_parts();
    if body.is_end_stream() {
        // Trailers-Only: the status is in the headers and there is no message.
        let status = status_from(&parts.headers, None);
        if status.code() != Code::Ok {
            if let Some(tap) = tap {
                tap.log_trailer(
                    &Metadata::from_owned_headers(parts.headers.clone()),
                    &status,
                );
            }
            return Err(status);
        }
    }
    // Headers are observed before the body is read; log them first so the
    // message entries that read_one_message emits stay in wire order.
    if let Some(tap) = tap {
        tap.log_server_header(&Metadata::from_owned_headers(parts.headers.clone()));
    }
    if let Err(status) = refuse_gzip_reply(&parts.headers, accept_gzip) {
        if let Some(tap) = tap {
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let framed = match read_one_message::<Resp>(&mut body, limits, accept_gzip, tap).await {
        Ok(framed) => framed,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            return Err(status);
        }
    };
    let trailers = match read_trailers(&mut body).await {
        Ok(trailers) => trailers,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            return Err(status);
        }
    };
    let status = status_from(&parts.headers, trailers.as_ref());
    let encoding = grpc_encoding(&parts.headers).map(str::to_owned);
    let header_md = Metadata::from_owned_headers(parts.headers);
    let trailers_md = trailers
        .map(Metadata::from_owned_headers)
        .unwrap_or_default();
    if let Some(tap) = tap {
        tap.log_trailer(&trailers_md, &status);
    }
    if status.code() != Code::Ok {
        return Err(status);
    }
    Ok(crate::request::Response::from_parts_compress(
        framed.message,
        header_md,
        trailers_md,
        framed.compressed,
    )
    .with_encoding(encoding))
}

pub(crate) async fn finish_stream<Resp: Parse + Default + Send + 'static>(
    response: http::Response<RecvStream>,
    limits: MessageLimits,
    deadline: Option<tokio::time::Instant>,
    accept_gzip: bool,
    tap: Option<CallLogger>,
) -> Result<crate::request::Response<Streaming<Resp>>, Status> {
    if response.status() != StatusCode::OK {
        let status = Status::unknown(format!("http {}", response.status()));
        if let Some(tap) = &tap {
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
        // Trailers-Only: the status is in the headers and there is no stream.
        let status = status_from(&parts.headers, None);
        if status.code() != Code::Ok {
            if let Some(tap) = &tap {
                tap.log_trailer(
                    &Metadata::from_owned_headers(parts.headers.clone()),
                    &status,
                );
            }
            return Err(status);
        }
    }
    if let Err(status) = refuse_gzip_reply(&parts.headers, accept_gzip) {
        if let Some(tap) = &tap {
            tap.log_server_header(&Metadata::from_owned_headers(parts.headers.clone()));
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let encoding = grpc_encoding(&parts.headers).map(str::to_owned);
    let header_md = Metadata::from_owned_headers(parts.headers);
    if let Some(tap) = &tap {
        tap.log_server_header(&header_md);
    }
    Ok(crate::request::Response::from_parts(
        Streaming::from_wire(WireStream::<Resp>::new(
            body,
            limits,
            deadline,
            accept_gzip,
            tap,
        )),
        header_md,
        Metadata::new(),
    )
    .with_encoding(encoding))
}

pub(crate) fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn percent_decode(s: &str) -> String {
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
