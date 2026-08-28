//! Native kernel message-size caps: oversize is RESOURCE_EXHAUSTED.

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

use common::{name_of, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Inbound, Request, Response, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const OVERSIZE: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const LIMIT: usize = 8;
const UNDER: usize = 1024;

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
        _request: Request<HelloRequest>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("message_size"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("message_size"))
    }
}

async fn spawn(
    server_max_dec: Option<usize>,
    server_max_enc: Option<usize>,
) -> Result<(SocketAddr, Channel), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        let mut srv = GreeterServer::new(Echo);
        if let Some(n) = server_max_dec {
            srv = srv.max_decoding_message_size(n);
        }
        if let Some(n) = server_max_enc {
            srv = srv.max_encoding_message_size(n);
        }
        srv.serve_listener(listener).await.ok();
    }));
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(ch) => return Ok((addr, ch)),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

#[tokio::test]
async fn under_limit_unary_still_works() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch)
        .max_decoding_message_size(UNDER)
        .max_encoding_message_size(UNDER);
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("under-limit unary");
    assert_eq!(name_of(&resp.into_inner()), "ada");
}

#[tokio::test]
async fn oversize_outbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_encoding_message_size(LIMIT);
    match client.say_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted),
        Ok(_) => panic!("oversize outbound must fail"),
    }
}

#[tokio::test]
async fn oversize_inbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_decoding_message_size(LIMIT);
    match client.say_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted),
        Ok(_) => panic!("oversize inbound must fail"),
    }
}

#[tokio::test]
async fn channel_oversize_outbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let ch = ch.max_encoding_message_size(LIMIT);
    match ch
        .unary::<HelloRequest, HelloReply>(SAY_HELLO, Request::new(req(OVERSIZE)))
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted),
        Ok(_) => panic!("channel outbound must fail"),
    }
}

#[tokio::test]
async fn server_oversize_decode_is_resource_exhausted() {
    let (_addr, ch) = spawn(Some(LIMIT), None).await.expect("spawn");
    let client = GreeterClient::new(ch);
    match client.say_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted),
        Ok(_) => panic!("server decode must fail"),
    }
}

#[tokio::test]
async fn server_oversize_encode_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, Some(LIMIT)).await.expect("spawn");
    let client = GreeterClient::new(ch);
    match client.say_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted),
        Ok(_) => panic!("server encode must fail"),
    }
}

#[tokio::test]
async fn client_stream_oversize_send_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_encoding_message_size(LIMIT);
    let (tx, call) = client.client_hello(Request::new(()));
    let err = tx.send(req(OVERSIZE)).await.expect_err("stream send");
    assert_eq!(err.code(), Code::ResourceExhausted);
    drop(call);
}
