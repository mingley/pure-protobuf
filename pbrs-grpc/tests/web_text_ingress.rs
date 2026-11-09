//! gRPC-Web text request limits and incremental decoding over HTTP/2.

#![cfg(feature = "grpc-web")]
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "bounded local HTTP/2 integration fixture"
)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use pbrs::Parse;
use pbrs_grpc::{
    Code, CompressionAlgorithm, HelloReply, HelloRequest, Request, Response, Rpc, Server,
    ServerConfig, Service, Status, Streaming,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);
const SERVICE: &str = "test.TextIngress";

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct CountingService(Arc<AtomicUsize>);

impl Service for CountingService {
    const NAME: &'static str = SERVICE;

    async fn call(&self, rpc: Rpc) {
        let calls = Arc::clone(&self.0);
        if rpc.method() == "Stream" {
            rpc.server_streaming(move |_: Request<HelloRequest>| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let (sender, stream) = Streaming::<HelloReply>::channel(1);
                drop(sender);
                Ok::<_, Status>(Response::new(stream))
            })
            .await;
        } else {
            rpc.unary(move |request: Request<HelloRequest>| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let mut reply = HelloReply::new();
                reply.set_message(request.get_ref().name());
                Ok::<_, Status>(Response::new(reply))
            })
            .await;
        }
    }
}

async fn server(limit: usize) -> (SocketAddr, Arc<AtomicUsize>, AbortOnDrop<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("address");
    let calls = Arc::new(AtomicUsize::new(0));
    let service = CountingService(Arc::clone(&calls));
    let task = tokio::spawn(async move {
        Server::new(service)
            .config(ServerConfig::new().max_decoding_message_size(limit))
            .serve_listener(listener)
            .await
            .expect("server");
    });
    (addr, calls, AbortOnDrop(task))
}

struct Peer {
    addr: SocketAddr,
    client: h2::client::SendRequest<Bytes>,
    _driver: AbortOnDrop<()>,
}

impl Peer {
    async fn connect(addr: SocketAddr) -> Self {
        let socket = tokio::time::timeout(WAIT, tokio::net::TcpStream::connect(addr))
            .await
            .expect("connect timeout")
            .expect("connect");
        let (client, connection) = tokio::time::timeout(WAIT, h2::client::handshake(socket))
            .await
            .expect("handshake timeout")
            .expect("handshake");
        let driver = AbortOnDrop(tokio::spawn(async move {
            let _result = connection.await;
        }));
        Self {
            addr,
            client,
            _driver: driver,
        }
    }

    async fn open(
        &self,
        method: &str,
        content_type: &str,
        gzip: bool,
    ) -> (h2::client::ResponseFuture, h2::SendStream<Bytes>) {
        let mut request = http::Request::builder()
            .method(http::Method::POST)
            .uri(format!("http://{}/{SERVICE}/{method}", self.addr))
            .header(http::header::CONTENT_TYPE, content_type);
        if gzip {
            request = request.header("grpc-encoding", "gzip");
        }
        let request = request.body(()).expect("request");
        let mut client = tokio::time::timeout(WAIT, self.client.clone().ready())
            .await
            .expect("client ready timeout")
            .expect("client ready");
        client.send_request(request, false).expect("open stream")
    }
}

struct Answer {
    code: i32,
    message: Option<String>,
    messages: Vec<Vec<u8>>,
}

async fn answer(response: h2::client::ResponseFuture, content_type: &str) -> Answer {
    let response = response.await.expect("response headers");
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some(content_type)
    );
    assert!(response.headers().get("grpc-status").is_none());
    let mut body = response.into_body();
    let mut encoded = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.expect("response data");
        encoded.extend_from_slice(&chunk);
        body.flow_control()
            .release_capacity(chunk.len())
            .expect("response flow credit");
    }
    assert!(body.trailers().await.expect("HTTP/2 trailers").is_none());
    let bytes = STANDARD.decode(encoded).expect("response base64");
    let mut frames = bytes.as_slice();
    let mut messages = Vec::new();
    let mut status = None;
    let mut message = None;
    while !frames.is_empty() {
        assert!(frames.len() >= 5, "incomplete response frame header");
        let flag = frames[0];
        let len = u32::from_be_bytes(frames[1..5].try_into().expect("response length"));
        let len = usize::try_from(len).expect("length fits usize");
        frames = &frames[5..];
        assert!(frames.len() >= len, "incomplete response frame");
        let (payload, rest) = frames.split_at(len);
        frames = rest;
        if flag == 0x80 {
            assert!(status.is_none(), "one Web trailer block");
            let trailers = std::str::from_utf8(payload).expect("trailer text");
            status = trailers
                .lines()
                .find_map(|line| line.strip_prefix("grpc-status: "))
                .map(|code| code.parse::<i32>().expect("numeric status"));
            message = trailers
                .lines()
                .find_map(|line| line.strip_prefix("grpc-message: "))
                .map(str::to_owned);
        } else {
            assert_eq!(flag, 0, "uncompressed response message");
            messages.push(payload.to_vec());
        }
    }
    Answer {
        code: status.expect("Web trailer status"),
        message,
        messages,
    }
}

