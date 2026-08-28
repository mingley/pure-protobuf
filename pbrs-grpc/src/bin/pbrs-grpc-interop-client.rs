//! Official-shape interop client: `-test_case` `-server_host` `-server_port`
//! `-use_tls=false`.
//!
//! `--bench` replaces the test case with a latency measurement against whatever
//! server is listening. Pointing it at this kernel's server and then at another
//! implementation's is a single-variable comparison: same client, same
//! `.proto`, same codec, different server.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop binary"
)]

use pbrs_grpc::interop_cases;
use pbrs_grpc::Status;
use std::net::{SocketAddr, ToSocketAddrs};

struct Args {
    host: String,
    port: u16,
    test_case: String,
    bench: bool,
}

fn parse_args() -> Args {
    let mut host = "127.0.0.1".to_string();
    let mut port = 10000u16;
    let mut test_case = "empty_unary".to_string();
    let mut bench = false;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        let a = args.get(i).map(String::as_str).unwrap_or("");
        let next = args.get(i + 1);
        let (key, val) = if let Some((k, v)) = a.split_once('=') {
            (k, Some(v.to_string()))
        } else if next.is_some_and(|n| !n.starts_with('-')) {
            i += 1;
            (a, next.cloned())
        } else {
            (a, None)
        };
        match key {
            "--server_host" | "-server_host" => {
                if let Some(v) = val {
                    host = v;
                }
            }
            "--server_port" | "-server_port" => {
                if let Some(v) = val {
                    if let Ok(p) = v.parse() {
                        port = p;
                    }
                }
            }
            "--test_case" | "-test_case" => {
                if let Some(v) = val {
                    test_case = v;
                }
            }
            "--bench" | "-bench" => bench = true,
            _ => {}
        }
        i += 1;
    }
    Args {
        host,
        port,
        test_case,
        bench,
    }
}

/// Warmup iterations, then measured iterations, per case.
const BENCH_WARMUP: u32 = 64;
const BENCH_EMPTY_ITERS: u32 = 2000;
const BENCH_LARGE_ITERS: u32 = 200;
const LARGE_REQ: i32 = 271_828;
const LARGE_RESP: i32 = 314_159;

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

/// Measure `empty_unary` and `large_unary` round-trip latency.
async fn bench(client: &pbrs_grpc::TestServiceClient) -> Result<(), Status> {
    use std::time::Instant;

    for _ in 0..BENCH_WARMUP {
        client
            .empty_call(pbrs_grpc::Request::new(pbrs_grpc::Empty::new()))
            .await?;
    }
    let mut empty = Vec::with_capacity(BENCH_EMPTY_ITERS as usize);
    for _ in 0..BENCH_EMPTY_ITERS {
        let start = Instant::now();
        client
            .empty_call(pbrs_grpc::Request::new(pbrs_grpc::Empty::new()))
            .await?;
        empty.push(start.elapsed().as_nanos());
    }

    let request = large_request();
    for _ in 0..BENCH_WARMUP / 4 {
        client
            .unary_call(pbrs_grpc::Request::new(request.clone()))
            .await?;
    }
    let mut large = Vec::with_capacity(BENCH_LARGE_ITERS as usize);
    for _ in 0..BENCH_LARGE_ITERS {
        let start = Instant::now();
        client
            .unary_call(pbrs_grpc::Request::new(request.clone()))
            .await?;
        large.push(start.elapsed().as_nanos());
    }

    let (empty_p50, empty_p99) = percentiles(empty);
    let (large_p50, large_p99) = percentiles(large);
    println!(
        "bench empty_p50={empty_p50} empty_p99={empty_p99} \
large_p50={large_p50} large_p99={large_p99}"
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {
            if !parse_args().bench {
                println!("Passed");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<(), Status> {
    let args = parse_args();
    let addr: SocketAddr = (args.host.as_str(), args.port)
        .to_socket_addrs()
        .map_err(|e| Status::unavailable(e.to_string()))?
        .next()
        .ok_or_else(|| Status::unavailable("resolve"))?;
    let client = interop_cases::connect(addr).await?;
    if args.bench {
        return bench(&client).await;
    }
    interop_cases::run_case(&client, &args.test_case).await
}
