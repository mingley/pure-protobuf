//! gRPC-Web over the native HTTP/2 server transport.

#![cfg(feature = "grpc-web")]
#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    unreachable_pub,
    reason = "integration tests speak raw HTTP/2"
)]

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::{BufMut, Bytes, BytesMut};
use common::{ServerGuard, spawn_greeter_server};
use http::{HeaderMap, HeaderValue, Method, Request as HttpRequest, StatusCode};
use pbrs::Parse;
use pbrs_grpc::{
    Code, CompressionAlgorithm, HelloReply, HelloRequest, Request, Response, Rpc, Server,
    ServerConfig, Service, Status,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const SERVER_HELLO: &str = "/helloworld.Greeter/ServerHello";

struct RawWebPeer {
    send: h2::client::SendRequest<Bytes>,
    authority: String,
}

impl RawWebPeer {
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
            .body(())
            .expect("request")
    }

    async fn call(&mut self, path: &str, content_type: &str, body: Bytes) -> WebAnswer {
        self.call_chunks(path, content_type, [body]).await
    }

    async fn call_chunks<I>(&mut self, path: &str, content_type: &str, chunks: I) -> WebAnswer
    where
        I: IntoIterator<Item = Bytes>,
    {
        let request = self.request(path, content_type);
        self.call_request(request, chunks).await
    }

    async fn call_request<I>(&mut self, request: HttpRequest<()>, chunks: I) -> WebAnswer
    where
        I: IntoIterator<Item = Bytes>,
    {
        let mut send = self.send.clone().ready().await.expect("ready");
        let (response, mut stream) = send.send_request(request, false).expect("send_request");
        let mut chunks = chunks.into_iter().peekable();
        while let Some(chunk) = chunks.next() {
            let end = chunks.peek().is_none();
            stream.send_data(chunk, end).expect("send_data");
        }
        let response = response.await.expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let header_grpc_status = response
            .headers()
            .get("grpc-status")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut body = response.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.expect("data");
            data.extend_from_slice(&chunk);
            body.flow_control().release_capacity(chunk.len()).ok();
        }
        let h2_trailers = body.trailers().await.expect("trailers");
        WebAnswer {
            status,
            headers,
            content_type,
            header_grpc_status,
            body: data.freeze(),
            h2_trailers,
        }
    }

    async fn preflight(&mut self, path: &str, origin: &str) -> WebAnswer {
        let uri = format!("http://{}{path}", self.authority);
        let request = HttpRequest::builder()
            .method(Method::OPTIONS)
            .uri(uri)
            .header(http::header::ORIGIN, origin)
            .header(http::header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                http::header::ACCESS_CONTROL_REQUEST_HEADERS,
                "content-type,x-grpc-web",
            )
            .body(())
            .expect("preflight");
        let mut send = self.send.clone().ready().await.expect("ready");
        let (response, _body) = send.send_request(request, true).expect("send_request");
        let response = response.await.expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let mut body = response.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.expect("data");
            data.extend_from_slice(&chunk);
            body.flow_control().release_capacity(chunk.len()).ok();
        }
        let h2_trailers = body.trailers().await.expect("trailers");
        WebAnswer {
            status,
            headers,
            content_type: None,
            header_grpc_status: None,
            body: data.freeze(),
            h2_trailers,
        }
    }
}

struct WebAnswer {
    status: StatusCode,
    headers: HeaderMap,
    content_type: Option<String>,
    header_grpc_status: Option<String>,
    body: Bytes,
    h2_trailers: Option<http::HeaderMap>,
}

impl WebAnswer {
    fn expect_ok_web(self, content_type: &str) -> ParsedWebBody {
        assert_eq!(self.status, StatusCode::OK);
        assert_eq!(self.content_type.as_deref(), Some(content_type));
        assert_eq!(
            self.header_grpc_status, None,
            "gRPC-Web status belongs in body trailers"
        );
        assert!(
            self.h2_trailers.is_none(),
            "gRPC-Web must not use HTTP/2 trailers"
        );
        let body = if content_type.starts_with("application/grpc-web-text") {
            Bytes::from(STANDARD.decode(&self.body).expect("base64 response"))
        } else {
            self.body
        };
        parse_web_body(&body)
    }

    fn expect_http(self, status: StatusCode) {
        assert_eq!(self.status, status);
        assert!(self.body.is_empty(), "HTTP rejection body");
        assert!(self.h2_trailers.is_none(), "HTTP rejection trailers");
    }

