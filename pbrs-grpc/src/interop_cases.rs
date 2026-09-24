//! Official `--test_case` procedures driven through the shipped kernel client.

use crate::Request;
use crate::status::{Code, Status};
use crate::stream::Framed;
use crate::testing::{
    BoolValue, Empty, Payload, SimpleRequest, StreamingInputCallRequest,
    StreamingOutputCallRequest, TestServiceClient, UnimplementedServiceClient,
};
use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const LARGE_REQ: i32 = 271828;
const LARGE_RESP: i32 = 314159;
const INITIAL_MD: &str = "x-grpc-test-echo-initial";
const INITIAL_VAL: &str = "test_initial_metadata_value";
const TRAILING_MD: &str = "x-grpc-test-echo-trailing-bin";
const TRAILING_VAL: &[u8] = &[0xab, 0xab, 0xab];

fn zeros(n: i32) -> Payload {
    let n = usize::try_from(n.max(0)).unwrap_or(0);
    let mut p = Payload::new();
    p.set_body(vec![0u8; n]);
    p
}

fn incompressible(n: i32) -> Result<Payload, Status> {
    let n = usize::try_from(n).map_err(|_| Status::invalid_argument("negative payload size"))?;
    let mut body = Vec::with_capacity(n);
    let mut state = 0x243f_6a88_85a3_08d3u64;
    while body.len() < n {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        body.extend_from_slice(&state.to_le_bytes());
    }
    body.truncate(n);
    let mut payload = Payload::new();
    payload.set_body(body);
    Ok(payload)
}

fn bool_val(v: bool) -> BoolValue {
    let mut b = BoolValue::new();
    b.set_value(v);
    b
}

fn assert_zero_payload(payload: &Payload, expected_len: i32) -> Result<(), Status> {
    let got = i32::try_from(payload.body().len()).unwrap_or(i32::MAX);
    if got != expected_len {
        return Err(Status::internal(format!(
            "payload len {got} want {expected_len}"
        )));
    }
    if payload.body().iter().any(|&b| b != 0) {
        return Err(Status::internal(
            "payload body contains non-zero bytes; expected all zeroes",
        ));
    }
    Ok(())
}

fn assert_status(got: &Status, expected_code: Code, expected_message: &str) -> Result<(), Status> {
    if got.code() != expected_code {
        return Err(Status::internal(format!(
            "status code mismatch: got {:?} want {:?}",
            got.code(),
            expected_code
        )));
    }
    if got.message() != expected_message {
        return Err(Status::internal(format!(
            "status message mismatch: got {:?} want {:?}",
            got.message(),
            expected_message
        )));
    }
    Ok(())
}

fn assert_payload_len(resp: &crate::testing::SimpleResponse, n: i32) -> Result<(), Status> {
    assert_zero_payload(resp.payload(), n)
}

/// Run one official uncompressed or gzip `_TEST_CASES` name.
///
/// Applies to the call shapes that case uses, including over TLS, mTLS,
/// Unix, and [`crate::Channel::from_io`].
pub async fn run_case(client: &TestServiceClient, name: &str) -> Result<(), Status> {
    match name {
        "empty_unary" => empty_unary(client).await,
        "large_unary" => large_unary(client).await,
        "client_streaming" => client_streaming(client).await,
        "server_streaming" => server_streaming(client).await,
        "ping_pong" => ping_pong(client).await,
        "empty_stream" => empty_stream(client).await,
        "cancel_after_begin" => cancel_after_begin(client).await,
        "cancel_after_first_response" => cancel_after_first_response(client).await,
        "timeout_on_sleeping_server" => timeout_on_sleeping_server(client).await,
        "custom_metadata" => custom_metadata(client).await,
        "status_code_and_message" => status_code_and_message(client).await,
        "special_status_message" => special_status_message(client).await,
        "unimplemented_method" => unimplemented_method(client).await,
        "unimplemented_service" => unimplemented_service(client).await,
        "client_compressed_unary" => client_compressed_unary(client).await,
        "server_compressed_unary" => server_compressed_unary(client).await,
        "client_compressed_streaming" => client_compressed_streaming(client).await,
        "server_compressed_streaming" => server_compressed_streaming(client).await,
        "rpc_soak" => rpc_soak(client, &SoakConfig::default()).await.map(|_| ()),
        "channel_soak" => channel_soak(client, &SoakConfig::default())
            .await
            .map(|_| ()),
        other => Err(Status::invalid_argument(format!(
            "unknown test_case {other}"
        ))),
    }
}

/// The official `empty_unary` test case.
pub async fn empty_unary(client: &TestServiceClient) -> Result<(), Status> {
    let _ = client.empty_call(Request::new(Empty::new())).await?;
    Ok(())
}

/// The official `large_unary` test case.
pub async fn large_unary(client: &TestServiceClient) -> Result<(), Status> {
    let mut req = SimpleRequest::new();
    req.set_response_size(LARGE_RESP);
    req.set_payload(zeros(LARGE_REQ));
    let resp = client.unary_call(Request::new(req)).await?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)
}

/// The official `client_streaming` test case.
pub async fn client_streaming(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.streaming_input_call(Request::new(()));
    for n in [27182, 8, 1828, 45904] {
        let mut m = StreamingInputCallRequest::new();
        m.set_payload(zeros(n));
        tx.send(m).await?;
    }
    tx.close();
    let resp = call.await?;
    let got = resp.into_inner().aggregated_payload_size();
    if got != 74922 {
        return Err(Status::internal(format!(
            "client_streaming: agg {got} want 74922"
        )));
    }
    Ok(())
}

/// The official `server_streaming` test case.
pub async fn server_streaming(client: &TestServiceClient) -> Result<(), Status> {
    let expected = [31415, 9, 2653, 58979];
    let mut req = StreamingOutputCallRequest::new();
    for &n in &expected {
        let mut p = crate::testing::ResponseParameters::new();
        p.set_size(n);
        req.response_parameters_mut().push(p);
    }
    let resp = client.streaming_output_call(Request::new(req)).await?;
    let mut inbound = resp.into_inner();
    let mut count = 0;
    while let Some(m) = inbound.message().await? {
        if count >= expected.len() {
            return Err(Status::internal(
                "server_streaming: received more messages than requested",
            ));
        }
        assert_zero_payload(m.payload(), expected.get(count).copied().unwrap_or(0))?;
        count += 1;
    }
    if count != expected.len() {
        return Err(Status::internal(format!(
            "server_streaming: received {count} messages, expected {}",
            expected.len()
        )));
    }
    Ok(())
}

/// The official `ping_pong` test case.
pub async fn ping_pong(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let first_resp = 31415i32;
    let first_req = 27182i32;
    let rest = [(9i32, 8i32), (2653, 1828), (58979, 45904)];
    let mut first = StreamingOutputCallRequest::new();
    let mut p = crate::testing::ResponseParameters::new();
    p.set_size(first_resp);
    first.response_parameters_mut().push(p);
    first.set_payload(zeros(first_req));
    tx.send(first).await?;
    let resp = call.await?;
    let mut inbound = resp.into_inner();
    let got = inbound
        .message()
        .await?
        .ok_or_else(|| Status::internal("missing ping_pong reply"))?;
    assert_zero_payload(got.payload(), first_resp)?;
    for &(resp_size, req_size) in &rest {
        let mut m = StreamingOutputCallRequest::new();
        let mut p = crate::testing::ResponseParameters::new();
        p.set_size(resp_size);
        m.response_parameters_mut().push(p);
        m.set_payload(zeros(req_size));
        tx.send(m).await?;
        let got = inbound
            .message()
            .await?
            .ok_or_else(|| Status::internal("missing ping_pong reply"))?;
        assert_zero_payload(got.payload(), resp_size)?;
    }
    tx.close();
    if inbound.message().await?.is_some() {
        return Err(Status::internal(
            "ping_pong: received unexpected extra message after stream closed",
        ));
    }
    Ok(())
}

