//! Official-shape interop client: `-test_case` `-server_host` `-server_port` `-use_tls=false`.

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
}

fn parse_args() -> Args {
    let mut host = "127.0.0.1".to_string();
    let mut port = 10000u16;
    let mut test_case = "empty_unary".to_string();
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
            _ => {}
        }
        i += 1;
    }
    Args {
        host,
        port,
        test_case,
    }
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {
            println!("Passed");
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
    interop_cases::run_case(&client, &args.test_case).await
}
