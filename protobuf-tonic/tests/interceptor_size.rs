//! Generated stubs: request interceptor and encode/decode size bounds.

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
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]
use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status};

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
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("interceptor_size"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("interceptor_size"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("interceptor_size"))
    }
}

fn attach_trace(mut req: Request<()>) -> Result<Request<()>, Status> {
    req.metadata_mut()
        .insert("x-pbrs-trace", "ada".parse().unwrap());
    Ok(req)
}

fn require_trace(req: Request<()>) -> Result<Request<()>, Status> {
    match req
        .metadata()
        .get("x-pbrs-trace")
        .and_then(|v| v.to_str().ok())
    {
        Some("ada") => Ok(req),
        _ => Err(Status::unauthenticated("missing x-pbrs-trace")),
    }
}

fn hello(name: &str) -> HelloRequest {
    let mut req = HelloRequest::new();
    req.set_name(name);
    req
}

async fn connect(addr: std::net::SocketAddr) -> Channel {
    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect")
}

#[tokio::test]
async fn interceptor_unary_rpc() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::with_interceptor(Echo, require_trace))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut client = GreeterClient::with_interceptor(connect(addr).await, attach_trace);
    let resp = client
        .say_hello(Request::new(hello("ada")))
        .await
        .expect("interceptor rpc");
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}

#[tokio::test]
async fn message_size_bounds_unary_rpc() {
    let limit = 1024 * 1024;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(
                GreeterServer::new(Echo)
                    .max_decoding_message_size(limit)
                    .max_encoding_message_size(limit),
            )
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut client = GreeterClient::new(connect(addr).await)
        .max_decoding_message_size(limit)
        .max_encoding_message_size(limit);
    let resp = client
        .say_hello(Request::new(hello("ada")))
        .await
        .expect("bounded rpc");
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}

#[tokio::test]
async fn message_size_bound_rejects_oversize() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Echo).max_decoding_message_size(8))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut client = GreeterClient::new(connect(addr).await);
    let err = client
        .say_hello(Request::new(hello(&"x".repeat(64))))
        .await
        .expect_err("oversize must fail");
    assert_eq!(err.code(), Code::OutOfRange);
}