/// The official `empty_stream` test case.
pub async fn empty_stream(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.close();
    let resp = call.await?;
    let mut inbound = resp.into_inner();
    if inbound.message().await?.is_some() {
        return Err(Status::internal("empty_stream got a message"));
    }
    Ok(())
}

/// The official `cancel_after_begin` test case.
pub async fn cancel_after_begin(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.streaming_input_call(Request::new(()));
    let handle = call.handle();
    handle.cancel();
    // Hold `tx` until the call settles. Dropping it is a half-close, which
    // can complete StreamingInputCall as OK before the RST is observed.
    let result = match call.await {
        Err(st) if st.code() == Code::Cancelled => Ok(()),
        Err(st) => Err(Status::internal(format!(
            "cancel_after_begin: expected CANCELLED status, got {st}"
        ))),
        Ok(_) => Err(Status::internal(
            "cancel_after_begin: expected CANCELLED status, got Ok",
        )),
    };
    drop(tx);
    result
}

/// The client cancels the RPC after receiving the first response message,
/// asserting that subsequent reads terminate with CANCELLED status.
pub async fn cancel_after_first_response(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let handle = call.handle();
    let mut req = StreamingOutputCallRequest::new();
    let mut p = crate::testing::ResponseParameters::new();
    p.set_size(31415);
    req.response_parameters_mut().push(p);
    req.set_payload(zeros(27182));
    tx.send(req).await?;
    let resp = call.await?;
    let mut inbound = resp.into_inner();
    inbound
        .message()
        .await?
        .ok_or_else(|| Status::internal("missing first response"))?;
    handle.cancel();
    let result = match inbound.message().await {
        Err(st) if st.code() == Code::Cancelled => Ok(()),
        Ok(None) => Err(Status::internal(
            "cancel_after_first_response: expected CANCELLED status, got clean EOF",
        )),
        Ok(Some(_)) => Err(Status::internal(
            "cancel_after_first_response: expected CANCELLED status, got extra message",
        )),
        Err(st) => Err(Status::internal(format!(
            "cancel_after_first_response: expected CANCELLED status, got {st}",
        ))),
    };
    drop(tx);
    result
}

/// The server accepts the stream and then never answers, so the deadline has to
/// fire on the *read*, not just on call setup. The official client asserts the
/// status of the first receive for exactly this reason.
pub async fn timeout_on_sleeping_server(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = {
        let mut r = Request::new(());
        r.set_timeout(Duration::from_millis(1));
        client.full_duplex_call(r)
    };
    let mut req = StreamingOutputCallRequest::new();
    req.set_payload(zeros(27182));
    tx.send(req).await.ok();
    let status = match call.await {
        // Setup lost the race with the deadline: already the expected answer.
        Err(st) => st,
        Ok(resp) => match resp.into_inner().message().await {
            Err(st) => st,
            Ok(Some(_)) => return Err(Status::internal("sleeping server sent a message")),
            Ok(None) => return Err(Status::internal("want DEADLINE_EXCEEDED got clean end")),
        },
    };
    if status.code() == Code::DeadlineExceeded {
        Ok(())
    } else {
        Err(Status::internal(format!(
            "want DEADLINE_EXCEEDED got {status}"
        )))
    }
}

fn attach_custom_md<T>(req: &mut Request<T>) {
    req.metadata_mut().insert(INITIAL_MD, INITIAL_VAL).ok();
    req.metadata_mut()
        .insert_bin(TRAILING_MD, TRAILING_VAL)
        .ok();
}

fn check_custom_md(headers: &crate::Metadata, trailers: &crate::Metadata) -> Result<(), Status> {
    if headers.get(INITIAL_MD) != Some(INITIAL_VAL) {
        return Err(Status::internal("missing initial metadata"));
    }
    if headers.get(TRAILING_MD).is_some() || headers.get_bin(TRAILING_MD).is_some() {
        return Err(Status::internal("trailing-bin in headers"));
    }
    if trailers.get(INITIAL_MD).is_some() {
        return Err(Status::internal("initial metadata leaked into trailers"));
    }
    if trailers.get_bin(TRAILING_MD).as_deref() != Some(TRAILING_VAL) {
        return Err(Status::internal("missing trailing-bin trailers"));
    }
    Ok(())
}

/// The official `custom_metadata` test case.
pub async fn custom_metadata(client: &TestServiceClient) -> Result<(), Status> {
    let mut sr = SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    sr.set_payload(zeros(LARGE_REQ));
    let mut req = Request::new(sr);
    attach_custom_md(&mut req);
    let resp = client.unary_call(req).await?;
    check_custom_md(resp.metadata(), resp.trailers())?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)?;

    let mut fd = Request::new(());
    attach_custom_md(&mut fd);
    let (tx, call) = client.full_duplex_call(fd);
    let mut m = StreamingOutputCallRequest::new();
    let mut p = crate::testing::ResponseParameters::new();
    p.set_size(LARGE_RESP);
    m.response_parameters_mut().push(p);
    m.set_payload(zeros(LARGE_REQ));
    tx.send(m).await?;
    tx.close();
    let resp = call.await?;
    if resp.metadata().get(INITIAL_MD) != Some(INITIAL_VAL) {
        return Err(Status::internal("missing initial metadata"));
    }
    if resp.metadata().get(TRAILING_MD).is_some() || resp.metadata().get_bin(TRAILING_MD).is_some()
    {
        return Err(Status::internal("trailing-bin in headers"));
    }
    let mut inbound = resp.into_inner();
    let mut count = 0;
    while let Some(msg) = inbound.message().await? {
        assert_zero_payload(msg.payload(), LARGE_RESP)?;
        count += 1;
    }
    if count != 1 {
        return Err(Status::internal(format!(
            "custom_metadata: expected 1 duplex message, got {count}"
        )));
    }
    let trailers = inbound.trailers().await?;
    if trailers.get(INITIAL_MD).is_some() {
        return Err(Status::internal("initial metadata leaked into trailers"));
    }
    if trailers.get_bin(TRAILING_MD).as_deref() != Some(TRAILING_VAL) {
        return Err(Status::internal("missing trailing-bin trailers"));
    }
    Ok(())
}

/// The official `status_code_and_message` test case.
pub async fn status_code_and_message(client: &TestServiceClient) -> Result<(), Status> {
    let want_code = Code::Unknown;
    let want_msg = "test status message";
    let mut sr = SimpleRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    sr.set_response_status(es);
    match client.unary_call(Request::new(sr)).await {
        Err(st) => assert_status(&st, want_code, want_msg)?,
        Ok(_) => return Err(Status::internal("unary status got ok")),
    }
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let mut m = StreamingOutputCallRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    m.set_response_status(es);
    tx.send(m).await?;
    tx.close();
    match call.await {
        Err(st) => assert_status(&st, want_code, want_msg)?,
        Ok(resp) => {
            let mut inbound = resp.into_inner();
            match inbound.message().await {
                Err(st) => assert_status(&st, want_code, want_msg)?,
                Ok(Some(_)) => return Err(Status::internal("duplex status extra message")),
                Ok(None) => return Err(Status::internal("duplex status missing")),
            }
        }
    }
    Ok(())
}