    fn expect_preflight_allowed(self, origin: &str) {
        assert_eq!(self.status, StatusCode::OK);
        assert_eq!(
            self.headers
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(origin)
        );
        assert_eq!(
            self.headers
                .get("access-control-allow-methods")
                .and_then(|value| value.to_str().ok()),
            Some("POST")
        );
        assert_eq!(
            self.headers
                .get("access-control-allow-headers")
                .and_then(|value| value.to_str().ok()),
            Some("content-type,x-grpc-web")
        );
        assert!(self.body.is_empty());
    }
}

struct ParsedWebBody {
    messages: Vec<Bytes>,
    trailers: HashMap<String, String>,
}

impl ParsedWebBody {
    fn expect_status(&self, code: Code) {
        let want = code.to_i32().to_string();
        assert_eq!(
            self.trailers.get("grpc-status").map(String::as_str),
            Some(want.as_str())
        );
    }
}

fn parse_web_body(mut body: &[u8]) -> ParsedWebBody {
    let mut messages = Vec::new();
    let mut trailers = None;
    while !body.is_empty() {
        assert!(body.len() >= 5, "short frame header: {body:?}");
        let flag = body[0];
        let len = u32::from_be_bytes(body[1..5].try_into().expect("len")) as usize;
        body = &body[5..];
        assert!(body.len() >= len, "short frame payload");
        let payload = Bytes::copy_from_slice(&body[..len]);
        body = &body[len..];
        if flag & 0x80 != 0 {
            assert!(trailers.is_none(), "duplicate trailers");
            trailers = Some(parse_trailers(&payload));
        } else {
            assert_eq!(flag, 0, "unexpected message flags");
            messages.push(payload);
        }
    }
    ParsedWebBody {
        messages,
        trailers: trailers.expect("trailers frame"),
    }
}

fn parse_trailers(bytes: &[u8]) -> HashMap<String, String> {
    let raw = std::str::from_utf8(bytes).expect("trailers utf8");
    raw.split("\r\n")
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(": ").expect("trailer header");
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

fn frame(payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(5 + payload.len());
    out.put_u8(0);
    out.put_u32(payload.len() as u32);
    out.extend_from_slice(payload);
    out.freeze()
}

fn text_frame(payload: &[u8]) -> Bytes {
    STANDARD.encode(frame(payload)).into()
}

fn hello_request(name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + name.len());
    out.push(0x0a);
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    out
}

fn reply_message(payload: &Bytes) -> HelloReply {
    HelloReply::parse(payload).expect("HelloReply")
}

const WEB_CONTENT_TYPES: [&str; 4] = [
    "application/grpc-web",
    "application/grpc-web+proto",
    "application/grpc-web-text",
    "application/grpc-web-text+proto",
];

struct CountingService(Arc<AtomicUsize>);

impl Service for CountingService {
    const NAME: &'static str = "helloworld.Greeter";

    async fn call(&self, rpc: Rpc) {
        self.0.fetch_add(1, Ordering::SeqCst);
        rpc.unary(|request: Request<HelloRequest>| async move {
            let mut reply = HelloReply::new();
            reply.set_message(request.get_ref().name());
            Ok::<_, Status>(Response::new(reply))
        })
        .await;
    }
}

async fn spawn_counting_server(
    config: ServerConfig,
) -> (SocketAddr, ServerGuard, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local address");
    let dispatches = Arc::new(AtomicUsize::new(0));
    let server_dispatches = Arc::clone(&dispatches);
    let task = tokio::spawn(async move {
        Server::new(CountingService(server_dispatches))
            .config(config)
            .serve_listener(listener)
            .await
            .expect("server");
    });
    (addr, ServerGuard(task), dispatches)
}

fn request_with_encoding(
    peer: &RawWebPeer,
    content_type: &str,
    encoding: Option<&'static str>,
) -> HttpRequest<()> {
    let mut request = peer.request(SAY_HELLO, content_type);
    request.headers_mut().insert(
        http::header::ORIGIN,
        HeaderValue::from_static("https://app.example"),
    );
    if let Some(encoding) = encoding {
        request
            .headers_mut()
            .insert("grpc-encoding", HeaderValue::from_static(encoding));
    }
    request
}

fn request_body(content_type: &str, payload: &[u8], compressed: bool) -> Bytes {
    let framed = pbrs_grpc::codec::encode(payload, compressed).expect("request frame");
    if content_type.starts_with("application/grpc-web-text") {
        STANDARD.encode(framed).into()
    } else {
        framed
    }
}

