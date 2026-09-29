//! gRPC-Web framing over the existing HTTP/2 transport.

use crate::binlog::{CallLogger, Logger};
use crate::codec::CodecMessage;
use crate::compression::Codec;
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
use bytes::{BufMut, Bytes, BytesMut};
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
const ACCESS_CONTROL_MAX_AGE: &str = "access-control-max-age";
const VARY: &str = "vary";
const DEFAULT_ALLOW_HEADERS: &str =
    "content-type,x-grpc-web,grpc-timeout,grpc-encoding,grpc-accept-encoding";

/// CORS policy for gRPC-Web preflight requests.
///
/// Default is [`Self::DenyAll`]. Use [`crate::ServerConfig::grpc_web_allow_any_origin`]
/// or [`crate::ServerConfig::grpc_web_allow_origin`] to opt in.
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
    fn allowed_origin(self, origin: &HeaderValue) -> Option<HeaderValue> {
        match self {
            Self::DenyAll => None,
            Self::AllowAny => Some(HeaderValue::from_static("*")),
            Self::AllowOrigin(allowed) => {
                let origin = origin.to_str().ok()?;
                if origin == allowed {
                    HeaderValue::from_str(allowed).ok()
                } else {
                    None
                }
            }
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
    headers.insert(VARY, HeaderValue::from_static("origin"));
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

/// Send initial gRPC-Web response headers.
pub(crate) fn send_ok_headers(
    respond: &mut backend::SendResponse,
    mode: Mode,
    md: &Metadata,
    send_codec: Option<Codec>,
    accept_gzip: bool,
) -> Result<backend::SendStream, Status> {
    let mut res = Response::new(());
    *res.status_mut() = StatusCode::OK;
    *res.headers_mut() = HeaderMap::with_capacity(HEADER_CAPACITY);
    let headers = res.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, mode.content_type());
    headers.insert(GRPC_ACCEPT_ENCODING, accept_encoding_value(accept_gzip));
    if let Some(codec) = send_codec {
        headers.insert(GRPC_ENCODING, encoding_value(codec));
    }
    md.write_to(headers)?;
    respond
        .send_response(res, false)
        .map_err(|e| Status::internal(e.to_string()))
}

/// Send a gRPC-Web trailers-only body response.
pub(crate) fn send_trailers_only(
    respond: &mut backend::SendResponse,
    mode: Mode,
    status: Status,
    extra_headers: &Metadata,
) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, mode.content_type())
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    extra_headers.write_to(res.headers_mut()).ok();
    let Ok(mut send) = respond.send_response(res, false) else {
        return;
    };
    if let Ok(frame) = trailer_frame(&status) {
        let frame = if mode.is_text() {
            TEXT_BASE64.encode(frame).into()
        } else {
            frame
        };
        send.send_data(frame, true).ok();
    }
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
pub(crate) async fn read_one_text_message<T: CodecMessage>(
    recv: &mut backend::RecvStream,
    limits: MessageLimits,
    accept_gzip: bool,
    codec: Codec,
    tap: Option<&CallLogger>,
) -> Result<Framed<T>, Status> {
    let mut encoded = BytesMut::new();
    while let Some(chunk) = next_data(recv).await? {
        let n = chunk.len();
        encoded.extend_from_slice(&chunk);
        release(recv, n)?;
    }
    let decoded = TEXT_BASE64
        .decode(&encoded)
        .map_err(|e| Status::invalid_argument(format!("grpc-web-text base64: {e}")))?;
    let mut reader = FrameReader::new(limits);
    reader.push(Bytes::from(decoded));
    let mut found = None;
    while let Some(frame) = reader.next_frame()? {
        if found.is_some() {
            return Err(Status::internal("unary rpc received more than one message"));
        }
        if let Some(tap) = tap {
            tap.log_read(&frame.payload);
        }
        found = Some(decode_frame(frame, limits, accept_gzip, codec)?);
    }
    reader.finish()?;
    if let Some(tap) = tap.filter(|tap| matches!(tap.role(), Logger::Server)) {
        tap.log_half_close();
    }
    Ok(found.unwrap_or_else(|| Framed::new(T::empty())))
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
    use super::{Mode, trailer_frame};
    use crate::status::Status;

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
}
