//! A deadline must discard a request waiting for peer stream admission.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "bounded raw-peer protocol assertions"
)]

use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

#[derive(Debug)]
struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

async fn frame(peer: &mut DuplexStream) -> Frame {
    let mut header = [0; 9];
    peer.read_exact(&mut header).await.expect("frame header");
    let length =
        usize::from(header[0]) * 65536 + usize::from(header[1]) * 256 + usize::from(header[2]);
    assert!(length <= 16384, "unexpected frame size");
    let mut payload = vec![0; length];
    peer.read_exact(&mut payload).await.expect("frame payload");
    Frame {
        kind: header[3],
        flags: header[4],
        stream: u32::from_be_bytes(header[5..9].try_into().expect("stream id")) & 0x7fff_ffff,
        payload,
    }
}

async fn settings(peer: &mut DuplexStream, streams: u32) {
    let mut bytes = vec![0, 0, 6, 4, 0, 0, 0, 0, 0, 0, 3];
    bytes.extend_from_slice(&streams.to_be_bytes());
    peer.write_all(&bytes).await.expect("SETTINGS");
    peer.flush().await.expect("flush SETTINGS");
}

fn unary(
    channel: &Channel,
    timeout: Option<Duration>,
) -> pbrs_grpc::Call<pbrs_grpc::Response<HelloReply>> {
    let mut request = Request::new(HelloRequest::new());
    if let Some(timeout) = timeout {
        request.set_timeout(timeout);
    }
    channel.unary("/helloworld.Greeter/SayHello", request)
}

#[tokio::test(start_paused = true)]
async fn expired_queued_unary_never_emits_request_after_peer_capacity_grows() {
    let (client, mut peer) = tokio::io::duplex(65536);
    let connect = tokio::spawn(Channel::from_io(client, "localhost"));
    let mut preface = [0; 24];
    peer.read_exact(&mut preface).await.expect("preface");
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    assert_eq!(frame(&mut peer).await.kind, 4);
    settings(&mut peer, 1).await;
    let channel = connect.await.expect("connect task").expect("channel");

    // Keep the first stream open on the peer while the client has sent EOF.
    let first = tokio::spawn(unary(&channel, None));
    loop {
        let received = frame(&mut peer).await;
        eprintln!("before deadline: {received:?}");
        if received.kind == 0 && received.stream == 1 && received.flags & 1 != 0 {
            assert!(!received.payload.is_empty(), "first request payload");
            break;
        }
    }

    let expired = unary(&channel, Some(Duration::from_millis(50))).await;
    assert_eq!(
        expired.expect_err("slot wait expires").code(),
        Code::DeadlineExceeded
    );

    // A SETTINGS increase deterministically releases the h2 queue without
    // allowing the first call to finish or relying on scheduler contention.
    settings(&mut peer, 2).await;
    let mut received = Vec::new();
    while let Ok(next) = tokio::time::timeout(Duration::from_millis(30), frame(&mut peer)).await {
        eprintln!("after deadline: {next:?}");
        received.push(next);
    }
    first.abort();
    assert!(
        received.iter().all(|next| next.stream == 0),
        "expired RPC emitted stream frames after capacity became available: {received:?}"
    );
}
