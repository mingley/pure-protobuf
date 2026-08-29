//! Four Greeter shapes on the shipped kernel client and server.

#![allow(
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
    reason = "integration tests"
)]

mod common;

use common::{name_of, req, spawn_greeter};
use pbrs_grpc::hello::{Greeter, GreeterClient, HelloReply, HelloRequest};
use pbrs_grpc::{Code, Request, Response, Status, Streaming};

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let mut reply = HelloReply::new();
        reply.set_message(name);
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut inbound = request.into_inner();
        let mut names = Vec::new();
        while let Some(msg) = inbound.message().await? {
            names.push(msg.name().to_str().unwrap_or("").to_string());
        }
        let mut reply = HelloReply::new();
        reply.set_message(names.join(","));
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let (tx, rx) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                let mut reply = HelloReply::new();
                reply.set_message(part.to_string());
                if tx.send(reply).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(rx))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                let mut reply = HelloReply::new();
                reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                if tx.send(reply).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(rx))
    }
}

struct Fail;

impl Greeter for Fail {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::not_found("missing"))
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }
}

struct RichFail;

impl Greeter for RichFail {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut status = Status::failed_precondition("quota");
        status.set_details(vec![0x08, 0x09]);
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        Err(status)
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }
}

struct TypedFail;

fn typed_status() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("API_DISABLED");
    info.set_domain("example.com");
    Status::with_error_details(
        Code::FailedPrecondition,
        "api disabled",
        [pbrs_grpc::Any::pack(&info).expect("pack")],
    )
    .expect("encode")
}

impl Greeter for TypedFail {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(typed_status())
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(typed_status())
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(typed_status())
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(typed_status())
    }
}

#[tokio::test]
async fn unary_echoes_name() {
    let (_addr, client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(&resp.into_inner()), "ada");
}

#[tokio::test]
async fn client_stream_aggregates_names() {
    let (_addr, client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.send(req("bob")).await.expect("send");
    tx.close();
    let resp = call.await.expect("client-stream");
    assert_eq!(name_of(&resp.into_inner()), "ada,bob");
}

#[tokio::test]
async fn server_stream_splits_name() {
    let (_addr, client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let resp = client
        .server_hello(Request::new(req("ada,bob")))
        .await
        .expect("server-stream");
    let mut inbound = resp.into_inner();
    let mut got = Vec::new();
    while let Some(msg) = inbound.message().await.expect("msg") {
        got.push(name_of(&msg));
    }
    assert!(
        got.len() > 1,
        "server-stream must yield more than one reply"
    );
    assert_eq!(got, ["ada", "bob"]);
}

#[tokio::test]
async fn server_stream_is_a_futures_stream() {
    use pbrs_grpc::Stream;
    use std::future::poll_fn;
    use std::pin::Pin;

    let (_addr, client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let resp = client
        .server_hello(Request::new(req("ada,bob")))
        .await
        .expect("server-stream");
    let mut inbound = resp.into_inner();
    let mut got = Vec::new();
    while let Some(item) = poll_fn(|cx| Pin::new(&mut inbound).poll_next(cx)).await {
        got.push(name_of(&item.expect("ok")));
    }
    assert_eq!(got, ["ada", "bob"]);
}

#[tokio::test]
async fn bidi_round_trip() {
    let (_addr, client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let resp = call.await.expect("bidi");
    let mut inbound = resp.into_inner();
    let first = inbound
        .message()
        .await
        .expect("msg")
        .expect("at least one bidi reply");
    assert_eq!(name_of(&first), "ada");
}

#[tokio::test]
async fn failing_rpc_nonzero_grpc_status() {
    let (_addr, client, _guard) = spawn_greeter(Fail).await.expect("spawn");
    match client.say_hello(Request::new(req("ada"))).await {
        Err(err) => {
            assert_ne!(err.code(), Code::Ok);
            assert_eq!(err.code(), Code::NotFound);
        }
        Ok(_) => panic!("expected nonzero grpc-status"),
    }
}

#[tokio::test]
async fn failing_rpc_carries_status_details() {
    let (_addr, client, _guard) = spawn_greeter(RichFail).await.expect("spawn");
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("details");
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "quota");
    assert_eq!(err.details(), &[0x08, 0x09]);
    assert_eq!(err.metadata().get("x-retry-after"), Some("30"));
    assert!(err.metadata().get_bin("grpc-status-details-bin").is_none());
}

#[tokio::test]
async fn failing_rpc_carries_typed_google_rpc_status() {
    let (_addr, client, _guard) = spawn_greeter(TypedFail).await.expect("spawn");
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("details");
    assert_eq!(err.code(), Code::FailedPrecondition);
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
}

fn assert_typed_fail(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "api disabled");
    let info = err
        .error_details()
        .expect("ErrorDetails")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
}

fn typed_stream_status() -> Status {
    let mut status = typed_status();
    status
        .metadata_mut()
        .insert("x-retry-after", "30")
        .expect("md");
    status
}

fn assert_typed_stream_fail(err: &Status) {
    assert_typed_fail(err);
    assert_eq!(err.metadata().get("x-retry-after"), Some("30"));
}

fn fail_after_one() -> Streaming<HelloReply> {
    let (tx, stream) = Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut reply = HelloReply::new();
        reply.set_message("ada");
        tx.send(reply).await.ok();
        tx.fail(typed_stream_status()).await;
    }));
    stream
}

struct TypedAfterHeaders;

impl Greeter for TypedAfterHeaders {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Ok(Response::new(fail_after_one()))
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Ok(Response::new(fail_after_one()))
    }
}

#[tokio::test]
async fn typed_google_rpc_status_on_every_call_shape() {
    let (_addr, client, _guard) = spawn_greeter(TypedFail).await.expect("spawn");

    assert_typed_fail(
        &client
            .say_hello(Request::new(req("ada")))
            .await
            .expect_err("unary"),
    );

    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    assert_typed_fail(&call.await.expect_err("client-stream"));

    assert_typed_fail(
        &client
            .server_hello(Request::new(req("ada")))
            .await
            .expect_err("server-stream"),
    );

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    assert_typed_fail(&call.await.expect_err("bidi"));
}

#[tokio::test]
async fn typed_google_rpc_status_after_a_streamed_message() {
    let (_addr, client, _guard) = spawn_greeter(TypedAfterHeaders).await.expect("spawn");

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_stream_fail(&stream.message().await.expect_err("status"));

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_stream_fail(&stream.message().await.expect_err("status"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_unary_on_connection_pool() {
    let (addr, _client, _guard) = spawn_greeter(Echo).await.expect("spawn");
    let client = GreeterClient::connect_pool(addr, 4).await.expect("pool");
    let mut hs = Vec::new();
    for i in 0..16u32 {
        let c = client.clone();
        hs.push(tokio::spawn(async move {
            let label = format!("n{i}");
            let resp = c.say_hello(Request::new(req(&label))).await.expect("unary");
            name_of(&resp.into_inner())
        }));
    }
    let mut got = Vec::new();
    for h in hs {
        got.push(h.await.expect("join"));
    }
    got.sort();
    let mut want: Vec<String> = (0..16u32).map(|i| format!("n{i}")).collect();
    want.sort();
    assert_eq!(got, want);
}
