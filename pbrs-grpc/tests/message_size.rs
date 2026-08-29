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

use common::{name_of, req, Echo};
use pbrs_grpc::hello::{GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const OVERSIZE: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
const LIMIT: usize = 8;
const UNDER: usize = 1024;

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

fn assert_exhausted(err: &Status) {
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
}

async fn echo_under(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert!(inbound.message().await.expect("end").is_none());
}

async fn oversize_unary(client: &GreeterClient) {
    match client.say_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_exhausted(&err),
        Ok(_) => panic!("oversize unary must fail"),
    }
}

async fn oversize_server_stream(client: &GreeterClient) {
    match client.server_hello(Request::new(req(OVERSIZE))).await {
        Err(err) => assert_exhausted(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_exhausted(&err),
            Ok(_) => panic!("oversize server-stream must fail"),
        },
    }
}

async fn oversize_client_stream_send(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    let err = tx.send(req(OVERSIZE)).await.expect_err("send");
    assert_exhausted(&err);
    drop(call);
}

async fn oversize_client_stream_call(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req(OVERSIZE)).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_exhausted(&err),
        Ok(_) => panic!("oversize client-stream call must fail"),
    }
}

async fn oversize_bidi_send(client: &GreeterClient) {
    let (tx, call) = client.stream_hello(Request::new(()));
    let err = tx.send(req(OVERSIZE)).await.expect_err("send");
    assert_exhausted(&err);
    drop(call);
}

async fn oversize_bidi_call_or_item(client: &GreeterClient) {
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req(OVERSIZE)).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_exhausted(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_exhausted(&err),
            Ok(_) => panic!("oversize bidi must fail"),
        },
    }
}

async fn oversize_outbound_every_shape(client: &GreeterClient) {
    oversize_unary(client).await;
    oversize_server_stream(client).await;
    oversize_client_stream_send(client).await;
    oversize_bidi_send(client).await;
}

async fn oversize_after_send_every_shape(client: &GreeterClient) {
    oversize_unary(client).await;
    oversize_server_stream(client).await;
    oversize_client_stream_call(client).await;
    oversize_bidi_call_or_item(client).await;
}

#[tokio::test]
async fn under_limit_unary_still_works() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch)
        .max_decoding_message_size(UNDER)
        .max_encoding_message_size(UNDER);
    echo_under(&client).await;
}

#[tokio::test]
async fn oversize_outbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_encoding_message_size(LIMIT);
    oversize_outbound_every_shape(&client).await;
}

#[tokio::test]
async fn oversize_inbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_decoding_message_size(LIMIT);
    oversize_after_send_every_shape(&client).await;
}

#[tokio::test]
async fn channel_oversize_outbound_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let ch = ch.max_encoding_message_size(LIMIT);
    match ch
        .unary::<HelloRequest, HelloReply>(SAY_HELLO, Request::new(req(OVERSIZE)))
        .await
    {
        Err(err) => assert_exhausted(&err),
        Ok(_) => panic!("channel outbound must fail"),
    }
}

#[tokio::test]
async fn server_oversize_decode_is_resource_exhausted() {
    let (_addr, ch) = spawn(Some(LIMIT), None).await.expect("spawn");
    let client = GreeterClient::new(ch);
    oversize_after_send_every_shape(&client).await;
}

#[tokio::test]
async fn server_oversize_encode_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, Some(LIMIT)).await.expect("spawn");
    let client = GreeterClient::new(ch);
    oversize_after_send_every_shape(&client).await;
}

#[tokio::test]
async fn client_stream_oversize_send_is_resource_exhausted() {
    let (_addr, ch) = spawn(None, None).await.expect("spawn");
    let client = GreeterClient::new(ch).max_encoding_message_size(LIMIT);
    oversize_client_stream_send(&client).await;
}