async fn closed_call(peer: &Peer, content_type: &str, gzip: bool, chunks: &[Bytes]) -> Answer {
    let (response, mut upload) = peer.open("Unary", content_type, gzip).await;
    assert!(!chunks.is_empty(), "fixture has a final chunk");
    for (index, chunk) in chunks.iter().enumerate() {
        upload
            .send_data(chunk.clone(), index + 1 == chunks.len())
            .expect("upload chunk");
    }
    let result = tokio::time::timeout(WAIT, answer(response, content_type))
        .await
        .expect("completed request response timeout");
    drop(upload);
    result
}

fn request_frame(payload: &[u8], compressed: bool) -> Bytes {
    pbrs_grpc::codec::encode(payload, compressed).expect("request frame")
}

// Reproducible pseudo-random DATA boundaries include splits within quanta.
fn split_chunks(bytes: &Bytes, mut state: u32) -> Vec<Bytes> {
    let mut chunks = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = usize::try_from((state >> 16) % 5 + 1).expect("small chunk");
        let end = pos.saturating_add(n).min(bytes.len());
        chunks.push(bytes.slice(pos..end));
        pos = end;
    }
    chunks
}

#[tokio::test]
async fn oversized_text_header_is_rejected_before_upload_half_close() {
    let (addr, calls, _server) = server(8).await;
    let peer = Peer::connect(addr).await;
    for content_type in [
        "application/grpc-web-text",
        "application/grpc-web-text+proto",
    ] {
        for method in ["Unary", "Stream"] {
            let (response, mut upload) = peer.open(method, content_type, false).await;
            // Six decoded bytes: flag 0, declared length 9, and one body byte.
            upload
                .send_data(Bytes::from_static(b"AAAAAAkA"), false)
                .expect("open oversized upload");
            let result = tokio::time::timeout(WAIT, answer(response, content_type))
                .await
                .expect("web text oversized header was not rejected before half-close");
            assert_eq!(result.code, Code::ResourceExhausted.to_i32());
            assert!(result.messages.is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 0, "handler must not run");
            // The upload remains open until the terminal response is observed.
            drop(upload);
        }
    }

    // A rejected upload must not poison the connection for the next RPC.
    let recovery_frame = request_frame(b"\x0a\x02ok", false);
    let recovery = closed_call(
        &peer,
        "application/grpc-web-text",
        false,
        &[Bytes::from(STANDARD.encode(recovery_frame))],
    )
    .await;
    assert_eq!(recovery.code, Code::Ok.to_i32());
    assert_eq!(recovery.messages.len(), 1);
    let reply = HelloReply::parse(&recovery.messages[0]).expect("recovery reply");
    assert_eq!(reply.message(), "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn fragmented_exact_limit_and_malformed_text_controls() {
    let (addr, calls, _server) = server(8).await;
    let peer = Peer::connect(addr).await;
    // The protobuf string is six bytes plus its tag and length: exactly 8.
    let message = b"\x0a\x06aaaaaa";
    let encoded = Bytes::from(STANDARD.encode(request_frame(message, false)));
    assert!(encoded.ends_with(b"=="), "fixture includes split padding");
    let content_type = "application/grpc-web-text";
    let mut expected_calls = 0;
    // The baseline uses padding-indifferent decoding. For a one-byte final
    // quantum, two, one, and zero padding bytes are all accepted at EOF.
    for padding_removed in 0..=2 {
        let variant = encoded.slice(..encoded.len() - padding_removed);
        for seed in [1, 7, 17, 42, 99] {
            let chunks = split_chunks(&variant, seed);
            let success = closed_call(&peer, content_type, false, &chunks).await;
            assert_eq!(success.code, Code::Ok.to_i32());
            assert_eq!(success.messages.len(), 1);
            let reply = HelloReply::parse(&success.messages[0]).expect("reply");
            assert_eq!(reply.message(), "aaaaaa");
            expected_calls += 1;
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        }
    }

    // YR has the same decoded high bits as YQ, but nonzero unused bits.
    let mut invalid_tail = encoded.to_vec();
    let penultimate = invalid_tail.len() - 3;
    assert_eq!(invalid_tail[penultimate], b'Q', "fixture last symbol");
    invalid_tail[penultimate] = b'R';
    let result = closed_call(&peer, content_type, false, &[Bytes::from(invalid_tail)]).await;
    assert_eq!(result.code, Code::InvalidArgument.to_i32());
    assert!(result.messages.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), expected_calls);

    for encoded in [b"!".as_slice(), b"A".as_slice()] {
        let result = closed_call(
            &peer,
            content_type,
            false,
            &[Bytes::copy_from_slice(encoded)],
        )
        .await;
        assert_eq!(result.code, Code::InvalidArgument.to_i32());
        assert!(result.messages.is_empty());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            expected_calls,
            "bad input reached handler"
        );
    }

    // Keep the existing single-base64-entity contract, including across DATA
    // boundaries. Independently padded entities must not be concatenated.
    let padded = Bytes::from_static(b"AAAAAAA=");
    for chunks in [
        vec![Bytes::from_static(b"AAAAAAA=AAAAAAA=")],
        vec![padded.clone(), padded.clone()],
    ] {
        let result = closed_call(&peer, content_type, false, &chunks).await;
        assert_eq!(result.code, Code::InvalidArgument.to_i32());
        assert!(result.messages.is_empty());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            expected_calls,
            "padded input reached handler"
        );
    }

    let mut two = Vec::new();
    two.extend_from_slice(&request_frame(&[], false));
    two.extend_from_slice(&request_frame(&[], false));
    let result = closed_call(
        &peer,
        content_type,
        false,
        &[Bytes::from(STANDARD.encode(two))],
    )
    .await;
    assert_eq!(result.code, Code::Internal.to_i32());
    assert!(result.messages.is_empty());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        expected_calls,
        "duplicate reached handler"
    );
}

