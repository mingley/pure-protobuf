//! Inbound reads: frame parsing, message reads, call completion.

use super::headers::{
    GRPC_MESSAGE, GRPC_RETRY_PUSHBACK_MS, GRPC_STATUS, GRPC_STATUS_DETAILS_BIN,
    encoding_not_supported, grpc_encoding, inbound_codec_from_token,
};
use crate::binlog::{CallLogger, Logger};
use crate::codec::{self, CodecMessage, Frame};
use crate::compression::{CompressionAlgorithm, DECODE_LOOKAHEAD_BYTES};
use crate::limits::MessageLimits;
use crate::metadata::{self, Metadata};
use crate::status::{Code, Status, parse_pushback_value};
use crate::stream::{Framed, Streaming};
use crate::transport::{Error as TransportError, RecvStream, h2 as backend};
use crate::workspace::RetainedDecodeLease;
use bytes::{Buf, Bytes, BytesMut};
use futures_util::future::Either;
use http::{HeaderMap, StatusCode};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Read `grpc-status` from trailers, falling back to headers for
/// Trailers-Only responses.
pub(crate) fn status_from(headers: &HeaderMap, trailers: Option<&HeaderMap>) -> Status {
    // The success trailer is exactly `grpc-status: 0`.
    if let Some(trailers) = trailers {
        if trailers.len() == 1 {
            if let Some((name, value)) = trailers.iter().next() {
                let bytes = value.as_bytes();
                if name == GRPC_STATUS && bytes == b"0" {
                    return Status::ok();
                }
            }
        }
    }
    // Single pass over the status block. OK responses carry exactly one
    // trailer, so this replaces four hashed lookups with one short scan.
    // First values win, matching `HeaderMap::get`; an unreadable or
    // unparseable first `grpc-status` fails the pick, as before.
    let pick = |map: &HeaderMap| {
        let mut code = None;
        let mut message = None;
        let mut pushback = None;
        let mut details = None;
        for (name, value) in map.iter() {
            if name == GRPC_STATUS {
                if code.is_none() {
                    code = Some(value.to_str().ok()?.parse::<i32>().ok()?);
                }
            } else if name == GRPC_MESSAGE {
                if message.is_none() {
                    message = value.to_str().ok().map(percent_decode);
                }
            } else if name == GRPC_RETRY_PUSHBACK_MS {
                if pushback.is_none() {
                    pushback = value.to_str().ok().and_then(parse_pushback_value);
                }
            } else if name == GRPC_STATUS_DETAILS_BIN && details.is_none() {
                details = value.to_str().ok().and_then(metadata::decode_base64);
            }
        }
        let code = code?;
        Some((
            Code::from_i32(code),
            message.unwrap_or_default(),
            pushback,
            details,
        ))
    };
    let found = trailers
        .and_then(|t| pick(t).map(|hit| (hit, t)))
        .or_else(|| pick(headers).map(|hit| (hit, headers)));
    match found {
        Some(((code, message, pushback, details), map)) => {
            let mut status = Status::new(code, message);
            if code != Code::Ok {
                *status.metadata_mut() = Metadata::from_headers(map);
            }
            if let Some(pushback) = pushback {
                status.set_retry_pushback(pushback);
            }
            if let Some(details) = details {
                status.set_details(details);
            }
            status
        }
        None => Status::unknown("missing grpc-status"),
    }
}

// An explicit non-OK wire status can terminate an unfinished response.
// Missing or malformed statuses must not hide a truncated frame.
fn has_error_status(headers: &HeaderMap) -> bool {
    headers
        .get(GRPC_STATUS)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i32>().ok())
        .is_some_and(|code| code != 0)
}

/// Next HTTP/2 DATA chunk. Flow-control capacity is *not* released; the caller
/// releases it once the chunk has been handed on, which is what turns a slow
/// reader into peer backpressure.
#[allow(
    dead_code,
    reason = "delayed-release callers; the steady path uses poll_data_released"
)]
pub(crate) fn poll_data(
    recv: &mut backend::RecvStream,
    cx: &mut Context<'_>,
) -> Poll<Result<Option<Bytes>, Status>> {
    match recv.poll_data(cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(None) => Poll::Ready(Ok(None)),
        Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Ok(Some(bytes))),
        Poll::Ready(Some(Err(e))) => Poll::Ready(Err(h2_error(e))),
    }
}

/// [`poll_data`] plus the immediate window release, under one stream lock.
///
/// The caller matches this chunk immediately. Out of line, the bytes
/// landed in this return slot and were copied out again.
#[inline(always)]
pub(crate) fn poll_data_released(
    recv: &mut backend::RecvStream,
    cx: &mut Context<'_>,
) -> Poll<Result<Option<Bytes>, Status>> {
    match recv.poll_data_released(cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(None) => Poll::Ready(Ok(None)),
        Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Ok(Some(bytes))),
        Poll::Ready(Some(Err(e))) => Poll::Ready(Err(h2_error(e))),
    }
}

