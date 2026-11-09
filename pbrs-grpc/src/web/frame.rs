//! gRPC-Web framing over the existing HTTP/2 transport.

use crate::binlog::{CallLogger, Logger};
use crate::codec::CodecMessage;
use crate::compression::CompressionAlgorithm;
use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::status::Status;
use crate::stream::Framed;
use crate::transport::h2 as backend;
use crate::transport::{SendResponse as _, SendStream as _};
use crate::wire::SegFrame;
use crate::wire::frame_reader::{FrameReader, decode_frame, next_data, release};
use crate::wire::grpc_trailers;
use crate::wire::headers::{
    GRPC_ACCEPT_ENCODING, GRPC_ENCODING, HEADER_CAPACITY, accept_encoding_value, encoding_value,
};
use crate::wire::send::send_bytes;
use base64::Engine;
use base64::alphabet;
use base64::engine::{
    DecodePaddingMode,
    general_purpose::{GeneralPurpose, GeneralPurposeConfig},
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use http::{HeaderMap, HeaderValue, Response, StatusCode};

const APPLICATION_GRPC_WEB: HeaderValue = HeaderValue::from_static("application/grpc-web");
const APPLICATION_GRPC_WEB_PROTO: HeaderValue =
    HeaderValue::from_static("application/grpc-web+proto");
const APPLICATION_GRPC_WEB_TEXT: HeaderValue =
    HeaderValue::from_static("application/grpc-web-text");
const APPLICATION_GRPC_WEB_TEXT_PROTO: HeaderValue =
    HeaderValue::from_static("application/grpc-web-text+proto");
const TEXT_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);
const ACCESS_CONTROL_ALLOW_ORIGIN: &str = "access-control-allow-origin";
const ACCESS_CONTROL_ALLOW_METHODS: &str = "access-control-allow-methods";
const ACCESS_CONTROL_ALLOW_HEADERS: &str = "access-control-allow-headers";
const ACCESS_CONTROL_EXPOSE_HEADERS: &str = "access-control-expose-headers";
const ACCESS_CONTROL_MAX_AGE: &str = "access-control-max-age";
const VARY: &str = "vary";
const DEFAULT_ALLOW_HEADERS: &str =
    "content-type,x-grpc-web,grpc-timeout,grpc-encoding,grpc-accept-encoding";

/// CORS policy for gRPC-Web preflight requests and optional response defaults.
///
/// Default is [`Self::DenyAll`]. Use [`crate::ServerConfig::grpc_web_allow_any_origin`]
/// or [`crate::ServerConfig::grpc_web_allow_origin`] to opt in. Response
/// defaults require [`crate::ServerConfig::grpc_web_cors_response_headers`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CorsPolicy {
    /// Reject every browser preflight.
    #[default]
    DenyAll,
    /// Allow every origin with `access-control-allow-origin: *`.
    AllowAny,
    /// Allow exactly this origin.
    AllowOrigin(&'static str),
}

impl CorsPolicy {
    fn matched_origin(self, origin: &HeaderValue) -> Option<&'static str> {
        match self {
            Self::DenyAll => None,
            Self::AllowAny => Some("*"),
            Self::AllowOrigin(allowed) => (origin.to_str().ok()? == allowed).then_some(allowed),
        }
    }

    fn allowed_origin(self, origin: &HeaderValue) -> Option<HeaderValue> {
        let matched = self.matched_origin(origin)?;
        if matched == "*" {
            Some(HeaderValue::from_static("*"))
        } else {
            HeaderValue::from_str(matched).ok()
        }
    }
}

/// gRPC-Web response mode selected by request content type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    /// `application/grpc-web`
    Binary,
    /// `application/grpc-web+proto`
    BinaryProto,
    /// `application/grpc-web-text`
    Text,
    /// `application/grpc-web-text+proto`
    TextProto,
}

impl Mode {
    /// Parse a supported gRPC-Web request content type.
    pub(crate) fn from_content_type(value: &str) -> Option<Self> {
        let value = value.trim();
        let (ty, rest) = value.split_once('/')?;
        if !ty.eq_ignore_ascii_case("application") {
            return None;
        }
        let subtype = match rest.split_once(';') {
            Some((sub, _)) => sub.trim(),
            None => rest.trim(),
        };
        if subtype.eq_ignore_ascii_case("grpc-web") {
            Some(Self::Binary)
        } else if subtype.eq_ignore_ascii_case("grpc-web+proto") {
            Some(Self::BinaryProto)
        } else if subtype.eq_ignore_ascii_case("grpc-web-text") {
            Some(Self::Text)
        } else if subtype.eq_ignore_ascii_case("grpc-web-text+proto") {
            Some(Self::TextProto)
        } else {
            None
        }
    }

