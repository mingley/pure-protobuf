//! Standard `grpc.health.v1.Health` Check, List, and Watch.

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

mod common;

use common::reserve_loopback;
use pbrs_grpc::health::{
    service, Health, HealthCheckRequest, HealthCheckResponse, HealthClient, HealthListRequest,
    HealthListResponse, HealthReporter, HealthServer, ServingStatus,
};
use pbrs_grpc::{
    Channel, ChannelConfig, ClientTls, Code, Identity, MessageLimits, Outgoing, Request, Response,
    Router, ServerTls, Status,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

struct ServeGuard(tokio::task::JoinHandle<()>);

impl Drop for ServeGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn bind_health() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

fn health_server() -> HealthServer<impl Health> {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    svc
}

fn serve_health_on(listener: TcpListener) -> ServeGuard {
    let svc = health_server();
    ServeGuard(tokio::spawn(async move {
        svc.serve_listener(listener).await.ok();
    }))
}

fn serve_health_tls_on(listener: TcpListener, tls: ServerTls) -> ServeGuard {
    let svc = health_server();
    ServeGuard(tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    }))
}

fn stamp_wait_ready<T>(
    mut request: Request<T>,
    wait_on_request: bool,
    timeout: Option<Duration>,
) -> Request<T> {
    if wait_on_request {
        request.set_wait_for_ready(true);
    }
    if let Some(timeout) = timeout {
        request.set_timeout(timeout);
    }
    request
}

fn stamp_opt_out<T>(mut request: Request<T>) -> Request<T> {
    request.set_wait_for_ready(false);
    request.set_timeout(Duration::from_secs(5));
    request
}

fn stamp_wait_deadline<T>(mut request: Request<T>, timeout: Duration) -> Request<T> {
    request.set_wait_for_ready(true);
    request.set_timeout(timeout);
    request
}

async fn assert_deadline_in<F, T>(call: F, min_elapsed: Duration, max_elapsed: Duration)
where
    F: std::future::Future<Output = Result<T, Status>>,
{
    let started = Instant::now();
    let err = match call.await {
        Ok(_) => panic!("expected deadline"),
        Err(status) => status,
    };
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() >= min_elapsed,
        "deadline returned too fast: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < max_elapsed,
        "deadline too slow: {:?}",
        started.elapsed()
    );
}

