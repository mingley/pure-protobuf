//! CLI flag parsing and rejection integration tests for interop client and server.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    missing_docs,
    reason = "integration tests"
)]

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const CLIENT_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-interop-client");
const SERVER_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-interop-server");

struct ServerGuard {
    child: std::process::Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn pick_unused_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.local_addr().expect("local addr").port()
}

fn spawn_server(args: &[&str], port: u16) -> ServerGuard {
    let child = Command::new(SERVER_BIN)
        .args(args)
        .spawn()
        .expect("failed to spawn interop server");
    let addr = format!("127.0.0.1:{port}");
    let start = std::time::Instant::now();
    let mut ready = false;
    while start.elapsed() < Duration::from_secs(5) {
        if std::net::TcpStream::connect(&addr).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        ready,
        "server on port {port} failed to become ready in time"
    );
    ServerGuard { child }
}

fn run_client(args: &[&str]) -> std::process::Output {
    Command::new(CLIENT_BIN)
        .args(args)
        .output()
        .expect("failed to run client")
}

fn run_server(args: &[&str]) -> std::process::Output {
    Command::new(SERVER_BIN)
        .args(args)
        .output()
        .expect("failed to run server")
}

// =========================================================================
// Client unknown flag rejection
// =========================================================================

#[test]
fn client_rejects_unknown_double_dash_flag() {
    let out = run_client(&["--unknown_flag"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn client_rejects_unknown_single_dash_flag() {
    let out = run_client(&["-unknown_flag"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn client_rejects_unknown_flag_with_value() {
    let out = run_client(&["--foo=bar"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn client_rejects_server_only_flag() {
    let out = run_client(&["--port=10000"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn client_rejects_unexpected_positional_arg() {
    let out = run_client(&["positional_arg"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected positional argument"));
}

// =========================================================================
// Client invalid port rejection
// =========================================================================

#[test]
fn client_rejects_port_zero() {
    let out = run_client(&["--server_port=0"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_client(&["--server_port", "0"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn client_rejects_port_overflow() {
    let out = run_client(&["--server_port=65536"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_client(&["--server_port", "65536"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn client_rejects_port_negative() {
    let out = run_client(&["--server_port=-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_client(&["--server_port", "-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn client_rejects_port_non_numeric() {
    let out = run_client(&["--server_port=abc"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_client(&["--server_port", "abc"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn client_rejects_missing_port_value() {
    let out = run_client(&["--server_port"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("missing value for flag"));
}

// =========================================================================
// Client TLS certificate error handling
// =========================================================================

#[test]
fn client_tls_without_ca_defaults_to_webpki_and_fails_on_self_signed() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("tls handshake")
            || err.contains("UNAUTHENTICATED")
            || err.contains("tls:")
            || err.contains("certificate"),
        "unexpected error message: {err}"
    );
}

#[test]
fn client_tls_wrong_ca_fails_verification() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let other_ca = manifest_dir.join("tests/tls_data/other.crt");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", other_ca.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("tls handshake") || err.contains("UNAUTHENTICATED"));
}

#[test]
fn client_tls_hostname_mismatch_fails_verification() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=mismatch.example.com",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("tls handshake") || err.contains("UNAUTHENTICATED"));
}

#[test]
fn client_rejects_use_tls_with_missing_ca_file() {
    let out = run_client(&[
        "--use_tls=true",
        "--tls_ca_file=/nonexistent/path/does_not_exist.crt",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to read TLS CA file"));
}

// =========================================================================
// Server unknown flag rejection
// =========================================================================

#[test]
fn server_rejects_unknown_double_dash_flag() {
    let out = run_server(&["--unknown_flag"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn server_rejects_unknown_single_dash_flag() {
    let out = run_server(&["-unknown_flag"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn server_rejects_client_only_flag() {
    let out = run_server(&["--server_port=10000"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown flag"));
}

#[test]
fn server_rejects_unexpected_positional_arg() {
    let out = run_server(&["8080"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected positional argument"));
}

// =========================================================================
// Server invalid port rejection
// =========================================================================

#[test]
fn server_rejects_port_zero() {
    let out = run_server(&["--port=0"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_server(&["--port", "0"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn server_rejects_port_overflow() {
    let out = run_server(&["--port=65536"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_server(&["--port", "65536"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn server_rejects_port_negative() {
    let out = run_server(&["--port=-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_server(&["--port", "-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn server_rejects_port_non_numeric() {
    let out = run_server(&["--port=xyz"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));

    let out = run_server(&["--port", "xyz"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid port"));
}

#[test]
fn server_rejects_missing_port_value() {
    let out = run_server(&["--port"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("missing value for flag"));
}

// =========================================================================
// Server TLS certificate error handling
// =========================================================================

#[test]
fn server_rejects_use_tls_without_cert_and_key() {
    let out = run_server(&["--use_tls=true"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--tls_cert_file"));

    let out = run_server(&["--use_tls", "true"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--tls_cert_file"));
}

#[test]
fn server_rejects_use_tls_missing_key_file() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let out = run_server(&[
        "--use_tls=true",
        &format!("--tls_cert_file={}", cert_file.display()),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--tls_key_file"));
}

#[test]
fn server_rejects_use_tls_with_nonexistent_files() {
    let out = run_server(&[
        "--use_tls=true",
        "--tls_cert_file=/nonexistent/cert.crt",
        "--tls_key_file=/nonexistent/key.key",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to read TLS certificate file"));
}

// =========================================================================
// Valid flags & both syntax styles end-to-end
// =========================================================================

#[test]
fn valid_flags_equal_syntax_style() {
    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(&[&format!("--port={port_str}")], port);

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--test_case=empty_unary",
        "--use_tls=false",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed"));
}

#[test]
fn valid_flags_space_syntax_style() {
    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(&["--port", &port_str], port);

    let out = run_client(&[
        "--server_host",
        "127.0.0.1",
        "--server_port",
        &port_str,
        "--test_case",
        "empty_unary",
        "--use_tls",
        "false",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed"));
}

#[test]
fn valid_flags_single_dash_interop_runner_style() {
    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(&["-port", &port_str], port);

    let out = run_client(&[
        "-server_host",
        "127.0.0.1",
        "-server_port",
        &port_str,
        "-test_case=empty_unary",
        "-use_tls=false",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed"));
}

#[test]
fn client_rejects_invalid_boolean() {
    let out = run_client(&["--use_tls=maybe"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid boolean value"));

    let out = run_client(&["--use_tls", "maybe"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid boolean value"));

    let out = run_client(&["--bench=invalid"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid boolean value"));
}

#[test]
fn server_rejects_invalid_boolean() {
    let out = run_server(&["--use_tls=invalid"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid boolean value"));

    let out = run_server(&["--use_tls", "invalid"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid boolean value"));
}

#[test]
fn valid_flags_tls_end_to_end() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed"));
}

#[test]
fn valid_flags_bench() {
    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(&[&format!("--port={port_str}")], port);

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--bench",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bench empty_p50="));
}
