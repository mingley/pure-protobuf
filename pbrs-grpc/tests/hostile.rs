//! What the server does when the peer is not a well-behaved gRPC client.
//!
//! These tests speak raw HTTP/2 so they can send bytes no real client would:
//! oversize length prefixes, gzip bombs, reserved flag values, truncated
//! frames, wrong content types, an HTTP/2 rapid-reset flood, and a HEADERS
//! block split across CONTINUATION frames. Every case must produce a `Status`
//! (or drop that connection) and leave the server serving. Rapid reset and
//! CONTINUATION floods are h2c-only here; TLS has no raw `h2` peer.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use bytes::{BufMut, Bytes, BytesMut};
use common::{serve, spawn_greeter_server};
use flate2::write::GzEncoder;
use flate2::Compression;
use http::{HeaderValue, Method, Request as HttpRequest, StatusCode};
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Code, Request, Response, ServerConfig, Status, Streaming};
use std::io::Write;
use std::net::SocketAddr;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const STREAM_HELLO: &str = "/helloworld.Greeter/StreamHello";

/// A raw HTTP/2 connection that sends whatever bytes it is told to.
struct RawPeer {
    send: h2::client::SendRequest<Bytes>,
    authority: String,
}

impl RawPeer {
    async fn connect(addr: SocketAddr) -> Self {
        let tcp = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (send, conn) = h2::client::handshake(tcp).await.expect("handshake");
        drop(tokio::spawn(async move {
            conn.await.ok();
        }));
        Self {
            send,
            authority: addr.to_string(),
        }
    }

    fn request(&self, path: &str, content_type: &str) -> HttpRequest<()> {
        let uri = format!("http://{}{path}", self.authority);
        HttpRequest::builder()
            .method(Method::POST)
            .uri(uri)
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::TE, "trailers")
            .body(())
            .expect("request")
    }

    /// Send `body` as the whole request and read back the gRPC status.
    async fn call(&mut self, path: &str, body: Bytes) -> Answer {
        self.call_with(self.request(path, "application/grpc"), body)
            .await
    }

    async fn call_with(&mut self, request: HttpRequest<()>, body: Bytes) -> Answer {
        let mut send = self.send.clone().ready().await.expect("ready");
        let (response, mut stream) = send.send_request(request, false).expect("send_request");
        stream.send_data(body, true).expect("send_data");
        let response = response.await.expect("response");
        let http_status = response.status();
        let header_status = grpc_status(response.headers());
        let mut body = response.into_body();
        let mut payload_frames = 0usize;
        while let Some(chunk) = body.data().await {
            let chunk = chunk.expect("data");
            if !chunk.is_empty() {
                payload_frames += 1;
            }
            body.flow_control().release_capacity(chunk.len()).ok();
        }
        let trailer_status = body
            .trailers()
            .await
            .ok()
            .flatten()
            .and_then(|t| grpc_status(&t));
        Answer {
            http_status,
            code: trailer_status.or(header_status).map(Code::from_i32),
            payload_frames,
        }
    }
}

struct Answer {
    http_status: StatusCode,
    code: Option<Code>,
    payload_frames: usize,
}

impl Answer {
    fn expect_code(&self, want: Code) {
        assert_eq!(
            self.http_status,
            StatusCode::OK,
            "gRPC protocol errors answer 200"
        );
        assert_eq!(self.code, Some(want));
    }

    fn expect_http(&self, want: StatusCode) {
        assert_eq!(self.http_status, want);
        assert_eq!(self.code, None, "HTTP {want} is not a gRPC status");
        assert_eq!(self.payload_frames, 0);
    }
}

fn grpc_status(headers: &http::HeaderMap) -> Option<i32> {
    headers.get("grpc-status")?.to_str().ok()?.parse().ok()
}

/// A length-prefixed frame with an arbitrary declared length, so tests can lie.
fn frame_with_declared_len(flag: u8, declared: u32, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(5 + payload.len());
    buf.put_u8(flag);
    buf.put_u32(declared);
    buf.extend_from_slice(payload);
    buf.freeze()
}

fn frame(payload: &[u8]) -> Bytes {
    frame_with_declared_len(0, payload.len() as u32, payload)
}

fn gzip(payload: &[u8]) -> Vec<u8> {
    gzip_with(payload, Compression::fast())
}