fn assert_accepted_codings(answer: &WebAnswer, compressed: bool) {
    let expected = if compressed {
        if cfg!(feature = "zstd") {
            "identity,gzip,deflate,zstd"
        } else {
            "identity,gzip,deflate"
        }
    } else {
        "identity"
    };
    assert_eq!(
        answer
            .headers
            .get("grpc-accept-encoding")
            .and_then(|value| value.to_str().ok()),
        Some(expected)
    );
}

#[tokio::test]
async fn grpc_web_rejects_unsupported_coding_before_dispatch() {
    let config = ServerConfig::new().grpc_web_allow_origin("https://app.example");
    let (addr, _guard, dispatches) = spawn_counting_server(config).await;
    let mut peer = RawWebPeer::connect(addr).await;
    peer.preflight(SAY_HELLO, "https://app.example")
        .await
        .expect_preflight_allowed("https://app.example");

    let message = hello_request("ada");
    let gzip = CompressionAlgorithm::Gzip
        .encode(&message)
        .expect("gzip request");
    for content_type in WEB_CONTENT_TYPES {
        let request = request_with_encoding(&peer, content_type, Some("identity"));
        let success = peer
            .call_request(request, [request_body(content_type, &message, false)])
            .await;
        let response_origin = success
            .headers
            .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .cloned();
        let success = success.expect_ok_web(content_type);
        success.expect_status(Code::Ok);
        assert_eq!(success.messages.len(), 1);
        assert_eq!(reply_message(&success.messages[0]).message(), "ada");

        let before_rejections = dispatches.load(Ordering::SeqCst);
        for compressed in [false, true] {
            let request = request_with_encoding(&peer, content_type, Some("snappy"));
            let answer = peer
                .call_request(request, [request_body(content_type, &message, compressed)])
                .await;
            assert_accepted_codings(&answer, true);
            assert_eq!(
                answer
                    .headers
                    .get(http::header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    .cloned(),
                response_origin
            );
            let body = answer.expect_ok_web(content_type);
            body.expect_status(Code::Unimplemented);
            assert!(body.messages.is_empty());
            assert_eq!(dispatches.load(Ordering::SeqCst), before_rejections);
        }

        for compressed in [false, true] {
            let before_success = dispatches.load(Ordering::SeqCst);
            let payload = if compressed { &gzip } else { &message };
            let request = request_with_encoding(&peer, content_type, Some("gzip"));
            let success = peer
                .call_request(request, [request_body(content_type, payload, compressed)])
                .await
                .expect_ok_web(content_type);
            success.expect_status(Code::Ok);
            assert_eq!(success.messages.len(), 1);
            assert_eq!(reply_message(&success.messages[0]).message(), "ada");
            assert_eq!(dispatches.load(Ordering::SeqCst), before_success + 1);
        }
    }

    let before_native = dispatches.load(Ordering::SeqCst);
    let request = request_with_encoding(&peer, "application/grpc", Some("snappy"));
    let rejection = peer
        .call_request(request, [request_body("application/grpc", &message, false)])
        .await;
    assert_eq!(rejection.status, StatusCode::OK);
    assert_eq!(rejection.header_grpc_status.as_deref(), Some("12"));
    assert_accepted_codings(&rejection, true);
    assert_eq!(dispatches.load(Ordering::SeqCst), before_native);

    let request = request_with_encoding(&peer, "application/grpc", None);
    let success = peer
        .call_request(request, [request_body("application/grpc", &message, false)])
        .await;
    assert_eq!(success.status, StatusCode::OK);
    assert_eq!(
        success
            .h2_trailers
            .as_ref()
            .and_then(|trailers| trailers.get("grpc-status"))
            .and_then(|value| value.to_str().ok()),
        Some("0")
    );
    let mut wire = BytesMut::from(success.body.as_ref());
    let reply = pbrs_grpc::codec::pop(&mut wire)
        .expect("native frame")
        .expect("native reply");
    assert_eq!(reply_message(&reply.payload).message(), "ada");
    assert!(wire.is_empty());
    assert_eq!(dispatches.load(Ordering::SeqCst), before_native + 1);
}

#[tokio::test]
async fn grpc_web_rejects_gzip_when_compression_is_disabled() {
    let config = ServerConfig::new().accept_compressed(false);
    let (addr, _guard, dispatches) = spawn_counting_server(config).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let message = hello_request("ada");
    let gzip = CompressionAlgorithm::Gzip
        .encode(&message)
        .expect("gzip request");
    for content_type in WEB_CONTENT_TYPES {
        for compressed in [false, true] {
            let payload = if compressed { &gzip } else { &message };
            let request = request_with_encoding(&peer, content_type, Some("gzip"));
            let answer = peer
                .call_request(request, [request_body(content_type, payload, compressed)])
                .await;
            assert_accepted_codings(&answer, false);
            let body = answer.expect_ok_web(content_type);
            body.expect_status(Code::Unimplemented);
            assert!(body.messages.is_empty());
            assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        }
    }

    let request = request_with_encoding(&peer, "application/grpc-web", Some("identity"));
    let success = peer
        .call_request(
            request,
            [request_body("application/grpc-web", &message, false)],
        )
        .await
        .expect_ok_web("application/grpc-web");
    success.expect_status(Code::Ok);
    assert_eq!(success.messages.len(), 1);
    assert_eq!(reply_message(&success.messages[0]).message(), "ada");
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn binary_grpc_web_unary_uses_body_trailers() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let body = peer
        .call(
            SAY_HELLO,
            "application/grpc-web+proto",
            frame(&hello_request("ada")),
        )
        .await
        .expect_ok_web("application/grpc-web+proto");
    body.expect_status(Code::Ok);
    assert_eq!(body.messages.len(), 1);
    assert_eq!(reply_message(&body.messages[0]).message(), "ada");
}

#[tokio::test]
async fn binary_grpc_web_server_streaming_uses_body_trailers() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let body = peer
        .call(
            SERVER_HELLO,
            "application/grpc-web",
            frame(&hello_request("ada,bob")),
        )
        .await
        .expect_ok_web("application/grpc-web");
    body.expect_status(Code::Ok);
    let replies: Vec<_> = body.messages.iter().map(reply_message).collect();
    assert_eq!(
        replies
            .iter()
            .map(|reply| reply.message().to_string())
            .collect::<Vec<_>>(),
        ["ada".to_owned(), "bob".to_owned()]
    );
}