/// The official `special_status_message` test case.
pub async fn special_status_message(client: &TestServiceClient) -> Result<(), Status> {
    let want_code = Code::Unknown;
    let want_msg = "\t\ntest with whitespace\r\nand Unicode BMP ☺ and non-BMP 😈\t\n";
    let mut sr = SimpleRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    sr.set_response_status(es);
    match client.unary_call(Request::new(sr)).await {
        Err(st) => assert_status(&st, want_code, want_msg),
        Ok(_) => Err(Status::internal("want status")),
    }
}

/// A method the service declares but the server refuses.
pub async fn unimplemented_method(client: &TestServiceClient) -> Result<(), Status> {
    expect_unimplemented(client.unimplemented_call(Request::new(Empty::new())).await)
}

/// A service the server does not host at all, so the router rejects the path.
pub async fn unimplemented_service(client: &TestServiceClient) -> Result<(), Status> {
    let absent = UnimplementedServiceClient::new(client.channel().clone());
    expect_unimplemented(absent.unimplemented_call(Request::new(Empty::new())).await)
}

fn expect_unimplemented<T>(result: Result<T, Status>) -> Result<(), Status> {
    match result {
        Err(st) if st.code() == Code::Unimplemented => Ok(()),
        Ok(_) => Err(Status::internal("want UNIMPLEMENTED got ok")),
        Err(st) => Err(Status::internal(format!("want UNIMPLEMENTED {st}"))),
    }
}

/// The official `client_compressed_unary` test case.
pub async fn client_compressed_unary(client: &TestServiceClient) -> Result<(), Status> {
    let mut probe = SimpleRequest::new();
    probe.set_expect_compressed(bool_val(true));
    probe.set_response_size(LARGE_RESP);
    probe.set_payload(zeros(LARGE_REQ));
    match client.unary_call(Request::new(probe)).await {
        Err(st) if st.code() == Code::InvalidArgument => {}
        Ok(_) => return Err(Status::internal("probe want INVALID_ARGUMENT got ok")),
        Err(st) => {
            return Err(Status::internal(format!(
                "probe want INVALID_ARGUMENT {st}"
            )));
        }
    }
    let mut compressed = SimpleRequest::new();
    compressed.set_expect_compressed(bool_val(true));
    compressed.set_response_size(LARGE_RESP);
    compressed.set_payload(zeros(LARGE_REQ));
    let mut req = Request::new(compressed);
    req.set_compress(true);
    let resp = client.unary_call(req).await?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)?;

    let mut uncompressed = SimpleRequest::new();
    uncompressed.set_expect_compressed(bool_val(false));
    uncompressed.set_response_size(LARGE_RESP);
    uncompressed.set_payload(zeros(LARGE_REQ));
    let resp = client.unary_call(Request::new(uncompressed)).await?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)?;

    // The three calls above are the official procedure. This extra compressed
    // leg checks a high-entropy body against the independent peer as well.
    let mut entropy = SimpleRequest::new();
    entropy.set_expect_compressed(bool_val(true));
    entropy.set_response_size(LARGE_RESP);
    entropy.set_payload(incompressible(LARGE_REQ)?);
    let mut request = Request::new(entropy);
    request.set_compress(true);
    let resp = client.unary_call(request).await?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)
}

/// The official `server_compressed_unary` test case.
pub async fn server_compressed_unary(client: &TestServiceClient) -> Result<(), Status> {
    for flag in [true, false] {
        let mut sr = SimpleRequest::new();
        sr.set_response_compressed(bool_val(flag));
        sr.set_response_size(LARGE_RESP);
        sr.set_payload(zeros(LARGE_REQ));
        let resp = client.unary_call(Request::new(sr)).await?;
        if resp.compressed() != flag {
            return Err(Status::internal(format!(
                "compressed flag {} want {flag}",
                resp.compressed()
            )));
        }
        if flag {
            let enc = resp
                .encoding()
                .or_else(|| resp.metadata().get("grpc-encoding"));
            if enc != Some("gzip") {
                return Err(Status::internal(format!(
                    "expected grpc-encoding: gzip, got {enc:?}"
                )));
            }
        } else {
            let enc = resp
                .encoding()
                .or_else(|| resp.metadata().get("grpc-encoding"));
            if enc.is_some_and(|e| e.eq_ignore_ascii_case("gzip")) {
                return Err(Status::internal(format!(
                    "uncompressed response must not advertise gzip encoding: {enc:?}"
                )));
            }
        }
        assert_payload_len(&resp.into_inner(), LARGE_RESP)?;
    }
    Ok(())
}

/// The official `client_compressed_streaming` test case.
pub async fn client_compressed_streaming(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.streaming_input_call(Request::new(()));
    let mut probe = StreamingInputCallRequest::new();
    probe.set_expect_compressed(bool_val(true));
    probe.set_payload(zeros(27182));
    tx.send(probe).await?;
    tx.close();
    match call.await {
        Err(st) if st.code() == Code::InvalidArgument => {}
        Ok(_) => return Err(Status::internal("probe want INVALID_ARGUMENT got ok")),
        Err(st) => {
            return Err(Status::internal(format!(
                "probe want INVALID_ARGUMENT {st}"
            )));
        }
    }
    // Negotiate grpc-encoding at the call level (required by reference peers
    // whenever any message carries the compression bit), but leave the
    // per-message default off so only the first message is compressed.
    let mut open = Request::new(());
    open.set_compress(true);
    let (mut tx, call) = client.streaming_input_call(open);
    tx.set_compress(false);
    let mut a = StreamingInputCallRequest::new();
    a.set_expect_compressed(bool_val(true));
    a.set_payload(zeros(27182));
    tx.send_compressed(a).await?;
    let mut b = StreamingInputCallRequest::new();
    b.set_expect_compressed(bool_val(false));
    b.set_payload(zeros(45904));
    tx.send(b).await?;
    tx.close();
    let resp = call.await?;
    let got = resp.into_inner().aggregated_payload_size();
    if got != 73086 {
        return Err(Status::internal(format!("agg {got} want 73086")));
    }
    Ok(())
}

/// The official `server_compressed_streaming` test case.
pub async fn server_compressed_streaming(client: &TestServiceClient) -> Result<(), Status> {
    let mut req = StreamingOutputCallRequest::new();
    let mut p0 = crate::testing::ResponseParameters::new();
    p0.set_size(31415);
    p0.set_compressed(bool_val(true));
    req.response_parameters_mut().push(p0);
    let mut p1 = crate::testing::ResponseParameters::new();
    p1.set_size(92653);
    p1.set_compressed(bool_val(false));
    req.response_parameters_mut().push(p1);
    let resp = client.streaming_output_call(Request::new(req)).await?;
    let enc = resp
        .encoding()
        .or_else(|| resp.metadata().get("grpc-encoding"));
    if enc != Some("gzip") {
        return Err(Status::internal(format!(
            "server_compressed_streaming: expected grpc-encoding: gzip, got {enc:?}"
        )));
    }
    let mut inbound = resp.into_inner();
    let mut items: Vec<Framed<crate::testing::StreamingOutputCallResponse>> = Vec::new();
    while let Some(item) = inbound.next_framed().await? {
        items.push(item);
    }
    let first = items
        .first()
        .ok_or_else(|| Status::internal("missing first compressed reply"))?;
    let second = items
        .get(1)
        .ok_or_else(|| Status::internal("missing second compressed reply"))?;
    if items.len() != 2 {
        return Err(Status::internal(format!("got {} replies", items.len())));
    }
    if !first.compressed || second.compressed {
        return Err(Status::internal("compressed flags"));
    }
    assert_zero_payload(first.message.payload(), 31415)?;
    assert_zero_payload(second.message.payload(), 92653)?;
    Ok(())
}