    /// Parse supported gRPC-Web mode from request headers.
    pub(crate) fn from_headers(headers: &HeaderMap) -> Option<Self> {
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(Self::from_content_type)
    }

    fn content_type(self) -> HeaderValue {
        match self {
            Self::Binary => APPLICATION_GRPC_WEB,
            Self::BinaryProto => APPLICATION_GRPC_WEB_PROTO,
            Self::Text => APPLICATION_GRPC_WEB_TEXT,
            Self::TextProto => APPLICATION_GRPC_WEB_TEXT_PROTO,
        }
    }

    pub(crate) fn is_text(self) -> bool {
        matches!(self, Self::Text | Self::TextProto)
    }
}

/// Request-local Web response context, captured before request metadata can
/// be changed by an interceptor. Configured origins are static, so this can
/// be copied into the deadline owner without retaining the request headers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WebContext {
    pub(crate) mode: Mode,
    enabled: bool,
    allow_origin: Option<&'static str>,
}

impl WebContext {
    pub(crate) fn new(mode: Mode, headers: &HeaderMap, policy: CorsPolicy, enabled: bool) -> Self {
        let allow_origin = if enabled {
            headers
                .get(http::header::ORIGIN)
                .and_then(|origin| policy.matched_origin(origin))
        } else {
            None
        };
        Self {
            mode,
            enabled,
            allow_origin,
        }
    }

    /// Supply CORS defaults after application metadata is added. Explicit
    /// application headers remain authoritative.
    pub(crate) fn apply_cors(self, headers: &mut HeaderMap) {
        if !self.enabled {
            return;
        }
        let varies = headers.get_all(VARY).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value.split(',').any(|token| {
                    let token = token.trim();
                    token == "*" || token.eq_ignore_ascii_case("origin")
                })
            })
        });
        if !varies {
            headers.append(VARY, HeaderValue::from_static("origin"));
        }
        let origin = if headers.contains_key(ACCESS_CONTROL_ALLOW_ORIGIN) {
            None
        } else {
            self.allow_origin.and_then(|origin| {
                if origin == "*" {
                    Some(HeaderValue::from_static("*"))
                } else {
                    HeaderValue::from_str(origin).ok()
                }
            })
        };
        if let Some(origin) = origin {
            headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            if !headers.contains_key(ACCESS_CONTROL_EXPOSE_HEADERS) {
                // Native CORS does not opt in to credentials. For ordinary
                // browser requests this exposes custom response metadata too.
                headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("*"));
            }
        }
    }
}

/// Whether this content type is a supported binary gRPC-Web variant.
pub(crate) fn is_supported_content_type(value: &str) -> bool {
    Mode::from_content_type(value).is_some()
}

/// Answer a gRPC-Web CORS preflight request, if this request is one.
pub(crate) fn send_cors_preflight(
    request: &http::Request<backend::RecvStream>,
    respond: &mut backend::SendResponse,
    policy: CorsPolicy,
) -> bool {
    if request.method() != http::Method::OPTIONS {
        return false;
    }
    let headers = request.headers();
    let Some(origin) = headers.get(http::header::ORIGIN) else {
        return false;
    };
    let Some(method) = headers.get(http::header::ACCESS_CONTROL_REQUEST_METHOD) else {
        return false;
    };
    if !method
        .to_str()
        .is_ok_and(|method| method.eq_ignore_ascii_case("POST"))
    {
        send_cors_status(respond, StatusCode::METHOD_NOT_ALLOWED, None, None);
        return true;
    }
    let allow_origin = policy.allowed_origin(origin);
    if allow_origin.is_none() {
        send_cors_status(respond, StatusCode::FORBIDDEN, None, None);
        return true;
    }
    let allow_headers = headers
        .get(http::header::ACCESS_CONTROL_REQUEST_HEADERS)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static(DEFAULT_ALLOW_HEADERS));
    send_cors_status(respond, StatusCode::OK, allow_origin, Some(allow_headers));
    true
}