#[tokio::test]
async fn binary_grpc_web_immediate_unimplemented_uses_body_trailers() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let body = peer
        .call(
            "/helloworld.Greeter/Nope",
            "application/grpc-web",
            frame(&hello_request("ada")),
        )
        .await
        .expect_ok_web("application/grpc-web");
    body.expect_status(Code::Unimplemented);
    assert!(body.messages.is_empty());
}

#[tokio::test]
async fn grpc_web_text_unary_decodes_split_base64_and_uses_body_trailers() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let encoded = text_frame(&hello_request("ada"));
    let chunks = [encoded.slice(..1), encoded.slice(1..3), encoded.slice(3..)];
    let body = peer
        .call_chunks(SAY_HELLO, "application/grpc-web-text", chunks)
        .await
        .expect_ok_web("application/grpc-web-text");
    body.expect_status(Code::Ok);
    assert_eq!(body.messages.len(), 1);
    assert_eq!(reply_message(&body.messages[0]).message(), "ada");
}

#[tokio::test]
async fn grpc_web_text_server_streaming_is_streaming_safe_base64() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    let body = peer
        .call(
            SERVER_HELLO,
            "application/grpc-web-text+proto",
            text_frame(&hello_request("ada,bob")),
        )
        .await
        .expect_ok_web("application/grpc-web-text+proto");
    body.expect_status(Code::Ok);
    let replies: Vec<_> = body.messages.iter().map(reply_message).collect();
    assert_eq!(
        replies
            .iter()
            .map(|reply| reply.message().to_string())
            .collect::<Vec<_>>(),
        ["ada".to_owned(), "bob".to_owned()]
    );
}

#[tokio::test]
async fn grpc_web_cors_preflight_defaults_to_deny_all() {
    let (addr, _guard) = spawn_greeter_server(ServerConfig::new()).await;
    let mut peer = RawWebPeer::connect(addr).await;
    peer.preflight(SAY_HELLO, "https://app.example")
        .await
        .expect_http(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn grpc_web_cors_preflight_allows_configured_origin() {
    let (addr, _guard) =
        spawn_greeter_server(ServerConfig::new().grpc_web_allow_origin("https://app.example"))
            .await;
    let mut peer = RawWebPeer::connect(addr).await;
    peer.preflight(SAY_HELLO, "https://app.example")
        .await
        .expect_preflight_allowed("https://app.example");
}
