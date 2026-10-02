//! SB-11: the `load` subcommand drives every (transport, shape) cell against
//! a loopback server with zero failures, and rejects invalid flag mixes.
//! Fails if any cell errors, reports failures/timeouts, or accepts a mix
//! the parser must reject (TLS halves, tonic+TLS, BenchmarkService scope).

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

    // tonic + TLS is explicitly unsupported.
    let (code, text) = run_load(&[
        "--transport=tonic",
        "--tls-ca=/tmp/ca.pem",
        "--tls-server-name=localhost",
    ]);
    assert_eq!(code, 2, "tonic+tls: {text}");
    assert!(text.contains("no TLS support"), "tonic+tls: {text}");

    // TLS without an explicit server is rejected before bind.
    let (code, text) = run_load(&["--tls-ca=/tmp/ca.pem", "--tls-server-name=localhost"]);
    assert_eq!(code, 1, "tls loopback: {text}");
    assert!(text.contains("plaintext-only"), "tls loopback: {text}");
}
