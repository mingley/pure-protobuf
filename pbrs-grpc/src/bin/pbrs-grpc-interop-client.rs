//! Official-shape interop client: `-test_case` `-server_host` `-server_port`
//! `-use_tls=false`.
//!
//! `--bench` replaces the test case with a latency and throughput measurement
//! against whatever server is listening. Pointing it at this kernel's server
//! and then at another implementation's is a single-variable comparison: same
//! client, same `.proto`, same codec, different server. Unary latency is
//! empty_unary / large_unary. Throughput is empty bidi ping-pong and
//! client-streaming upload, matching `rpc-bench` payloads.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop binary"
)]

use pbrs_grpc::interop_cases;
use pbrs_grpc::{
    Channel, ClientTls, Payload, Request, ResponseParameters, Status, StreamingInputCallRequest,
    StreamingOutputCallRequest, TestServiceClient,
};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

struct Args {
    server_host: String,
    server_port: u16,
    test_case: String,
    use_tls: bool,
    tls_ca_file: Option<String>,
    server_host_override: Option<String>,
    bench: bool,
    soak_iterations: usize,
    max_failures: usize,
    per_rpc_timeout_ms: u64,
    overall_timeout_seconds: u64,
    soak_min_time_ms_between_rpcs: u64,
    soak_request_size: i32,
    soak_response_size: i32,
    soak_num_threads: usize,
    qualification: Option<bool>,
}

fn parse_port(flag: &str, val: &str) -> u16 {
    match val.parse::<u16>() {
        Ok(p) if p >= 1 => p,
        Ok(_) => {
            eprintln!("invalid port 0 for {flag}: must be between 1 and 65535");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!(
                "invalid port value {val:?} for {flag}: must be an integer between 1 and 65535"
            );
            std::process::exit(1);
        }
    }
}

fn parse_bool(flag: &str, val: &str) -> bool {
    match val.to_ascii_lowercase().as_str() {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => {
            eprintln!("invalid boolean value {val:?} for {flag}");
            std::process::exit(1);
        }
    }
}

fn parse_positive_usize(flag: &str, val: &str) -> usize {
    match val.parse::<usize>() {
        Ok(n) if n >= 1 => n,
        Ok(_) => {
            eprintln!("invalid value 0 for {flag}: must be at least 1");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("invalid value {val:?} for {flag}: must be a positive integer");
            std::process::exit(1);
        }
    }
}

fn parse_non_negative_usize(flag: &str, val: &str) -> usize {
    match val.parse::<usize>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("invalid value {val:?} for {flag}: must be a non-negative integer");
            std::process::exit(1);
        }
    }
}

fn parse_u64(flag: &str, val: &str) -> u64 {
    match val.parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("invalid value {val:?} for {flag}: must be a non-negative integer");
            std::process::exit(1);
        }
    }
}

fn parse_i32(flag: &str, val: &str) -> i32 {
    match val.parse::<i32>() {
        Ok(n) if n >= 0 => n,
        _ => {
            eprintln!("invalid value {val:?} for {flag}: must be a non-negative integer");
            std::process::exit(1);
        }
    }
}

