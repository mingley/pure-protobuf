//! Official `grpc.testing` services and a conformant [`TestService`].
//!
//! The messages, `TestService` / `UnimplementedService` traits, servers, and
//! clients are all generated from `proto/grpc/testing/test.proto`. Only
//! [`InteropTestService`] is written by hand: it is the behaviour the
//! cross-language interop suite checks, so the kernel is verified against Go,
//! Java, and C++ peers rather than only against itself. A [`TestServiceClient`]
//! `message_limits` is `RESOURCE_EXHAUSTED` on UnaryCall /
//! StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
//! TLS, mTLS, Unix, and [`crate::Channel::from_io`]. Distinct from wrapping
//! `max_encoding_message_size` / `max_decoding_message_size`.
//! [`TestServiceClient::connect_tls_with`] /
//! [`TestServiceClient::connect_unix_with`] /
//! [`TestServiceClient::from_io_with`] with
//! [`crate::ChannelConfig::message_limits`] refuse the same oversize, distinct
//! from wrapping a live client. [`TestServiceServer::max_header_list_size`]
//! refuses oversize metadata on EmptyCall / StreamingOutputCall /
//! StreamingInputCall / FullDuplexCall, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server. [`TestServiceServer::max_frame_size`] still serves EmptyCall /
//! StreamingOutputCall / StreamingInputCall / FullDuplexCall at the HTTP/2
//! 16 KiB SETTINGS minimum, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server. [`TestServiceServer::max_pending_accept_reset_streams`] still serves
//! EmptyCall / StreamingOutputCall / StreamingInputCall / FullDuplexCall at a
//! pending-reset cap of 1, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. A well-behaved client never fills that
//! queue. Distinct from wrapping only a Greeter server.
//! [`TestServiceServer::max_send_buffer_size`] still serves EmptyCall /
//! StreamingOutputCall / StreamingInputCall / FullDuplexCall at a 16 KiB send
//! buffer, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server.
//! [`TestServiceServer::initial_stream_window_size`] /
//! [`TestServiceServer::initial_connection_window_size`] still serve EmptyCall /
//! StreamingOutputCall / StreamingInputCall / FullDuplexCall at a 64 KiB stream
//! / 128 KiB connection window, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server.
//! A [`TestServiceClient`] pool larger than
//! [`TestServiceServer::max_concurrent_connections`] fails the whole dial as
//! `UNAVAILABLE` on TLS, mTLS, and Unix. [`TestServiceClient::from_io_with`]
//! cannot pool.
//! [`crate::Status::from_error_details`] is the typed bag after this testing interceptor Err; those trailers reach the client without reading the body.
//! Distinct from a testing handler Err: that is after the handler ran; this testing interceptor Err is trailers without reading the body.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this testing interceptor Err is trailers without reading the body.
//! Distinct from a testing client interceptor Err: that is a local reject never opens a stream; this testing interceptor Err is trailers without reading the body.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this testing interceptor Err is trailers without reading the body.
//! Distinct from a testing StreamSender fail: that is trailers after any messages already sent; this testing interceptor Err is trailers without reading the body.
//! Distinct from a testing client interceptor: that runs on the outbound call before the stream opens; this testing interceptor runs on the inbound RPC before the handler.
//! [`crate::Status::from_error_details`] is the typed bag after this testing handler Err; those trailers reach the client.
//! Distinct from a testing interceptor Err: that is trailers without reading the body; this testing handler Err is after the handler ran.
//! Distinct from a testing client interceptor Err: that is a local reject never opens a stream; this testing handler Err is after the handler ran.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this testing handler Err is after the handler ran.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this testing handler Err is after the handler ran.
//! Distinct from a testing StreamSender fail: that is trailers after any messages already sent; this testing handler Err is after the handler ran.
//! [`crate::Outgoing::connected`] is the live-socket snapshot on this testing client interceptor path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//! [`crate::Status::from_error_details`] is the typed bag after this testing client interceptor Err; a local reject never opens a stream.
//! Distinct from a testing handler Err: that is after the handler ran; this testing client interceptor Err is a local reject never opens a stream.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this testing client interceptor Err is a local reject never opens a stream.
//! Distinct from a testing interceptor Err: that is trailers without reading the body; this testing client interceptor Err is a local reject never opens a stream.
//! Distinct from a testing StreamSender fail: that is trailers after any messages already sent; this testing client interceptor Err is a local reject never opens a stream.
//! Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this testing client interceptor already ran, so a local Err never consumes that budget.
//! Distinct from a testing interceptor: that runs on the inbound RPC before the handler; this testing client interceptor runs on the outbound call before the stream opens.
//! [`crate::Status::from_error_details`] is the typed bag after this testing StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//! Distinct from a testing handler Err: that is after the handler ran; this testing StreamSender fail is trailers after any messages already sent.
//! Distinct from a testing interceptor Err: that is trailers without reading the body; this testing StreamSender fail is trailers after any messages already sent.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this testing StreamSender fail is trailers after any messages already sent.
//! Distinct from a testing client interceptor Err: that is a local reject never opens a stream; this testing StreamSender fail is trailers after any messages already sent.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this testing StreamSender fail is trailers after any messages already sent.
//! [`crate::Status::from_error_details`] is the typed bag after this testing server on_response Err; a local reject is trailers-only after handler Ok.
//! Distinct from a testing handler Err: that is after the handler ran; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from a testing interceptor Err: that is trailers without reading the body; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from an UnimplementedService interceptor Err: that is trailers without reading the body; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from a testing StreamSender fail: that is trailers after any messages already sent; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from an InteropTestService interceptor Err: that is trailers without reading the body; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from an InteropTestService StreamSender fail: that is trailers after any messages already sent; this testing server on_response Err is trailers-only after handler Ok.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this testing server on_response Err is trailers-only after handler Ok.
//! [`crate::Status::from_error_details`] is the typed bag after this testing Channel on_response Err; a local reject fails the Call after a successful receive.
//! Distinct from a testing handler Err: that is after the handler ran; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from a testing interceptor Err: that is trailers without reading the body; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from a testing client interceptor Err: that is a local reject never opens a stream; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from an UnimplementedService client interceptor Err: that is a local reject never opens a stream; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from an UnimplementedService interceptor Err: that is trailers without reading the body; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from a testing StreamSender fail: that is trailers after any messages already sent; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from an InteropTestService client interceptor Err: that is a local reject never opens a stream; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from an InteropTestService interceptor Err: that is trailers without reading the body; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from an InteropTestService StreamSender fail: that is trailers after any messages already sent; this testing Channel on_response Err fails the Call after a successful receive.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this testing Channel on_response Err fails the Call after a successful receive.
//! [`crate::Status::from_error_details`] is the typed bag after this UnimplementedService interceptor Err; those trailers reach the client without reading the body.
//! Distinct from an UnimplementedService handler Err: that is after the handler ran; this UnimplementedService interceptor Err is trailers without reading the body.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this UnimplementedService interceptor Err is trailers without reading the body.
//! Distinct from an UnimplementedService client interceptor Err: that is a local reject never opens a stream; this UnimplementedService interceptor Err is trailers without reading the body.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this UnimplementedService interceptor Err is trailers without reading the body.
//! Distinct from an UnimplementedService client interceptor: that runs on the outbound call before the stream opens; this UnimplementedService interceptor runs on the inbound RPC before the handler.
//! [`crate::Status::from_error_details`] is the typed bag after this UnimplementedService handler Err; those trailers reach the client.
//! Distinct from an UnimplementedService interceptor Err: that is trailers without reading the body; this UnimplementedService handler Err is after the handler ran.
//! Distinct from an UnimplementedService client interceptor Err: that is a local reject never opens a stream; this UnimplementedService handler Err is after the handler ran.
//! Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this UnimplementedService handler Err is after the handler ran.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this UnimplementedService handler Err is after the handler ran.
//! [`crate::Outgoing::connected`] is the live-socket snapshot on this UnimplementedService client interceptor path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//! [`crate::Status::from_error_details`] is the typed bag after this UnimplementedService client interceptor Err; a local reject never opens a stream.
//! Distinct from an UnimplementedService handler Err: that is after the handler ran; this UnimplementedService client interceptor Err is a local reject never opens a stream.
//! Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this UnimplementedService client interceptor Err is a local reject never opens a stream.
//! Distinct from an UnimplementedService interceptor Err: that is trailers without reading the body; this UnimplementedService client interceptor Err is a local reject never opens a stream.
//! Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this UnimplementedService client interceptor already ran, so a local Err never consumes that budget.
//! Distinct from an UnimplementedService interceptor: that runs on the inbound RPC before the handler; this UnimplementedService client interceptor runs on the outbound call before the stream opens.

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
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService interceptor Err; those trailers reach the client without reading the body.
/// Distinct from an InteropTestService handler Err: that is after the handler ran; this InteropTestService interceptor Err is trailers without reading the body.
/// Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this InteropTestService interceptor Err is trailers without reading the body.
/// Distinct from an InteropTestService client interceptor Err: that is a local reject never opens a stream; this InteropTestService interceptor Err is trailers without reading the body.
/// Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this InteropTestService interceptor Err is trailers without reading the body.
/// Distinct from an InteropTestService StreamSender fail: that is trailers after any messages already sent; this InteropTestService interceptor Err is trailers without reading the body.
/// Distinct from an InteropTestService client interceptor: that runs on the outbound call before the stream opens; this InteropTestService interceptor runs on the inbound RPC before the handler.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService handler Err; those trailers reach the client.
/// Distinct from an InteropTestService interceptor Err: that is trailers without reading the body; this InteropTestService handler Err is after the handler ran.
/// Distinct from an InteropTestService client interceptor Err: that is a local reject never opens a stream; this InteropTestService handler Err is after the handler ran.
/// Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this InteropTestService handler Err is after the handler ran.
/// Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this InteropTestService handler Err is after the handler ran.
/// Distinct from an InteropTestService StreamSender fail: that is trailers after any messages already sent; this InteropTestService handler Err is after the handler ran.
/// [`crate::Outgoing::connected`] is the live-socket snapshot on this InteropTestService client interceptor path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService client interceptor Err; a local reject never opens a stream.
/// Distinct from an InteropTestService handler Err: that is after the handler ran; this InteropTestService client interceptor Err is a local reject never opens a stream.
/// Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this InteropTestService client interceptor Err is a local reject never opens a stream.
/// Distinct from an InteropTestService interceptor Err: that is trailers without reading the body; this InteropTestService client interceptor Err is a local reject never opens a stream.
/// Distinct from an InteropTestService StreamSender fail: that is trailers after any messages already sent; this InteropTestService client interceptor Err is a local reject never opens a stream.
/// Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this InteropTestService client interceptor already ran, so a local Err never consumes that budget.
/// Distinct from an InteropTestService interceptor: that runs on the inbound RPC before the handler; this InteropTestService client interceptor runs on the outbound call before the stream opens.
/// [`crate::Status::from_error_details`] is the typed bag after this InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
/// Distinct from an InteropTestService handler Err: that is after the handler ran; this InteropTestService StreamSender fail is trailers after any messages already sent.
/// Distinct from an InteropTestService interceptor Err: that is trailers without reading the body; this InteropTestService StreamSender fail is trailers after any messages already sent.
/// Distinct from a testing server on_response Err: that is trailers-only after handler Ok; this InteropTestService StreamSender fail is trailers after any messages already sent.
/// Distinct from an InteropTestService client interceptor Err: that is a local reject never opens a stream; this InteropTestService StreamSender fail is trailers after any messages already sent.
/// Distinct from a testing Channel on_response Err: that fails the Call after a successful receive; this InteropTestService StreamSender fail is trailers after any messages already sent.
#[derive(Default)]
pub struct InteropTestService;

