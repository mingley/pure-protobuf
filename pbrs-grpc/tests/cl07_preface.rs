//! CL-07: distinguish HTTP/2's permitted preface pipelining from the
//! native channel's SETTINGS-bounded connection and stream admission.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "bounded raw-peer integration assertions"
)]

use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Channel, ChannelConfig, Code, Request};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const QUIET: Duration = Duration::from_millis(30);
const BOUND: Duration = Duration::from_secs(2);

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
}

async fn frame(io: &mut (impl AsyncRead + Unpin)) -> Frame {
    let mut header = [0; 9];
    io.read_exact(&mut header).await.expect("frame header");
    let length =
        usize::from(header[0]) * 65536 + usize::from(header[1]) * 256 + usize::from(header[2]);
    assert!(length <= 16384, "unexpected large frame in tiny request");
    let mut payload = vec![0; length];
    io.read_exact(&mut payload).await.expect("frame payload");
    Frame {
        kind: header[3],
        flags: header[4],
        stream: u32::from_be_bytes(header[5..9].try_into().expect("stream id")) & 0x7fff_ffff,
    }
}

async fn client_preface(io: &mut (impl AsyncRead + Unpin)) {
    let mut preface = [0; 24];
    io.read_exact(&mut preface).await.expect("client preface");
    assert_eq!(&preface, PREFACE);
    let settings = frame(io).await;
    assert_eq!((settings.kind, settings.flags, settings.stream), (4, 0, 0));
}

async fn settings(io: &mut (impl AsyncWrite + Unpin), streams: u32) {
    let mut bytes = vec![0, 0, 6, 4, 0, 0, 0, 0, 0, 0, 3];
    bytes.extend_from_slice(&streams.to_be_bytes());
    io.write_all(&bytes).await.expect("peer SETTINGS");
    io.flush().await.expect("flush SETTINGS");
}

async fn next_headers(io: &mut (impl AsyncRead + Unpin)) -> u32 {
    tokio::time::timeout(BOUND, async {
        loop {
            let next = frame(io).await;
            if next.kind == 1 {
                return next.stream;
            }
        }
    })
    .await
    .expect("request HEADERS")
}

async fn no_headers(io: &mut (impl AsyncRead + Unpin)) {
    let result = tokio::time::timeout(QUIET, async {
        loop {
            assert_ne!(frame(io).await.kind, 1, "HEADERS before stream admission");
        }
    })
    .await;
    assert!(result.is_err(), "quiet interval must end at its bound");
}

fn rpc(
    channel: &Channel,
) -> AbortOnDrop<Result<pbrs_grpc::Response<HelloReply>, pbrs_grpc::Status>> {
    let call = channel.unary(
        "/helloworld.Greeter/SayHello",
        Request::new(HelloRequest::new()),
    );
    AbortOnDrop(tokio::spawn(call))
}

#[tokio::test(start_paused = true)]
async fn h2_api_can_pipeline_headers_before_peer_settings() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let (send, conn) = h2::client::handshake(client)
        .await
        .expect("h2 handshake without peer bytes");
    let _driver = AbortOnDrop(tokio::spawn(conn));
    client_preface(&mut peer).await;
    let mut send = send.ready().await.expect("ready without peer SETTINGS");
    let (response, _stream) = send
        .send_request(
            http::Request::builder()
                .method("POST")
                .uri("http://localhost/helloworld.Greeter/SayHello")
                .body(())
                .expect("request"),
            true,
        )
        .expect("pre-SETTINGS request");
    assert_eq!(next_headers(&mut peer).await, 1);
    drop(response);
}

#[tokio::test(start_paused = true)]
async fn eager_connect_waits_for_settings_and_positive_stream_cap() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let mut connect = AbortOnDrop(tokio::spawn(Channel::from_io(client, "localhost")));
    client_preface(&mut peer).await;
    no_headers(&mut peer).await;
    assert!(
        !connect.0.is_finished(),
        "silent peer must not look connected"
    );
    settings(&mut peer, 0).await;
    no_headers(&mut peer).await;
    assert!(!connect.0.is_finished(), "zero peer stream cap must wait");
    settings(&mut peer, 1).await;
    let channel = tokio::time::timeout(BOUND, &mut connect.0)
        .await
        .expect("positive SETTINGS finishes connect")
        .expect("connect task")
        .expect("channel");
    let _call = rpc(&channel);
    assert_eq!(next_headers(&mut peer).await, 1);
}

#[tokio::test(start_paused = true)]
async fn silent_peer_connect_timeout_drops_transport() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let mut connect = AbortOnDrop(tokio::spawn(Channel::from_io_with(
        client,
        "localhost",
        ChannelConfig::new().connect_timeout(Duration::from_millis(100)),
    )));
    client_preface(&mut peer).await;
    let error = (&mut connect.0)
        .await
        .expect("connect task")
        .expect_err("silent peer cannot finish connect");
    assert_eq!(error.code(), Code::Unavailable);
    let mut tail = Vec::new();
    peer.read_to_end(&mut tail).await.expect("transport EOF");
    // An initial connection WINDOW_UPDATE may follow SETTINGS, but no RPC
    // exists in this eager-connect test.
}