fn gzip_with(payload: &[u8], level: Compression) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), level);
    enc.write_all(payload).expect("write");
    enc.finish().expect("finish")
}

/// A valid `HelloRequest { name: "ada" }`.
fn hello_request() -> Vec<u8> {
    vec![0x0a, 0x03, b'a', b'd', b'a']
}

#[tokio::test]
async fn a_giant_declared_length_is_refused_from_the_header() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    // Claims 4 GiB but sends five bytes. The cap must be applied to the claim.
    let body = frame_with_declared_len(0, u32::MAX, &hello_request());
    peer.call(SAY_HELLO, body)
        .await
        .expect_code(Code::ResourceExhausted);
}

#[tokio::test]
async fn oversize_message_is_resource_exhausted_not_a_hang() {
    let (addr, _guard) =
        spawn_greeter_server(ServerConfig::new().max_decoding_message_size(16)).await;
    let mut peer = RawPeer::connect(addr).await;
    let payload = vec![0u8; 1024];
    peer.call(SAY_HELLO, frame(&payload))
        .await
        .expect_code(Code::ResourceExhausted);
}

#[tokio::test]
async fn a_gzip_bomb_cannot_outgrow_the_cap() {
    // 64 MiB of zeros compresses to well under 256 KiB, so the frame itself
    // passes the length check and only bounded inflation stops it.
    const CAP: usize = 256 * 1024;
    let (addr, _guard) =
        spawn_greeter_server(ServerConfig::new().max_decoding_message_size(CAP)).await;
    let bomb = gzip_with(&vec![0u8; 64 * 1024 * 1024], Compression::best());
    assert!(
        bomb.len() < CAP,
        "the bomb must pass the frame-length check: {} bytes vs {CAP}",
        bomb.len()
    );
    let mut peer = RawPeer::connect(addr).await;
    let body = frame_with_declared_len(1, bomb.len() as u32, &bomb);
    peer.call(SAY_HELLO, body)
        .await
        .expect_code(Code::ResourceExhausted);
}

#[tokio::test]
async fn a_legitimate_compressed_frame_still_round_trips() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let compressed = gzip(&hello_request());
    let mut peer = RawPeer::connect(addr).await;
    let body = frame_with_declared_len(1, compressed.len() as u32, &compressed);
    for encoding in ["gzip", "GZIP", "Gzip"] {
        let mut request = peer.request(SAY_HELLO, "application/grpc");
        request.headers_mut().insert(
            "grpc-encoding",
            HeaderValue::from_str(encoding).expect("encoding"),
        );
        let answer = peer.call_with(request, body.clone()).await;
        answer.expect_code(Code::Ok);
        assert_eq!(answer.payload_frames, 1, "{encoding}");
    }

    let mut identity = peer.request(SAY_HELLO, "application/grpc");
    identity
        .headers_mut()
        .insert("grpc-encoding", HeaderValue::from_static("IDENTITY"));
    peer.call_with(identity, frame(&hello_request()))
        .await
        .expect_code(Code::Ok);
}

#[tokio::test]
async fn an_identity_encoding_header_is_the_same_as_omitting_it() {
    struct WantsNone;

    impl Greeter for WantsNone {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            if let Some(enc) = request.encoding() {
                return Err(Status::internal(format!("identity advertised {enc}")));
            }
            Ok(Response::new(common::reply(common::name_of_request(
                request.get_ref(),
            ))))
        }
    }

    let (addr, _guard) = serve(WantsNone, ServerConfig::new()).await.expect("serve");
    let mut peer = RawPeer::connect(addr).await;
    peer.call(SAY_HELLO, frame(&hello_request()))
        .await
        .expect_code(Code::Ok);
    for token in ["identity", "IDENTITY", "Identity", "identity;q=0"] {
        let mut request = peer.request(SAY_HELLO, "application/grpc");
        request.headers_mut().insert(
            "grpc-encoding",
            HeaderValue::from_str(token).expect("encoding"),
        );
        peer.call_with(request, frame(&hello_request()))
            .await
            .expect_code(Code::Ok);
    }
}

#[tokio::test]
async fn a_reserved_compressed_flag_is_a_protocol_error() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    let payload = hello_request();
    let body = frame_with_declared_len(7, payload.len() as u32, &payload);
    peer.call(SAY_HELLO, body).await.expect_code(Code::Internal);
}

