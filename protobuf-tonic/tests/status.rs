use futures_util::StreamExt;
use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use std::net::SocketAddr;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status, Streaming};

struct Missing;

impl Greeter for Missing {
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
        Err(Status::not_found(format!("no such user: {name}")))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut inbound = request.into_inner();
        let name = match inbound.next().await {
            Some(Ok(msg)) => msg.name().to_str().unwrap_or("").to_string(),
            _ => String::new(),
        };
        Err(Status::not_found(format!("no such user: {name}")))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        Err(Status::not_found(format!("no such user: {name}")))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("status test"))
    }
}

async fn spawn_missing() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Missing))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn unary_not_found_code_and_message() {
    let addr = spawn_missing().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let err = client
        .say_hello(Request::new(req))
        .await
        .expect_err("expected non-OK status");
    assert_eq!(err.code(), Code::NotFound);
    assert_eq!(err.message(), "no such user: ada");
}

#[tokio::test]
async fn client_streaming_not_found_code_and_message() {
    let addr = spawn_missing().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    tx.send(req).await.unwrap();
    drop(tx);
    let err = client
        .client_hello(Request::new(ReceiverStream::new(rx)))
        .await
        .expect_err("expected non-OK status");
    assert_eq!(err.code(), Code::NotFound);
    assert_eq!(err.message(), "no such user: ada");
}

#[tokio::test]
async fn server_streaming_not_found_code_and_message() {
    let addr = spawn_missing().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let err = client
        .server_hello(Request::new(req))
        .await
        .expect_err("expected non-OK status");
    assert_eq!(err.code(), Code::NotFound);
    assert_eq!(err.message(), "no such user: ada");
}
