//! Standard `grpc.reflection.v1` list and file lookup.

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

use common::{reserve_loopback, Echo, ServerGuard};
use pbrs_grpc::hello::{GreeterServer, FILE_DESCRIPTOR_SET};
use pbrs_grpc::reflection::{
    service, ExtensionRequest, ListServiceResponse, ServerReflection, ServerReflectionClient,
    ServerReflectionRequest, ServerReflectionResponse, ServerReflectionServer, ServiceResponse,
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

async fn serve() -> (SocketAddr, ServerGuard) {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(reflection)
            .add_service(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

async fn client(addr: SocketAddr) -> ServerReflectionClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return ServerReflectionClient::new(channel),
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

async fn tls_client_with(addr: SocketAddr, client_tls: ClientTls) -> ServerReflectionClient {
    let mut last = None;
    for _ in 0..80 {
        match ServerReflectionClient::connect_tls(addr, client_tls.clone()).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn tls_client(addr: SocketAddr) -> ServerReflectionClient {
    tls_client_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await
}

#[cfg(unix)]
async fn unix_client(path: &std::path::Path) -> ServerReflectionClient {
    let mut last = None;
    for _ in 0..80 {
        match ServerReflectionClient::connect_unix(path).await {
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
        "pbrs-grpc-reflection-{prefix}-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

fn reflection_server() -> ServerReflectionServer<impl ServerReflection> {
    service([FILE_DESCRIPTOR_SET]).expect("reflection")
}

async fn bind_reflection() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

fn serve_reflection_on(listener: TcpListener) -> ServerGuard {
    let reflection = reflection_server();
    ServerGuard(tokio::spawn(async move {
        reflection.serve_listener(listener).await.ok();
    }))
}

fn serve_reflection_tls_on(listener: TcpListener, tls: ServerTls) -> ServerGuard {
    let reflection = reflection_server();
    ServerGuard(tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
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

async fn assert_reflection_opt_out(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
}

async fn assert_reflection_unavailable(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
}

async fn assert_reflection_wait_deadline(client: &ServerReflectionClient) {
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    let (tx, call) = client.server_reflection_info(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

async fn wait_then_complete_reflection(
    client: &ServerReflectionClient,
    wait_on_request: bool,
    start: impl std::future::Future,
) {
    let timeout = Some(Duration::from_secs(5));
    let (tx, mut call) =
        client.server_reflection_info(stamp_wait_ready(Request::new(()), wait_on_request, timeout));

    tokio::select! {
        biased;
        result = &mut call => panic!("bidi finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = start.await;

    let mut inbound = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("bidi hung after listen")
        .expect("bidi")
        .into_inner();
    tx.send(list_req()).await.expect("send");
    let resp = inbound
        .message()
        .await
        .expect("read")
        .expect("reflection reply");
    assert!(
        resp.has_list_services_response(),
        "expected list, got error {:?}",
        resp.error_response().error_message()
    );
    let names = service_names(&resp);
    assert!(
        names.contains(&"helloworld.Greeter".to_owned()),
        "{names:?}"
    );
}

fn list_req() -> ServerReflectionRequest {
    let mut req = ServerReflectionRequest::new();
    req.set_list_services("");
    req
}

fn symbol_req(symbol: &str) -> ServerReflectionRequest {
    let mut req = ServerReflectionRequest::new();
    req.set_file_containing_symbol(symbol);
    req
}

fn filename_req(name: &str) -> ServerReflectionRequest {
    let mut req = ServerReflectionRequest::new();
    req.set_file_by_filename(name);
    req
}

async fn ask(
    client: &ServerReflectionClient,
    req: ServerReflectionRequest,
) -> ServerReflectionResponse {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    let mut inbound = call.await.expect("open").into_inner();
    tx.send(req).await.expect("send");
    inbound
        .message()
        .await
        .expect("read")
        .expect("reflection reply")
}

fn service_names(resp: &ServerReflectionResponse) -> Vec<String> {
    resp.list_services_response()
        .service()
        .iter()
        .map(|s| s.name().to_str().unwrap_or("").to_owned())
        .collect()
}

#[test]
fn reflection_crate_docs_name_interceptor_wait_for_ready() {
    let src = include_str!("../src/reflection.rs");
    assert!(
        src.contains("wait-for-ready is set on the request, the client, or a client interceptor."),
        "reflection crate rustdoc must name interceptor-set wait-for-ready"
    );
    assert!(
        src.contains(
            "`Request::set_wait_for_ready(false)` and a client interceptor\n//! `set_wait_for_ready(false)` opt out of a client default. A waiting Call's\n//! deadline applies on those dialers."
        ),
        "reflection crate rustdoc must name wait-for-ready opt-out and deadline"
    );
    assert!(
        src.contains(
            "[`crate::StreamSender::fail`] after a streamed DATA frame on\n//! `ServerReflectionInfo` ships those trailers the same way."
        ),
        "reflection crate rustdoc must name typed Status after streamed DATA"
    );
    assert!(
        src.contains(
            "`file_containing_symbol` and `file_by_filename` return the\n//! registered `FileDescriptorProto` on that method, including over TLS, mTLS,\n//! Unix, and [`crate::Channel::from_io`]. A missing symbol is `NOT_FOUND` on\n//! the stream."
        ),
        "reflection crate rustdoc must name file lookups on every transport"
    );
    assert!(
        src.contains(
            "`file_containing_extension` and `all_extension_numbers_of_type`\n//! answer from the same method on those transports; a missing extension is\n//! `NOT_FOUND` on the stream."
        ),
        "reflection crate rustdoc must name extension lookups on every transport"
    );
    assert!(
        src.contains(
            "over the decoding cap fails the stream as `RESOURCE_EXHAUSTED` trailers\n//! (`StreamSender::fail`), not a quiet OK end, including over TLS, mTLS, Unix,\n//! and [`crate::Channel::from_io`]."
        ),
        "reflection crate rustdoc must name oversize RESOURCE_EXHAUSTED on every transport"
    );
    assert!(
        src.contains(
            "A [`ServerReflectionClient`]\n//! `max_encoding_message_size` / `max_decoding_message_size` is\n//! `RESOURCE_EXHAUSTED` on the one bidi method on those transports, distinct\n//! from the server decoding cap."
        ),
        "reflection crate rustdoc must name client message caps on every transport"
    );
    assert!(
        src.contains(
            "[`ServerReflectionClient::message_limits`]\n//! refuses the same oversize, distinct from those single-cap wrappers."
        ),
        "reflection crate rustdoc must name wrap message_limits on every transport"
    );
    assert!(
        src.contains(
            "`Router::message_limits` /\n//! [`ServerReflectionServer::message_limits`] refuse the same oversize as\n//! `RESOURCE_EXHAUSTED` trailers on that method, distinct from\n//! [`crate::Router::max_decoding_message_size`]."
        ),
        "reflection crate rustdoc must name combined-setter oversize on every transport"
    );
    assert!(
        src.contains(
            "[`ServerReflectionClient::connect_tls_with`] /\n//! [`ServerReflectionClient::connect_unix_with`] /\n//! [`ServerReflectionClient::from_io_with`] with\n//! [`crate::ChannelConfig::message_limits`] refuse the same oversize, distinct\n//! from wrapping a live client."
        ),
        "reflection crate rustdoc must name dial-time ChannelConfig message_limits on every transport"
    );
    assert!(
        src.contains(
            "[`ServerReflectionServer::max_header_list_size`]\n//! refuses oversize metadata on the one bidi method, including over TLS, mTLS,\n//! Unix, and [`crate::Server::serve_connection`]. Distinct from wrapping only a\n//! Greeter server."
        ),
        "reflection crate rustdoc must name header-list flood on ServerReflectionInfo"
    );
    assert!(
        src.contains(
            "[`ServerReflectionServer::max_frame_size`] still serves the\n//! one bidi method at the HTTP/2 16 KiB SETTINGS minimum, including over TLS,\n//! mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from wrapping\n//! only a Greeter server."
        ),
        "reflection crate rustdoc must name max_frame_size still-serves on ServerReflectionInfo"
    );
    assert!(
        src.contains(
            "[`ServerReflectionServer::max_pending_accept_reset_streams`]\n//! still serves the one bidi method at a pending-reset cap of 1, including over\n//! TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. A well-behaved\n//! client never fills that queue. Distinct from wrapping only a Greeter server."
        ),
        "reflection crate rustdoc must name pending-reset still-serves on ServerReflectionInfo"
    );
    assert!(
        src.contains(
            "[`ServerReflectionServer::max_send_buffer_size`] still serves the one bidi\n//! method at a 16 KiB send buffer, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter\n//! server."
        ),
        "reflection crate rustdoc must name send-buffer still-serves on ServerReflectionInfo"
    );
    assert!(
        src.contains(
            "[`ServerReflectionServer::initial_stream_window_size`] /\n//! [`ServerReflectionServer::initial_connection_window_size`] still serve the\n//! one bidi method at a 64 KiB stream / 128 KiB connection window, including\n//! over TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from\n//! wrapping only a Greeter server."
        ),
        "reflection crate rustdoc must name HTTP/2 window still-serves on ServerReflectionInfo"
    );
    assert!(
        src.contains(
            "A [`ServerReflectionClient`] pool larger than\n//! [`ServerReflectionServer::max_concurrent_connections`] fails the whole dial\n//! as `UNAVAILABLE` on TLS, mTLS, and Unix.\n//! [`ServerReflectionClient::from_io_with`] cannot pool."
        ),
        "reflection crate rustdoc must name pool-vs-cap UNAVAILABLE on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this reflection interceptor Err; those trailers reach the client without reading the body."
        ),
        "reflection crate rustdoc must name from_error_details typed bag next to interceptor Err"
    );
    assert!(
        src.contains(
            "[`crate::Status::from_error_details`] is the typed bag after this reflection handler Err; those trailers reach the client."
        ),
        "reflection crate rustdoc must name from_error_details typed bag next to handler Err"
    );
}

#[tokio::test]
async fn list_services_includes_the_registered_greeter() {
    let (addr, _guard) = serve().await;
    let client = client(addr).await;
    let resp = ask(&client, list_req()).await;
    assert!(
        resp.has_list_services_response(),
        "expected list, got error {:?}",
        resp.error_response().error_message()
    );
    let names = service_names(&resp);
    assert!(
        names.contains(&"helloworld.Greeter".to_owned()),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("ServerReflection")),
        "reflection itself is not registered: {names:?}"
    );
}

#[tokio::test]
async fn file_containing_symbol_returns_the_greeter_descriptor() {
    let (addr, _guard) = serve().await;
    let client = client(addr).await;
    let resp = ask(&client, symbol_req("helloworld.Greeter")).await;
    assert!(
        resp.has_file_descriptor_response(),
        "expected file, got error {:?}",
        resp.error_response().error_message()
    );
    let files = resp.file_descriptor_response().file_descriptor_proto();
    assert!(!files.is_empty(), "missing FileDescriptorProto");
    let joined: Vec<u8> = files.iter().flat_map(|b| b.as_bytes().to_vec()).collect();
    let haystack = String::from_utf8_lossy(&joined);
    assert!(
        haystack.contains("Greeter") || haystack.contains("helloworld"),
        "descriptor should name the service"
    );
}

#[tokio::test]
async fn unknown_symbol_is_not_found_on_the_stream() {
    let (addr, _guard) = serve().await;
    let client = client(addr).await;
    let resp = ask(&client, symbol_req("nope.Missing")).await;
    assert!(resp.has_error_response());
    assert_eq!(resp.error_response().error_code(), Code::NotFound.to_i32());
}

#[tokio::test]
async fn file_by_filename_round_trips_hello_proto() {
    let (addr, _guard) = serve().await;
    let client = client(addr).await;
    let by_symbol = ask(&client, symbol_req("helloworld.HelloRequest")).await;
    assert!(by_symbol.has_file_descriptor_response());
    let first = by_symbol
        .file_descriptor_response()
        .file_descriptor_proto()
        .get(0)
        .expect("file");
    // FileDescriptorProto field 1 is the name; we registered hello.proto.
    let resp = ask(&client, filename_req("hello.proto")).await;
    assert!(
        resp.has_file_descriptor_response(),
        "hello.proto: {:?}",
        resp.error_response().error_message()
    );
    assert!(!resp
        .file_descriptor_response()
        .file_descriptor_proto()
        .is_empty());
    assert!(!first.as_bytes().is_empty());
}

mod ext {
    #![allow(missing_docs, unused, reason = "generated descriptor set fixture")]
    include!(concat!(env!("OUT_DIR"), "/extend.rs"));
}

fn ext_req(ty: &str, number: i32) -> ServerReflectionRequest {
    let mut er = ExtensionRequest::new();
    er.set_containing_type(ty);
    er.set_extension_number(number);
    let mut req = ServerReflectionRequest::new();
    req.set_file_containing_extension(er);
    req
}

fn ext_numbers_req(ty: &str) -> ServerReflectionRequest {
    let mut req = ServerReflectionRequest::new();
    req.set_all_extension_numbers_of_type(ty);
    req
}

async fn serve_ext() -> (SocketAddr, ServerGuard) {
    let reflection = service([ext::FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(reflection)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

#[tokio::test]
async fn file_containing_extension_returns_the_declaring_file() {
    let (addr, _guard) = serve_ext().await;
    let client = client(addr).await;
    let resp = ask(&client, ext_req("demo.ext.Host", 100)).await;
    assert!(
        resp.has_file_descriptor_response(),
        "expected file, got error {:?}",
        resp.error_response().error_message()
    );
    assert!(!resp
        .file_descriptor_response()
        .file_descriptor_proto()
        .is_empty());
}

#[tokio::test]
async fn unknown_extension_is_not_found_on_the_stream() {
    let (addr, _guard) = serve_ext().await;
    let client = client(addr).await;
    let resp = ask(&client, ext_req("demo.ext.Host", 199)).await;
    assert!(resp.has_error_response());
    assert_eq!(resp.error_response().error_code(), Code::NotFound.to_i32());
}

#[tokio::test]
async fn all_extension_numbers_of_type_lists_registered_tags() {
    let (addr, _guard) = serve_ext().await;
    let client = client(addr).await;
    let resp = ask(&client, ext_numbers_req("demo.ext.Host")).await;
    assert!(
        resp.has_all_extension_numbers_response(),
        "expected numbers, got error {:?}",
        resp.error_response().error_message()
    );
    let nums: Vec<i32> = resp
        .all_extension_numbers_response()
        .extension_number()
        .iter()
        .collect();
    assert_eq!(nums, vec![100]);
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

struct FailReflection;

impl ServerReflection for FailReflection {
    async fn server_reflection_info(
        &self,
        _: Request<pbrs_grpc::Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<ServerReflectionResponse>>, Status> {
        Err(interceptor_blocked())
    }
}

struct FailReflectionFromErrorDetails;

impl ServerReflection for FailReflectionFromErrorDetails {
    async fn server_reflection_info(
        &self,
        _: Request<pbrs_grpc::Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<ServerReflectionResponse>>, Status> {
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

fn fail_reflection_after_one() -> pbrs_grpc::Streaming<ServerReflectionResponse> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut list = ListServiceResponse::new();
        let mut svc = ServiceResponse::new();
        svc.set_name("ada");
        list.service_mut().push(svc);
        let mut resp = ServerReflectionResponse::new();
        resp.set_list_services_response(list);
        tx.send(resp).await.ok();
        tx.fail(typed_after_headers_status()).await;
    }));
    stream
}

/// One bidi method: send a list_services-shaped reply, then trailers.
struct TypedAfterHeadersReflection;

impl ServerReflection for TypedAfterHeadersReflection {
    async fn server_reflection_info(
        &self,
        _: Request<pbrs_grpc::Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<ServerReflectionResponse>>, Status> {
        Ok(Response::new(fail_reflection_after_one()))
    }
}

async fn assert_reflection_typed_status_after_streamed_message(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert!(
        first.has_list_services_response(),
        "expected list, got error {:?}",
        first.error_response().error_message()
    );
    assert_eq!(service_names(&first), vec!["ada".to_owned()]);
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let (tx, call) = client.server_reflection_info(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert!(
        first.has_list_services_response(),
        "expected list, got error {:?}",
        first.error_response().error_message()
    );
    assert_eq!(service_names(&first), vec!["ada".to_owned()]);
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));
}

async fn assert_reflection_blocked(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_interceptor_rejects_with_typed_status() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr).await;
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_interceptor_rejects_with_from_error_details() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked_from_error_details()))
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr).await;
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

async fn echo_reflection_list(client: &ServerReflectionClient) {
    let resp = ask(client, list_req()).await;
    assert!(
        resp.has_list_services_response(),
        "expected list, got error {:?}",
        resp.error_response().error_message()
    );
    let names = service_names(&resp);
    assert!(
        names.contains(&"helloworld.Greeter".to_owned()),
        "{names:?}"
    );
}

async fn gzip_reflection_list(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert!(tx.compress(), "reflection StreamSender must gzip");
    let reply = call.await.expect("open");
    assert_eq!(reply.encoding(), Some("gzip"), "reflection encoding");
    let mut inbound = reply.into_inner();
    tx.send(list_req()).await.expect("send");
    let framed = inbound
        .next_framed()
        .await
        .expect("frame")
        .expect("reflection reply");
    assert!(framed.compressed, "reflection frames gzip");
    let resp = framed.message;
    assert!(
        resp.has_list_services_response(),
        "expected list, got error {:?}",
        resp.error_response().error_message()
    );
    let names = service_names(&resp);
    assert!(
        names.contains(&"helloworld.Greeter".to_owned()),
        "{names:?}"
    );
    tx.close();
}

#[tokio::test]
async fn reflection_client_interceptor_rejects_with_typed_status() {
    let (addr, _guard) = serve().await;
    let client = client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_client_interceptor_rejects_with_from_error_details() {
    let (addr, _guard) = serve().await;
    let client = client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked_from_error_details()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_client_interceptor_sees_list_services_context() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_context)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr).await.intercept(stamp_outgoing_context);
    echo_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_from_io_lists_the_registered_greeter() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    echo_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_from_io_send_compressed_gzips_list_services() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .send_compressed();
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_from_io_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_from_io_client_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_from_io_client_interceptor_sees_list_services_context() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_context)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(stamp_outgoing_context);
    echo_reflection_list(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_lists_the_registered_greeter() {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-reflection-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match ServerReflectionClient::connect_unix(&path).await {
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
    echo_reflection_list(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_send_compressed_gzips_list_services() {
    let path = unix_sock("gzip");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.send_compressed().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    gzip_reflection_list(&unix_client(&path).await.send_compressed()).await;
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_interceptor_rejects_with_typed_status() {
    let path = unix_sock("reject");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = unix_client(&path).await;
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_client_interceptor_rejects_with_typed_status() {
    let path = unix_sock("client-reject");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = unix_client(&path)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_client_interceptor_sees_list_services_context() {
    let path = unix_sock("context");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_context)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = unix_client(&path).await.intercept(stamp_outgoing_context);
    echo_reflection_list(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn oversize_reflection_request_is_resource_exhausted() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .max_decoding_message_size(16)
            .add_service(reflection)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr).await;
    let mut fat = ServerReflectionRequest::new();
    fat.set_file_containing_symbol("k".repeat(64));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    tx.send(fat).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("oversize reflection request must fail as trailers"),
        },
    }
}

#[tokio::test]
async fn reflection_tls_lists_the_registered_greeter() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match ServerReflectionClient::connect_tls(addr, client_tls.clone()).await {
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
    echo_reflection_list(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr).expect("lazy");
    wait_then_complete_reflection(&client, true, async move {
        serve_reflection_on(reserved.listen())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_on(reserved.listen())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_tls_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_reflection(&client, true, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_tls_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_mtls_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_reflection(&client, true, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_mtls_channel_wait_for_ready_completes_once_the_server_listens() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_unix_wait_for_ready_completes_once_the_server_listens() {
    let path = unix_sock("wait");
    let client = ServerReflectionClient::connect_unix_lazy(&path).expect("lazy");
    wait_then_complete_reflection(&client, true, async {
        let sock = path.clone();
        let reflection = reflection_server();
        ServerGuard(tokio::spawn(async move {
            reflection.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_unix_channel_wait_for_ready_completes_once_the_server_listens() {
    let path = unix_sock("channel-wait");
    let client = ServerReflectionClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reflection(&client, false, async {
        let sock = path.clone();
        let reflection = reflection_server();
        ServerGuard(tokio::spawn(async move {
            reflection.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_on(reserved.listen())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_tls_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::new(server_identity()).expect("server tls"),
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_mtls_client_interceptor_can_set_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reflection(&client, false, async move {
        serve_reflection_tls_on(
            reserved.listen(),
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_unix_client_interceptor_can_set_wait_for_ready() {
    let path = unix_sock("intercept-wait");
    let client = ServerReflectionClient::connect_unix_lazy(&path)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reflection(&client, false, async {
        let sock = path.clone();
        let reflection = reflection_server();
        ServerGuard(tokio::spawn(async move {
            reflection.serve_unix(sock).await.ok();
        }))
    })
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reflection_unavailable(&client),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_tls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reflection_unavailable(&client),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reflection_mtls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reflection_unavailable(&client),
    )
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
async fn a_reflection_unix_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let path = unix_sock("intercept-opt-out");
    let client = ServerReflectionClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reflection_unavailable(&client),
    )
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

fn overlay_after_clear_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client
        .timeout(Duration::from_secs(5))
        .wait_for_ready()
        .send_compressed()
        .intercept(overlays_survive_clear)
}

async fn assert_cleared_wait_fails_fast_reflection(client: &ServerReflectionClient) {
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reflection_unavailable(client),
    )
    .await
    .expect("cleared wait-for-ready hung");
}

#[tokio::test]
async fn a_reflection_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client =
        overlay_after_clear_reflection(ServerReflectionClient::connect_lazy(addr).expect("lazy"));
    assert_cleared_wait_fails_fast_reflection(&client).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = overlay_after_clear_reflection(
        ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
    );
    assert_cleared_wait_fails_fast_reflection(&client).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_sees_channel_overlays_after_clear() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = overlay_after_clear_reflection(
        ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
    );
    assert_cleared_wait_fails_fast_reflection(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_sees_channel_overlays_after_clear() {
    let path = unix_sock("reflect-overlay-clear");
    let client = overlay_after_clear_reflection(
        ServerReflectionClient::connect_unix_lazy(&path).expect("lazy"),
    );
    assert_cleared_wait_fails_fast_reflection(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_sees_channel_overlays_after_clear() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = overlay_after_clear_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_reflection_list(&client).await;
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
async fn a_reflection_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_reflection_list(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let path = unix_sock("reflect-gzip-reapply");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.send_compressed().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = unix_client(&path)
        .await
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_reflection_list(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .send_compressed()
        .intercept(reapply_channel_gzip);
    gzip_reflection_list(&client).await;
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

fn stacked_trace_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client
        .intercept(interceptor_insert_trace)
        .intercept(interceptor_stamp_trace)
}

#[tokio::test]
async fn reflection_client_interceptors_stack_and_share_extensions() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_trace)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&stacked_trace_reflection(client(addr).await)).await;
}

#[tokio::test]
async fn reflection_tls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&stacked_trace_reflection(tls_client(addr).await)).await;
}

#[tokio::test]
async fn reflection_mtls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&stacked_trace_reflection(
        tls_client_with(addr, client_tls).await,
    ))
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_client_interceptors_stack_and_share_extensions() {
    let path = unix_sock("reflection-stack-trace");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_trace)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&stacked_trace_reflection(unix_client(&path).await)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_client_interceptors_stack_and_share_extensions() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_trace)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&stacked_trace_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
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

async fn assert_reflection_err(client: &ServerReflectionClient, want: Code) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
}

async fn echo_tenant_reflection(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(with_tenant(Request::new(())));
    let mut inbound = call.await.expect("open").into_inner();
    tx.send(list_req()).await.expect("send");
    let resp = inbound
        .message()
        .await
        .expect("read")
        .expect("reflection reply");
    assert!(
        resp.has_list_services_response(),
        "expected list, got error {:?}",
        resp.error_response().error_message()
    );
    let names = service_names(&resp);
    assert!(
        names.contains(&"helloworld.Greeter".to_owned()),
        "{names:?}"
    );
    tx.close();
    assert_reflection_err(client, Code::Internal).await;
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

fn user_agent_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
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

fn override_ua_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
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

fn limits_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client
        .message_limits(test_message_limits())
        .intercept(interceptor_require_limits)
}

#[tokio::test]
async fn a_reflection_client_interceptor_reads_caller_extensions() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_tenant)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_tenant_reflection(&client(addr).await.intercept(interceptor_stamp_tenant)).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_tenant_reflection(&tls_client(addr).await.intercept(interceptor_stamp_tenant)).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_tenant_reflection(
        &tls_client_with(addr, client_tls)
            .await
            .intercept(interceptor_stamp_tenant),
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_reads_caller_extensions() {
    let path = unix_sock("reflection-tenant");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_tenant)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_tenant_reflection(&unix_client(&path).await.intercept(interceptor_stamp_tenant)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_reads_caller_extensions() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_tenant)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_tenant_reflection(
        &ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .intercept(interceptor_stamp_tenant),
    )
    .await;
}

#[tokio::test]
async fn a_reflection_client_interceptor_sees_the_user_agent() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&user_agent_reflection(client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&user_agent_reflection(tls_client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&user_agent_reflection(
        tls_client_with(addr, client_tls).await,
    ))
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_sees_the_user_agent() {
    let path = unix_sock("reflection-ua");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&user_agent_reflection(unix_client(&path).await)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_sees_the_user_agent() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&user_agent_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
}

#[tokio::test]
async fn a_reflection_client_interceptor_sets_the_user_agent() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_override_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&override_ua_reflection(client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_sets_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_override_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&override_ua_reflection(tls_client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_sets_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_override_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&override_ua_reflection(
        tls_client_with(addr, client_tls).await,
    ))
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_sets_the_user_agent() {
    let path = unix_sock("reflection-ua-set");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_override_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&override_ua_reflection(unix_client(&path).await)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_sets_the_user_agent() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_override_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&override_ua_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
}

#[tokio::test]
async fn a_reflection_client_interceptor_sees_message_limits() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection.serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&limits_reflection(client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&limits_reflection(tls_client(addr).await)).await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&limits_reflection(tls_client_with(addr, client_tls).await)).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_sees_message_limits() {
    let path = unix_sock("reflection-limits");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&limits_reflection(unix_client(&path).await)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_sees_message_limits() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&limits_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
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

fn reserved_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client.intercept(interceptor_reserved_metadata)
}

fn hop_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client.intercept(interceptor_hop_by_hop)
}

fn fail_open_reflection(client: ServerReflectionClient) -> ServerReflectionClient {
    client.intercept(interceptor_fail_before_open)
}

#[tokio::test]
async fn a_reflection_client_interceptor_cannot_insert_reserved_metadata() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection.serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &reserved_reflection(client(addr).await),
        Code::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &reserved_reflection(tls_client(addr).await),
        Code::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_err(
        &reserved_reflection(tls_client_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_cannot_insert_reserved_metadata() {
    let path = unix_sock("reflection-reserved");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &reserved_reflection(unix_client(&path).await),
        Code::InvalidArgument,
    )
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_cannot_insert_reserved_metadata() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &reserved_reflection(
            ServerReflectionClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection.serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(&hop_reflection(client(addr).await), Code::InvalidArgument).await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &hop_reflection(tls_client(addr).await),
        Code::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_err(
        &hop_reflection(tls_client_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let path = unix_sock("reflection-hop");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &hop_reflection(unix_client(&path).await),
        Code::InvalidArgument,
    )
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &hop_reflection(
            ServerReflectionClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection.serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &fail_open_reflection(client(addr).await),
        Code::FailedPrecondition,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &fail_open_reflection(tls_client(addr).await),
        Code::FailedPrecondition,
    )
    .await;
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_err(
        &fail_open_reflection(tls_client_with(addr, client_tls).await),
        Code::FailedPrecondition,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let path = unix_sock("reflection-fail-open");
    let sock = path.clone();
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &fail_open_reflection(unix_client(&path).await),
        Code::FailedPrecondition,
    )
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_err(
        &fail_open_reflection(
            ServerReflectionClient::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::FailedPrecondition,
    )
    .await;
}

fn intercept_counts_create_reflection(
    client: ServerReflectionClient,
    ran: &Arc<AtomicUsize>,
) -> ServerReflectionClient {
    let flag = Arc::clone(ran);
    client.intercept(move |_: &mut Outgoing<'_>| {
        flag.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

fn assert_interceptors_run_on_create_reflection(
    client: &ServerReflectionClient,
    ran: &Arc<AtomicUsize>,
) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "ServerReflectionInfo interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);
}

#[tokio::test]
async fn reflection_client_interceptors_run_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_reflection(
        ServerReflectionClient::connect_lazy(addr).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_reflection(&client, &ran);
}

#[tokio::test]
async fn a_reflection_tls_client_interceptor_runs_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = intercept_counts_create_reflection(
        ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_reflection(&client, &ran);
}

#[tokio::test]
async fn a_reflection_mtls_client_interceptor_runs_when_the_call_is_created() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = intercept_counts_create_reflection(
        ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_reflection(&client, &ran);
}

#[cfg(unix)]
#[tokio::test]
async fn a_reflection_unix_client_interceptor_runs_when_the_call_is_created() {
    let path = unix_sock("reflection-on-create");
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_reflection(
        ServerReflectionClient::connect_unix_lazy(&path).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_reflection(&client, &ran);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reflection_from_io_client_interceptor_runs_when_the_call_is_created() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_reflection(
        ServerReflectionClient::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &ran,
    );
    assert_interceptors_run_on_create_reflection(&client, &ran);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reflection_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client = ServerReflectionClient::connect_lazy(addr).expect("lazy");
    assert_reflection_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_tls_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reflection_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_tls_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_reflection_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_mtls_request_can_opt_out_of_channel_wait_for_ready() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reflection_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_mtls_wait_for_ready_times_out_when_nothing_is_listening() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = ServerReflectionClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_reflection_wait_deadline(&client).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflection_unix_request_can_opt_out_of_channel_wait_for_ready() {
    let path = unix_sock("opt-out");
    let client = ServerReflectionClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reflection_opt_out(&client))
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
async fn reflection_unix_wait_for_ready_times_out_when_nothing_is_listening() {
    let path = unix_sock("deadline");
    let client = ServerReflectionClient::connect_unix_lazy(&path).expect("lazy");
    assert_reflection_wait_deadline(&client).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_send_compressed_gzips_list_services() {
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = client(addr).await.send_compressed();
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_tls_send_compressed_gzips_list_services() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let mut last = None;
    let client = {
        let mut found = None;
        for _ in 0..80 {
            match ServerReflectionClient::connect_tls(addr, client_tls.clone()).await {
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
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_mtls_send_compressed_gzips_list_services() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls).await.send_compressed();
    gzip_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_tls_interceptor_rejects_with_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr).await;
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_tls_client_interceptor_rejects_with_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_tls_client_interceptor_sees_list_services_context() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr).await.intercept(stamp_outgoing_context);
    echo_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_mtls_interceptor_rejects_with_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(|_rpc: &mut pbrs_grpc::Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls).await;
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_mtls_client_interceptor_rejects_with_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

#[tokio::test]
async fn reflection_mtls_client_interceptor_sees_list_services_context() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client_with(addr, client_tls)
        .await
        .intercept(stamp_outgoing_context);
    echo_reflection_list(&client).await;
}

#[tokio::test]
async fn reflection_handlers_return_typed_status() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflection)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_blocked(&client(addr).await).await;
}

#[tokio::test]
async fn reflection_handlers_return_from_error_details() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflectionFromErrorDetails)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_blocked(&client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_handlers_return_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflection)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_blocked(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_handlers_return_typed_status() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflection)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_blocked(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_handlers_return_typed_status() {
    let path = unix_sock("typed-handler");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflection)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_blocked(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_handlers_return_typed_status() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(FailReflection)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_blocked(&client).await;
}

#[tokio::test]
async fn reflection_typed_google_rpc_status_after_a_streamed_message() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(TypedAfterHeadersReflection)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_typed_status_after_streamed_message(&client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_typed_google_rpc_status_after_a_streamed_message() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::new(identity).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(TypedAfterHeadersReflection)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_typed_status_after_streamed_message(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_typed_google_rpc_status_after_a_streamed_message() {
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let tls = ServerTls::mtls(identity, CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(TypedAfterHeadersReflection)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_typed_status_after_streamed_message(&tls_client_with(addr, client_tls).await)
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_typed_google_rpc_status_after_a_streamed_message() {
    let path = unix_sock("typed-after-headers");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(TypedAfterHeadersReflection)
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_typed_status_after_streamed_message(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_typed_google_rpc_status_after_a_streamed_message() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        ServerReflectionServer::new(TypedAfterHeadersReflection)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_typed_status_after_streamed_message(&client).await;
}

async fn assert_reflection_file_lookups(client: &ServerReflectionClient) {
    let resp = ask(client, symbol_req("helloworld.Greeter")).await;
    assert!(
        resp.has_file_descriptor_response(),
        "expected file, got error {:?}",
        resp.error_response().error_message()
    );
    let files = resp.file_descriptor_response().file_descriptor_proto();
    assert!(!files.is_empty(), "missing FileDescriptorProto");
    let joined: Vec<u8> = files.iter().flat_map(|b| b.as_bytes().to_vec()).collect();
    let haystack = String::from_utf8_lossy(&joined);
    assert!(
        haystack.contains("Greeter") || haystack.contains("helloworld"),
        "descriptor should name the service"
    );

    let missing = ask(client, symbol_req("nope.Missing")).await;
    assert!(missing.has_error_response());
    assert_eq!(
        missing.error_response().error_code(),
        Code::NotFound.to_i32()
    );

    let by_symbol = ask(client, symbol_req("helloworld.HelloRequest")).await;
    assert!(by_symbol.has_file_descriptor_response());
    let first = by_symbol
        .file_descriptor_response()
        .file_descriptor_proto()
        .get(0)
        .expect("file");
    let by_name = ask(client, filename_req("hello.proto")).await;
    assert!(
        by_name.has_file_descriptor_response(),
        "hello.proto: {:?}",
        by_name.error_response().error_message()
    );
    assert!(!by_name
        .file_descriptor_response()
        .file_descriptor_proto()
        .is_empty());
    assert!(!first.as_bytes().is_empty());
}

async fn assert_reflection_extensions(client: &ServerReflectionClient) {
    let resp = ask(client, ext_req("demo.ext.Host", 100)).await;
    assert!(
        resp.has_file_descriptor_response(),
        "expected file, got error {:?}",
        resp.error_response().error_message()
    );
    assert!(!resp
        .file_descriptor_response()
        .file_descriptor_proto()
        .is_empty());

    let missing = ask(client, ext_req("demo.ext.Host", 199)).await;
    assert!(missing.has_error_response());
    assert_eq!(
        missing.error_response().error_code(),
        Code::NotFound.to_i32()
    );

    let nums = ask(client, ext_numbers_req("demo.ext.Host")).await;
    assert!(
        nums.has_all_extension_numbers_response(),
        "expected numbers, got error {:?}",
        nums.error_response().error_message()
    );
    let tags: Vec<i32> = nums
        .all_extension_numbers_response()
        .extension_number()
        .iter()
        .collect();
    assert_eq!(tags, vec![100]);
}

#[tokio::test]
async fn reflection_tls_file_lookups() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_file_lookups(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_file_lookups() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_file_lookups(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_file_lookups() {
    let path = unix_sock("file-lookup");
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_file_lookups(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_file_lookups() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_file_lookups(&client).await;
}

#[tokio::test]
async fn reflection_tls_extension_lookups() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let reflection = service([ext::FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_extensions(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_extension_lookups() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let reflection = service([ext::FILE_DESCRIPTOR_SET]).expect("reflection");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_extensions(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_extension_lookups() {
    let path = unix_sock("ext-lookup");
    let reflection = service([ext::FILE_DESCRIPTOR_SET]).expect("reflection");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection.serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_extensions(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_extension_lookups() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let reflection = service([ext::FILE_DESCRIPTOR_SET]).expect("reflection");
    let handle = tokio::spawn(async move {
        reflection.serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_extensions(&client).await;
}

async fn assert_reflection_oversize(client: &ServerReflectionClient) {
    let mut fat = ServerReflectionRequest::new();
    fat.set_file_containing_symbol("k".repeat(64));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    tx.send(fat).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("oversize reflection request must fail as trailers"),
        },
    }
}

fn reflection_oversize_router() -> Router {
    Router::new()
        .max_decoding_message_size(16)
        .add_service(service([FILE_DESCRIPTOR_SET]).expect("reflection"))
}

#[tokio::test]
async fn reflection_tls_oversize_request_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_oversize_request_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_oversize(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_oversize_request_is_resource_exhausted() {
    let path = unix_sock("oversize");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_oversize_router().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_oversize_request_is_resource_exhausted() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_oversize_router()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_oversize(&client).await;
}

async fn assert_reflection_client_encode_cap(client: &ServerReflectionClient) {
    let mut fat = ServerReflectionRequest::new();
    fat.set_file_containing_symbol("k".repeat(64));
    let (tx, call) = client.server_reflection_info(Request::new(()));
    let err = tx.send(fat).await.expect_err("send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
}

async fn assert_reflection_client_decode_cap(client: &ServerReflectionClient) {
    let (tx, call) = client.server_reflection_info(Request::new(()));
    tx.send(list_req()).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("reflection client decode cap must fail"),
        },
    }
}

async fn assert_reflection_client_message_caps(client: ServerReflectionClient) {
    assert_reflection_client_encode_cap(&client.clone().max_encoding_message_size(16)).await;
    assert_reflection_client_decode_cap(&client.clone().max_decoding_message_size(16)).await;
    assert_reflection_client_encode_cap(
        &client
            .clone()
            .message_limits(MessageLimits::new().with_max_encoding(16)),
    )
    .await;
    assert_reflection_client_decode_cap(
        &client.message_limits(MessageLimits::new().with_max_decoding(16)),
    )
    .await;
}

#[tokio::test]
async fn reflection_client_message_caps_are_resource_exhausted() {
    let (addr, _guard) = serve().await;
    assert_reflection_client_message_caps(client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_client_message_caps(tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_client_message_caps(tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_client_message_caps_are_resource_exhausted() {
    let path = unix_sock("client-caps");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_server().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_client_message_caps(unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_client_message_caps_are_resource_exhausted() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_server().serve_connection(server_io).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_reflection_client_message_caps(client).await;
}

fn reflection_decode_limits() -> MessageLimits {
    MessageLimits::new().with_max_decoding(16)
}

fn reflection_oversize_limits_router() -> Router {
    Router::new()
        .message_limits(reflection_decode_limits())
        .add_service(service([FILE_DESCRIPTOR_SET]).expect("reflection"))
}

fn reflection_oversize_limits_server() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().message_limits(reflection_decode_limits())
}

#[tokio::test]
async fn reflection_message_limits_oversize_is_resource_exhausted() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_router()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&client(addr).await).await;
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_server()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_message_limits_oversize_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&tls_client(addr).await).await;
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_message_limits_oversize_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_oversize(&tls_client_with(addr, client_tls).await).await;
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_oversize(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_message_limits_oversize_is_resource_exhausted() {
    let path = unix_sock("msg-limits");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_router()
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
    let path = unix_sock("msg-limits-srv");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_server()
            .serve_unix(sock)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_oversize(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_message_limits_oversize_is_resource_exhausted() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_router()
            .serve_connection(s1)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(c1, "localhost")
        .await
        .expect("from_io router");
    assert_reflection_oversize(&client).await;
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_oversize_limits_server()
            .serve_connection(s2)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io(c2, "localhost")
        .await
        .expect("from_io server");
    assert_reflection_oversize(&client).await;
}

fn reflection_dial_encode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(MessageLimits::new().with_max_encoding(16))
}

fn reflection_dial_decode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(MessageLimits::new().with_max_decoding(16))
}

async fn reflection_cfg(addr: SocketAddr, cfg: ChannelConfig) -> ServerReflectionClient {
    let mut last = None;
    for _ in 0..80 {
        match ServerReflectionClient::connect_with(addr, cfg).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn reflection_tls_cfg(
    addr: SocketAddr,
    tls: ClientTls,
    cfg: ChannelConfig,
) -> ServerReflectionClient {
    let mut last = None;
    for _ in 0..80 {
        match ServerReflectionClient::connect_tls_with(addr, cfg, tls.clone()).await {
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
async fn reflection_unix_cfg(path: &std::path::Path, cfg: ChannelConfig) -> ServerReflectionClient {
    let mut last = None;
    for _ in 0..80 {
        match ServerReflectionClient::connect_unix_with(path, cfg).await {
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
async fn reflection_channel_config_message_limits_are_resource_exhausted() {
    let (addr, _guard) = serve().await;
    assert_reflection_client_encode_cap(
        &reflection_cfg(addr, reflection_dial_encode_limits()).await,
    )
    .await;
    assert_reflection_client_decode_cap(
        &reflection_cfg(addr, reflection_dial_decode_limits()).await,
    )
    .await;
}

#[tokio::test]
async fn reflection_tls_channel_config_message_limits_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_reflection_client_encode_cap(
        &reflection_tls_cfg(addr, client_tls.clone(), reflection_dial_encode_limits()).await,
    )
    .await;
    assert_reflection_client_decode_cap(
        &reflection_tls_cfg(addr, client_tls, reflection_dial_decode_limits()).await,
    )
    .await;
}

#[tokio::test]
async fn reflection_mtls_channel_config_message_limits_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        reflection_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_client_encode_cap(
        &reflection_tls_cfg(addr, client_tls.clone(), reflection_dial_encode_limits()).await,
    )
    .await;
    assert_reflection_client_decode_cap(
        &reflection_tls_cfg(addr, client_tls, reflection_dial_decode_limits()).await,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_channel_config_message_limits_are_resource_exhausted() {
    let path = unix_sock("dial-limits");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_server().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_client_encode_cap(
        &reflection_unix_cfg(&path, reflection_dial_encode_limits()).await,
    )
    .await;
    assert_reflection_client_decode_cap(
        &reflection_unix_cfg(&path, reflection_dial_decode_limits()).await,
    )
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_channel_config_message_limits_are_resource_exhausted() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_server().serve_connection(s1).await.ok();
    });
    let _guard = ServerGuard(handle);
    let encode =
        ServerReflectionClient::from_io_with(c1, "localhost", reflection_dial_encode_limits())
            .await
            .expect("from_io encode");
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_server().serve_connection(s2).await.ok();
    });
    let _guard = ServerGuard(handle);
    let decode =
        ServerReflectionClient::from_io_with(c2, "localhost", reflection_dial_decode_limits())
            .await
            .expect("from_io decode");
    assert_reflection_client_encode_cap(&encode).await;
    assert_reflection_client_decode_cap(&decode).await;
}

fn reflection_header_list_cap() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().max_header_list_size(1024)
}

fn flood_reflection() -> Request<()> {
    let mut request = Request::new(());
    request
        .metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    request
}

async fn assert_reflection_header_flood_then_echo(
    flood: ServerReflectionClient,
    healthy: ServerReflectionClient,
) {
    let (tx, call) = flood.server_reflection_info(flood_reflection());
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(2), call).await;
    echo_reflection_list(&healthy).await;
}

#[tokio::test]
async fn reflection_header_list_cap_refuses_oversize_metadata() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_header_list_cap()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_header_flood_then_echo(client(addr).await, client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_header_flood_then_echo(tls_client(addr).await, tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reflection_header_flood_then_echo(
        tls_client_with(addr, client_tls.clone()).await,
        tls_client_with(addr, client_tls).await,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_header_list_cap_refuses_oversize_metadata() {
    let path = unix_sock("hdr-list");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_header_list_cap().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    assert_reflection_header_flood_then_echo(unix_client(&path).await, unix_client(&path).await)
        .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_header_list_cap_refuses_oversize_metadata() {
    let (c1, s1) = tokio::io::duplex(1024 * 1024);
    let flood_handle = tokio::spawn(async move {
        reflection_header_list_cap().serve_connection(s1).await.ok();
    });
    let _flood_guard = ServerGuard(flood_handle);
    let flood = ServerReflectionClient::from_io(c1, "localhost")
        .await
        .expect("from_io flood");
    let (c2, s2) = tokio::io::duplex(1024 * 1024);
    let healthy_handle = tokio::spawn(async move {
        reflection_header_list_cap().serve_connection(s2).await.ok();
    });
    let _healthy_guard = ServerGuard(healthy_handle);
    let healthy = ServerReflectionClient::from_io(c2, "localhost")
        .await
        .expect("from_io healthy");
    assert_reflection_header_flood_then_echo(flood, healthy).await;
}

fn reflection_conn_cap() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().max_concurrent_connections(1)
}

fn reflection_pool_against_cap() -> ChannelConfig {
    ChannelConfig::new()
        .connect_timeout(Duration::from_millis(300))
        .connections(2)
}

fn reflection_pool_cfg() -> ChannelConfig {
    ChannelConfig::new().connections(2)
}

async fn assert_reflection_cap_refuses_then_echo(
    first: ServerReflectionClient,
    second: Result<ServerReflectionClient, Status>,
    reconnect: impl std::future::Future<Output = ServerReflectionClient>,
) {
    let err = second.expect_err("pool larger than the accept-loop cap should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    echo_reflection_list(&reconnect.await).await;
}

#[tokio::test]
async fn reflection_pool_against_cap_is_unavailable() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_conn_cap().serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    let first = client(addr).await;
    assert_reflection_cap_refuses_then_echo(
        first,
        ServerReflectionClient::connect_with(addr, reflection_pool_against_cap()).await,
        client(addr),
    )
    .await;
}

#[tokio::test]
async fn tls_reflection_pool_against_cap_is_unavailable() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = tls_client_with(addr, client_tls.clone()).await;
    assert_reflection_cap_refuses_then_echo(
        first,
        ServerReflectionClient::connect_tls_with(
            addr,
            reflection_pool_against_cap(),
            client_tls.clone(),
        )
        .await,
        tls_client_with(addr, client_tls),
    )
    .await;
}

#[tokio::test]
async fn mtls_reflection_pool_against_cap_is_unavailable() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = tls_client_with(addr, client_tls.clone()).await;
    assert_reflection_cap_refuses_then_echo(
        first,
        ServerReflectionClient::connect_tls_with(
            addr,
            reflection_pool_against_cap(),
            client_tls.clone(),
        )
        .await,
        tls_client_with(addr, client_tls),
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reflection_pool_against_cap_is_unavailable() {
    let path = unix_sock("pool-cap");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_conn_cap().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    let first = unix_client(&path).await;
    assert_reflection_cap_refuses_then_echo(
        first,
        ServerReflectionClient::connect_unix_with(&path, reflection_pool_against_cap()).await,
        unix_client(&path),
    )
    .await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_reflection_pool_config_is_still_one_duplex() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_server().serve_connection(s).await.ok();
    });
    let _guard = ServerGuard(handle);
    let client = ServerReflectionClient::from_io_with(c, "localhost", reflection_pool_cfg())
        .await
        .expect("from_io");
    echo_reflection_list(&client).await;
}

fn reflection_frame_size() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().max_frame_size(16 * 1024)
}

#[tokio::test]
async fn reflection_frame_size_still_serves_list() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_frame_size().serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&client(addr).await).await;
}

#[tokio::test]
async fn reflection_tls_frame_size_still_serves_list() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&tls_client(addr).await).await;
}

#[tokio::test]
async fn reflection_mtls_frame_size_still_serves_list() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn reflection_unix_frame_size_still_serves_list() {
    let path = unix_sock("frame-size");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_frame_size().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn reflection_from_io_frame_size_still_serves_list() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_frame_size().serve_connection(s).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(
        &ServerReflectionClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
}

fn reflection_pending_reset() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().max_pending_accept_reset_streams(1)
}

#[tokio::test]
async fn reflection_pending_reset_still_serves_list() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_pending_reset()
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&client(addr).await).await;
}

#[tokio::test]
async fn tls_reflection_pending_reset_still_serves_list() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_pending_reset()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&tls_client(addr).await).await;
}

#[tokio::test]
async fn mtls_reflection_pending_reset_still_serves_list() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_pending_reset()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reflection_pending_reset_still_serves_list() {
    let path = unix_sock("reflection-pending-reset");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_pending_reset().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_reflection_pending_reset_still_serves_list() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_pending_reset().serve_connection(s).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(
        &ServerReflectionClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
}

fn reflection_send_buffer() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server().max_send_buffer_size(16 * 1024)
}

#[tokio::test]
async fn reflection_send_buffer_still_serves_list() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_send_buffer().serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&client(addr).await).await;
}

#[tokio::test]
async fn tls_reflection_send_buffer_still_serves_list() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_send_buffer()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&tls_client(addr).await).await;
}

#[tokio::test]
async fn mtls_reflection_send_buffer_still_serves_list() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_send_buffer()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reflection_send_buffer_still_serves_list() {
    let path = unix_sock("reflection-send-buffer");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_send_buffer().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_reflection_send_buffer_still_serves_list() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_send_buffer().serve_connection(s).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(
        &ServerReflectionClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
}

fn reflection_window_size() -> ServerReflectionServer<impl ServerReflection> {
    reflection_server()
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(128 * 1024)
}

#[tokio::test]
async fn reflection_window_size_still_serves_list() {
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_window_size().serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&client(addr).await).await;
}

#[tokio::test]
async fn tls_reflection_window_size_still_serves_list() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_window_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&tls_client(addr).await).await;
}

#[tokio::test]
async fn mtls_reflection_window_size_still_serves_list() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind_reflection().await;
    let handle = tokio::spawn(async move {
        reflection_window_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reflection_list(&tls_client_with(addr, client_tls).await).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reflection_window_size_still_serves_list() {
    let path = unix_sock("reflection-window-size");
    let sock = path.clone();
    let handle = tokio::spawn(async move {
        reflection_window_size().serve_unix(sock).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(&unix_client(&path).await).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn from_io_reflection_window_size_still_serves_list() {
    let (c, s) = tokio::io::duplex(1024 * 1024);
    let handle = tokio::spawn(async move {
        reflection_window_size().serve_connection(s).await.ok();
    });
    let _guard = ServerGuard(handle);
    echo_reflection_list(
        &ServerReflectionClient::from_io(c, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
}