async fn assert_health_opt_out(client: &HealthClient) {
    let err = client
        .check(stamp_opt_out(Request::new(HealthCheckRequest::new())))
        .await
        .expect_err("check");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .watch(stamp_opt_out(Request::new(HealthCheckRequest::new())))
        .await
        .expect_err("watch");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .list(stamp_opt_out(Request::new(HealthListRequest::new())))
        .await
        .expect_err("list");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

async fn assert_health_unavailable(client: &HealthClient) {
    let err = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect_err("check");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect_err("watch");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect_err("list");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

async fn assert_health_wait_deadline(client: &HealthClient) {
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.check(stamp_wait_deadline(
            Request::new(HealthCheckRequest::new()),
            timeout,
        )),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.watch(stamp_wait_deadline(
            Request::new(HealthCheckRequest::new()),
            timeout,
        )),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.list(stamp_wait_deadline(
            Request::new(HealthListRequest::new()),
            timeout,
        )),
        min,
        max,
    )
    .await;
}

async fn wait_then_complete_health(
    client: &HealthClient,
    wait_on_request: bool,
    start: impl std::future::Future,
) {
    let timeout = Some(Duration::from_secs(5));
    let mut check = client.check(stamp_wait_ready(
        Request::new(HealthCheckRequest::new()),
        wait_on_request,
        timeout,
    ));
    let mut watch = client.watch(stamp_wait_ready(
        Request::new(HealthCheckRequest::new()),
        wait_on_request,
        timeout,
    ));
    let mut list = client.list(stamp_wait_ready(
        Request::new(HealthListRequest::new()),
        wait_on_request,
        timeout,
    ));

    tokio::select! {
        biased;
        result = &mut check => panic!("check finished before the server listened: {result:?}"),
        result = &mut watch => panic!("watch finished before the server listened: {result:?}"),
        result = &mut list => panic!("list finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = start.await;

    let overall = tokio::time::timeout(Duration::from_secs(2), check)
        .await
        .expect("check hung after listen")
        .expect("check")
        .into_inner();
    assert_eq!(overall.status(), ServingStatus::Serving);

    let mut stream = tokio::time::timeout(Duration::from_secs(2), watch)
        .await
        .expect("watch hung after listen")
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);

    let listed = tokio::time::timeout(Duration::from_secs(2), list)
        .await
        .expect("list hung after listen")
        .expect("list")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);
}

fn assert_listed_known(resp: &HealthListResponse, greeter: ServingStatus, overall: ServingStatus) {
    let statuses = resp.statuses();
    assert_eq!(
        statuses.get("").map(|s| s.status()),
        Some(overall),
        "List must include the process name"
    );
    assert_eq!(
        statuses.get("helloworld.Greeter").map(|s| s.status()),
        Some(greeter),
        "List must include names that have been set"
    );
    assert!(
        statuses.get("no.Such").is_none(),
        "List must omit unknown names"
    );
    assert_eq!(statuses.len(), 2, "List is only known names");
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
    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);
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

    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list");
    assert!(listed.compressed(), "list gzip");
    assert_eq!(listed.encoding(), Some("gzip"));
    assert_listed_known(
        listed.get_ref(),
        ServingStatus::Serving,
        ServingStatus::Serving,
    );

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
    assert_interceptor_blocked(
        &client
            .list(Request::new(HealthListRequest::new()))
            .await
            .expect_err("list"),
    );
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

#[test]
fn health_crate_docs_name_interceptor_wait_for_ready() {
    let src = include_str!("../src/health.rs");
    assert!(
        src.contains("wait-for-ready is set on the request, the client, or a client interceptor."),
        "Health crate rustdoc must name interceptor-set wait-for-ready"
    );
    assert!(
        src.contains(
            "`Request::set_wait_for_ready(false)` and a client interceptor\n//! `set_wait_for_ready(false)` opt out of a client default. A waiting Call's\n//! deadline applies on those dialers."
        ),
        "Health crate rustdoc must name wait-for-ready opt-out and deadline"
    );
    assert!(
        src.contains(
            "Watch\n//! [`crate::StreamSender::fail`] after a streamed DATA frame ships those\n//! trailers the same way (Check is unary: no response DATA then trailers)."
        ),
        "Health crate rustdoc must name Watch typed Status after streamed DATA"
    );
    assert!(
        src.contains(
            "Check of a never-set name is [`crate::Code::NotFound`]. Watch\n//! of that name streams [`ServingStatus::ServiceUnknown`]. Watch streams later\n//! `set_not_serving` / [`HealthReporter::shutdown`] / [`HealthReporter::resume`]\n//! changes, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Health crate rustdoc must name Check/Watch protocol on every transport"
    );
    assert!(
        src.contains(
            "Dropping a Watch releases the subscription without waiting for a status\n//! change on those transports."
        ),
        "Health crate rustdoc must name Watch drop on every transport"
    );
    assert!(
        src.contains(
            "Check, List, and Watch are the proto methods. List is a snapshot of every\n//! known name (the process `\"\"` and names you set); unknown names are omitted,\n//! matching [`HealthReporter::names`]."
        ),
        "Health crate rustdoc must name List as a snapshot of known names"
    );
    assert!(
        src.contains(
            "decoding cap is `RESOURCE_EXHAUSTED` on both, including\n//! over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Health crate rustdoc must name oversize RESOURCE_EXHAUSTED on every transport"
    );
    assert!(
        src.contains(
            "A [`HealthClient`]\n//! `max_encoding_message_size` / `max_decoding_message_size` is\n//! `RESOURCE_EXHAUSTED` on Check and Watch on those transports, distinct from\n//! the server decoding cap."
        ),
        "Health crate rustdoc must name client message caps on every transport"
    );
    assert!(
        src.contains(
            "[`HealthClient::message_limits`] refuses the same\n//! oversize, distinct from those single-cap wrappers."
        ),
        "Health crate rustdoc must name wrap message_limits on every transport"
    );
    assert!(
        src.contains(
            "`Router::message_limits` /\n//! [`HealthServer::message_limits`] refuse the same oversize as\n//! `RESOURCE_EXHAUSTED` on both, distinct from\n//! [`crate::Router::max_decoding_message_size`]."
        ),
        "Health crate rustdoc must name combined-setter oversize on every transport"
    );
    assert!(
        src.contains(
            "[`HealthClient::connect_tls_with`] / [`HealthClient::connect_unix_with`] /\n//! [`HealthClient::from_io_with`] with [`crate::ChannelConfig::message_limits`]\n//! refuse the same oversize, distinct from wrapping a live client."
        ),
        "Health crate rustdoc must name dial-time ChannelConfig message_limits on every transport"
    );
    assert!(
        src.contains(
            "[`HealthServer::max_header_list_size`] refuses oversize metadata on Check\n//! and Watch, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter\n//! server."
        ),
        "Health crate rustdoc must name header-list flood on Check and Watch"
    );
    assert!(
        src.contains(
            "[`HealthServer::max_frame_size`] still serves Check, List, and Watch at\n//! the HTTP/2 16 KiB SETTINGS minimum, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter\n//! server."
        ),
        "Health crate rustdoc must name max_frame_size still-serves on Check, List, and Watch"
    );
    assert!(
        src.contains(
            "[`HealthServer::max_pending_accept_reset_streams`] still serves Check,\n//! List, and Watch at a pending-reset cap of 1, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. A well-behaved client never fills that\n//! queue. Distinct from wrapping only a Greeter server."
        ),
        "Health crate rustdoc must name pending-reset still-serves on Check, List, and Watch"
    );
    assert!(
        src.contains(
            "[`HealthServer::max_send_buffer_size`] still serves Check, List, and Watch at a\n//! 16 KiB send buffer, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter\n//! server."
        ),
        "Health crate rustdoc must name send-buffer still-serves on Check, List, and Watch"
    );
    assert!(
        src.contains(
            "[`HealthServer::initial_stream_window_size`] /\n//! [`HealthServer::initial_connection_window_size`] still serve Check, List, and Watch\n//! at a 64 KiB stream / 128 KiB connection window, including over TLS, mTLS,\n//! Unix, and [`crate::Server::serve_connection`]. Distinct from wrapping only a\n//! Greeter server."
        ),
        "Health crate rustdoc must name HTTP/2 window still-serves on Check, List, and Watch"
    );
    assert!(
        src.contains(
            "A [`HealthClient`] pool larger than\n//! [`HealthServer::max_concurrent_connections`] fails the whole dial as\n//! `UNAVAILABLE` on TLS, mTLS, and Unix. [`HealthClient::from_io_with`]\n//! cannot pool."
        ),
        "Health crate rustdoc must name pool-vs-cap UNAVAILABLE on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "path / service / method / `:authority` / `:scheme` on Check, List, and Watch."
        ),
        "Health crate rustdoc must name client interceptor context on Check, List, and Watch"
    );
    assert!(
        src.contains("retry Check, List, and Watch until listen when"),
        "Health crate rustdoc must name wait-for-ready on Check, List, and Watch"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this health interceptor Err; those trailers reach the client without reading the body."
        ),
        "Health crate rustdoc must name from_error_details typed bag next to interceptor Err"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this health handler Err; those trailers reach the client."
        ),
        "Health crate rustdoc must name from_error_details typed bag next to handler Err"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this health client interceptor Err; a local reject never opens a stream."
        ),
        "Health crate rustdoc must name from_error_details typed bag next to client interceptor Err"
    );
    assert!(
        src.contains(
            "Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this health client interceptor already ran, so a local Err never consumes that budget."
        ),
        "Health crate rustdoc must Distinct max_concurrent_rpcs slot grab after client intercept"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this health StreamSender fail on a server response producer; those trailers ship after any messages already sent."
        ),
        "Health crate rustdoc must name from_error_details typed bag next to StreamSender fail"
    );
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

fn interceptor_blocked_from_error_details() -> Status {
    let details = pbrs_grpc::pb::ErrorDetails {
        error_info: Some(pbrs_grpc::pb::ErrorInfo::with_reason(
            "BLOCKED",
            "example.com",
        )),
        ..pbrs_grpc::pb::ErrorDetails::default()
    };
    Status::from_error_details(Code::FailedPrecondition, "blocked locally", &details)
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

    async fn list(
        &self,
        _: Request<HealthListRequest>,
    ) -> Result<Response<HealthListResponse>, Status> {
        Err(interceptor_blocked())
    }

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HealthCheckResponse>>, Status> {
        Err(interceptor_blocked())
    }
}

struct FailHealthFromErrorDetails;

