//! Same-process tonic analogues of official gRPC interop cases.
//!
//! Uses `hello.proto` / generated Greeter stubs and `ProtobufCodec`.
//! Not `grpc.testing.TestService`, and not a second HTTP/2 stack.

use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use protobuf_tonic::ProtobufCodec;
use std::net::SocketAddr;
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status};

/// Official gRPC interop `special_status_message` text (code 2 / Unknown).
/// Tabs, CR, LF, BMP ☺ (U+263A), and non-BMP 😈 (U+1F608) must survive.
const SPECIAL_STATUS_MESSAGE: &str =
    "\t\ntest with whitespace\r\nand Unicode BMP ☺ and non-BMP 😈\t\n";

/// Missing-style server: unary handler returns a non-OK Status.
struct SpecialStatus;

impl Greeter for SpecialStatus {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unknown(SPECIAL_STATUS_MESSAGE))
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

/// Official interop `large_unary` sizes (`grpc-go` / gRPC interop).
/// hello.proto has strings, not `SimpleRequest.payload.body`, so these
/// are UTF-8 field lengths (`name` / `message`), not exact wire sizes.
const LARGE_REQ: usize = 271828;
const LARGE_RESP: usize = 314159;

/// Echo-style Greeter: empty `HelloRequest` yields an empty `HelloReply`.
/// A `LARGE_REQ`-sized name gets a `LARGE_RESP`-sized reply (not an echo).
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
        if name.len() == LARGE_REQ {
            reply.set_message("x".repeat(LARGE_RESP));
        } else {
            reply.set_message(name);
        }
        Ok(Response::new(reply))
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

async fn spawn_echo() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Echo))
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

/// Official interop `special_status_message`: server Status message keeps
/// leading/trailing whitespace and non-ASCII exactly.
#[tokio::test]
async fn special_status_message() {
    let addr = spawn_greeter().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let err = client
        .say_hello(Request::new(HelloRequest::new()))
        .await
        .expect_err("expected special status");
    assert_eq!(err.code(), Code::Unknown);
    assert_eq!(err.message(), SPECIAL_STATUS_MESSAGE);
}

/// Official interop `empty_unary`: empty request, empty reply.
/// On hello.proto this is `HelloRequest` / `HelloReply` with empty strings.
#[tokio::test]
async fn empty_unary() {
    let addr = spawn_echo().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let resp = client
        .say_hello(Request::new(HelloRequest::new()))
        .await
        .expect("empty_unary");
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "");
}

/// Official interop `large_unary`: one-shot SayHello with a large payload.
/// Request `name` is `LARGE_REQ` bytes; reply `message` is `LARGE_RESP`.
#[tokio::test]
async fn large_unary() {
    let addr = spawn_echo().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("x".repeat(LARGE_REQ));
    assert_eq!(req.name().as_bytes().len(), LARGE_REQ);
    let resp = client
        .say_hello(Request::new(req))
        .await
        .expect("large_unary");
    let reply = resp.into_inner();
    assert_eq!(reply.message().as_bytes().len(), LARGE_RESP);
}