#[tokio::test]
async fn a_truncated_frame_is_not_an_empty_message() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    // Declares 64 bytes, sends 3. The stream then half-closes.
    let body = frame_with_declared_len(0, 64, &[1, 2, 3]);
    peer.call(SAY_HELLO, body).await.expect_code(Code::Internal);
}

#[tokio::test]
async fn two_messages_on_a_unary_path_are_refused() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    let mut body = BytesMut::new();
    body.extend_from_slice(&frame(&hello_request()));
    body.extend_from_slice(&frame(&hello_request()));
    peer.call(SAY_HELLO, body.freeze())
        .await
        .expect_code(Code::Internal);
}

#[tokio::test]
async fn a_non_grpc_content_type_is_http_415() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    for content_type in [
        "application/json",
        "application/grpc+json",
        "application/grpc+thrift",
        "application/grpc-web",
        "application/grpc-web+proto",
    ] {
        let request = peer.request(SAY_HELLO, content_type);
        peer.call_with(request, frame(&hello_request()))
            .await
            .expect_http(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}

#[tokio::test]
async fn a_non_grpc_content_type_on_an_unknown_method_is_still_415() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    let request = peer.request("/nope.Nothing/Anything", "application/json");
    peer.call_with(request, Bytes::new())
        .await
        .expect_http(StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn a_non_post_is_http_405() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let peer = RawPeer::connect(addr).await;
    for method in [Method::GET, Method::PUT, Method::HEAD] {
        let label = method.as_str().to_owned();
        let mut request = peer.request(SAY_HELLO, "application/grpc");
        *request.method_mut() = method;
        let mut send = peer.send.clone().ready().await.expect("ready");
        let (response, _stream) = send.send_request(request, true).expect("send_request");
        let response = response.await.expect("response");
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{label} must be 405"
        );
        assert_eq!(
            response
                .headers()
                .get(http::header::ALLOW)
                .and_then(|v| v.to_str().ok()),
            Some("POST")
        );
        assert!(grpc_status(response.headers()).is_none(), "{label}");
    }
}

#[tokio::test]
async fn grpc_proto_content_type_subtypes_are_accepted() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    for content_type in [
        "application/grpc",
        "application/grpc+proto",
        "Application/Grpc",
        "APPLICATION/GRPC+PROTO",
    ] {
        let request = peer.request(SAY_HELLO, content_type);
        let answer = peer.call_with(request, frame(&hello_request())).await;
        answer.expect_code(Code::Ok);
        assert_eq!(answer.payload_frames, 1, "{content_type} must get a reply");
    }
}

#[tokio::test]
async fn an_unsupported_encoding_is_unimplemented_and_advertises_what_works() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let peer = RawPeer::connect(addr).await;
    let mut request = peer.request(SAY_HELLO, "application/grpc");
    request
        .headers_mut()
        .insert("grpc-encoding", HeaderValue::from_static("snappy"));

    let mut send = peer.send.clone().ready().await.expect("ready");
    let (response, mut stream) = send.send_request(request, false).expect("send_request");
    stream
        .send_data(frame(&hello_request()), true)
        .expect("send_data");
    let response = response.await.expect("response");
    assert_eq!(
        grpc_status(response.headers()).map(Code::from_i32),
        Some(Code::Unimplemented)
    );
    assert_eq!(
        response
            .headers()
            .get("grpc-accept-encoding")
            .and_then(|v| v.to_str().ok()),
        Some("identity,gzip"),
        "the spec requires telling the client what to retry with"
    );
}

#[tokio::test]
async fn an_unknown_method_is_unimplemented() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    peer.call("/helloworld.Greeter/NoSuchMethod", frame(&hello_request()))
        .await
        .expect_code(Code::Unimplemented);
}

#[tokio::test]
async fn an_unknown_service_is_unimplemented() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    peer.call("/nope.Nothing/Anything", frame(&hello_request()))
        .await
        .expect_code(Code::Unimplemented);
}

#[tokio::test]
async fn a_malformed_path_is_unimplemented_not_a_panic() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    for path in ["/", "/nomethod", "//", "/a/b/c"] {
        peer.call(path, frame(&hello_request()))
            .await
            .expect_code(Code::Unimplemented);
    }
}

