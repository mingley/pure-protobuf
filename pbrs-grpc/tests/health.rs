//! Standard `grpc.health.v1.Health` Check and Watch.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unreachable_pub,
    missing_docs,
    reason = "integration tests"
)]

use pbrs_grpc::health::{service, HealthCheckRequest, HealthClient, ServingStatus};
use pbrs_grpc::{Channel, Code, Request, Router, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

async fn serve() -> (
    SocketAddr,
    pbrs_grpc::health::HealthReporter,
    tokio::task::JoinHandle<()>,
) {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(svc)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, reporter, handle)
}

async fn client(addr: SocketAddr) -> HealthClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return HealthClient::new(channel),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("connect {addr}: {last}");
}

fn req(name: &str) -> HealthCheckRequest {
    let mut r = HealthCheckRequest::new();
    r.set_service(name);
    r
}

#[tokio::test]
async fn check_overall_and_named() {
    let (addr, _reporter, _handle) = serve().await;
    let client = client(addr).await;

    let overall = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect("overall")
        .into_inner();
    assert_eq!(overall.status(), ServingStatus::Serving);

    let named = client
        .check(Request::new(req("helloworld.Greeter")))
        .await
        .expect("named")
        .into_inner();
    assert_eq!(named.status(), ServingStatus::Serving);

    let err = client
        .check(Request::new(req("no.Such")))
        .await
        .expect_err("unknown");
    assert_eq!(err.code(), Code::NotFound, "{err}");
}

#[tokio::test]
async fn watch_sees_status_changes() {
    let (addr, reporter, _handle) = serve().await;
    let client = client(addr).await;
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);

    reporter.set_not_serving("");
    let second = tokio::time::timeout(Duration::from_secs(2), stream.message())
        .await
        .expect("timeout")
        .expect("second")
        .expect("msg");
    assert_eq!(second.status(), ServingStatus::NotServing);
}

#[tokio::test]
async fn watch_unknown_is_service_unknown() {
    let (addr, _reporter, _handle) = serve().await;
    let client = client(addr).await;
    let mut stream = client
        .watch(Request::new(req("no.Such")))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::ServiceUnknown);
}