impl Health for FailHealthFromErrorDetails {
    async fn check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Err(interceptor_blocked_from_error_details())
    }

    async fn list(
        &self,
        _: Request<HealthListRequest>,
    ) -> Result<Response<HealthListResponse>, Status> {
        Err(interceptor_blocked_from_error_details())
    }

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HealthCheckResponse>>, Status> {
        Err(interceptor_blocked_from_error_details())
    }
}

fn typed_after_headers_status() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("API_DISABLED");
    info.set_domain("example.com");
    let mut status = Status::with_error_details(
        Code::FailedPrecondition,
        "api disabled",
        [pbrs_grpc::pb::Any::pack(&info).expect("pack")],
    )
    .expect("encode");
    status
        .metadata_mut()
        .insert("x-retry-after", "30")
        .expect("md");
    status
}

fn typed_after_headers_from_error_details() -> Status {
    let details = pbrs_grpc::pb::ErrorDetails {
        error_info: Some(pbrs_grpc::pb::ErrorInfo::with_reason(
            "API_DISABLED",
            "example.com",
        )),
        ..pbrs_grpc::pb::ErrorDetails::default()
    };
    let mut status = Status::from_error_details(Code::FailedPrecondition, "api disabled", &details)
        .expect("encode");
    status
        .metadata_mut()
        .insert("x-retry-after", "30")
        .expect("md");
    status
}

fn assert_typed_after_headers(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "api disabled");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let unpacked = err
        .error_details()
        .expect("ErrorDetails")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(unpacked.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(unpacked.domain().to_str().unwrap_or(""), "example.com");
    assert_eq!(err.metadata().get("x-retry-after"), Some("30"));
    assert!(err.metadata().get_bin("grpc-status-details-bin").is_none());
}

fn fail_health_after_one() -> pbrs_grpc::Streaming<HealthCheckResponse> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut reply = HealthCheckResponse::new();
        reply.set_status(ServingStatus::Serving);
        tx.send(reply).await.ok();
        tx.fail(typed_after_headers_status()).await;
    }));
    stream
}

fn fail_health_after_one_from_error_details() -> pbrs_grpc::Streaming<HealthCheckResponse> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut reply = HealthCheckResponse::new();
        reply.set_status(ServingStatus::Serving);
        tx.send(reply).await.ok();
        tx.fail(typed_after_headers_from_error_details()).await;
    }));
    stream
}

/// Watch only: Check is unary and has no response DATA then trailers.
struct TypedAfterHeadersHealth;

impl Health for TypedAfterHeadersHealth {
    async fn check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HealthCheckResponse>>, Status> {
        Ok(Response::new(fail_health_after_one()))
    }
}

struct TypedAfterHeadersHealthFromErrorDetails;

