//! Process roles and orchestration for `rpc-bench`.
//!
//! Provides explicit role separation (`server`, `client`, `smoke`) with bounded lifecycle,
//! stdout readiness reporting, external endpoint resolution, and payload/order/status
//! validation across all four gRPC call shapes (unary, client streaming, server streaming,
//! and bidirectional streaming).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    missing_docs,
    reason = "bench binary"
)]

use std::collections::HashSet;
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};
use tokio::net::TcpListener;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

use crate::report::{
    BenchmarkConfig, BenchmarkReport, BenchmarkRun, HostInfo, LatencyDistribution,
    REPORT_SCHEMA_VERSION, RpcMetrics, ScenarioInfo, StartEndConditions, ToolPins, TransportMode,
    detect_git_commit, format_rfc3339,
};
use crate::tonic_gen;

use pbrs_grpc::{
    Empty, InteropTestService, Payload, Request as KReq, ResponseParameters, SimpleRequest,
    StreamingInputCallRequest, StreamingOutputCallRequest, TestServiceClient, TestServiceServer,
};

pub const LARGE_REQ: i32 = 271828;
pub const LARGE_RESP: i32 = 314159;
pub const STREAM_SIZE: i32 = 1024;

pub const STREAM_PARITY: f64 = 0.9;
pub const PING_PONG_PARITY: f64 = 0.9;
pub const UPLOAD_PARITY: f64 = 0.9;

pub const QPS_CONC_LOW: u32 = 1;
pub const QPS_CONNS_LOW: usize = 1;
pub const QPS_CONC_HIGH: u32 = 16;
pub const QPS_CONNS_HIGH: usize = 4;

/// Iteration and duration parameters for benchmarks.
#[derive(Clone, Copy, Debug)]
pub struct BenchConfig {
    pub warmup: u32,
    pub iters: u32,
    pub large_iters: u32,
    pub qps_secs: f64,
    pub qps_rounds: usize,
    pub stream_msgs: i32,
    pub stream_size: i32,
    pub stream_rounds: usize,
    pub ping_pongs: u64,
}

impl BenchConfig {
    pub fn standard() -> Self {
        Self {
            warmup: 64,
            iters: 2000,
            large_iters: 200,
            qps_secs: 2.0,
            qps_rounds: 3,
            stream_msgs: 2000,
            stream_size: STREAM_SIZE,
            stream_rounds: 9,
            ping_pongs: 256,
        }
    }

    pub fn quick() -> Self {
        Self {
            warmup: 4,
            iters: 20,
            large_iters: 5,
            qps_secs: 0.5,
            qps_rounds: 1,
            stream_msgs: 20,
            stream_size: STREAM_SIZE,
            stream_rounds: 2,
            ping_pongs: 8,
        }
    }
}

/// Call shapes available for benchmarking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallShape {
    Unary,
    Stream,
    PingPong,
    Upload,
    Qps,
    All,
}

impl CallShape {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "unary" | "empty_unary" | "large_unary" => Some(Self::Unary),
            "stream" | "server_streaming" | "download" => Some(Self::Stream),
            "ping_pong" | "pingpong" | "bidi" | "bidi_streaming" => Some(Self::PingPong),
            "upload" | "client_streaming" => Some(Self::Upload),
            "qps" | "sustained_qps" => Some(Self::Qps),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub transport: TransportMode,
    pub timeout_secs: Option<u64>,
}

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub server_addr: String,
    pub transport: TransportMode,
    pub quick: bool,
    pub shapes: HashSet<CallShape>,
    pub output_file: Option<String>,
    pub print_json: bool,
}

/// Single-process smoke configuration.
#[derive(Debug, Clone)]
pub struct SmokeConfig {
    pub quick: bool,
    pub output_file: Option<String>,
    pub print_json: bool,
}

/// Top-level role to execute.
#[derive(Debug, Clone)]
pub enum ProcessRole {
    Server(ServerConfig),
    Client(ClientConfig),
    Smoke(SmokeConfig),
}

