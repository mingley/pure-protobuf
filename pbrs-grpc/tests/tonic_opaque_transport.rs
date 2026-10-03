//! Configured native identity/gzip message caps on unchanged opaque tonic bodies.

#![cfg(feature = "tonic")]

use bytes::{Bytes, BytesMut};
use http::{Request, Response};
use http_body::{Body, Frame};
use pbrs_grpc::tonic_server::TonicServerExt;
use pbrs_grpc::{Server, ServerConfig};
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tonic::body::Body as TonicBody;
use tonic::server::NamedService;
use tower::Service;

#[derive(Clone)]
struct Probe {
    output: Vec<Bytes>,
    state: Arc<State>,
    read_input: bool,
    encoding: Option<&'static str>,
    pending_tail: bool,
    disable_compression: bool,
}

#[derive(Default)]
struct State {
    ready: AtomicUsize,
    calls: AtomicUsize,
    dropped: AtomicUsize,
    input_dropped: AtomicUsize,
}

struct InputDrop(Arc<State>);
impl Drop for InputDrop {
    fn drop(&mut self) {
        self.0.input_dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl NamedService for Probe {
    const NAME: &'static str = "test.IdentityCaps";
}

impl Service<Request<TonicBody>> for Probe {
    type Response = Response<Frames>;
    type Error = tonic::Status;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.state.ready.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<TonicBody>) -> Self::Future {
        self.state.calls.fetch_add(1, Ordering::SeqCst);
        let probe = self.clone();
        Box::pin(async move {
            let mut frames = VecDeque::new();
            if probe.read_input {
                let _guard = InputDrop(probe.state.clone());
                let mut body = request.into_body();
                while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
                    if let Ok(bytes) = frame?.into_data() {
                        frames.push_back(Ok(Frame::data(bytes)));
                    }
                }
            } else {
                frames.extend(
                    probe
                        .output
                        .iter()
                        .cloned()
                        .map(|bytes| Ok(Frame::data(bytes))),
                );
            }
            if !probe.pending_tail {
                let mut trailers = http::HeaderMap::new();
                trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                trailers.insert("x-terminal", http::HeaderValue::from_static("preserved"));
                frames.push_back(Ok(Frame::trailers(trailers)));
            }
            let mut response = Response::builder()
                .header("content-type", "application/grpc")
                .header("x-initial", "preserved");
            if let Some(encoding) = probe.encoding {
                response = response.header("grpc-encoding", encoding);
            }
            let mut response = response
                .body(Frames {
                    frames,
                    state: probe.state,
                    pending_tail: probe.pending_tail,
                })
                .map_err(|error| tonic::Status::internal(error.to_string()))?;
            if probe.disable_compression {
                response
                    .extensions_mut()
                    .insert(tonic::codec::SingleMessageCompressionOverride::Disable);
            }
            Ok(response)
        })
    }
}

struct Frames {
    frames: VecDeque<Result<Frame<Bytes>, tonic::Status>>,
    state: Arc<State>,
    pending_tail: bool,
}
impl Drop for Frames {
    fn drop(&mut self) {
        self.state.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl Body for Frames {
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        match self.frames.pop_front() {
            Some(frame) => Poll::Ready(Some(frame)),
            None if self.pending_tail => Poll::Pending,
            None => Poll::Ready(None),
        }
    }
}

struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

struct Fixture {
    client: h2::client::SendRequest<Bytes>,
    _tasks: Tasks,
    state: Arc<State>,
}

async fn connect(service: Probe, config: ServerConfig, window: u32) -> Fixture {
    connect_with_tracker(service, config, window, None).await
}

#[expect(
    clippy::expect_used,
    reason = "transport fixture setup must fail the test on error"
)]
async fn connect_with_tracker(
    service: Probe,
    config: ServerConfig,
    window: u32,
    tracker: Option<pbrs_grpc::ByteBudgetTracker>,
) -> Fixture {
    let state = service.state.clone();
    let (client, io) = tokio::io::duplex(64 * 1024);
    let mut server = Server::new(service.into_pbrs_service()).config(config);
    if let Some(tracker) = tracker {
        server = server.with_byte_budget_tracker(tracker);
    }
    let serving = tokio::spawn(async move {
        server.serve_connection(io).await.expect("server");
    });
    let (client, connection) = h2::client::Builder::new()
        .initial_window_size(window)
        .handshake(client)
        .await
        .expect("handshake");
    let driver = tokio::spawn(async move {
        connection.await.ok();
    });
    Fixture {
        client,
        _tasks: Tasks(vec![serving, driver]),
        state,
    }
}

