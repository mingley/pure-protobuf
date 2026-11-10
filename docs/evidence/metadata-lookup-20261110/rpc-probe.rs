//! Temporary loopback RPC screen of request metadata reads.
use pbrs_grpc::hello::HelloRequest;
use pbrs_grpc::{Channel, Request, Response, Router, Rpc, Service, Status};
use std::hint::black_box;
use std::time::Instant;

const PATH: &str = "/bench.Metadata/Unary";

struct Echo {
    reads: usize,
}

impl Service for Echo {
    const NAME: &'static str = "bench.Metadata";

    async fn call(&self, rpc: Rpc) {
        let reads = self.reads;
        rpc.unary(move |req: Request<HelloRequest>| async move {
            for _ in 0..reads {
                assert_eq!(
                    black_box(req.metadata()).get(black_box("X-Request-Id")),
                    Some("first-value")
                );
            }
            Ok::<_, Status>(Response::new(req.into_inner()))
        }).await
    }
}

async fn call(channel: &Channel) -> HelloRequest {
    let mut request_message = HelloRequest::new();
    request_message.set_name("bench");
    let mut request = Request::new(request_message);
    request.metadata_mut().insert("x-request-id", "first-value").unwrap();
    channel.unary::<HelloRequest, HelloRequest>(PATH, request)
        .await.unwrap().into_inner()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reads: usize = args[1].parse().unwrap();
    let iterations: usize = args[2].parse().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        Router::new().add_service(Echo { reads }).serve_listener(listener).await.unwrap();
    });
    let channel = Channel::connect(addr).await.unwrap();
    assert_eq!(call(&channel).await.name().to_str().unwrap(), "bench");
    for _ in 0..2000 {
        black_box(call(&channel).await);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(call(&channel).await);
    }
    let elapsed = start.elapsed();
    println!("{}", elapsed.as_nanos());
    server.abort();
}