#[tokio::test(start_paused = true)]
async fn cancelling_eager_handshake_drops_transport() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let mut connect = AbortOnDrop(tokio::spawn(Channel::from_io(client, "localhost")));
    client_preface(&mut peer).await;
    connect.0.abort();
    assert!(
        (&mut connect.0)
            .await
            .expect_err("aborted connect")
            .is_cancelled()
    );
    let mut tail = Vec::new();
    peer.read_to_end(&mut tail).await.expect("transport EOF");
}

#[tokio::test]
async fn lazy_channel_does_not_dial_until_polled_rpc_and_cancel_closes_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let channel = Channel::connect_lazy(listener.local_addr().expect("address")).expect("lazy");
    assert!(
        tokio::time::timeout(QUIET, listener.accept())
            .await
            .is_err()
    );
    let mut call = rpc(&channel);
    let (mut peer, _) = tokio::time::timeout(BOUND, listener.accept())
        .await
        .expect("lazy first dial")
        .expect("accepted");
    client_preface(&mut peer).await;
    no_headers(&mut peer).await;
    call.0.abort();
    assert!((&mut call.0).await.expect_err("aborted RPC").is_cancelled());
    let mut tail = Vec::new();
    tokio::time::timeout(BOUND, peer.read_to_end(&mut tail))
        .await
        .expect("cancel closes cold transport")
        .expect("EOF");
}

async fn cold_deadline(wait_for_ready: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let channel = Channel::connect_lazy_with(
        listener.local_addr().expect("address"),
        ChannelConfig::new().wait_for_ready(wait_for_ready),
    )
    .expect("lazy")
    .timeout(Duration::from_millis(150));
    let mut call = rpc(&channel);
    let (mut peer, _) = listener.accept().await.expect("accepted");
    client_preface(&mut peer).await;
    no_headers(&mut peer).await;
    let error = tokio::time::timeout(BOUND, &mut call.0)
        .await
        .expect("deadline bounds handshake")
        .expect("RPC task")
        .expect_err("silent peer");
    assert_eq!(error.code(), Code::DeadlineExceeded);
    let mut tail = Vec::new();
    tokio::time::timeout(BOUND, peer.read_to_end(&mut tail))
        .await
        .expect("deadline closes cold transport")
        .expect("EOF");
}

#[tokio::test]
async fn lazy_first_rpc_deadline_bounds_settings_wait() {
    cold_deadline(false).await;
}

#[tokio::test]
async fn lazy_first_rpc_waits_for_peer_settings_zero_then_one() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let channel = Channel::connect_lazy(listener.local_addr().expect("address")).expect("lazy");
    let _call = rpc(&channel);
    let (mut peer, _) = listener.accept().await.expect("accepted");
    client_preface(&mut peer).await;
    no_headers(&mut peer).await;
    settings(&mut peer, 0).await;
    no_headers(&mut peer).await;
    settings(&mut peer, 1).await;
    assert_eq!(next_headers(&mut peer).await, 1);
}

#[tokio::test]
async fn wait_for_ready_deadline_bounds_settings_wait() {
    cold_deadline(true).await;
}

#[tokio::test(start_paused = true)]
async fn peer_stream_cap_one_serializes_first_two_requests() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let mut connect = AbortOnDrop(tokio::spawn(Channel::from_io(client, "localhost")));
    client_preface(&mut peer).await;
    settings(&mut peer, 1).await;
    let channel = (&mut connect.0).await.expect("task").expect("channel");
    let mut first = rpc(&channel);
    assert_eq!(next_headers(&mut peer).await, 1);
    let _second = rpc(&channel);
    no_headers(&mut peer).await;
    first.0.abort();
    assert!(
        (&mut first.0)
            .await
            .expect_err("aborted first RPC")
            .is_cancelled()
    );
    assert_eq!(next_headers(&mut peer).await, 3);
}

#[tokio::test]
async fn wait_for_ready_retries_closed_handshake_before_opening_first_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let channel = Channel::connect_lazy(listener.local_addr().expect("address"))
        .expect("lazy")
        .wait_for_ready()
        .timeout(BOUND);
    let _call = rpc(&channel);
    let (mut first, _) = listener.accept().await.expect("first dial");
    client_preface(&mut first).await;
    drop(first);
    let (mut second, _) = tokio::time::timeout(BOUND, listener.accept())
        .await
        .expect("retry")
        .expect("second dial");
    client_preface(&mut second).await;
    no_headers(&mut second).await;
    settings(&mut second, 1).await;
    assert_eq!(next_headers(&mut second).await, 1);
}
