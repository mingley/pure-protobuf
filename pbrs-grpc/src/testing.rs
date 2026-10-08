//! Generated `grpc.testing` types and interoperability handlers.
//!
//! [`TestServiceClient`] and [`TestServiceServer`] provide the official test
//! service's RPC shapes. [`InteropTestService`] implements its payload,
//! streaming, metadata, compression, and error cases.
//!
//! These stubs use the same channel, server, interceptor, and status APIs as
//! application services. See [`crate::Outgoing`], [`crate::ResponseParts`],
//! and [`crate::Status`] for their contracts.

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/test.rs"));

use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::{StreamSender, Streaming};
use std::time::Duration;

/// Metadata the interop suite requires a server to echo back.
const ECHO_INITIAL: &str = "x-grpc-test-echo-initial";
const ECHO_TRAILING: &str = "x-grpc-test-echo-trailing-bin";

/// The echo headers lifted off a request, so a response can carry them without
/// keeping the request alive.
struct Echo {
    initial: Option<String>,
    trailing: Option<Vec<u8>>,
}

impl Echo {
    fn capture<T>(request: &Request<T>) -> Self {
        Self {
            initial: request.metadata().get(ECHO_INITIAL).map(str::to_owned),
            trailing: request.metadata().get_bin(ECHO_TRAILING),
        }
    }

    fn apply<T>(&self, response: &mut Response<T>) {
        if let Some(v) = &self.initial {
            response.metadata_mut().insert(ECHO_INITIAL, v).ok();
        }
        if let Some(v) = &self.trailing {
            response.trailers_mut().insert_bin(ECHO_TRAILING, v).ok();
        }
    }
}

/// The reference [`TestService`] implementation used by the interop suite.
///
/// Official uncompressed `_TEST_CASES` and the four gzip cases pass against
/// this server over TLS, mTLS, Unix, and [`crate::Server::serve_connection`].
/// `UnaryCall` also honors `orca_per_query_report`, stamping the bounded
/// per-call report the official `orca_per_rpc` procedure checks.
/// This local demo caps each generated response `Payload.body` at the default
/// 4 MiB decoded-message budget. A serialized `SimpleResponse` or
/// `StreamingOutputCallResponse` adds protobuf envelope bytes, so callers
/// requesting the full body cap may need a higher inbound message limit.
/// This is a sample-service allocation policy, not an upstream interop limit.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService interceptor Err; those trailers reach the client without reading the body.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService handler Err; those trailers reach the client.
/// [`crate::Outgoing::connected`] is the live-socket snapshot on this InteropTestService client interceptor path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService client interceptor Err; a local reject never opens a stream.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
#[derive(Default)]
pub struct InteropTestService;

const MAX_INTEROP_RESPONSE_BODY_SIZE: usize = crate::DEFAULT_MAX_DECODING_MESSAGE_SIZE;

fn checked_response_body_size(cap: usize, size: i32) -> Result<usize, Status> {
    let n = usize::try_from(size)
        .map_err(|_| Status::invalid_argument("negative TestService response size"))?;
    if n > cap {
        return Err(Status::resource_exhausted(format!(
            "TestService response body exceeds the {cap}-byte cap"
        )));
    }
    Ok(n)
}

fn zeros_payload(cap: usize, n: i32) -> Result<Payload, Status> {
    let n = checked_response_body_size(cap, n)?;
    let mut p = Payload::new();
    p.set_body(vec![0u8; n]);
    Ok(p)
}

fn checked_input_total(total: i32, body_len: usize) -> Result<i32, Status> {
    let n = i32::try_from(body_len)
        .map_err(|_| Status::resource_exhausted("TestService streaming input total exceeds i32"))?;
    total
        .checked_add(n)
        .ok_or_else(|| Status::resource_exhausted("TestService streaming input total exceeds i32"))
}

/// `(size, interval_us, compressed)` for each response the client asked for.
type ResponsePlan = Vec<(i32, i32, bool)>;

fn response_plan(req: &StreamingOutputCallRequest) -> ResponsePlan {
    req.response_parameters()
        .iter()
        .map(|p| {
            (
                p.size(),
                p.interval_us(),
                p.has_compressed() && p.compressed().value(),
            )
        })
        .collect()
}

