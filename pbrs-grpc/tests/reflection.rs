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

use common::{Echo, ServerGuard};
use pbrs_grpc::hello::{GreeterServer, FILE_DESCRIPTOR_SET};
use pbrs_grpc::reflection::{
    service, ExtensionRequest, ServerReflection, ServerReflectionClient, ServerReflectionRequest,
    ServerReflectionResponse, ServerReflectionServer,
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