#[expect(clippy::expect_used, reason = "the fixed test request must be valid")]
fn request() -> Request<()> {
    Request::builder()
        .method("POST")
        .uri("http://localhost/test.IdentityCaps/Call")
        .header("content-type", "application/grpc")
        .body(())
        .expect("request")
}

#[expect(
    clippy::expect_used,
    reason = "unexpected fixture transport failures must fail the test"
)]
async fn exchange(
    fixture: &mut Fixture,
    request: Request<()>,
    upload: Vec<Bytes>,
) -> (Bytes, http::HeaderMap, http::HeaderMap) {
    let (response, mut send) = fixture
        .client
        .send_request(request, upload.is_empty())
        .expect("request");
    let mut upload = upload.into_iter().peekable();
    while let Some(bytes) = upload.next() {
        send.send_data(bytes, upload.peek().is_none())
            .expect("upload");
    }
    collect(response.await.expect("response")).await
}

#[expect(
    clippy::expect_used,
    reason = "unexpected DATA, credit or trailer failures must fail the test"
)]
async fn collect(response: Response<h2::RecvStream>) -> (Bytes, http::HeaderMap, http::HeaderMap) {
    let headers = response.headers().clone();
    let mut body = response.into_body();
    let mut bytes = BytesMut::new();
    while let Some(data) = body.data().await {
        let data = data.expect("data");
        body.flow_control()
            .release_capacity(data.len())
            .expect("credit");
        bytes.extend_from_slice(&data);
    }
    let terminal = body
        .trailers()
        .await
        .expect("trailers")
        .unwrap_or_else(|| headers.clone());
    (bytes.freeze(), terminal, headers)
}

fn probe(output: Vec<Bytes>) -> Probe {
    Probe {
        output,
        state: Arc::new(State::default()),
        read_input: false,
        encoding: None,
        pending_tail: false,
        disable_compression: false,
    }
}

#[expect(
    clippy::expect_used,
    reason = "gzip test fixtures must encode successfully"
)]
fn gzip_wire(payload: &[u8]) -> Bytes {
    let encoded = pbrs_grpc::gzip::encode(payload).expect("gzip");
    let len = u32::try_from(encoded.len()).expect("small fixture");
    let mut wire = vec![1];
    wire.extend_from_slice(&len.to_be_bytes());
    wire.extend_from_slice(&encoded);
    Bytes::from(wire)
}

#[tokio::test]
async fn native_outbound_gzip_zero_cap_does_not_cap_encoded_overhead() {
    let mut fixture = connect(
        probe(vec![Bytes::from_static(b"\0\0\0\0\0")]),
        ServerConfig::default()
            .send_compressed(true)
            .max_encoding_message_size(0),
        65_535,
    )
    .await;
    let mut req = request();
    req.headers_mut().insert(
        "grpc-accept-encoding",
        http::HeaderValue::from_static("gzip"),
    );
    let (bytes, terminal, headers) = exchange(&mut fixture, req, Vec::new()).await;
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(headers.get("grpc-encoding").expect("gzip"), "gzip");
    assert_eq!(bytes, gzip_wire(b""));
    assert!(bytes.len() > 5);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 1);
}

fn identity_wire(payload: &[u8]) -> Bytes {
    let mut wire = vec![0];
    wire.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small fixture")
            .to_be_bytes(),
    );
    wire.extend_from_slice(payload);
    Bytes::from(wire)
}

