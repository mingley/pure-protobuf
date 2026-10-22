//! Request/response headers: grpc-* parsing, timeouts, content checks.

use crate::compression::CompressionAlgorithm;
use crate::metadata::Metadata;
use crate::status::Status;
use crate::transport::h2::RecvStream;
use http::uri::{Authority, PathAndQuery, Scheme};
use http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode};
use std::sync::OnceLock;
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
// One address. A `const` HeaderValue is rebuilt per codegen unit, so the
// channel's copy does not share bytes with the value `grpc_request` compares.
pub(crate) static STOCK_UA: HeaderValue = HeaderValue::from_static(DEFAULT_UA);

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

/// One `:path` per process. Callers pass a `'static` method path, so a hit
/// is two atomics and a copy of the static path. Parsing stays off this path.
fn cached_static_path(path: &'static str) -> PathAndQuery {
    use std::sync::atomic::Ordering;
    let ptr = path.as_ptr();
    if static_path::PTR.load(Ordering::Acquire) == ptr as *mut u8
        && static_path::LEN.load(Ordering::Relaxed) == path.len()
    {
        return crate::static_copy::copy_static_path(static_path::PARSED.get().unwrap());
    }
    cached_static_path_miss(path)
}

#[cold]
#[inline(never)]
fn cached_static_path_miss(path: &'static str) -> PathAndQuery {
    use std::cell::RefCell;
    use std::sync::atomic::Ordering;
    thread_local! {
        static LAST: RefCell<Option<(&'static str, PathAndQuery)>> = const { RefCell::new(None) };
    }
    LAST.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((prev, parsed)) = slot.as_ref() {
            if std::ptr::eq(*prev, path) {
                return parsed.clone();
            }
        }
        let parsed = PathAndQuery::from_static(path);
        *slot = Some((path, parsed.clone()));
        if static_path::PTR.load(Ordering::Relaxed).is_null()
            && static_path::PARSED.set(parsed.clone()).is_ok()
        {
            static_path::LEN.store(path.len(), Ordering::Relaxed);
            static_path::PTR.store(path.as_ptr() as *mut u8, Ordering::Release);
        }
        parsed
    })
}

mod static_path {
    use std::sync::atomic::{AtomicPtr, AtomicUsize};

    pub(super) static PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    pub(super) static LEN: AtomicUsize = AtomicUsize::new(0);
    pub(super) static PARSED: std::sync::OnceLock<super::PathAndQuery> = std::sync::OnceLock::new();
}

/// One `:authority` per process. The channel keeps its own `Authority`; a hit
/// copies the leaked static one instead of cloning the `Bytes`.
fn cached_authority(authority: &Authority) -> Authority {
    use std::sync::atomic::Ordering;
    let text = authority.as_str();
    if static_authority::PTR.load(Ordering::Acquire) == text.as_ptr() as *mut u8
        && static_authority::LEN.load(Ordering::Relaxed) == text.len()
    {
        return crate::static_copy::copy_static_authority(
            &static_authority::ENTRY.get().unwrap().parsed,
        );
    }
    cached_authority_miss(authority)
}

#[cold]
#[inline(never)]
fn cached_authority_miss(authority: &Authority) -> Authority {
    use std::sync::atomic::Ordering;
    let entry = static_authority::ENTRY.get_or_init(|| {
        let leaked: &'static str = Box::leak(authority.as_str().to_owned().into_boxed_str());
        static_authority::Entry {
            original: authority.clone(),
            parsed: Authority::from_static(leaked),
        }
    });
    let original = entry.original.as_str();
    static_authority::LEN.store(original.len(), Ordering::Relaxed);
    static_authority::PTR.store(original.as_ptr() as *mut u8, Ordering::Release);
    if std::ptr::eq(original.as_ptr(), authority.as_str().as_ptr())
        && original.len() == authority.as_str().len()
    {
        crate::static_copy::copy_static_authority(&entry.parsed)
    } else {
        authority.clone()
    }
}

