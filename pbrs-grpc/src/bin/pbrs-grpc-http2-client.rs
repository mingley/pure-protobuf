//! HTTP/2 negative and conformance interop client adapter:
//! `--server_host=HOST`, `--server_port=PORT`, `--test_case=CASE`.
//!
//! Implements official HTTP/2 test cases:
//! - `goaway`: sends first UnaryCall (response_size 314159, payload zeros 271828);
//!   sleeps 1s; sends second UnaryCall; asserts both succeed with response body 314159.
//! - `rst_after_header`: sends UnaryCall; asserts call fails (non-zero / error status).
//! - `rst_during_data`: sends UnaryCall; asserts call fails.
//! - `rst_after_data`: sends UnaryCall; asserts call fails.
//! - `ping`: sends UnaryCall; asserts call succeeds with response body 314159 zeros.
//! - `max_streams`: sends initial UnaryCall, then sends 10 concurrent UnaryCalls under
//!   server's MAX_CONCURRENT_STREAMS; asserts all succeed with response body 314159 zeros.
//! - `data_frame_padding`: sends UnaryCall expecting padded DATA frames; asserts call
//!   succeeds without flow-control deadlock and with response body 314159 zeros.
//! - `no_df_padding_sanity_test`: sends UnaryCall expecting small unpadded DATA frames;
//!   asserts call succeeds with response body 314159 zeros.
//!
//! Exits 0 on test pass, non-zero on assertion failure.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop binary"
)]

use pbrs_grpc::{
    Channel, ChannelConfig, Payload, Request, SimpleRequest, SimpleResponse, Status,
    TestServiceClient,
};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

const LARGE_REQ: i32 = 271_828;
const LARGE_RESP: i32 = 314_159;

