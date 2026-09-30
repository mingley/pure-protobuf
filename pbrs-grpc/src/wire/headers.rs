//! Request/response headers: grpc-* parsing, timeouts, content checks.

use crate::compression::CompressionAlgorithm;
use crate::metadata::Metadata;
use crate::status::Status;
use crate::transport::h2::RecvStream;
use http::uri::{Authority, PathAndQuery, Scheme};
use http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode};
use std::time::Duration;

pub(crate) const GRPC_STATUS: HeaderName = HeaderName::from_static("grpc-status");
pub(crate) const GRPC_MESSAGE: HeaderName = HeaderName::from_static("grpc-message");
pub(crate) const GRPC_STATUS_DETAILS_BIN: HeaderName =
    HeaderName::from_static("grpc-status-details-bin");
pub(crate) const GRPC_RETRY_PUSHBACK_MS: HeaderName =
    HeaderName::from_static("grpc-retry-pushback-ms");
pub(crate) const GRPC_TIMEOUT: HeaderName = HeaderName::from_static("grpc-timeout");
pub(crate) const GRPC_ENCODING: HeaderName = HeaderName::from_static("grpc-encoding");
pub(crate) const GRPC_ACCEPT_ENCODING: HeaderName = HeaderName::from_static("grpc-accept-encoding");
pub(crate) const USER_AGENT: HeaderName = HeaderName::from_static("user-agent");
pub(crate) const APPLICATION_GRPC: HeaderValue = HeaderValue::from_static("application/grpc");
pub(crate) const TRAILERS: HeaderValue = HeaderValue::from_static("trailers");
#[cfg(not(feature = "zstd"))]
pub(crate) const IDENTITY_GZIP_DEFLATE: HeaderValue =
    HeaderValue::from_static("identity,gzip,deflate");
#[cfg(feature = "zstd")]
pub(crate) const IDENTITY_GZIP_DEFLATE: HeaderValue =
    HeaderValue::from_static("identity,gzip,deflate,zstd");
pub(crate) const IDENTITY: HeaderValue = HeaderValue::from_static("identity");
pub(crate) const GZIP: HeaderValue = HeaderValue::from_static("gzip");
pub(crate) const DEFLATE: HeaderValue = HeaderValue::from_static("deflate");
#[cfg(feature = "zstd")]
pub(crate) const ZSTD: HeaderValue = HeaderValue::from_static("zstd");
pub(crate) const STATUS_OK: HeaderValue = HeaderValue::from_static("0");

/// Kernel identity stamped on every outbound RPC. Prefixed by
/// [`crate::Channel::user_agent`].
pub(crate) const DEFAULT_UA: &str = concat!("pbrs-grpc/", env!("CARGO_PKG_VERSION"));
pub(crate) const PBRS_GRPC_UA: HeaderValue = HeaderValue::from_static(DEFAULT_UA);

/// `"{prefix} pbrs-grpc/<version>"`, matching grpc-go `WithUserAgent`.
pub(crate) fn user_agent_value(prefix: &str) -> Result<HeaderValue, Status> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return Ok(PBRS_GRPC_UA);
    }
    HeaderValue::from_str(&format!("{prefix} {DEFAULT_UA}"))
        .map_err(|_| Status::invalid_argument("user-agent is not valid HTTP"))
}

/// `grpc-accept-encoding` this process advertises.
pub(crate) fn accept_encoding_value(gzip: bool) -> HeaderValue {
    if gzip {
        IDENTITY_GZIP_DEFLATE
    } else {
        IDENTITY
    }
}

/// `grpc-encoding` value for an outbound `codec`.
pub(crate) fn encoding_value(codec: CompressionAlgorithm) -> HeaderValue {
    match codec {
        CompressionAlgorithm::Gzip => GZIP,
        CompressionAlgorithm::Deflate => DEFLATE,
        #[cfg(feature = "zstd")]
        CompressionAlgorithm::Zstd => ZSTD,
    }
}

/// Headers a gRPC-Web response carries before user metadata, rounded to
/// what `HeaderMap` will actually allocate. Sizing up front avoids a rehash.
#[cfg(feature = "grpc-web")]
pub(crate) const HEADER_CAPACITY: usize = 10;