#[tokio::test]
async fn native_outbound_levels_preferences_and_peer_fallback_use_native_bytes() {
    use pbrs_grpc::compression::{Codec, CompressionAlgorithm};
    let payload = b"a fairly compressible payload ".repeat(64);
    for level in [0, 1, 9] {
        for (preferred, accepted, selected) in [
            (
                Codec::Gzip,
                "gzip,deflate",
                Some(CompressionAlgorithm::Gzip),
            ),
            (
                Codec::Deflate,
                "gzip,deflate",
                Some(CompressionAlgorithm::Deflate),
            ),
            (Codec::Deflate, "gzip", Some(CompressionAlgorithm::Gzip)),
            (Codec::Gzip, "deflate", Some(CompressionAlgorithm::Deflate)),
            (Codec::Gzip, "identity", None),
        ] {
            let identity = identity_wire(&payload);
            let mut fixture = connect(
                probe(
                    (0..identity.len())
                        .map(|offset| identity.slice(offset..offset + 1))
                        .collect(),
                ),
                ServerConfig::default()
                    .send_compressed(true)
                    .gzip_compression_level(level)
                    .compression_codec(preferred)
                    .max_encoding_message_size(payload.len()),
                1,
            )
            .await;
            let mut req = request();
            req.headers_mut().insert(
                "grpc-accept-encoding",
                accepted.parse().expect("acceptance"),
            );
            let (wire, terminal, headers) = exchange(&mut fixture, req, Vec::new()).await;
            assert_eq!(terminal.get("grpc-status").expect("status"), "0");
            assert_eq!(headers.get("x-initial").expect("header"), "preserved");
            assert_eq!(terminal.get("x-terminal").expect("trailer"), "preserved");
            let expected = if let Some(codec) = selected {
                assert_eq!(headers.get("grpc-encoding").expect("coding"), codec.name());
                let encoded = codec
                    .encode_level(&payload, level)
                    .expect("native encoding");
                let mut expected = identity_wire(&encoded).to_vec();
                *expected.first_mut().expect("flag") = 1;
                Bytes::from(expected)
            } else {
                assert!(headers.get("grpc-encoding").is_none());
                identity_wire(&payload)
            };
            assert_eq!(wire, expected);
        }
    }
}