#[allow(
    dead_code,
    reason = "delayed-release callers; the steady path uses next_data_released"
)]
pub(crate) async fn next_data(recv: &mut backend::RecvStream) -> Result<Option<Bytes>, Status> {
    poll_fn(|cx| poll_data(recv, cx)).await
}

pub(crate) async fn next_data_released(
    recv: &mut backend::RecvStream,
) -> Result<Option<Bytes>, Status> {
    poll_fn(|cx| poll_data_released(recv, cx)).await
}

#[allow(
    dead_code,
    reason = "delayed-release callers; the steady path releases inside poll_data_released"
)]
pub(crate) fn release(recv: &mut backend::RecvStream, n: usize) -> Result<(), Status> {
    if n == 0 {
        return Ok(());
    }
    recv.release_capacity(n)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn h2_error(e: TransportError) -> Status {
    Status::from_h2(e)
}

/// Splits inbound DATA chunks into gRPC frames.
///
/// `carry` precedes `chunk` on the wire. Only the bytes needed to complete a
/// straddling frame enter `carry`; the remainder stays in `chunk`, so following
/// whole frames retain the ordinary shared-buffer path.
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
        if !self.chunk.is_empty() {
            crate::copy_counts::note_carry(self.chunk.len());
            self.carry.extend_from_slice(&self.chunk);
        }
        self.chunk = next;
        // NOTE (PK-09): a reserve-once here (Phase 1a) removes regrowth
        // copies in isolation (5x microbench win, fewer cycles/faults/RSS)
        // but reproducibly costs -18% on loopback 8 MiB RPCs: both sides do
        // less CPU work yet wait longer in recvfrom, an emergent
        // pipeline/batch interaction, not CPU. Reverted per ship-only-if-
        // faster-or-better; Phase 3 (segmented receive) re-asks framing
        // with fresh data. See docs/evidence/pk09-zero-copy.md.
    }

    pub(crate) fn next_frame(&mut self) -> Result<Option<Frame>, Status> {
        if self.carry.is_empty() {
            return codec::pop_from_chunk(&mut self.chunk, self.limits);
        }
        // Complete and validate the header before copying any more payload.
        // This reserves only bytes that have already arrived, never the
        // announced frame length.
        if self.carry.len() < codec::HEADER_LEN {
            self.copy_chunk_prefix(codec::HEADER_LEN - self.carry.len());
        }
        if let Some(frame) = codec::pop_limited(&mut self.carry, self.limits)? {
            return Ok(Some(frame));
        }
        let Some(encoded_len) = self.carry.get(1..codec::HEADER_LEN) else {
            return Ok(None);
        };
        if self.chunk.is_empty() {
            return Ok(None);
        }
        let mut length = [0u8; 4];
        length.copy_from_slice(encoded_len);
        let payload_len = usize::try_from(u32::from_be_bytes(length))
            .map_err(|_| Status::internal("message too large"))?;
        let frame_len = codec::HEADER_LEN
            .checked_add(payload_len)
            .ok_or_else(|| Status::internal("message too large"))?;
        self.copy_chunk_prefix(frame_len.saturating_sub(self.carry.len()));
        codec::pop_limited(&mut self.carry, self.limits)
    }

    fn copy_chunk_prefix(&mut self, needed: usize) {
        let count = needed.min(self.chunk.len());
        if count != 0 {
            crate::copy_counts::note_carry(count);
            self.carry
                .extend_from_slice(self.chunk.get(..count).unwrap_or_default());
            self.chunk.advance(count);
        }
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

pub(crate) fn decode_frame<T: CodecMessage>(
    frame: Frame,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
) -> Result<Framed<T>, Status> {
    let message = if frame.compressed {
        // gRPC compression requires INTERNAL for flag one without a named,
        // non-identity coding; accepting it as gzip hides a negotiation error.
        let codec = codec.ok_or_else(|| {
            Status::internal("compressed-flag set without a non-identity grpc-encoding")
        })?;
        if !accept_gzip {
            return Err(encoding_not_supported(false));
        }
        let raw = codec.decode_limited(&frame.payload, limits)?;
        T::decode_payload(Bytes::from(raw))?
    } else {
        T::decode_payload(frame.payload)?
    };
    Ok(Framed {
        message,
        compressed: frame.compressed,
    })
}

/// A retained allocation is charged only while idle between messages. Fields
/// drop in this order, so the allocation is freed before its lease is released.
struct DecodedWorkspace {
    buffer: Vec<u8>,
    lease: Option<RetainedDecodeLease>,
}

impl DecodedWorkspace {
    fn take_buffer(&mut self) -> Vec<u8> {
        let buffer = std::mem::take(&mut self.buffer);
        // The allocation is now active decode memory, governed by message
        // limits. The retention budget applies again only after validation.
        self.lease = None;
        buffer
    }
}

fn decode_stream_frame<T: CodecMessage>(
    frame: Frame,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    workspace: &mut Option<Box<DecodedWorkspace>>,
) -> Result<Framed<T>, Status> {
    if !frame.compressed {
        return decode_frame(frame, limits, accept_gzip, codec);
    }
    let result = (|| {
        let codec = codec.ok_or_else(|| {
            Status::internal("compressed-flag set without a non-identity grpc-encoding")
        })?;
        if !accept_gzip {
            return Err(encoding_not_supported(false));
        }
        let output = workspace
            .as_mut()
            .map_or_else(Vec::new, |workspace| workspace.take_buffer());
        let raw = codec.decode_limited_into(&frame.payload, limits, output)?;
        // Cold output within the initial decode lookahead has little growth
        // to amortize. Keep its ordinary codec ownership path, avoiding cache
        // metadata, a workspace Box and retention atomics. Existing workspaces
        // still serve later short or empty messages in a mixed-size stream.
        if workspace.is_none() && raw.len() <= DECODE_LOOKAHEAD_BYTES {
            return T::decode_payload(Bytes::from(raw)).map(|message| Framed {
                message,
                compressed: true,
            });
        }
        let mut payload = Bytes::from(raw);
        let message = T::decode_payload_reusable(&mut payload)?;
        // A codec may move, split or retain its input. Reclaim only a fully
        // consumed cursor whose backing allocation is still exclusively ours.
        let retained = payload
            .is_empty()
            .then(|| payload.try_into_mut().ok())
            .flatten()
            .map(Vec::from)
            .and_then(|buffer| {
                RetainedDecodeLease::try_acquire(buffer.capacity()).map(|lease| (buffer, lease))
            });
        if let Some((buffer, lease)) = retained {
            if let Some(workspace) = workspace {
                workspace.buffer = buffer;
                workspace.lease = Some(lease);
            } else {
                *workspace = Some(Box::new(DecodedWorkspace {
                    buffer,
                    lease: Some(lease),
                }));
            }
        } else {
            *workspace = None;
        }
        Ok(Framed {
            message,
            compressed: true,
        })
    })();
    if result.is_err() {
        *workspace = None;
    }
    result
}

/// Read the single message of a unary request or response.
///
/// An empty body decodes to `T::default()`, matching gRPC's treatment of a
/// zero-field message. More than one message is a protocol violation.
pub(crate) async fn read_one_message<T: CodecMessage>(
    recv: &mut backend::RecvStream,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    tap: Option<&CallLogger>,
    response: bool,
) -> Result<Framed<T>, Status> {
    let mut reader = FrameReader::new(limits);
    let mut found: Option<Framed<T>> = None;
    while let Some(chunk) = next_data_released(recv).await? {
        reader.push(chunk);
        while let Some(frame) = reader.next_frame()? {
            if found.is_some() {
                return Err(Status::internal("unary rpc received more than one message"));
            }
            if let Some(tap) = tap {
                tap.log_read(&frame.payload);
            }
            found = Some(decode_frame(frame, limits, accept_gzip, codec)?);
        }
    }
    if let Err(framing_error) = reader.finish() {
        if response {
            if let Some(trailers) = read_trailers(recv).await? {
                if has_error_status(&trailers) {
                    return Err(status_from(&trailers, Some(&trailers)));
                }
            }
        }
        return Err(framing_error);
    }
    // A clean unary-request end is the client's half-close. Response reads
    // end in a trailer, logged by the caller.
    if let Some(tap) = tap.filter(|tap| matches!(tap.role(), Logger::Server)) {
        tap.log_half_close();
    }
    Ok(found.unwrap_or_else(|| Framed::new(T::empty())))
}

/// Drain trailers so the HTTP/2 stream closes cleanly.
pub(crate) async fn read_trailers(
    recv: &mut backend::RecvStream,
) -> Result<Option<HeaderMap>, Status> {
    recv.trailers().await.map_err(h2_error)
}

type DecodeFrame<T> = fn(
    Frame,
    MessageLimits,
    bool,
    Option<CompressionAlgorithm>,
    &mut Option<Box<DecodedWorkspace>>,
) -> Result<Framed<T>, Status>;

/// An inbound message stream, decoded straight off its HTTP/2 stream.
///
/// There is no pump task and no intermediate queue: reading a message reads the
/// wire. That removes a task hop and a copy per message, and makes backpressure
/// exact, because a reader that stops reading stops releasing HTTP/2 capacity
/// and the peer stalls at the window.
pub(crate) struct WireStream<T> {
    recv: backend::RecvStream,
    reader: FrameReader,
    limits: MessageLimits,
    /// Bound at construction, where `T: CodecMessage` is known, so the public
    /// [`Streaming`] type needs no `Parse` bound of its own.
    decode: DecodeFrame<T>,
    /// Allocated only after a codec actually relinquishes a reusable buffer.
    workspace: Option<Box<DecodedWorkspace>>,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    /// When the RPC's deadline expires. A deadline has to reach the reads, not
    /// just the call setup: a server that answers with headers and then goes
    /// quiet would otherwise hang the reader forever.
    deadline: Option<tokio::time::Instant>,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    ended: bool,
    trailers_done: bool,
    /// Requests may half-close without status; body responses must finish
    /// with grpc-status unless it was validated in Trailers-Only headers.
    require_response_status: bool,
    trailers: Metadata,
    tap: Option<CallLogger>,
}

impl<T: CodecMessage> WireStream<T> {
    pub(crate) fn new(
        recv: backend::RecvStream,
        limits: MessageLimits,
        deadline: Option<tokio::time::Instant>,
        accept_gzip: bool,
        codec: Option<CompressionAlgorithm>,
        tap: Option<CallLogger>,
    ) -> Self {
        Self {
            recv,
            reader: FrameReader::new(limits),
            limits,
            decode: decode_stream_frame::<T>,
            workspace: None,
            accept_gzip,
            codec,
            deadline,
            sleep: deadline.map(|at| Box::pin(tokio::time::sleep_until(at))),
            ended: false,
            trailers_done: false,
            require_response_status: false,
            trailers: Metadata::new(),
            tap,
        }
    }
}

impl<T> WireStream<T> {
    fn requiring_response_status(mut self, required: bool) -> Self {
        self.require_response_status = required;
        self
    }

    /// Release retained decode memory even if a terminal stream stays alive.
    pub(crate) fn release_workspace(&mut self) {
        self.workspace = None;
    }

    /// Whether DATA and trailers have both been consumed, so a Drop must
    /// not RST a finished RPC.
    pub(crate) fn finished(&self) -> bool {
        self.trailers_done
    }

    /// Preserve deadline precedence when a local cancellation bypasses the
    /// asynchronous HTTP/2 reset that normally supplies the read error.
    pub(crate) fn local_cancel_status(&self) -> Status {
        if self
            .deadline
            .is_some_and(|at| tokio::time::Instant::now() >= at)
        {
            Status::deadline_exceeded()
        } else {
            Status::cancelled()
        }
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
                self.release_workspace();
                return Poll::Ready(Err(Status::deadline_exceeded()));
            }
            if let Some(sleep) = &mut self.sleep {
                if sleep.as_mut().poll(cx).is_ready() {
                    self.release_workspace();
                    return Poll::Ready(Err(Status::deadline_exceeded()));
                }
            }
        }
        let result = self.poll_next_inner(cx);
        if matches!(result, Poll::Ready(Err(_)) | Poll::Ready(Ok(None))) {
            self.release_workspace();
        }
        match result {
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
                        (self.decode)(
                            frame,
                            self.limits,
                            self.accept_gzip,
                            self.codec,
                            &mut self.workspace,
                        )
                        .map(Some),
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
            match poll_data_released(&mut self.recv, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(Some(chunk))) => {
                    self.reader.push(chunk);
                }
                Poll::Ready(Ok(None)) => {
                    self.ended = true;
                    self.release_workspace();
                    if !self.require_response_status {
                        if let Err(e) = self.reader.finish() {
                            return Poll::Ready(Err(e));
                        }
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
                if self.require_response_status {
                    let status = self
                        .reader
                        .finish()
                        .err()
                        .unwrap_or_else(|| Status::unknown("missing grpc-status"));
                    self.log_response_trailer(&status);
                    Poll::Ready(Err(status))
                } else {
                    self.log_response_trailer(&Status::from_code(Code::Ok));
                    Poll::Ready(Ok(()))
                }
            }
            Poll::Ready(Ok(Some(map))) => {
                // `status_from` repeats this check. A bare OK trailer is the
                // common block, so classify it once and recycle from that.
                let bare_ok = crate::h2_backend::header_pool::is_bare_ok_trailer(&map);
                let mut status = if bare_ok {
                    Status::ok()
                } else {
                    status_from(&map, Some(&map))
                };
                if self.require_response_status {
                    if let Err(framing_error) = self.reader.finish() {
                        if !has_error_status(&map) {
                            status = framing_error;
                        }
                    }
                }
                self.trailers = if bare_ok {
                    crate::h2_backend::header_pool::recycle_received_ok_trailer(map);
                    Metadata::new()
                } else {
                    metadata_from_trailers(map)
                };
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

/// Refuse a reply coding this channel will not inflate.
///
/// A coding the registry does not know is refused even when inbound
/// compression is on: failing the gunzip with `Internal` would hide a
/// negotiation bug as a data error. A known coding is refused only when
/// the channel opted out of inbound compression.
pub(crate) fn refuse_encoding_reply(token: Option<&str>, accept_gzip: bool) -> Result<(), Status> {
    let Some(token) = token else {
        return Ok(());
    };
    if CompressionAlgorithm::parse(token).is_none() {
        #[cfg(feature = "zstd")]
        let accepted = "identity, gzip, deflate, and zstd";
        #[cfg(not(feature = "zstd"))]
        let accepted = "identity, gzip, and deflate";
        return Err(Status::unimplemented(format!(
            "grpc-encoding {token} not supported; this client accepts {accepted}"
        )));
    }
    if !accept_gzip {
        return Err(Status::unimplemented(
            "grpc-encoding not supported; this client accepts identity",
        ));
    }
    Ok(())
}

struct PreparedUnary {
    parts: http::response::Parts,
    body: backend::RecvStream,
    codec: Option<crate::compression::CompressionAlgorithm>,
    stock_response: Option<bool>,
    encoding: Option<String>,
}

fn prepare_unary(
    response: http::Response<backend::RecvStream>,
    accept_gzip: bool,
    tap: Option<&CallLogger>,
) -> Result<PreparedUnary, Status> {
    if response.status() != StatusCode::OK {
        let status = Status::unknown(format!("http {}", response.status()));
        if let Some(tap) = tap {
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
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
    if let Some(tap) = tap {
        tap.log_server_header(&Metadata::from_owned_headers(parts.headers.clone()));
    }
    let stock_response = crate::h2_backend::header_pool::is_stock_response(&parts.headers);
    let encoding = if stock_response.is_some() {
        None
    } else {
        grpc_encoding(&parts.headers).map(str::to_owned)
    };
    if let Err(status) = refuse_encoding_reply(encoding.as_deref(), accept_gzip) {
        if let Some(tap) = tap {
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let codec = inbound_codec_from_token(encoding.as_deref());
    Ok(PreparedUnary {
        parts,
        body,
        codec,
        stock_response,
        encoding,
    })
}

pub(crate) fn finish_unary<Resp: CodecMessage>(
    response: http::Response<backend::RecvStream>,
    limits: MessageLimits,
    accept_gzip: bool,
    tap: Option<&CallLogger>,
) -> impl Future<Output = Result<crate::request::Response<Resp>, Status>> {
    match prepare_unary(response, accept_gzip, tap) {
        Err(status) => Either::Left(std::future::ready(Err(status))),
        Ok(prepared) => Either::Right(complete_unary::<Resp>(prepared, limits, accept_gzip, tap)),
    }
}

async fn complete_unary<Resp: CodecMessage>(
    prepared: PreparedUnary,
    limits: MessageLimits,
    accept_gzip: bool,
    tap: Option<&CallLogger>,
) -> Result<crate::request::Response<Resp>, Status> {
    let PreparedUnary {
        parts,
        mut body,
        codec,
        stock_response,
        encoding,
    } = prepared;
    let framed =
        match read_one_message::<Resp>(&mut body, limits, accept_gzip, codec, tap, true).await {
            Ok(framed) => framed,
            Err(status) => {
                if let Some(tap) = tap {
                    tap.log_trailer(status.metadata(), &status);
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
    let (status, trailers_md) = match trailers {
        Some(map) => {
            let bare_ok = crate::h2_backend::header_pool::is_bare_ok_trailer(&map);
            let status = if bare_ok {
                Status::ok()
            } else {
                status_from(&parts.headers, Some(&map))
            };
            let trailers_md = if bare_ok {
                crate::h2_backend::header_pool::recycle_received_ok_trailer(map);
                Metadata::new()
            } else {
                metadata_from_trailers(map)
            };
            (status, trailers_md)
        }
        None => (status_from(&parts.headers, None), Metadata::new()),
    };
    let header_md = metadata_from_response(parts.headers, stock_response);
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

pub(crate) fn finish_stream<Resp: CodecMessage + Send + 'static>(
    response: http::Response<backend::RecvStream>,
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
    let trailers_only = body.is_end_stream();
    if trailers_only {
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
    // A stock response is content-type plus accept-encoding. It has no
    // grpc-encoding, so the hashed miss is skipped.
    let stock_response = crate::h2_backend::header_pool::is_stock_response(&parts.headers);
    let encoding_token = if stock_response.is_some() {
        None
    } else {
        grpc_encoding(&parts.headers)
    };
    if let Err(status) = refuse_encoding_reply(encoding_token, accept_gzip) {
        if let Some(tap) = &tap {
            tap.log_server_header(&Metadata::from_owned_headers(parts.headers.clone()));
            tap.log_trailer(&Metadata::new(), &status);
        }
        return Err(status);
    }
    let encoding = encoding_token.map(str::to_owned);
    let codec = inbound_codec_from_token(encoding_token);
    let header_md = metadata_from_response(parts.headers, stock_response);
    if let Some(tap) = &tap {
        tap.log_server_header(&header_md);
    }
    Ok(crate::request::Response::from_parts(
        Streaming::from_wire(
            WireStream::<Resp>::new(body, limits, deadline, accept_gzip, codec, tap)
                .requiring_response_status(!trailers_only),
        ),
        header_md,
        Metadata::new(),
    )
    .with_encoding(encoding))
}

fn metadata_from_response(map: HeaderMap, stock: Option<bool>) -> Metadata {
    if let Some(grpc_proto) = stock {
        crate::h2_backend::header_pool::recycle_received_response(map, grpc_proto);
        Metadata::new()
    } else {
        Metadata::from_owned_headers(map)
    }
}

fn metadata_from_trailers(map: HeaderMap) -> Metadata {
    if crate::h2_backend::header_pool::is_bare_ok_trailer(&map) {
        crate::h2_backend::header_pool::recycle_received_ok_trailer(map);
        Metadata::new()
    } else {
        Metadata::from_owned_headers(map)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(codec::HEADER_LEN + payload.len());
        buf.push(0);
        buf.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("test len fits u32")
                .to_be_bytes(),
        );
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    fn fragmented_frame_does_not_copy_following_whole_frames() {
        let first = framed(b"first");
        let second = framed(&vec![0xCD; 64 * 1024]);
        let mut tail = BytesMut::new();
        tail.extend_from_slice(first.get(3..).expect("first tail"));
        tail.extend_from_slice(&second);
        let tail = tail.freeze();
        let second_payload = tail
            .as_ptr()
            .wrapping_add(first.len() - 3 + codec::HEADER_LEN);
        let mut reader = FrameReader::new(MessageLimits::unlimited());
        reader.push(Bytes::copy_from_slice(
            first.get(..3).expect("split header"),
        ));
        assert!(reader.next_frame().expect("partial header").is_none());
        reader.push(tail.clone());
        let got = reader.next_frame().expect("frame").expect("first");
        assert_eq!(got.payload.as_ref(), b"first");
        assert!(reader.carry.is_empty());
        let got = reader.next_frame().expect("frame").expect("second");
        assert_eq!(got.payload.as_ptr(), second_payload);
        assert_eq!(got.payload.as_ref(), vec![0xCD; 64 * 1024]);
        assert!(reader.next_frame().expect("clean end").is_none());
        reader.finish().expect("no remainder");
    }

    #[test]
    fn split_hostile_header_is_rejected_before_copying_its_body() {
        for invalid_flag in [false, true] {
            let mut wire = framed(&vec![0xAB; 64 * 1024]);
            if invalid_flag {
                *wire.first_mut().expect("flag") = 2;
            } else {
                wire.get_mut(1..codec::HEADER_LEN)
                    .expect("length")
                    .copy_from_slice(&u32::MAX.to_be_bytes());
            }
            let mut reader = FrameReader::new(MessageLimits::default());
            reader.push(Bytes::copy_from_slice(
                wire.get(..3).expect("header prefix"),
            ));
            assert!(reader.next_frame().expect("partial header").is_none());
            reader.push(Bytes::copy_from_slice(
                wire.get(3..).expect("header and body"),
            ));
            assert!(reader.next_frame().is_err());
            assert_eq!(reader.carry.len(), codec::HEADER_LEN);
            assert!(reader.carry.capacity() < 1024);
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RawMessage(Bytes);

    impl CodecMessage for RawMessage {
        fn encoded_len(&self) -> usize {
            self.0.len()
        }

        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.put_slice(&self.0);
            Ok(())
        }

        fn decode_payload(payload: Bytes) -> Result<Self, Status> {
            Ok(Self(payload))
        }

        fn empty() -> Self {
            Self(Bytes::new())
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct OwnedMessage(Vec<u8>);

    impl CodecMessage for OwnedMessage {
        fn encoded_len(&self) -> usize {
            self.0.len()
        }

        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.put_slice(&self.0);
            Ok(())
        }

        fn decode_payload(payload: Bytes) -> Result<Self, Status> {
            Ok(Self(payload.to_vec()))
        }

        fn decode_payload_reusable(payload: &mut Bytes) -> Result<Self, Status> {
            use bytes::Buf;
            let message = Self(payload.to_vec());
            payload.advance(payload.len());
            Ok(message)
        }

        fn empty() -> Self {
            Self(Vec::new())
        }
    }

    fn compressed(payload: &[u8]) -> Frame {
        Frame {
            compressed: true,
            payload: CompressionAlgorithm::Gzip
                .encode(payload)
                .expect("gzip")
                .into(),
        }
    }

    #[test]
    fn cold_short_messages_do_not_start_a_workspace_but_warm_short_messages_reuse_it() {
        let limits = MessageLimits::unlimited().with_max_decoding(131072);
        let mut workspace = None;
        for length in [
            0,
            1,
            1024,
            DECODE_LOOKAHEAD_BYTES - 1,
            DECODE_LOOKAHEAD_BYTES,
        ] {
            let payload = vec![7; length];
            assert_eq!(
                decode_stream_frame::<OwnedMessage>(
                    compressed(&payload),
                    limits,
                    true,
                    Some(CompressionAlgorithm::Gzip),
                    &mut workspace,
                )
                .expect("cold short decode")
                .message
                .0,
                payload
            );
            assert!(
                workspace.is_none(),
                "cold short output must not start a cache"
            );
        }

        let payload = vec![9; DECODE_LOOKAHEAD_BYTES + 1];
        assert_eq!(
            decode_stream_frame::<OwnedMessage>(
                compressed(&payload),
                limits,
                true,
                Some(CompressionAlgorithm::Gzip),
                &mut workspace,
            )
            .expect("large decode starts retention")
            .message
            .0,
            payload
        );
        let retained = workspace.as_ref().expect("large owned output retained");
        let pointer = retained.buffer.as_ptr();
        let capacity = retained.buffer.capacity();
        for length in [17, 0, 1024] {
            let payload = vec![11; length];
            assert_eq!(
                decode_stream_frame::<OwnedMessage>(
                    compressed(&payload),
                    limits,
                    true,
                    Some(CompressionAlgorithm::Gzip),
                    &mut workspace,
                )
                .expect("warm short decode")
                .message
                .0,
                payload
            );
            let retained = workspace.as_ref().expect("short output keeps warm storage");
            assert_eq!(retained.buffer.as_ptr(), pointer);
            assert_eq!(retained.buffer.capacity(), capacity);
        }
    }

    #[test]
    fn owned_stream_decode_reuses_the_allocation_and_discards_it_on_error() {
        let limits = MessageLimits::unlimited().with_max_decoding(131072);
        let mut workspace = None;
        let first = vec![7; 65539];
        assert_eq!(
            decode_stream_frame::<OwnedMessage>(
                compressed(&first),
                limits,
                true,
                Some(CompressionAlgorithm::Gzip),
                &mut workspace,
            )
            .expect("owned decode")
            .message
            .0,
            first
        );
        let retained = workspace
            .as_ref()
            .expect("uniquely owned decode output is retained");
        let pointer = retained.buffer.as_ptr();
        let capacity = retained.buffer.capacity();
        for length in [1, 8193, 0, 65539] {
            let next = vec![9; length];
            assert_eq!(
                decode_stream_frame::<OwnedMessage>(
                    compressed(&next),
                    MessageLimits::unlimited().with_max_decoding(length),
                    true,
                    Some(CompressionAlgorithm::Gzip),
                    &mut workspace,
                )
                .expect("decode with smaller allowance")
                .message
                .0,
                next
            );
            let retained = workspace.as_ref().expect("same allocation retained");
            assert_eq!(retained.buffer.as_ptr(), pointer);
            assert_eq!(retained.buffer.capacity(), capacity);
        }
        let mut bad = compressed(&first);
        let mut payload = bad.payload.to_vec();
        payload.pop();
        bad.payload = payload.into();
        assert_eq!(
            decode_stream_frame::<OwnedMessage>(
                bad,
                limits,
                true,
                Some(CompressionAlgorithm::Gzip),
                &mut workspace,
            )
            .expect_err("truncated checksum after a warm cache")
            .code(),
            Code::Internal
        );
        assert!(workspace.is_none());
    }

    #[test]
    fn retained_input_and_uncompressed_messages_do_not_create_a_workspace() {
        let limits = MessageLimits::default();
        let mut workspace = None;
        let first = decode_stream_frame::<RawMessage>(
            compressed(&vec![7; 65539]),
            limits,
            true,
            Some(CompressionAlgorithm::Gzip),
            &mut workspace,
        )
        .expect("retained input")
        .message;
        let pointer = first.0.as_ptr();
        assert!(
            workspace.is_none(),
            "default codecs retain the original ownership behavior"
        );
        let second = decode_stream_frame::<RawMessage>(
            compressed(&vec![9; 65539]),
            limits,
            true,
            Some(CompressionAlgorithm::Gzip),
            &mut workspace,
        )
        .expect("later message")
        .message;
        assert!(workspace.is_none());
        assert_eq!(first.0.as_ptr(), pointer);
        assert_eq!(first.0.as_ref(), vec![7; 65539]);
        assert_eq!(second.0.as_ref(), vec![9; 65539]);
        let frame = Frame {
            compressed: false,
            payload: Bytes::from_static(b"plain"),
        };
        assert_eq!(
            decode_stream_frame::<OwnedMessage>(
                frame,
                limits,
                true,
                Some(CompressionAlgorithm::Gzip),
                &mut workspace,
            )
            .expect("uncompressed decode")
            .message
            .0,
            b"plain"
        );
        assert!(workspace.is_none());
    }

    #[test]
    fn consumed_unique_bytes_recover_the_entire_original_allocation() {
        use bytes::Buf;

        // Cover both Bytes representations: exact-size boxed storage and a
        // Vec with spare capacity. No owner is dropped between conversions.
        for capacity in [65539, 131072] {
            let mut original = Vec::with_capacity(capacity);
            original.resize(65539, 7);
            let pointer = original.as_ptr();
            let capacity = original.capacity();
            let mut payload = Bytes::from(original);
            let length = payload.len();
            payload.advance(length);
            assert!(payload.is_empty());
            let recovered: Vec<u8> = payload
                .try_into_mut()
                .expect("the consumed cursor still uniquely owns the allocation")
                .into();
            assert!(recovered.is_empty());
            assert_eq!(recovered.as_ptr(), pointer);
            assert_eq!(recovered.capacity(), capacity);
        }
    }

    #[test]
    fn compressed_frames_require_a_negotiated_encoding() {
        let limits = MessageLimits::default();
        let payload = Bytes::from_static(b"message");
        for token in [None, Some("identity"), Some("")] {
            let codec = inbound_codec_from_token(token);
            assert!(codec.is_none());
            let frame = Frame {
                compressed: true,
                payload: CompressionAlgorithm::Gzip
                    .encode(&payload)
                    .expect("gzip")
                    .into(),
            };
            let status = decode_frame::<RawMessage>(frame, limits, true, codec)
                .expect_err("flag one without a coding");
            assert_eq!(status.code(), Code::Internal);
        }
        for codec in [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate] {
            let frame = Frame {
                compressed: true,
                payload: codec.encode(&payload).expect("compress").into(),
            };
            assert_eq!(
                decode_frame::<RawMessage>(frame, limits, true, Some(codec))
                    .expect("negotiated coding")
                    .message,
                RawMessage(payload.clone())
            );
            // A named coding does not require every message to be compressed.
            let frame = Frame {
                compressed: false,
                payload: payload.clone(),
            };
            assert_eq!(
                decode_frame::<RawMessage>(frame, limits, true, Some(codec))
                    .expect("per-message identity")
                    .message,
                RawMessage(payload.clone())
            );
        }
    }

    #[test]
    fn spanning_frame_pops_intact() {
        // A frame split across three chunks reassembles with an intact
        // payload, whatever the split points (including inside the header).
        for split in [1, 4, 5, 6, 64] {
            let wire = framed(&vec![0xABu8; 300]);
            let (a, rest) = wire.split_at(split.min(wire.len() - 1));
            let (b, c) = rest.split_at(rest.len() / 2);
            let mut reader = FrameReader::new(MessageLimits::new());
            reader.push(Bytes::copy_from_slice(a));
            assert!(reader.next_frame().expect("frame").is_none());
            reader.push(Bytes::copy_from_slice(b));
            reader.push(Bytes::copy_from_slice(c));
            let frame = reader.next_frame().expect("frame").expect("one frame");
            assert!(!frame.compressed);
            assert_eq!(frame.payload.as_ref(), vec![0xABu8; 300]);
            assert!(reader.next_frame().expect("frame").is_none());
        }
    }

    #[test]
    fn hostile_header_still_errors_at_pop() {
        // A length past the decode limit surfaces at pop (reserve-once was
        // reverted; see the NOTE on push).
        let mut wire = framed(&[9u8; 16]);
        wire[1..5].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut reader = FrameReader::new(MessageLimits::new());
        reader.push(Bytes::copy_from_slice(&wire[..8]));
        reader.push(Bytes::copy_from_slice(&wire[8..]));
        assert!(reader.next_frame().is_err());
    }
}