/// Connect helper.
pub async fn connect(addr: SocketAddr) -> Result<TestServiceClient, Status> {
    let ch = crate::Channel::connect(addr).await?;
    Ok(TestServiceClient::new(ch))
}

/// Minimum iterations required to qualify as a full qualification soak.
pub const QUALIFICATION_SOAK_MIN_ITERATIONS: usize = 1000;

/// Configuration parameters for `rpc_soak` and `channel_soak`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoakConfig {
    /// Number of RPC iterations to execute across all threads. Default: 10.
    pub soak_iterations: usize,
    /// Maximum allowed failures before the test fails. Default: 0.
    pub max_failures: usize,
    /// Maximum acceptable latency per RPC in milliseconds. Default: 1000 ms.
    pub per_rpc_timeout_ms: u64,
    /// Overall deadline for the test in seconds. Default: 10 s.
    pub overall_timeout_seconds: u64,
    /// Minimum time in milliseconds between consecutive RPCs on each thread. Default: 0 ms.
    pub min_time_ms_between_rpcs: u64,
    /// Number of concurrent worker threads. Default: 1.
    pub soak_num_threads: usize,
    /// Request payload size in bytes. Default: 271828.
    pub request_size: i32,
    /// Response payload size in bytes. Default: 314159.
    pub response_size: i32,
    /// Explicit mode override (true = qualification soak, false = smoke).
    /// If None, inferred automatically based on soak_iterations >= 1000.
    pub qualification_mode: Option<bool>,
}

impl Default for SoakConfig {
    fn default() -> Self {
        Self {
            soak_iterations: 10,
            max_failures: 0,
            per_rpc_timeout_ms: 1000,
            overall_timeout_seconds: 10,
            min_time_ms_between_rpcs: 0,
            soak_num_threads: 1,
            request_size: LARGE_REQ,
            response_size: LARGE_RESP,
            qualification_mode: None,
        }
    }
}

impl SoakConfig {
    /// Validate configuration invariants.
    pub fn validate(&self) -> Result<(), Status> {
        if self.soak_iterations == 0 {
            return Err(Status::invalid_argument(
                "soak_iterations must be greater than 0",
            ));
        }
        if self.soak_num_threads == 0 {
            return Err(Status::invalid_argument(
                "soak_num_threads must be at least 1",
            ));
        }
        if self.soak_iterations % self.soak_num_threads != 0 {
            return Err(Status::invalid_argument(format!(
                "soak_iterations ({}) must be divisible by soak_num_threads ({})",
                self.soak_iterations, self.soak_num_threads
            )));
        }
        if self.qualification_mode == Some(true)
            && self.soak_iterations < QUALIFICATION_SOAK_MIN_ITERATIONS
        {
            return Err(Status::invalid_argument(format!(
                "cannot label run as qualification_soak with {} iterations: qualification soak requires at least {QUALIFICATION_SOAK_MIN_ITERATIONS} iterations; shorter runs are local smoke tests and cannot be labeled qualification soak",
                self.soak_iterations
            )));
        }
        Ok(())
    }

    /// Whether this configuration represents a full qualification soak.
    #[must_use]
    pub fn is_qualification(&self) -> bool {
        match self.qualification_mode {
            Some(mode) => mode,
            None => self.soak_iterations >= QUALIFICATION_SOAK_MIN_ITERATIONS,
        }
    }

    /// The human/machine readable label for this run: "qualification_soak" or "smoke".
    #[must_use]
    pub fn run_type(&self) -> &'static str {
        if self.is_qualification() {
            "qualification_soak"
        } else {
            "smoke"
        }
    }
}

/// Detailed failure class for soak accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoakFailureClass {
    /// RPC completed with a non-OK gRPC status.
    NonOkStatus(Code, String),
    /// RPC completed, but elapsed latency exceeded `per_rpc_timeout_ms`.
    LatencyBudgetExceeded {
        /// Elapsed time in milliseconds.
        elapsed_ms: u64,
        /// Latency budget in milliseconds.
        budget_ms: u64,
    },
    /// Response payload had incorrect length or non-zero bytes.
    PayloadMismatch(String),
    /// Channel connect or creation failed.
    ChannelError(String),
    /// Iteration omitted due to overall timeout / deadline expiry.
    Omitted,
}

impl SoakFailureClass {
    /// Descriptive name of the failure class.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NonOkStatus(..) => "non_ok_status",
            Self::LatencyBudgetExceeded { .. } => "latency_budget_exceeded",
            Self::PayloadMismatch(..) => "payload_mismatch",
            Self::ChannelError(..) => "channel_error",
            Self::Omitted => "omitted",
        }
    }
}

/// Breakdown of failures by class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FailureBreakdown {
    /// Count of RPCs that returned non-OK status.
    pub non_ok_status: usize,
    /// Count of RPCs that exceeded the configured latency budget.
    pub latency_budget_exceeded: usize,
    /// Count of RPCs whose payload failed validation.
    pub payload_mismatch: usize,
    /// Count of channel creation / connect failures.
    pub channel_error: usize,
    /// Count of omitted iterations due to overall deadline.
    pub omitted_iterations: usize,
}

impl FailureBreakdown {
    /// Sum of all failures across classes.
    #[must_use]
    pub fn total(&self) -> usize {
        self.non_ok_status
            + self.latency_budget_exceeded
            + self.payload_mismatch
            + self.channel_error
            + self.omitted_iterations
    }
}

/// One recorded failure in a soak run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoakFailure {
    /// Iteration index within the thread.
    pub iteration: usize,
    /// Thread ID that executed the iteration.
    pub thread_id: usize,
    /// Measured latency in milliseconds.
    pub elapsed_ms: u64,
    /// Failure class.
    pub class: SoakFailureClass,
    /// Diagnostic details or error string.
    pub details: String,
}

/// Latency distribution percentiles in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyStats {
    /// Minimum latency observed in milliseconds.
    pub min_ms: u64,
    /// 50th percentile (median) latency in milliseconds.
    pub p50_ms: u64,
    /// 90th percentile latency in milliseconds.
    pub p90_ms: u64,
    /// 99th percentile latency in milliseconds.
    pub p99_ms: u64,
    /// Maximum (worst) latency in milliseconds.
    pub max_ms: u64,
}

impl LatencyStats {
    /// Compute nearest-rank latency percentiles from measured samples.
    #[must_use]
    pub fn compute(mut samples: Vec<u64>) -> Self {
        if samples.is_empty() {
            return Self {
                min_ms: 0,
                p50_ms: 0,
                p90_ms: 0,
                p99_ms: 0,
                max_ms: 0,
            };
        }
        samples.sort_unstable();
        let n = samples.len();
        let pick = |percent: usize| {
            let last = n.saturating_sub(1);
            let index = (n * percent).div_ceil(100).saturating_sub(1).min(last);
            samples.get(index).copied().unwrap_or(0)
        };
        Self {
            min_ms: samples.first().copied().unwrap_or(0),
            p50_ms: pick(50),
            p90_ms: pick(90),
            p99_ms: pick(99),
            max_ms: samples.last().copied().unwrap_or(0),
        }
    }
}

