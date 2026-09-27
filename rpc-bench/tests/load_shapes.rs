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
                std::thread::sleep(Duration::from_millis(50));
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
    assert!(
        successes > 0,
        "load {args:?} reported no successes: {text}"
    );
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
    let (code, text) = run_load(&[
        "--tls-ca=/tmp/ca.pem",
        "--tls-server-name=localhost",
    ]);
    assert_eq!(code, 1, "tls loopback: {text}");
    assert!(text.contains("plaintext-only"), "tls loopback: {text}");
}