#[tokio::test]
async fn compressed_text_obeys_uncompressed_message_limit() {
    let (addr, calls, _server) = server(64).await;
    let peer = Peer::connect(addr).await;
    let mut payload = vec![0x0a, 0x7f];
    payload.extend_from_slice(&[b'a'; 127]);
    let gzip = CompressionAlgorithm::Gzip
        .encode(&payload)
        .expect("gzip fixture");
    assert!(gzip.len() <= 64, "compressed frame itself fits the limit");
    assert!(payload.len() > 64, "inflated payload exceeds the limit");
    let encoded = Bytes::from(STANDARD.encode(request_frame(&gzip, true)));
    let chunks = [encoded.slice(..3), encoded.slice(3..)];
    let result = closed_call(&peer, "application/grpc-web-text", true, &chunks).await;
    assert_eq!(result.code, Code::ResourceExhausted.to_i32());
    assert!(result.messages.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0, "handler must not run");
}

#[tokio::test]
async fn text_error_precedence_follows_frame_order() {
    let (addr, calls, _server) = server(8).await;
    let peer = Peer::connect(addr).await;
    let content_type = "application/grpc-web-text";

    // The first complete message has a compressed flag but no grpc-encoding.
    // Its error must win over the following complete second message.
    let mut invalid_then_valid = Vec::new();
    invalid_then_valid.extend_from_slice(&request_frame(&[], true));
    invalid_then_valid.extend_from_slice(&request_frame(&[], false));
    let result = closed_call(
        &peer,
        content_type,
        false,
        &[Bytes::from(STANDARD.encode(invalid_then_valid))],
    )
    .await;
    assert_eq!(result.code, Code::Internal.to_i32());
    assert!(
        result
            .message
            .as_deref()
            .is_some_and(|s| s.contains("compressed-flag")),
        "first-frame error must take precedence: {:?}",
        result.message
    );
    assert!(result.messages.is_empty());

    // A valid first message followed by a partial next header is truncated,
    // rather than a complete duplicate or a successful unary request.
    let mut valid_then_partial = Vec::new();
    valid_then_partial.extend_from_slice(&request_frame(&[], false));
    valid_then_partial.push(0);
    let result = closed_call(
        &peer,
        content_type,
        false,
        &[Bytes::from(STANDARD.encode(valid_then_partial))],
    )
    .await;
    assert_eq!(result.code, Code::Internal.to_i32());
    assert!(
        result
            .message
            .as_deref()
            .is_some_and(|s| s.contains("truncated")),
        "partial-frame error: {:?}",
        result.message
    );
    assert!(result.messages.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0, "handler must not run");
}

#[tokio::test]
async fn text_crosses_decode_batch_with_exact_limit() {
    // 0x2081 varint describes 4,097 bytes; the tag and length make 4,100.
    let name = "a".repeat(4097);
    let mut message = vec![0x0a, 0x81, 0x20];
    message.extend_from_slice(name.as_bytes());
    assert_eq!(message.len(), 4100);
    let (addr, calls, _server) = server(message.len()).await;
    let peer = Peer::connect(addr).await;
    let encoded = Bytes::from(STANDARD.encode(request_frame(&message, false)));
    assert!(encoded.len() > 4097, "fixture crosses the decode batch");

    for (index, chunks) in [
        vec![encoded.clone()],
        vec![
            encoded.slice(..4095),
            encoded.slice(4095..4097),
            encoded.slice(4097..encoded.len() - 1),
            encoded.slice(encoded.len() - 1..),
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let result = closed_call(&peer, "application/grpc-web-text", false, &chunks).await;
        assert_eq!(result.code, Code::Ok.to_i32());
        assert_eq!(result.messages.len(), 1);
        let reply = HelloReply::parse(&result.messages[0]).expect("reply");
        assert_eq!(reply.message(), name.as_str());
        assert_eq!(calls.load(Ordering::SeqCst), index + 1);
    }
}
