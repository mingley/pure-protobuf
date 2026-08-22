//! Same-process tonic analogues of official gRPC interop cases.
//!
//! Uses `hello.proto` / generated Greeter stubs and `ProtobufCodec`.
//! Not `grpc.testing.TestService`, and not a second HTTP/2 stack.

use protobuf_tonic::hello::{Greeter, GreeterServer, HelloReply, HelloRequest};
use protobuf_tonic::ProtobufCodec;
use std::net::SocketAddr;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status};

/// Missing-style server used as a registered Greeter for path probes.
struct SpecialStatus;

impl Greeter for SpecialStatus {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(HelloReply::new()))
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("interop dummy"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("interop dummy"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("interop dummy"))
    }
}

async fn spawn_greeter() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(SpecialStatus))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

async fn connect(addr: SocketAddr) -> tonic::client::Grpc<Channel> {
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    tonic::client::Grpc::new(channel)
}

async fn unary_at(
    grpc: &mut tonic::client::Grpc<Channel>,
    path: &str,
) -> Result<Response<HelloReply>, Status> {
    grpc.ready()
        .await
        .map_err(|e| Status::unknown(e.to_string()))?;
    grpc.unary(
        Request::new(HelloRequest::new()),
        path.parse().unwrap(),
        ProtobufCodec::<HelloRequest, HelloReply>::default(),
    )
    .await
}

/// Official interop `unimplemented_method`: path is on Greeter's service,
/// but the method is not one of SayHello / ClientHello / ServerHello / StreamHello.
#[tokio::test]
async fn unimplemented_method() {
    let addr = spawn_greeter().await;
    let mut grpc = connect(addr).await;
    let err = unary_at(&mut grpc, "/helloworld.Greeter/UnimplementedCall")
        .await
        .expect_err("expected unimplemented method");
    assert_eq!(err.code(), Code::Unimplemented);
    // Generated GreeterServer catch-all sets grpc-status only; no grpc-message.
    assert_eq!(err.message(), "");
}

/// Official interop `unimplemented_service`: service name is not registered
/// on the tonic server (only `helloworld.Greeter` is).
#[tokio::test]
async fn unimplemented_service() {
    let addr = spawn_greeter().await;
    let mut grpc = connect(addr).await;
    let err = unary_at(
        &mut grpc,
        "/grpc.testing.UnimplementedService/UnimplementedCall",
    )
    .await
    .expect_err("expected unimplemented service");
    assert_eq!(err.code(), Code::Unimplemented);
    // tonic router fallback also sets grpc-status only.
    assert_eq!(err.message(), "");
}