fn send_cors_status(
    respond: &mut backend::SendResponse,
    status: StatusCode,
    allow_origin: Option<HeaderValue>,
    allow_headers: Option<HeaderValue>,
) {
    let mut res = match Response::builder().status(status).body(()) {
        Ok(res) => res,
        Err(_) => return,
    };
    let headers = res.headers_mut();
    headers.insert(
        VARY,
        HeaderValue::from_static(
            "origin, access-control-request-method, access-control-request-headers",
        ),
    );
    if let Some(origin) = allow_origin {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            allow_headers.unwrap_or_else(|| HeaderValue::from_static(DEFAULT_ALLOW_HEADERS)),
        );
        headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
    }
    respond.send_response(res, true).ok();
}

/// Send an HTTP-level rejection for a request identified as gRPC-Web.
pub(crate) fn send_http_rejection(
    respond: &mut backend::SendResponse,
    web: WebContext,
    status: StatusCode,
) {
    let mut response = Response::new(());
    *response.status_mut() = status;
    if status == StatusCode::METHOD_NOT_ALLOWED {
        response
            .headers_mut()
            .insert(http::header::ALLOW, HeaderValue::from_static("POST"));
    }
    web.apply_cors(response.headers_mut());
    respond.send_response(response, true).ok();
}

/// Send initial gRPC-Web response headers.
pub(crate) fn send_ok_headers(
    respond: &mut backend::SendResponse,
    web: WebContext,
    md: &Metadata,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
) -> Result<backend::SendStream, Status> {
    let mut res = Response::new(());
    *res.status_mut() = StatusCode::OK;
    *res.headers_mut() = HeaderMap::with_capacity(HEADER_CAPACITY);
    let headers = res.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, web.mode.content_type());
    headers.insert(GRPC_ACCEPT_ENCODING, accept_encoding_value(accept_gzip));
    if let Some(codec) = send_codec {
        headers.insert(GRPC_ENCODING, encoding_value(codec));
    }
    md.write_to(headers)?;
    web.apply_cors(headers);
    respond
        .send_response(res, false)
        .map_err(|e| Status::internal(e.to_string()))
}

/// Send a gRPC-Web trailers-only body response.
pub(crate) fn send_trailers_only(
    respond: &mut backend::SendResponse,
    web: WebContext,
    status: Status,
    extra_headers: &Metadata,
) {
    send_trailers_only_inner(respond, web, status, extra_headers, None);
}

/// Reject an unsupported request coding and advertise the supported codings.
pub(crate) fn send_encoding_rejection(
    respond: &mut backend::SendResponse,
    web: WebContext,
    status: Status,
    accept_compressed: bool,
) {
    send_trailers_only_inner(
        respond,
        web,
        status,
        &Metadata::new(),
        Some(accept_compressed),
    );
}

fn send_trailers_only_inner(
    respond: &mut backend::SendResponse,
    web: WebContext,
    status: Status,
    extra_headers: &Metadata,
    accept_compressed: Option<bool>,
) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, web.mode.content_type())
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    extra_headers.write_to(res.headers_mut()).ok();
    web.apply_cors(res.headers_mut());
    if let Some(accept_compressed) = accept_compressed {
        res.headers_mut().insert(
            GRPC_ACCEPT_ENCODING,
            accept_encoding_value(accept_compressed),
        );
    }
    let Ok(mut send) = respond.send_response(res, false) else {
        return;
    };
    if let Ok(frame) = trailer_frame(&status) {
        let frame = if web.mode.is_text() {
            TEXT_BASE64.encode(frame).into()
        } else {
            frame
        };
        send.send_data(frame, true).ok();
    }
}

/// Queue a trailers-only body while preserving serialization and send errors.
pub(crate) fn try_send_trailers_only(
    respond: &mut backend::SendResponse,
    web: WebContext,
    status: &Status,
    extra_headers: &Metadata,
) -> Result<(), Status> {
    let mut response = Response::new(());
    response
        .headers_mut()
        .insert(http::header::CONTENT_TYPE, web.mode.content_type());
    extra_headers.write_to(response.headers_mut())?;
    web.apply_cors(response.headers_mut());
    let frame = trailer_frame(status)?;
    let frame = if web.mode.is_text() {
        TEXT_BASE64.encode(frame).into()
    } else {
        frame
    };
    let mut send = respond
        .send_response(response, false)
        .map_err(Status::from_h2_send)?;
    send.send_data(frame, true).map_err(Status::from_h2_send)
}