#[allow(
    clippy::too_many_arguments,
    reason = "gRPC request headers: routing, timeout, gzip offer, identity"
)]
pub(crate) fn grpc_request(
    authority: &Authority,
    path: &'static str,
    md: &Metadata,
    timeout: Option<Duration>,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> Result<Request<()>, Status> {
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(if https { Scheme::HTTPS } else { Scheme::HTTP });
    parts.authority = Some(authority.clone());
    parts.path_and_query = Some(PathAndQuery::from_static(path));
    let uri = http::Uri::from_parts(parts).map_err(|e| Status::internal(e.to_string()))?;
    let mut req = Request::new(());
    *req.method_mut() = http::Method::POST;
    *req.uri_mut() = uri;
    // Exact sizing: the fixed headers plus user metadata. A minimal table
    // for the common no-metadata call, and never a rehash past it.
    let capacity =
        4 + md.len() + usize::from(send_codec.is_some()) + usize::from(timeout.is_some());
    *req.headers_mut() = HeaderMap::with_capacity(capacity);
    let headers = req.headers_mut();
    headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
    headers.insert(http::header::TE, TRAILERS);
    headers.insert(GRPC_ACCEPT_ENCODING, accept_encoding_value(accept_gzip));
    if let Some(codec) = send_codec {
        headers.insert(GRPC_ENCODING, encoding_value(codec));
    }
    if let Some(d) = timeout {
        let val = HeaderValue::from_str(&crate::timeout::encode_timeout(d))
            .map_err(|e| Status::internal(e.to_string()))?;
        headers.insert(GRPC_TIMEOUT, val);
    }
    md.write_to(headers)?;
    // After user metadata so a `user-agent` smuggled in metadata cannot win.
    headers.insert(USER_AGENT, user_agent.clone());
    Ok(req)
}

/// Content-coding token in `grpc-encoding`: trim whitespace and a trailing
/// `;parameter`. Case is kept.
pub(crate) fn encoding_token(value: &str) -> &str {
    match value.split_once(';') {
        Some((coding, _)) => coding.trim(),
        None => value.trim(),
    }
}

/// The peer's `grpc-encoding` token, if it sent a non-identity coding.
///
/// Missing, empty, or `identity` (any case, optional `;parameter`) is `None` —
/// the spec treats those as the same coding. The token is trimmed; case is
/// kept (`"GZIP"` stays `"GZIP"`). [`grpc_encoding_supported`] is what the
/// kernel used to admit it.
pub(crate) fn grpc_encoding(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(GRPC_ENCODING).and_then(|v| v.to_str().ok())?;
    let token = encoding_token(raw);
    if token.is_empty() || token.eq_ignore_ascii_case("identity") {
        None
    } else {
        Some(token)
    }
}

/// Whether the peer advertised `codec` in `grpc-accept-encoding`.
///
/// Tokens are comma-separated; a `q=` parameter is ignored. Missing or
/// unreadable header means identity only — never compress for a peer that
/// did not ask for it.
pub(crate) fn accepts_codec(headers: &HeaderMap, codec: CompressionAlgorithm) -> bool {
    let Some(value) = headers.get(GRPC_ACCEPT_ENCODING) else {
        return false;
    };
    // Fast path: our own clients (and the common peer set) advertise exactly
    // the codings we emit. A byte compare skips UTF-8 validation, splitting,
    // and trimming; anything else falls through to the slow path.
    if *value == IDENTITY_GZIP_DEFLATE {
        #[cfg(feature = "zstd")]
        return true;
        #[cfg(not(feature = "zstd"))]
        return matches!(
            codec,
            CompressionAlgorithm::Gzip | CompressionAlgorithm::Deflate
        );
    }
    let want = codec.name();
    value.to_str().is_ok_and(|raw| {
        raw.split(',').any(|part| {
            part.split(';')
                .next()
                .is_some_and(|token| token.trim().eq_ignore_ascii_case(want))
        })
    })
}

/// Whether the peer advertised gzip in `grpc-accept-encoding`.
pub(crate) fn accepts_gzip(headers: &HeaderMap) -> bool {
    accepts_codec(headers, CompressionAlgorithm::Gzip)
}

/// The coding compressed inbound frames on this RPC use.
///
/// Parsed from the peer's `grpc-encoding` token. Missing, empty, or
/// `identity` is [`CompressionAlgorithm::Gzip`]: a Compressed-Flag with no usable token
/// inflates as gzip, matching the pre-deflate behavior. Callers admit the
/// token first ([`check_request`] on the server,
/// [`super::frame_reader::refuse_encoding_reply`] on the client), so an
/// unknown coding never reaches the decoder.
pub(crate) fn inbound_codec(headers: &HeaderMap) -> CompressionAlgorithm {
    inbound_codec_from_token(grpc_encoding(headers))
}

/// [`inbound_codec`] when the caller already fetched the token, so one
/// header lookup serves the refuse check, the decode, and the envelope.
pub(crate) fn inbound_codec_from_token(token: Option<&str>) -> CompressionAlgorithm {
    token
        .and_then(CompressionAlgorithm::parse)
        .unwrap_or_default()
}

/// The best coding the peer accepts, preferring `configured`.
///
/// Falls back to the other coding when the peer only accepts that one, so
/// a deflate-configured server still compresses for a gzip-only peer (and
/// vice versa) instead of silently sending identity. `None` means the peer
/// accepts neither.
pub(crate) fn preferred_codec(
    configured: CompressionAlgorithm,
    peer_gzip: bool,
    peer_deflate: bool,
    #[cfg(feature = "zstd")] peer_zstd: bool,
) -> Option<CompressionAlgorithm> {
    let accepts = |codec| match codec {
        CompressionAlgorithm::Gzip => peer_gzip,
        CompressionAlgorithm::Deflate => peer_deflate,
        #[cfg(feature = "zstd")]
        CompressionAlgorithm::Zstd => peer_zstd,
    };
    if accepts(configured) {
        return Some(configured);
    }
    [
        CompressionAlgorithm::Gzip,
        CompressionAlgorithm::Deflate,
        #[cfg(feature = "zstd")]
        CompressionAlgorithm::Zstd,
    ]
    .into_iter()
    .find(|&fallback| fallback != configured && accepts(fallback))
}

/// Compress this payload only if the handler (or config, when the handler
/// omitted a choice) asked, and the peer accepts the negotiated coding.
///
/// `None` follows `send` (fill-if-unset). `Some(false)` opts out of a
/// [`crate::ServerConfig::send_compressed`] overlay.
pub(crate) fn select_outbound_codec(
    handler: Option<bool>,
    send: bool,
    negotiated: Option<CompressionAlgorithm>,
) -> Option<CompressionAlgorithm> {
    negotiated.filter(|_| handler.unwrap_or(send))
}

/// Per-message Compressed-Flag on a server stream.
///
/// `send_compressed` (`framed`) stays compressed when the peer accepts the
/// negotiated coding. Identity `send` frames follow the overlay unless the
/// envelope opted out with [`crate::Response::set_compress`]`(false)`.
/// [`crate::Response::set_compress`]`(true)` advertises the negotiated
/// `grpc-encoding` and does not rewrite identity frames, so a mixed stream
/// keeps both flags.
pub(crate) fn select_stream_codec(
    framed: bool,
    envelope: Option<bool>,
    send: bool,
    negotiated: Option<CompressionAlgorithm>,
) -> Option<CompressionAlgorithm> {
    select_outbound_codec(
        if framed {
            Some(true)
        } else if envelope == Some(false) {
            Some(false)
        } else {
            None
        },
        send,
        negotiated,
    )
}

/// How [`check_request`] turns a request away.
pub(crate) enum RequestReject {
    /// Trailers-only gRPC status on HTTP 200.
    Grpc(Status),
    /// Bare HTTP status, no `grpc-status`. Used when the request is not gRPC
    /// (wrong method or content-type), per PROTOCOL-HTTP2.md.
    Http(StatusCode),
}

/// `application/grpc` and `application/grpc+proto`, with optional parameters.
///
/// Type and subtype are matched case-insensitively. This kernel only
/// decodes protobuf messages, so `application/grpc+json` and other `+`
/// suffixes are not native gRPC here: they are HTTP 415 unless another
/// feature explicitly admits them. Without the `grpc-web` feature,
/// `application/grpc-web` is not a match either (`-web` is not `+` / `;`).
pub(crate) fn grpc_content_type(ct: &str) -> bool {
    // Fast path: exact tokens cover pbrs/tonic/go peers without trimming,
    // splitting, or case folding; anything else falls through below.
    if ct == "application/grpc" || ct == "application/grpc+proto" {
        return true;
    }
    let ct = ct.trim();
    let Some((ty, rest)) = ct.split_once('/') else {
        return false;
    };
    if !ty.eq_ignore_ascii_case("application") {
        return false;
    }
    let subtype = match rest.split_once(';') {
        Some((sub, _)) => sub.trim(),
        None => rest.trim(),
    };
    subtype.eq_ignore_ascii_case("grpc") || subtype.eq_ignore_ascii_case("grpc+proto")
}

/// Whether `grpc-encoding` is identity, or a registry coding when `gzip`
/// is true.
///
/// HTTP content-codings are case-insensitive. Surrounding whitespace and a
/// trailing `;parameter` (a `q=` some peers copy from accept-encoding) are
/// ignored. Anything else is unsupported.
#[must_use]
pub(crate) fn grpc_encoding_supported(value: &str) -> bool {
    grpc_encoding_admitted(value, true)
}

/// Admit identity always, gzip and deflate only when `gzip` is true.
pub(crate) fn grpc_encoding_admitted(value: &str, gzip: bool) -> bool {
    let token = encoding_token(value);
    token.eq_ignore_ascii_case("identity") || (gzip && CompressionAlgorithm::parse(token).is_some())
}

/// Trailers-only status for an encoding this process will not inflate.
pub(crate) fn encoding_not_supported(gzip: bool) -> Status {
    Status::unimplemented(if gzip {
        #[cfg(feature = "zstd")]
        {
            "grpc-encoding not supported; this server accepts identity, gzip, deflate, and zstd"
        }
        #[cfg(not(feature = "zstd"))]
        {
            "grpc-encoding not supported; this server accepts identity, gzip, and deflate"
        }
    } else {
        "grpc-encoding not supported; this server accepts identity"
    })
}

/// Reject anything that is not a gRPC request we can answer.
///
/// Runs before a handler is spawned, so a malformed or unsupported request
/// costs one response and no RPC slot. Non-POST is HTTP 405; a content-type
/// that is not protobuf gRPC (`application/grpc+json`, grpc-web, JSON, a
/// missing type) is HTTP 415, so a browser does not take HTTP 200 as
/// success. Unsupported `grpc-encoding` stays a gRPC `UNIMPLEMENTED`.
pub(crate) fn check_request(
    request: &Request<RecvStream>,
    accept_gzip: bool,
) -> Result<(), RequestReject> {
    if request.method() != http::Method::POST {
        return Err(RequestReject::Http(StatusCode::METHOD_NOT_ALLOWED));
    }
    let Some(ct) = request.headers().get(http::header::CONTENT_TYPE) else {
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    };
    let Ok(ct) = ct.to_str() else {
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    };
    if !grpc_content_type(ct) {
        #[cfg(feature = "grpc-web")]
        if crate::web::is_supported_content_type(ct) {
            return Ok(());
        }
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    }
    if let Some(enc) = request.headers().get(GRPC_ENCODING) {
        let supported = enc.to_str().is_ok_and(|value| {
            if accept_gzip {
                grpc_encoding_supported(value)
            } else {
                grpc_encoding_admitted(value, false)
            }
        });
        if !supported {
            return Err(RequestReject::Grpc(encoding_not_supported(accept_gzip)));
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

/// The deadline from the request headers and the server cap. An interceptor
/// cap is applied on top by [`crate::Rpc::effective_timeout`].
pub(crate) fn effective_timeout(headers: &HeaderMap, server: Option<Duration>) -> Option<Duration> {
    soonest(timeout_from_headers(headers), server)
}

pub(crate) fn soonest(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}