/// Comprehensive accounting and outcome of a completed or failed soak test run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoakSummary {
    /// Test case name: "rpc_soak" or "channel_soak".
    pub test_case: String,
    /// Peer / server URI contacted.
    pub server_uri: String,
    /// Execution mode: "qualification_soak" or "smoke".
    pub run_type: String,
    /// Total iterations requested.
    pub iterations_requested: usize,
    /// Iterations that completed an RPC call.
    pub iterations_completed: usize,
    /// Iterations omitted due to deadline timeout.
    pub iterations_omitted: usize,
    /// Iterations that succeeded within status and latency budgets.
    pub iterations_succeeded: usize,
    /// Total failures across all classes.
    pub total_failures: usize,
    /// Configured failure budget threshold.
    pub max_failures: usize,
    /// Configured per-RPC latency budget in milliseconds.
    pub per_rpc_timeout_ms: u64,
    /// Configured overall timeout in seconds.
    pub overall_timeout_seconds: u64,
    /// Number of concurrent worker threads.
    pub thread_count: usize,
    /// Count of channels created.
    pub channels_created: usize,
    /// Count of channels dropped/closed.
    pub channels_dropped: usize,
    /// Detailed failure counts by class.
    pub failure_breakdown: FailureBreakdown,
    /// Individual failure records.
    pub failures: Vec<SoakFailure>,
    /// Latency percentiles.
    pub latency_stats: LatencyStats,
    /// Total wall-clock duration of the test.
    pub elapsed_duration: Duration,
}

impl SoakSummary {
    /// Format the human-readable report and machine-readable `[soak_summary]` line.
    #[must_use]
    pub fn summary_string(&self) -> String {
        format!(
            "(server_uri: {}) soak test {}: {} / {} iterations succeeded. Total failures: {} (budget: {}).\n\
             Latencies (ms): min={}, p50={}, p90={}, p99={}, max={}.\n\
             Resources: thread_count={}, channels_created={}, channels_dropped={}, duration_ms={}.\n\
             Failure breakdown: non_ok_status={}, latency_budget_exceeded={}, payload_mismatch={}, channel_error={}, omitted_iterations={}.\n\
             [soak_summary] test_case={} run_type={} iterations_requested={} iterations_completed={} iterations_omitted={} iterations_succeeded={} total_failures={} max_failures={} latency_budget_ms={} overall_timeout_s={} thread_count={} channels_created={} channels_dropped={} duration_ms={} non_ok_status={} latency_budget_exceeded={} payload_mismatch={} channel_error={} omitted_iterations={}",
            self.server_uri,
            self.run_type,
            self.iterations_succeeded,
            self.iterations_requested,
            self.total_failures,
            self.max_failures,
            self.latency_stats.min_ms,
            self.latency_stats.p50_ms,
            self.latency_stats.p90_ms,
            self.latency_stats.p99_ms,
            self.latency_stats.max_ms,
            self.thread_count,
            self.channels_created,
            self.channels_dropped,
            self.elapsed_duration.as_millis(),
            self.failure_breakdown.non_ok_status,
            self.failure_breakdown.latency_budget_exceeded,
            self.failure_breakdown.payload_mismatch,
            self.failure_breakdown.channel_error,
            self.failure_breakdown.omitted_iterations,
            self.test_case,
            self.run_type,
            self.iterations_requested,
            self.iterations_completed,
            self.iterations_omitted,
            self.iterations_succeeded,
            self.total_failures,
            self.max_failures,
            self.per_rpc_timeout_ms,
            self.overall_timeout_seconds,
            self.thread_count,
            self.channels_created,
            self.channels_dropped,
            self.elapsed_duration.as_millis(),
            self.failure_breakdown.non_ok_status,
            self.failure_breakdown.latency_budget_exceeded,
            self.failure_breakdown.payload_mismatch,
            self.failure_breakdown.channel_error,
            self.failure_breakdown.omitted_iterations,
        )
    }
}

struct WorkerIterationResult {
    iteration: usize,
    thread_id: usize,
    elapsed_ms: u64,
    success: bool,
    failure_class: Option<SoakFailureClass>,
    details: String,
}

fn build_soak_summary(
    test_case: &str,
    server_uri: String,
    config: &SoakConfig,
    results: Vec<WorkerIterationResult>,
    elapsed_duration: Duration,
    channels_created: usize,
    channels_dropped: usize,
) -> SoakSummary {
    let iterations_started = results.len();
    let iterations_completed = results
        .iter()
        .filter(|r| !matches!(r.failure_class.as_ref(), Some(SoakFailureClass::Omitted)))
        .count();
    let iterations_omitted = config.soak_iterations.saturating_sub(iterations_completed);
    let unstarted_omitted = config.soak_iterations.saturating_sub(iterations_started);
    let mut iterations_succeeded = 0usize;
    let mut failure_breakdown = FailureBreakdown::default();
    let mut failures = Vec::new();
    let mut latencies = Vec::with_capacity(iterations_completed);

    for r in results {
        latencies.push(r.elapsed_ms);
        if r.success {
            iterations_succeeded += 1;
        } else if let Some(fc) = r.failure_class {
            match fc {
                SoakFailureClass::NonOkStatus(..) => failure_breakdown.non_ok_status += 1,
                SoakFailureClass::LatencyBudgetExceeded { .. } => {
                    failure_breakdown.latency_budget_exceeded += 1
                }
                SoakFailureClass::PayloadMismatch(..) => failure_breakdown.payload_mismatch += 1,
                SoakFailureClass::ChannelError(..) => failure_breakdown.channel_error += 1,
                SoakFailureClass::Omitted => failure_breakdown.omitted_iterations += 1,
            }
            failures.push(SoakFailure {
                iteration: r.iteration,
                thread_id: r.thread_id,
                elapsed_ms: r.elapsed_ms,
                class: fc,
                details: r.details,
            });
        }
    }

    if unstarted_omitted > 0 {
        failure_breakdown.omitted_iterations += unstarted_omitted;
        for idx in 0..unstarted_omitted {
            failures.push(SoakFailure {
                iteration: iterations_started + idx,
                thread_id: 0,
                elapsed_ms: 0,
                class: SoakFailureClass::Omitted,
                details: "iteration omitted due to deadline timeout".to_string(),
            });
        }
    }

    let total_failures = failure_breakdown.total();
    let latency_stats = LatencyStats::compute(latencies);

    SoakSummary {
        test_case: test_case.to_string(),
        server_uri,
        run_type: config.run_type().to_string(),
        iterations_requested: config.soak_iterations,
        iterations_completed,
        iterations_omitted,
        iterations_succeeded,
        total_failures,
        max_failures: config.max_failures,
        per_rpc_timeout_ms: config.per_rpc_timeout_ms,
        overall_timeout_seconds: config.overall_timeout_seconds,
        thread_count: config.soak_num_threads,
        channels_created,
        channels_dropped,
        failure_breakdown,
        failures,
        latency_stats,
        elapsed_duration,
    }
}

fn evaluate_soak_summary(summary: SoakSummary, config: &SoakConfig) -> Result<SoakSummary, Status> {
    if summary.iterations_omitted > 0 {
        return Err(Status::new(
            Code::DeadlineExceeded,
            format!(
                "soak test {} timed out after {}s: completed {} out of {} iterations ({} omitted). Summary:\n{}",
                summary.test_case,
                config.overall_timeout_seconds,
                summary.iterations_completed,
                summary.iterations_requested,
                summary.iterations_omitted,
                summary.summary_string()
            ),
        ));
    }
    if summary.total_failures > config.max_failures {
        return Err(Status::internal(format!(
            "soak test {} total failures ({}) exceeded budget ({}). Summary:\n{}",
            summary.test_case,
            summary.total_failures,
            config.max_failures,
            summary.summary_string()
        )));
    }
    Ok(summary)
}

