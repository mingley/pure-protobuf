//! Loopback unary Greeter: print the echoed request name and exit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "example binary"
)]

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Inbound, Request, Response, Status};
use std::net::SocketAddr;
use tokio::net::TcpListener;

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
        _request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("unary example"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("unary example"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Inbound<HelloRequest>>,
    ) -> Result<Response<Inbound<HelloReply>>, Status> {
        Err(Status::unimplemented("unary example"))
    }
}

async fn run() -> Result<String, Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    }));
    let ch = Channel::connect(addr).await?;
    let client = GreeterClient::new(ch);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let resp = client.say_hello(Request::new(req)).await?;
    Ok(resp
        .into_inner()
        .message()
        .to_str()
        .unwrap_or("")
        .to_string())
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(body) => println!("{body}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
