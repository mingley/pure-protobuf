//! Tower client adapter coverage.

#![cfg(feature = "tower")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use common::{Echo, name_of, req, serve, until_ok};
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Request, ServerConfig};
use std::time::Duration;
use tower::{ServiceBuilder, ServiceExt};

#[tokio::test]
async fn tower_layers_wrap_unary_channel_without_buffering_default_path() {
    let (addr, _guard) = serve(Echo, ServerConfig::default()).await.expect("server");
    let channel = until_ok("connect", || async { Channel::connect(addr).await }).await;
    let service = channel.tower_unary::<HelloRequest, HelloReply>("/helloworld.Greeter/SayHello");
    let layered = ServiceBuilder::new()
        .timeout(Duration::from_secs(5))
        .concurrency_limit(1)
        .rate_limit(10, Duration::from_secs(1))
        .buffer(1)
        .load_shed()
        .service(service);

    let response = layered
        .oneshot(Request::new(req("tower")))
        .await
        .expect("tower call")
        .into_inner();
    assert_eq!(name_of(&response), "tower");
}
