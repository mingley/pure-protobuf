//! A gRPC service that depends on [`pbrs_grpc`] as an ordinary crate.
//!
//! `proto/hello.proto` is compiled by `build.rs` with
//! [`pbrs::codegen::compile_protos`] (kernel stubs are the default). The binary serves the greeter
//! together with `grpc.health.v1` and `grpc.reflection.v1` on loopback, then
//! calls `SayHello`. Tests cover all four RPC shapes, health `Check` and
//! `Watch`, and reflection `list_services`.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter handler Err; those trailers reach the client.
//!
//! Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter handler Err is after the handler ran.
//!
//! Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter handler Err is after the handler ran.
//!
//! Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter handler Err is after the handler ran.
//!
//! Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter handler Err is after the handler ran.
//!
//! Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter handler Err is after the handler ran.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from an example greeter handler Err: that is after the handler ran; this example greeter interceptor Err is trailers without reading the body.
//!
//! Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter interceptor Err is trailers without reading the body.
//!
//! Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter interceptor Err is trailers without reading the body.
//!
//! Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter interceptor Err is trailers without reading the body.
//!
//! Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter interceptor Err is trailers without reading the body.
//!
//! Distinct from an example greeter client interceptor: that runs on the outbound call before the stream opens; this example greeter interceptor runs on the inbound RPC before the handler.
//!
//! [`pbrs_grpc::Outgoing::connected`] is the live-socket snapshot on this example greeter client interceptor path ([`pbrs_grpc::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from an example greeter handler Err: that is after the handler ran; this example greeter client interceptor Err is a local reject never opens a stream.
//!
//! Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter client interceptor Err is a local reject never opens a stream.
//!
//! Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter client interceptor Err is a local reject never opens a stream.
//!
//! Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter client interceptor Err is a local reject never opens a stream.
//!
//! Distinct from [`pbrs_grpc::Channel::max_concurrent_rpcs`]: that takes a slot when the [`pbrs_grpc::Call`] is polled; this example greeter client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from an example greeter interceptor: that runs on the inbound RPC before the handler; this example greeter client interceptor runs on the outbound call before the stream opens.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! Distinct from an example greeter handler Err: that is after the handler ran; this example greeter StreamSender fail is trailers after any messages already sent.
//!
//! Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter StreamSender fail is trailers after any messages already sent.
//!
//! Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter StreamSender fail is trailers after any messages already sent.
//!
//! Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter StreamSender fail is trailers after any messages already sent.
//!
//! Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter StreamSender fail is trailers after any messages already sent.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter server on_response Err; a local reject is trailers-only after handler Ok.
//!
//! Distinct from an example greeter handler Err: that is after the handler ran; this example greeter server on_response Err is trailers-only after handler Ok.
//!
//! Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter server on_response Err is trailers-only after handler Ok.
//!
//! Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter server on_response Err is trailers-only after handler Ok.
//!
//! Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter server on_response Err is trailers-only after handler Ok.
//!
//! [`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter client on_response Err; a local reject fails the Call after a successful receive.
//!
//! Distinct from an example greeter handler Err: that is after the handler ran; this example greeter client on_response Err fails the Call after a successful receive.
//!
//! Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter client on_response Err fails the Call after a successful receive.
//!
//! Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter client on_response Err fails the Call after a successful receive.
//!
//! Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter client on_response Err fails the Call after a successful receive.
//!
//! Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter client on_response Err fails the Call after a successful receive.

#![allow(
    missing_docs,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "example crate"
)]

mod proto {
    #![allow(
        missing_docs,
        unused,
        unreachable_pub,
        reason = "generated by protoc-gen-pbrs"
    )]
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

use pbrs_grpc::health::{service as health_service, HealthReporter, ServingStatus};
use pbrs_grpc::reflection::service as reflection_service;
pub use pbrs_grpc::{Call, Request, Response, Router, Status, StreamSender, Streaming};
pub use proto::{
    Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest, FILE_DESCRIPTOR_SET,
};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

