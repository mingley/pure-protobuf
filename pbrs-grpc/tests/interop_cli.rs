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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const CLIENT_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-interop-client");
const SERVER_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-interop-server");
#[cfg(unix)]
const HTTP2_CLIENT_BIN: &str = env!("CARGO_BIN_EXE_pbrs-grpc-http2-client");

// The child owns the append handle and the parent opens a separate reader.
// A regular file avoids pipe backpressure while the server is starting or
// serving, and does not require joining a reader thread when a child exits.
struct ServerLog {
    path: PathBuf,
    reader: Option<std::fs::File>,
    line: Vec<u8>,
    oversized_line: bool,
}

impl ServerLog {
    fn new() -> (Self, std::fs::File) {
        Self::new_with_reader(|path| std::fs::File::open(path))
    }

    fn new_with_reader<F>(open_reader: F) -> (Self, std::fs::File)
    where
        F: FnOnce(&Path) -> std::io::Result<std::fs::File>,
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pbrs-grpc-interop-stderr-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let writer = std::fs::OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&path)
            .expect("create interop server stderr log");
        let reader = match open_reader(&path) {
            Ok(reader) => reader,
            Err(error) => {
                drop(writer);
                let _ = std::fs::remove_file(&path);
                panic!("open interop server stderr log: {error}");
            }
        };
        (
            Self {
                path,
                reader: Some(reader),
                line: Vec::new(),
                oversized_line: false,
            },
            writer,
        )
    }

    fn poll_ready(&mut self, port: u16) -> std::io::Result<bool> {
        use std::io::Read;

        let plain = format!("pbrs-grpc interop server listening on 0.0.0.0:{port}");
        let tls = format!("pbrs-grpc interop server listening with TLS on 0.0.0.0:{port}");
        let mut ready = false;
        let mut chunk = [0_u8; 4096];
        // Bound each poll even if the child is continuously writing.
        for _ in 0..16 {
            let size = self
                .reader
                .as_mut()
                .expect("open log reader")
                .read(&mut chunk)?;
            if size == 0 {
                break;
            }
            for &byte in &chunk[..size] {
                if byte == b'\n' {
                    if !self.oversized_line {
                        let line = self.line.strip_suffix(b"\r").unwrap_or(&self.line);
                        ready |= line == plain.as_bytes() || line == tls.as_bytes();
                    }
                    self.line.clear();
                    self.oversized_line = false;
                } else if !self.oversized_line {
                    if self.line.len() < 1024 {
                        self.line.push(byte);
                    } else {
                        self.line.clear();
                        self.oversized_line = true;
                    }
                }
            }
        }
        Ok(ready)
    }

    fn contents(&self) -> String {
        match std::fs::read(&self.path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => format!("could not read server stderr: {error}"),
        }
    }
}

impl Drop for ServerLog {
    fn drop(&mut self) {
        // Close the independent reader before unlinking, including on Windows.
        drop(self.reader.take());
        if let Ok(mut source) = std::fs::File::open(&self.path) {
            let _ = std::io::copy(&mut source, &mut std::io::stderr().lock());
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

struct ServerGuard {
    child: std::process::Child,
    stderr_log: Option<ServerLog>,
    port: u16,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.stderr_log.take());
    }
}

#[derive(Debug)]
enum ServerStartError {
    Spawn(std::io::Error),
    Poll(std::io::Error),
    Read(std::io::Error),
    Exited {
        port: u16,
        status: std::process::ExitStatus,
        stderr: String,
    },
    Timeout {
        port: u16,
        stderr: String,
    },
}

impl ServerStartError {
    fn is_bind_collision(&self) -> bool {
        let Self::Exited { port, stderr, .. } = self else {
            return false;
        };
        if stderr
            .lines()
            .any(|line| line.starts_with("pbrs-grpc interop server listening"))
        {
            return false;
        }
        let prefix = format!("pbrs-grpc interop server bind failed on 0.0.0.0:{port}: ");
        stderr.lines().any(|line| {
            let Some(detail) = line.strip_prefix(prefix.as_str()) else {
                return false;
            };
            let Some((_, raw)) = detail.rsplit_once(" (os error ") else {
                return false;
            };
            let Some(code) = raw
                .strip_suffix(')')
                .and_then(|value| value.parse::<i32>().ok())
            else {
                return false;
            };
            std::io::Error::from_raw_os_error(code).kind() == std::io::ErrorKind::AddrInUse
        })
    }
}

impl std::fmt::Display for ServerStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "failed to spawn interop server: {error}"),
            Self::Poll(error) => write!(f, "poll interop server: {error}"),
            Self::Read(error) => write!(f, "read interop server stderr: {error}"),
            Self::Exited {
                port,
                status,
                stderr,
            } => write!(
                f,
                "server on port {port} exited before readiness: {status}; stderr:\n{stderr}"
            ),
            Self::Timeout { port, stderr } => write!(
                f,
                "server on port {port} failed to become ready in time; stderr:\n{stderr}"
            ),
        }
    }
}

