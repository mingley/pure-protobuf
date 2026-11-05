//! Public server lifetime options must not overflow the monotonic clock.

#![allow(
    clippy::expect_used,
    reason = "bounded public-API protocol regressions"
)]

use pbrs_grpc::{
    Channel, HelloReply, HelloRequest, Request, Response, Rpc, Server, ServerConfig, Service,
    Status,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};

const BOUND: Duration = Duration::from_secs(2);
const PATH: &str = "/timeout.Limits/Unary";

struct Gate {
    entered: Notify,
    release: Semaphore,
}

struct Echo(Option<Arc<Gate>>);

impl Service for Echo {
    const NAME: &'static str = "timeout.Limits";

    async fn call(&self, rpc: Rpc) {
        let gate = self.0.clone();
        rpc.unary(|_request: Request<HelloRequest>| async move {
            if let Some(gate) = gate {
                gate.entered.notify_one();
                drop(gate.release.acquire().await.expect("test gate is open"));
            }
            let mut reply = HelloReply::new();
            reply.set_message("ok");
            Ok::<_, Status>(Response::new(reply))
        })
        .await;
    }
}

#[tokio::test(start_paused = true)]
async fn unrepresentable_age_and_idle_budgets_keep_the_server_connection_usable() {
    for config in [
        ServerConfig::new().max_connection_age(Duration::MAX),
        ServerConfig::new().max_connection_idle(Duration::MAX),
        ServerConfig::new()
            .max_connection_age(Duration::MAX)
            .max_connection_idle(Duration::MAX),
    ] {
        let (peer, server_io) = tokio::io::duplex(65_536);
        let server = tokio::spawn(async move {
            Server::new(Echo(None))
                .config(config)
                .serve_connection(server_io)
                .await
        });
        let channel = Channel::from_io(peer, "localhost")
            .await
            .expect("native channel");
        for _ in 0..2 {
            let response = tokio::time::timeout(
                BOUND,
                channel.unary::<_, HelloReply>(PATH, Request::new(HelloRequest::new())),
            )
            .await
            .expect("RPC remained bounded")
            .expect("server lifetime did not panic or close the connection");
            assert_eq!(response.get_ref().message(), "ok");
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert!(!server.is_finished(), "connection remains usable");
        drop(channel);
        tokio::time::timeout(BOUND, server)
            .await
            .expect("connection closes after channel drop")
            .expect("connection task did not panic")
            .expect("connection ended cleanly");
    }
}

#[tokio::test]
async fn unrepresentable_age_grace_allows_an_inflight_response_after_goaway() {
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        release: Semaphore::new(0),
    });
    let (peer, server_io) = tokio::io::duplex(65_536);
    let service_gate = Arc::clone(&gate);
    let server = tokio::spawn(async move {
        Server::new(Echo(Some(service_gate)))
            .config(
                ServerConfig::new()
                    .max_connection_age(Duration::from_millis(10))
                    .max_connection_age_grace(Duration::MAX),
            )
            .serve_connection(server_io)
            .await
    });
    let (mut requests, connection) = h2::client::handshake(peer)
        .await
        .expect("independent h2 handshake");
    let driver = tokio::spawn(connection);
    let request = http::Request::builder()
        .method(http::Method::POST)
        .uri("http://localhost/timeout.Limits/Unary")
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(())
        .expect("request");
    let (response, mut upload) = requests.send_request(request, false).expect("start RPC");
    upload
        .send_data(bytes::Bytes::from_static(b"\0\0\0\0\0"), true)
        .expect("empty protobuf request");
    tokio::time::timeout(BOUND, gate.entered.notified())
        .await
        .expect("handler admitted");
    // The independent peer observes the actual GOAWAY before allowing the
    // handler to finish; merely completing before the age timer is insufficient.
    tokio::time::timeout(BOUND, async move {
        while let Ok(ready) = requests.ready().await {
            requests = ready;
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("server sent GOAWAY");
    assert!(!server.is_finished(), "grace must retain the in-flight RPC");
    gate.release.add_permits(1);
    let mut body = tokio::time::timeout(BOUND, response)
        .await
        .expect("response after GOAWAY")
        .expect("in-flight stream remains valid")
        .into_body();
    let trailers = tokio::time::timeout(BOUND, async {
        while let Some(data) = body.data().await {
            let data = data.expect("valid response DATA");
            body.flow_control()
                .release_capacity(data.len())
                .expect("release response capacity");
        }
        body.trailers().await
    })
    .await
    .expect("response completed within grace")
    .expect("response trailers")
    .expect("gRPC trailers present");
    assert_eq!(trailers.get("grpc-status").expect("gRPC status"), "0");
    drop((body, upload));
    tokio::time::timeout(BOUND, server)
        .await
        .expect("server drained after response")
        .expect("server connection did not panic")
        .expect("server connection completed");
    tokio::time::timeout(BOUND, driver)
        .await
        .expect("peer connection closed")
        .expect("peer driver did not panic")
        .expect("peer connection completed");
}
