//! Assertions for gRPC interop test case behavior and failure modes.

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
    reason = "integration tests"
)]

use pbrs_grpc::interop_cases::{cancel_after_first_response, connect, run_case};
use pbrs_grpc::testing::{
    Empty, InteropTestService, Payload, SimpleRequest, SimpleResponse, StreamingInputCallRequest,
    StreamingInputCallResponse, StreamingOutputCallRequest, StreamingOutputCallResponse,
    TestService, TestServiceClient, TestServiceServer,
};
use pbrs_grpc::{Code, Request, Response, Status, Streaming};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

struct ServerGuard(JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn spawn_test_server<S: TestService>(service: S) -> (SocketAddr, ServerGuard) {
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
    (addr, ServerGuard(handle))
}

async fn connect_client(addr: SocketAddr) -> TestServiceClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match connect(addr).await {
            Ok(client) => return client,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

/// A mock server that ignores client cancellation and cleanly closes the
/// stream after the first response message without signaling CANCELLED.
struct CleanEofAfterFirstResponseService;

impl TestService for CleanEofAfterFirstResponseService {
    async fn full_duplex_call(
        &self,
        _request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        let mut msg = StreamingOutputCallResponse::new();
        let mut p = Payload::new();
        p.set_body(vec![0u8; 10]);
        msg.set_payload(p);
        tx.send(msg).await.ok();
        // Close the stream cleanly (EOF) before returning the response stream
        // so that message and trailers are batched together onto the wire.
        drop(tx);
        Ok(Response::new(stream))
    }
}

/// A mock server that ignores client cancellation and continues sending
/// subsequent messages after the first response.
struct ExtraMessageAfterFirstResponseService;

impl TestService for ExtraMessageAfterFirstResponseService {
    async fn full_duplex_call(
        &self,
        _request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        let mut msg1 = StreamingOutputCallResponse::new();
        let mut p1 = Payload::new();
        p1.set_body(vec![0u8; 31415]);
        msg1.set_payload(p1);
        tx.send(msg1).await.ok();

        // Send a second message before returning response stream so both are queued.
        let mut msg2 = StreamingOutputCallResponse::new();
        let mut p2 = Payload::new();
        p2.set_body(vec![0u8; 31415]);
        msg2.set_payload(p2);
        tx.send(msg2).await.ok();
        Ok(Response::new(stream))
    }
}

/// A mock server that correctly propagates cancellation by watching inbound for
/// the client reset and failing the response stream with CANCELLED.
struct PropagatingMockService;

impl TestService for PropagatingMockService {
    async fn full_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            if let Ok(Some(_req)) = inbound.message().await {
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0u8; 31415]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
                if let Err(st) = inbound.message().await {
                    tx.fail(st).await;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

#[tokio::test]
async fn cancel_after_first_response_succeeds_with_interop_server() {
    let (addr, _guard) = spawn_test_server(InteropTestService).await;
    let client = connect_client(addr).await;

    let res = cancel_after_first_response(&client).await;
    assert!(
        res.is_ok(),
        "expected cancel_after_first_response to succeed on InteropTestService, got: {res:?}"
    );

    let res_run_case = run_case(&client, "cancel_after_first_response").await;
    assert!(
        res_run_case.is_ok(),
        "expected run_case(cancel_after_first_response) to succeed, got: {res_run_case:?}"
    );
}

#[tokio::test]
async fn cancel_after_first_response_succeeds_with_propagating_server() {
    let (addr, _guard) = spawn_test_server(PropagatingMockService).await;
    let client = connect_client(addr).await;

    let res = cancel_after_first_response(&client).await;
    assert!(
        res.is_ok(),
        "expected cancel_after_first_response to succeed on propagating server, got: {res:?}"
    );
}

#[tokio::test]
async fn cancel_after_first_response_fails_on_clean_eof_server() {
    let (addr, _guard) = spawn_test_server(CleanEofAfterFirstResponseService).await;
    let client = connect_client(addr).await;

    let res = cancel_after_first_response(&client).await;
    assert!(
        res.is_err(),
        "expected cancel_after_first_response to fail when server cleanly closes stream"
    );
    let status = res.unwrap_err();
    assert_eq!(status.code(), Code::Internal);
    assert!(
        status.message().contains("CANCELLED"),
        "expected status message to explain CANCELLED was required, got: {:?}",
        status.message()
    );
    assert!(
        status.message().contains("clean EOF"),
        "expected status message to note clean EOF, got: {:?}",
        status.message()
    );

    let client2 = connect_client(addr).await;
    let run_res = run_case(&client2, "cancel_after_first_response").await;
    assert!(run_res.is_err());
    assert_eq!(run_res.unwrap_err().code(), Code::Internal);
}

#[tokio::test]
async fn cancel_after_first_response_fails_on_extra_message_server() {
    let (addr, _guard) = spawn_test_server(ExtraMessageAfterFirstResponseService).await;
    let client = connect_client(addr).await;

    let res = cancel_after_first_response(&client).await;
    assert!(
        res.is_err(),
        "expected cancel_after_first_response to fail when server sends extra message"
    );
    let status = res.unwrap_err();
    assert_eq!(status.code(), Code::Internal);
    assert!(
        status.message().contains("CANCELLED"),
        "expected status message to explain CANCELLED was required, got: {:?}",
        status.message()
    );
    assert!(
        status.message().contains("extra message"),
        "expected status message to note extra message, got: {:?}",
        status.message()
    );

    let client2 = connect_client(addr).await;
    let run_res = run_case(&client2, "cancel_after_first_response").await;
    assert!(run_res.is_err());
    assert_eq!(run_res.unwrap_err().code(), Code::Internal);
}

// --- Mock services for asserting failure modes ---

struct CorruptPayloadUnaryService;

impl TestService for CorruptPayloadUnaryService {
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let size = usize::try_from(request.into_inner().response_size()).unwrap_or(0);
        let mut msg = SimpleResponse::new();
        let mut p = Payload::new();
        p.set_body(vec![0x7f; size]);
        msg.set_payload(p);
        Ok(Response::new(msg))
    }
}

struct WrongPayloadLengthUnaryService;

impl TestService for WrongPayloadLengthUnaryService {
    async fn unary_call(
        &self,
        _request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let mut msg = SimpleResponse::new();
        let mut p = Payload::new();
        p.set_body(vec![0u8; 128]);
        msg.set_payload(p);
        Ok(Response::new(msg))
    }
}

struct WrongAggregatedSizeStreamingInputService;

impl TestService for WrongAggregatedSizeStreamingInputService {
    async fn streaming_input_call(
        &self,
        request: Request<Streaming<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        let mut inbound = request.into_inner();
        while inbound.message().await?.is_some() {}
        let mut resp = StreamingInputCallResponse::new();
        resp.set_aggregated_payload_size(12345);
        Ok(Response::new(resp))
    }
}

struct WrongLengthsServerStreamingService;

impl TestService for WrongLengthsServerStreamingService {
    async fn streaming_output_call(
        &self,
        _request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            for &n in &[31415, 10, 2653, 58979] {
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0u8; n]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct CorruptPayloadServerStreamingService;

impl TestService for CorruptPayloadServerStreamingService {
    async fn streaming_output_call(
        &self,
        _request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            for &n in &[31415, 9, 2653, 58979] {
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0xAA; n]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct WrongCountServerStreamingService;

impl TestService for WrongCountServerStreamingService {
    async fn streaming_output_call(
        &self,
        _request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            for &n in &[31415, 9] {
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0u8; n]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct WrongLengthPingPongService;

impl TestService for WrongLengthPingPongService {
    async fn full_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            while let Ok(Some(_req)) = inbound.message().await {
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0u8; 10]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct CorruptPayloadPingPongService;

impl TestService for CorruptPayloadPingPongService {
    async fn full_duplex_call(
        &self,
        request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                let size = req
                    .response_parameters()
                    .get(0)
                    .map(|p| p.size())
                    .unwrap_or(0);
                let mut msg = StreamingOutputCallResponse::new();
                let mut p = Payload::new();
                p.set_body(vec![0xBE; usize::try_from(size).unwrap_or(0)]);
                msg.set_payload(p);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

struct ExtraMessageEmptyStreamService;

impl TestService for ExtraMessageEmptyStreamService {
    async fn full_duplex_call(
        &self,
        _request: Request<Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        let mut msg = StreamingOutputCallResponse::new();
        let mut p = Payload::new();
        p.set_body(vec![0u8; 10]);
        msg.set_payload(p);
        tx.send(msg).await.ok();
        Ok(Response::new(stream))
    }
}

struct LeakedTrailingMetadataService;

impl TestService for LeakedTrailingMetadataService {
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let size = usize::try_from(request.get_ref().response_size()).unwrap_or(0);
        let mut resp = Response::new(SimpleResponse::new());
        if let Some(init) = request.metadata().get("x-grpc-test-echo-initial") {
            resp.metadata_mut()
                .insert("x-grpc-test-echo-initial", init)
                .ok();
        }
        if let Some(tb) = request.metadata().get_bin("x-grpc-test-echo-trailing-bin") {
            resp.metadata_mut()
                .insert_bin("x-grpc-test-echo-trailing-bin", &tb)
                .ok();
            resp.trailers_mut()
                .insert_bin("x-grpc-test-echo-trailing-bin", &tb)
                .ok();
        }
        let mut p = Payload::new();
        p.set_body(vec![0u8; size]);
        resp.get_mut().set_payload(p);
        Ok(resp)
    }
}

struct MissingTrailingMetadataService;

impl TestService for MissingTrailingMetadataService {
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let size = usize::try_from(request.get_ref().response_size()).unwrap_or(0);
        let mut resp = Response::new(SimpleResponse::new());
        if let Some(init) = request.metadata().get("x-grpc-test-echo-initial") {
            resp.metadata_mut()
                .insert("x-grpc-test-echo-initial", init)
                .ok();
        }
        let mut p = Payload::new();
        p.set_body(vec![0u8; size]);
        resp.get_mut().set_payload(p);
        Ok(resp)
    }
}

struct WrongStatusCodeService;

impl TestService for WrongStatusCodeService {
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let inner = request.into_inner();
        let msg = if inner.has_response_status() {
            inner.response_status().message().to_string()
        } else {
            "test status message".to_string()
        };
        Err(Status::new(Code::InvalidArgument, msg))
    }
}

struct WrongStatusMessageService;

impl TestService for WrongStatusMessageService {
    async fn unary_call(
        &self,
        _request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        Err(Status::new(Code::Unknown, "completely wrong message"))
    }
}

struct UnimplementedReturnsOkService;

impl TestService for UnimplementedReturnsOkService {
    async fn unimplemented_call(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        Ok(Response::new(Empty::new()))
    }
}

struct ServerCompressedUnaryFailsWhenNotCompressed;

impl TestService for ServerCompressedUnaryFailsWhenNotCompressed {
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let size = usize::try_from(request.get_ref().response_size()).unwrap_or(0);
        let mut resp = Response::new(SimpleResponse::new());
        let mut p = Payload::new();
        p.set_body(vec![0u8; size]);
        resp.get_mut().set_payload(p);
        resp.set_compress(false);
        Ok(resp)
    }
}

struct ServerCompressedStreamingFailsWhenNotCompressed;

impl TestService for ServerCompressedStreamingFailsWhenNotCompressed {
    async fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<StreamingOutputCallResponse>>, Status> {
        let (tx, stream) = Streaming::channel(8);
        let inner = request.into_inner();
        let sizes: Vec<usize> = inner
            .response_parameters()
            .iter()
            .map(|p| usize::try_from(p.size()).unwrap_or(0))
            .collect();
        tokio::spawn(async move {
            for size in sizes {
                let mut msg = StreamingOutputCallResponse::new();
                let mut payload = Payload::new();
                payload.set_body(vec![0u8; size]);
                msg.set_payload(payload);
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

// --- Test cases verifying assertion failures ---

#[tokio::test]
async fn large_unary_fails_on_corrupt_payload() {
    let (addr, _guard) = spawn_test_server(CorruptPayloadUnaryService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "large_unary").await;
    assert!(
        res.is_err(),
        "expected large_unary to fail on corrupt payload"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("non-zero"),
        "expected error message about non-zero bytes, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn large_unary_fails_on_wrong_length() {
    let (addr, _guard) = spawn_test_server(WrongPayloadLengthUnaryService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "large_unary").await;
    assert!(res.is_err(), "expected large_unary to fail on wrong length");
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("payload len"),
        "expected error message about payload length, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn client_streaming_fails_on_wrong_aggregated_size() {
    let (addr, _guard) = spawn_test_server(WrongAggregatedSizeStreamingInputService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "client_streaming").await;
    assert!(
        res.is_err(),
        "expected client_streaming to fail on wrong aggregated size"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("74922"),
        "expected error message mentioning 74922, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn server_streaming_fails_on_wrong_lengths() {
    let (addr, _guard) = spawn_test_server(WrongLengthsServerStreamingService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "server_streaming").await;
    assert!(
        res.is_err(),
        "expected server_streaming to fail on wrong lengths"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
}

#[tokio::test]
async fn server_streaming_fails_on_corrupt_payload() {
    let (addr, _guard) = spawn_test_server(CorruptPayloadServerStreamingService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "server_streaming").await;
    assert!(
        res.is_err(),
        "expected server_streaming to fail on corrupt payload"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("non-zero"),
        "expected error message about non-zero bytes, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn server_streaming_fails_on_wrong_count() {
    let (addr, _guard) = spawn_test_server(WrongCountServerStreamingService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "server_streaming").await;
    assert!(
        res.is_err(),
        "expected server_streaming to fail on truncated stream"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
}

#[tokio::test]
async fn ping_pong_fails_on_wrong_length() {
    let (addr, _guard) = spawn_test_server(WrongLengthPingPongService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "ping_pong").await;
    assert!(res.is_err(), "expected ping_pong to fail on wrong length");
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
}

#[tokio::test]
async fn ping_pong_fails_on_corrupt_payload() {
    let (addr, _guard) = spawn_test_server(CorruptPayloadPingPongService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "ping_pong").await;
    assert!(
        res.is_err(),
        "expected ping_pong to fail on corrupt payload"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("non-zero"),
        "expected error message about non-zero bytes, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn empty_stream_fails_on_unexpected_message() {
    let (addr, _guard) = spawn_test_server(ExtraMessageEmptyStreamService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "empty_stream").await;
    assert!(
        res.is_err(),
        "expected empty_stream to fail on unexpected message"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
}

#[tokio::test]
async fn custom_metadata_fails_when_trailing_metadata_leaked_into_headers() {
    let (addr, _guard) = spawn_test_server(LeakedTrailingMetadataService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "custom_metadata").await;
    assert!(
        res.is_err(),
        "expected custom_metadata to fail when trailing-bin in headers"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("trailing-bin in headers"),
        "expected error message about trailing-bin in headers, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn custom_metadata_fails_when_trailing_metadata_missing() {
    let (addr, _guard) = spawn_test_server(MissingTrailingMetadataService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "custom_metadata").await;
    assert!(
        res.is_err(),
        "expected custom_metadata to fail when trailing-bin missing"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("missing trailing-bin trailers"),
        "expected error message about missing trailing-bin, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn status_code_and_message_fails_on_wrong_status_code() {
    let (addr, _guard) = spawn_test_server(WrongStatusCodeService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "status_code_and_message").await;
    assert!(res.is_err(), "expected failure on wrong status code");
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("status code mismatch"),
        "expected error about status code mismatch, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn status_code_and_message_fails_on_wrong_status_message() {
    let (addr, _guard) = spawn_test_server(WrongStatusMessageService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "status_code_and_message").await;
    assert!(res.is_err(), "expected failure on wrong status message");
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("status message mismatch"),
        "expected error about status message mismatch, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn special_status_message_fails_on_wrong_message() {
    let (addr, _guard) = spawn_test_server(WrongStatusMessageService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "special_status_message").await;
    assert!(
        res.is_err(),
        "expected failure on wrong special status message"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("status message mismatch"),
        "expected error about status message mismatch, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn unimplemented_method_fails_when_server_returns_ok() {
    let (addr, _guard) = spawn_test_server(UnimplementedReturnsOkService).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "unimplemented_method").await;
    assert!(
        res.is_err(),
        "expected failure when unimplemented method returns OK"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("UNIMPLEMENTED"),
        "expected error mentioning UNIMPLEMENTED, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn server_compressed_unary_fails_when_not_compressed() {
    let (addr, _guard) = spawn_test_server(ServerCompressedUnaryFailsWhenNotCompressed).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "server_compressed_unary").await;
    assert!(
        res.is_err(),
        "expected failure when response is uncompressed"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert!(
        err.message().contains("compressed"),
        "expected error mentioning compressed, got: {:?}",
        err.message()
    );
}

#[tokio::test]
async fn server_compressed_streaming_fails_when_not_compressed() {
    let (addr, _guard) = spawn_test_server(ServerCompressedStreamingFailsWhenNotCompressed).await;
    let client = connect_client(addr).await;
    let res = run_case(&client, "server_compressed_streaming").await;
    assert!(
        res.is_err(),
        "expected failure when streaming response is uncompressed"
    );
    let err = res.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
}

#[tokio::test]
async fn all_cases_succeed_against_reference_interop_server() {
    let (addr, _guard) = spawn_test_server(InteropTestService).await;
    let client = connect_client(addr).await;
    let cases = [
        "empty_unary",
        "large_unary",
        "client_streaming",
        "server_streaming",
        "ping_pong",
        "empty_stream",
        "cancel_after_begin",
        "cancel_after_first_response",
        "timeout_on_sleeping_server",
        "custom_metadata",
        "status_code_and_message",
        "special_status_message",
        "unimplemented_method",
        "unimplemented_service",
        "client_compressed_unary",
        "server_compressed_unary",
        "client_compressed_streaming",
        "server_compressed_streaming",
    ];
    for c in cases {
        let res = run_case(&client, c).await;
        assert!(
            res.is_ok(),
            "expected case {c} to succeed against InteropTestService, got: {res:?}"
        );
    }
}
