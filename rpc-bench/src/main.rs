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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
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
const ITERS: u32 = 800;
const LARGE_ITERS: u32 = 80;
const QPS_SECS: f64 = 3.0;
const QPS_CONC_LOW: u32 = 1;
const QPS_CONNS_LOW: usize = 1;
const QPS_CONC_HIGH: u32 = 16;
const QPS_CONNS_HIGH: usize = 4;

struct TonicInterop;

impl tonic_gen::TestService for TonicInterop {
    async fn empty_call(
        &self,
        _req: Request<tonic_gen::Empty>,
    ) -> Result<Response<tonic_gen::Empty>, Status> {
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
    type StreamingOutputCallStream = tokio_stream::wrappers::ReceiverStream<
        Result<tonic_gen::StreamingOutputCallResponse, Status>,
    >;
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
    type FullDuplexCallStream = tokio_stream::wrappers::ReceiverStream<
        Result<tonic_gen::StreamingOutputCallResponse, Status>,
    >;
    async fn full_duplex_call(
        &self,
        _req: Request<tonic::Streaming<tonic_gen::StreamingOutputCallRequest>>,
    ) -> Result<Response<Self::FullDuplexCallStream>, Status> {
        Err(Status::unimplemented("bench"))
    }
    type HalfDuplexCallStream = tokio_stream::wrappers::ReceiverStream<
        Result<tonic_gen::StreamingOutputCallResponse, Status>,
    >;
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

fn large_kernel_req() -> SimpleRequest {
    let mut sr = SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = pbrs_grpc::Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    sr
}

fn large_tonic_req() -> tonic_gen::SimpleRequest {
    let mut sr = tonic_gen::SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = tonic_gen::Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    sr
}

async fn qps_kernel_empty(addr: SocketAddr, conc: u32, conns: usize, dur: Duration) -> (u64, u64) {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect_pool(addr, conns.max(1))
            .await
            .unwrap(),
    );
    for _ in 0..32 {
        client.empty_call(KReq::new(Empty::new())).await.unwrap();
    }
    let n = Arc::new(AtomicU64::new(0));
    let err = Arc::new(AtomicU64::new(0));
    let run = Arc::new(AtomicBool::new(true));
    let mut hs = Vec::new();
    for _ in 0..conc {
        let c = client.clone();
        let n = Arc::clone(&n);
        let err = Arc::clone(&err);
        let run = Arc::clone(&run);
        hs.push(tokio::spawn(async move {
            while run.load(Ordering::Relaxed) {
                match c.empty_call(KReq::new(Empty::new())).await {
                    Ok(_) => {
                        n.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(dur).await;
    run.store(false, Ordering::Relaxed);
    for h in hs {
        h.await.unwrap();
    }
    (n.load(Ordering::Relaxed), err.load(Ordering::Relaxed))
}

async fn qps_kernel_large(addr: SocketAddr, conc: u32, conns: usize, dur: Duration) -> (u64, u64) {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect_pool(addr, conns.max(1))
            .await
            .unwrap(),
    );
    let sr = large_kernel_req();
    for _ in 0..8 {
        client.unary_call(KReq::new(sr.clone())).await.unwrap();
    }
    let n = Arc::new(AtomicU64::new(0));
    let err = Arc::new(AtomicU64::new(0));
    let run = Arc::new(AtomicBool::new(true));
    let mut hs = Vec::new();
    for _ in 0..conc {
        let c = client.clone();
        let sr = sr.clone();
        let n = Arc::clone(&n);
        let err = Arc::clone(&err);
        let run = Arc::clone(&run);
        hs.push(tokio::spawn(async move {
            while run.load(Ordering::Relaxed) {
                match c.unary_call(KReq::new(sr.clone())).await {
                    Ok(_) => {
                        n.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(dur).await;
    run.store(false, Ordering::Relaxed);
    for h in hs {
        h.await.unwrap();
    }
    (n.load(Ordering::Relaxed), err.load(Ordering::Relaxed))
}

async fn qps_tonic_empty(addr: SocketAddr, conc: u32, conns: usize, dur: Duration) -> (u64, u64) {
    let nconn = conns.max(1);
    let mut clients = Vec::with_capacity(nconn);
    for _ in 0..nconn {
        let ch = Channel::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        clients.push(tonic_gen::TestServiceClient::new(ch));
    }
    {
        let Some(c0) = clients.first_mut() else {
            return (0, 1);
        };
        for _ in 0..32 {
            c0.empty_call(Request::new(tonic_gen::Empty::new()))
                .await
                .unwrap();
        }
    }
    let n = Arc::new(AtomicU64::new(0));
    let err = Arc::new(AtomicU64::new(0));
    let run = Arc::new(AtomicBool::new(true));
    let mut hs = Vec::new();
    for i in 0..conc {
        let mut c = clients
            .get(i as usize % nconn)
            .cloned()
            .unwrap_or_else(|| clients.first().cloned().unwrap());
        let n = Arc::clone(&n);
        let err = Arc::clone(&err);
        let run = Arc::clone(&run);
        hs.push(tokio::spawn(async move {
            while run.load(Ordering::Relaxed) {
                match c.empty_call(Request::new(tonic_gen::Empty::new())).await {
                    Ok(_) => {
                        n.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(dur).await;
    run.store(false, Ordering::Relaxed);
    for h in hs {
        h.await.unwrap();
    }
    (n.load(Ordering::Relaxed), err.load(Ordering::Relaxed))
}

async fn qps_tonic_large(addr: SocketAddr, conc: u32, conns: usize, dur: Duration) -> (u64, u64) {
    let nconn = conns.max(1);
    let mut clients = Vec::with_capacity(nconn);
    for _ in 0..nconn {
        let ch = Channel::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        clients.push(tonic_gen::TestServiceClient::new(ch));
    }
    let sr = large_tonic_req();
    {
        let Some(c0) = clients.first_mut() else {
            return (0, 1);
        };
        for _ in 0..8 {
            c0.unary_call(Request::new(sr.clone())).await.unwrap();
        }
    }
    let n = Arc::new(AtomicU64::new(0));
    let err = Arc::new(AtomicU64::new(0));
    let run = Arc::new(AtomicBool::new(true));
    let mut hs = Vec::new();
    for i in 0..conc {
        let mut c = clients
            .get(i as usize % nconn)
            .cloned()
            .unwrap_or_else(|| clients.first().cloned().unwrap());
        let sr = sr.clone();
        let n = Arc::clone(&n);
        let err = Arc::clone(&err);
        let run = Arc::clone(&run);
        hs.push(tokio::spawn(async move {
            while run.load(Ordering::Relaxed) {
                match c.unary_call(Request::new(sr.clone())).await {
                    Ok(_) => {
                        n.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }
    tokio::time::sleep(dur).await;
    run.store(false, Ordering::Relaxed);
    for h in hs {
        h.await.unwrap();
    }
    (n.load(Ordering::Relaxed), err.load(Ordering::Relaxed))
}

fn qps(count: u64, dur: Duration) -> u64 {
    (count as f64 / dur.as_secs_f64()).round() as u64
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

    let dur = Duration::from_secs_f64(QPS_SECS);
    let (ek1n, ek1e) = qps_kernel_empty(k_addr, QPS_CONC_LOW, QPS_CONNS_LOW, dur).await;
    let (et1n, et1e) = qps_tonic_empty(t_addr, QPS_CONC_LOW, QPS_CONNS_LOW, dur).await;
    let (ekhn, ekhe) = qps_kernel_empty(k_addr, QPS_CONC_HIGH, QPS_CONNS_HIGH, dur).await;
    let (ethn, ethe) = qps_tonic_empty(t_addr, QPS_CONC_HIGH, QPS_CONNS_HIGH, dur).await;
    let ek1 = qps(ek1n, dur);
    let et1 = qps(et1n, dur);
    let ekh = qps(ekhn, dur);
    let eth = qps(ethn, dur);
    println!(
        "qps empty conc={QPS_CONC_LOW} conns={QPS_CONNS_LOW} kernel={ek1} tonic={et1} kernel_err={ek1e} tonic_err={et1e}"
    );
    println!(
        "qps empty conc={QPS_CONC_HIGH} conns={QPS_CONNS_HIGH} kernel={ekh} tonic={eth} kernel_err={ekhe} tonic_err={ethe}"
    );

    let (lk1n, lk1e) = qps_kernel_large(k_addr, QPS_CONC_LOW, QPS_CONNS_LOW, dur).await;
    let (lt1n, lt1e) = qps_tonic_large(t_addr, QPS_CONC_LOW, QPS_CONNS_LOW, dur).await;
    let (lkhn, lkhe) = qps_kernel_large(k_addr, QPS_CONC_HIGH, QPS_CONNS_HIGH, dur).await;
    let (lthn, lthe) = qps_tonic_large(t_addr, QPS_CONC_HIGH, QPS_CONNS_HIGH, dur).await;
    let lk1 = qps(lk1n, dur);
    let lt1 = qps(lt1n, dur);
    let lkh = qps(lkhn, dur);
    let lth = qps(lthn, dur);
    println!(
        "qps large conc={QPS_CONC_LOW} conns={QPS_CONNS_LOW} kernel={lk1} tonic={lt1} kernel_err={lk1e} tonic_err={lt1e}"
    );
    println!(
        "qps large conc={QPS_CONC_HIGH} conns={QPS_CONNS_HIGH} kernel={lkh} tonic={lth} kernel_err={lkhe} tonic_err={lthe}"
    );

    let mut failed = false;
    if k_empty >= t_empty || k_large >= t_large {
        eprintln!("perf gate failed: kernel empty {k_empty} vs tonic {t_empty}; large {k_large} vs {t_large}");
        failed = true;
    }
    if ek1e != 0
        || et1e != 0
        || ekhe != 0
        || ethe != 0
        || lk1e != 0
        || lt1e != 0
        || lkhe != 0
        || lthe != 0
    {
        eprintln!("rpc-bench failed: nonzero RPC errors");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
