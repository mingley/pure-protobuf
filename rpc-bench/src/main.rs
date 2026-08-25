//! Loopback empty_unary and large_unary: pbrs-grpc vs tonic 0.14.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    missing_docs,
    reason = "bench binary"
)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

mod tonic_gen {
    #![allow(missing_docs, unused, reason = "generated tonic TestService")]
    include!(concat!(env!("OUT_DIR"), "/test.rs"));
}

use pbrs_grpc::{
    Empty, InteropTestService, Request as KReq, SimpleRequest, TestServiceClient, TestServiceServer,
};
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

const LARGE_REQ: i32 = 271828;
const LARGE_RESP: i32 = 314159;
const ITERS: u32 = 400;
const LARGE_ITERS: u32 = 80;

struct TonicInterop;

impl tonic_gen::TestService for TonicInterop {
    async fn empty_call(&self, _req: Request<tonic_gen::Empty>) -> Result<Response<tonic_gen::Empty>, Status> {
        Ok(Response::new(tonic_gen::Empty::new()))
    }
    async fn unary_call(
        &self,
        req: Request<tonic_gen::SimpleRequest>,
    ) -> Result<Response<tonic_gen::SimpleResponse>, Status> {
        let n = req.into_inner().response_size();
        let mut resp = tonic_gen::SimpleResponse::new();
        let mut p = tonic_gen::Payload::new();
        p.set_body(vec![0u8; usize::try_from(n.max(0)).unwrap_or(0)]);
        resp.set_payload(p);
        Ok(Response::new(resp))
    }
    async fn cacheable_unary_call(
        &self,
        req: Request<tonic_gen::SimpleRequest>,
    ) -> Result<Response<tonic_gen::SimpleResponse>, Status> {
        self.unary_call(req).await
    }
    type StreamingOutputCallStream =
        tokio_stream::wrappers::ReceiverStream<Result<tonic_gen::StreamingOutputCallResponse, Status>>;
    async fn streaming_output_call(
        &self,
        _req: Request<tonic_gen::StreamingOutputCallRequest>,
    ) -> Result<Response<Self::StreamingOutputCallStream>, Status> {
        Err(Status::unimplemented("bench"))
    }
    async fn streaming_input_call(
        &self,
        _req: Request<tonic::Streaming<tonic_gen::StreamingInputCallRequest>>,
    ) -> Result<Response<tonic_gen::StreamingInputCallResponse>, Status> {
        Err(Status::unimplemented("bench"))
    }
    type FullDuplexCallStream =
        tokio_stream::wrappers::ReceiverStream<Result<tonic_gen::StreamingOutputCallResponse, Status>>;
    async fn full_duplex_call(
        &self,
        _req: Request<tonic::Streaming<tonic_gen::StreamingOutputCallRequest>>,
    ) -> Result<Response<Self::FullDuplexCallStream>, Status> {
        Err(Status::unimplemented("bench"))
    }
    type HalfDuplexCallStream =
        tokio_stream::wrappers::ReceiverStream<Result<tonic_gen::StreamingOutputCallResponse, Status>>;
    async fn half_duplex_call(
        &self,
        _req: Request<tonic::Streaming<tonic_gen::StreamingOutputCallRequest>>,
    ) -> Result<Response<Self::HalfDuplexCallStream>, Status> {
        Err(Status::unimplemented("bench"))
    }
    async fn unimplemented_call(
        &self,
        _req: Request<tonic_gen::Empty>,
    ) -> Result<Response<tonic_gen::Empty>, Status> {
        Err(Status::unimplemented("bench"))
    }
}

fn ns(d: Duration) -> u128 {
    d.as_nanos()
}

fn median_ns(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

async fn bench_kernel(addr: SocketAddr) -> (u128, u128) {
    let client = TestServiceClient::new(pbrs_grpc::Channel::connect(addr).await.unwrap());
    for _ in 0..20 {
        let _ = client.empty_call(KReq::new(Empty::new())).await.unwrap();
    }
    let mut empty = Vec::new();
    for _ in 0..ITERS {
        let t = Instant::now();
        let _ = client.empty_call(KReq::new(Empty::new())).await.unwrap();
        empty.push(ns(t.elapsed()));
    }
    let mut sr = SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = pbrs_grpc::Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    for _ in 0..20 {
        let _ = client.unary_call(KReq::new(sr.clone())).await.unwrap();
    }
    let mut large = Vec::new();
    for _ in 0..LARGE_ITERS {
        let t = Instant::now();
        let _ = client.unary_call(KReq::new(sr.clone())).await.unwrap();
        large.push(ns(t.elapsed()));
    }
    (median_ns(empty), median_ns(large))
}

async fn bench_tonic(addr: SocketAddr) -> (u128, u128) {
    let ch = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = tonic_gen::TestServiceClient::new(ch);
    for _ in 0..20 {
        let _ = client
            .empty_call(Request::new(tonic_gen::Empty::new()))
            .await
            .unwrap();
    }
    let mut empty = Vec::new();
    for _ in 0..ITERS {
        let t = Instant::now();
        let _ = client
            .empty_call(Request::new(tonic_gen::Empty::new()))
            .await
            .unwrap();
        empty.push(ns(t.elapsed()));
    }
    let mut sr = tonic_gen::SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = tonic_gen::Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    for _ in 0..20 {
        let _ = client.unary_call(Request::new(sr.clone())).await.unwrap();
    }
    let mut large = Vec::new();
    for _ in 0..LARGE_ITERS {
        let t = Instant::now();
        let _ = client.unary_call(Request::new(sr.clone())).await.unwrap();
        large.push(ns(t.elapsed()));
    }
    (median_ns(empty), median_ns(large))
}

#[tokio::main]
async fn main() {
    let k_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let k_addr = k_listener.local_addr().unwrap();
    tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(k_listener)
            .await
            .ok();
    });

    let t_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let t_addr = t_listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(tonic_gen::TestServiceServer::new(TonicInterop))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(t_listener))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (k_empty, k_large) = bench_kernel(k_addr).await;
    let (t_empty, t_large) = bench_tonic(t_addr).await;
    println!(
        "empty_unary kernel_ns={k_empty} tonic_ns={t_empty}\nlarge_unary kernel_ns={k_large} tonic_ns={t_large}"
    );
    if k_empty >= t_empty || k_large >= t_large {
        eprintln!("perf gate failed: kernel empty {k_empty} vs tonic {t_empty}; large {k_large} vs {t_large}");
        std::process::exit(1);
    }
}


