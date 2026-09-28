//! grpc_bench `SayHello` echo server on pbrs-grpc (single-thread).
//!
//! Mirrors `rust_tonic_st_bench`: same echo handler on a
//! `current_thread` runtime, serves `0.0.0.0:50051`.

mod proto {
    #![allow(missing_docs, unused)]
    include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
}

use pbrs_grpc::{Request, Response, Router, Status};
use proto::{Greeter, GreeterServer, HelloReply, HelloRequest};

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub struct MyGreeter;

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_response(request.into_inner().request().clone());
        Ok(Response::new(reply))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse().unwrap();
    println!("GreeterServer listening on {}", addr);
    Router::new()
        .add_service(GreeterServer::new(MyGreeter))
        .serve(addr)
        .await?;
    Ok(())
}
