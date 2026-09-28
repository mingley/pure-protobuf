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
#[cfg(unix)]
const HTTP2_CLIENT_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-http2-client");

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

#[cfg(unix)]
fn proof_log_dir(case: &str) -> PathBuf {
    let log_dir = std::env::temp_dir().join(format!(
        "pbrs-http2-proof-{}-{}-{}",
        case,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos()
    ));
    std::fs::create_dir(&log_dir).expect("proof log directory");
    log_dir
}

#[cfg(unix)]
fn assert_http2_script_rejects_missing_proof(script: &str, case: &str, env: &[(&str, &str)]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = root.parent().expect("workspace root");
    let log_dir = proof_log_dir(case);
    let output = Command::new("bash")
        .arg(root.join("scripts").join(script))
        .arg("--skip-build")
        .arg(format!("--cases={case}"))
        .arg(format!("--log-dir={}", log_dir.display()))
        .arg("--results-json=/dev/null/blocked.json")
        .envs(env.iter().copied())
        .output()
        .expect("run HTTP/2 proof adapter");
    let report_exists = log_dir.join("report.json").exists();
    std::fs::remove_dir_all(&log_dir).expect("remove only this proof's logs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("1 passed, 0 failed"),
        "{script} probe did not pass independently:\n{stdout}\n{stderr}"
    );
    assert!(
        !output.status.success() && !report_exists,
        "{script} falsely qualified missing proof:\n{stdout}\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("could not record {case}")),
        "{script} did not surface the write error:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn http2_negative_runner_does_not_qualify_without_persisted_results() {
    assert_http2_script_rejects_missing_proof(
        "grpc-http2-interop.sh",
        "ping",
        &[("GRPC_HTTP2_CLIENT", HTTP2_CLIENT_BIN)],
    );
}

#[cfg(unix)]
#[test]
fn http2_server_probe_runner_does_not_qualify_without_persisted_results() {
    assert_http2_script_rejects_missing_proof(
        "grpc-http2-server-interop.sh",
        "server_tls_probe",
        &[
            ("GRPC_INTEROP_KERNEL_CLIENT", CLIENT_BIN),
            ("GRPC_INTEROP_KERNEL_SERVER", SERVER_BIN),
        ],
    );
}

#[cfg(unix)]
#[test]
fn http2_negative_runner_does_not_reuse_stale_result_rows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = root.parent().expect("workspace root");
    let log_dir = proof_log_dir("stale");
    let results = log_dir.join("results.json");
    let old = b"{\"results\":[]}";
    std::fs::write(&results, old).expect("prior proof");
    let output = Command::new("bash")
        .arg(root.join("scripts/grpc-http2-interop.sh"))
        .arg("--skip-build")
        .arg(format!("--log-dir={}", log_dir.display()))
        .output()
        .expect("run with stale results");
    let after = std::fs::read(&results).expect("prior proof survived");
    std::fs::remove_dir_all(&log_dir).expect("remove only this proof's logs");
    assert!(!output.status.success(), "stale results qualified the run");
    assert_eq!(after, old, "runner modified evidence from an earlier run");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("fresh results and report paths"),
        "missing stale-evidence diagnosis: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

fn spawn_openssl_server_without_alpn(cert: &PathBuf, key: &PathBuf, port: u16) -> ServerGuard {
    let port_str = port.to_string();
    let child = Command::new("openssl")
        .args([
            "s_server",
            "-accept",
            &format!("127.0.0.1:{port_str}"),
            "-cert",
            &cert.to_string_lossy(),
            "-key",
            &key.to_string_lossy(),
            "-quiet",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("openssl s_server must be installed for the missing-ALPN test");
    let mut guard = ServerGuard { child };
    let addr = format!("127.0.0.1:{port_str}");
    let start = std::time::Instant::now();
    let mut ready = false;
    while start.elapsed() < Duration::from_secs(10) {
        if let Some(code) = guard.child.try_wait().expect("poll openssl") {
            panic!("openssl s_server exited during startup with {code}");
        }
        if std::net::TcpStream::connect(&addr).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        ready,
        "openssl s_server on port {port} failed to become ready in time"
    );
    // The readiness probe above aborts a handshake; give s_server a beat to
    // return to accept before the real client connects.
    std::thread::sleep(Duration::from_millis(100));
    guard
}

// =========================================================================
// mTLS flag wiring (IO-02)
// =========================================================================

#[test]
fn client_mtls_end_to_end() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");
    let client_cert = manifest_dir.join("tests/tls_data/client.crt");
    let client_key = manifest_dir.join("tests/tls_data/client.key");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
            &format!("--tls_client_ca_file={}", ca_file.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        &format!("--tls_client_cert_file={}", client_cert.display()),
        &format!("--tls_client_key_file={}", client_key.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed"));
}

#[test]
fn client_mtls_missing_client_identity_fails() {
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
            &format!("--tls_client_ca_file={}", ca_file.display()),
        ],
        port,
    );

    // Plain one-way TLS client against an mTLS-requiring server: the missing
    // client identity must fail, never silently run unverified.
    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("tls handshake")
            || err.contains("UNAUTHENTICATED")
            || err.contains("certificate"),
        "unexpected error message: {err}"
    );
}

#[test]
fn client_mtls_wrong_client_ca_fails() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");
    let other_ca = manifest_dir.join("tests/tls_data/other.crt");
    let client_cert = manifest_dir.join("tests/tls_data/client.crt");
    let client_key = manifest_dir.join("tests/tls_data/client.key");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
            &format!("--tls_client_ca_file={}", other_ca.display()),
        ],
        port,
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        &format!("--tls_client_cert_file={}", client_cert.display()),
        &format!("--tls_client_key_file={}", client_key.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("tls handshake")
            || err.contains("UNAUTHENTICATED")
            || err.contains("certificate"),
        "unexpected error message: {err}"
    );
}