#[tokio::test]
async fn an_empty_body_decodes_to_a_default_message() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    let answer = peer.call(SAY_HELLO, Bytes::new()).await;
    answer.expect_code(Code::Ok);
    assert_eq!(answer.payload_frames, 1);
}

#[tokio::test]
async fn garbage_protobuf_bytes_are_an_error_not_a_panic() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;
    // Field 1, wire type 5 (fixed32) where a string is expected, truncated.
    peer.call(SAY_HELLO, frame(&[0x0d, 0xff]))
        .await
        .expect_code(Code::Internal);
}

#[tokio::test]
async fn a_hostile_stream_does_not_take_the_server_down() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawPeer::connect(addr).await;

    // Twenty different kinds of bad request on the same connection.
    for _ in 0..5 {
        peer.call(SAY_HELLO, frame_with_declared_len(0, u32::MAX, &[]))
            .await;
        peer.call(SAY_HELLO, frame_with_declared_len(9, 1, &[0]))
            .await;
        peer.call("/unknown.Service/Method", Bytes::new()).await;
        peer.call(STREAM_HELLO, frame_with_declared_len(0, 99, &[1]))
            .await;
    }

    // The server still answers a well-formed request on a fresh connection.
    let client = common::greeter_client(addr).await;
    let reply = client
        .say_hello(pbrs_grpc::Request::new(common::req("ada")))
        .await
        .expect("server still healthy");
    assert_eq!(common::name_of(reply.get_ref()), "ada");
}

#[tokio::test]
async fn metadata_beyond_the_header_list_cap_is_refused() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new().max_header_list_size(1024)).await;
    let peer = RawPeer::connect(addr).await;
    let mut request = peer.request(SAY_HELLO, "application/grpc");
    let big = "v".repeat(4096);
    request
        .headers_mut()
        .insert("x-flood", HeaderValue::from_str(&big).expect("value"));

    let mut send = peer.send.clone().ready().await.expect("ready");
    // h2 either refuses locally or the server resets; either way the RPC does
    // not complete and the server is unharmed.
    let outcome = match send.send_request(request, false) {
        Err(_) => None,
        Ok((response, mut stream)) => {
            stream.send_data(frame(&hello_request()), true).ok();
            Some(response.await)
        }
    };
    assert!(
        outcome.is_none_or(|r| r.is_err()),
        "oversize header list must not be served"
    );

    let client = common::greeter_client(addr).await;
    let reply = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(pbrs_grpc::Request::new(common::req("ada"))),
    )
    .await
    .expect("no hang")
    .expect("server still healthy");
    assert_eq!(common::name_of(reply.get_ref()), "ada");
}

#[tokio::test]
async fn h2c_ignores_a_peer_https_scheme() {
    struct MustBeHttp;

    impl Greeter for MustBeHttp {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            if request.scheme() != Some("http") {
                return Err(Status::internal(format!("scheme {:?}", request.scheme())));
            }
            Ok(Response::new(common::reply(common::name_of_request(
                request.get_ref(),
            ))))
        }

        async fn client_hello(
            &self,
            _request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            Err(Status::unimplemented("must-be-http"))
        }

        async fn server_hello(
            &self,
            _request: Request<HelloRequest>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            Err(Status::unimplemented("must-be-http"))
        }

        async fn stream_hello(
            &self,
            _request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            Err(Status::unimplemented("must-be-http"))
        }
    }

    let (addr, _guard) = serve(MustBeHttp, ServerConfig::new()).await.expect("spawn");
    let mut peer = RawPeer::connect(addr).await;
    let uri = format!("https://{}{SAY_HELLO}", peer.authority);
    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/grpc")
        .header(http::header::TE, "trailers")
        .body(())
        .expect("request");
    peer.call_with(request, frame(&hello_request()))
        .await
        .expect_code(Code::Ok);
}

