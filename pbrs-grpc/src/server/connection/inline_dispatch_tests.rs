//! A handler parked on the connection task must not block the next RPC.
#![allow(
    clippy::expect_used,
    reason = "the test fails closed when a parked handler stalls accept"
)]

use crate::{Channel, HelloReply, HelloRequest, Request, Response, Rpc, Server, Service, Status};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

struct Park {
    entered: watch::Sender<bool>,
    release: watch::Receiver<bool>,
}

impl Service for Park {
    const NAME: &'static str = "park.Park";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Block" => {
                let mut release = self.release.clone();
                self.entered.send(true).ok();
                rpc.unary(|_req: Request<HelloRequest>| async move {
                    release.wait_for(|ready| *ready).await.ok();
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await;
            }
            _ => {
                rpc.unary(|_req: Request<HelloRequest>| async move {
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parked_handler_does_not_block_the_next_unary() {
    let (entered_tx, mut entered_rx) = watch::channel(false);
    let (release_tx, release_rx) = watch::channel(false);
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        Server::new(Park {
            entered: entered_tx,
            release: release_rx,
        })
        .serve_connection(server_io)
        .await
        .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("handshake");
    let channel = Arc::new(channel);
    let blocked = channel
        .unary::<HelloRequest, HelloReply>("/park.Park/Block", Request::new(HelloRequest::new()));
    let block_task = tokio::spawn(blocked);
    tokio::time::timeout(Duration::from_secs(2), entered_rx.wait_for(|ready| *ready))
        .await
        .expect("blocked handler should start")
        .expect("entered watch");
    let passed = tokio::time::timeout(
        Duration::from_secs(2),
        channel.unary::<HelloRequest, HelloReply>(
            "/park.Park/Pass",
            Request::new(HelloRequest::new()),
        ),
    )
    .await
    .expect("a parked handler stalled the next RPC on the same connection")
    .expect("pass");
    drop(passed.into_inner());
    release_tx.send(true).ok();
    tokio::time::timeout(Duration::from_secs(2), block_task)
        .await
        .expect("blocked rpc did not finish")
        .expect("block task")
        .expect("block status");
    server.abort();
}