/// Prepare a gRPC-Web deadline ending after the response writer is destroyed.
/// Text mode resumes the writer's unencoded final one or two bytes before the
/// trailer frame, so the resulting body remains one continuous base64 stream.
pub(crate) fn deadline_response(
    mode: Mode,
    status: &Status,
    carry: &[u8],
) -> Result<(Response<()>, HeaderMap, Option<Bytes>), Status> {
    debug_assert!(carry.len() <= 2, "base64 carry is at most two bytes");
    debug_assert!(
        mode.is_text() || carry.is_empty(),
        "binary mode has no base64 carry"
    );
    let mut response = Response::new(());
    response
        .headers_mut()
        .insert(http::header::CONTENT_TYPE, mode.content_type());
    let frame = trailer_frame(status)?;
    let body = if mode.is_text() {
        let bytes = if carry.is_empty() {
            frame
        } else {
            let capacity = frame
                .len()
                .checked_add(carry.len())
                .ok_or_else(|| Status::internal("gRPC-Web deadline body too large"))?;
            let mut bytes = BytesMut::with_capacity(capacity);
            bytes.extend_from_slice(carry);
            bytes.extend_from_slice(&frame);
            bytes.freeze()
        };
        TEXT_BASE64.encode(bytes).into()
    } else {
        frame
    };
    // Web status belongs in the DATA trailer frame, not HTTP/2 trailers.
    Ok((response, HeaderMap::new(), Some(body)))
}

/// Stateful gRPC-Web body encoder for `grpc-web-text`.
#[derive(Default)]
pub(crate) struct TextEncoder {
    carry: BytesMut,
}

impl TextEncoder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Copy the bounded raw suffix for a surviving terminal response owner.
    pub(crate) fn carry(&self) -> ([u8; 2], usize) {
        let len = self.carry.len();
        assert!(len <= 2, "base64 carry is at most two bytes");
        let mut bytes = [0; 2];
        for (output, input) in bytes.iter_mut().zip(self.carry.iter()) {
            *output = *input;
        }
        (bytes, len)
    }

    fn encode(&mut self, bytes: Bytes, end: bool) -> Option<Bytes> {
        if self.carry.is_empty() {
            return encode_text_chunk(bytes, end, &mut self.carry);
        }
        self.carry.extend_from_slice(&bytes);
        let combined = std::mem::take(&mut self.carry).freeze();
        encode_text_chunk(combined, end, &mut self.carry)
    }
}

fn encode_text_chunk(bytes: Bytes, end: bool, carry: &mut BytesMut) -> Option<Bytes> {
    let full = if end {
        bytes.len()
    } else {
        bytes.len() / 3 * 3
    };
    let (encode, rest) = bytes.split_at(full);
    if !rest.is_empty() {
        carry.extend_from_slice(rest);
    }
    if encode.is_empty() {
        return None;
    }
    Some(TEXT_BASE64.encode(encode).into())
}

/// Send one gRPC message frame, base64-encoding when this is grpc-web-text.
pub(crate) async fn send_frame(
    send: &mut backend::SendStream,
    mode: Mode,
    text: &mut TextEncoder,
    frame: SegFrame,
    send_buffer: usize,
) -> Result<(), Status> {
    if !mode.is_text() {
        return crate::wire::send_frame(send, frame, false, send_buffer).await;
    }
    for segment in frame.into_segments() {
        if let Some(encoded) = text.encode(segment, false) {
            send_bytes(send, encoded, false, send_buffer).await?;
        }
    }
    Ok(())
}

/// Send the body-encoded gRPC-Web trailer frame and end the stream.
pub(crate) async fn send_trailers(
    send: &mut backend::SendStream,
    mode: Mode,
    text: &mut TextEncoder,
    status: &Status,
    send_buffer: usize,
) -> Result<(), Status> {
    let frame = trailer_frame(status)?;
    if !mode.is_text() {
        return send_bytes(send, frame, true, send_buffer).await;
    }
    if let Some(encoded) = text.encode(frame, true) {
        send_bytes(send, encoded, true, send_buffer).await
    } else {
        send_bytes(send, Bytes::new(), true, send_buffer).await
    }
}