fn zeros_payload(n: i32) -> Payload {
    let n = usize::try_from(n.max(0)).unwrap_or(0);
    let mut p = Payload::new();
    p.set_body(vec![0u8; n]);
    p
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

/// Emit the planned responses. `false` means the peer went away.
async fn emit_plan(tx: &StreamSender<StreamingOutputCallResponse>, plan: ResponsePlan) -> bool {
    for (size, interval_us, compress) in plan {
        if interval_us > 0 {
            let us = u64::try_from(interval_us).unwrap_or(0);
            tokio::time::sleep(Duration::from_micros(us)).await;
        }
        let mut msg = StreamingOutputCallResponse::new();
        msg.set_payload(zeros_payload(size));
        let sent = if compress {
            tx.send_compressed(msg).await
        } else {
            tx.send(msg).await
        };
        if sent.is_err() {
            return false;
        }
    }
    true
}

fn echoed_status(code: i32, message: impl Into<String>) -> Status {
    Status::new(Code::from_i32(code), message)
}

async fn unary_reply(
    echo: &Echo,
    request: SimpleRequest,
    compressed: bool,
) -> Result<Response<SimpleResponse>, Status> {
    if request.has_expect_compressed() && request.expect_compressed().value() && !compressed {
        return Err(Status::invalid_argument("request not compressed"));
    }
    if request.has_response_status() {
        let st = request.response_status();
        return Err(echoed_status(st.code(), st.message().to_string()));
    }
    let mut msg = SimpleResponse::new();
    msg.set_payload(zeros_payload(request.response_size()));
    let mut resp = Response::new(msg);
    echo.apply(&mut resp);
    if request.has_response_compressed() && request.response_compressed().value() {
        resp.set_compress(true);
    }
    Ok(resp)
}

impl TestService for InteropTestService {
    async fn empty_call(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
        let echo = Echo::capture(&request);
        let mut resp = Response::new(Empty::new());
        echo.apply(&mut resp);
        Ok(resp)
    }

    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let echo = Echo::capture(&request);
        let compressed = request.compressed();
        unary_reply(&echo, request.into_inner(), compressed).await
    }

    /// Identical to `UnaryCall`; the interop suite only cares that it answers.
    async fn cacheable_unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let echo = Echo::capture(&request);
        let compressed = request.compressed();
        unary_reply(&echo, request.into_inner(), compressed).await
    }

    async fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let echo = Echo::capture(&request);
        let inner = request.into_inner();
        if inner.has_response_status() {
            let st = inner.response_status();
            return Err(echoed_status(st.code(), st.message().to_string()));
        }
        let plan = response_plan(&inner);
        let want_gzip = plan.iter().any(|(_, _, compress)| *compress);
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            emit_plan(&tx, plan).await;
        }));
        let mut resp = Response::new(stream);
        echo.apply(&mut resp);
        if want_gzip {
            resp.set_compress(true);
        }
        Ok(resp)
    }

    async fn streaming_input_call(
        &self,
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
            total = total.saturating_add(i32::try_from(n).unwrap_or(i32::MAX));
        }
        let mut msg = StreamingInputCallResponse::new();
        msg.set_aggregated_payload_size(total);
        let mut resp = Response::new(msg);
        echo.apply(&mut resp);
        Ok(resp)
    }

    async fn full_duplex_call(
        &self,
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
                        if !emit_plan(&tx, response_plan(&req)).await {
                            return;
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