fn parse_args() -> Args {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut server_host = "127.0.0.1".to_string();
    let mut server_port = 10000u16;
    let mut test_case = "empty_unary".to_string();
    let mut use_tls = false;
    let mut tls_ca_file = None;
    let mut server_host_override = None;
    let mut bench = false;
    let mut soak_iterations = 10usize;
    let mut max_failures = 0usize;
    let mut per_rpc_timeout_ms = 1000u64;
    let mut overall_timeout_seconds = 10u64;
    let mut soak_min_time_ms_between_rpcs = 0u64;
    let mut soak_request_size = 271828i32;
    let mut soak_response_size = 314159i32;
    let mut soak_num_threads = 1usize;
    let mut qualification = None;

    let mut i = 0;
    while i < raw_args.len() {
        let Some(arg) = raw_args.get(i) else {
            break;
        };
        if !arg.starts_with('-') {
            eprintln!("unexpected positional argument: {arg}");
            std::process::exit(1);
        }

        let (raw_key, inline_val) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };

        let normalized = raw_key.trim_start_matches('-');

        match normalized {
            "server_host" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                server_host = val;
            }
            "server_port" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                server_port = parse_port(raw_key, &val);
            }
            "test_case" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                test_case = val;
            }
            "use_tls" => {
                use_tls = match inline_val {
                    Some(v) => parse_bool(raw_key, &v),
                    None => {
                        if let Some(next) = raw_args.get(i + 1) {
                            if !next.starts_with('-') {
                                i += 1;
                                parse_bool(raw_key, next)
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    }
                };
            }
            "tls_ca_file" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                tls_ca_file = Some(val);
            }
            "server_host_override" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                server_host_override = Some(val);
            }
            "bench" => {
                bench = match inline_val {
                    Some(v) => parse_bool(raw_key, &v),
                    None => {
                        if let Some(next) = raw_args.get(i + 1) {
                            if !next.starts_with('-') {
                                i += 1;
                                parse_bool(raw_key, next)
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    }
                };
            }
            "soak_iterations" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                soak_iterations = parse_positive_usize(raw_key, &val);
            }
            "max_failures" | "soak_max_failures" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                max_failures = parse_non_negative_usize(raw_key, &val);
            }
            "per_rpc_timeout_ms"
            | "soak_per_iteration_max_acceptable_latency_ms"
            | "soak_per_rpc_timeout_ms" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                per_rpc_timeout_ms = parse_u64(raw_key, &val);
            }
            "overall_timeout_seconds" | "soak_overall_timeout_seconds" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                overall_timeout_seconds = parse_u64(raw_key, &val);
            }
            "soak_min_time_ms_between_rpcs" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                soak_min_time_ms_between_rpcs = parse_u64(raw_key, &val);
            }
            "soak_request_size" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                soak_request_size = parse_i32(raw_key, &val);
            }
            "soak_response_size" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                soak_response_size = parse_i32(raw_key, &val);
            }
            "soak_num_threads" | "soak_threads" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match raw_args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprintln!("missing value for flag {raw_key}");
                                std::process::exit(1);
                            }
                        }
                    }
                };
                soak_num_threads = parse_positive_usize(raw_key, &val);
            }
            "qualification" => {
                let b = match inline_val {
                    Some(v) => parse_bool(raw_key, &v),
                    None => {
                        if let Some(next) = raw_args.get(i + 1) {
                            if !next.starts_with('-') {
                                i += 1;
                                parse_bool(raw_key, next)
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    }
                };
                qualification = Some(b);
            }
            "smoke" => {
                let b = match inline_val {
                    Some(v) => parse_bool(raw_key, &v),
                    None => {
                        if let Some(next) = raw_args.get(i + 1) {
                            if !next.starts_with('-') {
                                i += 1;
                                parse_bool(raw_key, next)
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    }
                };
                qualification = Some(!b);
            }
            _ => {
                eprintln!("unknown flag: {arg}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if qualification == Some(true)
        && soak_iterations < interop_cases::QUALIFICATION_SOAK_MIN_ITERATIONS
    {
        eprintln!(
            "error: qualification soak requires at least {} iterations (got {}): shorter runs are local smoke tests and cannot be labeled qualification soak",
            interop_cases::QUALIFICATION_SOAK_MIN_ITERATIONS,
            soak_iterations
        );
        std::process::exit(1);
    }

    if soak_iterations % soak_num_threads != 0 {
        eprintln!(
            "error: soak_iterations ({}) must be divisible by soak_num_threads ({})",
            soak_iterations, soak_num_threads
        );
        std::process::exit(1);
    }

    Args {
        server_host,
        server_port,
        test_case,
        use_tls,
        tls_ca_file,
        server_host_override,
        bench,
        soak_iterations,
        max_failures,
        per_rpc_timeout_ms,
        overall_timeout_seconds,
        soak_min_time_ms_between_rpcs,
        soak_request_size,
        soak_response_size,
        soak_num_threads,
        qualification,
    }
}

/// Warmup iterations, then measured iterations, per case.
const BENCH_WARMUP: u32 = 64;
const BENCH_EMPTY_ITERS: u32 = 2000;
const BENCH_LARGE_ITERS: u32 = 200;
const LARGE_REQ: i32 = 271_828;
const LARGE_RESP: i32 = 314_159;
/// Empty request/response pairs in one bidi `FullDuplexCall`. Matches `rpc-bench`.
const PING_PONGS: u64 = 256;
/// Messages and payload size for one client-streaming `StreamingInputCall`.
const UPLOAD_MSGS: i32 = 2000;
const UPLOAD_SIZE: i32 = 1024;
const STREAM_ROUNDS: usize = 9;

/// Nearest-rank p50 and p99, computed in integers so no cast can lose
/// precision or a sign.
fn percentiles(mut samples: Vec<u128>) -> (u128, u128) {
    samples.sort_unstable();
    let n = samples.len();
    let pick = |percent: usize| {
        let last = n.saturating_sub(1);
        let index = (n * percent).div_ceil(100).saturating_sub(1).min(last);
        samples.get(index).copied().unwrap_or(0)
    };
    (pick(50), pick(99))
}

fn large_request() -> pbrs_grpc::SimpleRequest {
    let mut req = pbrs_grpc::SimpleRequest::new();
    req.set_response_size(LARGE_RESP);
    let mut payload = pbrs_grpc::Payload::new();
    payload.set_body(vec![0u8; LARGE_REQ as usize]);
    req.set_payload(payload);
    req
}

fn rate(count: u64, dur: Duration) -> u64 {
    let nanos = dur.as_nanos();
    if nanos == 0 {
        return 0;
    }
    let per_sec = u128::from(count).saturating_mul(1_000_000_000) / nanos;
    u64::try_from(per_sec).unwrap_or(u64::MAX)
}

fn ping_pong_req() -> StreamingOutputCallRequest {
    let mut req = StreamingOutputCallRequest::new();
    let mut p = ResponseParameters::new();
    p.set_size(0);
    req.response_parameters_mut().push(p);
    req
}

fn upload_req() -> StreamingInputCallRequest {
    let mut req = StreamingInputCallRequest::new();
    let mut p = Payload::new();
    p.set_body(vec![0u8; usize::try_from(UPLOAD_SIZE.max(0)).unwrap_or(0)]);
    req.set_payload(p);
    req
}

/// Best of eight rounds after warmup: empty bidi round-trips/s.
async fn bench_ping_pong(client: &TestServiceClient) -> Result<u64, Status> {
    let req = ping_pong_req();
    let mut best = 0u64;
    for round in 0..STREAM_ROUNDS {
        let started = Instant::now();
        let (tx, call) = client.full_duplex_call(Request::new(()));
        tx.send(req.clone()).await?;
        let mut inbound = call.await?.into_inner();
        if inbound.message().await?.is_none() {
            return Err(Status::internal("ping_pong missing first reply"));
        }
        for _ in 1..PING_PONGS {
            tx.send(req.clone()).await?;
            if inbound.message().await?.is_none() {
                return Err(Status::internal("ping_pong ended early"));
            }
        }
        tx.close();
        while inbound.message().await?.is_some() {}
        if round > 0 {
            best = best.max(rate(PING_PONGS, started.elapsed()));
        }
    }
    Ok(best)
}

/// Best of eight rounds after warmup: client-streaming messages/s.
async fn bench_upload(client: &TestServiceClient) -> Result<u64, Status> {
    let req = upload_req();
    let want = UPLOAD_MSGS.saturating_mul(UPLOAD_SIZE);
    let mut best = 0u64;
    for round in 0..STREAM_ROUNDS {
        let started = Instant::now();
        let (tx, call) = client.streaming_input_call(Request::new(()));
        let send = async {
            for _ in 0..UPLOAD_MSGS {
                tx.send(req.clone()).await?;
            }
            tx.close();
            Ok::<(), Status>(())
        };
        let (sent, resp) = tokio::join!(send, call);
        sent?;
        let got = resp?.into_inner().aggregated_payload_size();
        if got != want {
            return Err(Status::internal(format!("upload agg {got} want {want}")));
        }
        if round > 0 {
            let n = u64::try_from(UPLOAD_MSGS.max(0)).unwrap_or(0);
            best = best.max(rate(n, started.elapsed()));
        }
    }
    Ok(best)
}

/// Measure unary latency plus ping_pong / upload throughput.
async fn bench(client: &TestServiceClient) -> Result<(), Status> {
    for _ in 0..BENCH_WARMUP {
        client
            .empty_call(Request::new(pbrs_grpc::Empty::new()))
            .await?;
    }
    let mut empty = Vec::with_capacity(BENCH_EMPTY_ITERS as usize);
    for _ in 0..BENCH_EMPTY_ITERS {
        let start = Instant::now();
        client
            .empty_call(Request::new(pbrs_grpc::Empty::new()))
            .await?;
        empty.push(start.elapsed().as_nanos());
    }

    let request = large_request();
    for _ in 0..BENCH_WARMUP / 4 {
        client.unary_call(Request::new(request.clone())).await?;
    }
    let mut large = Vec::with_capacity(BENCH_LARGE_ITERS as usize);
    for _ in 0..BENCH_LARGE_ITERS {
        let start = Instant::now();
        client.unary_call(Request::new(request.clone())).await?;
        large.push(start.elapsed().as_nanos());
    }

    let (empty_p50, empty_p99) = percentiles(empty);
    let (large_p50, large_p99) = percentiles(large);
    let ping_pong_rps = bench_ping_pong(client).await?;
    let upload_rps = bench_upload(client).await?;
    println!(
        "bench empty_p50={empty_p50} empty_p99={empty_p99} \
large_p50={large_p50} large_p99={large_p99} \
ping_pong_rps={ping_pong_rps} upload_rps={upload_rps}"
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let is_bench = args.bench;
    match run(args).await {
        Ok(()) => {
            if !is_bench {
                println!("Passed");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

async fn run(args: Args) -> Result<(), Status> {
    if args.test_case == "channel_soak" {
        let soak_config = interop_cases::SoakConfig {
            soak_iterations: args.soak_iterations,
            max_failures: args.max_failures,
            per_rpc_timeout_ms: args.per_rpc_timeout_ms,
            overall_timeout_seconds: args.overall_timeout_seconds,
            min_time_ms_between_rpcs: args.soak_min_time_ms_between_rpcs,
            soak_num_threads: args.soak_num_threads,
            request_size: args.soak_request_size,
            response_size: args.soak_response_size,
            qualification_mode: args.qualification,
        };
        let host = args.server_host.clone();
        let port = args.server_port;
        let use_tls = args.use_tls;
        let host_override = args.server_host_override.clone();
        let tls_ca_file = args.tls_ca_file.clone();
        let factory = move || {
            let host = host.clone();
            let host_override = host_override.clone();
            let ca_file = tls_ca_file.clone();
            async move {
                if use_tls {
                    let server_name = host_override.as_deref().unwrap_or(host.as_str());
                    let client_tls = match ca_file.as_deref() {
                        Some(ca_path) => {
                            let ca_pem = std::fs::read(ca_path)
                                .map_err(|e| Status::unavailable(format!("read ca: {e}")))?;
                            ClientTls::ca(server_name, &ca_pem)
                                .map_err(|e| Status::unavailable(format!("tls ca: {e}")))?
                        }
                        None => ClientTls::webpki(server_name)
                            .map_err(|e| Status::unavailable(format!("tls webpki: {e}")))?,
                    };
                    let addr: SocketAddr = (host.as_str(), port)
                        .to_socket_addrs()
                        .map_err(|e| Status::unavailable(e.to_string()))?
                        .next()
                        .ok_or_else(|| Status::unavailable("resolve"))?;
                    let mut ch = Channel::connect_tls(addr, client_tls).await?;
                    if let Some(ref ho) = host_override {
                        ch = ch.origin(format!("{ho}:{port}"))?;
                    }
                    Ok(TestServiceClient::new(ch))
                } else {
                    let addr: SocketAddr = (host.as_str(), port)
                        .to_socket_addrs()
                        .map_err(|e| Status::unavailable(e.to_string()))?
                        .next()
                        .ok_or_else(|| Status::unavailable("resolve"))?;
                    interop_cases::connect(addr).await
                }
            }
        };
        let summary = interop_cases::channel_soak_with(factory, &soak_config).await?;
        println!("{}", summary.summary_string());
        return Ok(());
    }

    let client = if args.use_tls {
        let server_name = args
            .server_host_override
            .as_deref()
            .unwrap_or(args.server_host.as_str());
        let client_tls = match args.tls_ca_file.as_deref() {
            Some(ca_path) => {
                let ca_pem = match std::fs::read(ca_path) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        eprintln!("failed to read TLS CA file {ca_path:?}: {e}");
                        std::process::exit(1);
                    }
                };
                match ClientTls::ca(server_name, &ca_pem) {
                    Ok(tls) => tls,
                    Err(e) => {
                        eprintln!("failed to configure TLS CA: {e}");
                        std::process::exit(1);
                    }
                }
            }
            None => match ClientTls::webpki(server_name) {
                Ok(tls) => tls,
                Err(e) => {
                    eprintln!("failed to configure WebPKI TLS: {e}");
                    std::process::exit(1);
                }
            },
        };
        let addr: SocketAddr = (args.server_host.as_str(), args.server_port)
            .to_socket_addrs()
            .map_err(|e| Status::unavailable(e.to_string()))?
            .next()
            .ok_or_else(|| Status::unavailable("resolve"))?;
        let mut ch = if args.test_case == "rpc_soak" {
            Channel::connect_tls_lazy(addr, client_tls)?
        } else {
            Channel::connect_tls(addr, client_tls).await?
        };
        if let Some(ref host_override) = args.server_host_override {
            ch = ch.origin(format!("{host_override}:{}", args.server_port))?;
        }
        TestServiceClient::new(ch)
    } else {
        if args.test_case == "rpc_soak" {
            let target = format!("{}:{}", args.server_host, args.server_port);
            let ch = Channel::connect_lazy(target)?;
            TestServiceClient::new(ch)
        } else {
            let addr: SocketAddr = (args.server_host.as_str(), args.server_port)
                .to_socket_addrs()
                .map_err(|e| Status::unavailable(e.to_string()))?
                .next()
                .ok_or_else(|| Status::unavailable("resolve"))?;
            interop_cases::connect(addr).await?
        }
    };

    if args.bench {
        return bench(&client).await;
    }
    if args.test_case == "rpc_soak" {
        let soak_config = interop_cases::SoakConfig {
            soak_iterations: args.soak_iterations,
            max_failures: args.max_failures,
            per_rpc_timeout_ms: args.per_rpc_timeout_ms,
            overall_timeout_seconds: args.overall_timeout_seconds,
            min_time_ms_between_rpcs: args.soak_min_time_ms_between_rpcs,
            soak_num_threads: args.soak_num_threads,
            request_size: args.soak_request_size,
            response_size: args.soak_response_size,
            qualification_mode: args.qualification,
        };
        let summary = interop_cases::rpc_soak(&client, &soak_config).await?;
        println!("{}", summary.summary_string());
        return Ok(());
    }
    interop_cases::run_case(&client, &args.test_case).await
}