/// The official `rpc_soak` test case.
///
/// Sends `soak_iterations` large_unary RPCs sequentially or concurrently over
/// the shared channel, enforcing completion accounting, latency budgets, failure
/// budgets, and overall deadlines.
pub async fn rpc_soak(
    client: &TestServiceClient,
    config: &SoakConfig,
) -> Result<SoakSummary, Status> {
    config.validate()?;
    let server_uri = client.channel().authority().to_string();
    let iters_per_thread = config.soak_iterations / config.soak_num_threads;
    let start_total = Instant::now();
    let deadline = start_total + Duration::from_secs(config.overall_timeout_seconds);

    let mut join_set = tokio::task::JoinSet::new();

    for thread_id in 0..config.soak_num_threads {
        let client_clone = client.clone();
        let cfg = config.clone();
        let uri = server_uri.clone();
        join_set.spawn(async move {
            let mut results = Vec::with_capacity(iters_per_thread);
            let mut req = SimpleRequest::new();
            req.set_response_size(cfg.response_size);
            req.set_payload(zeros(cfg.request_size));

            for iteration in 0..iters_per_thread {
                if Instant::now() >= deadline {
                    break;
                }
                let earliest_next_start = Instant::now()
                    + Duration::from_millis(cfg.min_time_ms_between_rpcs);

                let rpc_start = Instant::now();
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let call_fut = client_clone.unary_call(Request::new(req.clone()));
                let rpc_res = tokio::time::timeout(remaining, call_fut).await;

                let elapsed = rpc_start.elapsed();
                let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);

                match rpc_res {
                    Err(_timeout) => {
                        eprintln!(
                            "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: overall deadline expired"
                        );
                        results.push(WorkerIterationResult {
                            iteration,
                            thread_id,
                            elapsed_ms,
                            success: false,
                            failure_class: Some(SoakFailureClass::Omitted),
                            details: "overall deadline expired".to_string(),
                        });
                        break;
                    }
                    Ok(Err(status)) => {
                        let details = format!("status {} ({})", status.code(), status.message());
                        eprintln!(
                            "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                        );
                        results.push(WorkerIterationResult {
                            iteration,
                            thread_id,
                            elapsed_ms,
                            success: false,
                            failure_class: Some(SoakFailureClass::NonOkStatus(
                                status.code(),
                                status.message().to_string(),
                            )),
                            details,
                        });
                    }
                    Ok(Ok(resp)) => {
                        let inner = resp.into_inner();
                        match assert_payload_len(&inner, cfg.response_size) {
                            Err(val_err) => {
                            let details = format!("payload validation: {val_err}");
                            eprintln!(
                                "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                            );
                            results.push(WorkerIterationResult {
                                iteration,
                                thread_id,
                                elapsed_ms,
                                success: false,
                                failure_class: Some(SoakFailureClass::PayloadMismatch(
                                    val_err.message().to_string(),
                                )),
                                details,
                            });
                            }
                            Ok(()) if elapsed_ms > cfg.per_rpc_timeout_ms => {
                            let details = format!(
                                "elapsed {elapsed_ms}ms exceeds budget {}ms",
                                cfg.per_rpc_timeout_ms
                            );
                            eprintln!(
                                "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                            );
                            results.push(WorkerIterationResult {
                                iteration,
                                thread_id,
                                elapsed_ms,
                                success: false,
                                failure_class: Some(SoakFailureClass::LatencyBudgetExceeded {
                                    elapsed_ms,
                                    budget_ms: cfg.per_rpc_timeout_ms,
                                }),
                                details,
                            });
                            }
                            Ok(()) => {
                            eprintln!(
                                "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} succeeded"
                            );
                            results.push(WorkerIterationResult {
                                iteration,
                                thread_id,
                                elapsed_ms,
                                success: true,
                                failure_class: None,
                                details: String::new(),
                            });
                            }
                        }
                    }
                }

                if cfg.min_time_ms_between_rpcs > 0 {
                    if let Some(wait) =
                        earliest_next_start.checked_duration_since(Instant::now())
                    {
                        tokio::time::sleep(wait).await;
                    }
                }
            }
            results
        });
    }

    let mut all_results = Vec::with_capacity(config.soak_iterations);
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(worker_results) => all_results.extend(worker_results),
            Err(e) => {
                return Err(Status::internal(format!("soak worker task failed: {e}")));
            }
        }
    }

    let summary = build_soak_summary(
        "rpc_soak",
        server_uri,
        config,
        all_results,
        start_total.elapsed(),
        1,
        1,
    );

    evaluate_soak_summary(summary, config)
}

