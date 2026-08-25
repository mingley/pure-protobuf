//! Unary Greeter with tonic gzip message compression.

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
use std::net::SocketAddr;
use tonic::codec::CompressionEncoding;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

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
        Err(Status::unimplemented("gzip unary"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("gzip unary"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("gzip unary"))
    }
}

async fn echo_once(addr: SocketAddr) -> String {
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel)
        .send_compressed(CompressionEncoding::Gzip)
        .accept_compressed(CompressionEncoding::Gzip);
    let resp = client.say_hello(Request::new(req)).await.expect("unary");
    resp.into_inner()
        .message()
        .to_str()
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn gzip_unary_echo_twice() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(
                GreeterServer::new(Echo)
                    .accept_compressed(CompressionEncoding::Gzip)
                    .send_compressed(CompressionEncoding::Gzip),
            )
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let a = echo_once(addr).await;
    let b = echo_once(addr).await;
    assert_eq!(a, "ada");
    assert_eq!(b, "ada");
    println!("ok {a}");
    println!("ok {b}");
}
