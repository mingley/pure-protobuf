//! Co-host native pbrs-grpc and axum REST routes on one port.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "example binary"
)]

use axum::Router as AxumRouter;
use axum::routing::get;
use pbrs_grpc::hello::{FILE_DESCRIPTOR_SET, Greeter, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Request, Response, Router, Status};

struct GreeterSvc;

impl Greeter for GreeterSvc {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    let (health, reporter) = pbrs_grpc::health::service();
    reporter.set_serving("helloworld.Greeter");
    let reflection = pbrs_grpc::reflection::Builder::new()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build()?;
    let grpc = Router::new()
        .add_service(GreeterServer::new(GreeterSvc))
        .add_service(health)
        .add_service(reflection)
        .into_tower_service();

    let app = AxumRouter::new()
        .route("/ready", get(|| async { "ok" }))
        .fallback_service(grpc);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:50051")
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))
}
