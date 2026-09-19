//! Official-shape interop server: `--port` `--use_tls=false`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop binary"
)]

use pbrs_grpc::{Identity, InteropTestService, ServerTls, Status, TestServiceServer};
use std::net::SocketAddr;
use tokio::net::TcpListener;

struct ServerArgs {
    port: u16,
    use_tls: bool,
    tls_cert_file: Option<String>,
    tls_key_file: Option<String>,
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

fn parse_args() -> ServerArgs {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = 10000u16;
    let mut use_tls = false;
    let mut tls_cert_file = None;
    let mut tls_key_file = None;

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
            "port" => {
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
                port = parse_port(raw_key, &val);
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
            "tls_cert_file" => {
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
                tls_cert_file = Some(val);
            }
            "tls_key_file" => {
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
                tls_key_file = Some(val);
            }
            _ => {
                eprintln!("unknown flag: {arg}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    ServerArgs {
        port,
        use_tls,
        tls_cert_file,
        tls_key_file,
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    if let Err(e) = run(args).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn run(args: ServerArgs) -> Result<(), Status> {
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    if args.use_tls {
        let cert_path = match args.tls_cert_file.as_deref() {
            Some(p) => p,
            None => {
                eprintln!("missing required flag --tls_cert_file when --use_tls is enabled");
                std::process::exit(1);
            }
        };
        let key_path = match args.tls_key_file.as_deref() {
            Some(p) => p,
            None => {
                eprintln!("missing required flag --tls_key_file when --use_tls is enabled");
                std::process::exit(1);
            }
        };
        let cert_pem = match std::fs::read(cert_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("failed to read TLS certificate file {cert_path:?}: {e}");
                std::process::exit(1);
            }
        };
        let key_pem = match std::fs::read(key_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("failed to read TLS key file {key_path:?}: {e}");
                std::process::exit(1);
            }
        };
        let identity = match Identity::from_pem(&cert_pem, &key_pem) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("invalid TLS certificate or key: {e}");
                std::process::exit(1);
            }
        };
        let server_tls = match ServerTls::new(identity) {
            Ok(tls) => tls,
            Err(e) => {
                eprintln!("failed to configure TLS: {e}");
                std::process::exit(1);
            }
        };
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        let bound = listener
            .local_addr()
            .map_err(|e| Status::unavailable(e.to_string()))?;
        eprintln!("pbrs-grpc interop server listening with TLS on {bound}");
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), server_tls)
            .await
    } else {
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
}