struct Args {
    server_host: String,
    server_port: u16,
    test_case: String,
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

fn parse_args() -> Args {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut server_host = "127.0.0.1".to_string();
    let mut server_port = 10000u16;
    let mut test_case = "goaway".to_string();

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
            "server_host" => {
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
                server_host = val;
            }
            "server_port" => {
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
                server_port = parse_port(raw_key, &val);
            }
            "test_case" => {
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
                test_case = val;
            }
            "use_tls" => {
                // Ignore optional use_tls flag if passed
                if inline_val.is_none() {
                    if let Some(next) = raw_args.get(i + 1) {
                        if !next.starts_with('-') {
                            i += 1;
                        }
                    }
                }
            }
            _ => {
                // Ignore any other optional runner flags
                if inline_val.is_none() {
                    if let Some(next) = raw_args.get(i + 1) {
                        if !next.starts_with('-') {
                            i += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }

    Args {
        server_host,
        server_port,
        test_case,
    }
}

fn large_simple_request() -> SimpleRequest {
    let mut req = SimpleRequest::new();
    req.set_response_size(LARGE_RESP);
    let mut payload = Payload::new();
    let n = usize::try_from(LARGE_REQ.max(0)).unwrap_or(0);
    payload.set_body(vec![0u8; n]);
    req.set_payload(payload);
    req
}

fn assert_response_payload(resp: &SimpleResponse, expected_len: i32) -> Result<(), Status> {
    let expected = usize::try_from(expected_len.max(0)).unwrap_or(0);
    let body = resp.payload().body();
    if body.len() != expected {
        return Err(Status::internal(format!(
            "response payload body len is {}, want {expected}",
            body.len()
        )));
    }
    if body.iter().any(|&b| b != 0) {
        return Err(Status::internal(
            "response payload body contents are not all zeros",
        ));
    }
    Ok(())
}

/// Case `goaway`:
/// Sends first UnaryCall (response_size 314159, payload zeros 271828);
/// sleeps 1s; sends second UnaryCall; asserts both succeed with response body 314159.
async fn run_goaway(client: &TestServiceClient) -> Result<(), Status> {
    let req1 = large_simple_request();
    let resp1 = client.unary_call(Request::new(req1)).await?;
    assert_response_payload(&resp1.into_inner(), LARGE_RESP)?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let req2 = large_simple_request();
    let resp2 = client.unary_call(Request::new(req2)).await?;
    assert_response_payload(&resp2.into_inner(), LARGE_RESP)?;

    Ok(())
}

/// Case `rst_after_header`:
/// Sends UnaryCall; asserts call fails (non-zero / error status).
async fn run_rst_after_header(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    match client.unary_call(Request::new(req)).await {
        Ok(_) => Err(Status::internal(
            "rst_after_header: expected call to fail, but succeeded with Ok",
        )),
        Err(_) => Ok(()),
    }
}

/// Case `rst_during_data`:
/// Sends UnaryCall; asserts call fails.
async fn run_rst_during_data(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    match client.unary_call(Request::new(req)).await {
        Ok(_) => Err(Status::internal(
            "rst_during_data: expected call to fail, but succeeded with Ok",
        )),
        Err(_) => Ok(()),
    }
}

/// Case `rst_after_data`:
/// Sends UnaryCall; asserts call fails.
async fn run_rst_after_data(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    match client.unary_call(Request::new(req)).await {
        Ok(_) => Err(Status::internal(
            "rst_after_data: expected call to fail, but succeeded with Ok",
        )),
        Err(_) => Ok(()),
    }
}

/// Case `ping`:
/// Sends UnaryCall; asserts call succeeds with response body 314159 zeros.
/// Waits briefly before exit so the peer observes the final PING ACK.
async fn run_ping(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    let resp = client.unary_call(Request::new(req)).await?;
    assert_response_payload(&resp.into_inner(), LARGE_RESP)?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok(())
}

/// Case `max_streams`:
/// Sends initial UnaryCall to allow server to update MAX_CONCURRENT_STREAMS;
/// then concurrently sends 10 UnaryCalls; asserts all 11 succeed with response
/// body 314159 zeros.
async fn run_max_streams(client: &TestServiceClient) -> Result<(), Status> {
    let req0 = large_simple_request();
    let resp0 = client.unary_call(Request::new(req0)).await?;
    assert_response_payload(&resp0.into_inner(), LARGE_RESP)?;

    let mut tasks = Vec::with_capacity(10);
    for _ in 0..10 {
        let client_clone = client.clone();
        tasks.push(tokio::spawn(async move {
            let req = large_simple_request();
            client_clone.unary_call(Request::new(req)).await
        }));
    }

    for task in tasks {
        let resp = task
            .await
            .map_err(|e| Status::internal(format!("join error: {e}")))?
            .map_err(|e| Status::internal(format!("concurrent unary call failed: {e}")))?;
        assert_response_payload(&resp.into_inner(), LARGE_RESP)?;
    }

    Ok(())
}

/// Case `data_frame_padding`:
/// Sends UnaryCall expecting padded DATA frames; asserts call succeeds
/// without flow-control deadlock and with response body 314159 zeros.
async fn run_data_frame_padding(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    let resp = client.unary_call(Request::new(req)).await?;
    assert_response_payload(&resp.into_inner(), LARGE_RESP)
}

/// Case `no_df_padding_sanity_test`:
/// Sends UnaryCall expecting small unpadded DATA frames; asserts call succeeds
/// with response body 314159 zeros.
async fn run_no_df_padding_sanity_test(client: &TestServiceClient) -> Result<(), Status> {
    let req = large_simple_request();
    let resp = client.unary_call(Request::new(req)).await?;
    assert_response_payload(&resp.into_inner(), LARGE_RESP)
}

async fn run(args: Args) -> Result<(), Status> {
    let addr: SocketAddr = (args.server_host.as_str(), args.server_port)
        .to_socket_addrs()
        .map_err(|e| Status::unavailable(e.to_string()))?
        .next()
        .ok_or_else(|| Status::unavailable("resolve"))?;

    let config = ChannelConfig::new().data_frame_budget(16 * 1024 * 1024);
    let channel = Channel::connect_with(addr, config).await?;
    let client = TestServiceClient::new(channel);

    match args.test_case.as_str() {
        "goaway" => run_goaway(&client).await,
        "rst_after_header" => run_rst_after_header(&client).await,
        "rst_during_data" => run_rst_during_data(&client).await,
        "rst_after_data" => run_rst_after_data(&client).await,
        "ping" => run_ping(&client).await,
        "max_streams" => run_max_streams(&client).await,
        "data_frame_padding" => run_data_frame_padding(&client).await,
        "no_df_padding_sanity_test" => run_no_df_padding_sanity_test(&client).await,
        other => Err(Status::invalid_argument(format!(
            "unknown test_case: {other}"
        ))),
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let test_case = args.test_case.clone();
    match run(args).await {
        Ok(()) => {
            println!("Passed");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Test case {test_case} failed: {e}");
            std::process::exit(1);
        }
    }
}