/// Emit planned responses. `Ok(false)` means the peer went away; invalid sizes return a status.
async fn emit_plan(
    tx: &StreamSender<StreamingOutputCallResponse>,
    plan: ResponsePlan,
    cap: usize,
) -> Result<bool, Status> {
    for (size, interval_us, compress) in plan {
        let payload = zeros_payload(cap, size)?;
        if interval_us > 0 {
            let us = u64::try_from(interval_us).unwrap_or(0);
            tokio::time::sleep(Duration::from_micros(us)).await;
        }
        let mut msg = StreamingOutputCallResponse::new();
        msg.set_payload(payload);
        let sent = if compress {
            tx.send_compressed(msg).await
        } else {
            tx.send(msg).await
        };
        if sent.is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn echoed_status(code: i32, message: impl Into<String>) -> Status {
    Status::new(Code::from_i32(code), message)
}

async fn unary_reply(
    echo: &Echo,
    request: SimpleRequest,
    compressed: bool,
    cap: usize,
) -> Result<Response<SimpleResponse>, Status> {
    if request.has_expect_compressed() && request.expect_compressed().value() && !compressed {
        return Err(Status::invalid_argument("request not compressed"));
    }
    if request.has_response_status() {
        let st = request.response_status();
        return Err(echoed_status(st.code(), st.message().to_string()));
    }
    let mut msg = SimpleResponse::new();
    msg.set_payload(zeros_payload(cap, request.response_size())?);
    let mut resp = Response::new(msg);
    echo.apply(&mut resp);
    if request.has_response_compressed() && request.response_compressed().value() {
        resp.set_compress(true);
    }
    Ok(resp)
}

/// Shared handler bodies parameterized by the response-body cap.
/// [`InteropTestService`] passes the 4 MiB demo cap;
/// [`SizedInteropTestService`] passes its configured cap.
async fn unary_call_impl(
    cap: usize,
    request: Request<SimpleRequest>,
) -> Result<Response<SimpleResponse>, Status> {
    let echo = Echo::capture(&request);
    let compressed = request.compressed();
    // Cloned before `into_inner` moves the request: a `response_status`
    // echo still wins over an invalid ORCA field, matching the pinned
    // peers (the Go server checks the echo first).
    let orca = request.get_ref().orca_per_query_report_opt().cloned();
    let mut response = unary_reply(&echo, request.into_inner(), compressed, cap).await?;
    if let Some(data) = orca {
        let report = crate::orca::per_query_report(&data)?;
        let bytes = crate::orca::encode_trailer(&report)?;
        response
            .trailers_mut()
            .insert_bin(crate::orca::TRAILER, bytes)?;
    }
    Ok(response)
}

async fn streaming_output_call_impl(
    cap: usize,
    request: Request<StreamingOutputCallRequest>,
) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
    let echo = Echo::capture(&request);
    let inner = request.into_inner();
    if inner.has_response_status() {
        let st = inner.response_status();
        return Err(echoed_status(st.code(), st.message().to_string()));
    }
    let plan = response_plan(&inner);
    if let Some((size, _, _)) = plan.first() {
        checked_response_body_size(cap, *size)?;
    }
    let want_gzip = plan.iter().any(|(_, _, compress)| *compress);
    let (tx, stream) = Streaming::channel(8);
    drop(tokio::spawn(async move {
        if let Err(status) = emit_plan(&tx, plan, cap).await {
            tx.fail(status).await;
        }
    }));
    let mut resp = Response::new(stream);
    echo.apply(&mut resp);
    if want_gzip {
        resp.set_compress(true);
    }
    Ok(resp)
}

async fn streaming_input_call_impl(
    request: Request<Streaming<StreamingInputCallRequest>>,
) -> Result<Response<StreamingInputCallResponse>, Status> {
    let echo = Echo::capture(&request);
    let mut stream = request.into_inner();
    let mut total: i32 = 0;
    while let Some(item) = stream.next_framed().await? {
        if item.message.has_expect_compressed()
            && item.message.expect_compressed().value()
            && !item.compressed
        {
            return Err(Status::invalid_argument("request not compressed"));
        }
        let n = item.message.payload().body().len();
        total = checked_input_total(total, n)?;
    }
    let mut msg = StreamingInputCallResponse::new();
    msg.set_aggregated_payload_size(total);
    let mut resp = Response::new(msg);
    echo.apply(&mut resp);
    Ok(resp)
}

async fn full_duplex_call_impl(
    cap: usize,
    request: Request<Streaming<StreamingOutputCallRequest>>,
) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
    let echo = Echo::capture(&request);
    let mut inbound = request.into_inner();
    let (tx, stream) = Streaming::channel(8);
    drop(tokio::spawn(async move {
        loop {
            match inbound.message().await {
                Ok(Some(req)) => {
                    if req.has_response_status() {
                        let st = req.response_status();
                        tx.fail(echoed_status(st.code(), st.message().to_string()))
                            .await;
                        return;
                    }
                    match emit_plan(&tx, response_plan(&req), cap).await {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(status) => {
                            tx.fail(status).await;
                            return;
                        }
                    }
                }
                Ok(None) => return,
                Err(status) => {
                    tx.fail(status).await;
                    return;
                }
            }
        }
    }));
    let mut resp = Response::new(stream);
    echo.apply(&mut resp);
    Ok(resp)
}

async fn empty_call_impl(request: Request<Empty>) -> Result<Response<Empty>, Status> {
    let echo = Echo::capture(&request);
    let mut resp = Response::new(Empty::new());
    echo.apply(&mut resp);
    Ok(resp)
}