/// Decode a unary grpc-web-text request body and parse its single message.
///
/// Decode bounded batches as DATA arrives, so the gRPC frame length is checked
/// before the complete HTTP body is buffered or the peer half-closes.
pub(crate) async fn read_one_text_message<T: CodecMessage>(
    recv: &mut backend::RecvStream,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    tap: Option<&CallLogger>,
) -> Result<Framed<T>, Status> {
    // A DATA frame can be much larger than the configured message cap. Decode
    // in bounded pieces, keeping at most one incomplete base64 quantum.
    const ENCODED_BATCH: usize = 4096;
    let mut encoded = Vec::new();
    let mut reader = FrameReader::new(limits);
    let mut found = None;
    let mut padded = false;
    while let Some(mut chunk) = next_data(recv).await? {
        let n = chunk.len();
        release(recv, n)?;
        while !chunk.is_empty() {
            if padded {
                return Err(Status::invalid_argument(
                    "grpc-web-text base64: data after padding",
                ));
            }
            let take = (ENCODED_BATCH - encoded.len()).min(chunk.len());
            let (part, _) = chunk.split_at(take);
            encoded.extend_from_slice(part);
            chunk.advance(take);

            let complete = encoded.len() / 4 * 4;
            if complete == 0 {
                continue;
            }
            let (part, tail) = encoded.split_at(complete);
            padded = part.contains(&b'=');
            if padded && (!tail.is_empty() || !chunk.is_empty()) {
                return Err(Status::invalid_argument(
                    "grpc-web-text base64: data after padding",
                ));
            }
            read_text_part::<T>(
                part,
                &mut reader,
                &mut found,
                limits,
                accept_gzip,
                codec,
                tap,
            )?;
            let remaining = encoded.len() - complete;
            encoded.copy_within(complete.., 0);
            encoded.truncate(remaining);
        }
    }
    if !encoded.is_empty() {
        read_text_part::<T>(
            &encoded,
            &mut reader,
            &mut found,
            limits,
            accept_gzip,
            codec,
            tap,
        )?;
    }
    // As before, decode the first protobuf message before reporting a
    // truncated following frame. Decoding also applies the inflated-size cap.
    let found = found
        .map(|frame| decode_text_message::<T>(frame, limits, accept_gzip, codec, tap))
        .transpose()?;
    reader.finish()?;
    if let Some(tap) = tap.filter(|tap| matches!(tap.role(), Logger::Server)) {
        tap.log_half_close();
    }
    found.ok_or_else(|| Status::internal("Missing request message."))
}

/// Decode one complete group of base64 quanta, or the final unpadded suffix.
fn read_text_part<T: CodecMessage>(
    encoded: &[u8],
    reader: &mut FrameReader,
    found: &mut Option<crate::codec::Frame>,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    tap: Option<&CallLogger>,
) -> Result<(), Status> {
    let decoded = TEXT_BASE64
        .decode(encoded)
        .map_err(|e| Status::invalid_argument(format!("grpc-web-text base64: {e}")))?;
    reader.push(Bytes::from(decoded));
    loop {
        match reader.next_frame() {
            Ok(Some(frame)) => {
                if let Some(first) = found.take() {
                    // The original reader decodes the first message before
                    // returning a duplicate-message error.
                    decode_text_message::<T>(first, limits, accept_gzip, codec, tap)?;
                    return Err(Status::internal("unary rpc received more than one message"));
                }
                *found = Some(frame);
            }
            Ok(None) => return Ok(()),
            Err(status) => {
                if let Some(first) = found.take() {
                    decode_text_message::<T>(first, limits, accept_gzip, codec, tap)?;
                }
                return Err(status);
            }
        }
    }
}

fn decode_text_message<T: CodecMessage>(
    frame: crate::codec::Frame,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Option<CompressionAlgorithm>,
    tap: Option<&CallLogger>,
) -> Result<Framed<T>, Status> {
    if let Some(tap) = tap {
        tap.log_read(&frame.payload);
    }
    decode_frame(frame, limits, accept_gzip, codec)
}

