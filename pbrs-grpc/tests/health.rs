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

#[test]
fn reporter_status_round_trips_without_an_rpc() {
    let (_, reporter) = service();
    assert_eq!(reporter.status(""), Some(ServingStatus::Serving));
    assert_eq!(reporter.status("helloworld.Greeter"), None);
    reporter.set_serving("helloworld.Greeter");
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::Serving)
    );
    reporter.set_not_serving("helloworld.Greeter");
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::NotServing)
    );
    reporter.clear("helloworld.Greeter");
    assert_eq!(reporter.status("helloworld.Greeter"), None);
    reporter.clear("");
    assert_eq!(reporter.status(""), None);
    assert_eq!(reporter.watchers(), 0);
}

#[test]
fn reporter_names_lists_known_services() {
    let (_, reporter) = service();
    assert_eq!(reporter.names(), vec![String::new()]);
    reporter.set_serving("helloworld.Greeter");
    reporter.set_not_serving("demo.Other");
    assert_eq!(
        reporter.names(),
        vec![
            String::new(),
            "demo.Other".to_owned(),
            "helloworld.Greeter".to_owned()
        ]
    );
    reporter.clear("demo.Other");
    assert_eq!(
        reporter.names(),
        vec![String::new(), "helloworld.Greeter".to_owned()]
    );
    reporter.shutdown();
    assert_eq!(
        reporter.names(),
        vec![String::new(), "helloworld.Greeter".to_owned()]
    );
    reporter.clear("helloworld.Greeter");
    reporter.clear("");
    assert!(reporter.names().is_empty());
}

#[test]
fn shutdown_marks_known_names_not_serving() {
    let (_, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    reporter.shutdown();
    assert_eq!(reporter.status(""), Some(ServingStatus::NotServing));
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::NotServing)
    );
    assert_eq!(reporter.status("no.Such"), None);
    reporter.clear("helloworld.Greeter");
    reporter.shutdown();
    assert_eq!(reporter.status("helloworld.Greeter"), None);
    reporter.set_serving("helloworld.Greeter");
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::Serving)
    );
}

#[test]
fn resume_marks_known_names_serving() {
    let (_, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    reporter.shutdown();
    reporter.resume();
    assert_eq!(reporter.status(""), Some(ServingStatus::Serving));
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::Serving)
    );
    assert_eq!(reporter.status("no.Such"), None);
    assert_eq!(
        reporter.names(),
        vec![String::new(), "helloworld.Greeter".to_owned()]
    );
    reporter.set_not_serving("helloworld.Greeter");
    assert_eq!(
        reporter.status("helloworld.Greeter"),
        Some(ServingStatus::NotServing)
    );
    assert_eq!(reporter.status(""), Some(ServingStatus::Serving));
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

#[tokio::test]
async fn dropping_a_watch_releases_the_subscription_without_a_status_change() {
    let (addr, reporter, _handle) = serve().await;
    let client = client(addr).await;
    assert_eq!(reporter.watchers(), 0);
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);
    assert!(
        reporter.watchers() >= 1,
        "Watch must hold a subscription while the stream is live"
    );
    drop(stream);
    for _ in 0..80 {
        if reporter.watchers() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        reporter.watchers(),
        0,
        "Watch must not wait for the next status change after the client leaves"
    );
}

#[tokio::test]
async fn shutdown_is_visible_to_check_and_watch() {
    let (addr, reporter, _handle) = serve().await;
    let client = client(addr).await;
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);

    reporter.shutdown();
    let second = tokio::time::timeout(Duration::from_secs(2), stream.message())
        .await
        .expect("timeout")
        .expect("second")
        .expect("msg");
    assert_eq!(second.status(), ServingStatus::NotServing);

    let overall = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect("overall")
        .into_inner();
    assert_eq!(overall.status(), ServingStatus::NotServing);
    let named = client
        .check(Request::new(req("helloworld.Greeter")))
        .await
        .expect("named")
        .into_inner();
    assert_eq!(named.status(), ServingStatus::NotServing);
    let missing = client
        .check(Request::new(req("no.Such")))
        .await
        .expect_err("unknown stays not found");
    assert_eq!(missing.code(), Code::NotFound, "{missing}");

    reporter.resume();
    let third = tokio::time::timeout(Duration::from_secs(2), stream.message())
        .await
        .expect("timeout")
        .expect("third")
        .expect("msg");
    assert_eq!(third.status(), ServingStatus::Serving);
    let overall = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect("overall after resume")
        .into_inner();
    assert_eq!(overall.status(), ServingStatus::Serving);
    let named = client
        .check(Request::new(req("helloworld.Greeter")))
        .await
        .expect("named after resume")
        .into_inner();
    assert_eq!(named.status(), ServingStatus::Serving);
    let missing = client
        .check(Request::new(req("no.Such")))
        .await
        .expect_err("unknown still not found");
    assert_eq!(missing.code(), Code::NotFound, "{missing}");
}

#[tokio::test]
async fn oversize_health_request_is_resource_exhausted() {
    let (svc, reporter) = service();
    reporter.set_serving("");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .max_decoding_message_size(16)
            .add_service(svc)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = client(addr).await;
    let mut fat = HealthCheckRequest::new();
    fat.set_service("k".repeat(64));
    let err = client
        .check(Request::new(fat.clone()))
        .await
        .expect_err("check");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match client.watch(Request::new(fat)).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("oversize Watch must fail as trailers"),
        },
    }
    handle.abort();
}

#[tokio::test]
async fn health_interceptor_rejects_check_and_watch() {
    let (svc, reporter) = service();
    reporter.set_serving("");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(Status::unauthenticated("nope")))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = client(addr).await;
    let err = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect_err("check");
    assert_eq!(err.code(), Code::Unauthenticated, "{err}");
    match client.watch(Request::new(HealthCheckRequest::new())).await {
        Err(err) => assert_eq!(err.code(), Code::Unauthenticated, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::Unauthenticated, "{err}"),
            Ok(_) => panic!("Watch interceptor reject must fail"),
        },
    }
    handle.abort();
}
