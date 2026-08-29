//! Official `--test_case` procedures driven through the shipped kernel client.

use crate::status::{Code, Status};
use crate::stream::Framed;
use crate::testing::{
    BoolValue, Empty, Payload, SimpleRequest, StreamingInputCallRequest,
    StreamingOutputCallRequest, TestServiceClient, UnimplementedServiceClient,
};
use crate::{Request, Response};
use std::net::SocketAddr;
use std::time::Duration;

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

fn bool_val(v: bool) -> BoolValue {
    let mut b = BoolValue::new();
    b.set_value(v);
    b
}

fn assert_payload_len(resp: &crate::testing::SimpleResponse, n: i32) -> Result<(), Status> {
    let got = i32::try_from(resp.payload().body().len()).unwrap_or(i32::MAX);
    if got != n {
        return Err(Status::internal(format!("payload len {got} want {n}")));
    }
    Ok(())
}

/// Run one official uncompressed or compressed `_TEST_CASES` name.
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
        other => Err(Status::invalid_argument(format!(
            "unknown test_case {other}"
        ))),
    }
}

async fn empty_unary(client: &TestServiceClient) -> Result<(), Status> {
    let _ = client.empty_call(Request::new(Empty::new())).await?;
    Ok(())
}

async fn large_unary(client: &TestServiceClient) -> Result<(), Status> {
    let mut req = SimpleRequest::new();
    req.set_response_size(LARGE_RESP);
    req.set_payload(zeros(LARGE_REQ));
    let resp = client.unary_call(Request::new(req)).await?;
    assert_payload_len(&resp.into_inner(), LARGE_RESP)
}

async fn client_streaming(client: &TestServiceClient) -> Result<(), Status> {
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
        return Err(Status::internal(format!("agg {got} want 74922")));
    }
    Ok(())
}

async fn server_streaming(client: &TestServiceClient) -> Result<(), Status> {
    let mut req = StreamingOutputCallRequest::new();
    for n in [31415, 9, 2653, 58979] {
        let mut p = crate::testing::ResponseParameters::new();
        p.set_size(n);
        req.response_parameters_mut().push(p);
    }
    let resp = client.streaming_output_call(Request::new(req)).await?;
    let mut inbound = resp.into_inner();
    let mut sizes = Vec::new();
    while let Some(m) = inbound.message().await? {
        sizes.push(i32::try_from(m.payload().body().len()).unwrap_or(0));
    }
    if sizes != [31415, 9, 2653, 58979] {
        return Err(Status::internal(format!("sizes {sizes:?}")));
    }
    Ok(())
}

async fn ping_pong(client: &TestServiceClient) -> Result<(), Status> {
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
    let n = i32::try_from(got.payload().body().len()).unwrap_or(0);
    if n != first_resp {
        return Err(Status::internal(format!("ping_pong {n} want {first_resp}")));
    }
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
        let n = i32::try_from(got.payload().body().len()).unwrap_or(0);
        if n != resp_size {
            return Err(Status::internal(format!("ping_pong {n} want {resp_size}")));
        }
    }
    tx.close();
    while inbound.message().await?.is_some() {}
    Ok(())
}

async fn empty_stream(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.close();
    let resp = call.await?;
    let mut inbound = resp.into_inner();
    if inbound.message().await?.is_some() {
        return Err(Status::internal("empty_stream got a message"));
    }
    Ok(())
}

async fn cancel_after_begin(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.streaming_input_call(Request::new(()));
    let handle = call.handle();
    handle.cancel();
    // Hold `tx` until the call settles. Dropping it is a half-close, which
    // can complete StreamingInputCall as OK before the RST is observed.
    let result = match call.await {
        Err(st) if st.code() == Code::Cancelled => Ok(()),
        Err(st) => Err(Status::internal(format!("want CANCELLED got {st}"))),
        Ok(_) => Err(Status::internal("want CANCELLED got ok")),
    };
    drop(tx);
    result
}

async fn cancel_after_first_response(client: &TestServiceClient) -> Result<(), Status> {
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let handle = call.handle();
    let mut req = StreamingOutputCallRequest::new();
    let mut p = crate::testing::ResponseParameters::new();
    p.set_size(31415);
    req.response_parameters_mut().push(p);
    req.set_payload(zeros(27182));
    tx.send(req).await?;
    match call.await {
        Ok(r) => {
            let mut inbound = r.into_inner();
            inbound
                .message()
                .await?
                .ok_or_else(|| Status::internal("missing first response"))?;
            handle.cancel();
            match inbound.message().await {
                Err(st) if st.code() == Code::Cancelled => Ok(()),
                Ok(None) => Ok(()),
                Ok(Some(_)) => Ok(()),
                Err(st) => Err(st),
            }
        }
        Err(st) if st.code() == Code::Cancelled => Ok(()),
        Err(st) => Err(st),
    }
}

