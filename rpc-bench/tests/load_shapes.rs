//! SB-11: the `load` subcommand drives every (transport, shape) cell against
//! a loopback server with zero failures, and rejects invalid flag mixes.
//! Fails if any cell errors, reports failures/timeouts, or accepts invalid
//! TLS pairs, TLS on plaintext loopback, or BenchmarkService flag mixes.

use std::process::{Command, Stdio};
use std::time::Duration;

fn run_load(args: &[&str]) -> (i32, String) {
    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    let mut cmd = Command::new(binary);
    cmd.arg("load").arg("--quick").args(args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn rpc-bench load");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait().expect("poll load child") {
            Some(status) => {
                let out = child.wait_with_output().expect("read load output");
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                return (status.code().unwrap_or(-1), text);
            }
            None => {
                if std::time::Instant::now() > deadline {
                    child.kill().expect("kill hung load child");
                    let out = child.wait_with_output().expect("read load output");
                    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                    panic!("load {args:?} hung past 60s: {text}");
                }
                std::thread::park_timeout(Duration::from_millis(50));
            }
        }
    }
}

/// Parse `successes=N failures=M timeouts=K queue_overflows=J` from load output.
fn counters(text: &str) -> (u64, u64, u64, u64) {
    let mut successes = None;
    let mut failures = None;
    let mut timeouts = None;
    let mut overflows = None;
    for kv in text.split_whitespace() {
        let (k, v) = match kv.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };
        let n: u64 = match v.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        match k {
            "successes" => successes = Some(n),
            "failures" => failures = Some(n),
            "timeouts" => timeouts = Some(n),
            "queue_overflows" => overflows = Some(n),
            _ => {}
        }
    }
    (
        successes.expect("load printed successes"),
        failures.expect("load printed failures"),
        timeouts.expect("load printed timeouts"),
        overflows.expect("load printed queue_overflows"),
    )
}

fn assert_clean_cell(args: &[&str]) {
    let (code, text) = run_load(args);
    assert_eq!(code, 0, "load {args:?} exited {code}: {text}");
    let (successes, failures, timeouts, overflows) = counters(&text);
    assert!(successes > 0, "load {args:?} reported no successes: {text}");
    assert_eq!(failures, 0, "load {args:?} failures: {text}");
    assert_eq!(timeouts, 0, "load {args:?} timeouts: {text}");
    assert_eq!(overflows, 0, "load {args:?} overflows: {text}");
}

#[test]
fn load_native_unary_cells() {
    assert_clean_cell(&["--transport=native"]);
    assert_clean_cell(&[
        "--transport=native",
        "--req-bytes=1024",
        "--resp-bytes=1024",
    ]);
    assert_clean_cell(&[
        "--transport=native",
        "--req-bytes=65536",
        "--resp-bytes=65536",
    ]);
    assert_clean_cell(&["--transport=native", "--benchmark-service"]);
}

#[test]
fn load_native_streaming_cells() {
    assert_clean_cell(&[
        "--transport=native",
        "--shape=server_stream",
        "--stream-msgs=8",
        "--resp-bytes=16",
    ]);
    assert_clean_cell(&[
        "--transport=native",
        "--shape=bidi",
        "--stream-msgs=4",
        "--resp-bytes=16",
    ]);
    assert_clean_cell(&[
        "--transport=native",
        "--shape=client_stream",
        "--stream-msgs=4",
        "--req-bytes=1024",
    ]);
}

#[test]
fn load_large_payload_loopback() {
    // 8 MiB unary up+down against the loopback server with raised limits.
    assert_clean_cell(&[
        "--transport=native",
        "--req-bytes=8388608",
        "--resp-bytes=8388608",
        "--max-message-size=16777216",
    ]);
    // 8x1 MiB client-streaming upload in one RPC.
    assert_clean_cell(&[
        "--transport=native",
        "--shape=client_stream",
        "--stream-msgs=8",
        "--req-bytes=1048576",
        "--max-message-size=16777216",
    ]);
    // Tonic client over the same loopback at 1 MiB.
    assert_clean_cell(&[
        "--transport=tonic",
        "--shape=client_stream",
        "--stream-msgs=4",
        "--req-bytes=1048576",
        "--max-message-size=16777216",
    ]);
}

#[test]
fn load_tonic_cells_against_native_loopback() {
    assert_clean_cell(&["--transport=tonic", "--resp-bytes=1024"]);
    assert_clean_cell(&[
        "--transport=tonic",
        "--shape=server_stream",
        "--stream-msgs=8",
        "--resp-bytes=16",
    ]);
    assert_clean_cell(&[
        "--transport=tonic",
        "--shape=bidi",
        "--stream-msgs=4",
        "--resp-bytes=16",
    ]);
}

