//! Official-shape interop server: `--port` `--use_tls=false`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop binary"
)]

use pbrs_grpc::{InteropTestService, Status, TestServiceServer};
use std::net::SocketAddr;
use tokio::net::TcpListener;

fn parse_port() -> u16 {
    let mut port = 10000u16;
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--port=") {
            if let Ok(p) = v.parse() {
                port = p;
            }
        } else if arg == "--port" {
            continue;
        } else if arg.chars().next().is_some_and(|c| c.is_ascii_digit()) && !arg.starts_with('-') {
            if let Ok(p) = arg.parse() {
                port = p;
            }
        }
    }
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args.get(i).map(String::as_str) == Some("--port") {
            if let Some(v) = args.get(i + 1) {
                if let Ok(p) = v.parse() {
                    port = p;
                }
            }
        }
    }
    port
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Status> {
    let port = parse_port();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let bound = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    eprintln!("pbrs-grpc interop server listening on {bound}");
    TestServiceServer::new(InteropTestService)
        .serve_listener(listener)
        .await
}