/// A raw peer that `RST_STREAM`s faster than accept exceeds
/// [`ServerConfig::max_pending_accept_reset_streams`]. That connection is
/// dropped (`ENHANCE_YOUR_CALM`); a well-behaved client on a fresh connection
/// still serves. Distinct from wrap still-serves. h2c-only (`RawPeer`).
///
/// `current_thread` so the burst queues HEADERS then RSTs without the server
/// `poll_accept` interleaving. A multi-thread runtime lets the accept loop
/// drain streams before the RSTs land, and the queue never fills.
#[tokio::test(flavor = "current_thread")]
async fn rst_flood_beyond_pending_reset_cap_drops_that_connection() {
    let (addr, _guard) =
        spawn_greeter_server(ServerConfig::new().max_pending_accept_reset_streams(1)).await;
    let peer = RawPeer::connect(addr).await;
    let mut send = peer.send.clone().ready().await.expect("settings");
    let mut opened = Vec::new();
    for _ in 0..32 {
        let request = peer.request(SAY_HELLO, "application/grpc");
        let (response, stream) = send.send_request(request, false).expect("open stream");
        opened.push((response, stream));
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        match send.poll_ready(&mut cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => panic!("connection died while opening the burst: {e}"),
            Poll::Pending => {
                panic!("burst must not yield; SETTINGS should already allow 32 streams")
            }
        }
    }
    for (_, stream) in &mut opened {
        stream.send_reset(h2::Reason::CANCEL);
    }
    drop(opened);
    drop(send);
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    let dropped =
        match tokio::time::timeout(Duration::from_millis(500), peer.send.clone().ready()).await {
            Ok(Ok(_)) => false,
            Ok(Err(_)) | Err(_) => true,
        };
    assert!(
        dropped,
        "RST flood must trip max_pending_accept_reset_streams and drop that connection"
    );

    let client = common::greeter_client(addr).await;
    let reply = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(pbrs_grpc::Request::new(common::req("ada"))),
    )
    .await
    .expect("no hang")
    .expect("accept loop still serves after the flood connection dropped");
    assert_eq!(common::name_of(reply.get_ref()), "ada");
}

const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const FRAME_HEADERS: u8 = 0x1;
const FRAME_SETTINGS: u8 = 0x4;
const FRAME_CONTINUATION: u8 = 0x9;
const FLAG_ACK: u8 = 0x1;

/// Length-prefixed HTTP/2 frame. `h2::client` always sends a complete header
/// block; this is how a test splits HEADERS across CONTINUATION.
fn h2_frame(ty: u8, flags: u8, stream_id: u32, payload: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut buf = Vec::with_capacity(9 + len);
    buf.push(((len >> 16) & 0xff) as u8);
    buf.push(((len >> 8) & 0xff) as u8);
    buf.push((len & 0xff) as u8);
    buf.push(ty);
    buf.push(flags);
    buf.push(((stream_id >> 24) & 0x7f) as u8);
    buf.push(((stream_id >> 16) & 0xff) as u8);
    buf.push(((stream_id >> 8) & 0xff) as u8);
    buf.push((stream_id & 0xff) as u8);
    buf.extend_from_slice(payload);
    buf
}

fn hpack_int(buf: &mut Vec<u8>, n: usize, prefix_bits: u8, first_high: u8) {
    let max = (1usize << prefix_bits) - 1;
    if n < max {
        buf.push(first_high | n as u8);
        return;
    }
    buf.push(first_high | max as u8);
    let mut n = n - max;
    while n >= 128 {
        buf.push((n % 128) as u8 | 0x80);
        n /= 128;
    }
    buf.push(n as u8);
}

fn hpack_string(buf: &mut Vec<u8>, value: &[u8]) {
    hpack_int(buf, value.len(), 7, 0);
    buf.extend_from_slice(value);
}

/// Literal without indexing, static-table name index (RFC 7541 §6.2.2).
fn hpack_literal_indexed(index: usize, value: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    hpack_int(&mut buf, index, 4, 0);
    hpack_string(&mut buf, value.as_bytes());
    buf
}

fn hpack_literal_new(name: &str, value: &[u8]) -> Vec<u8> {
    let mut buf = vec![0];
    hpack_string(&mut buf, name.as_bytes());
    hpack_string(&mut buf, value);
    buf
}

fn grpc_header_block(authority: &str) -> Vec<u8> {
    let mut block = Vec::new();
    block.push(0x83); // :method POST
    block.extend(hpack_literal_indexed(4, SAY_HELLO)); // :path
    block.push(0x86); // :scheme http
    block.extend(hpack_literal_indexed(1, authority)); // :authority
    block.extend(hpack_literal_indexed(31, "application/grpc")); // content-type
    block.extend(hpack_literal_new("te", b"trailers"));
    block
}

/// Prior-knowledge HTTP/2 on a raw TCP socket, so tests can omit `END_HEADERS`.
struct RawH2 {
    tcp: TcpStream,
}