/// The server accepts the stream and then never answers, so the deadline has to
/// fire on the *read*, not just on call setup. The official client asserts the
/// status of the first receive for exactly this reason.
async fn timeout_on_sleeping_server(client: &TestServiceClient) -> Result<(), Status> {
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

fn check_custom_md<T>(resp: &Response<T>) -> Result<(), Status> {
    if resp.metadata().get(INITIAL_MD) != Some(INITIAL_VAL) {
        return Err(Status::internal("missing initial metadata"));
    }
    if resp.metadata().get_bin(TRAILING_MD).is_some() {
        return Err(Status::internal("trailing-bin in headers"));
    }
    if resp.trailers().get_bin(TRAILING_MD).as_deref() != Some(TRAILING_VAL) {
        return Err(Status::internal("missing trailing-bin trailers"));
    }
    Ok(())
}

async fn custom_metadata(client: &TestServiceClient) -> Result<(), Status> {
    let mut sr = SimpleRequest::new();
    sr.set_response_size(LARGE_RESP);
    sr.set_payload(zeros(LARGE_REQ));
    let mut req = Request::new(sr);
    attach_custom_md(&mut req);
    let resp = client.unary_call(req).await?;
    check_custom_md(&resp)?;
    drop(resp);

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
    let mut inbound = resp.into_inner();
    while inbound.message().await?.is_some() {}
    let trailers = inbound.trailers().await?;
    if trailers.get_bin(TRAILING_MD).as_deref() != Some(TRAILING_VAL) {
        return Err(Status::internal("missing trailing-bin trailers"));
    }
    Ok(())
}

async fn status_code_and_message(client: &TestServiceClient) -> Result<(), Status> {
    let want = "test status message";
    let mut sr = SimpleRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(2);
    es.set_message(want);
    sr.set_response_status(es);
    match client.unary_call(Request::new(sr)).await {
        Err(st) if st.code() == Code::Unknown && st.message() == want => {}
        Ok(_) => return Err(Status::internal("unary status got ok")),
        Err(st) => return Err(Status::internal(format!("unary status {st}"))),
    }
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let mut m = StreamingOutputCallRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(2);
    es.set_message(want);
    m.set_response_status(es);
    tx.send(m).await?;
    tx.close();
    match call.await {
        Err(st) if st.code() == Code::Unknown && st.message() == want => Ok(()),
        Ok(resp) => {
            let mut inbound = resp.into_inner();
            match inbound.message().await {
                Err(st) if st.code() == Code::Unknown && st.message() == want => Ok(()),
                Ok(Some(_)) => Err(Status::internal("duplex status extra message")),
                Ok(None) => Err(Status::internal("duplex status missing")),
                Err(st) => Err(Status::internal(format!("duplex status {st}"))),
            }
        }
        Err(st) => Err(Status::internal(format!("duplex status {st}"))),
    }
}

async fn special_status_message(client: &TestServiceClient) -> Result<(), Status> {
    let want = "\t\ntest with whitespace\r\nand Unicode BMP ☺ and non-BMP 😈\t\n";
    let mut sr = SimpleRequest::new();
    let mut es = crate::testing::EchoStatus::new();
    es.set_code(2);
    es.set_message(want);
    sr.set_response_status(es);
    match client.unary_call(Request::new(sr)).await {
        Err(st) if st.code() == Code::Unknown && st.message() == want => Ok(()),
        Err(st) => Err(Status::internal(format!(
            "special message mismatch: {:?}",
            st.message()
        ))),
        Ok(_) => Err(Status::internal("want status")),
    }
}

/// A method the service declares but the server refuses.
async fn unimplemented_method(client: &TestServiceClient) -> Result<(), Status> {
    expect_unimplemented(client.unimplemented_call(Request::new(Empty::new())).await)
}

/// A service the server does not host at all, so the router rejects the path.
async fn unimplemented_service(client: &TestServiceClient) -> Result<(), Status> {
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

async fn client_compressed_unary(client: &TestServiceClient) -> Result<(), Status> {
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
    assert_payload_len(&resp.into_inner(), LARGE_RESP)
}

async fn server_compressed_unary(client: &TestServiceClient) -> Result<(), Status> {
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
        assert_payload_len(&resp.into_inner(), LARGE_RESP)?;
    }
    Ok(())
}

async fn client_compressed_streaming(client: &TestServiceClient) -> Result<(), Status> {
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
    let (tx, call) = client.streaming_input_call(Request::new(()));
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

async fn server_compressed_streaming(client: &TestServiceClient) -> Result<(), Status> {
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
    let s0 = i32::try_from(first.message.payload().body().len()).unwrap_or(0);
    let s1 = i32::try_from(second.message.payload().body().len()).unwrap_or(0);
    if s0 != 31415 || s1 != 92653 {
        return Err(Status::internal(format!("sizes {s0} {s1}")));
    }
    if !first.compressed || second.compressed {
        return Err(Status::internal("compressed flags"));
    }
    Ok(())
}

/// Connect helper.
pub async fn connect(addr: SocketAddr) -> Result<TestServiceClient, Status> {
    let ch = crate::Channel::connect(addr).await?;
    Ok(TestServiceClient::new(ch))
}