#[tokio::test]
async fn native_outbound_failure_withholds_later_prefix_and_preserves_first_message() {
    for later in [identity_wire(b"ab"), Bytes::from_static(b"\x01\0\0\0\0")] {
        let first = identity_wire(b"x");
        let mut fixture = connect(
            probe(vec![Bytes::from([first.as_ref(), later.as_ref()].concat())]),
            ServerConfig::default()
                .send_compressed(true)
                .max_encoding_message_size(1),
            65_535,
        )
        .await;
        let mut req = request();
        req.headers_mut().insert(
            "grpc-accept-encoding",
            http::HeaderValue::from_static("gzip"),
        );
        let (wire, terminal, _) = exchange(&mut fixture, req, Vec::new()).await;
        assert_eq!(wire, gzip_wire(b"x"));
        assert_eq!(
            terminal.get("grpc-status").expect("status"),
            if later.first() == Some(&1) { "9" } else { "8" }
        );
        assert_eq!(fixture.state.dropped.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn native_outbound_transform_preserves_independent_input_opt_out_advertisement() {
    let mut fixture = connect(
        probe(vec![identity_wire(b"x")]),
        ServerConfig::default()
            .accept_compressed(false)
            .send_compressed(true)
            .max_encoding_message_size(1),
        1,
    )
    .await;
    let mut req = request();
    req.headers_mut().insert(
        "grpc-accept-encoding",
        http::HeaderValue::from_static("gzip"),
    );
    let (wire, terminal, headers) = exchange(&mut fixture, req, Vec::new()).await;
    assert_eq!(wire, gzip_wire(b"x"));
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(headers.get("grpc-encoding").expect("coding"), "gzip");
    assert_eq!(
        headers.get("grpc-accept-encoding").expect("acceptance"),
        "identity"
    );
    assert_eq!(
        terminal.get("grpc-accept-encoding").expect("acceptance"),
        "identity"
    );
}

#[tokio::test]
async fn native_outbound_unexpected_coding_and_override_fail_before_headers() {
    for override_only in [false, true] {
        let mut service = probe(vec![identity_wire(b"x")]);
        service.disable_compression = override_only;
        service.encoding = (!override_only).then_some("gzip");
        let mut fixture = connect(
            service,
            ServerConfig::default()
                .send_compressed(true)
                .max_encoding_message_size(1),
            65_535,
        )
        .await;
        let (wire, terminal, headers) = exchange(&mut fixture, request(), Vec::new()).await;
        assert!(wire.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "9");
        assert!(headers.get("x-initial").is_none());
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.state.dropped.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn native_outbound_deadline_drops_partial_transform_and_its_capacity() {
    let tracker = pbrs_grpc::ByteBudgetTracker::unlimited();
    let mut service = probe(vec![Bytes::from_static(b"\0\0\0\0\x03ab")]);
    service.pending_tail = true;
    let mut fixture = connect_with_tracker(
        service,
        ServerConfig::default()
            .send_compressed(true)
            .max_encoding_message_size(3),
        65_535,
        Some(tracker.clone()),
    )
    .await;
    let mut req = request();
    req.headers_mut().insert(
        "grpc-accept-encoding",
        http::HeaderValue::from_static("gzip"),
    );
    req.headers_mut()
        .insert("grpc-timeout", http::HeaderValue::from_static("50m"));
    let (wire, terminal, _) = exchange(&mut fixture, req, Vec::new()).await;
    assert!(wire.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "4");
    assert!(tracker.peak_allocated() >= 3);
    assert_eq!(tracker.allocated(), 0);
    assert_eq!(tracker.active_byte_permit_tokens(), 0);
    assert_eq!(fixture.state.dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_outbound_h2_backlog_keeps_output_permit_until_peer_reset() {
    let tracker = pbrs_grpc::ByteBudgetTracker::unlimited();
    let payload = vec![7; 32 * 1024];
    let mut service = probe(vec![identity_wire(&payload)]);
    service.pending_tail = true;
    let mut fixture = connect_with_tracker(
        service,
        ServerConfig::default()
            .send_compressed(true)
            .gzip_compression_level(0)
            .max_encoding_message_size(payload.len()),
        1,
        Some(tracker.clone()),
    )
    .await;
    let mut req = request();
    req.headers_mut().insert(
        "grpc-accept-encoding",
        http::HeaderValue::from_static("gzip"),
    );
    let (response, mut send) = fixture.client.send_request(req, true).expect("request");
    let response = response.await.expect("headers");
    tokio::time::timeout(Duration::from_secs(2), async {
        while tracker.peak_allocated() < payload.len() * 2
            || tracker.active_byte_permit_tokens() != 1
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completed compressed output held by backlog");
    assert!(tracker.allocated() >= payload.len());
    assert!(tracker.active_byte_permit_tokens() > 0);
    send.send_reset(h2::Reason::CANCEL);
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), async {
        while tracker.allocated() != 0 || tracker.active_byte_permit_tokens() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("reset releases all transform storage");
    wait_counter(&fixture.state.dropped, 1).await;
}

#[cfg(feature = "zstd")]
#[tokio::test]
async fn native_outbound_selected_zstd_stays_unsupported_before_readiness() {
    let mut fixture = connect(
        probe(vec![identity_wire(b"")]),
        ServerConfig::default()
            .send_compressed(true)
            .compression_algorithm(pbrs_grpc::compression::CompressionAlgorithm::Zstd)
            .max_encoding_message_size(0),
        65_535,
    )
    .await;
    let mut req = request();
    req.headers_mut().insert(
        "grpc-accept-encoding",
        http::HeaderValue::from_static("zstd"),
    );
    let (wire, terminal, _) = exchange(&mut fixture, req, Vec::new()).await;
    assert!(wire.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "9");
    assert_eq!(fixture.state.ready.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn native_gzip_decoder_uses_first_member_and_ignores_encoded_tail() {
    let first = pbrs_grpc::gzip::encode(b"one").expect("first");
    for tail in [
        pbrs_grpc::gzip::encode(b"two").expect("second"),
        vec![255, 0, 123],
    ] {
        let joined = [first.as_slice(), tail.as_slice()].concat();
        assert_eq!(
            pbrs_grpc::gzip::decode_limited(
                &joined,
                pbrs_grpc::MessageLimits::default().with_max_decoding(3),
            )
            .expect("first member only"),
            b"one"
        );
    }
}

#[tokio::test]
async fn finite_inbound_gzip_cap_preserves_original_encoded_frame() {
    let wire = gzip_wire(b"abc");
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(64)
            .initial_stream_window_size(1),
        1,
    )
    .await;
    let mut req = request();
    req.headers_mut()
        .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
    let upload = (0..wire.len())
        .map(|offset| wire.slice(offset..offset + 1))
        .collect();
    let (bytes, terminal, _) =
        tokio::time::timeout(Duration::from_secs(2), exchange(&mut fixture, req, upload))
            .await
            .expect("tiny windows must progress");
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(bytes, wire);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 1);
}

async fn response(
    output: Vec<Bytes>,
    config: ServerConfig,
) -> (Bytes, http::HeaderMap, Arc<State>) {
    let mut fixture = connect(probe(output), config, 65_535).await;
    let (bytes, terminal, _) = exchange(&mut fixture, request(), Vec::new()).await;
    (bytes, terminal, fixture.state.clone())
}

#[tokio::test]
async fn identity_outbound_cap_rejects_oversize_without_emitting_prefix() {
    let wire = Bytes::from_static(b"\x00\x00\x00\x00\x04abcd");
    let (bytes, terminal, calls) = response(
        vec![wire.slice(..2), wire.slice(2..)],
        ServerConfig::default().max_encoding_message_size(3),
    )
    .await;
    assert_eq!(terminal.get("grpc-status").expect("status"), "8");
    assert!(bytes.is_empty(), "rejected prefix must stay off the wire");
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn identity_output_caps_accept_boundaries_and_preserve_valid_messages_before_failure() {
    let first = Bytes::from_static(b"\x00\x00\x00\x00\x02ab");
    let exact = Bytes::from_static(b"\x00\x00\x00\x00\x03abc");
    let oversized = Bytes::from_static(b"\x00\x00\x00\x00\x04abcd");
    let mut joined = BytesMut::new();
    joined.extend_from_slice(&first);
    joined.extend_from_slice(&exact);
    joined.extend_from_slice(&oversized);
    let (bytes, terminal, _) = response(
        vec![joined.freeze()],
        ServerConfig::default().max_encoding_message_size(3),
    )
    .await;
    assert_eq!(bytes, [first.as_ref(), exact.as_ref()].concat());
    assert_eq!(terminal.get("grpc-status").expect("status"), "8");
}

#[tokio::test]
async fn identity_input_and_output_caps_handle_tiny_windows_fragmentation_and_trailers() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let config = ServerConfig::default()
        .max_decoding_message_size(3)
        .max_encoding_message_size(3)
        .initial_stream_window_size(1);
    let mut fixture = connect(echo, config, 1).await;
    let wire =
        Bytes::from_static(b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03abc\x00\x00\x00\x00\x02xy");
    let mut upload = vec![Bytes::new()];
    upload.extend((0..wire.len()).map(|offset| wire.slice(offset..offset + 1)));
    let (bytes, terminal, headers) = tokio::time::timeout(
        Duration::from_secs(2),
        exchange(&mut fixture, request(), upload),
    )
    .await
    .expect("tiny-window prefix forwarding must make progress");
    assert_eq!(bytes, wire);
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(terminal.get("x-terminal").expect("trailers"), "preserved");
    assert_eq!(headers.get("x-initial").expect("headers"), "preserved");
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn identity_input_caps_reject_header_claim_before_payload_arrives() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default().max_decoding_message_size(3),
        65_535,
    )
    .await;
    let (bytes, terminal, _) = exchange(
        &mut fixture,
        request(),
        vec![
            Bytes::from_static(b"\x00\x00"),
            Bytes::from_static(b"\x00\x00\x04"),
        ],
    )
    .await;
    assert!(bytes.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "8");
}

#[tokio::test]
async fn unsupported_compressed_input_rejects_before_readiness_but_output_after_handler() {
    let mut fixture = connect(
        probe(Vec::new()),
        ServerConfig::default().max_decoding_message_size(3),
        65_535,
    )
    .await;
    let mut compressed = request();
    compressed
        .headers_mut()
        .insert("grpc-encoding", http::HeaderValue::from_static("deflate"));
    let (bytes, terminal, _) = exchange(&mut fixture, compressed, Vec::new()).await;
    assert!(bytes.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "9");
    assert_eq!(fixture.state.ready.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 0);
    let mut encoded = probe(vec![Bytes::from_static(b"\x00\x00\x00\x00\x00")]);
    encoded.encoding = Some("gzip");
    let mut fixture = connect(
        encoded,
        ServerConfig::default().max_encoding_message_size(3),
        65_535,
    )
    .await;
    let (bytes, terminal, headers) = exchange(&mut fixture, request(), Vec::new()).await;
    assert!(bytes.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "9");
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 1);
    assert!(
        !headers.contains_key("x-initial"),
        "unsupported response headers stay off the wire"
    );
}

#[tokio::test]
async fn malformed_identity_output_never_counts_as_clean_success() {
    for wire in [
        b"\x00\x00".as_slice(),
        b"\x00\x00\x00\x00\x02x".as_slice(),
        b"\x02\x00\x00\x00\x00".as_slice(),
    ] {
        let (_, terminal, state) = response(
            vec![Bytes::copy_from_slice(wire)],
            ServerConfig::default().max_encoding_message_size(3),
        )
        .await;
        assert_eq!(terminal.get("grpc-status").expect("status"), "13");
        assert_eq!(state.dropped.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn deadline_while_waiting_for_output_prefix_drops_body_and_recovers_rpc_slot() {
    let mut pending = probe(vec![Bytes::from_static(b"\x00\x00")]);
    pending.pending_tail = true;
    let mut fixture = connect(
        pending,
        ServerConfig::default()
            .max_encoding_message_size(3)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    for call in 1..=2 {
        let mut request = request();
        request
            .headers_mut()
            .insert("grpc-timeout", http::HeaderValue::from_static("50m"));
        let (bytes, terminal, _) = exchange(&mut fixture, request, Vec::new()).await;
        assert!(bytes.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "4");
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), call);
        assert_eq!(fixture.state.dropped.load(Ordering::SeqCst), call);
    }
}

#[expect(
    clippy::expect_used,
    reason = "missing server lifecycle progress must fail the test"
)]
async fn wait_counter(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("server lifecycle progress");
}

#[tokio::test]
async fn reset_during_partial_input_prefix_drops_reader_and_next_rpc_succeeds() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(3)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    let (response, mut send) = fixture
        .client
        .send_request(request(), false)
        .expect("request");
    send.send_data(Bytes::from_static(b"\x00\x00"), false)
        .expect("partial prefix");
    wait_counter(&fixture.state.calls, 1).await;
    send.send_reset(h2::Reason::CANCEL);
    assert!(response.await.is_err());
    wait_counter(&fixture.state.input_dropped, 1).await;
    let wire = Bytes::from_static(b"\x00\x00\x00\x00\x00");
    let (bytes, terminal, _) = exchange(&mut fixture, request(), vec![wire.clone()]).await;
    assert_eq!(bytes, wire);
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn deadline_during_open_partial_input_prefix_drops_reader() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default().max_decoding_message_size(3),
        65_535,
    )
    .await;
    let mut request = request();
    request
        .headers_mut()
        .insert("grpc-timeout", http::HeaderValue::from_static("50m"));
    let (response, mut send) = fixture
        .client
        .send_request(request, false)
        .expect("request");
    send.send_data(Bytes::from_static(b"\x00\x00"), false)
        .expect("partial prefix");
    let (bytes, terminal, _) = collect(response.await.expect("response")).await;
    assert!(bytes.is_empty());
    assert_eq!(terminal.get("grpc-status").expect("status"), "4");
    assert_eq!(fixture.state.input_dropped.load(Ordering::SeqCst), 1);
    drop(send);
}

#[tokio::test]
async fn reset_during_partial_output_prefix_drops_producer_and_recovers_rpc_slot() {
    let mut pending = probe(vec![Bytes::from_static(b"\x00\x00")]);
    pending.pending_tail = true;
    let mut fixture = connect(
        pending,
        ServerConfig::default()
            .max_encoding_message_size(3)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    for call in 1..=2 {
        let (response, mut send) = fixture
            .client
            .send_request(request(), true)
            .expect("request");
        let response = response.await.expect("headers before partial prefix");
        assert!(!response.headers().contains_key("grpc-status"));
        send.send_reset(h2::Reason::CANCEL);
        drop(response);
        wait_counter(&fixture.state.dropped, call).await;
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), call);
    }
}

#[tokio::test]
async fn finite_gzip_input_rejects_encoded_claim_and_inflated_bomb_without_prefix() {
    for (wire, max) in [
        (Bytes::from_static(b"\x01\0\0\0\x80"), 64),
        (gzip_wire(b""), 3),
        (gzip_wire(&[7; 513]), 512),
    ] {
        let mut echo = probe(Vec::new());
        echo.read_input = true;
        let mut fixture = connect(
            echo,
            ServerConfig::default().max_decoding_message_size(max),
            65_535,
        )
        .await;
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        let (bytes, terminal, _) = exchange(&mut fixture, req, vec![wire]).await;
        assert!(bytes.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "8");
        assert_eq!(fixture.state.input_dropped.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn invalid_gzip_and_partial_encoded_frames_end_in_internal() {
    let mut corrupt = gzip_wire(b"abc").to_vec();
    *corrupt.last_mut().expect("footer") ^= 1;
    let valid = gzip_wire(b"abc");
    for wire in [
        Bytes::from(corrupt),
        valid.slice(..valid.len() - 1),
        Bytes::from_static(b"\x01\0\0\0\x03bad"),
    ] {
        let mut echo = probe(Vec::new());
        echo.read_input = true;
        let mut fixture = connect(
            echo,
            ServerConfig::default().max_decoding_message_size(64),
            65_535,
        )
        .await;
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        let (bytes, terminal, _) = exchange(&mut fixture, req, vec![wire]).await;
        assert!(bytes.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "13");
        assert_eq!(fixture.state.input_dropped.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn unlimited_gzip_input_remains_unsupported_before_readiness() {
    let config = ServerConfig::default()
        .message_limits(pbrs_grpc::MessageLimits::default().with_unlimited_decoding());
    let mut fixture = connect(probe(Vec::new()), config, 65_535).await;
    let mut req = request();
    req.headers_mut()
        .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
    let (_, terminal, _) = exchange(&mut fixture, req, Vec::new()).await;
    assert_eq!(terminal.get("grpc-status").expect("status"), "9");
    assert_eq!(fixture.state.ready.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deadline_in_partial_gzip_header_drops_reader_and_recovers_slot() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(64)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    let wire = gzip_wire(b"abc");
    for call in 1..=2 {
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        req.headers_mut()
            .insert("grpc-timeout", http::HeaderValue::from_static("50m"));
        let (response, mut send) = fixture.client.send_request(req, false).expect("request");
        send.send_data(wire.slice(..8), false)
            .expect("partial header");
        let (bytes, terminal, _) = collect(response.await.expect("response")).await;
        assert!(bytes.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "4");
        wait_counter(&fixture.state.input_dropped, call).await;
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), call);
    }
}

#[tokio::test]
async fn reset_in_partial_gzip_header_drops_reader_and_next_rpc_succeeds() {
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(64)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    let wire = gzip_wire(b"abc");
    let mut req = request();
    req.headers_mut()
        .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
    let (response, mut send) = fixture.client.send_request(req, false).expect("request");
    send.send_data(wire.slice(..8), false)
        .expect("partial header");
    wait_counter(&fixture.state.calls, 1).await;
    send.send_reset(h2::Reason::CANCEL);
    drop(response);
    wait_counter(&fixture.state.input_dropped, 1).await;
    let mut req = request();
    req.headers_mut()
        .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
    let (bytes, terminal, _) = exchange(&mut fixture, req, vec![wire.clone()]).await;
    assert_eq!(bytes, wire);
    assert_eq!(terminal.get("grpc-status").expect("status"), "0");
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn zero_native_gzip_cap_distinguishes_identity_empty_member_and_missing_gzip_header() {
    for (wire, status) in [
        (Bytes::from_static(b"\0\0\0\0\0"), "0"),
        (gzip_wire(b""), "8"),
        (Bytes::from_static(b"\x01\0\0\0\0"), "13"),
    ] {
        let mut echo = probe(Vec::new());
        echo.read_input = true;
        let mut fixture = connect(
            echo,
            ServerConfig::default().max_decoding_message_size(0),
            65_535,
        )
        .await;
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        let (bytes, terminal, _) = exchange(&mut fixture, req, vec![wire.clone()]).await;
        assert_eq!(terminal.get("grpc-status").expect("status"), status);
        if status == "0" {
            assert_eq!(bytes, wire);
        } else {
            assert!(bytes.is_empty());
        }
    }
}

#[tokio::test]
async fn deadline_while_gzip_crc_or_ignored_tail_is_incomplete_recovers_slot() {
    let wire = gzip_wire(&[7; 64]);
    let mut tail = wire.to_vec();
    tail.get_mut(1..5).expect("prefix").copy_from_slice(
        &u32::try_from(wire.len() - 5 + 3)
            .expect("small fixture")
            .to_be_bytes(),
    );
    tail.extend_from_slice(&[255, 0, 123]);
    let tail = Bytes::from(tail);
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(64)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    for (n, partial) in [wire.slice(..wire.len() - 4), tail.slice(..tail.len() - 1)]
        .into_iter()
        .enumerate()
    {
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        req.headers_mut()
            .insert("grpc-timeout", http::HeaderValue::from_static("50m"));
        let (response, mut send) = fixture.client.send_request(req, false).expect("request");
        send.send_data(partial, false).expect("partial gzip");
        let (bytes, terminal, _) = collect(response.await.expect("response")).await;
        assert!(bytes.is_empty());
        assert_eq!(terminal.get("grpc-status").expect("status"), "4");
        wait_counter(&fixture.state.input_dropped, n + 1).await;
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), n + 1);
    }
}

#[tokio::test]
async fn reset_while_gzip_crc_or_ignored_tail_is_incomplete_recovers_slot() {
    let wire = gzip_wire(&[7; 64]);
    let mut tail = wire.to_vec();
    tail.get_mut(1..5).expect("prefix").copy_from_slice(
        &u32::try_from(wire.len() - 5 + 3)
            .expect("small fixture")
            .to_be_bytes(),
    );
    tail.extend_from_slice(&[255, 0, 123]);
    let tail = Bytes::from(tail);
    let mut echo = probe(Vec::new());
    echo.read_input = true;
    let mut fixture = connect(
        echo,
        ServerConfig::default()
            .max_decoding_message_size(64)
            .max_concurrent_rpcs(1),
        65_535,
    )
    .await;
    for (n, partial) in [wire.slice(..wire.len() - 4), tail.slice(..tail.len() - 1)]
        .into_iter()
        .enumerate()
    {
        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        let (response, mut send) = fixture.client.send_request(req, false).expect("request");
        send.send_data(partial, false).expect("partial gzip");
        wait_counter(&fixture.state.calls, n * 2 + 1).await;
        send.send_reset(h2::Reason::CANCEL);
        drop(response);
        wait_counter(&fixture.state.input_dropped, n * 2 + 1).await;

        let mut req = request();
        req.headers_mut()
            .insert("grpc-encoding", http::HeaderValue::from_static("gzip"));
        let (bytes, terminal, _) = exchange(&mut fixture, req, vec![wire.clone()]).await;
        assert_eq!(bytes, wire);
        assert_eq!(terminal.get("grpc-status").expect("status"), "0");
        assert_eq!(fixture.state.calls.load(Ordering::SeqCst), n * 2 + 2);
    }
}