mod static_authority {
    use std::sync::atomic::{AtomicPtr, AtomicUsize};

    pub(super) static PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    pub(super) static LEN: AtomicUsize = AtomicUsize::new(0);
    pub(super) struct Entry {
        // Retain the allocation used as the pointer key after channel drop.
        pub(super) original: super::Authority,
        pub(super) parsed: super::Authority,
    }
    pub(super) static ENTRY: std::sync::OnceLock<Entry> = std::sync::OnceLock::new();
}

#[allow(
    clippy::too_many_arguments,
    reason = "gRPC request headers: routing, timeout, gzip offer, identity"
)]
#[inline(never)]
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
    let stock = is_stock_request(md, timeout, send_codec, accept_gzip, user_agent);
    if stock
        && crate::h2_backend::header_pool::is_parked(crate::h2_backend::header_pool::Slot::Request)
    {
        if let Some(req) = cached_stock_post(https, path, authority) {
            crate::h2_backend::header_pool::set_pending_stock(
                crate::h2_backend::header_pool::STOCK_REQUEST,
            );
            return Ok(req);
        }
    }
    grpc_request_miss(
        authority,
        path,
        md,
        timeout,
        send_codec,
        accept_gzip,
        user_agent,
        https,
        stock,
    )
}

fn is_stock_request(
    md: &Metadata,
    timeout: Option<Duration>,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
) -> bool {
    md.is_empty()
        && timeout.is_none()
        && send_codec.is_none()
        && accept_gzip
        && (std::ptr::eq(user_agent.as_bytes(), STOCK_UA.as_bytes()) || user_agent == &STOCK_UA)
}

