//! Same-process tonic analogues of official gRPC interop cases.
//!
//! Uses `hello.proto` / generated Greeter stubs and `ProtobufCodec`.
//! Not `grpc.testing.TestService`, and not a second HTTP/2 stack.

use futures_util::StreamExt;
use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use protobuf_tonic::ProtobufCodec;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataValue;
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
        request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = inbound.next().await {
                let mut reply = HelloReply::new();
                reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                if tx.send(Ok(reply)).await.is_err() {
                    break;
                }
            }
            // Empty inbound: no send, then drop(tx) => empty outbound, OK.
        });
        Ok(Response::new(ReceiverStream::new(rx)))
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

/// Official interop `empty_stream`: FullDuplexCall with no messages.
/// Client opens StreamHello and half-closes (`drop(tx)`) before any send.
/// Server sends nothing (`drop(tx)` after empty inbound). Client sees
/// zero replies and stream end (OK), not a `Status` error.
#[tokio::test]
async fn empty_stream() {
    let addr = spawn_echo().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let mut stream = client
        .stream_hello(Request::new(ReceiverStream::new(rx)))
        .await
        .expect("empty_stream should start OK");
    drop(tx);
    let mut replies = 0usize;
    while let Some(item) = stream.get_mut().next().await {
        item.expect("empty_stream must complete OK, not Status");
        replies += 1;
    }
    assert_eq!(replies, 0);
}

/// Holds ClientHello / StreamHello open so the client can cancel.
struct Hang;

