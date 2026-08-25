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
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Code, InItem, Inbound, Request, Response, Status};

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
        request: Request<Inbound<HelloRequest>>,
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
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let (tx, rx) = Inbound::channel(4);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                let mut reply = HelloReply::new();
                reply.set_message(part.to_string());
                if tx
                    .send(Ok(InItem {
                        message: reply,
                        compressed: false,
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(Response::new(rx))
    }

    async fn stream_hello(
        &self,
        request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = Inbound::channel(4);
        drop(tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                let mut reply = HelloReply::new();
                reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                if tx
                    .send(Ok(InItem {
                        message: reply,
                        compressed: false,
                    }))
                    .await
                    .is_err()
                {
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
        _request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("fail"))
    }
}

#[tokio::test]
async fn unary_echoes_name() {
    let (_addr, client) = spawn_greeter(Echo).await.expect("spawn");
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(&resp.into_inner()), "ada");
}

#[tokio::test]
async fn client_stream_aggregates_names() {
    let (_addr, client) = spawn_greeter(Echo).await.expect("spawn");
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.send(req("bob")).await.expect("send");
    tx.close();
    let resp = call.await.expect("client-stream");
    assert_eq!(name_of(&resp.into_inner()), "ada,bob");
}

#[tokio::test]
async fn server_stream_splits_name() {
    let (_addr, client) = spawn_greeter(Echo).await.expect("spawn");
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
async fn bidi_round_trip() {
    let (_addr, client) = spawn_greeter(Echo).await.expect("spawn");
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
    let (_addr, client) = spawn_greeter(Fail).await.expect("spawn");
    match client.say_hello(Request::new(req("ada"))).await {
        Err(err) => {
            assert_ne!(err.code(), Code::Ok);
            assert_eq!(err.code(), Code::NotFound);
        }
        Ok(_) => panic!("expected nonzero grpc-status"),
    }
}