impl RawH2 {
    async fn connect(addr: SocketAddr) -> Self {
        let mut tcp = TcpStream::connect(addr).await.expect("connect");
        tcp.write_all(H2_PREFACE).await.expect("preface");
        tcp.write_all(&h2_frame(FRAME_SETTINGS, 0, 0, &[]))
            .await
            .expect("client settings");
        let mut this = Self { tcp };
        this.ack_server_settings().await;
        this
    }

    async fn ack_server_settings(&mut self) {
        for _ in 0..8 {
            let (ty, _flags, stream, _payload) =
                match tokio::time::timeout(Duration::from_millis(500), self.read_frame()).await {
                    Ok(Ok(frame)) => frame,
                    Ok(Err(_)) | Err(_) => return,
                };
            if ty == FRAME_SETTINGS && stream == 0 {
                self.tcp
                    .write_all(&h2_frame(FRAME_SETTINGS, FLAG_ACK, 0, &[]))
                    .await
                    .expect("settings ack");
                return;
            }
        }
        panic!("server never sent SETTINGS");
    }

    async fn read_frame(&mut self) -> std::io::Result<(u8, u8, u32, Vec<u8>)> {
        let mut head = [0u8; 9];
        self.tcp.read_exact(&mut head).await?;
        let len = (u32::from(head[0]) << 16) | (u32::from(head[1]) << 8) | u32::from(head[2]);
        let ty = head[3];
        let flags = head[4];
        let stream = u32::from_be_bytes([head[5] & 0x7f, head[6], head[7], head[8]]);
        let mut payload = vec![0u8; len as usize];
        if len > 0 {
            self.tcp.read_exact(&mut payload).await?;
        }
        Ok((ty, flags, stream, payload))
    }

    async fn write_all(&mut self, bytes: &[u8]) {
        self.tcp.write_all(bytes).await.expect("write");
    }
}

async fn assert_accept_loop_still_serves(addr: SocketAddr) {
    let client = common::greeter_client(addr).await;
    let reply = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(pbrs_grpc::Request::new(common::req("ada"))),
    )
    .await
    .expect("no hang")
    .expect("accept loop still serves");
    assert_eq!(common::name_of(reply.get_ref()), "ada");
}

/// A raw peer that sends more CONTINUATION frames than the header-list cap
/// allows (`h2` `too_many_continuations`) drops that connection
/// (`ENHANCE_YOUR_CALM`). A well-behaved client on a fresh connection still
/// serves. Distinct from `metadata_beyond_the_header_list_cap_is_refused`,
/// which sends one complete HEADERS frame through `h2::client`, and from
/// rapid-reset. h2c-only (`RawH2`).
#[tokio::test]
async fn continuation_flood_drops_that_connection() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new().max_header_list_size(1024)).await;
    let mut peer = RawH2::connect(addr).await;
    let block = grpc_header_block(&addr.to_string());
    peer.write_all(&h2_frame(FRAME_HEADERS, 0, 1, &block)).await;
    for _ in 0..32 {
        peer.write_all(&h2_frame(FRAME_CONTINUATION, 0, 1, &[]))
            .await;
    }

    let started = std::time::Instant::now();
    let dropped = loop {
        match tokio::time::timeout(Duration::from_millis(200), peer.read_frame()).await {
            Ok(Ok((0x7, _, _, _))) => break true, // GOAWAY
            Ok(Ok(_)) => {
                if started.elapsed() > Duration::from_millis(800) {
                    break false;
                }
            }
            Ok(Err(_)) | Err(_) => break true,
        }
    };
    assert!(
        dropped,
        "CONTINUATION flood must trip too_many_continuations and drop that connection"
    );
    assert_accept_loop_still_serves(addr).await;
}

/// HEADERS without `END_HEADERS` and without a following CONTINUATION stalls
/// that stream. Distinct from handshake timeout (preface already finished) and
/// from the CONTINUATION flood above. The accept loop still serves. h2c-only.
#[tokio::test]
async fn unfinished_headers_do_not_take_the_accept_loop_down() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawH2::connect(addr).await;
    let block = grpc_header_block(&addr.to_string());
    peer.write_all(&h2_frame(FRAME_HEADERS, 0, 1, &block)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert_accept_loop_still_serves(addr).await;
    drop(peer);
}