impl Greeter for Hang {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("cancel hang"))
    }

    async fn client_hello(
        &self,
        request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut inbound = request.into_inner();
        // Wait for a message or inbound end. tonic maps a request-stream
        // cancel (`Code::Cancelled`) to `None`, same as half-close.
        match inbound.next().await {
            Some(Ok(msg)) => {
                let mut reply = HelloReply::new();
                reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                Ok(Response::new(reply))
            }
            Some(Err(status)) => Err(status),
            None => {
                let mut reply = HelloReply::new();
                reply.set_message("inbound-end");
                Ok(Response::new(reply))
            }
        }
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("cancel hang"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            while let Some(item) = inbound.next().await {
                match item {
                    Ok(msg) => {
                        let mut reply = HelloReply::new();
                        reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                        if tx.send(Ok(reply)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Stay open after the first echo until inbound ends (or the
            // client drops the response stream).
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn spawn_hang() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Hang))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Official interop `cancel_after_begin`: start StreamingInputCall and
/// cancel before any request messages. ClientHello is the analogue.
///
/// tonic 0.14 has no `call.cancel()` / context. Cancel is abort/drop of
/// the client future (not `drop(tx)`, which is half-close). The aborted
/// task is `JoinError::Cancelled`; the client does not get a `Status`.
#[tokio::test]
async fn cancel_after_begin() {
    let addr = spawn_hang().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let handle = tokio::spawn(async move {
        client
            .client_hello(Request::new(ReceiverStream::new(rx)))
            .await
    });
    // Headers can go out; do not send a request message.
    tokio::time::sleep(Duration::from_millis(30)).await;
    handle.abort();
    let join = handle.await;
    assert!(
        join.unwrap_err().is_cancelled(),
        "tonic cancel-before-send is task abort, not Status"
    );
    drop(tx);
}

/// Official interop `cancel_after_first_response`: FullDuplexCall, recv
/// one message, then cancel. StreamHello is the analogue.
///
/// After the first `HelloReply`, abort the remaining `next()`. tonic
/// 0.14 has no context-cancel + Recv: the client sees
/// `JoinError::Cancelled`, not `Status` / `Code::Cancelled`.
#[tokio::test]
async fn cancel_after_first_response() {
    let addr = spawn_hang().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let mut stream = client
        .stream_hello(Request::new(ReceiverStream::new(rx)))
        .await
        .expect("stream should start");
    let mut req = HelloRequest::new();
    req.set_name("one");
    tx.send(req).await.unwrap();
    let first = stream
        .get_mut()
        .next()
        .await
        .expect("one reply")
        .expect("first HelloReply");
    assert_eq!(first.message().to_str().unwrap_or(""), "one");
    let handle = tokio::spawn(async move { stream.get_mut().next().await });
    handle.abort();
    let join = handle.await;
    assert!(
        join.unwrap_err().is_cancelled(),
        "tonic cancel-after-first is task abort, not Status"
    );
    drop(tx);
}

/// Sleeps past a short client deadline on unary SayHello.
struct Sleeping;

impl Greeter for Sleeping {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut reply = HelloReply::new();
        reply.set_message("late");
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("sleeping"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("sleeping"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("sleeping"))
    }
}

async fn spawn_sleeping() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Sleeping))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Official interop `timeout_on_sleeping_server`: short deadline against
/// a server that sleeps past it. Official wants `DeadlineExceeded`
/// (FullDuplexCall, 1ms).
///
/// `Request::set_timeout` writes `grpc-timeout`. tonic 0.14.6 maps
/// `TimeoutExpired` to `Code::Cancelled` / "Timeout expired", not
/// `DeadlineExceeded`. Unary SayHello so the timeout covers the whole
/// RPC (bidi `set_timeout` only covers stream open).
#[tokio::test]
async fn timeout_on_sleeping_server() {
    let addr = spawn_sleeping().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = Request::new(HelloRequest::new());
    req.set_timeout(Duration::from_millis(50));
    let err = client
        .say_hello(req)
        .await
        .expect_err("expected deadline to fire");
    assert_eq!(err.code(), Code::Cancelled);
    assert_eq!(err.message(), "Timeout expired");
}

/// Official interop `custom_metadata` keys (`grpc` interop-test-descriptions).
const ECHO_INITIAL: &str = "x-grpc-test-echo-initial";
const ECHO_INITIAL_VAL: &str = "test_initial_metadata_value";
const ECHO_TRAILING_BIN: &str = "x-grpc-test-echo-trailing-bin";
const ECHO_TRAILING_BIN_VAL: &[u8] = &[0xab, 0xab, 0xab];

/// Reads request metadata and echoes the ascii key as `Response.metadata`
/// (HTTP/2 headers). The `-bin` key is required so the server actually
/// reads it; it is not copied onto the Response (that would still be
/// headers, not trailers).
struct EchoMetadata;

impl Greeter for EchoMetadata {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let initial = request.metadata().get(ECHO_INITIAL).cloned();
        let trail = request.metadata().get_bin(ECHO_TRAILING_BIN).cloned();
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let Some(trail) = trail else {
            return Err(Status::invalid_argument(
                "missing x-grpc-test-echo-trailing-bin",
            ));
        };
        if trail != MetadataValue::from_bytes(ECHO_TRAILING_BIN_VAL) {
            return Err(Status::invalid_argument(
                "bad x-grpc-test-echo-trailing-bin",
            ));
        }
        let mut reply = HelloReply::new();
        reply.set_message(name);
        let mut response = Response::new(reply);
        if let Some(initial) = initial {
            response.metadata_mut().insert(ECHO_INITIAL, initial);
        }
        // tonic 0.14 Response has no trailers / trailing-metadata API.
        // EncodeBody on Ok emits only Status::ok("") (grpc-status: 0).
        // Err(Status::ok(...)) would be into_http headers, not body
        // trailers after a successful message — do not fake that.
        Ok(response)
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("custom_metadata"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("custom_metadata"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("custom_metadata"))
    }
}

async fn spawn_echo_metadata() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(EchoMetadata))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Official interop `custom_metadata` (unary SayHello first).
/// Client sends ascii + `-bin` request metadata. Server reads
/// `request.metadata()` and echoes the ascii key into
/// `Response.metadata` (initial headers).
#[tokio::test]
async fn custom_metadata() {
    let addr = spawn_echo_metadata().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut body = HelloRequest::new();
    body.set_name("ada");
    let mut req = Request::new(body);
    req.metadata_mut()
        .insert(ECHO_INITIAL, ECHO_INITIAL_VAL.parse().unwrap());
    req.metadata_mut().insert_bin(
        ECHO_TRAILING_BIN,
        MetadataValue::from_bytes(ECHO_TRAILING_BIN_VAL),
    );
    let resp = client
        .say_hello(req)
        .await
        .expect("custom_metadata should succeed");
    let got = resp
        .metadata()
        .get(ECHO_INITIAL)
        .expect("missing echoed initial metadata")
        .to_str()
        .unwrap();
    assert_eq!(got, ECHO_INITIAL_VAL);
    // Official also wants x-grpc-test-echo-trailing-bin as trailing
    // metadata. tonic 0.14 has no Response::trailers(); the unary
    // client merges EncodeBody's OK trailers into this same
    // Response.metadata bag. Those trailers are grpc-status: 0 only.
    // Custom -bin is not attached and is not exposed via extensions.
    assert_eq!(
        resp.metadata()
            .get("grpc-status")
            .map(|v| v.to_str().unwrap()),
        Some("0")
    );
    assert!(
        resp.metadata().get_bin(ECHO_TRAILING_BIN).is_none(),
        "custom OK-path trailers are not a first-class tonic Response API"
    );
    assert!(resp.extensions().is_empty());
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}
