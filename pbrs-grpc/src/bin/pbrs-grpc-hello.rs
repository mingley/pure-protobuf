//! End-to-end `helloworld.Greeter` over loopback, exercising all four gRPC
//! call shapes. Doubles as the crate's worked example.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "example binary"
)]

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Request, Response, Status, Streaming};
use std::net::SocketAddr;
use tokio::net::TcpListener;

fn reply(message: impl Into<String>) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(message.into());
    reply
}

fn request(name: &str) -> HelloRequest {
    let mut req = HelloRequest::new();
    req.set_name(name);
    req
}

fn name_of(req: &HelloRequest) -> String {
    req.name().to_str().unwrap_or_default().to_owned()
}

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(reply(format!(
            "hello {}",
            name_of(request.get_ref())
        ))))
    }

    /// Read every request, answer once with the joined names.
    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(req) = stream.message().await? {
            names.push(name_of(&req));
        }
        Ok(Response::new(reply(format!("hello {}", names.join(", ")))))
    }

    /// Answer one request with a stream of greetings.
    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = name_of(request.get_ref());
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

    /// Answer each request as it arrives.
    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            loop {
                match inbound.message().await {
                    Ok(Some(req)) => {
                        if tx
                            .send(reply(format!("hello {}", name_of(&req))))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(status) => {
                        tx.fail(status).await;
                        break;
                    }
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

async fn run() -> Result<Vec<String>, Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    }));

    let client = GreeterClient::connect(addr).await?;
    let mut out = Vec::new();

    let unary = client.say_hello(Request::new(request("ada"))).await?;
    out.push(text(unary.get_ref()));

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(request("grace")).await?;
    tx.send(request("alan")).await?;
    tx.close();
    out.push(text(call.await?.get_ref()));

    let mut server_stream = client
        .server_hello(Request::new(request("edsger")))
        .await?
        .into_inner();
    while let Some(msg) = server_stream.message().await? {
        out.push(text(&msg));
    }

    let (tx, call) = client.stream_hello(Request::new(()));
    let mut bidi = call.await?.into_inner();
    for name in ["barbara", "katherine"] {
        tx.send(request(name)).await?;
        if let Some(msg) = bidi.message().await? {
            out.push(text(&msg));
        }
    }
    tx.close();
    while bidi.message().await?.is_some() {}

    Ok(out)
}

fn text(reply: &HelloReply) -> String {
    reply.message().to_str().unwrap_or_default().to_owned()
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