/// The official `channel_soak` test case using a custom channel factory.
///
/// In `channel_soak`, each RPC iteration creates a new channel before the call
/// and drops it immediately after. Channel creation time is included in the
/// latency measurement, but teardown time is excluded.
pub async fn channel_soak_with<F, Fut>(
    factory: F,
    config: &SoakConfig,
) -> Result<SoakSummary, Status>
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<TestServiceClient, Status>> + Send + 'static,
{
    config.validate()?;
    let iters_per_thread = config.soak_iterations / config.soak_num_threads;
    let start_total = Instant::now();
    let deadline = start_total + Duration::from_secs(config.overall_timeout_seconds);

    let mut join_set = tokio::task::JoinSet::new();

    for thread_id in 0..config.soak_num_threads {
        let factory_clone = factory.clone();
        let cfg = config.clone();
        join_set.spawn(async move {
            let mut results = Vec::with_capacity(iters_per_thread);
            let mut channels_created = 0usize;
            let mut channels_dropped = 0usize;
            let mut detected_uri = String::new();

            let mut req = SimpleRequest::new();
            req.set_response_size(cfg.response_size);
            req.set_payload(zeros(cfg.request_size));

            for iteration in 0..iters_per_thread {
                if Instant::now() >= deadline {
                    break;
                }
                let earliest_next_start = Instant::now()
                    + Duration::from_millis(cfg.min_time_ms_between_rpcs);

                let rpc_start = Instant::now();
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }

                // Channel creation is INCLUDED in the latency measurement.
                let conn_fut = factory_clone();
                let conn_res = tokio::time::timeout(remaining, conn_fut).await;

                match conn_res {
                    Err(_timeout) => {
                        let elapsed_ms =
                            u64::try_from(rpc_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                        eprintln!(
                            "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: unknown server_uri: unknown failed: deadline expired during channel connect"
                        );
                        results.push(WorkerIterationResult {
                            iteration,
                            thread_id,
                            elapsed_ms,
                            success: false,
                            failure_class: Some(SoakFailureClass::Omitted),
                            details: "deadline expired during channel connect".to_string(),
                        });
                        break;
                    }
                    Ok(Err(status)) => {
                        let elapsed_ms =
                            u64::try_from(rpc_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                        let details = format!("channel creation failed: {}", status.message());
                        eprintln!(
                            "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: unknown server_uri: unknown failed: {details}"
                        );
                        results.push(WorkerIterationResult {
                            iteration,
                            thread_id,
                            elapsed_ms,
                            success: false,
                            failure_class: Some(SoakFailureClass::ChannelError(
                                status.message().to_string(),
                            )),
                            details,
                        });
                    }
                    Ok(Ok(client)) => {
                        channels_created += 1;
                        if detected_uri.is_empty() {
                            detected_uri = client.channel().authority().to_string();
                        }
                        let uri = detected_uri.clone();

                        let call_remaining = deadline.saturating_duration_since(Instant::now());
                        let call_fut = client.unary_call(Request::new(req.clone()));
                        let rpc_res = tokio::time::timeout(call_remaining, call_fut).await;

                        let elapsed = rpc_start.elapsed();
                        let elapsed_ms =
                            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);

                        // Channel teardown is OUTSIDE the latency measurement.
                        drop(client);
                        channels_dropped += 1;

                        match rpc_res {
                            Err(_timeout) => {
                                eprintln!(
                                    "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: overall deadline expired"
                                );
                                results.push(WorkerIterationResult {
                                    iteration,
                                    thread_id,
                                    elapsed_ms,
                                    success: false,
                                    failure_class: Some(SoakFailureClass::Omitted),
                                    details: "overall deadline expired".to_string(),
                                });
                                break;
                            }
                            Ok(Err(status)) => {
                                let details = format!(
                                    "status {} ({})",
                                    status.code(),
                                    status.message()
                                );
                                eprintln!(
                                    "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                                );
                                results.push(WorkerIterationResult {
                                    iteration,
                                    thread_id,
                                    elapsed_ms,
                                    success: false,
                                    failure_class: Some(SoakFailureClass::NonOkStatus(
                                        status.code(),
                                        status.message().to_string(),
                                    )),
                                    details,
                                });
                            }
                            Ok(Ok(resp)) => {
                                let inner = resp.into_inner();
                                match assert_payload_len(&inner, cfg.response_size) {
                                    Err(val_err) => {
                                    let details = format!("payload validation: {val_err}");
                                    eprintln!(
                                        "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                                    );
                                    results.push(WorkerIterationResult {
                                        iteration,
                                        thread_id,
                                        elapsed_ms,
                                        success: false,
                                        failure_class: Some(SoakFailureClass::PayloadMismatch(
                                            val_err.message().to_string(),
                                        )),
                                        details,
                                    });
                                    }
                                    Ok(()) if elapsed_ms > cfg.per_rpc_timeout_ms => {
                                    let details = format!(
                                        "elapsed {elapsed_ms}ms exceeds budget {}ms",
                                        cfg.per_rpc_timeout_ms
                                    );
                                    eprintln!(
                                        "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} failed: {details}"
                                    );
                                    results.push(WorkerIterationResult {
                                        iteration,
                                        thread_id,
                                        elapsed_ms,
                                        success: false,
                                        failure_class: Some(
                                            SoakFailureClass::LatencyBudgetExceeded {
                                                elapsed_ms,
                                                budget_ms: cfg.per_rpc_timeout_ms,
                                            },
                                        ),
                                        details,
                                    });
                                    }
                                    Ok(()) => {
                                    eprintln!(
                                        "thread_id: {thread_id} soak iteration: {iteration} elapsed_ms: {elapsed_ms} peer: {uri} server_uri: {uri} succeeded"
                                    );
                                    results.push(WorkerIterationResult {
                                        iteration,
                                        thread_id,
                                        elapsed_ms,
                                        success: true,
                                        failure_class: None,
                                        details: String::new(),
                                    });
                                    }
                                }
                            }
                        }
                    }
                }

                if cfg.min_time_ms_between_rpcs > 0 {
                    if let Some(wait) =
                        earliest_next_start.checked_duration_since(Instant::now())
                    {
                        tokio::time::sleep(wait).await;
                    }
                }
            }
            (results, channels_created, channels_dropped, detected_uri)
        });
    }

    let mut all_results = Vec::with_capacity(config.soak_iterations);
    let mut total_created = 0usize;
    let mut total_dropped = 0usize;
    let mut server_uri = String::from("unknown");

    while let Some(res) = join_set.join_next().await {
        match res {
            Ok((worker_results, created, dropped, uri)) => {
                all_results.extend(worker_results);
                total_created += created;
                total_dropped += dropped;
                if !uri.is_empty() && (server_uri == "unknown" || server_uri.is_empty()) {
                    server_uri = uri;
                }
            }
            Err(e) => {
                return Err(Status::internal(format!("soak worker task failed: {e}")));
            }
        }
    }

    let summary = build_soak_summary(
        "channel_soak",
        server_uri,
        config,
        all_results,
        start_total.elapsed(),
        total_created,
        total_dropped,
    );

    evaluate_soak_summary(summary, config)
}

/// The official `channel_soak` test case.
pub async fn channel_soak(
    client: &TestServiceClient,
    config: &SoakConfig,
) -> Result<SoakSummary, Status> {
    let authority = client.channel().authority().to_string();
    channel_soak_with(
        move || {
            let auth = authority.clone();
            async move {
                let ch = crate::Channel::connect(auth).await?;
                Ok(TestServiceClient::new(ch))
            }
        },
        config,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn compressed_unary_probes_cover_distinct_payload_entropy() {
        let low = zeros(LARGE_REQ);
        let high = incompressible(LARGE_REQ).expect("positive probe length");
        assert!(incompressible(-1).is_err());
        assert_eq!(low.body().len(), high.body().len());

        let gzip_size = |body: &[u8]| {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            gz.write_all(body).expect("compress probe");
            gz.finish().expect("finish probe").len()
        };
        assert!(
            gzip_size(low.body().as_ref()) < low.body().len() / 10,
            "zero payload must compress substantially"
        );
        assert!(
            gzip_size(high.body().as_ref()) > high.body().len() * 9 / 10,
            "high-entropy payload must not collapse like zeros"
        );
    }

    #[test]
    fn test_soak_config_validation() {
        let mut cfg = SoakConfig::default();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.run_type(), "smoke");
        assert!(!cfg.is_qualification());

        // Zero iterations
        cfg.soak_iterations = 0;
        assert!(cfg.validate().is_err());

        // Zero threads
        cfg.soak_iterations = 10;
        cfg.soak_num_threads = 0;
        assert!(cfg.validate().is_err());

        // Indivisible iterations and threads
        cfg.soak_num_threads = 3;
        assert!(cfg.validate().is_err());

        // Divisible
        cfg.soak_num_threads = 2;
        assert!(cfg.validate().is_ok());

        // Qualification labeling enforcement
        cfg.qualification_mode = Some(true);
        assert!(
            cfg.validate().is_err(),
            "cannot mislabel <1000 as qualification"
        );

        cfg.soak_iterations = 1000;
        cfg.soak_num_threads = 1;
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.run_type(), "qualification_soak");
        assert!(cfg.is_qualification());
    }

    #[test]
    fn test_latency_stats() {
        let stats = LatencyStats::compute(vec![]);
        assert_eq!(stats.min_ms, 0);
        assert_eq!(stats.p50_ms, 0);

        let samples: Vec<u64> = (1..=100).collect();
        let stats = LatencyStats::compute(samples);
        assert_eq!(stats.min_ms, 1);
        assert_eq!(stats.p50_ms, 50);
        assert_eq!(stats.p90_ms, 90);
        assert_eq!(stats.p99_ms, 99);
        assert_eq!(stats.max_ms, 100);
    }

    #[test]
    fn test_failure_breakdown_total() {
        let fb = FailureBreakdown {
            non_ok_status: 2,
            latency_budget_exceeded: 3,
            payload_mismatch: 1,
            channel_error: 1,
            omitted_iterations: 4,
        };
        assert_eq!(fb.total(), 11);
    }

    #[test]
    fn test_soak_summary_counts_started_timeout_as_omitted() {
        let config = SoakConfig {
            soak_iterations: 3,
            ..SoakConfig::default()
        };
        let results = vec![
            WorkerIterationResult {
                iteration: 0,
                thread_id: 0,
                elapsed_ms: 1,
                success: true,
                failure_class: None,
                details: String::new(),
            },
            WorkerIterationResult {
                iteration: 1,
                thread_id: 0,
                elapsed_ms: 5,
                success: false,
                failure_class: Some(SoakFailureClass::Omitted),
                details: "overall deadline expired".to_string(),
            },
        ];
        let summary = build_soak_summary(
            "rpc_soak",
            "localhost".to_string(),
            &config,
            results,
            Duration::from_millis(6),
            1,
            1,
        );
        assert_eq!(summary.iterations_completed, 1);
        assert_eq!(summary.iterations_omitted, 2);
        assert_eq!(summary.iterations_succeeded, 1);
        assert_eq!(summary.failure_breakdown.omitted_iterations, 2);
        assert_eq!(summary.total_failures, 2);
        assert_eq!(summary.failures.len(), 2);
        assert_eq!(summary.failures[0].iteration, 1);
        assert_eq!(summary.failures[1].iteration, 2);
    }

    use crate::Response;
    use crate::testing::{InteropTestService, SimpleResponse, TestService, TestServiceServer};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    struct TestServerGuard(JoinHandle<()>);

    impl Drop for TestServerGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn start_server<S: TestService>(service: S) -> (SocketAddr, TestServerGuard) {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            TestServiceServer::new(service)
                .serve_listener(listener)
                .await
                .ok();
        });
        (addr, TestServerGuard(handle))
    }

    #[tokio::test]
    async fn test_rpc_soak_happy_path() {
        let (addr, _guard) = start_server(InteropTestService).await;
        let client = connect(addr).await.expect("connect");
        let config = SoakConfig {
            soak_iterations: 4,
            max_failures: 0,
            per_rpc_timeout_ms: 5000,
            overall_timeout_seconds: 10,
            min_time_ms_between_rpcs: 0,
            soak_num_threads: 2,
            request_size: 100,
            response_size: 100,
            qualification_mode: None,
        };
        let summary = rpc_soak(&client, &config).await.expect("rpc_soak");
        assert_eq!(summary.run_type, "smoke");
        assert_eq!(summary.iterations_requested, 4);
        assert_eq!(summary.iterations_completed, 4);
        assert_eq!(summary.iterations_succeeded, 4);
        assert_eq!(summary.total_failures, 0);
        assert_eq!(summary.channels_created, 1);
        assert_eq!(summary.channels_dropped, 1);
        assert_eq!(summary.thread_count, 2);
    }

    #[tokio::test]
    async fn test_channel_soak_creates_and_drops_channels() {
        let (addr, _guard) = start_server(InteropTestService).await;
        let client = connect(addr).await.expect("connect");
        let config = SoakConfig {
            soak_iterations: 4,
            max_failures: 0,
            per_rpc_timeout_ms: 5000,
            overall_timeout_seconds: 10,
            min_time_ms_between_rpcs: 0,
            soak_num_threads: 1,
            request_size: 100,
            response_size: 100,
            qualification_mode: None,
        };
        let summary = channel_soak(&client, &config).await.expect("channel_soak");
        assert_eq!(summary.run_type, "smoke");
        assert_eq!(summary.iterations_requested, 4);
        assert_eq!(summary.iterations_completed, 4);
        assert_eq!(summary.iterations_succeeded, 4);
        assert_eq!(summary.total_failures, 0);
        assert_eq!(summary.channels_created, 4);
        assert_eq!(summary.channels_dropped, 4);
    }

    struct SlowService;
    impl TestService for SlowService {
        async fn unary_call(
            &self,
            request: Request<SimpleRequest>,
        ) -> Result<Response<SimpleResponse>, Status> {
            tokio::time::sleep(Duration::from_millis(5)).await;
            let size = request.into_inner().response_size();
            let mut resp = Response::new(SimpleResponse::new());
            let mut p = Payload::new();
            p.set_body(vec![0u8; usize::try_from(size.max(0)).unwrap_or(0)]);
            resp.get_mut().set_payload(p);
            Ok(resp)
        }
    }

    #[tokio::test]
    async fn test_rpc_soak_detects_exceeded_latency_budget() {
        let (addr, _guard) = start_server(SlowService).await;
        let client = connect(addr).await.expect("connect");
        let config = SoakConfig {
            soak_iterations: 4,
            max_failures: 0,
            per_rpc_timeout_ms: 1, // 1ms budget: SlowService sleeps 5ms, so all iterations exceed budget
            overall_timeout_seconds: 10,
            min_time_ms_between_rpcs: 0,
            soak_num_threads: 1,
            request_size: 100,
            response_size: 100,
            qualification_mode: None,
        };
        let err = rpc_soak(&client, &config)
            .await
            .expect_err("should exceed budget");
        assert_eq!(err.code(), Code::Internal);
        assert!(err.message().contains("latency_budget_exceeded"));

        // If max_failures budget covers it, the call succeeds but records the failures:
        let mut lenient_config = config.clone();
        lenient_config.max_failures = 4;
        let summary = rpc_soak(&client, &lenient_config)
            .await
            .expect("should pass under budget");
        assert_eq!(summary.total_failures, 4);
        assert_eq!(summary.failure_breakdown.latency_budget_exceeded, 4);
        assert_eq!(summary.iterations_succeeded, 0);
    }

    struct FailingService;
    impl TestService for FailingService {
        async fn unary_call(
            &self,
            _request: Request<SimpleRequest>,
        ) -> Result<Response<SimpleResponse>, Status> {
            Err(Status::internal("simulated failure"))
        }
    }

    #[tokio::test]
    async fn test_rpc_soak_detects_failed_calls() {
        let (addr, _guard) = start_server(FailingService).await;
        let client = connect(addr).await.expect("connect");
        let config = SoakConfig {
            soak_iterations: 4,
            max_failures: 0,
            per_rpc_timeout_ms: 5000,
            overall_timeout_seconds: 10,
            min_time_ms_between_rpcs: 0,
            soak_num_threads: 1,
            request_size: 100,
            response_size: 100,
            qualification_mode: None,
        };
        let err = rpc_soak(&client, &config).await.expect_err("should fail");
        assert_eq!(err.code(), Code::Internal);
        assert!(err.message().contains("non_ok_status"));
    }

    #[tokio::test]
    async fn test_rpc_soak_detects_omitted_iterations() {
        let (addr, _guard) = start_server(InteropTestService).await;
        let client = connect(addr).await.expect("connect");
        let config = SoakConfig {
            soak_iterations: 100,
            max_failures: 0,
            per_rpc_timeout_ms: 5000,
            overall_timeout_seconds: 1,
            min_time_ms_between_rpcs: 200, // 200ms sleep * 100 = 20s, will time out after 1s
            soak_num_threads: 1,
            request_size: 100,
            response_size: 100,
            qualification_mode: None,
        };
        let err = rpc_soak(&client, &config)
            .await
            .expect_err("should time out");
        assert_eq!(err.code(), Code::DeadlineExceeded);
        assert!(err.message().contains("omitted"));
    }
}
