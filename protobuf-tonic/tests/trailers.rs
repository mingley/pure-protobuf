use futures_util::StreamExt;
use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use std::net::SocketAddr;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status, Streaming};

/// Initial response metadata (HTTP/2 headers), not gRPC trailers.
const HEADER_KEY: &str = "x-pbrs-meta";
const HEADER_VAL: &str = "ok";

/// Custom key attached to `Status`; tonic sends this as HTTP/2 trailers.
const TRAILER_KEY: &str = "x-pbrs-trail";
const TRAILER_VAL: &str = "stale";

struct WithHeaders;

impl Greeter for WithHeaders {
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
        let mut response = Response::new(reply);
        response
            .metadata_mut()
            .insert(HEADER_KEY, HEADER_VAL.parse().unwrap());
        Ok(response)
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
        let mut reply = HelloReply::new();
        reply.set_message(name);
        let mut response = Response::new(reply);
        response
            .metadata_mut()
            .insert(HEADER_KEY, HEADER_VAL.parse().unwrap());
        Ok(response)
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
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let mut reply = HelloReply::new();
        reply.set_message(name);
        let _ = tx.send(Ok(reply)).await;
        let mut response = Response::new(ReceiverStream::new(rx));
        response
            .metadata_mut()
            .insert(HEADER_KEY, HEADER_VAL.parse().unwrap());
        Ok(response)
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("headers test"))
    }
}

struct WithStatusTrailers;

impl Greeter for WithStatusTrailers {
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
        let mut status = Status::failed_precondition(format!("not ready: {name}"));
        status
            .metadata_mut()
            .insert(TRAILER_KEY, TRAILER_VAL.parse().unwrap());
        Err(status)
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
        let mut status = Status::failed_precondition(format!("not ready: {name}"));
        status
            .metadata_mut()
            .insert(TRAILER_KEY, TRAILER_VAL.parse().unwrap());
        Err(status)
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
        let mut status = Status::failed_precondition(format!("not ready: {name}"));
        status
            .metadata_mut()
            .insert(TRAILER_KEY, TRAILER_VAL.parse().unwrap());
        Err(status)
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("trailers test"))
    }
}

async fn spawn(svc: impl Greeter) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(svc))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

/// `Response::metadata` is initial metadata (headers), not HTTP/2 trailers.
#[tokio::test]
async fn unary_response_metadata_round_trip() {
    let addr = spawn(WithHeaders).await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let resp = client
        .say_hello(Request::new(req))
        .await
        .expect("unary with initial metadata");
    let got = resp
        .metadata()
        .get(HEADER_KEY)
        .expect("missing response metadata")
        .to_str()
        .unwrap();
    assert_eq!(got, HEADER_VAL);
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}

/// Client-stream `Response::metadata` is initial headers, same as unary.
/// tonic attaches them on the successful `HelloReply`, not before it.
#[tokio::test]
async fn client_streaming_response_metadata_round_trip() {
    let addr = spawn(WithHeaders).await;
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
    let resp = client
        .client_hello(Request::new(ReceiverStream::new(rx)))
        .await
        .expect("client-streaming with initial metadata");
    let got = resp
        .metadata()
        .get(HEADER_KEY)
        .expect("missing response metadata")
        .to_str()
        .unwrap();
    assert_eq!(got, HEADER_VAL);
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}

/// Server-stream initial metadata lives on the `Response` that wraps the
/// stream (HTTP headers), not on a later trailer.
#[tokio::test]
async fn server_streaming_response_metadata_round_trip() {
    let addr = spawn(WithHeaders).await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let resp = client
        .server_hello(Request::new(req))
        .await
        .expect("server-streaming with initial metadata");
    let got = resp
        .metadata()
        .get(HEADER_KEY)
        .expect("missing response metadata")
        .to_str()
        .unwrap();
    assert_eq!(got, HEADER_VAL);
    let msg = resp
        .into_inner()
        .next()
        .await
        .expect("one reply")
        .expect("HelloReply");
    assert_eq!(msg.message().to_str().unwrap_or(""), "ada");
}

/// Non-OK `Status` metadata is sent as gRPC HTTP/2 trailers.
#[tokio::test]
async fn unary_status_trailers_code_message_and_metadata() {
    let addr = spawn(WithStatusTrailers).await;
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
        .expect_err("expected non-OK status with trailers");
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "not ready: ada");
    let got = err
        .metadata()
        .get(TRAILER_KEY)
        .expect("missing status trailers")
        .to_str()
        .unwrap();
    assert_eq!(got, TRAILER_VAL);
}

/// Client-stream non-OK `Status` metadata is HTTP/2 trailers, same as unary.
#[tokio::test]
async fn client_streaming_status_trailers_code_message_and_metadata() {
    let addr = spawn(WithStatusTrailers).await;
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
        .expect_err("expected non-OK status with trailers");
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "not ready: ada");
    let got = err
        .metadata()
        .get(TRAILER_KEY)
        .expect("missing status trailers")
        .to_str()
        .unwrap();
    assert_eq!(got, TRAILER_VAL);
}

/// Server-stream Status fails before a stream (same path as status.rs).
/// Custom metadata is still HTTP/2 trailers on that Err.
#[tokio::test]
async fn server_streaming_status_trailers_code_message_and_metadata() {
    let addr = spawn(WithStatusTrailers).await;
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
        .expect_err("expected non-OK status with trailers");
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "not ready: ada");
    let got = err
        .metadata()
        .get(TRAILER_KEY)
        .expect("missing status trailers")
        .to_str()
        .unwrap();
    assert_eq!(got, TRAILER_VAL);
}
