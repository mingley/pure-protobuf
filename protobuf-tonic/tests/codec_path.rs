//! Existing tonic-prost-build pipelines can use pbrs messages by configuration.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    missing_docs,
    unreachable_pub,
    reason = "integration test and tonic-prost-build generated fixture"
)]

use futures_util::StreamExt;
use std::net::SocketAddr;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

mod messages {
    pub use protobuf_tonic::hello::{HelloReply, HelloRequest};
}

#[allow(
    clippy::allow_attributes_without_reason,
    reason = "unmodified tonic-prost-build output owns its lint attributes"
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/external-tonic/helloworld.rs"));
}

use generated::greeter_client::GreeterClient;
use generated::greeter_server::{Greeter, GreeterServer};
use messages::{HelloReply, HelloRequest};

#[derive(Default)]
struct Echo;

#[tonic::async_trait]
impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(
            request
                .into_inner()
                .name()
                .to_str()
                .unwrap_or("")
                .to_owned(),
        );
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("not needed by this fixture"))
    }

    type ServerHelloStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("not needed by this fixture"))
    }

    type StreamHelloStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        let replies = request.into_inner().map(|request| {
            request.map(|request| {
                let mut reply = HelloReply::new();
                reply.set_message(request.name().to_str().unwrap_or("").to_owned());
                reply
            })
        });
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            tokio::pin!(replies);
            while let Some(reply) = replies.next().await {
                if tx.send(reply).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn spawn_server() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("server address");
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Echo))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("serve codec_path fixture");
    });
    address
}

fn request(name: &str) -> HelloRequest {
    let mut request = HelloRequest::new();
    request.set_name(name.to_owned());
    request
}

#[tokio::test]
async fn external_pbrs_messages_round_trip_unary_and_streaming() {
    let address = spawn_server().await;
    let mut client = GreeterClient::connect(format!("http://{address}"))
        .await
        .expect("connect client");

    let unary = client
        .say_hello(request("unary"))
        .await
        .expect("unary call")
        .into_inner();
    assert_eq!(unary.message().to_str(), Ok("unary"));

    let input = tokio_stream::iter([request("one"), request("two")]);
    let mut output = client
        .stream_hello(input)
        .await
        .expect("bidirectional streaming call")
        .into_inner();
    assert_eq!(
        output
            .message()
            .await
            .expect("first stream message")
            .expect("first reply")
            .message()
            .to_str(),
        Ok("one")
    );
    assert_eq!(
        output
            .message()
            .await
            .expect("second stream message")
            .expect("second reply")
            .message()
            .to_str(),
        Ok("two")
    );
    assert!(output.message().await.expect("stream end").is_none());
}