/// A repeated stock POST whose pseudos are already saved.
///
/// Returns `true` without copying the 216-byte request. The frame is built
/// from the saved pseudos, so this does not mark the header pool. A deadline
/// still needs that request so a relative timeout can be written into it.
pub(crate) fn begin_stock_request(
    authority: &Authority,
    path: &'static str,
    md: &Metadata,
    timeout: Option<Duration>,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> bool {
    if !is_stock_request(md, timeout, send_codec, accept_gzip, user_agent) {
        return false;
    }
    let auth = authority.as_str();
    // The pointer hit already passed the parked-map check. The stock frame
    // is built from the saved pseudos and does not read that map.
    if crate::h2_backend::header_pool::stock_caller_hit(https, path, auth) {
        return true;
    }
    if !crate::h2_backend::header_pool::is_parked(crate::h2_backend::header_pool::Slot::Request) {
        return false;
    }
    crate::h2_backend::header_pool::stock_post_cached(https, path, auth)
}

/// First request, or one that is not the cached stock POST.
#[cold]
#[inline(never)]
fn grpc_request_miss(
    authority: &Authority,
    path: &'static str,
    md: &Metadata,
    timeout: Option<Duration>,
    send_codec: Option<CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
    stock: bool,
) -> Result<Request<()>, Status> {
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(if https { Scheme::HTTPS } else { Scheme::HTTP });
    parts.authority = Some(cached_authority(authority));
    parts.path_and_query = Some(cached_static_path(path));
    let uri = match http::Uri::from_parts(parts) {
        Ok(uri) => uri,
        Err(e) => {
            crate::h2_backend::header_pool::clear_pending_stock();
            return Err(Status::internal(e.to_string()));
        }
    };
    let mut req = Request::new(());
    *req.method_mut() = http::Method::POST;
    // `Request::new` installs `Uri::default()` (`/` plus empty scheme and
    // authority, all static). Forgetting it skips that drop; nothing is freed.
    std::mem::forget(std::mem::replace(req.uri_mut(), uri));
    // The benchmark and the default client repeat the same four headers.
    // Cloning that block skips four hashed inserts. That path only sets the
    // stock flag; clearing it first would write the thread slot twice.
    if stock {
        // Once the stock map is parked, replay does not read it. Moving it
        // into this request and back on encode copies the map twice.
        let parked = crate::h2_backend::header_pool::is_parked(
            crate::h2_backend::header_pool::Slot::Request,
        );
        if !parked {
            crate::h2_backend::header_pool::replace_map(
                req.headers_mut(),
                crate::h2_backend::header_pool::take(
                    crate::h2_backend::header_pool::Slot::Request,
                    || standard_request_headers().clone(),
                ),
            );
        } else {
            // Empty map and a static URI. Later calls copy this request.
            remember_stock_post(&req, https, path, authority);
        }
        crate::h2_backend::header_pool::set_pending_stock(
            crate::h2_backend::header_pool::STOCK_REQUEST,
        );
        return Ok(req);
    }
    crate::h2_backend::header_pool::clear_pending_stock();
    // Exact sizing: the fixed headers plus user metadata. A minimal table
    // for the common no-metadata call, and never a rehash past it.
    let capacity =
        4 + md.len() + usize::from(send_codec.is_some()) + usize::from(timeout.is_some());
    crate::h2_backend::header_pool::replace_map(
        req.headers_mut(),
        HeaderMap::with_capacity(capacity),
    );
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

struct StockPost {
    https: bool,
    path: usize,
    path_len: usize,
    auth: usize,
    auth_len: usize,
    req: &'static http::Request<()>,
}

fn cached_stock_post(
    https: bool,
    path: &'static str,
    authority: &Authority,
) -> Option<http::Request<()>> {
    let slot = STOCK_POST.get()?;
    let auth = authority.as_str();
    if slot.https == https
        && slot.path == path.as_ptr() as usize
        && slot.path_len == path.len()
        && slot.auth == auth.as_ptr() as usize
        && slot.auth_len == auth.len()
    {
        Some(crate::static_copy::clone_http_request(slot.req))
    } else {
        None
    }
}

fn remember_stock_post(
    req: &http::Request<()>,
    https: bool,
    path: &'static str,
    authority: &Authority,
) {
    STOCK_POST.get_or_init(|| {
        let auth = authority.as_str();
        let owned = Box::leak(Box::new(crate::static_copy::clone_http_request(req)));
        StockPost {
            https,
            path: path.as_ptr() as usize,
            path_len: path.len(),
            auth: auth.as_ptr() as usize,
            auth_len: auth.len(),
            req: owned,
        }
    });
}

static STOCK_POST: std::sync::OnceLock<StockPost> = std::sync::OnceLock::new();

/// `content-type`, `te`, `grpc-accept-encoding`, and the default user-agent.
fn standard_request_headers() -> &'static HeaderMap {
    static HEADERS: OnceLock<HeaderMap> = OnceLock::new();
    HEADERS.get_or_init(|| {
        let mut headers = HeaderMap::with_capacity(4);
        headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
        headers.insert(http::header::TE, TRAILERS);
        headers.insert(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP_DEFLATE);
        headers.insert(USER_AGENT, PBRS_GRPC_UA.clone());
        headers
    })
}

/// `content-type` and `grpc-accept-encoding` for an uncompressed response.
pub(crate) fn standard_response_headers() -> &'static HeaderMap {
    static HEADERS: OnceLock<HeaderMap> = OnceLock::new();
    HEADERS.get_or_init(|| {
        let mut headers = HeaderMap::with_capacity(2);
        headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
        headers.insert(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP_DEFLATE);
        headers
    })
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

/// Accept-encoding, grpc-encoding, and grpc-timeout from one pass.
///
/// The default request is four headers and none of the three names need a
/// hashed lookup. Larger or unusual blocks use the single-key helpers.
pub(crate) fn request_facts(headers: &HeaderMap) -> (bool, bool, Option<&str>, Option<Duration>) {
    if headers.len() > 8 {
        let (gzip, deflate) = accept_pair(headers);
        return (
            gzip,
            deflate,
            grpc_encoding(headers),
            timeout_from_headers(headers),
        );
    }
    let mut gzip = false;
    let mut deflate = false;
    let mut encoding = None;
    let mut timeout = None;
    let mut unusual_accept = false;
    for (name, value) in headers {
        if name == GRPC_ACCEPT_ENCODING && !gzip && !deflate && !unusual_accept {
            if *value == IDENTITY_GZIP_DEFLATE {
                gzip = true;
                deflate = true;
            } else {
                unusual_accept = true;
            }
        } else if name == GRPC_ENCODING && encoding.is_none() {
            encoding = value.to_str().ok().and_then(|raw| {
                let token = encoding_token(raw);
                if token.is_empty() || token.eq_ignore_ascii_case("identity") {
                    None
                } else {
                    Some(token)
                }
            });
        } else if name == GRPC_TIMEOUT && timeout.is_none() {
            timeout = value.to_str().ok().and_then(crate::timeout::parse_timeout);
        }
    }
    if unusual_accept {
        let (gzip, deflate) = accept_pair(headers);
        return (gzip, deflate, encoding, timeout);
    }
    (gzip, deflate, encoding, timeout)
}

/// Gzip and deflate advertisements from one lookup of `grpc-accept-encoding`.
pub(crate) fn accept_pair(headers: &HeaderMap) -> (bool, bool) {
    let Some(value) = headers.get(GRPC_ACCEPT_ENCODING) else {
        return (false, false);
    };
    if *value == IDENTITY_GZIP_DEFLATE {
        return (true, true);
    }
    (
        accepts_codec(headers, CompressionAlgorithm::Gzip),
        accepts_codec(headers, CompressionAlgorithm::Deflate),
    )
}

/// The coding compressed inbound frames on this RPC use.
///
/// Parsed from the peer's `grpc-encoding` token. Missing, empty, or
/// `identity` is [`CompressionAlgorithm::Gzip`]: a Compressed-Flag with no usable token
/// inflates as gzip, matching the pre-deflate behavior. Callers admit the
/// token first ([`check_request`] on the server,
/// [`super::frame_reader::refuse_encoding_reply`] on the client), so an
/// unknown coding never reaches the decoder.
#[allow(dead_code, reason = "one-lookup callers use inbound_codec_from_token")]
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

/// One pass over a small request block: the stock-protobuf check plus the
/// accept-encoding facts [`request_facts`] would return when the block has
/// no `grpc-encoding` and no `grpc-timeout`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StockFacts {
    /// Content-type is protobuf gRPC and neither encoding nor timeout is set.
    pub stock: bool,
    /// `gzip` / `deflate` (and `zstd`, when that feature is on) are final.
    /// An unusual accept-encoding value leaves this false so the caller
    /// still runs [`request_facts`].
    pub classified: bool,
    pub gzip: bool,
    pub deflate: bool,
    #[cfg(feature = "zstd")]
    pub zstd: bool,
}

