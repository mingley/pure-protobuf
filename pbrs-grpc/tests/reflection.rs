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
    service, ExtensionRequest, ServerReflectionClient, ServerReflectionRequest,
    ServerReflectionResponse,
};
use pbrs_grpc::{Channel, Code, Outgoing, Request, Router, Status};
use std::net::SocketAddr;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

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