fn tls_data() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pbrs-grpc/tests/tls_data")
}

fn read_ready_port(child: &mut std::process::Child) -> u16 {
    use std::io::BufRead as _;
    let stdout = child.stdout.take().expect("server stdout pipe");
    let mut reader = std::io::BufReader::new(stdout);
    // The server prints READY immediately or exits; EOF means it died.
    for _ in 0..50 {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.strip_prefix("READY ") {
                    for kv in rest.split_whitespace() {
                        if let Some(value) = kv.strip_prefix("port=")
                            && let Ok(port) = value.parse::<u16>()
                        {
                            return port;
                        }
                    }
                }
            }
            Err(e) => {
                child.kill().ok();
                panic!("reading server stdout: {e}");
            }
        }
        if let Some(status) = child.try_wait().expect("poll server") {
            panic!("server exited {status} before READY");
        }
    }
    child.kill().ok();
    panic!("server never printed READY");
}

#[test]
fn load_tls_unary_against_tls_server() {
    let data = tls_data();
    let cert = data.join("server.crt");
    let key = data.join("server.key");
    let ca = data.join("ca.crt");
    for path in [&cert, &key, &ca] {
        assert!(path.is_file(), "missing test cert {}", path.display());
    }

    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    let mut server = Command::new(binary)
        .arg("server")
        .arg("--transport=native")
        .arg("--port=0")
        .arg("--timeout-secs=30")
        .arg(format!("--tls-cert={}", cert.display()))
        .arg(format!("--tls-key={}", key.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn TLS server");
    let port = read_ready_port(&mut server);

    // Verified TLS load: zero failures.
    let out = Command::new(binary)
        .arg("load")
        .arg("--quick")
        .arg(format!("--server_addr=127.0.0.1:{port}"))
        .arg("--resp-bytes=1024")
        .arg(format!("--tls-ca={}", ca.display()))
        .arg("--tls-server-name=localhost")
        .output()
        .expect("run TLS load");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    server.kill().ok();
    let _ = server.wait();
    assert!(out.status.success(), "TLS load failed: {text}");
    let (successes, failures, timeouts, _) = counters(&text);
    assert!(successes > 0, "TLS load no successes: {text}");
    assert_eq!(failures, 0, "TLS load failures: {text}");
    assert_eq!(timeouts, 0, "TLS load timeouts: {text}");

    // Wrong server name must fail verification, not silently connect.
    let mut server = Command::new(binary)
        .arg("server")
        .arg("--transport=native")
        .arg("--port=0")
        .arg("--timeout-secs=30")
        .arg(format!("--tls-cert={}", cert.display()))
        .arg(format!("--tls-key={}", key.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn TLS server");
    let port = read_ready_port(&mut server);
    let out = Command::new(binary)
        .arg("load")
        .arg("--quick")
        .arg(format!("--server_addr=127.0.0.1:{port}"))
        .arg(format!("--tls-ca={}", ca.display()))
        .arg("--tls-server-name=wrong.example")
        .output()
        .expect("run TLS load with wrong name");
    server.kill().ok();
    let _ = server.wait();
    assert!(
        !out.status.success(),
        "wrong-name TLS load must fail verification"
    );
}

#[test]
fn server_rejects_invalid_tls_mixes() {
    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    // Half a pair.
    let out = Command::new(binary)
        .arg("server")
        .arg("--tls-cert=/tmp/x.crt")
        .arg("--timeout-secs=2")
        .output()
        .expect("run server");
    assert_eq!(out.status.code(), Some(2));
    // tonic + TLS.
    let out = Command::new(binary)
        .arg("server")
        .arg("--transport=tonic")
        .arg("--tls-cert=/tmp/x.crt")
        .arg("--tls-key=/tmp/x.key")
        .arg("--timeout-secs=2")
        .output()
        .expect("run server");
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(text.contains("native-only"), "tonic+tls: {text}");
}

#[test]
fn load_rejects_invalid_flag_mixes() {
    // Unknown shape exits 2 with a parse error.
    let (code, text) = run_load(&["--shape=all"]);
    assert_eq!(code, 2, "shape=all: {text}");
    assert!(text.contains("unknown load shape"), "shape=all: {text}");

    // Half a TLS pair exits 2.
    let (code, text) = run_load(&["--tls-ca=/tmp/ca.pem"]);
    assert_eq!(code, 2, "tls-ca alone: {text}");

    // Tonic TLS is supported, but needs an explicit TLS server endpoint.
    let (code, text) = run_load(&[
        "--transport=tonic",
        "--tls-ca=/tmp/ca.pem",
        "--tls-server-name=localhost",
    ]);
    assert_eq!(code, 1, "tonic TLS loopback: {text}");
    assert!(
        text.contains("TLS needs an explicit --server_addr: the loopback server is plaintext-only"),
        "tonic TLS loopback: {text}"
    );

    // TLS without an explicit server is rejected before bind.
    let (code, text) = run_load(&["--tls-ca=/tmp/ca.pem", "--tls-server-name=localhost"]);
    assert_eq!(code, 1, "tls loopback: {text}");
    assert!(text.contains("plaintext-only"), "tls loopback: {text}");
}

#[test]
fn load_rejects_nonfinite_and_unrepresentable_timing_without_panicking() {
    for flag in ["--rate", "--duration-secs", "--latency-rtt-ms"] {
        for value in ["NaN", "inf", "-inf", "1e300"] {
            let arg = format!("{flag}={value}");
            let (code, text) = run_load(&[&arg]);
            assert_eq!(code, 2, "{arg}: {text}");
            assert!(!text.contains("panicked"), "{arg}: {text}");
        }
    }
    for arg in ["--rate=1e-300", "--duration-secs=1e-300"] {
        let (code, text) = run_load(&[arg]);
        assert_eq!(code, 2, "{arg}: {text}");
    }
}

struct StopServer(std::process::Child);

impl Drop for StopServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous CLI test reads its saved report outside an async runtime"
)]
fn failed_rpc_validation_exits_nonzero_after_saving_complete_metrics() {
    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    let mut server = StopServer(
        Command::new(binary)
            .args(["load-server", "--transport=native", "--port=0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start identity-only response server"),
    );
    let port = read_ready_port(&mut server.0);
    for (transport, codec) in [
        ("native", "pbrs"),
        ("native", "prost"),
        ("tonic", "pbrs"),
        ("tonic", "prost"),
    ] {
        let report = std::env::temp_dir().join(format!(
            "pbrs-load-failure-{}-{transport}-{codec}.json",
            std::process::id(),
        ));
        let out = Command::new(binary)
            .args([
                "load",
                "--duration-secs=0.05",
                "--max-in-flight=1",
                "--compression=gzip",
            ])
            .arg(format!("--transport={transport}"))
            .arg(format!("--codec={codec}"))
            .arg(format!("--server_addr=127.0.0.1:{port}"))
            .arg(format!("--output={}", report.display()))
            .output()
            .expect("run mismatched response encoding");
        let saved = std::fs::read(&report).expect("failed run must save its report");
        std::fs::remove_file(report).expect("remove test report");
        let metrics: serde_json::Value = serde_json::from_slice(&saved).expect("parse report");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(1), "{transport}/{codec}: {text}");
        assert!(text.contains("load run failed"), "{text}");
        assert!(metrics["failed_rpcs"].as_u64().unwrap() > 0, "{metrics}");
        assert_eq!(metrics["successful_rpcs"], 0);
        assert_eq!(metrics["offered_rpcs"], metrics["dispatched_rpcs"]);
        assert_eq!(metrics["dispatched_rpcs"], metrics["completed_rpcs"]);
        assert_eq!(metrics["completed_rpcs"], metrics["failed_rpcs"]);
        assert_eq!(metrics["unstarted_rpcs"], 0);
        assert_eq!(metrics["unfinished_rpcs"], 0);
    }
}

#[test]
fn load_native_prost_and_pipelined_cells() {
    for transport in ["--transport=native", "--transport=tonic"] {
        for codec in ["--codec=pbrs", "--codec=prost"] {
            for shape in [
                "--shape=unary",
                "--shape=server_stream",
                "--shape=client_stream",
                "--shape=bidi",
                "--shape=bidi_pipelined",
            ] {
                assert_clean_cell(&[
                    transport,
                    codec,
                    shape,
                    "--stream-msgs=16",
                    "--req-bytes=65536",
                    "--resp-bytes=65536",
                    "--max-in-flight=1",
                ]);
            }
        }
    }
}

#[test]
fn fixed_rpc_count_is_exact_and_cannot_pass_on_partial_work() {
    for transport in ["--transport=native", "--transport=tonic"] {
        for codec in ["--codec=pbrs", "--codec=prost"] {
            let (code, text) = run_load(&[transport, codec, "--rpc-count=17", "--duration-secs=5"]);
            assert_eq!(code, 0, "{text}");
            assert_eq!(counters(&text), (17, 0, 0, 0));
        }
    }
    let (code, text) = run_load(&["--rpc-count=1000000", "--duration-secs=0.000001"]);
    assert_eq!(code, 1, "incomplete fixed count must fail: {text}");
    for value in ["--rpc-count=0", "--rpc-count=-1", "--rpc-count=1000001"] {
        assert_eq!(run_load(&[value]).0, 2);
    }
}