fn trailer_frame(status: &Status) -> Result<Bytes, Status> {
    let trailers = grpc_trailers(status)?;
    let mut block = BytesMut::new();
    for (name, value) in &trailers {
        block.extend_from_slice(name.as_str().as_bytes());
        block.extend_from_slice(b": ");
        block.extend_from_slice(value.as_bytes());
        block.extend_from_slice(b"\r\n");
    }
    let len = u32::try_from(block.len())
        .map_err(|_| Status::internal("gRPC-Web trailer block too large"))?;
    let mut frame = BytesMut::with_capacity(5 + block.len());
    frame.put_u8(0x80);
    frame.put_u32(len);
    frame.extend_from_slice(&block);
    Ok(frame.freeze())
}

#[cfg(test)]
mod tests {
    use super::{CorsPolicy, Mode, TextEncoder, WebContext, deadline_response, trailer_frame};
    use crate::status::{Code, Status};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use bytes::{BufMut, BytesMut};
    use http::{HeaderMap, HeaderValue, StatusCode};

    #[test]
    fn response_cors_matches_origin_and_preserves_vary() {
        for (policy, origin, expected) in [
            (CorsPolicy::DenyAll, Some("https://app.example"), None),
            (CorsPolicy::AllowAny, Some("https://app.example"), Some("*")),
            (CorsPolicy::AllowAny, None, None),
            (
                CorsPolicy::AllowOrigin("https://app.example"),
                Some("https://app.example"),
                Some("https://app.example"),
            ),
            (
                CorsPolicy::AllowOrigin("https://app.example"),
                Some("https://other.example"),
                None,
            ),
            (CorsPolicy::AllowOrigin("https://app.example"), None, None),
            (CorsPolicy::AllowOrigin("null"), Some("null"), Some("null")),
        ] {
            let mut request = HeaderMap::new();
            if let Some(origin) = origin {
                request.insert(http::header::ORIGIN, HeaderValue::from_static(origin));
            }
            let web = WebContext::new(Mode::Binary, &request, policy, true);
            let mut response = HeaderMap::new();
            response.insert(
                http::header::VARY,
                HeaderValue::from_static("accept-encoding"),
            );
            web.apply_cors(&mut response);
            assert_eq!(
                response
                    .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|value| value.to_str().ok()),
                expected
            );
            assert_eq!(
                response
                    .get_all(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .iter()
                    .count(),
                if expected.is_some() { 1 } else { 0 }
            );
            let vary: Vec<_> = response
                .get_all(http::header::VARY)
                .iter()
                .map(|value| value.to_str().expect("vary"))
                .collect();
            assert_eq!(vary, ["accept-encoding", "origin"]);
            assert_eq!(
                response
                    .get(http::header::ACCESS_CONTROL_EXPOSE_HEADERS)
                    .and_then(|value| value.to_str().ok()),
                expected.map(|_| "*")
            );

            let mut explicit = HeaderMap::new();
            explicit.insert(
                http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("https://override.example"),
            );
            explicit.insert(
                http::header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("x-custom"),
            );
            web.apply_cors(&mut explicit);
            assert_eq!(
                explicit
                    .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|value| value.to_str().ok()),
                Some("https://override.example")
            );
            assert_eq!(
                explicit
                    .get(http::header::ACCESS_CONTROL_EXPOSE_HEADERS)
                    .and_then(|value| value.to_str().ok()),
                Some("x-custom")
            );

            let mut origin_only = HeaderMap::new();
            origin_only.insert(
                http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("https://override.example"),
            );
            web.apply_cors(&mut origin_only);
            assert_eq!(
                origin_only
                    .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|value| value.to_str().ok()),
                Some("https://override.example")
            );
            assert!(
                !origin_only.contains_key(http::header::ACCESS_CONTROL_EXPOSE_HEADERS),
                "application-managed origins must not gain automatic exposure"
            );

            let mut exposure_only = HeaderMap::new();
            exposure_only.insert(
                http::header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("x-custom"),
            );
            web.apply_cors(&mut exposure_only);
            assert_eq!(
                exposure_only
                    .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .and_then(|value| value.to_str().ok()),
                expected
            );
            assert_eq!(
                exposure_only
                    .get(http::header::ACCESS_CONTROL_EXPOSE_HEADERS)
                    .and_then(|value| value.to_str().ok()),
                Some("x-custom")
            );