fn pick_unused_port() -> u16 {
    // This releases the port before the child binds; a verified bind collision
    // is retried on a fresh port and the actual port is returned to the caller.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.local_addr().expect("local addr").port()
}

fn args_with_port(args: &[&str], port: u16) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut awaiting_port = false;
    let mut replaced = 0;
    for &arg in args {
        if awaiting_port {
            out.push(port.to_string());
            awaiting_port = false;
            replaced += 1;
        } else if arg == "--port" || arg == "-port" {
            out.push(arg.to_owned());
            awaiting_port = true;
        } else if let Some((flag, _)) = arg.split_once('=') {
            if flag == "--port" || flag == "-port" {
                out.push(format!("{flag}={port}"));
                replaced += 1;
            } else {
                out.push(arg.to_owned());
            }
        } else {
            out.push(arg.to_owned());
        }
    }
    assert!(
        !awaiting_port && replaced == 1,
        "expected exactly one port flag"
    );
    out
}

fn spawn_server(args: &[&str], port: u16) -> ServerGuard {
    spawn_server_with_retry(port, |actual_port| {
        let mut command = Command::new(SERVER_BIN);
        command.args(args_with_port(args, actual_port));
        command
    })
    .unwrap_or_else(|error| panic!("{error}"))
}

fn spawn_server_with_retry<F>(
    mut port: u16,
    mut command_for_port: F,
) -> Result<ServerGuard, ServerStartError>
where
    F: FnMut(u16) -> Command,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        let (log, writer) = ServerLog::new();
        match try_spawn_server_command(
            command_for_port(port),
            port,
            Duration::from_secs(5),
            log,
            writer,
        ) {
            Ok(server) => return Ok(server),
            Err(error) if error.is_bind_collision() && attempts < 8 => {
                port = pick_unused_port();
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn spawn_server_command(
    command: Command,
    port: u16,
    timeout: Duration,
    log: ServerLog,
    writer: std::fs::File,
) -> ServerGuard {
    try_spawn_server_command(command, port, timeout, log, writer)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn try_spawn_server_command(
    mut command: Command,
    port: u16,
    timeout: Duration,
    log: ServerLog,
    writer: std::fs::File,
) -> Result<ServerGuard, ServerStartError> {
    let child = command.stderr(std::process::Stdio::from(writer)).spawn();
    // Drop the parent's writer before waiting or handling a spawn failure.
    drop(command);
    let child = child.map_err(ServerStartError::Spawn)?;
    let mut server = ServerGuard {
        child,
        stderr_log: Some(log),
        port,
    };
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = server.child.try_wait().map_err(ServerStartError::Poll)? {
            let stderr = server.stderr_log.as_ref().expect("server log").contents();
            return Err(ServerStartError::Exited {
                port,
                status,
                stderr,
            });
        }
        if start.elapsed() >= timeout {
            let stderr = server.stderr_log.as_ref().expect("server log").contents();
            return Err(ServerStartError::Timeout { port, stderr });
        }
        let ready = server
            .stderr_log
            .as_mut()
            .expect("server log")
            .poll_ready(port)
            .map_err(ServerStartError::Read)?;
        if ready {
            if let Some(status) = server.child.try_wait().map_err(ServerStartError::Poll)? {
                let stderr = server.stderr_log.as_ref().expect("server log").contents();
                return Err(ServerStartError::Exited {
                    port,
                    status,
                    stderr,
                });
            }
            return Ok(server);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn readiness_panic_message(result: std::thread::Result<ServerGuard>) -> String {
    let error = match result {
        Ok(_) => panic!("expected server startup to fail"),
        Err(error) => error,
    };
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            error
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
        })
        .expect("readiness panic message")
}

#[cfg(unix)]
#[test]
fn interop_server_readiness_rejects_a_foreign_listener() {
    let decoy = std::net::TcpListener::bind("127.0.0.1:0").expect("bind decoy");
    let port = decoy.local_addr().expect("decoy address").port();
    let mut command = Command::new("sh");
    command.args(["-c", "printf 'child has not bound\\n' >&2; exec sleep 3"]);
    let (log, writer) = ServerLog::new();
    let path = log.path.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        spawn_server_command(command, port, Duration::from_millis(200), log, writer)
    }));
    let message = readiness_panic_message(result);
    assert!(
        message.contains("failed to become ready in time"),
        "unexpected readiness failure: {message}"
    );
    assert!(!path.exists(), "startup failure must remove the stderr log");
    println!("INTEROP_OWN_READINESS_REJECTED_DECOY");
}

