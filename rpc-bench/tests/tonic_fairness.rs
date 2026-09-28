//! SB-01: the tonic bench server must set TCP_NODELAY on accepted
//! sockets (tonic ignores the builder flag under serve_with_incoming)
//! and prove it with getsockopt. Fails if a tonic accepted socket
//! lacks NODELAY or the FAIRNESS record never prints.

use std::io::{BufRead, Read as _};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

#[test]
fn tonic_server_verifies_nodelay_on_accepted_sockets() {
    let binary = env!("CARGO_BIN_EXE_rpc-bench");
    let mut child = Command::new(binary)
        .arg("server")
        .arg("--transport=tonic")
        .arg("--port=0")
        .arg("--timeout-secs=6")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tonic server");

    let stdout = child.stdout.take().expect("stdout pipe");
    let mut reader = std::io::BufReader::new(stdout);
    let mut port: Option<u16> = None;
    let mut lines: Vec<String> = Vec::new();
    // Read until READY (bounded: the server prints it immediately).
    for _ in 0..50 {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read server line");
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("READY ") {
            for kv in rest.split_whitespace() {
                if let Some(value) = kv.strip_prefix("port=") {
                    port = value.parse().ok();
                }
            }
        }
        lines.push(line);
        if port.is_some() {
            break;
        }
    }
    let port = port.expect("server printed READY with a port");

    // Two raw connections: the adapter must set + verify NODELAY on both.
    let mut conns = Vec::new();
    for _ in 0..2 {
        let conn = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().expect("addr"),
            Duration::from_secs(5),
        )
        .expect("connect");
        conns.push(conn);
    }
    // Hold the connections until the server shuts down: the adapter
    // counts at accept, and the FAIRNESS record prints at shutdown.
    let mut rest = String::new();
    reader.read_to_string(&mut rest).expect("drain stdout");
    lines.extend(rest.lines().map(|l| format!("{l}\n")));
    drop(conns);
    let status = child.wait().expect("wait");
    assert!(status.success(), "tonic server must exit 0 when fair");

    let fairness = lines
        .iter()
        .find_map(|line| line.strip_prefix("FAIRNESS "))
        .expect("server printed a FAIRNESS record");
    let record: Value = serde_json::from_str(fairness.trim()).expect("fairness JSON parses");
    assert_eq!(record["transport"], Value::String("tonic".to_string()));
    assert_eq!(
        record["tcp_nodelay_observed"],
        Value::Bool(true),
        "getsockopt must observe NODELAY on: {record}"
    );
    let accepted = record["accepted_conns"].as_u64().expect("accepted count");
    let verified = record["nodelay_verified_conns"]
        .as_u64()
        .expect("verified count");
    assert!(accepted >= 2, "both probe connections counted: {record}");
    assert_eq!(verified, accepted, "every connection verified: {record}");
    // Matched native transport scalars (SB-01 spec).
    assert_eq!(record["stream_window"], Value::from(16 * 1024 * 1024));
    assert_eq!(record["conn_window"], Value::from(16 * 1024 * 1024));
    assert_eq!(record["frame_size"], Value::from(1024 * 1024));
    assert_eq!(record["max_streams"], Value::from(256));
    assert_eq!(record["adaptive_window"], Value::Bool(false));
}
