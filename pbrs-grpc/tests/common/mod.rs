#![allow(
    dead_code,
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
    missing_docs,
    reason = "integration test helpers"
)]

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer};
use pbrs_grpc::{Channel, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

pub async fn spawn_greeter<G: Greeter>(g: G) -> Result<(SocketAddr, GreeterClient), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        GreeterServer::new(g).serve_listener(listener).await.ok();
    }));
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(ch) => return Ok((addr, GreeterClient::new(ch))),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

pub fn name_of(reply: &pbrs_grpc::HelloReply) -> String {
    reply.message().to_str().unwrap_or("").to_string()
}

pub fn req(name: &str) -> pbrs_grpc::HelloRequest {
    let mut r = pbrs_grpc::HelloRequest::new();
    r.set_name(name);
    r
}