#[cfg(unix)]
#[test]
fn interop_server_readiness_timeout_and_spawn_failure_clean_logs() {
    let (log, writer) = ServerLog::new();
    let path = log.path.clone();
    let mut command = Command::new("sh");
    command.args(["-c", "printf 'waiting for startup\\n' >&2; exec sleep 3"]);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        spawn_server_command(command, 1, Duration::from_secs(1), log, writer)
    }));
    let message = readiness_panic_message(result);
    assert!(
        message.contains("failed to become ready in time")
            && message.contains("waiting for startup"),
        "unexpected timeout failure: {message}"
    );
    assert!(!path.exists(), "timeout must remove the stderr log");

    let (log, writer) = ServerLog::new();
    let path = log.path.clone();
    let missing = path.with_extension("missing-executable");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        spawn_server_command(
            Command::new(missing),
            1,
            Duration::from_secs(1),
            log,
            writer,
        )
    }));
    let message = readiness_panic_message(result);
    assert!(
        message.contains("failed to spawn interop server"),
        "unexpected spawn failure: {message}"
    );
    assert!(!path.exists(), "spawn failure must remove the stderr log");
}

#[test]
fn interop_server_retries_verified_bind_collisions() {
    for style in 0..3 {
        // Bind exactly the wildcard address used by the child on every host.
        let decoy = std::net::TcpListener::bind("0.0.0.0:0").expect("bind collision");
        let port = decoy.local_addr().expect("collision address").port();
        let value = port.to_string();
        let args = match style {
            0 => vec![format!("--port={value}")],
            1 => vec!["--port".to_owned(), value],
            _ => vec!["-port".to_owned(), value],
        };
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let server = spawn_server(&borrowed, port);
        assert_ne!(server.port, port, "the occupied first port must be retried");
        let port_str = server.port.to_string();
        let out = run_client(&[
            "--server_host=127.0.0.1",
            &format!("--server_port={port_str}"),
            "--test_case=empty_unary",
            "--use_tls=false",
        ]);
        assert!(
            out.status.success(),
            "RPC through retried listener failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        drop(server);
        drop(decoy);
    }
}

#[test]
fn interop_server_does_not_retry_tls_setup_errors() {
    let mut attempts = 0;
    let result = spawn_server_with_retry(pick_unused_port(), |port| {
        attempts += 1;
        let mut command = Command::new(SERVER_BIN);
        command.args([format!("--port={port}"), "--use_tls=true".to_owned()]);
        command
    });
    let error = match result {
        Ok(_) => panic!("server without TLS identity must fail"),
        Err(error) => error,
    };
    assert_eq!(attempts, 1, "TLS setup errors must not be retried");
    assert!(!error.is_bind_collision());
    assert!(
        error
            .to_string()
            .contains("missing required flag --tls_cert_file")
    );
}

#[cfg(unix)]
#[test]
fn interop_server_does_not_retry_generic_address_error() {
    let decoy = std::net::TcpListener::bind("0.0.0.0:0").expect("bind collision");
    let port = decoy.local_addr().expect("collision address").port();
    let bind_error =
        std::net::TcpListener::bind(("0.0.0.0", port)).expect_err("address is occupied");
    assert_eq!(bind_error.kind(), std::io::ErrorKind::AddrInUse);
    let diagnostic = format!("UNAVAILABLE: {bind_error}");
    let mut attempts = 0;
    let result = spawn_server_with_retry(port, |_| {
        attempts += 1;
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf '%s\\n' \"$1\" >&2; exit 1",
            "fake-error",
            &diagnostic,
        ]);
        command
    });
    let error = match result {
        Ok(_) => panic!("a generic error must fail startup"),
        Err(error) => error,
    };
    assert_eq!(attempts, 1, "a generic error must not be retried");
    assert!(!error.is_bind_collision());
    assert!(error.to_string().contains("UNAVAILABLE:"));
}

