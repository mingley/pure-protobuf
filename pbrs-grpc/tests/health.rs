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

use pbrs_grpc::health::{
    service, Health, HealthCheckRequest, HealthCheckResponse, HealthClient, HealthServer,
    ServingStatus,
};
use pbrs_grpc::{
    Channel, ClientTls, Code, Identity, Outgoing, Request, Response, Router, ServerTls, Status,
};
use std::net::SocketAddr;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");
const CLIENT_CERT: &str = include_str!("tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("tls_data/client.key");

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

fn client_identity() -> Identity {
    Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity")
}

async fn tls_client_with(addr: SocketAddr, client_tls: ClientTls) -> HealthClient {
    let mut last = None;
    for _ in 0..80 {
        match HealthClient::connect_tls(addr, client_tls.clone()).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn tls_client(addr: SocketAddr) -> HealthClient {
    tls_client_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await
}

#[cfg(unix)]
async fn unix_client(path: &std::path::Path) -> HealthClient {
    let mut last = None;
    for _ in 0..80 {
        match HealthClient::connect_unix(path).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

#[cfg(unix)]
fn unix_sock(prefix: &str) -> std::path::PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-health-{prefix}-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    path
}

async fn echo_health_check_and_watch(client: &HealthClient) {
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
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);
}

async fn gzip_health_check_and_watch(client: &HealthClient) {
    let overall = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect("overall");
    assert!(overall.compressed(), "check gzip");
    assert_eq!(overall.encoding(), Some("gzip"), "{:?}", overall.encoding());
    assert_eq!(overall.get_ref().status(), ServingStatus::Serving);

    let named = client
        .check(Request::new(req("helloworld.Greeter")))
        .await
        .expect("named");
    assert!(named.compressed(), "named check gzip");
    assert_eq!(named.encoding(), Some("gzip"));
    assert_eq!(named.get_ref().status(), ServingStatus::Serving);

    let reply = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch");
    assert_eq!(reply.encoding(), Some("gzip"), "watch encoding");
    let mut stream = reply.into_inner();
    let framed = stream.next_framed().await.expect("frame").expect("msg");
    assert!(framed.compressed, "watch frames gzip");
    assert_eq!(framed.message.status(), ServingStatus::Serving);
}

async fn assert_health_blocked(client: &HealthClient) {
    assert_interceptor_blocked(
        &client
            .check(Request::new(HealthCheckRequest::new()))
            .await
            .expect_err("check"),
    );
    match client.watch(Request::new(HealthCheckRequest::new())).await {
        Err(err) => assert_interceptor_blocked(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_interceptor_blocked(&err),
            Ok(_) => panic!("Watch interceptor reject must fail"),
        },
    }
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

fn stamp_outgoing_context(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let path = call.path();
    call.metadata_mut().insert("x-path", path)?;
    let service = call.service();
    call.metadata_mut().set("x-service", service)?;
    let method = call.method();
    call.metadata_mut().set("x-method", method)?;
    let authority = call.authority();
    call.metadata_mut().insert("x-authority", authority)?;
    let scheme = call.scheme();
    call.metadata_mut().set("x-scheme", scheme)?;
    Ok(())
}

fn require_stamped_context(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-path") != Some(rpc.path()) {
        return Err(Status::invalid_argument(format!(
            "x-path {:?} path {}",
            rpc.metadata().get("x-path"),
            rpc.path()
        )));
    }
    if rpc.metadata().get("x-service") != Some(rpc.service()) {
        return Err(Status::invalid_argument(format!(
            "x-service {:?} service {}",
            rpc.metadata().get("x-service"),
            rpc.service()
        )));
    }
    if rpc.metadata().get("x-method") != Some(rpc.method()) {
        return Err(Status::invalid_argument(format!(
            "x-method {:?} method {}",
            rpc.metadata().get("x-method"),
            rpc.method()
        )));
    }
    if rpc.metadata().get("x-authority") != rpc.authority() {
        return Err(Status::invalid_argument(format!(
            "x-authority {:?} authority {:?}",
            rpc.metadata().get("x-authority"),
            rpc.authority()
        )));
    }
    if rpc.metadata().get("x-scheme") != rpc.scheme() {
        return Err(Status::invalid_argument(format!(
            "x-scheme {:?} scheme {:?}",
            rpc.metadata().get("x-scheme"),
            rpc.scheme()
        )));
    }
    Ok(())
}

fn interceptor_blocked() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("BLOCKED");
    info.set_domain("example.com");
    Status::with_error_details(
        Code::FailedPrecondition,
        "blocked locally",
        [pbrs_grpc::pb::Any::pack(&info).expect("pack")],
    )
    .expect("details")
}

fn assert_interceptor_blocked(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "blocked locally");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "BLOCKED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let unpacked = err
        .error_details()
        .expect("ErrorDetails")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(unpacked.reason().to_str().unwrap_or(""), "BLOCKED");
    assert_eq!(unpacked.domain().to_str().unwrap_or(""), "example.com");
}

struct FailHealth;

impl Health for FailHealth {
    async fn check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Err(interceptor_blocked())
    }

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HealthCheckResponse>>, Status> {
        Err(interceptor_blocked())
    }
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
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = client(addr).await;
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_client_interceptor_rejects_check_and_watch() {
    let (addr, _reporter, handle) = serve().await;
    let client = client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_client_interceptor_sees_check_and_watch_context() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_context)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = client(addr).await.intercept(stamp_outgoing_context);
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_io_round_trips_check_and_watch() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_io_send_compressed_gzips_check_and_watch() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .send_compressed();
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_io_interceptor_rejects_check_and_watch() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("");
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_io_client_interceptor_rejects_check_and_watch() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_io_client_interceptor_sees_check_and_watch_context() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_context)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(stamp_outgoing_context);
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_round_trips_check_and_watch() {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-health-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match HealthClient::connect_unix(&path).await {
                Ok(client) => {
                    found = Some(client);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("could not connect: {last:?}"))
    };
    echo_health_check_and_watch(&client).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_send_compressed_gzips_check_and_watch() {
    let path = unix_sock("gzip");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_unix(sock).await.ok();
    });
    let client = unix_client(&path).await.send_compressed();
    gzip_health_check_and_watch(&client).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_interceptor_rejects_check_and_watch() {
    let path = unix_sock("reject");
    let (svc, reporter) = service();
    reporter.set_serving("");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_health_blocked(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_client_interceptor_rejects_check_and_watch() {
    let path = unix_sock("client-reject");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    let client = unix_client(&path)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_health_blocked(&client).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_client_interceptor_sees_check_and_watch_context() {
    let path = unix_sock("context");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_context)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = unix_client(&path).await.intercept(stamp_outgoing_context);
    echo_health_check_and_watch(&client).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_tls_round_trips_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match HealthClient::connect_tls(addr, client_tls.clone()).await {
                Ok(client) => {
                    found = Some(client);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("could not connect: {last:?}"))
    };
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_send_compressed_gzips_check_and_watch() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_listener(listener).await.ok();
    });
    let client = client(addr).await.send_compressed();
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_send_compressed_gzips_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match HealthClient::connect_tls(addr, client_tls.clone()).await {
                Ok(client) => {
                    found = Some(client);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("could not connect: {last:?}"))
    }
    .send_compressed();
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_send_compressed_gzips_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls).await.send_compressed();
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_interceptor_rejects_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("");
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = tls_client(addr).await;
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_client_interceptor_rejects_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = tls_client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_client_interceptor_sees_check_and_watch_context() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = tls_client(addr).await.intercept(stamp_outgoing_context);
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_interceptor_rejects_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("");
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_blocked(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_client_interceptor_rejects_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_health_blocked(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_client_interceptor_sees_check_and_watch_context() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls)
        .await
        .intercept(stamp_outgoing_context);
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn health_handlers_return_typed_status_on_check_and_watch() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealth)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health_blocked(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_handlers_return_typed_status_on_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealth)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_blocked(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_handlers_return_typed_status_on_check_and_watch() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealth)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_blocked(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_handlers_return_typed_status_on_check_and_watch() {
    let path = unix_sock("typed-handler");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealth).serve_unix(sock).await.ok();
    });
    assert_health_blocked(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_handlers_return_typed_status_on_check_and_watch() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealth)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_blocked(&client).await;
    handle.abort();
}