thread_local! {
    static ACCEPT_FACTS: std::cell::Cell<Option<StockFacts>> = const { std::cell::Cell::new(None) };
}

/// `next_incoming` just popped one request. The accept loop takes it before
/// it parks again.
pub(crate) fn publish_accept_facts(facts: Option<StockFacts>) {
    ACCEPT_FACTS.set(facts);
}

pub(crate) fn take_accept_facts() -> Option<StockFacts> {
    ACCEPT_FACTS.replace(None)
}

impl StockFacts {
    pub(crate) const fn not_stock() -> Self {
        Self {
            stock: false,
            classified: false,
            gzip: false,
            deflate: false,
            #[cfg(feature = "zstd")]
            zstd: false,
        }
    }
}

pub(crate) fn stock_facts(headers: &HeaderMap) -> StockFacts {
    if headers.len() > 8 {
        return StockFacts::not_stock();
    }
    let mut saw_type = false;
    let mut gzip = false;
    let mut deflate = false;
    #[cfg(feature = "zstd")]
    let mut zstd = false;
    let mut unusual_accept = false;
    let mut saw_accept = false;
    for (name, value) in headers {
        if name == http::header::CONTENT_TYPE {
            let raw = value.as_bytes();
            if raw != b"application/grpc" && raw != b"application/grpc+proto" {
                return StockFacts::not_stock();
            }
            saw_type = true;
        } else if name == GRPC_ENCODING || name == GRPC_TIMEOUT {
            return StockFacts::not_stock();
        } else if name == GRPC_ACCEPT_ENCODING && !saw_accept {
            saw_accept = true;
            if *value == IDENTITY_GZIP_DEFLATE {
                gzip = true;
                deflate = true;
                #[cfg(feature = "zstd")]
                {
                    zstd = true;
                }
            } else {
                unusual_accept = true;
            }
        }
    }
    StockFacts {
        stock: saw_type,
        classified: saw_type && !unusual_accept,
        gzip,
        deflate,
        #[cfg(feature = "zstd")]
        zstd,
    }
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
    // The bytes our own clients send. Skip UTF-8 validation and the second
    // string compare; anything else still goes through the general parser.
    let stock_grpc = {
        let raw = ct.as_bytes();
        raw == b"application/grpc" || raw == b"application/grpc+proto"
    };
    if !stock_grpc {
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

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;
    use std::time::Duration;

    #[test]
    fn request_facts_matches_the_single_key_helpers() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
        headers.insert(http::header::TE, TRAILERS);
        headers.insert(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP_DEFLATE);
        headers.insert(USER_AGENT, PBRS_GRPC_UA.clone());
        assert_eq!(
            request_facts(&headers),
            (
                accept_pair(&headers).0,
                accept_pair(&headers).1,
                grpc_encoding(&headers),
                timeout_from_headers(&headers),
            )
        );

        headers.insert(GRPC_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(GRPC_TIMEOUT, HeaderValue::from_static("100m"));
        let facts = request_facts(&headers);
        assert_eq!(facts.0, accept_pair(&headers).0);
        assert_eq!(facts.1, accept_pair(&headers).1);
        assert_eq!(facts.2, grpc_encoding(&headers));
        assert_eq!(facts.3, timeout_from_headers(&headers));
        assert_eq!(facts.3, Some(Duration::from_millis(100)));

        headers.insert(GRPC_ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        let facts = request_facts(&headers);
        assert_eq!((facts.0, facts.1), accept_pair(&headers));
        assert_eq!(facts.2, grpc_encoding(&headers));
    }

    #[test]
    fn stock_request_without_deadline_is_only_the_plain_protobuf_set() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, APPLICATION_GRPC);
        headers.insert(http::header::TE, TRAILERS);
        headers.insert(GRPC_ACCEPT_ENCODING, IDENTITY_GZIP_DEFLATE);
        headers.insert(USER_AGENT, PBRS_GRPC_UA.clone());
        assert!(stock_facts(&headers).stock);
        let classified = stock_facts(&headers);
        assert!(classified.stock && classified.classified);
        assert!(classified.gzip && classified.deflate);
        assert_eq!(
            (classified.gzip, classified.deflate, None, None),
            (
                request_facts(&headers).0,
                request_facts(&headers).1,
                request_facts(&headers).2,
                request_facts(&headers).3,
            )
        );

        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc+proto"),
        );
        assert!(stock_facts(&headers).stock);

        headers.insert(GRPC_TIMEOUT, HeaderValue::from_static("100m"));
        assert!(!stock_facts(&headers).stock);
        headers.remove(GRPC_TIMEOUT);
        headers.insert(GRPC_ENCODING, HeaderValue::from_static("identity"));
        assert!(!stock_facts(&headers).stock);
        headers.remove(GRPC_ENCODING);
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        assert!(!stock_facts(&headers).stock);
    }
}