/// Tonic TestService implementation mirroring the kernel InteropTestService.
pub struct TonicInterop;

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
        req: Request<tonic_gen::StreamingOutputCallRequest>,
    ) -> Result<Response<Self::StreamingOutputCallStream>, Status> {
        let sizes: Vec<i32> = req
            .into_inner()
            .response_parameters()
            .iter()
            .map(|p| p.size())
            .collect();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            for size in sizes {
                let mut msg = tonic_gen::StreamingOutputCallResponse::new();
                let mut p = tonic_gen::Payload::new();
                p.set_body(vec![0u8; usize::try_from(size.max(0)).unwrap_or(0)]);
                msg.set_payload(p);
                if tx.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn streaming_input_call(
        &self,
        req: Request<tonic::Streaming<tonic_gen::StreamingInputCallRequest>>,
    ) -> Result<Response<tonic_gen::StreamingInputCallResponse>, Status> {
        let mut inbound = req.into_inner();
        let mut total: i32 = 0;
        while let Some(item) = inbound.message().await? {
            let n = i32::try_from(item.payload().body().len()).unwrap_or(i32::MAX);
            total = total.saturating_add(n);
        }
        let mut msg = tonic_gen::StreamingInputCallResponse::new();
        msg.set_aggregated_payload_size(total);
        Ok(Response::new(msg))
    }

    type FullDuplexCallStream = tokio_stream::wrappers::ReceiverStream<
        Result<tonic_gen::StreamingOutputCallResponse, Status>,
    >;

    async fn full_duplex_call(
        &self,
        req: Request<tonic::Streaming<tonic_gen::StreamingOutputCallRequest>>,
    ) -> Result<Response<Self::FullDuplexCallStream>, Status> {
        let mut inbound = req.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            loop {
                match inbound.message().await {
                    Ok(Some(msg)) => {
                        let sizes: Vec<i32> =
                            msg.response_parameters().iter().map(|p| p.size()).collect();
                        for size in sizes {
                            let mut out = tonic_gen::StreamingOutputCallResponse::new();
                            let mut payload = tonic_gen::Payload::new();
                            payload.set_body(vec![0u8; usize::try_from(size.max(0)).unwrap_or(0)]);
                            out.set_payload(payload);
                            if tx.send(Ok(out)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(None) => return,
                    Err(_) => return,
                }
            }
        });
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
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

/// Latency summary in nanoseconds.
#[derive(Clone, Copy, Debug)]
pub struct Latency {
    pub p50: u128,
    pub p99: u128,
}

impl Latency {
    pub fn from(mut samples: Vec<u128>) -> Self {
        samples.sort_unstable();
        let pick = |q: f64| {
            let i = ((samples.len() as f64 - 1.0) * q).round() as usize;
            samples.get(i).copied().unwrap_or(0)
        };
        Self {
            p50: pick(0.5),
            p99: pick(0.99),
        }
    }

    pub fn beats(self, other: Self) -> bool {
        self.p50 < other.p50 && self.p99 < other.p99
    }
}

pub struct LatencyMeasurement {
    pub latency: Latency,
    pub samples: Vec<u128>,
    pub duration: Duration,
}

pub struct StreamMeasurement {
    pub best: u64,
    pub rounds: Vec<(usize, u64, Duration)>,
}

#[derive(Clone)]
pub struct Counters {
    pub ok: Arc<AtomicU64>,
    pub err: Arc<AtomicU64>,
    pub run: Arc<AtomicBool>,
}

impl Counters {
    pub fn new() -> Self {
        Self {
            ok: Arc::new(AtomicU64::new(0)),
            err: Arc::new(AtomicU64::new(0)),
            run: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn record(&self, ok: bool) {
        if ok {
            self.ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.err.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn running(&self) -> bool {
        self.run.load(Ordering::Relaxed)
    }

    pub async fn stop_after(&self, dur: Duration) {
        tokio::time::sleep(dur).await;
        self.run.store(false, Ordering::Relaxed);
    }
}

pub fn rate(count: u64, dur: Duration) -> u64 {
    (count as f64 / dur.as_secs_f64()).round() as u64
}

pub fn large_kernel_req() -> SimpleRequest {
    let mut sr = SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    sr
}

pub fn large_tonic_req() -> tonic_gen::SimpleRequest {
    let mut sr = tonic_gen::SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    let mut p = tonic_gen::Payload::new();
    p.set_body(vec![0u8; LARGE_REQ as usize]);
    sr.set_payload(p);
    sr
}

pub fn stream_kernel_req(msgs: i32, size: i32) -> StreamingOutputCallRequest {
    let mut req = StreamingOutputCallRequest::new();
    for _ in 0..msgs {
        let mut p = ResponseParameters::new();
        p.set_size(size);
        req.response_parameters_mut().push(p);
    }
    req
}

pub fn stream_tonic_req(msgs: i32, size: i32) -> tonic_gen::StreamingOutputCallRequest {
    let mut req = tonic_gen::StreamingOutputCallRequest::new();
    for _ in 0..msgs {
        let mut p = tonic_gen::ResponseParameters::new();
        p.set_size(size);
        req.response_parameters_mut().push(p);
    }
    req
}

pub fn ping_pong_kernel_req() -> StreamingOutputCallRequest {
    let mut req = StreamingOutputCallRequest::new();
    let mut p = ResponseParameters::new();
    p.set_size(0);
    req.response_parameters_mut().push(p);
    req
}

pub fn ping_pong_tonic_req() -> tonic_gen::StreamingOutputCallRequest {
    let mut req = tonic_gen::StreamingOutputCallRequest::new();
    let mut p = tonic_gen::ResponseParameters::new();
    p.set_size(0);
    req.response_parameters_mut().push(p);
    req
}

pub fn upload_kernel_req(size: i32) -> StreamingInputCallRequest {
    let mut m = StreamingInputCallRequest::new();
    let mut p = Payload::new();
    p.set_body(vec![0u8; size as usize]);
    m.set_payload(p);
    m
}

pub fn upload_tonic_req(size: i32) -> tonic_gen::StreamingInputCallRequest {
    let mut m = tonic_gen::StreamingInputCallRequest::new();
    let mut p = tonic_gen::Payload::new();
    p.set_body(vec![0u8; size as usize]);
    m.set_payload(p);
    m
}

pub fn upload_want_bytes(msgs: i32, size: i32) -> i32 {
    msgs.saturating_mul(size)
}

pub async fn tonic_channel(addr: SocketAddr) -> Result<Channel, String> {
    Channel::from_shared(format!("http://{addr}"))
        .map_err(|e| format!("invalid tonic URI http://{addr}: {e}"))?
        .connect()
        .await
        .map_err(|e| format!("tonic failed to connect to {addr}: {e}"))
}

// ---------------------------------------------------------------------------
// Call Shape 1: Unary (Empty & Large) with payload, status and order checks
// ---------------------------------------------------------------------------

pub async fn latency_kernel(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<(LatencyMeasurement, LatencyMeasurement), String> {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect(addr)
            .await
            .map_err(|e| format!("kernel connect to {addr}: {e}"))?,
    );

    // Warmup empty
    for _ in 0..cfg.warmup {
        client
            .empty_call(KReq::new(Empty::new()))
            .await
            .map_err(|e| format!("kernel warmup empty_call failed: {e}"))?;
    }

    let t_start_empty = Instant::now();
    let mut empty = Vec::with_capacity(cfg.iters as usize);
    for _ in 0..cfg.iters {
        let t = Instant::now();
        let resp = client
            .empty_call(KReq::new(Empty::new()))
            .await
            .map_err(|e| format!("kernel empty_call failed: {e}"))?;
        let _ = resp.into_inner();
        empty.push(t.elapsed().as_nanos());
    }
    let empty_dur = t_start_empty.elapsed();

    // Warmup large
    let sr = large_kernel_req();
    for _ in 0..(cfg.warmup / 4).max(1) {
        let resp = client
            .unary_call(KReq::new(sr.clone()))
            .await
            .map_err(|e| format!("kernel warmup unary_call failed: {e}"))?;
        let body_len = resp.into_inner().payload().body().len();
        if body_len != LARGE_RESP as usize {
            return Err(format!(
                "kernel large_unary warmup payload mismatch: got {body_len}, want {LARGE_RESP}"
            ));
        }
    }

    let t_start_large = Instant::now();
    let mut large = Vec::with_capacity(cfg.large_iters as usize);
    for _ in 0..cfg.large_iters {
        let t = Instant::now();
        let resp = client
            .unary_call(KReq::new(sr.clone()))
            .await
            .map_err(|e| format!("kernel unary_call failed: {e}"))?;
        let body_len = resp.into_inner().payload().body().len();
        if body_len != LARGE_RESP as usize {
            return Err(format!(
                "kernel large_unary payload mismatch: got {body_len}, want {LARGE_RESP}"
            ));
        }
        large.push(t.elapsed().as_nanos());
    }
    let large_dur = t_start_large.elapsed();

    Ok((
        LatencyMeasurement {
            latency: Latency::from(empty.clone()),
            samples: empty,
            duration: empty_dur,
        },
        LatencyMeasurement {
            latency: Latency::from(large.clone()),
            samples: large,
            duration: large_dur,
        },
    ))
}

pub async fn latency_tonic(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<(LatencyMeasurement, LatencyMeasurement), String> {
    let mut client = tonic_gen::TestServiceClient::new(tonic_channel(addr).await?);

    // Warmup empty
    for _ in 0..cfg.warmup {
        client
            .empty_call(Request::new(tonic_gen::Empty::new()))
            .await
            .map_err(|e| format!("tonic warmup empty_call failed: {e}"))?;
    }

    let t_start_empty = Instant::now();
    let mut empty = Vec::with_capacity(cfg.iters as usize);
    for _ in 0..cfg.iters {
        let t = Instant::now();
        let resp = client
            .empty_call(Request::new(tonic_gen::Empty::new()))
            .await
            .map_err(|e| format!("tonic empty_call failed: {e}"))?;
        let _ = resp.into_inner();
        empty.push(t.elapsed().as_nanos());
    }
    let empty_dur = t_start_empty.elapsed();

    // Warmup large
    let sr = large_tonic_req();
    for _ in 0..(cfg.warmup / 4).max(1) {
        let resp = client
            .unary_call(Request::new(sr.clone()))
            .await
            .map_err(|e| format!("tonic warmup unary_call failed: {e}"))?;
        let body_len = resp.into_inner().payload().body().len();
        if body_len != LARGE_RESP as usize {
            return Err(format!(
                "tonic large_unary warmup payload mismatch: got {body_len}, want {LARGE_RESP}"
            ));
        }
    }

    let t_start_large = Instant::now();
    let mut large = Vec::with_capacity(cfg.large_iters as usize);
    for _ in 0..cfg.large_iters {
        let t = Instant::now();
        let resp = client
            .unary_call(Request::new(sr.clone()))
            .await
            .map_err(|e| format!("tonic unary_call failed: {e}"))?;
        let body_len = resp.into_inner().payload().body().len();
        if body_len != LARGE_RESP as usize {
            return Err(format!(
                "tonic large_unary payload mismatch: got {body_len}, want {LARGE_RESP}"
            ));
        }
        large.push(t.elapsed().as_nanos());
    }
    let large_dur = t_start_large.elapsed();

    Ok((
        LatencyMeasurement {
            latency: Latency::from(empty.clone()),
            samples: empty,
            duration: empty_dur,
        },
        LatencyMeasurement {
            latency: Latency::from(large.clone()),
            samples: large,
            duration: large_dur,
        },
    ))
}

// ---------------------------------------------------------------------------
// Call Shape 2: Server Streaming Download with count, payload & order checks
// ---------------------------------------------------------------------------

pub async fn stream_kernel(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect(addr)
            .await
            .map_err(|e| format!("kernel stream connect: {e}"))?,
    );
    let req = stream_kernel_req(cfg.stream_msgs, cfg.stream_size);
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let mut stream = client
            .streaming_output_call(KReq::new(req.clone()))
            .await
            .map_err(|e| format!("kernel streaming_output_call round {round}: {e}"))?
            .into_inner();
        let mut n = 0u64;
        while let Some(msg) = stream
            .message()
            .await
            .map_err(|e| format!("kernel stream msg round {round}: {e}"))?
        {
            let payload_len = msg.payload().body().len();
            if payload_len != cfg.stream_size as usize {
                return Err(format!(
                    "kernel stream payload mismatch round {round} msg {n}: got {payload_len}, want {}",
                    cfg.stream_size
                ));
            }
            n += 1;
        }
        if n != cfg.stream_msgs as u64 {
            return Err(format!(
                "kernel stream count mismatch round {round}: got {n}, want {}",
                cfg.stream_msgs
            ));
        }
        let dur = t.elapsed();
        rounds.push((round, n, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(n, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

pub async fn stream_tonic(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let mut client = tonic_gen::TestServiceClient::new(tonic_channel(addr).await?);
    let req = stream_tonic_req(cfg.stream_msgs, cfg.stream_size);
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let mut stream = client
            .streaming_output_call(Request::new(req.clone()))
            .await
            .map_err(|e| format!("tonic streaming_output_call round {round}: {e}"))?
            .into_inner();
        let mut n = 0u64;
        while let Some(msg) = stream
            .message()
            .await
            .map_err(|e| format!("tonic stream msg round {round}: {e}"))?
        {
            let payload_len = msg.payload().body().len();
            if payload_len != cfg.stream_size as usize {
                return Err(format!(
                    "tonic stream payload mismatch round {round} msg {n}: got {payload_len}, want {}",
                    cfg.stream_size
                ));
            }
            n += 1;
        }
        if n != cfg.stream_msgs as u64 {
            return Err(format!(
                "tonic stream count mismatch round {round}: got {n}, want {}",
                cfg.stream_msgs
            ));
        }
        let dur = t.elapsed();
        rounds.push((round, n, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(n, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

// ---------------------------------------------------------------------------
// Call Shape 3: Bidi Streaming Ping-Pong with lockstep payload & status checks
// ---------------------------------------------------------------------------

pub async fn ping_pong_kernel(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect(addr)
            .await
            .map_err(|e| format!("kernel ping_pong connect: {e}"))?,
    );
    let req = ping_pong_kernel_req();
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let (tx, call) = client.full_duplex_call(KReq::new(()));
        let mut inbound = call
            .await
            .map_err(|e| format!("kernel bidi call round {round}: {e}"))?
            .into_inner();
        for i in 0..cfg.ping_pongs {
            tx.send(req.clone())
                .await
                .map_err(|e| format!("kernel ping_pong send round {round} pair {i}: {e}"))?;
            let reply = inbound
                .message()
                .await
                .map_err(|e| format!("kernel ping_pong recv round {round} pair {i}: {e}"))?
                .ok_or_else(|| format!("kernel ping_pong ended early round {round} pair {i}"))?;
            let len = reply.payload().body().len();
            if len != 0 {
                return Err(format!(
                    "kernel ping_pong payload mismatch: got {len}, want 0"
                ));
            }
        }
        tx.close();
        while let Some(_) = inbound
            .message()
            .await
            .map_err(|e| format!("kernel ping_pong drain round {round}: {e}"))?
        {
            return Err("kernel ping_pong unexpected extra message after close".to_string());
        }
        let dur = t.elapsed();
        rounds.push((round, cfg.ping_pongs, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(cfg.ping_pongs, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

pub async fn ping_pong_tonic(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let mut client = tonic_gen::TestServiceClient::new(tonic_channel(addr).await?);
    let req = ping_pong_tonic_req();
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let mut inbound = client
            .full_duplex_call(Request::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
            .await
            .map_err(|e| format!("tonic bidi call round {round}: {e}"))?
            .into_inner();
        for i in 0..cfg.ping_pongs {
            tx.send(req.clone())
                .await
                .map_err(|e| format!("tonic ping_pong send round {round} pair {i}: {e}"))?;
            let reply = inbound
                .message()
                .await
                .map_err(|e| format!("tonic ping_pong recv round {round} pair {i}: {e}"))?
                .ok_or_else(|| format!("tonic ping_pong ended early round {round} pair {i}"))?;
            let len = reply.payload().body().len();
            if len != 0 {
                return Err(format!(
                    "tonic ping_pong payload mismatch: got {len}, want 0"
                ));
            }
        }
        drop(tx);
        while let Some(_) = inbound
            .message()
            .await
            .map_err(|e| format!("tonic ping_pong drain round {round}: {e}"))?
        {
            return Err("tonic ping_pong unexpected extra message after close".to_string());
        }
        let dur = t.elapsed();
        rounds.push((round, cfg.ping_pongs, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(cfg.ping_pongs, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

// ---------------------------------------------------------------------------
// Call Shape 4: Client Streaming Upload with aggregated payload check
// ---------------------------------------------------------------------------

pub async fn upload_kernel(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let client = TestServiceClient::new(
        pbrs_grpc::Channel::connect(addr)
            .await
            .map_err(|e| format!("kernel upload connect: {e}"))?,
    );
    let req = upload_kernel_req(cfg.stream_size);
    let want = upload_want_bytes(cfg.stream_msgs, cfg.stream_size);
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let (tx, call) = client.streaming_input_call(KReq::new(()));
        let send = async {
            for i in 0..cfg.stream_msgs {
                tx.send(req.clone())
                    .await
                    .map_err(|e| format!("kernel upload send round {round} msg {i}: {e}"))?;
            }
            tx.close();
            Ok::<(), String>(())
        };
        let (send_res, resp_res) = tokio::join!(send, call);
        send_res?;
        let resp = resp_res.map_err(|e| format!("kernel upload call round {round}: {e}"))?;
        let got = resp.into_inner().aggregated_payload_size();
        if got != want {
            return Err(format!(
                "kernel upload payload mismatch round {round}: got {got}, want {want}"
            ));
        }
        let dur = t.elapsed();
        rounds.push((round, cfg.stream_msgs as u64, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(cfg.stream_msgs as u64, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

pub async fn upload_tonic(
    addr: SocketAddr,
    cfg: &BenchConfig,
) -> Result<StreamMeasurement, String> {
    let mut client = tonic_gen::TestServiceClient::new(tonic_channel(addr).await?);
    let req = upload_tonic_req(cfg.stream_size);
    let want = upload_want_bytes(cfg.stream_msgs, cfg.stream_size);
    let mut best = 0u64;
    let mut rounds = Vec::with_capacity(cfg.stream_rounds);
    for round in 0..cfg.stream_rounds {
        let t = Instant::now();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let send = async {
            for i in 0..cfg.stream_msgs {
                if tx.send(req.clone()).await.is_err() {
                    return Err(format!("tonic upload send round {round} msg {i} failed"));
                }
            }
            drop(tx);
            Ok::<(), String>(())
        };
        let recv = client.streaming_input_call(Request::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ));
        let (send_res, resp_res) = tokio::join!(send, recv);
        send_res?;
        let resp = resp_res.map_err(|e| format!("tonic upload call round {round}: {e}"))?;
        let got = resp.into_inner().aggregated_payload_size();
        if got != want {
            return Err(format!(
                "tonic upload payload mismatch round {round}: got {got}, want {want}"
            ));
        }
        let dur = t.elapsed();
        rounds.push((round, cfg.stream_msgs as u64, dur));
        if round > 0 || cfg.stream_rounds == 1 {
            best = best.max(rate(cfg.stream_msgs as u64, dur));
        }
    }
    Ok(StreamMeasurement { best, rounds })
}

// ---------------------------------------------------------------------------
// QPS benchmarks
// ---------------------------------------------------------------------------

pub async fn qps_kernel(
    addr: SocketAddr,
    conc: u32,
    conns: usize,
    dur: Duration,
    large: bool,
) -> (u64, u64) {
    let client = match pbrs_grpc::Channel::connect_pool(addr, conns.max(1)).await {
        Ok(c) => TestServiceClient::new(c),
        Err(_) => return (0, conc as u64),
    };
    let sr = large_kernel_req();
    for _ in 0..8 {
        if large {
            let _ = client.unary_call(KReq::new(sr.clone())).await;
        } else {
            let _ = client.empty_call(KReq::new(Empty::new())).await;
        }
    }
    let counters = Counters::new();
    let mut handles = Vec::with_capacity(conc as usize);
    for _ in 0..conc {
        let client = client.clone();
        let counters = counters.clone();
        let sr = sr.clone();
        handles.push(tokio::spawn(async move {
            while counters.running() {
                let ok = if large {
                    client.unary_call(KReq::new(sr.clone())).await.is_ok()
                } else {
                    client.empty_call(KReq::new(Empty::new())).await.is_ok()
                };
                counters.record(ok);
            }
        }));
    }
    counters.stop_after(dur).await;
    for h in handles {
        let _ = h.await;
    }
    (
        counters.ok.load(Ordering::Relaxed),
        counters.err.load(Ordering::Relaxed),
    )
}

pub async fn qps_tonic(
    addr: SocketAddr,
    conc: u32,
    conns: usize,
    dur: Duration,
    large: bool,
) -> (u64, u64) {
    let nconn = conns.max(1);
    let mut clients = Vec::with_capacity(nconn);
    for _ in 0..nconn {
        match tonic_channel(addr).await {
            Ok(ch) => clients.push(tonic_gen::TestServiceClient::new(ch)),
            Err(_) => return (0, conc as u64),
        }
    }
    let sr = large_tonic_req();
    {
        if let Some(c0) = clients.first_mut() {
            for _ in 0..8 {
                if large {
                    let _ = c0.unary_call(Request::new(sr.clone())).await;
                } else {
                    let _ = c0.empty_call(Request::new(tonic_gen::Empty::new())).await;
                }
            }
        }
    }
    let counters = Counters::new();
    let mut handles = Vec::with_capacity(conc as usize);
    for i in 0..conc {
        let mut client = clients.get(i as usize % nconn).cloned().unwrap();
        let counters = counters.clone();
        let sr = sr.clone();
        handles.push(tokio::spawn(async move {
            while counters.running() {
                let ok = if large {
                    client.unary_call(Request::new(sr.clone())).await.is_ok()
                } else {
                    client
                        .empty_call(Request::new(tonic_gen::Empty::new()))
                        .await
                        .is_ok()
                };
                counters.record(ok);
            }
        }));
    }
    counters.stop_after(dur).await;
    for h in handles {
        let _ = h.await;
    }
    (
        counters.ok.load(Ordering::Relaxed),
        counters.err.load(Ordering::Relaxed),
    )
}

pub async fn best_qps<F, Fut>(
    rounds: usize,
    dur: Duration,
    mut run: F,
) -> (u64, u64, Vec<(usize, u64, u64)>)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = (u64, u64)>,
{
    let mut best = 0;
    let mut errors = 0;
    let mut recorded_rounds = Vec::with_capacity(rounds);
    for i in 0..rounds {
        let (ok, err) = run().await;
        best = best.max(rate(ok, dur));
        errors += err;
        recorded_rounds.push((i, ok, err));
    }
    (best, errors, recorded_rounds)
}

// ---------------------------------------------------------------------------
// BenchmarkRun report builder helpers
// ---------------------------------------------------------------------------

pub fn make_latency_run(
    scenario_id: &str,
    scenario_name: &str,
    req_size: usize,
    resp_size: usize,
    transport: TransportMode,
    measurement: &LatencyMeasurement,
    host_info: &HostInfo,
    git_commit: &str,
) -> BenchmarkRun {
    let dur_nanos = measurement.duration.as_nanos().max(1) as u64;
    let now = SystemTime::now();
    let start_time = now - measurement.duration;
    let latency = LatencyDistribution::from_nanos_samples(&measurement.samples);

    let mut metrics = RpcMetrics::new(
        measurement.samples.len() as u64,
        measurement.samples.len() as u64,
        0,
        dur_nanos,
    );
    metrics.timeouts = Some(0);

    BenchmarkRun {
        schema_version: REPORT_SCHEMA_VERSION.to_string(),
        run_id: format!("{scenario_id}-{transport}"),
        timestamp: format_rfc3339(now),
        git_commit: git_commit.to_string(),
        host_info: host_info.clone(),
        tool_pins: ToolPins::detect(transport, git_commit),
        scenario: ScenarioInfo {
            id: scenario_id.to_string(),
            name: scenario_name.to_string(),
            category: Some("primary".to_string()),
            rpc_type: "unary".to_string(),
            request_size_bytes: Some(req_size),
            response_size_bytes: Some(resp_size),
            description: None,
        },
        transport,
        connections: 1,
        concurrency: 1,
        config: Some(BenchmarkConfig {
            transport,
            connections: 1,
            concurrency: 1,
            warmup_duration_nanos: None,
            target_duration_nanos: Some(dur_nanos),
            repetition_index: None,
        }),
        start_end: StartEndConditions {
            start_time_rfc3339: format_rfc3339(start_time),
            end_time_rfc3339: format_rfc3339(now),
            duration_nanos: dur_nanos,
            warmup_completed: true,
            early_termination: false,
            termination_reason: None,
        },
        metrics,
        latency,
    }
}

pub fn make_qps_run(
    scenario_id: &str,
    scenario_name: &str,
    transport: TransportMode,
    conc: u32,
    conns: u32,
    dur: Duration,
    round_idx: usize,
    ok: u64,
    err: u64,
    large: bool,
    host_info: &HostInfo,
    git_commit: &str,
) -> BenchmarkRun {
    let dur_nanos = dur.as_nanos().max(1) as u64;
    let now = SystemTime::now();
    let start_time = now - dur;

    let mut metrics = RpcMetrics::new(ok + err, ok, err, dur_nanos);
    metrics.timeouts = Some(0);
    if err > 0 {
        metrics.record_status_error("UNKNOWN", err);
    }
    metrics.throughput_qps = Some(rate(ok, dur) as f64);

    let (req_size, resp_size) = if large {
        (LARGE_REQ as usize, LARGE_RESP as usize)
    } else {
        (0, 0)
    };

    BenchmarkRun {
        schema_version: REPORT_SCHEMA_VERSION.to_string(),
        run_id: format!("{scenario_id}-{transport}-round{round_idx}"),
        timestamp: format_rfc3339(now),
        git_commit: git_commit.to_string(),
        host_info: host_info.clone(),
        tool_pins: ToolPins::detect(transport, git_commit),
        scenario: ScenarioInfo {
            id: scenario_id.to_string(),
            name: scenario_name.to_string(),
            category: Some("primary".to_string()),
            rpc_type: "unary".to_string(),
            request_size_bytes: Some(req_size),
            response_size_bytes: Some(resp_size),
            description: None,
        },
        transport,
        connections: conns,
        concurrency: conc,
        config: Some(BenchmarkConfig {
            transport,
            connections: conns,
            concurrency: conc,
            warmup_duration_nanos: None,
            target_duration_nanos: Some(dur_nanos),
            repetition_index: Some(round_idx as u32),
        }),
        start_end: StartEndConditions {
            start_time_rfc3339: format_rfc3339(start_time),
            end_time_rfc3339: format_rfc3339(now),
            duration_nanos: dur_nanos,
            warmup_completed: true,
            early_termination: false,
            termination_reason: None,
        },
        metrics,
        latency: None,
    }
}

pub fn make_streaming_run(
    scenario_id: &str,
    scenario_name: &str,
    rpc_type: &str,
    transport: TransportMode,
    round_idx: usize,
    count: u64,
    dur: Duration,
    host_info: &HostInfo,
    git_commit: &str,
) -> BenchmarkRun {
    let dur_nanos = dur.as_nanos().max(1) as u64;
    let now = SystemTime::now();
    let start_time = now - dur;

    let mut metrics = RpcMetrics::new(count, count, 0, dur_nanos);
    metrics.timeouts = Some(0);
    metrics.throughput_qps = Some(rate(count, dur) as f64);
    if rpc_type == "server_streaming" {
        metrics.messages_received = Some(count);
        metrics.bytes_received = Some(count * STREAM_SIZE as u64);
    } else if rpc_type == "client_streaming" {
        metrics.messages_sent = Some(count);
        metrics.bytes_sent = Some(count * STREAM_SIZE as u64);
    }

    let (req_size, resp_size) = match rpc_type {
        "server_streaming" => (64, STREAM_SIZE as usize),
        "client_streaming" => (STREAM_SIZE as usize, 64),
        _ => (0, 0),
    };

    BenchmarkRun {
        schema_version: REPORT_SCHEMA_VERSION.to_string(),
        run_id: format!("{scenario_id}-{transport}-round{round_idx}"),
        timestamp: format_rfc3339(now),
        git_commit: git_commit.to_string(),
        host_info: host_info.clone(),
        tool_pins: ToolPins::detect(transport, git_commit),
        scenario: ScenarioInfo {
            id: scenario_id.to_string(),
            name: scenario_name.to_string(),
            category: Some("primary".to_string()),
            rpc_type: rpc_type.to_string(),
            request_size_bytes: Some(req_size),
            response_size_bytes: Some(resp_size),
            description: None,
        },
        transport,
        connections: 1,
        concurrency: 1,
        config: Some(BenchmarkConfig {
            transport,
            connections: 1,
            concurrency: 1,
            warmup_duration_nanos: None,
            target_duration_nanos: Some(dur_nanos),
            repetition_index: Some(round_idx as u32),
        }),
        start_end: StartEndConditions {
            start_time_rfc3339: format_rfc3339(start_time),
            end_time_rfc3339: format_rfc3339(now),
            duration_nanos: dur_nanos,
            warmup_completed: true,
            early_termination: false,
            termination_reason: None,
        },
        metrics,
        latency: None,
    }
}

// ---------------------------------------------------------------------------
// Role 1: Server runner
// ---------------------------------------------------------------------------

pub async fn run_server(
    config: ServerConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_addr: SocketAddr =
        format!("{}:{}", config.host, config.port)
            .parse()
            .map_err(|e| {
                format!(
                    "invalid bind address '{}:{}': {e}",
                    config.host, config.port
                )
            })?;

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("failed to bind to {bind_addr}: {e}"))?;
    let local_addr = listener.local_addr()?;

    // Output readiness line on stdout
    println!(
        "READY port={} addr={} transport={}",
        local_addr.port(),
        local_addr,
        config.transport
    );
    std::io::stdout().flush()?;
    eprintln!("server listening on {local_addr} ({})", config.transport);

    let max_duration = config.timeout_secs.map(Duration::from_secs);

    let server_fut = async {
        match config.transport {
            TransportMode::Native => TestServiceServer::new(InteropTestService)
                .serve_listener(listener)
                .await
                .map_err(|e| format!("native server error: {e}")),
            TransportMode::Tonic => Server::builder()
                .add_service(tonic_gen::TestServiceServer::new(TonicInterop))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .map_err(|e| format!("tonic server error: {e}")),
        }
    };

    if let Some(dur) = max_duration {
        tokio::select! {
            res = server_fut => {
                if let Err(e) = res {
                    eprintln!("{e}");
                    return Err(e.into());
                }
            }
            _ = tokio::time::sleep(dur) => {
                eprintln!("server reached max timeout, shutting down");
            }
        }
    } else if let Err(e) = server_fut.await {
        eprintln!("{e}");
        return Err(e.into());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Role 2: Client runner
// ---------------------------------------------------------------------------

pub async fn run_client(
    config: ClientConfig,
) -> Result<BenchmarkReport, Box<dyn std::error::Error + Send + Sync>> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&config.server_addr)
        .await
        .map_err(|e| {
            format!(
                "failed to resolve server_addr '{}': {e}",
                config.server_addr
            )
        })?
        .collect();
    let addr = addrs
        .first()
        .copied()
        .ok_or_else(|| format!("no addresses resolved for '{}'", config.server_addr))?;

    let host_info = HostInfo::detect();
    let git_commit = detect_git_commit();
    let mut runs: Vec<BenchmarkRun> = Vec::new();

    let bench_cfg = if config.quick {
        BenchConfig::quick()
    } else {
        BenchConfig::standard()
    };

    let run_all = config.shapes.contains(&CallShape::All) || config.shapes.is_empty();
    let run_unary = run_all || config.shapes.contains(&CallShape::Unary);
    let run_stream = run_all || config.shapes.contains(&CallShape::Stream);
    let run_ping = run_all || config.shapes.contains(&CallShape::PingPong);
    let run_upload = run_all || config.shapes.contains(&CallShape::Upload);
    let run_qps = run_all || config.shapes.contains(&CallShape::Qps);

    match config.transport {
        TransportMode::Native => {
            if run_unary {
                let (empty, large) = latency_kernel(addr, &bench_cfg).await?;
                runs.push(make_latency_run(
                    "unary_empty_plaintext",
                    "Unary Empty Payload (Plaintext)",
                    0,
                    0,
                    TransportMode::Native,
                    &empty,
                    &host_info,
                    &git_commit,
                ));
                runs.push(make_latency_run(
                    "unary_large_plaintext",
                    "Unary Large Payload (Plaintext)",
                    LARGE_REQ as usize,
                    LARGE_RESP as usize,
                    TransportMode::Native,
                    &large,
                    &host_info,
                    &git_commit,
                ));
                println!(
                    "empty_unary native_p50={} native_p99={}",
                    empty.latency.p50, empty.latency.p99
                );
                println!(
                    "large_unary native_p50={} native_p99={}",
                    large.latency.p50, large.latency.p99
                );
            }

            if run_qps {
                let dur = Duration::from_secs_f64(bench_cfg.qps_secs);
                for (label, conc, conns) in [
                    ("low", QPS_CONC_LOW, QPS_CONNS_LOW),
                    ("high", QPS_CONC_HIGH, QPS_CONNS_HIGH),
                ] {
                    for (shape, large) in [("empty", false), ("large", true)] {
                        let (qps, err, r_rounds) = best_qps(bench_cfg.qps_rounds, dur, || {
                            qps_kernel(addr, conc, conns, dur, large)
                        })
                        .await;
                        for (idx, ok, err_cnt) in r_rounds {
                            runs.push(make_qps_run(
                                &format!("qps_{shape}_{label}"),
                                &format!("QPS {shape} {label}"),
                                TransportMode::Native,
                                conc,
                                conns as u32,
                                dur,
                                idx,
                                ok,
                                err_cnt,
                                large,
                                &host_info,
                                &git_commit,
                            ));
                        }
                        println!(
                            "qps {shape} {label} conc={conc} conns={conns} native={qps} err={err}"
                        );
                    }
                }
            }

            if run_stream {
                let s_res = stream_kernel(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &s_res.rounds {
                    runs.push(make_streaming_run(
                        "server_streaming_1kib_download",
                        "Server Streaming 1 KiB Download",
                        "server_streaming",
                        TransportMode::Native,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                let bytes_per_msg = bench_cfg.stream_size as u64;
                println!(
                    "stream msgs={} size={} native_msgs_per_s={} native_mib_per_s={}",
                    bench_cfg.stream_msgs,
                    bench_cfg.stream_size,
                    s_res.best,
                    s_res.best * bytes_per_msg / (1024 * 1024)
                );
            }

            if run_ping {
                let p_res = ping_pong_kernel(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &p_res.rounds {
                    runs.push(make_streaming_run(
                        "bidi_ping_pong_empty",
                        "Bidirectional Streaming Ping-Pong Empty",
                        "bidi_streaming",
                        TransportMode::Native,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                println!(
                    "ping_pong pairs={} native_round_trips_per_s={}",
                    bench_cfg.ping_pongs, p_res.best
                );
            }

            if run_upload {
                let u_res = upload_kernel(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &u_res.rounds {
                    runs.push(make_streaming_run(
                        "client_streaming_1kib_upload",
                        "Client Streaming 1 KiB Upload",
                        "client_streaming",
                        TransportMode::Native,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                let bytes_per_msg = bench_cfg.stream_size as u64;
                println!(
                    "upload msgs={} size={} native_msgs_per_s={} native_mib_per_s={}",
                    bench_cfg.stream_msgs,
                    bench_cfg.stream_size,
                    u_res.best,
                    u_res.best * bytes_per_msg / (1024 * 1024)
                );
            }
        }

        TransportMode::Tonic => {
            if run_unary {
                let (empty, large) = latency_tonic(addr, &bench_cfg).await?;
                runs.push(make_latency_run(
                    "unary_empty_plaintext",
                    "Unary Empty Payload (Plaintext)",
                    0,
                    0,
                    TransportMode::Tonic,
                    &empty,
                    &host_info,
                    &git_commit,
                ));
                runs.push(make_latency_run(
                    "unary_large_plaintext",
                    "Unary Large Payload (Plaintext)",
                    LARGE_REQ as usize,
                    LARGE_RESP as usize,
                    TransportMode::Tonic,
                    &large,
                    &host_info,
                    &git_commit,
                ));
                println!(
                    "empty_unary tonic_p50={} tonic_p99={}",
                    empty.latency.p50, empty.latency.p99
                );
                println!(
                    "large_unary tonic_p50={} tonic_p99={}",
                    large.latency.p50, large.latency.p99
                );
            }

            if run_qps {
                let dur = Duration::from_secs_f64(bench_cfg.qps_secs);
                for (label, conc, conns) in [
                    ("low", QPS_CONC_LOW, QPS_CONNS_LOW),
                    ("high", QPS_CONC_HIGH, QPS_CONNS_HIGH),
                ] {
                    for (shape, large) in [("empty", false), ("large", true)] {
                        let (qps, err, r_rounds) = best_qps(bench_cfg.qps_rounds, dur, || {
                            qps_tonic(addr, conc, conns, dur, large)
                        })
                        .await;
                        for (idx, ok, err_cnt) in r_rounds {
                            runs.push(make_qps_run(
                                &format!("qps_{shape}_{label}"),
                                &format!("QPS {shape} {label}"),
                                TransportMode::Tonic,
                                conc,
                                conns as u32,
                                dur,
                                idx,
                                ok,
                                err_cnt,
                                large,
                                &host_info,
                                &git_commit,
                            ));
                        }
                        println!(
                            "qps {shape} {label} conc={conc} conns={conns} tonic={qps} err={err}"
                        );
                    }
                }
            }

            if run_stream {
                let s_res = stream_tonic(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &s_res.rounds {
                    runs.push(make_streaming_run(
                        "server_streaming_1kib_download",
                        "Server Streaming 1 KiB Download",
                        "server_streaming",
                        TransportMode::Tonic,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                let bytes_per_msg = bench_cfg.stream_size as u64;
                println!(
                    "stream msgs={} size={} tonic_msgs_per_s={} tonic_mib_per_s={}",
                    bench_cfg.stream_msgs,
                    bench_cfg.stream_size,
                    s_res.best,
                    s_res.best * bytes_per_msg / (1024 * 1024)
                );
            }

            if run_ping {
                let p_res = ping_pong_tonic(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &p_res.rounds {
                    runs.push(make_streaming_run(
                        "bidi_ping_pong_empty",
                        "Bidirectional Streaming Ping-Pong Empty",
                        "bidi_streaming",
                        TransportMode::Tonic,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                println!(
                    "ping_pong pairs={} tonic_round_trips_per_s={}",
                    bench_cfg.ping_pongs, p_res.best
                );
            }

            if run_upload {
                let u_res = upload_tonic(addr, &bench_cfg).await?;
                for (idx, count, round_dur) in &u_res.rounds {
                    runs.push(make_streaming_run(
                        "client_streaming_1kib_upload",
                        "Client Streaming 1 KiB Upload",
                        "client_streaming",
                        TransportMode::Tonic,
                        *idx,
                        *count,
                        *round_dur,
                        &host_info,
                        &git_commit,
                    ));
                }
                let bytes_per_msg = bench_cfg.stream_size as u64;
                println!(
                    "upload msgs={} size={} tonic_msgs_per_s={} tonic_mib_per_s={}",
                    bench_cfg.stream_msgs,
                    bench_cfg.stream_size,
                    u_res.best,
                    u_res.best * bytes_per_msg / (1024 * 1024)
                );
            }
        }
    }

    let report = BenchmarkReport::new(runs);
    if let Err(e) = report.validate() {
        eprintln!("benchmark report validation warning: {e}");
    }

    if config.print_json {
        if let Ok(json) = report.to_json_pretty() {
            println!("{json}");
        }
    }
    if let Some(ref path) = config.output_file {
        if let Err(e) = report.save_to_file(path) {
            eprintln!("failed to save benchmark report to {path}: {e}");
        } else {
            println!("saved benchmark report to {path}");
        }
    }
    println!(
        "recorded {} benchmark run records in BenchmarkReport (schema {})",
        report.runs.len(),
        report.schema_version
    );

    Ok(report)
}

// ---------------------------------------------------------------------------
// Role 3: Smoke runner (existing single-process loopback behavior)
// ---------------------------------------------------------------------------

pub async fn run_smoke(
    config: SmokeConfig,
) -> Result<BenchmarkReport, Box<dyn std::error::Error + Send + Sync>> {
    let host_info = HostInfo::detect();
    let git_commit = detect_git_commit();
    let mut runs: Vec<BenchmarkRun> = Vec::new();

    let bench_cfg = if config.quick {
        BenchConfig::quick()
    } else {
        BenchConfig::standard()
    };

    let k_listener = TcpListener::bind("127.0.0.1:0").await?;
    let k_addr = k_listener.local_addr()?;
    tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(k_listener)
            .await
            .ok();
    });

    let t_listener = TcpListener::bind("127.0.0.1:0").await?;
    let t_addr = t_listener.local_addr()?;
    tokio::spawn(async move {
        Server::builder()
            .add_service(tonic_gen::TestServiceServer::new(TonicInterop))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(t_listener))
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (k_empty, k_large) = latency_kernel(k_addr, &bench_cfg).await?;
    let (t_empty, t_large) = latency_tonic(t_addr, &bench_cfg).await?;

    runs.push(make_latency_run(
        "unary_empty_plaintext",
        "Unary Empty Payload (Plaintext)",
        0,
        0,
        TransportMode::Native,
        &k_empty,
        &host_info,
        &git_commit,
    ));
    runs.push(make_latency_run(
        "unary_empty_plaintext",
        "Unary Empty Payload (Plaintext)",
        0,
        0,
        TransportMode::Tonic,
        &t_empty,
        &host_info,
        &git_commit,
    ));

    runs.push(make_latency_run(
        "unary_large_plaintext",
        "Unary Large Payload (Plaintext)",
        LARGE_REQ as usize,
        LARGE_RESP as usize,
        TransportMode::Native,
        &k_large,
        &host_info,
        &git_commit,
    ));
    runs.push(make_latency_run(
        "unary_large_plaintext",
        "Unary Large Payload (Plaintext)",
        LARGE_REQ as usize,
        LARGE_RESP as usize,
        TransportMode::Tonic,
        &t_large,
        &host_info,
        &git_commit,
    ));

    println!(
        "empty_unary kernel_p50={} kernel_p99={} tonic_p50={} tonic_p99={}",
        k_empty.latency.p50, k_empty.latency.p99, t_empty.latency.p50, t_empty.latency.p99
    );
    println!(
        "large_unary kernel_p50={} kernel_p99={} tonic_p50={} tonic_p99={}",
        k_large.latency.p50, k_large.latency.p99, t_large.latency.p50, t_large.latency.p99
    );

    let dur = Duration::from_secs_f64(bench_cfg.qps_secs);
    let mut errors = 0;
    for (label, conc, conns) in [
        ("low", QPS_CONC_LOW, QPS_CONNS_LOW),
        ("high", QPS_CONC_HIGH, QPS_CONNS_HIGH),
    ] {
        for (shape, large) in [("empty", false), ("large", true)] {
            let (kernel, kerr, k_rounds) = best_qps(bench_cfg.qps_rounds, dur, || {
                qps_kernel(k_addr, conc, conns, dur, large)
            })
            .await;
            let (tonic, terr, t_rounds) = best_qps(bench_cfg.qps_rounds, dur, || {
                qps_tonic(t_addr, conc, conns, dur, large)
            })
            .await;
            errors += kerr + terr;

            for (idx, ok, err_cnt) in k_rounds {
                runs.push(make_qps_run(
                    &format!("qps_{shape}_{label}"),
                    &format!("QPS {shape} {label}"),
                    TransportMode::Native,
                    conc,
                    conns as u32,
                    dur,
                    idx,
                    ok,
                    err_cnt,
                    large,
                    &host_info,
                    &git_commit,
                ));
            }
            for (idx, ok, err_cnt) in t_rounds {
                runs.push(make_qps_run(
                    &format!("qps_{shape}_{label}"),
                    &format!("QPS {shape} {label}"),
                    TransportMode::Tonic,
                    conc,
                    conns as u32,
                    dur,
                    idx,
                    ok,
                    err_cnt,
                    large,
                    &host_info,
                    &git_commit,
                ));
            }

            println!(
                "qps {shape} {label} conc={conc} conns={conns} kernel={kernel} tonic={tonic} \
                 kernel_err={kerr} tonic_err={terr}"
            );
        }
    }

    let k_stream = stream_kernel(k_addr, &bench_cfg).await?;
    let t_stream = stream_tonic(t_addr, &bench_cfg).await?;
    for (idx, count, round_dur) in &k_stream.rounds {
        runs.push(make_streaming_run(
            "server_streaming_1kib_download",
            "Server Streaming 1 KiB Download",
            "server_streaming",
            TransportMode::Native,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }
    for (idx, count, round_dur) in &t_stream.rounds {
        runs.push(make_streaming_run(
            "server_streaming_1kib_download",
            "Server Streaming 1 KiB Download",
            "server_streaming",
            TransportMode::Tonic,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }

    let bytes_per_msg = bench_cfg.stream_size as u64;
    println!(
        "stream msgs={} size={} kernel_msgs_per_s={} \
         tonic_msgs_per_s={} kernel_mib_per_s={} tonic_mib_per_s={}",
        bench_cfg.stream_msgs,
        bench_cfg.stream_size,
        k_stream.best,
        t_stream.best,
        k_stream.best * bytes_per_msg / (1024 * 1024),
        t_stream.best * bytes_per_msg / (1024 * 1024)
    );

    let k_ping = ping_pong_kernel(k_addr, &bench_cfg).await?;
    let t_ping = ping_pong_tonic(t_addr, &bench_cfg).await?;
    for (idx, count, round_dur) in &k_ping.rounds {
        runs.push(make_streaming_run(
            "bidi_ping_pong_empty",
            "Bidirectional Streaming Ping-Pong Empty",
            "bidi_streaming",
            TransportMode::Native,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }
    for (idx, count, round_dur) in &t_ping.rounds {
        runs.push(make_streaming_run(
            "bidi_ping_pong_empty",
            "Bidirectional Streaming Ping-Pong Empty",
            "bidi_streaming",
            TransportMode::Tonic,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }

    println!(
        "ping_pong pairs={} kernel_round_trips_per_s={} \
         tonic_round_trips_per_s={}",
        bench_cfg.ping_pongs, k_ping.best, t_ping.best
    );

    let k_upload = upload_kernel(k_addr, &bench_cfg).await?;
    let t_upload = upload_tonic(t_addr, &bench_cfg).await?;
    for (idx, count, round_dur) in &k_upload.rounds {
        runs.push(make_streaming_run(
            "client_streaming_1kib_upload",
            "Client Streaming 1 KiB Upload",
            "client_streaming",
            TransportMode::Native,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }
    for (idx, count, round_dur) in &t_upload.rounds {
        runs.push(make_streaming_run(
            "client_streaming_1kib_upload",
            "Client Streaming 1 KiB Upload",
            "client_streaming",
            TransportMode::Tonic,
            *idx,
            *count,
            *round_dur,
            &host_info,
            &git_commit,
        ));
    }

    println!(
        "upload msgs={} size={} kernel_msgs_per_s={} \
         tonic_msgs_per_s={} kernel_mib_per_s={} tonic_mib_per_s={}",
        bench_cfg.stream_msgs,
        bench_cfg.stream_size,
        k_upload.best,
        t_upload.best,
        k_upload.best * bytes_per_msg / (1024 * 1024),
        t_upload.best * bytes_per_msg / (1024 * 1024)
    );

    let report = BenchmarkReport::new(runs);
    if let Err(e) = report.validate() {
        eprintln!("benchmark report validation warning: {e}");
    }

    if config.print_json {
        if let Ok(json) = report.to_json_pretty() {
            println!("{json}");
        }
    }
    if let Some(ref path) = config.output_file {
        if let Err(e) = report.save_to_file(path) {
            eprintln!("failed to save benchmark report to {path}: {e}");
        } else {
            println!("saved benchmark report to {path}");
        }
    }
    println!(
        "recorded {} benchmark run records in BenchmarkReport (schema {})",
        report.runs.len(),
        report.schema_version
    );

    let mut failed = false;
    if !k_empty.latency.beats(t_empty.latency) {
        eprintln!(
            "perf gate failed: empty_unary kernel p50={} p99={} vs tonic p50={} p99={}",
            k_empty.latency.p50, k_empty.latency.p99, t_empty.latency.p50, t_empty.latency.p99
        );
        failed = true;
    }
    if !k_large.latency.beats(t_large.latency) {
        eprintln!(
            "perf gate failed: large_unary kernel p50={} p99={} vs tonic p50={} p99={}",
            k_large.latency.p50, k_large.latency.p99, t_large.latency.p50, t_large.latency.p99
        );
        failed = true;
    }
    if (k_stream.best as f64) < (t_stream.best as f64) * STREAM_PARITY {
        eprintln!(
            "perf gate failed: stream kernel {} vs tonic {} msgs/s (must be within {}%)",
            k_stream.best,
            t_stream.best,
            ((1.0 - STREAM_PARITY) * 100.0).round()
        );
        failed = true;
    }
    if (k_ping.best as f64) < (t_ping.best as f64) * PING_PONG_PARITY {
        eprintln!(
            "perf gate failed: ping_pong kernel {} vs tonic {} round-trips/s (must be within {}%)",
            k_ping.best,
            t_ping.best,
            ((1.0 - PING_PONG_PARITY) * 100.0).round()
        );
        failed = true;
    }
    if (k_upload.best as f64) < (t_upload.best as f64) * UPLOAD_PARITY {
        eprintln!(
            "perf gate failed: upload kernel {} vs tonic {} msgs/s (must be within {}%)",
            k_upload.best,
            t_upload.best,
            ((1.0 - UPLOAD_PARITY) * 100.0).round()
        );
        failed = true;
    }
    if errors != 0 {
        eprintln!("rpc-bench failed: {errors} RPC errors");
        failed = true;
    }

    if failed {
        return Err("smoke perf gate failed".into());
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// CLI Argument Parsing
// ---------------------------------------------------------------------------

pub fn usage() -> &'static str {
    "Usage: rpc-bench [SUBCOMMAND|OPTIONS]\n\n\
     Subcommands:\n  \
       server    Start a gRPC server (native or tonic) listening on --port\n  \
       client    Run gRPC benchmark against --server_addr=host:port\n  \
       smoke     Run loopback in-process benchmark (default when no subcommand)\n\n\
     Server options:\n  \
       --port <PORT>            Port to listen on (default: 0 for dynamic/random)\n  \
       --host <HOST>            Host to bind to (default: 127.0.0.1)\n  \
       --transport <MODE>       Transport mode: native or tonic (default: native)\n  \
       --timeout-secs <SECS>    Maximum runtime before clean shutdown (default: infinite)\n\n\
     Client options:\n  \
       --server_addr <ADDR>     Target host:port (required for client)\n  \
       --transport <MODE>       Client transport: native or tonic (default: native)\n  \
       --shape <SHAPE>          Benchmark shape: unary, stream, ping_pong, upload, qps, all (default: all)\n  \
       --quick                  Run abbreviated benchmark with reduced iterations\n  \
       --output, -o <PATH>      Save benchmark report JSON to file\n  \
       --json                   Print benchmark report JSON to stdout\n\n\
     Smoke options:\n  \
       --quick                  Run abbreviated loopback benchmark\n  \
       --output, -o <PATH>      Save benchmark report JSON to file\n  \
       --json                   Print benchmark report JSON to stdout\n"
}

fn get_arg_val(args: &[String], flag: &str) -> Option<String> {
    for i in 0..args.len() {
        if let Some(val) = args[i].strip_prefix(&format!("{flag}=")) {
            return Some(val.to_string());
        }
        if args[i] == flag && i + 1 < args.len() && !args[i + 1].starts_with('-') {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter()
        .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
}

fn parse_transport(args: &[String]) -> Result<TransportMode, String> {
    let raw = get_arg_val(args, "--transport")
        .or_else(|| get_arg_val(args, "--server_type"))
        .or_else(|| get_arg_val(args, "--server-type"))
        .or_else(|| get_arg_val(args, "--client_type"))
        .or_else(|| get_arg_val(args, "--client-type"))
        .unwrap_or_else(|| "native".to_string());

    match raw.to_ascii_lowercase().as_str() {
        "native" | "kernel" | "pbrs" => Ok(TransportMode::Native),
        "tonic" => Ok(TransportMode::Tonic),
        other => Err(format!(
            "unknown transport mode '{other}': expected 'native' or 'tonic'"
        )),
    }
}

pub fn parse_args(args: &[String]) -> Result<ProcessRole, String> {
    if args
        .iter()
        .any(|a| a == "--help" || a == "-h" || a == "help")
    {
        println!("{}", usage());
        std::process::exit(0);
    }

    let subcmd = args.get(1).map(String::as_str);

    let is_server = subcmd == Some("server")
        || has_flag(args, "--server")
        || get_arg_val(args, "--role").as_deref() == Some("server");
    let is_client = subcmd == Some("client")
        || has_flag(args, "--client")
        || get_arg_val(args, "--role").as_deref() == Some("client");
    let is_smoke = subcmd == Some("smoke")
        || has_flag(args, "--smoke")
        || get_arg_val(args, "--role").as_deref() == Some("smoke");

    if is_server && is_client {
        return Err("cannot specify both server and client roles".to_string());
    }

    if is_server {
        let transport = parse_transport(args)?;
        let host = get_arg_val(args, "--host")
            .or_else(|| get_arg_val(args, "--bind"))
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port: u16 = get_arg_val(args, "--port")
            .or_else(|| get_arg_val(args, "-p"))
            .map(|p| p.parse().map_err(|e| format!("invalid --port '{p}': {e}")))
            .transpose()?
            .unwrap_or(0);
        let timeout_secs: Option<u64> = get_arg_val(args, "--timeout-secs")
            .or_else(|| get_arg_val(args, "--timeout_secs"))
            .or_else(|| get_arg_val(args, "--timeout"))
            .or_else(|| get_arg_val(args, "--max-seconds"))
            .or_else(|| get_arg_val(args, "--max_seconds"))
            .map(|s| s.parse().map_err(|e| format!("invalid timeout '{s}': {e}")))
            .transpose()?;

        return Ok(ProcessRole::Server(ServerConfig {
            host,
            port,
            transport,
            timeout_secs,
        }));
    }

    if is_client {
        let transport = parse_transport(args)?;
        let server_addr = get_arg_val(args, "--server_addr")
            .or_else(|| get_arg_val(args, "--server-addr"))
            .or_else(|| get_arg_val(args, "--serveraddr"))
            .or_else(|| get_arg_val(args, "--target"))
            .ok_or_else(|| "--server_addr=host:port is required for client role".to_string())?;

        let quick = has_flag(args, "--quick") || has_flag(args, "-q");
        let print_json = has_flag(args, "--json");
        let output_file = get_arg_val(args, "--output")
            .or_else(|| get_arg_val(args, "-o"))
            .or_else(|| get_arg_val(args, "--report"))
            .or_else(|| std::env::var("BENCH_REPORT_PATH").ok());

        let mut shapes = HashSet::new();
        if let Some(shape_str) =
            get_arg_val(args, "--shape").or_else(|| get_arg_val(args, "--scenario"))
        {
            for part in shape_str.split(',') {
                let part = part.trim();
                if let Some(cs) = CallShape::parse(part) {
                    shapes.insert(cs);
                } else {
                    return Err(format!(
                        "unknown call shape '{part}': expected unary, stream, ping_pong, upload, qps, all"
                    ));
                }
            }
        } else {
            shapes.insert(CallShape::All);
        }

        return Ok(ProcessRole::Client(ClientConfig {
            server_addr,
            transport,
            quick,
            shapes,
            output_file,
            print_json,
        }));
    }

    // Default to Smoke
    let quick = has_flag(args, "--quick") || has_flag(args, "-q");
    let print_json = has_flag(args, "--json");
    let output_file = get_arg_val(args, "--output")
        .or_else(|| get_arg_val(args, "-o"))
        .or_else(|| get_arg_val(args, "--report"))
        .or_else(|| std::env::var("BENCH_REPORT_PATH").ok());

    if !is_smoke && subcmd.is_some() && !subcmd.unwrap().starts_with('-') {
        return Err(format!("unknown subcommand '{}'", subcmd.unwrap()));
    }

    Ok(ProcessRole::Smoke(SmokeConfig {
        quick,
        output_file,
        print_json,
    }))
}