#[test]
fn interop_server_readiness_reader_failure_cleans_log() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ServerLog::new_with_reader(|path| {
            sender.send(path.to_owned()).expect("record stderr path");
            Err(std::io::Error::other("simulated reader failure"))
        })
    }));
    assert!(result.is_err(), "reader setup should fail");
    let path = receiver.recv().expect("stderr path");
    assert!(!path.exists(), "reader failure must remove the stderr log");
}

#[test]
fn interop_server_readiness_requires_a_complete_bounded_line() {
    use std::io::Write;

    let (mut log, mut writer) = ServerLog::new();
    let path = log.path.clone();
    writer.write_all(&vec![b'x'; 2048]).expect("long line");
    writer.write_all(b"\n").expect("finish long line");
    writer
        .write_all(b"pbrs-grpc interop server listening on 0.0.0.0:12345")
        .expect("partial marker");
    writer.flush().expect("flush partial marker");
    assert!(!log.poll_ready(12345).expect("read partial marker"));
    writer.write_all(b"\n").expect("finish marker");
    writer.flush().expect("flush marker");
    assert!(log.poll_ready(12345).expect("read complete marker"));
    drop(writer);
    drop(log);
    assert!(!path.exists(), "completed log must be removed");
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
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");
    let other_ca = manifest_dir.join("tests/tls_data/other.crt");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

    let trusted = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", ca_file.display()),
        "--test_case=empty_unary",
    ]);
    assert!(
        trusted.status.success(),
        "trusted-CA control failed: status={} stderr={}",
        trusted.status,
        String::from_utf8_lossy(&trusted.stderr)
    );
    assert!(
        String::from_utf8_lossy(&trusted.stdout).contains("Passed"),
        "trusted-CA control did not complete its RPC: stdout={} stderr={}",
        String::from_utf8_lossy(&trusted.stdout),
        String::from_utf8_lossy(&trusted.stderr)
    );

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--server_host_override=localhost",
        "--use_tls=true",
        &format!("--tls_ca_file={}", other_ca.display()),
        "--test_case=empty_unary",
    ]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "wrong-CA client status={} stderr={err}",
        out.status
    );
    assert!(
        err.contains("invalid peer certificate") && err.contains("UnknownIssuer"),
        "wrong CA must fail certificate verification: status={} stderr={err}",
        out.status
    );
}

#[test]
fn client_tls_hostname_mismatch_fails_verification() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cert_file = manifest_dir.join("tests/tls_data/server.crt");
    let key_file = manifest_dir.join("tests/tls_data/server.key");
    let ca_file = manifest_dir.join("tests/tls_data/ca.crt");

    let port = pick_unused_port();
    let port_str = port.to_string();
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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
    let server = spawn_server(&[&format!("--port={port_str}")], port);
    let port_str = server.port.to_string();

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
    let server = spawn_server(&["--port", &port_str], port);
    let port_str = server.port.to_string();

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
    let server = spawn_server(&["-port", &port_str], port);
    let port_str = server.port.to_string();

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
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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

fn spawn_openssl_server_without_alpn(cert: &Path, key: &Path, port: u16) -> ServerGuard {
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
    let mut guard = ServerGuard {
        child,
        stderr_log: None,
        port,
    };
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
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
            &format!("--tls_client_ca_file={}", ca_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
            &format!("--tls_client_ca_file={}", ca_file.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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
    let server = spawn_server(
        &[
            &format!("--port={port_str}"),
            "--use_tls=true",
            &format!("--tls_cert_file={}", cert_file.display()),
            &format!("--tls_key_file={}", key_file.display()),
            &format!("--tls_client_ca_file={}", other_ca.display()),
        ],
        port,
    );
    let port_str = server.port.to_string();

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
    let server = spawn_server(&[&format!("--port={port_str}")], port);
    let port_str = server.port.to_string();

    let out = run_client(&[
        "--server_host=127.0.0.1",
        &format!("--server_port={port_str}"),
        "--bench",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bench empty_p50="));
}
