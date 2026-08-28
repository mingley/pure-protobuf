//! Shared helpers: spawn a Greeter, connect a client, build messages.

#![allow(
    dead_code,
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
    missing_docs,
    reason = "integration test helpers"
)]

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Request, Response, ServerConfig, Status, Streaming};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Keeps the server task alive for the duration of a test and aborts it after.
pub struct ServerGuard(JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Bind an ephemeral port and serve `service` on it.
pub async fn serve<G: Greeter>(
    service: G,
    config: ServerConfig,
) -> Result<(SocketAddr, ServerGuard), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let handle = tokio::spawn(async move {
        GreeterServer::new(service)
            .config(config)
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard(handle)))
}

/// Serve the echo Greeter used by the hostile-peer tests.
pub async fn spawn_greeter_server(config: ServerConfig) -> (SocketAddr, ServerGuard) {
    serve(Echo, config).await.expect("spawn greeter")
}

/// Connect a client, retrying while the listener comes up.
pub async fn greeter_client(addr: SocketAddr) -> GreeterClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return GreeterClient::new(channel),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

/// Serve `service` and return a connected client.
pub async fn spawn_greeter<G: Greeter>(
    service: G,
) -> Result<(SocketAddr, GreeterClient, ServerGuard), Status> {
    let (addr, guard) = serve(service, ServerConfig::default()).await?;
    Ok((addr, greeter_client(addr).await, guard))
}

pub fn name_of(reply: &HelloReply) -> String {
    reply.message().to_str().unwrap_or("").to_string()
}

pub fn req(name: &str) -> HelloRequest {
    let mut r = HelloRequest::new();
    r.set_name(name);
    r
}

pub fn reply(message: impl Into<String>) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(message.into());
    r
}

/// The reference echo Greeter: unary echoes the name, client-stream joins with
/// commas, server-stream splits on commas, bidi echoes each request.
pub struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(reply(name_of_request(request.get_ref()))))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(msg) = stream.message().await? {
            names.push(name_of_request(&msg));
        }
        Ok(Response::new(reply(names.join(","))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = name_of_request(request.get_ref());
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(reply(part.to_string())).await.is_err() {
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
            while let Ok(Some(msg)) = inbound.message().await {
                if tx.send(reply(name_of_request(&msg))).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

pub fn name_of_request(request: &HelloRequest) -> String {
    request.name().to_str().unwrap_or("").to_string()
}