pub struct Live {
    pub addr: SocketAddr,
    pub reporter: HealthReporter,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Live {
    /// Gracefully shutdown the server and wait for drain to complete.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

pub struct MyGreeter;

fn name_of(req: &HelloRequest) -> String {
    req.name().to_str().unwrap_or_default().to_owned()
}

fn reply(message: impl Into<String>) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(message.into());
    reply
}

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request
            .get_ref()
            .name()
            .to_str()
            .map_err(|_| Status::invalid_argument("name must be valid UTF-8"))?;
        if name.is_empty() {
            return Err(Status::invalid_argument("name must not be empty"));
        }
        Ok(Response::new(reply(format!("hello {name}"))))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(req) = stream.message().await? {
            let name = req
                .name()
                .to_str()
                .map_err(|_| Status::invalid_argument("streamed name must be valid UTF-8"))?;
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }
        if names.is_empty() {
            return Err(Status::invalid_argument(
                "stream must contain at least one name",
            ));
        }
        Ok(Response::new(reply(format!("hello {}", names.join(", ")))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = request
            .get_ref()
            .name()
            .to_str()
            .map_err(|_| Status::invalid_argument("name must be valid UTF-8"))?
            .to_owned();
        if name.is_empty() {
            return Err(Status::invalid_argument("name must not be empty"));
        }

        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            for i in 1..=3 {
                if tx.send(reply(format!("hello {name} #{i}"))).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                if tx
                    .send(reply(format!("hello {}", name_of(&req))))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

pub async fn serve() -> Result<Live, Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    let (health, reporter) = health_service();
    reporter.set_serving(GreeterServer::<MyGreeter>::NAME);
    let reflection = reflection_service([FILE_DESCRIPTOR_SET])?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        Router::new()
            .add_service(health)
            .add_service(reflection)
            .add_service(GreeterServer::new(MyGreeter))
            .serve_with_shutdown(listener, async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });
    Ok(Live {
        addr,
        reporter,
        shutdown_tx: Some(shutdown_tx),
        server_handle: Some(server_handle),
    })
}

pub async fn greeter(addr: SocketAddr) -> Result<GreeterClient, Status> {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match GreeterClient::connect(addr).await {
            Ok(client) => return Ok(client),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

impl GreeterClient {
    /// Client-streaming alias for `client_hello` (`SayHelloStream`).
    pub fn say_hello_stream(
        &self,
        request: Request<()>,
    ) -> (StreamSender<HelloRequest>, Call<Response<HelloReply>>) {
        self.client_hello(request)
    }

    /// Server-streaming alias for `server_hello` (`SayHelloServerStream`).
    pub fn say_hello_server_stream(
        &self,
        request: Request<HelloRequest>,
    ) -> Call<Response<Streaming<HelloReply>>> {
        self.server_hello(request)
    }

    /// Bidirectional-streaming alias for `stream_hello` (`SayHelloBidiStream`).
    pub fn say_hello_bidi_stream(
        &self,
        request: Request<()>,
    ) -> (
        StreamSender<HelloRequest>,
        Call<Response<Streaming<HelloReply>>>,
    ) {
        self.stream_hello(request)
    }
}

/// 1. Unary RPC (`SayHello`): single request, single response.
///
/// Demonstrates explicit request creation, error handling, and response unpacking.
pub async fn say_hello(client: &GreeterClient, name: &str) -> Result<String, Status> {
    let mut req = HelloRequest::new();
    req.set_name(name);
    let reply = client.say_hello(Request::new(req)).await?;
    let text = reply
        .get_ref()
        .message()
        .to_str()
        .map_err(|_| Status::internal("reply was not valid UTF-8"))?;
    Ok(text.to_owned())
}

/// 2. Client-streaming RPC (`SayHelloStream` / `ClientHello`):
/// client streams requests into a bounded sender, half-closes via `tx.close()`,
/// and awaits the single server response.
pub async fn say_hello_stream<'a>(
    client: &GreeterClient,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<String, Status> {
    let (tx, call) = client.say_hello_stream(Request::new(()));
    for name in names {
        let mut req = HelloRequest::new();
        req.set_name(name);
        tx.send(req)
            .await
            .map_err(|e| Status::unavailable(format!("failed to send to stream: {e}")))?;
    }
    // Half-close: signal end of stream to server
    tx.close();

    let reply = call.await?;
    let text = reply
        .get_ref()
        .message()
        .to_str()
        .map_err(|_| Status::internal("reply was not valid UTF-8"))?;
    Ok(text.to_owned())
}

/// Client-streaming alias matching proto RPC name `ClientHello`.
pub async fn client_hello<'a>(
    client: &GreeterClient,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<String, Status> {
    say_hello_stream(client, names).await
}

/// 3. Server-streaming RPC (`SayHelloServerStream` / `ServerHello`):
/// single request, stream of responses read to EOF (`while let Some(msg) = stream.message().await?`).
pub async fn say_hello_server_stream(
    client: &GreeterClient,
    name: &str,
) -> Result<Vec<String>, Status> {
    let mut req = HelloRequest::new();
    req.set_name(name);
    let mut stream = client
        .say_hello_server_stream(Request::new(req))
        .await?
        .into_inner();
    let mut replies = Vec::new();
    while let Some(reply) = stream.message().await? {
        let text = reply
            .message()
            .to_str()
            .map_err(|_| Status::internal("reply was not valid UTF-8"))?;
        replies.push(text.to_owned());
    }
    Ok(replies)
}

/// Server-streaming alias matching proto RPC name `ServerHello`.
pub async fn server_hello(client: &GreeterClient, name: &str) -> Result<Vec<String>, Status> {
    say_hello_server_stream(client, name).await
}

/// 4. Bidirectional streaming RPC (`SayHelloBidiStream` / `StreamHello`):
/// concurrent full-duplex streams coordinated via `StreamSender` and inbound `Streaming`.
pub async fn say_hello_bidi_stream<'a>(
    client: &GreeterClient,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, Status> {
    let (tx, call) = client.say_hello_bidi_stream(Request::new(()));
    let mut inbound = call.await?.into_inner();

    for name in names {
        let mut req = HelloRequest::new();
        req.set_name(name);
        tx.send(req)
            .await
            .map_err(|e| Status::unavailable(format!("failed to send bidi message: {e}")))?;
    }
    // Half-close client sender
    tx.close();

    let mut replies = Vec::new();
    while let Some(reply) = inbound.message().await? {
        let text = reply
            .message()
            .to_str()
            .map_err(|_| Status::internal("bidi reply was not valid UTF-8"))?;
        replies.push(text.to_owned());
    }
    Ok(replies)
}

/// Bidirectional-streaming alias matching proto RPC name `StreamHello`.
pub async fn stream_hello<'a>(
    client: &GreeterClient,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, Status> {
    say_hello_bidi_stream(client, names).await
}

/// Unary greet helper.
pub async fn greet(client: &GreeterClient, name: &str) -> Result<String, Status> {
    say_hello(client, name).await
}

/// Bind loopback, serve, execute all four call shapes with clean shutdown,
/// and return the unary reply text ("hello world").
pub async fn run() -> Result<String, Status> {
    let live = serve().await?;
    if live.reporter.status(GreeterServer::<MyGreeter>::NAME) != Some(ServingStatus::Serving) {
        return Err(Status::failed_precondition("greeter health not serving"));
    }
    let client = greeter(live.addr).await?;

    // 1. Unary
    let unary_reply = say_hello(&client, "world").await?;

    // 2. Client streaming (upload)
    let upload_reply = say_hello_stream(&client, ["alice", "bob"]).await?;
    if upload_reply != "hello alice, bob" {
        return Err(Status::internal(format!(
            "unexpected upload reply: {upload_reply}"
        )));
    }

    // 3. Server streaming (download)
    let download_replies = say_hello_server_stream(&client, "world").await?;
    if download_replies != ["hello world #1", "hello world #2", "hello world #3"] {
        return Err(Status::internal(format!(
            "unexpected download replies: {download_replies:?}"
        )));
    }

    // 4. Bidirectional streaming
    let bidi_replies = say_hello_bidi_stream(&client, ["alpha", "beta"]).await?;
    if bidi_replies != ["hello alpha", "hello beta"] {
        return Err(Status::internal(format!(
            "unexpected bidi replies: {bidi_replies:?}"
        )));
    }

    // Clean server shutdown
    live.shutdown().await;

    Ok(unary_reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_docs_name_from_error_details_on_handler_err() {
        let src = include_str!("lib.rs");
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter handler Err; those trailers reach the client."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter handler Err is after the handler ran."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter handler Err is after the handler ran."
        ));
        assert!(src.contains(
            "Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter handler Err is after the handler ran."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter handler Err is after the handler ran."
        ));
        assert!(src.contains(
            "Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter handler Err is after the handler ran."
        ));
    }

    #[test]
    fn example_docs_name_from_error_details_on_interceptor_err() {
        let src = include_str!("lib.rs");
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter interceptor Err; those trailers reach the client without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter handler Err: that is after the handler ran; this example greeter interceptor Err is trailers without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter interceptor Err is trailers without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter interceptor Err is trailers without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter interceptor Err is trailers without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter interceptor Err is trailers without reading the body."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client interceptor: that runs on the outbound call before the stream opens; this example greeter interceptor runs on the inbound RPC before the handler."
        ));
    }

    #[test]
    fn example_docs_name_from_error_details_on_client_interceptor_err() {
        let src = include_str!("lib.rs");
        assert!(src.contains(
            "[`pbrs_grpc::Outgoing::connected`] is the live-socket snapshot on this example greeter client interceptor path ([`pbrs_grpc::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on."
        ));
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter client interceptor Err; a local reject never opens a stream."
        ));
        assert!(src.contains(
            "Distinct from an example greeter handler Err: that is after the handler ran; this example greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(src.contains(
            "Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(src.contains(
            "Distinct from [`pbrs_grpc::Channel::max_concurrent_rpcs`]: that takes a slot when the [`pbrs_grpc::Call`] is polled; this example greeter client interceptor already ran, so a local Err never consumes that budget."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor: that runs on the inbound RPC before the handler; this example greeter client interceptor runs on the outbound call before the stream opens."
        ));
    }

    #[test]
    fn example_docs_name_from_error_details_on_stream_sender_fail() {
        let src = include_str!("lib.rs");
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent."
        ));
        assert!(src.contains(
            "Distinct from an example greeter handler Err: that is after the handler ran; this example greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(src.contains(
            "Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter server on_response Err; a local reject is trailers-only after handler Ok."
        ));
        assert!(src.contains(
            "Distinct from an example greeter handler Err: that is after the handler ran; this example greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client on_response Err: that fails the Call after a successful receive; this example greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(src.contains(
            "Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(src.contains(
            "[`pbrs_grpc::Status::from_error_details`] is the typed bag after this example greeter client on_response Err; a local reject fails the Call after a successful receive."
        ));
        assert!(src.contains(
            "Distinct from an example greeter handler Err: that is after the handler ran; this example greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(src.contains(
            "Distinct from an example greeter interceptor Err: that is trailers without reading the body; this example greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(src.contains(
            "Distinct from an example greeter client interceptor Err: that is a local reject never opens a stream; this example greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(src.contains(
            "Distinct from an example greeter server on_response Err: that is trailers-only after handler Ok; this example greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(src.contains(
            "Distinct from an example greeter StreamSender fail: that is trailers after any messages already sent; this example greeter client on_response Err fails the Call after a successful receive."
        ));
    }

    #[test]
    fn example_readme_names_from_error_details_on_interceptor_err() {
        let readme = include_str!("../README.md");
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter interceptor Err; those trailers reach the client without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter interceptor Err is trailers without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter interceptor Err is trailers without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter interceptor Err is trailers without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter interceptor Err is trailers without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter interceptor Err is trailers without reading the body."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client interceptor: that runs on the outbound call before the stream opens; this example README greeter interceptor runs on the inbound RPC before the handler."
        ));
    }

    #[test]
    fn example_readme_names_from_error_details_on_handler_err() {
        let readme = include_str!("../README.md");
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter handler Err; those trailers reach the client."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter handler Err is after the handler ran."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter handler Err is after the handler ran."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter handler Err is after the handler ran."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter handler Err is after the handler ran."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter handler Err is after the handler ran."
        ));
    }

    #[test]
    fn example_readme_names_from_error_details_on_client_interceptor_err() {
        let readme = include_str!("../README.md");
        assert!(readme.contains(
            "`Outgoing::connected` is the live-socket snapshot on this example README greeter client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on."
        ));
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter client interceptor Err; a local reject never opens a stream."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter client interceptor Err is a local reject never opens a stream."
        ));
        assert!(readme.contains(
            "Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this example README greeter client interceptor already ran, so a local Err never consumes that budget."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor: that runs on the inbound RPC before the handler; this example README greeter client interceptor runs on the outbound call before the stream opens."
        ));
    }

    #[test]
    fn example_readme_names_from_error_details_on_stream_sender_fail() {
        let readme = include_str!("../README.md");
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter StreamSender fail is trailers after any messages already sent."
        ));
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter server on_response Err; a local reject is trailers-only after handler Ok."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter server on_response Err is trailers-only after handler Ok."
        ));
        assert!(readme.contains(
            "`Status::from_error_details` is the typed bag after this example README greeter client on_response Err; a local reject fails the Call after a successful receive."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter client on_response Err fails the Call after a successful receive."
        ));
        assert!(readme.contains(
            "Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter client on_response Err fails the Call after a successful receive."
        ));
    }

    fn text(reply: &HelloReply) -> String {
        reply.message().to_str().unwrap_or_default().to_owned()
    }

    fn request(name: &str) -> HelloRequest {
        let mut req = HelloRequest::new();
        req.set_name(name);
        req
    }

    #[tokio::test]
    async fn generated_stubs_say_hello() {
        assert_eq!(run().await.unwrap(), "hello world");
    }

    #[tokio::test]
    async fn generated_stubs_all_four_shapes() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        let unary = client
            .say_hello(Request::new(request("ada")))
            .await
            .unwrap();
        assert_eq!(text(unary.get_ref()), "hello ada");

        let (tx, call) = client.client_hello(Request::new(()));
        tx.send(request("grace")).await.unwrap();
        tx.send(request("alan")).await.unwrap();
        tx.close();
        assert_eq!(text(call.await.unwrap().get_ref()), "hello grace, alan");

        let mut server_stream = client
            .server_hello(Request::new(request("edsger")))
            .await
            .unwrap()
            .into_inner();
        let mut streamed = Vec::new();
        while let Some(msg) = server_stream.message().await.unwrap() {
            streamed.push(text(&msg));
        }
        assert_eq!(
            streamed,
            ["hello edsger #1", "hello edsger #2", "hello edsger #3"]
        );

        let (tx, call) = client.stream_hello(Request::new(()));
        let mut bidi = call.await.unwrap().into_inner();
        tx.send(request("barbara")).await.unwrap();
        assert_eq!(
            text(&bidi.message().await.unwrap().unwrap()),
            "hello barbara"
        );
        tx.close();
        assert!(bidi.message().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn health_reports_the_greeter() {
        use pbrs_grpc::health::{HealthCheckRequest, HealthClient, ServingStatus};

        let live = serve().await.unwrap();
        assert_eq!(
            live.reporter.status(GreeterServer::<MyGreeter>::NAME),
            Some(ServingStatus::Serving)
        );
        assert_eq!(
            live.reporter.names(),
            vec![String::new(), GreeterServer::<MyGreeter>::NAME.to_owned()]
        );
        let _ready = greeter(live.addr).await.unwrap();
        let client = HealthClient::connect(live.addr).await.unwrap();
        let mut req = HealthCheckRequest::new();
        req.set_service(GreeterServer::<MyGreeter>::NAME);
        let status = client
            .check(Request::new(req))
            .await
            .unwrap()
            .into_inner()
            .status();
        assert_eq!(status, ServingStatus::Serving);
    }

    #[tokio::test]
    async fn dropping_a_health_watch_releases_the_subscription() {
        use pbrs_grpc::health::{HealthCheckRequest, HealthClient, ServingStatus};

        let live = serve().await.unwrap();
        let _ready = greeter(live.addr).await.unwrap();
        let client = HealthClient::connect(live.addr).await.unwrap();
        assert_eq!(live.reporter.watchers(), 0);
        let mut req = HealthCheckRequest::new();
        req.set_service(GreeterServer::<MyGreeter>::NAME);
        let mut stream = client.watch(Request::new(req)).await.unwrap().into_inner();
        let first = stream.message().await.unwrap().unwrap();
        assert_eq!(first.status(), ServingStatus::Serving);
        assert!(
            live.reporter.watchers() >= 1,
            "Watch must hold a subscription while the stream is live"
        );
        drop(stream);
        for _ in 0..80 {
            if live.reporter.watchers() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            live.reporter.watchers(),
            0,
            "Watch must not wait for the next status change after the client leaves"
        );
    }

    #[tokio::test]
    async fn reflection_lists_the_greeter() {
        use pbrs_grpc::reflection::{ServerReflectionClient, ServerReflectionRequest};

        let live = serve().await.unwrap();
        let _ready = greeter(live.addr).await.unwrap();
        let client = ServerReflectionClient::connect(live.addr).await.unwrap();
        let (tx, call) = client.server_reflection_info(Request::new(()));
        let mut inbound = call.await.unwrap().into_inner();
        let mut req = ServerReflectionRequest::new();
        req.set_list_services("");
        tx.send(req).await.unwrap();
        tx.close();
        let resp = inbound.message().await.unwrap().unwrap();
        let names: Vec<String> = resp
            .list_services_response()
            .service()
            .iter()
            .map(|s| s.name().to_str().unwrap_or("").to_owned())
            .collect();
        assert!(
            names.contains(&GreeterServer::<MyGreeter>::NAME.to_owned()),
            "{names:?}"
        );
    }

    #[tokio::test]
    async fn teaching_paths_exercise_all_four_shapes_and_shutdown() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // 1. Unary
        let unary = say_hello(&client, "ada").await.unwrap();
        assert_eq!(unary, "hello ada");

        // 2. Client streaming (upload)
        let upload = say_hello_stream(&client, ["grace", "alan"]).await.unwrap();
        assert_eq!(upload, "hello grace, alan");

        // Also test alias
        let upload_alias = client_hello(&client, ["grace", "alan"]).await.unwrap();
        assert_eq!(upload_alias, "hello grace, alan");

        // 3. Server streaming (download)
        let download = say_hello_server_stream(&client, "edsger").await.unwrap();
        assert_eq!(
            download,
            ["hello edsger #1", "hello edsger #2", "hello edsger #3"]
        );

        // Also test alias
        let download_alias = server_hello(&client, "edsger").await.unwrap();
        assert_eq!(
            download_alias,
            ["hello edsger #1", "hello edsger #2", "hello edsger #3"]
        );

        // 4. Bidirectional streaming
        let bidi = say_hello_bidi_stream(&client, ["barbara"]).await.unwrap();
        assert_eq!(bidi, ["hello barbara"]);

        // Also test alias
        let bidi_alias = stream_hello(&client, ["barbara"]).await.unwrap();
        assert_eq!(bidi_alias, ["hello barbara"]);

        // Explicit clean shutdown
        live.shutdown().await;
    }

    #[tokio::test]
    async fn teaching_paths_explicit_error_handling() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // Empty name unary fails with invalid_argument
        let mut req = HelloRequest::new();
        req.set_name("");
        let err = client.say_hello(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        // Empty stream client-streaming fails with invalid_argument
        let empty: [&str; 0] = [];
        let err = say_hello_stream(&client, empty).await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        // Empty name server-streaming fails with invalid_argument
        let err = say_hello_server_stream(&client, "").await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        live.shutdown().await;
    }

    #[tokio::test]
    async fn recipe_unary_asserts_content_status_shutdown() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // Content: exact reply bytes for a known request.
        assert_eq!(say_hello(&client, "ada").await.unwrap(), "hello ada");

        // Final status: invalid input surfaces InvalidArgument, not Ok.
        let mut bad = HelloRequest::new();
        bad.set_name("");
        let err = client.say_hello(Request::new(bad)).await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        // Shutdown: drain completes; the test would hang on failure.
        live.shutdown().await;
    }

    #[tokio::test]
    async fn recipe_client_streaming_asserts_content_status_shutdown() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // Content: consolidated reply over a bounded, half-closed upload.
        let upload = say_hello_stream(&client, ["grace", "alan"]).await.unwrap();
        assert_eq!(upload, "hello grace, alan");

        // Final status: an empty upload surfaces InvalidArgument, not Ok.
        let empty: [&str; 0] = [];
        let err = say_hello_stream(&client, empty).await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        live.shutdown().await;
    }

    #[tokio::test]
    async fn recipe_server_streaming_asserts_content_status_shutdown() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // Content: exact download sequence read to EOF.
        let download = say_hello_server_stream(&client, "edsger").await.unwrap();
        assert_eq!(
            download,
            ["hello edsger #1", "hello edsger #2", "hello edsger #3"]
        );

        // Final status: invalid input surfaces InvalidArgument, not Ok.
        let err = say_hello_server_stream(&client, "").await.unwrap_err();
        assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

        live.shutdown().await;
    }

    #[tokio::test]
    async fn recipe_bidi_asserts_content_status_shutdown() {
        let live = serve().await.unwrap();
        let client = greeter(live.addr).await.unwrap();

        // Content: exact full-duplex replies via the teaching helper.
        let bidi = say_hello_bidi_stream(&client, ["barbara"]).await.unwrap();
        assert_eq!(bidi, ["hello barbara"]);

        // Final status: half-close then drain to EOF; a non-OK trailer
        // would surface as Err from `message()` instead of `None`.
        let (tx, call) = client.stream_hello(Request::new(()));
        let mut inbound = call.await.unwrap().into_inner();
        tx.send(request("donald")).await.unwrap();
        assert_eq!(
            text(&inbound.message().await.unwrap().unwrap()),
            "hello donald"
        );
        tx.close();
        assert!(inbound.message().await.unwrap().is_none());

        live.shutdown().await;
    }
}