impl Health for TypedAfterHeadersHealthFromErrorDetails {
    async fn check(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn watch(
        &self,
        _: Request<HealthCheckRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HealthCheckResponse>>, Status> {
        Ok(Response::new(fail_health_after_one_from_error_details()))
    }
}

async fn assert_health_typed_status_after_streamed_message(client: &HealthClient) {
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.status(), ServingStatus::Serving);
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.status(), ServingStatus::Serving);
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));
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

    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);

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
    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list after shutdown")
        .into_inner();
    assert_listed_known(
        &listed,
        ServingStatus::NotServing,
        ServingStatus::NotServing,
    );
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
    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list after resume")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);
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
async fn health_interceptor_rejects_check_and_watch_with_from_error_details() {
    let (svc, reporter) = service();
    reporter.set_serving("");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked_from_error_details()))
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
async fn health_client_interceptor_rejects_check_and_watch_with_from_error_details() {
    let (addr, _reporter, handle) = serve().await;
    let client = client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked_from_error_details()));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = HealthClient::connect_lazy(addr).expect("lazy");
    wait_then_complete_health(
        &client,
        true,
        async move { serve_health_on(reserved.listen()) },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = HealthClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_health(
        &client,
        false,
        async move { serve_health_on(reserved.listen()) },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_tls_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_health(&client, true, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_tls_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_health(&client, false, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_mtls_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_health(&client, true, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_mtls_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_health(&client, false, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_unix_wait_for_ready_completes_once_the_server_listens() {
    let path = unix_sock("wait");
    let client = HealthClient::connect_unix_lazy(&path).expect("lazy");
    wait_then_complete_health(&client, true, async {
        let sock = path.clone();
        let svc = health_server();
        ServeGuard(tokio::spawn(async move {
            svc.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_unix_channel_wait_for_ready_completes_once_the_server_listens() {
    let path = unix_sock("channel-wait");
    let client = HealthClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_health(&client, false, async {
        let sock = path.clone();
        let svc = health_server();
        ServeGuard(tokio::spawn(async move {
            svc.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client =
        HealthClient::connect_lazy(addr)
            .expect("lazy")
            .intercept(|call: &mut Outgoing<'_>| {
                call.set_wait_for_ready(true);
                Ok(())
            });
    wait_then_complete_health(
        &client,
        false,
        async move { serve_health_on(reserved.listen()) },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_tls_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_health(&client, false, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_mtls_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_health(&client, false, async move {
        serve_health_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_unix_client_interceptor_can_set_wait_for_ready() {
    let path = unix_sock("intercept-wait");
    let client = HealthClient::connect_unix_lazy(&path)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_health(&client, false, async {
        let sock = path.clone();
        let svc = health_server();
        ServeGuard(tokio::spawn(async move {
            svc.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = HealthClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_unavailable(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_tls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_unavailable(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_mtls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_unavailable(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_health_unix_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let path = unix_sock("intercept-opt-out");
    let client = HealthClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_unavailable(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_file(&path);
}

fn overlays_survive_clear(call: &mut Outgoing<'_>) -> Result<(), Status> {
    if call.rpc_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!(
            "rpc_timeout {:?}",
            call.rpc_timeout()
        )));
    }
    if !call.waits_for_ready() {
        return Err(Status::internal("waits_for_ready overlay"));
    }
    if !call.compresses_outbound() {
        return Err(Status::internal("compresses_outbound overlay"));
    }
    if call.timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!("timeout {:?}", call.timeout())));
    }
    if !call.wait_for_ready_is_set() || !call.wait_for_ready() {
        return Err(Status::internal("wait-for-ready not filled"));
    }
    if !call.compress_is_set() || !call.compress() {
        return Err(Status::internal("compress not filled"));
    }
    call.clear_timeout();
    call.clear_wait_for_ready();
    call.clear_compress();
    if call.rpc_timeout() != Some(Duration::from_secs(5))
        || !call.waits_for_ready()
        || !call.compresses_outbound()
    {
        return Err(Status::internal("overlays vanished after clear"));
    }
    Ok(())
}

fn overlay_after_clear_health(client: HealthClient) -> HealthClient {
    client
        .timeout(Duration::from_secs(5))
        .wait_for_ready()
        .send_compressed()
        .intercept(overlays_survive_clear)
}

async fn assert_cleared_wait_fails_fast_health(client: &HealthClient) {
    tokio::time::timeout(Duration::from_secs(2), assert_health_unavailable(client))
        .await
        .expect("cleared wait-for-ready hung");
}

#[tokio::test]
async fn a_health_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = overlay_after_clear_health(HealthClient::connect_lazy(addr).expect("lazy"));
    assert_cleared_wait_fails_fast_health(&client).await;
}

#[tokio::test]
async fn a_health_tls_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client =
        overlay_after_clear_health(HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast_health(&client).await;
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client =
        overlay_after_clear_health(HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast_health(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_sees_channel_overlays_after_clear() {
    let path = unix_sock("health-overlay-clear");
    let client = overlay_after_clear_health(HealthClient::connect_unix_lazy(&path).expect("lazy"));
    assert_cleared_wait_fails_fast_health(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_sees_channel_overlays_after_clear() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    let client = overlay_after_clear_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

fn reapply_channel_gzip(call: &mut Outgoing<'_>) -> Result<(), Status> {
    if !call.compresses_outbound() {
        return Err(Status::internal("compresses_outbound overlay"));
    }
    call.clear_compress();
    call.set_compress(call.compresses_outbound());
    Ok(())
}

#[tokio::test]
async fn a_health_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_listener(listener).await.ok();
    });
    let client = client(addr)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_can_reapply_channel_gzip_after_clear() {
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
    let client = tls_client(addr)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_can_reapply_channel_gzip_after_clear() {
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
    let client = tls_client_with(addr, client_tls)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let path = unix_sock("health-gzip-reapply");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_unix(sock).await.ok();
    });
    let client = unix_client(&path)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_health_check_and_watch(&client).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.send_compressed().serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_health_check_and_watch(&client).await;
    handle.abort();
}

#[derive(Clone, Copy)]
struct Trace(&'static str);

fn interceptor_insert_trace(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.extensions_mut().insert(Trace("abc"));
    Ok(())
}

fn interceptor_stamp_trace(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let Some(trace) = call.extensions().get::<Trace>().copied() else {
        return Err(Status::internal("first interceptor did not run"));
    };
    call.metadata_mut().insert("x-trace", trace.0)?;
    Ok(())
}

fn require_trace(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-trace") != Some("abc") {
        return Err(Status::invalid_argument("missing trace"));
    }
    Ok(())
}

fn stacked_trace_health(client: HealthClient) -> HealthClient {
    client
        .intercept(interceptor_insert_trace)
        .intercept(interceptor_stamp_trace)
}

#[tokio::test]
async fn health_client_interceptors_stack_and_share_extensions() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_trace)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_health_check_and_watch(&stacked_trace_health(client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&stacked_trace_health(tls_client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&stacked_trace_health(
        tls_client_with(addr, client_tls).await,
    ))
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_client_interceptors_stack_and_share_extensions() {
    let path = unix_sock("health-stack-trace");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_trace).serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&stacked_trace_health(unix_client(&path).await)).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_client_interceptors_stack_and_share_extensions() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_trace)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_health_check_and_watch(&stacked_trace_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    handle.abort();
}

#[derive(Clone)]
struct Tenant(String);

fn interceptor_stamp_tenant(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let Some(tenant) = call.extensions().get::<Tenant>().cloned() else {
        return Err(Status::internal("missing Tenant"));
    };
    call.metadata_mut().insert("x-tenant", tenant.0)?;
    Ok(())
}

fn require_tenant(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-tenant") != Some("acme") {
        return Err(Status::unauthenticated("missing tenant"));
    }
    Ok(())
}

fn with_tenant<T>(mut request: Request<T>) -> Request<T> {
    request.extensions_mut().insert(Tenant("acme".into()));
    request
}

async fn assert_health_err(client: &HealthClient, want: Code) {
    let err = client
        .check(Request::new(HealthCheckRequest::new()))
        .await
        .expect_err("check");
    assert_eq!(err.code(), want, "{err}");
    let err = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect_err("watch");
    assert_eq!(err.code(), want, "{err}");
    let err = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect_err("list");
    assert_eq!(err.code(), want, "{err}");
}

async fn echo_tenant_health(client: &HealthClient) {
    let overall = client
        .check(with_tenant(Request::new(HealthCheckRequest::new())))
        .await
        .expect("overall")
        .into_inner();
    assert_eq!(overall.status(), ServingStatus::Serving);
    let named = client
        .check(with_tenant(Request::new(req("helloworld.Greeter"))))
        .await
        .expect("named")
        .into_inner();
    assert_eq!(named.status(), ServingStatus::Serving);
    let listed = client
        .list(with_tenant(Request::new(HealthListRequest::new())))
        .await
        .expect("list")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);
    let mut stream = client
        .watch(with_tenant(Request::new(HealthCheckRequest::new())))
        .await
        .expect("watch")
        .into_inner();
    let first = stream.message().await.expect("first").expect("msg");
    assert_eq!(first.status(), ServingStatus::Serving);
    assert_health_err(client, Code::Internal).await;
}

fn interceptor_stamp_user_agent(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let ua = call.user_agent();
    if !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("user-agent {ua}")));
    }
    call.metadata_mut().set("x-ua", ua)?;
    Ok(())
}

fn require_stamped_user_agent(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    let ua = rpc.metadata().get("user-agent").unwrap_or("");
    let stamped = rpc.metadata().get("x-ua").unwrap_or("");
    if stamped != ua || !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("ua {ua:?} x-ua {stamped:?}")));
    }
    Ok(())
}

fn user_agent_health(client: HealthClient) -> HealthClient {
    client
        .user_agent("inventory/2.1")
        .expect("user-agent")
        .intercept(interceptor_stamp_user_agent)
}

fn interceptor_set_user_agent(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.set_user_agent("override/1.0")?;
    let ua = call.user_agent();
    if !ua.starts_with("override/1.0 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("user-agent {ua}")));
    }
    Ok(())
}

fn require_override_user_agent(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    let ua = rpc.metadata().get("user-agent").unwrap_or("");
    if !ua.starts_with("override/1.0 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("ua {ua}")));
    }
    Ok(())
}

fn override_ua_health(client: HealthClient) -> HealthClient {
    client
        .user_agent("inventory/2.1")
        .expect("user-agent")
        .intercept(interceptor_set_user_agent)
}

fn test_message_limits() -> MessageLimits {
    MessageLimits::new()
        .with_max_decoding(64 * 1024)
        .with_max_encoding(64 * 1024)
}

fn interceptor_require_limits(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let want = test_message_limits();
    if call.limits() != want {
        return Err(Status::internal(format!("limits {:?}", call.limits())));
    }
    Ok(())
}

fn limits_health(client: HealthClient) -> HealthClient {
    client
        .message_limits(test_message_limits())
        .intercept(interceptor_require_limits)
}

#[tokio::test]
async fn a_health_client_interceptor_reads_caller_extensions() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_tenant)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_tenant_health(&client(addr).await.intercept(interceptor_stamp_tenant)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_tenant_health(&tls_client(addr).await.intercept(interceptor_stamp_tenant)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_tenant_health(
        &tls_client_with(addr, client_tls)
            .await
            .intercept(interceptor_stamp_tenant),
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_reads_caller_extensions() {
    let path = unix_sock("health-tenant");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_tenant).serve_unix(sock).await.ok();
    });
    echo_tenant_health(&unix_client(&path).await.intercept(interceptor_stamp_tenant)).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_reads_caller_extensions() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_tenant)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_tenant_health(
        &HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .intercept(interceptor_stamp_tenant),
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_client_interceptor_sees_the_user_agent() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_health_check_and_watch(&user_agent_health(client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&user_agent_health(tls_client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&user_agent_health(tls_client_with(addr, client_tls).await)).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_sees_the_user_agent() {
    let path = unix_sock("health-ua");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_health_check_and_watch(&user_agent_health(unix_client(&path).await)).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_sees_the_user_agent() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_stamped_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_health_check_and_watch(&user_agent_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_client_interceptor_sets_the_user_agent() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_override_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_health_check_and_watch(&override_ua_health(client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_sets_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_override_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&override_ua_health(tls_client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_sets_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.intercept(require_override_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&override_ua_health(tls_client_with(addr, client_tls).await)).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_sets_the_user_agent() {
    let path = unix_sock("health-ua-set");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_override_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_health_check_and_watch(&override_ua_health(unix_client(&path).await)).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_sets_the_user_agent() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.intercept(require_override_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_health_check_and_watch(&override_ua_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_client_interceptor_sees_message_limits() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_listener(listener).await.ok();
    });
    echo_health_check_and_watch(&limits_health(client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&limits_health(tls_client(addr).await)).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&limits_health(tls_client_with(addr, client_tls).await)).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_sees_message_limits() {
    let path = unix_sock("health-limits");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&limits_health(unix_client(&path).await)).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_sees_message_limits() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    echo_health_check_and_watch(&limits_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    handle.abort();
}

fn interceptor_reserved_metadata(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut()
        .insert("grpc-previous-rpc-attempts", "1")?;
    Ok(())
}

fn interceptor_hop_by_hop(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut().insert("connection", "close")?;
    Ok(())
}

fn interceptor_fail_before_open(_: &mut Outgoing<'_>) -> Result<(), Status> {
    Err(Status::failed_precondition("blocked locally"))
}

fn reserved_health(client: HealthClient) -> HealthClient {
    client.intercept(interceptor_reserved_metadata)
}

fn hop_health(client: HealthClient) -> HealthClient {
    client.intercept(interceptor_hop_by_hop)
}

fn fail_open_health(client: HealthClient) -> HealthClient {
    client.intercept(interceptor_fail_before_open)
}

#[tokio::test]
async fn a_health_client_interceptor_cannot_insert_reserved_metadata() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_listener(listener).await.ok();
    });
    assert_health_err(&reserved_health(client(addr).await), Code::InvalidArgument).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_err(
        &reserved_health(tls_client(addr).await),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_err(
        &reserved_health(tls_client_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_cannot_insert_reserved_metadata() {
    let path = unix_sock("health-reserved");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    assert_health_err(
        &reserved_health(unix_client(&path).await),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_cannot_insert_reserved_metadata() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    assert_health_err(
        &reserved_health(
            HealthClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_listener(listener).await.ok();
    });
    assert_health_err(&hop_health(client(addr).await), Code::InvalidArgument).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_err(&hop_health(tls_client(addr).await), Code::InvalidArgument).await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_err(
        &hop_health(tls_client_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let path = unix_sock("health-hop");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    assert_health_err(&hop_health(unix_client(&path).await), Code::InvalidArgument).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    assert_health_err(
        &hop_health(
            HealthClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_listener(listener).await.ok();
    });
    assert_health_err(
        &fail_open_health(client(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_tls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_err(
        &fail_open_health(tls_client(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        svc.serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_err(
        &fail_open_health(tls_client_with(addr, client_tls).await),
        Code::FailedPrecondition,
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let path = unix_sock("health-fail-open");
    let sock = path.clone();
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    assert_health_err(
        &fail_open_health(unix_client(&path).await),
        Code::FailedPrecondition,
    )
    .await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    assert_health_err(
        &fail_open_health(
            HealthClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::FailedPrecondition,
    )
    .await;
    handle.abort();
}

fn intercept_counts_create_health(client: HealthClient, ran: &Arc<AtomicUsize>) -> HealthClient {
    let flag = Arc::clone(ran);
    client.intercept(move |_: &mut Outgoing<'_>| {
        flag.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

fn assert_interceptors_run_on_create_health(client: &HealthClient, ran: &Arc<AtomicUsize>) {
    let check = client.check(Request::new(HealthCheckRequest::new()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "Check interceptor must run when the method returns"
    );
    drop(check);

    let watch = client.watch(Request::new(HealthCheckRequest::new()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "Watch interceptor must run when the method returns"
    );
    drop(watch);
}

#[tokio::test]
async fn health_client_interceptors_run_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client =
        intercept_counts_create_health(HealthClient::connect_lazy(addr).expect("lazy"), &ran);
    assert_interceptors_run_on_create_health(&client, &ran);
}

#[tokio::test]
async fn a_health_tls_client_interceptor_runs_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = intercept_counts_create_health(
        HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_health(&client, &ran);
}

#[tokio::test]
async fn a_health_mtls_client_interceptor_runs_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = intercept_counts_create_health(
        HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_health(&client, &ran);
}

#[cfg(unix)]
#[tokio::test]
async fn a_health_unix_client_interceptor_runs_when_the_call_is_created() {
    let path = unix_sock("health-on-create");
    let ran = Arc::new(AtomicUsize::new(0));
    let client =
        intercept_counts_create_health(HealthClient::connect_unix_lazy(&path).expect("lazy"), &ran);
    assert_interceptors_run_on_create_health(&client, &ran);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_health_from_io_client_interceptor_runs_when_the_call_is_created() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_health(
        HealthClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &ran,
    );
    assert_interceptors_run_on_create_health(&client, &ran);
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = HealthClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = HealthClient::connect_lazy(addr).expect("lazy");
    assert_health_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_tls_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_tls_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_health_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_mtls_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_mtls_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = HealthClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_health_wait_deadline(&client).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_unix_request_can_opt_out_of_channel_wait_for_ready() {
    let path = unix_sock("opt-out");
    let client = HealthClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_health_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_unix_wait_for_ready_times_out_when_nothing_is_listening() {
    let path = unix_sock("deadline");
    let client = HealthClient::connect_unix_lazy(&path).expect("lazy");
    assert_health_wait_deadline(&client).await;
    let _ = std::fs::remove_file(&path);
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
async fn health_handlers_return_from_error_details_on_check_and_watch() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(FailHealthFromErrorDetails)
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

#[tokio::test]
async fn health_typed_google_rpc_status_after_a_streamed_message() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealth)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health_typed_status_after_streamed_message(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_from_error_details_after_a_streamed_message() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealthFromErrorDetails)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health_typed_status_after_streamed_message(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_typed_google_rpc_status_after_a_streamed_message() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealth)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_typed_status_after_streamed_message(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_typed_google_rpc_status_after_a_streamed_message() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealth)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_typed_status_after_streamed_message(&tls_client_with(addr, client_tls).await)
        .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_typed_google_rpc_status_after_a_streamed_message() {
    let path = unix_sock("typed-after-headers");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealth)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_health_typed_status_after_streamed_message(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_typed_google_rpc_status_after_a_streamed_message() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        HealthServer::new(TypedAfterHeadersHealth)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_typed_status_after_streamed_message(&client).await;
    handle.abort();
}

async fn assert_health_protocol(client: &HealthClient, reporter: &HealthReporter) {
    let missing = client
        .check(Request::new(req("no.Such")))
        .await
        .expect_err("unknown");
    assert_eq!(missing.code(), Code::NotFound, "{missing}");

    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);

    let mut unknown = client
        .watch(Request::new(req("no.Such")))
        .await
        .expect("watch unknown")
        .into_inner();
    let first = unknown
        .message()
        .await
        .expect("unknown first")
        .expect("msg");
    assert_eq!(first.status(), ServingStatus::ServiceUnknown);
    drop(unknown);

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
    drop(stream);
    reporter.set_serving("");

    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch shutdown")
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
    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list after shutdown")
        .into_inner();
    assert_listed_known(
        &listed,
        ServingStatus::NotServing,
        ServingStatus::NotServing,
    );
    reporter.resume();
    let third = tokio::time::timeout(Duration::from_secs(2), stream.message())
        .await
        .expect("timeout")
        .expect("third")
        .expect("msg");
    assert_eq!(third.status(), ServingStatus::Serving);
    let listed = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect("list after resume")
        .into_inner();
    assert_listed_known(&listed, ServingStatus::Serving, ServingStatus::Serving);
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
        "shutdown Watch must release after drop"
    );
    let mut stream = client
        .watch(Request::new(HealthCheckRequest::new()))
        .await
        .expect("watch drop")
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
async fn health_tls_check_watch_protocol() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
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
    let client = tls_client_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    assert_health_protocol(&client, &reporter).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_check_watch_protocol() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
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
    assert_health_protocol(&tls_client_with(addr, client_tls).await, &reporter).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_check_watch_protocol() {
    let path = unix_sock("protocol");
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        svc.serve_unix(sock).await.ok();
    });
    assert_health_protocol(&unix_client(&path).await, &reporter).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_check_watch_protocol() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let (svc, reporter) = service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        svc.serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_protocol(&client, &reporter).await;
    handle.abort();
}

async fn assert_health_oversize(client: &HealthClient) {
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
}

fn health_oversize_router() -> Router {
    let (svc, reporter) = service();
    reporter.set_serving("");
    Router::new().max_decoding_message_size(16).add_service(svc)
}

#[tokio::test]
async fn health_tls_oversize_request_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_oversize_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_oversize(
        &tls_client_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_oversize_request_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_oversize_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_oversize(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_oversize_request_is_resource_exhausted() {
    let path = unix_sock("oversize");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_oversize_router().serve_unix(sock).await.ok();
    });
    assert_health_oversize(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_oversize_request_is_resource_exhausted() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_oversize_router()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_oversize(&client).await;
    handle.abort();
}

async fn assert_health_client_encode_cap(client: &HealthClient) {
    let mut fat = HealthCheckRequest::new();
    fat.set_service("k".repeat(64));
    let err = client
        .check(Request::new(fat.clone()))
        .await
        .expect_err("check encode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match client.watch(Request::new(fat)).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(_) => panic!("Watch client encode cap must fail before headers"),
    }
}

async fn assert_health_client_decode_cap(client: &HealthClient) {
    let mut empty = HealthCheckRequest::new();
    empty.set_service("");
    let err = client
        .check(Request::new(empty.clone()))
        .await
        .expect_err("check decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match client.watch(Request::new(empty)).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("Watch client decode cap must fail"),
        },
    }
    let err = client
        .list(Request::new(HealthListRequest::new()))
        .await
        .expect_err("list decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
}

async fn assert_health_client_message_caps(client: HealthClient) {
    assert_health_client_encode_cap(&client.clone().max_encoding_message_size(16)).await;
    assert_health_client_decode_cap(&client.clone().max_decoding_message_size(1)).await;
    assert_health_client_encode_cap(
        &client
            .clone()
            .message_limits(MessageLimits::new().with_max_encoding(16)),
    )
    .await;
    assert_health_client_decode_cap(
        &client.message_limits(MessageLimits::new().with_max_decoding(1)),
    )
    .await;
}

fn health_plain() -> HealthServer<impl Health> {
    service().0
}

#[tokio::test]
async fn health_client_message_caps_are_resource_exhausted() {
    let (addr, _reporter, handle) = serve().await;
    assert_health_client_message_caps(client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_plain()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_client_message_caps(
        tls_client_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_plain()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_client_message_caps(tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_client_message_caps_are_resource_exhausted() {
    let path = unix_sock("client-caps");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_plain().serve_unix(sock).await.ok();
    });
    assert_health_client_message_caps(unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_client_message_caps_are_resource_exhausted() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_plain().serve_connection(server_io).await.ok();
    });
    let client = HealthClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_health_client_message_caps(client).await;
    handle.abort();
}

fn health_decode_limits() -> MessageLimits {
    MessageLimits::new().with_max_decoding(16)
}

fn health_oversize_limits_router() -> Router {
    let (svc, reporter) = service();
    reporter.set_serving("");
    Router::new()
        .message_limits(health_decode_limits())
        .add_service(svc)
}

fn health_oversize_limits_server() -> HealthServer<impl Health> {
    health_plain().message_limits(health_decode_limits())
}

#[tokio::test]
async fn health_message_limits_oversize_is_resource_exhausted() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_router()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health_oversize(&client(addr).await).await;
    handle.abort();
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_server()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health_oversize(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_message_limits_oversize_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_oversize(&tls_client(addr).await).await;
    handle.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_oversize(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_message_limits_oversize_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_oversize(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_oversize_limits_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_oversize(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_message_limits_oversize_is_resource_exhausted() {
    let path = unix_sock("msg-limits");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_oversize_limits_router().serve_unix(sock).await.ok();
    });
    assert_health_oversize(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
    let path = unix_sock("msg-limits-srv");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_oversize_limits_server().serve_unix(sock).await.ok();
    });
    assert_health_oversize(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_message_limits_oversize_is_resource_exhausted() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let handle1 = tokio::spawn(async move {
        health_oversize_limits_router()
            .serve_connection(s1)
            .await
            .ok();
    });
    let client = HealthClient::from_io(c1, "localhost")
        .await
        .expect("from_io router");
    assert_health_oversize(&client).await;
    handle1.abort();
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let handle2 = tokio::spawn(async move {
        health_oversize_limits_server()
            .serve_connection(s2)
            .await
            .ok();
    });
    let client = HealthClient::from_io(c2, "localhost")
        .await
        .expect("from_io server");
    assert_health_oversize(&client).await;
    handle2.abort();
}

fn health_dial_encode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(MessageLimits::new().with_max_encoding(16))
}

fn health_dial_decode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(MessageLimits::new().with_max_decoding(1))
}

async fn health_cfg(addr: SocketAddr, cfg: ChannelConfig) -> HealthClient {
    let mut last = None;
    for _ in 0..80 {
        match HealthClient::connect_with(addr, cfg).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn health_tls_cfg(addr: SocketAddr, tls: ClientTls, cfg: ChannelConfig) -> HealthClient {
    let mut last = None;
    for _ in 0..80 {
        match HealthClient::connect_tls_with(addr, cfg, tls.clone()).await {
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
async fn health_unix_cfg(path: &std::path::Path, cfg: ChannelConfig) -> HealthClient {
    let mut last = None;
    for _ in 0..80 {
        match HealthClient::connect_unix_with(path, cfg).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

#[tokio::test]
async fn health_channel_config_message_limits_are_resource_exhausted() {
    let (addr, _reporter, handle) = serve().await;
    assert_health_client_encode_cap(&health_cfg(addr, health_dial_encode_limits()).await).await;
    assert_health_client_decode_cap(&health_cfg(addr, health_dial_decode_limits()).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_channel_config_message_limits_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_plain()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_health_client_encode_cap(
        &health_tls_cfg(addr, client_tls.clone(), health_dial_encode_limits()).await,
    )
    .await;
    assert_health_client_decode_cap(
        &health_tls_cfg(addr, client_tls, health_dial_decode_limits()).await,
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_channel_config_message_limits_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        health_plain()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_client_encode_cap(
        &health_tls_cfg(addr, client_tls.clone(), health_dial_encode_limits()).await,
    )
    .await;
    assert_health_client_decode_cap(
        &health_tls_cfg(addr, client_tls, health_dial_decode_limits()).await,
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_channel_config_message_limits_are_resource_exhausted() {
    let path = unix_sock("dial-limits");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_plain().serve_unix(sock).await.ok();
    });
    assert_health_client_encode_cap(&health_unix_cfg(&path, health_dial_encode_limits()).await)
        .await;
    assert_health_client_decode_cap(&health_unix_cfg(&path, health_dial_decode_limits()).await)
        .await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_channel_config_message_limits_are_resource_exhausted() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let handle1 = tokio::spawn(async move {
        health_plain().serve_connection(s1).await.ok();
    });
    let encode = HealthClient::from_io_with(c1, "localhost", health_dial_encode_limits())
        .await
        .expect("from_io encode");
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let handle2 = tokio::spawn(async move {
        health_plain().serve_connection(s2).await.ok();
    });
    let decode = HealthClient::from_io_with(c2, "localhost", health_dial_decode_limits())
        .await
        .expect("from_io decode");
    assert_health_client_encode_cap(&encode).await;
    assert_health_client_decode_cap(&decode).await;
    handle1.abort();
    handle2.abort();
}

fn health_header_list_cap() -> HealthServer<impl Health> {
    let (svc, reporter) = service();
    reporter.set_serving("");
    reporter.set_serving("helloworld.Greeter");
    svc.max_header_list_size(1024)
}

fn flood_check() -> Request<HealthCheckRequest> {
    let mut request = Request::new(HealthCheckRequest::new());
    request
        .metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    request
}

async fn assert_health_header_flood_then_echo(flood: HealthClient, healthy: HealthClient) {
    let _ = tokio::time::timeout(Duration::from_secs(2), flood.check(flood_check())).await;
    echo_health_check_and_watch(&healthy).await;
}

#[tokio::test]
async fn health_header_list_cap_refuses_oversize_metadata() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_header_list_cap().serve_listener(listener).await.ok();
    });
    assert_health_header_flood_then_echo(client(addr).await, client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_health_header_flood_then_echo(tls_client(addr).await, tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_health_header_flood_then_echo(
        tls_client_with(addr, client_tls.clone()).await,
        tls_client_with(addr, client_tls).await,
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_header_list_cap_refuses_oversize_metadata() {
    let path = unix_sock("hdr-list");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_header_list_cap().serve_unix(sock).await.ok();
    });
    assert_health_header_flood_then_echo(unix_client(&path).await, unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_header_list_cap_refuses_oversize_metadata() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let handle1 = tokio::spawn(async move {
        health_header_list_cap().serve_connection(s1).await.ok();
    });
    let flood = HealthClient::from_io(c1, "localhost")
        .await
        .expect("from_io flood");
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let handle2 = tokio::spawn(async move {
        health_header_list_cap().serve_connection(s2).await.ok();
    });
    let healthy = HealthClient::from_io(c2, "localhost")
        .await
        .expect("from_io healthy");
    assert_health_header_flood_then_echo(flood, healthy).await;
    handle1.abort();
    handle2.abort();
}

fn health_serving() -> HealthServer<impl Health> {
    let (svc, reporter) = service();
    reporter.set_serving("");
    reporter.set_serving("helloworld.Greeter");
    svc
}

fn health_conn_cap() -> HealthServer<impl Health> {
    health_serving().max_concurrent_connections(1)
}

fn health_pool_against_cap() -> ChannelConfig {
    ChannelConfig::new()
        .connect_timeout(Duration::from_millis(300))
        .connections(2)
}

fn health_pool_cfg() -> ChannelConfig {
    ChannelConfig::new().connections(2)
}

async fn assert_health_cap_refuses_then_echo(
    first: HealthClient,
    second: Result<HealthClient, Status>,
    reconnect: impl std::future::Future<Output = HealthClient>,
) {
    let err = second.expect_err("pool larger than the accept-loop cap should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    echo_health_check_and_watch(&reconnect.await).await;
}

#[tokio::test]
async fn health_pool_against_cap_is_unavailable() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_conn_cap().serve_listener(listener).await.ok();
    });
    let first = client(addr).await;
    assert_health_cap_refuses_then_echo(
        first,
        HealthClient::connect_with(addr, health_pool_against_cap()).await,
        client(addr),
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn tls_health_pool_against_cap_is_unavailable() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = tls_client_with(addr, client_tls.clone()).await;
    assert_health_cap_refuses_then_echo(
        first,
        HealthClient::connect_tls_with(addr, health_pool_against_cap(), client_tls.clone()).await,
        tls_client_with(addr, client_tls),
    )
    .await;
    handle.abort();
}

#[tokio::test]
async fn mtls_health_pool_against_cap_is_unavailable() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        health_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = tls_client_with(addr, client_tls.clone()).await;
    assert_health_cap_refuses_then_echo(
        first,
        HealthClient::connect_tls_with(addr, health_pool_against_cap(), client_tls.clone()).await,
        tls_client_with(addr, client_tls),
    )
    .await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_health_pool_against_cap_is_unavailable() {
    let path = unix_sock("pool-cap");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_conn_cap().serve_unix(sock).await.ok();
    });
    let first = unix_client(&path).await;
    assert_health_cap_refuses_then_echo(
        first,
        HealthClient::connect_unix_with(&path, health_pool_against_cap()).await,
        unix_client(&path),
    )
    .await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_health_pool_config_is_still_one_duplex() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_serving().serve_connection(s).await.ok();
    });
    let client = HealthClient::from_io_with(c, "localhost", health_pool_cfg())
        .await
        .expect("from_io");
    echo_health_check_and_watch(&client).await;
    handle.abort();
}

fn health_frame_size() -> HealthServer<impl Health> {
    health_serving().max_frame_size(16 * 1024)
}

#[tokio::test]
async fn health_frame_size_still_serves_check_and_watch() {
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_frame_size().serve_listener(listener).await.ok();
    });
    echo_health_check_and_watch(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_tls_frame_size_still_serves_check_and_watch() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn health_mtls_frame_size_still_serves_check_and_watch() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn health_unix_frame_size_still_serves_check_and_watch() {
    let path = unix_sock("frame-size");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_frame_size().serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn health_from_io_frame_size_still_serves_check_and_watch() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_frame_size().serve_connection(s).await.ok();
    });
    echo_health_check_and_watch(
        &HealthClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    handle.abort();
}

fn health_pending_reset() -> HealthServer<impl Health> {
    health_serving().max_pending_accept_reset_streams(1)
}

#[tokio::test]
async fn health_pending_reset_still_serves_check_and_watch() {
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_pending_reset().serve_listener(listener).await.ok();
    });
    echo_health_check_and_watch(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn tls_health_pending_reset_still_serves_check_and_watch() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_pending_reset()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn mtls_health_pending_reset_still_serves_check_and_watch() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_pending_reset()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_health_pending_reset_still_serves_check_and_watch() {
    let path = unix_sock("health-pending-reset");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_pending_reset().serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_health_pending_reset_still_serves_check_and_watch() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_pending_reset().serve_connection(s).await.ok();
    });
    echo_health_check_and_watch(
        &HealthClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    handle.abort();
}

fn health_send_buffer() -> HealthServer<impl Health> {
    health_serving().max_send_buffer_size(16 * 1024)
}

#[tokio::test]
async fn health_send_buffer_still_serves_check_and_watch() {
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_send_buffer().serve_listener(listener).await.ok();
    });
    echo_health_check_and_watch(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn tls_health_send_buffer_still_serves_check_and_watch() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_send_buffer()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn mtls_health_send_buffer_still_serves_check_and_watch() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_send_buffer()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_health_send_buffer_still_serves_check_and_watch() {
    let path = unix_sock("health-send-buffer");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_send_buffer().serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_health_send_buffer_still_serves_check_and_watch() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_send_buffer().serve_connection(s).await.ok();
    });
    echo_health_check_and_watch(
        &HealthClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    handle.abort();
}

fn health_window_size() -> HealthServer<impl Health> {
    health_serving()
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(128 * 1024)
}

#[tokio::test]
async fn health_window_size_still_serves_check_and_watch() {
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_window_size().serve_listener(listener).await.ok();
    });
    echo_health_check_and_watch(&client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn tls_health_window_size_still_serves_check_and_watch() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_window_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_health_check_and_watch(&tls_client(addr).await).await;
    handle.abort();
}

#[tokio::test]
async fn mtls_health_window_size_still_serves_check_and_watch() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_health().await;
    let handle = tokio::spawn(async move {
        health_window_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_health_check_and_watch(&tls_client_with(addr, client_tls).await).await;
    handle.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_health_window_size_still_serves_check_and_watch() {
    let path = unix_sock("health-window-size");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        health_window_size().serve_unix(sock).await.ok();
    });
    echo_health_check_and_watch(&unix_client(&path).await).await;
    handle.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_health_window_size_still_serves_check_and_watch() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        health_window_size().serve_connection(s).await.ok();
    });
    echo_health_check_and_watch(
        &HealthClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    handle.abort();
}