            let disabled = WebContext::new(Mode::Binary, &request, policy, false);
            let mut unchanged = HeaderMap::new();
            unchanged.insert(
                http::header::VARY,
                HeaderValue::from_static("accept-encoding"),
            );
            unchanged.insert(
                http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("https://override.example"),
            );
            let before = unchanged.clone();
            disabled.apply_cors(&mut unchanged);
            assert_eq!(unchanged, before);
            let mut empty = HeaderMap::new();
            disabled.apply_cors(&mut empty);
            assert!(empty.is_empty());
        }

        let web = WebContext::new(Mode::Binary, &HeaderMap::new(), CorsPolicy::DenyAll, true);
        let mut response = HeaderMap::new();
        response.insert(http::header::VARY, HeaderValue::from_static("*"));
        web.apply_cors(&mut response);
        assert_eq!(response.get_all(http::header::VARY).iter().count(), 1);
        assert_eq!(
            response.get(http::header::VARY),
            Some(&HeaderValue::from_static("*"))
        );
    }

    #[test]
    fn parses_content_types() {
        assert_eq!(
            Mode::from_content_type("application/grpc-web"),
            Some(Mode::Binary)
        );
        assert_eq!(
            Mode::from_content_type("application/grpc-web+proto; charset=utf-8"),
            Some(Mode::BinaryProto)
        );
        assert_eq!(
            Mode::from_content_type("application/grpc-web-text"),
            Some(Mode::Text)
        );
        assert_eq!(
            Mode::from_content_type("application/grpc-web-text+proto"),
            Some(Mode::TextProto)
        );
        assert_eq!(Mode::from_content_type("application/grpc+proto"), None);
    }

    #[test]
    fn encodes_trailer_frame() {
        let frame = trailer_frame(&Status::ok()).expect("frame");
        assert_eq!(frame.first(), Some(&0x80));
        assert!(
            frame
                .windows("grpc-status: 0\r\n".len())
                .any(|window| { window == b"grpc-status: 0\r\n" })
        );
    }

    #[test]
    fn deadline_status_uses_a_complete_body_trailer_in_every_web_mode() {
        let status = Status::new(Code::DeadlineExceeded, "elapsed");
        for (mode, content_type) in [
            (Mode::Binary, "application/grpc-web"),
            (Mode::BinaryProto, "application/grpc-web+proto"),
            (Mode::Text, "application/grpc-web-text"),
            (Mode::TextProto, "application/grpc-web-text+proto"),
        ] {
            let (response, trailers, body) =
                deadline_response(mode, &status, &[]).expect("web deadline ending");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[http::header::CONTENT_TYPE], content_type);
            assert!(!response.headers().contains_key("grpc-status"));
            assert!(trailers.is_empty());
            let body = body.expect("web status is DATA-encoded");
            let decoded = if mode.is_text() {
                STANDARD.decode(body).expect("complete padded base64")
            } else {
                body.to_vec()
            };
            assert_eq!(decoded[0], 0x80);
            let declared = u32::from_be_bytes(decoded[1..5].try_into().expect("frame length"));
            assert_eq!(
                usize::try_from(declared).expect("body length"),
                decoded.len() - 5
            );
            let block = std::str::from_utf8(&decoded[5..]).expect("ASCII trailer block");
            assert!(block.lines().any(|line| line == "grpc-status: 4"));
            assert!(block.lines().any(|line| line == "grpc-message: elapsed"));
            assert!(block.ends_with("\r\n"));
        }
    }

    #[test]
    fn deadline_text_ending_preserves_each_possible_raw_carry() {
        let status = Status::deadline_exceeded();
        for mode in [Mode::Text, Mode::TextProto] {
            for length in 1u8..=3 {
                let mut frame = BytesMut::new();
                frame.put_u8(0);
                frame.put_u32(u32::from(length));
                frame.extend_from_slice(&[0x61; 3][..usize::from(length)]);
                let raw = frame.freeze();
                let mut text = TextEncoder::new();
                let prefix = text.encode(raw.clone(), false).unwrap_or_default();
                let (carry, count) = text.carry();
                assert_eq!(count, raw.len() % 3);
                drop(text);
                let (_, _, ending) = deadline_response(mode, &status, &carry[..count])
                    .expect("deadline resumes the destroyed writer's raw suffix");
                let mut body = prefix.to_vec();
                body.extend_from_slice(&ending.expect("encoded trailer"));
                let decoded = STANDARD.decode(body).expect("continuous padded base64");
                let mut expected = raw.to_vec();
                expected.extend_from_slice(&trailer_frame(&status).expect("trailer"));
                assert_eq!(decoded, expected, "raw carry length {count}");
            }
        }
    }
}
