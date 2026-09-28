//! grpc_bench `SayHello` echo server on pbrs-grpc (multi-thread).
//!
//! Mirrors `rust_tonic_mt_bench`: plaintext unary echo of the inner
//! `Hello` message, worker threads from `GRPC_SERVER_CPUS`, jemalloc
//! global allocator, serves `0.0.0.0:50051`.

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cpus = std::env::var("GRPC_SERVER_CPUS")
        .map(|v| v.parse().unwrap())
        .unwrap_or(1);

    println!("Running with {} threads", cpus);

    // Same shape as the tonic entry: cap worker threads to the
    // container CPU grant to avoid thrashing under cgroup limits.
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(cpus)
        .enable_all()
        .build()
        .unwrap()
        .block_on(serve())
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse().unwrap();
    println!("GreeterServer listening on {}", addr);
    Router::new()
        .add_service(GreeterServer::new(MyGreeter))
        .serve(addr)
        .await?;
    Ok(())
}