#[test]
fn client_tls_missing_alpn_fails() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");

    // openssl s_server without -alpn completes the handshake with no
    // negotiated ALPN. The kernel client must refuse to treat that socket as
    // HTTP/2 instead of silently assuming h2.
    let port = pick_unused_port();
    let port_str = port.to_string();
    let _guard = spawn_openssl_server_without_alpn(&cert_file, &key_file, port);

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        "--test_case=empty_unary",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("ALPN h2"),
        "expected ALPN failure diagnostic, got: {err}"
    );
}

#[test]
fn client_rejects_client_cert_without_key() {
    let out = run_client(&["--use_tls=true", "--tls_client_cert_file=/tmp/client.crt"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must be used together"));

    let out = run_client(&["--use_tls=true", "--tls_client_key_file", "/tmp/client.key"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must be used together"));
}

#[test]
fn client_rejects_client_identity_without_tls() {
    let out = run_client(&[
        "--tls_client_cert_file=/tmp/client.crt",
        "--tls_client_key_file=/tmp/client.key",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("require --use_tls=true"));
}

#[test]
fn client_rejects_client_identity_with_missing_files() {
    let out = run_client(&[
        "--use_tls=true",
        "--tls_client_cert_file=/nonexistent/client.crt",
        "--tls_client_key_file=/nonexistent/client.key",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to read TLS client certificate file"));
}

#[test]
fn server_rejects_client_ca_without_tls() {
    let out = run_server(&["--tls_client_ca_file=/tmp/ca.crt"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("requires --use_tls=true"));
}

#[test]
fn server_rejects_mtls_with_missing_client_ca_file() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let out = run_server(&[
        "--use_tls=true",
        &format!("--tls_cert_file={}", cert_file.display()),
        &format!("--tls_key_file={}", key_file.display()),
        "--tls_client_ca_file=/nonexistent/client-ca.crt",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to read TLS client CA file"));
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
