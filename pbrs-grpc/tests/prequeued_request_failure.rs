//! A failed local upload need not be admitted by the remote service.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "independent HTTP/2 peer and regression assertions"
)]

use pbrs_grpc::{Channel, Code, GreeterClient, HelloRequest, Request, Status};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prequeued_failure_finishes_before_the_peer_can_admit_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let admitted = Arc::new(AtomicUsize::new(0));
    let peer_admitted = Arc::clone(&admitted);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("TCP accept");
        let mut connection = h2::server::handshake(socket)
            .await
            .expect("independent HTTP/2 handshake");
        ready_tx.send(()).expect("peer ready");
        // No request acceptance or connection polling until both local Calls
        // have returned. The peer cannot start a handler or cancellation child.
        release_rx.await.expect("release request-admission gate");
        if let Some(Ok((_request, _respond))) = connection.accept().await {
            peer_admitted.fetch_add(1, Ordering::Release);
        }
    });
    let socket = TcpStream::connect(address).await.expect("connect");
    let channel = Channel::from_io(socket, "localhost")
        .await
        .expect("native channel");
    ready_rx.await.expect("peer handshake finished");
    let client = GreeterClient::new(channel);
    let mut message = HelloRequest::new();
    message.set_name("ada");

    let (sender, call) = client.client_hello(Request::new(()));
    sender.send(message.clone()).await.expect("queue request");
    sender.fail(Status::new(Code::NotFound, "gone")).await;
    let status = tokio::time::timeout(Duration::from_millis(800), call)
        .await
        .expect("local failure must not wait for request admission")
        .expect_err("client-stream local failure");
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(status.message(), "gone");

    let (sender, call) = client.stream_hello(Request::new(()));
    sender.send(message).await.expect("queue request");
    sender.fail(Status::new(Code::NotFound, "gone")).await;
    let status = tokio::time::timeout(Duration::from_millis(800), call)
        .await
        .expect("local failure must not wait for request admission")
        .expect_err("bidi local failure");
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(status.message(), "gone");
    assert_eq!(admitted.load(Ordering::Acquire), 0);

    // Release only after recording zero admissions, then close the connection
    // so the independent peer can finish even when reset streams are discarded.
    release_tx.send(()).expect("release peer");
    drop(client);
    tokio::time::timeout(Duration::from_millis(800), peer)
        .await
        .expect("peer shutdown")
        .expect("peer task");
}