impl TestService for InteropTestService {
    async fn empty_call(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
        empty_call_impl(request).await
    }

    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        unary_call_impl(MAX_INTEROP_RESPONSE_BODY_SIZE, request).await
    }

    /// Identical to `UnaryCall`; the interop suite only cares that it answers.
    async fn cacheable_unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        unary_call_impl(MAX_INTEROP_RESPONSE_BODY_SIZE, request).await
    }

    async fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        streaming_output_call_impl(MAX_INTEROP_RESPONSE_BODY_SIZE, request).await
    }

    async fn streaming_input_call(
        &self,
        request: Request<Streaming<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        streaming_input_call_impl(request).await
    }

    async fn full_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        full_duplex_call_impl(MAX_INTEROP_RESPONSE_BODY_SIZE, request).await
    }

    async fn half_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        self.full_duplex_call(request).await
    }

    /// The interop suite requires this to answer `UNIMPLEMENTED`.
    async fn unimplemented_call(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented(
            "grpc.testing.TestService/UnimplementedCall",
        ))
    }
}

/// [`InteropTestService`] with a configurable generated-response cap.
///
/// Benchmarks that need responses above the 4 MiB demo cap (SB-13 large
/// payloads) mount this instead of raising the shared demo default.
/// Semantics are identical; only the cap differs.
#[derive(Debug, Clone, Copy)]
pub struct SizedInteropTestService {
    max_response_body_size: usize,
}

impl SizedInteropTestService {
    /// Serve generated response bodies up to `max_response_body_size`
    /// bytes. Requests above the cap fail `RESOURCE_EXHAUSTED`, exactly
    /// like the demo default.
    pub fn new(max_response_body_size: usize) -> Self {
        Self {
            max_response_body_size,
        }
    }
}

impl TestService for SizedInteropTestService {
    async fn empty_call(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
        empty_call_impl(request).await
    }

    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        unary_call_impl(self.max_response_body_size, request).await
    }

    async fn cacheable_unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        unary_call_impl(self.max_response_body_size, request).await
    }

    async fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        streaming_output_call_impl(self.max_response_body_size, request).await
    }

    async fn streaming_input_call(
        &self,
        request: Request<Streaming<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        streaming_input_call_impl(request).await
    }

    async fn full_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        full_duplex_call_impl(self.max_response_body_size, request).await
    }

    async fn half_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        self.full_duplex_call(request).await
    }

    async fn unimplemented_call(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented(
            "grpc.testing.TestService/UnimplementedCall",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_INTEROP_RESPONSE_BODY_SIZE, SimpleResponse, checked_input_total, zeros_payload,
    };
    use crate::status::Code;

    #[test]
    fn generated_response_body_has_an_explicit_demo_cap() {
        assert_eq!(
            MAX_INTEROP_RESPONSE_BODY_SIZE,
            crate::DEFAULT_MAX_DECODING_MESSAGE_SIZE
        );
        let cap = MAX_INTEROP_RESPONSE_BODY_SIZE;
        assert_eq!(zeros_payload(cap, 0).expect("empty body").body().len(), 0);
        for size in [-1, i32::MIN] {
            assert_eq!(
                zeros_payload(cap, size).expect_err("negative size").code(),
                Code::InvalidArgument
            );
        }
        let max = i32::try_from(MAX_INTEROP_RESPONSE_BODY_SIZE).expect("4 MiB fits i32");
        for size in [max + 1, i32::MAX] {
            assert_eq!(
                zeros_payload(cap, size).expect_err("oversize body").code(),
                Code::ResourceExhausted
            );
        }
        // A raised cap admits what the default rejects.
        assert_eq!(
            zeros_payload(2 * cap, max + 1)
                .expect("raised cap")
                .body()
                .len(),
            MAX_INTEROP_RESPONSE_BODY_SIZE + 1
        );
        let mut response = SimpleResponse::new();
        response.set_payload(zeros_payload(cap, max).expect("body at cap"));
        assert_eq!(
            response.payload().body().len(),
            MAX_INTEROP_RESPONSE_BODY_SIZE
        );
        assert!(
            pbrs::Serialize::serialized_len(&response) > MAX_INTEROP_RESPONSE_BODY_SIZE,
            "the protobuf envelope is additional to the body cap"
        );
    }

    #[test]
    fn streaming_input_aggregation_rejects_overflow_without_saturation() {
        assert_eq!(checked_input_total(7, 11).expect("within range"), 18);
        assert_eq!(
            checked_input_total(i32::MAX - 1, 1).expect("at boundary"),
            i32::MAX
        );
        for length in [2, usize::MAX] {
            let error = checked_input_total(i32::MAX - 1, length)
                .expect_err("unrepresentable aggregate must fail");
            assert_eq!(error.code(), Code::ResourceExhausted);
        }
    }
}
