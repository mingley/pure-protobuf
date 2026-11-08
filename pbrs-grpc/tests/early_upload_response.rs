//! Responses received before a flow-control-stalled request finishes uploading.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "local HTTP/2 peer and regression assertions"
)]

use bytes::Bytes;
use pbrs_grpc::{Channel, Code, HelloReply, HelloRequest, Request, Response, Status, Streaming};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

async fn rejected(http_status: u16, grpc_status: Option<&'static str>, streaming: bool) -> Status {
    rejected_with_body(http_status, grpc_status, streaming, false).await
}

async fn rejected_with_body(
    http_status: u16,
    grpc_status: Option<&'static str>,
    streaming: bool,
    open_body: bool,
) -> Status {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (settings_tx, settings_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let (socket, _) = listener.accept().await.unwrap();
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let mut ping = connection.ping_pong().expect("PING handle");
        tokio::select! {
            result = ping.ping(h2::Ping::opaque()) => {
                result.expect("SETTINGS acknowledgement");
            }
            incoming = connection.accept() => {
                panic!("request before zero-credit SETTINGS: {incoming:?}");
            }
        }
        settings_tx.send(()).expect("SETTINGS barrier");
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        let content_type = if grpc_status.is_some() {
            "application/grpc"
        } else {
            "text/plain"
        };
        let mut response = http::Response::builder()
            .status(http_status)
            .header("content-type", content_type)
            .header("x-terminal", "rejected")
            .header("x-trace-bin", "AAH/");
        if let Some(status) = grpc_status {
            response = response
                .header("grpc-status", status)
                .header("grpc-message", "busy")
                .header("grpc-status-details-bin", "AQID");
        }
        let send = respond
            .send_response(response.body(()).unwrap(), !open_body)
            .unwrap();
        // Keep both halves alive without granting credit or resetting. An
        // HTTP/2 rejection does not require the peer to read the request.
        let held = (request, respond, send);
        while let Some(request) = connection.accept().await {
            request.unwrap();
        }
        drop(held);
    });
    let channel = Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024);
    tokio::time::timeout(Duration::from_secs(3), settings_rx)
        .await
        .expect("SETTINGS barrier timed out")
        .expect("SETTINGS barrier failed");
    let mut message = HelloRequest::new();
    message.set_name("x".repeat(1024 * 1024));
    let mut request = Request::new(message);
    let is_rejection = (http_status == 200 && grpc_status == Some("8"))
        || (http_status != 200 && grpc_status != Some("0"));
    request.set_timeout(if is_rejection {
        Duration::from_secs(3)
    } else {
        Duration::from_millis(80)
    });
    let status = if streaming {
        channel
            .server_streaming::<_, HelloReply>("/helloworld.Greeter/ServerHello", request)
            .await
            .unwrap_err()
    } else {
        channel
            .unary::<_, HelloReply>("/helloworld.Greeter/SayHello", request)
            .await
            .unwrap_err()
    };
    peer.abort();
    status
}

#[tokio::test]
async fn unary_rejection_does_not_wait_for_upload_credit() {
    let status = rejected(200, Some("8"), false).await;
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(status.message(), "busy");
    assert_eq!(status.metadata().get("x-terminal"), Some("rejected"));
}

#[tokio::test]
async fn server_stream_rejection_does_not_wait_for_upload_credit() {
    let status = rejected(200, Some("8"), true).await;
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(status.message(), "busy");
    assert_eq!(status.metadata().get("x-terminal"), Some("rejected"));
}

#[tokio::test]
async fn invalid_or_success_headers_do_not_complete_an_incomplete_upload() {
    for streaming in [false, true] {
        for (http_status, grpc_status) in [
            (200, Some("0")),
            (200, None),
            (200, Some("bad")),
            (200, Some("17")),
            (503, Some("0")),
        ] {
            assert_eq!(
                rejected(http_status, grpc_status, streaming).await.code(),
                Code::DeadlineExceeded,
                "http={http_status}, grpc={grpc_status:?}, streaming={streaming}"
            );
        }
    }
}

#[tokio::test]
async fn non_200_rejection_does_not_wait_for_upload_credit() {
    for streaming in [false, true] {
        for open_body in [false, true] {
            for (grpc_status, expected) in [
                (None, Code::Unavailable),
                (Some("8"), Code::ResourceExhausted),
                (Some("bad"), Code::Unknown),
                (Some("17"), Code::Unknown),
            ] {
                let status = rejected_with_body(503, grpc_status, streaming, open_body).await;
                assert_eq!(status.code(), expected);
                assert_eq!(status.metadata().get("x-terminal"), Some("rejected"));
                assert_eq!(
                    status.metadata().get_bin("x-trace-bin"),
                    Some(vec![0, 1, 255])
                );
                if grpc_status == Some("8") {
                    assert_eq!(status.message(), "busy");
                    assert_eq!(status.details(), &[1, 2, 3]);
                }
            }
            let status = rejected_with_body(503, Some("0"), streaming, open_body).await;
            assert_eq!(
                status.code(),
                Code::DeadlineExceeded,
                "initial OK must not complete an unfinished upload"
            );
        }
    }
}

#[tokio::test]
async fn early_http_error_does_not_retry_and_connection_recovers() {
    for streaming in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let executions = Arc::new(AtomicUsize::new(0));
        let seen = executions.clone();
        let (settings_tx, settings_rx) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("connection");
            let mut builder = h2::server::Builder::new();
            builder.initial_window_size(0);
            let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
            let mut ping = connection.ping_pong().expect("PING handle");
            tokio::select! {
                result = ping.ping(h2::Ping::opaque()) => {
                    result.expect("SETTINGS acknowledgement");
                }
                incoming = connection.accept() => {
                    panic!("request before zero-credit SETTINGS: {incoming:?}");
                }
            }
            settings_tx.send(()).expect("SETTINGS barrier");
            let (first, mut respond) = connection
                .accept()
                .await
                .expect("first request")
                .expect("first headers");
            seen.fetch_add(1, Ordering::SeqCst);
            let send = respond
                .send_response(
                    http::Response::builder()
                        .status(503)
                        .header("content-type", "text/plain")
                        .header("x-terminal", "proxy")
                        .body(())
                        .expect("error response"),
                    false,
                )
                .expect("error headers");
            let first_held = (first, respond, send);
            let (second, mut respond) = connection
                .accept()
                .await
                .expect("second request")
                .expect("second headers");
            seen.fetch_add(1, Ordering::SeqCst);
            connection
                .set_initial_window_size(128 * 1024)
                .expect("grant follow-up credit");
            let mut send = respond
                .send_response(
                    http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .expect("success response"),
                    false,
                )
                .expect("success headers");
            send.send_data(Bytes::from_static(b"\0\0\0\0\x07\n\x05reply"), false)
                .expect("response DATA");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
            send.send_trailers(trailers).expect("response trailers");
            let second_held = (second, respond);
            if let Some(Ok(_)) = connection.accept().await {
                panic!("unexpected third request");
            }
            drop((first_held, second_held));
        });
        let channel = Channel::connect(address)
            .await
            .expect("connect")
            .max_send_buffer_size(1024)
            .service_config(
                r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                    "maxAttempts":2,"initialBackoff":"0s","maxBackoff":"0s",
                    "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#,
            )
            .expect("policy");
        tokio::time::timeout(Duration::from_secs(3), settings_rx)
            .await
            .expect("SETTINGS barrier timed out")
            .expect("SETTINGS barrier failed");
        let mut message = HelloRequest::new();
        message.set_name("x".repeat(32 * 1024));
        let mut request = Request::new(message);
        request.set_timeout(Duration::from_secs(5));
        let status = if streaming {
            tokio::time::timeout(
                Duration::from_secs(8),
                channel.server_streaming::<_, HelloReply>(
                    "/helloworld.Greeter/ServerHello",
                    request,
                ),
            )
            .await
            .expect("early response timed out")
            .expect_err("committed response cannot be retried")
        } else {
            tokio::time::timeout(
                Duration::from_secs(8),
                channel.unary::<_, HelloReply>("/helloworld.Greeter/SayHello", request),
            )
            .await
            .expect("early response timed out")
            .expect_err("committed response cannot be retried")
        };
        assert_eq!(status.code(), Code::Unavailable);
        assert_eq!(status.metadata().get("x-terminal"), Some("proxy"));
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(channel.retry_stats().policy_retries, 0);
        assert_eq!(channel.retry_stats().transparent_retries, 0);

        let follow_up = tokio::time::timeout(
            Duration::from_secs(5),
            channel.unary::<_, HelloReply>(
                "/helloworld.Greeter/SayHello",
                Request::new(HelloRequest::new()),
            ),
        )
        .await
        .expect("follow-up timed out")
        .expect("connection remains usable");
        assert_eq!(follow_up.into_inner().message(), "reply");
        assert_eq!(executions.load(Ordering::SeqCst), 2);
        peer.abort();
    }
}

async fn completed_after_initial_headers(streaming: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        let mut send = respond
            .send_response(
                http::Response::builder()
                    .header("content-type", "application/grpc")
                    .header("x-initial", "observed")
                    .body(())
                    .unwrap(),
                false,
            )
            .unwrap();
        // Flush headers while credit remains zero, then allow the request.
        tokio::select! {
            result = connection.accept() => panic!("unexpected second request: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(20)) => {},
        }
        connection.set_initial_window_size(2 * 1024 * 1024).unwrap();
        let read = async {
            let mut body = request.into_body();
            let mut bytes = 0;
            while let Some(data) = body.data().await {
                let data = data.unwrap();
                bytes += data.len();
                body.flow_control().release_capacity(data.len()).unwrap();
            }
            assert!(bytes > 1024 * 1024, "the full request frame must arrive");
            send.send_data(Bytes::from_static(b"\0\0\0\0\x0a\x0a\x08accepted"), false)
                .unwrap();
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
            trailers.insert("x-terminal", http::HeaderValue::from_static("done"));
            send.send_trailers(trailers).unwrap();
        };
        let drive = async {
            while let Some(request) = connection.accept().await {
                request.unwrap();
            }
        };
        tokio::join!(read, drive);
    });
    let channel = Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024);
    let mut message = HelloRequest::new();
    message.set_name("x".repeat(1024 * 1024));
    let mut request = Request::new(message);
    request.set_timeout(Duration::from_secs(3));
    if streaming {
        let mut response = channel
            .server_streaming::<_, HelloReply>("/helloworld.Greeter/ServerHello", request)
            .await
            .unwrap();
        assert_eq!(response.metadata().get("x-initial"), Some("observed"));
        let reply = response.get_mut().message().await.unwrap().unwrap();
        assert_eq!(reply.message(), "accepted");
        assert!(response.get_mut().message().await.unwrap().is_none());
        assert_eq!(
            response
                .get_mut()
                .trailers()
                .await
                .unwrap()
                .get("x-terminal"),
            Some("done")
        );
    } else {
        let response = channel
            .unary::<_, HelloReply>("/helloworld.Greeter/SayHello", request)
            .await
            .unwrap();
        assert_eq!(response.metadata().get("x-initial"), Some("observed"));
        assert_eq!(response.get_ref().message(), "accepted");
        assert_eq!(response.trailers().get("x-terminal"), Some("done"));
    }
    peer.abort();
}

#[tokio::test]
async fn unary_retains_initial_headers_while_finishing_upload() {
    completed_after_initial_headers(false).await;
}

#[tokio::test]
async fn server_stream_retains_initial_headers_while_finishing_upload() {
    completed_after_initial_headers(true).await;
}

#[derive(Clone, Copy, Debug)]
enum CompletedShape {
    Unary,
    ServerStream,
    ClientStream,
    Bidi,
}

#[derive(Clone, Copy)]
enum PeerBody {
    HeadersOnly,
    Hold,
    HoldWithDeadline,
    Message,
    MessageWithErrorTrailer,
    MessageWithMalformedTrailer,
    MessageWithMetadataTrailer,
    TruncatedWithErrorTrailer,
}

fn inspect_unary_response(response: Response<HelloReply>, peer_body: PeerBody) -> usize {
    assert_eq!(response.metadata().get("x-terminal"), Some("kept"));
    if matches!(peer_body, PeerBody::MessageWithMetadataTrailer) {
        assert_eq!(response.trailers().get("x-trailer"), Some("kept"));
    }
    assert_eq!(
        response
            .into_inner()
            .message()
            .to_str()
            .expect("reply UTF-8"),
        "reply"
    );
    1
}

async fn inspect_stream_response(
    response: Response<Streaming<HelloReply>>,
    peer_body: PeerBody,
) -> Result<usize, Status> {
    assert_eq!(response.metadata().get("x-terminal"), Some("kept"));
    let mut stream = response.into_inner();
    let first = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("first stream message timed out")?;
    let count = if let Some(reply) = first {
        assert_eq!(reply.message().to_str().expect("reply UTF-8"), "reply");
        let second = tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await
            .expect("stream termination timed out")?;
        assert!(second.is_none(), "unexpected second response message");
        1
    } else {
        0
    };
    if matches!(peer_body, PeerBody::MessageWithMetadataTrailer) {
        let trailers = tokio::time::timeout(Duration::from_secs(5), stream.trailers())
            .await
            .expect("stream trailers timed out")?;
        assert_eq!(trailers.get("x-trailer"), Some("kept"));
    }
    Ok(count)
}

// The peer waits for the full request half-close before sending a response.
// Hold keeps an error body open to prove the client does not wait for it.
async fn response_after_complete_upload(
    http_status: u16,
    grpc_status: Option<&'static str>,
    peer_body: PeerBody,
    shape: CompletedShape,
) -> Result<usize, Status> {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server).await.expect("peer handshake");
        let (request, mut respond) = connection
            .accept()
            .await
            .expect("client connection")
            .expect("request headers");
        let handler = async move {
            let mut body = request.into_body();
            let mut request_bytes = 0;
            while let Some(chunk) = body.data().await {
                let chunk = chunk.expect("request DATA");
                request_bytes += chunk.len();
                body.flow_control()
                    .release_capacity(chunk.len())
                    .expect("request credit");
            }
            assert!(request_bytes >= 5, "expected a framed request message");
            assert!(body.trailers().await.expect("request trailers").is_none());
            let content_type = if grpc_status.is_some() {
                "application/grpc"
            } else {
                "text/plain"
            };
            let mut response = http::Response::builder()
                .status(http_status)
                .header("content-type", content_type)
                .header("x-terminal", "kept")
                .header("x-trace-bin", "AAH/");
            if let Some(code) = grpc_status {
                response = response
                    .header("grpc-status", code)
                    .header("grpc-message", "proxy%20detail")
                    .header("grpc-status-details-bin", "AQID");
            }
            let mut send = respond
                .send_response(
                    response.body(()).expect("response"),
                    matches!(peer_body, PeerBody::HeadersOnly),
                )
                .expect("response headers");
            match peer_body {
                PeerBody::HeadersOnly => {}
                PeerBody::Hold | PeerBody::HoldWithDeadline => {
                    let held = (send, respond);
                    std::future::pending::<()>().await;
                    drop(held);
                }
                PeerBody::Message => {
                    send.send_data(Bytes::from_static(b"\0\0\0\0\x07\n\x05reply"), true)
                        .expect("response message");
                }
                PeerBody::MessageWithErrorTrailer
                | PeerBody::MessageWithMalformedTrailer
                | PeerBody::MessageWithMetadataTrailer
                | PeerBody::TruncatedWithErrorTrailer => {
                    let message = if matches!(peer_body, PeerBody::TruncatedWithErrorTrailer) {
                        Bytes::from_static(b"\0\0")
                    } else {
                        Bytes::from_static(b"\0\0\0\0\x07\n\x05reply")
                    };
                    send.send_data(message, false).expect("response DATA");
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("x-trailer", http::HeaderValue::from_static("kept"));
                    match peer_body {
                        PeerBody::MessageWithErrorTrailer | PeerBody::TruncatedWithErrorTrailer => {
                            trailers.insert("grpc-status", http::HeaderValue::from_static("8"));
                        }
                        PeerBody::MessageWithMalformedTrailer => {
                            trailers.insert("grpc-status", http::HeaderValue::from_static("bad"));
                        }
                        _ => {}
                    }
                    send.send_trailers(trailers).expect("response trailers");
                }
            }
        };
        let drive = async {
            if let Some(Ok(_)) = connection.accept().await {
                panic!("unexpected second request");
            }
        };
        let ((), ()) = tokio::join!(handler, drive);
    });
    let channel = tokio::time::timeout(
        Duration::from_secs(5),
        Channel::from_io(client, "localhost"),
    )
    .await
    .expect("client handshake timed out")
    .expect("client handshake");
    let outcome = match shape {
        CompletedShape::Unary => {
            let mut request = Request::new(HelloRequest::new());
            if matches!(peer_body, PeerBody::HoldWithDeadline) {
                request.set_timeout(Duration::from_secs(1));
            }
            tokio::time::timeout(
                Duration::from_secs(5),
                channel.unary::<HelloRequest, HelloReply>("/helloworld.Greeter/SayHello", request),
            )
            .await
            .expect("unary response timed out")
            .map(|response| inspect_unary_response(response, peer_body))
        }
        CompletedShape::ServerStream => {
            let mut request = Request::new(HelloRequest::new());
            if matches!(peer_body, PeerBody::HoldWithDeadline) {
                request.set_timeout(Duration::from_secs(1));
            }
            match tokio::time::timeout(
                Duration::from_secs(5),
                channel.server_streaming::<HelloRequest, HelloReply>(
                    "/helloworld.Greeter/ServerHello",
                    request,
                ),
            )
            .await
            .expect("server-streaming response timed out")
            {
                Ok(response) => inspect_stream_response(response, peer_body).await,
                Err(status) => Err(status),
            }
        }
        CompletedShape::ClientStream => {
            let (sender, call) = channel.client_streaming::<HelloRequest, HelloReply>(
                "/helloworld.Greeter/ClientHello",
                Request::new(()),
            );
            tokio::time::timeout(Duration::from_secs(5), sender.send(HelloRequest::new()))
                .await
                .expect("queue request timed out")
                .expect("queue request");
            drop(sender);
            tokio::time::timeout(Duration::from_secs(5), call)
                .await
                .expect("client-streaming response timed out")
                .map(|response| inspect_unary_response(response, peer_body))
        }
        CompletedShape::Bidi => {
            let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(
                "/helloworld.Greeter/StreamHello",
                Request::new(()),
            );
            tokio::time::timeout(Duration::from_secs(5), sender.send(HelloRequest::new()))
                .await
                .expect("queue request timed out")
                .expect("queue request");
            drop(sender);
            match tokio::time::timeout(Duration::from_secs(5), call)
                .await
                .expect("bidi response timed out")
            {
                Ok(response) => inspect_stream_response(response, peer_body).await,
                Err(status) => Err(status),
            }
        }
    };
    drop(channel);
    peer.abort();
    outcome
}

#[tokio::test]
async fn completed_upload_maps_http_errors_without_grpc_status() {
    let cases = [
        (400, Code::Internal),
        (401, Code::Unauthenticated),
        (403, Code::PermissionDenied),
        (404, Code::Unimplemented),
        (429, Code::Unavailable),
        (502, Code::Unavailable),
        (503, Code::Unavailable),
        (504, Code::Unavailable),
        (418, Code::Unknown),
    ];
    for shape in [CompletedShape::Unary, CompletedShape::ServerStream] {
        for (http_status, expected) in cases {
            let status =
                response_after_complete_upload(http_status, None, PeerBody::HeadersOnly, shape)
                    .await
                    .expect_err("HTTP error must fail");
            assert_eq!(
                status.code(),
                expected,
                "http={http_status}, shape={shape:?}"
            );
            assert_eq!(status.metadata().get("x-terminal"), Some("kept"));
            assert_eq!(
                status.metadata().get_bin("x-trace-bin"),
                Some(vec![0, 1, 255])
            );
        }
        let status = response_after_complete_upload(503, None, PeerBody::Hold, shape)
            .await
            .expect_err("open error body must not be awaited");
        assert_eq!(status.code(), Code::Unavailable);
    }
}

#[tokio::test]
async fn completed_upload_prefers_present_grpc_status_on_http_error() {
    for shape in [CompletedShape::Unary, CompletedShape::ServerStream] {
        for peer_body in [PeerBody::HeadersOnly, PeerBody::Hold] {
            let status = response_after_complete_upload(503, Some("8"), peer_body, shape)
                .await
                .expect_err("gRPC status must win");
            assert_eq!(status.code(), Code::ResourceExhausted);
            assert_eq!(status.message(), "proxy detail");
            assert_eq!(status.metadata().get("x-terminal"), Some("kept"));
            assert_eq!(status.details(), &[1, 2, 3]);

            let malformed = response_after_complete_upload(503, Some("bad"), peer_body, shape)
                .await
                .expect_err("malformed gRPC status must fail");
            assert_eq!(malformed.code(), Code::Unknown);
            assert_eq!(malformed.metadata().get("x-terminal"), Some("kept"));
            assert_eq!(
                malformed.metadata().get_bin("x-trace-bin"),
                Some(vec![0, 1, 255])
            );
        }
    }
}

#[tokio::test]
async fn completed_upload_honors_initial_ok_and_validates_later_status() {
    let unary = response_after_complete_upload(
        503,
        Some("0"),
        PeerBody::HeadersOnly,
        CompletedShape::Unary,
    )
    .await
    .expect_err("a unary response still needs a message");
    assert_eq!(unary.code(), Code::Internal);
    assert_eq!(unary.message(), "Missing response message.");
    assert_eq!(
        response_after_complete_upload(
            503,
            Some("0"),
            PeerBody::HeadersOnly,
            CompletedShape::ServerStream,
        )
        .await
        .expect("a terminal OK server stream may be empty"),
        0
    );

    for shape in [CompletedShape::Unary, CompletedShape::ServerStream] {
        for peer_body in [PeerBody::Message, PeerBody::MessageWithMetadataTrailer] {
            assert_eq!(
                response_after_complete_upload(503, Some("0"), peer_body, shape)
                    .await
                    .expect("initial OK must allow response DATA"),
                1
            );
        }
        for (peer_body, expected) in [
            (PeerBody::MessageWithErrorTrailer, Code::ResourceExhausted),
            (PeerBody::MessageWithMalformedTrailer, Code::Unknown),
            (PeerBody::TruncatedWithErrorTrailer, Code::Internal),
        ] {
            let status = response_after_complete_upload(503, Some("0"), peer_body, shape)
                .await
                .expect_err("invalid response must fail");
            assert_eq!(status.code(), expected);
        }
    }
}

#[tokio::test]
async fn half_closed_streaming_uploads_use_response_header_mapping() {
    for shape in [CompletedShape::ClientStream, CompletedShape::Bidi] {
        let fallback = response_after_complete_upload(503, None, PeerBody::Hold, shape)
            .await
            .expect_err("HTTP error must fail");
        assert_eq!(fallback.code(), Code::Unavailable);
        assert_eq!(fallback.metadata().get("x-terminal"), Some("kept"));
        let grpc = response_after_complete_upload(503, Some("8"), PeerBody::Hold, shape)
            .await
            .expect_err("initial gRPC error must win");
        assert_eq!(grpc.code(), Code::ResourceExhausted);
        assert_eq!(grpc.metadata().get("x-terminal"), Some("kept"));
        assert_eq!(grpc.details(), &[1, 2, 3]);
        assert_eq!(
            response_after_complete_upload(503, Some("0"), PeerBody::Message, shape)
                .await
                .expect("initial OK must allow response DATA"),
            1
        );
    }
}

#[tokio::test]
async fn initial_ok_with_open_body_remains_deadline_bounded() {
    for shape in [CompletedShape::Unary, CompletedShape::ServerStream] {
        let status =
            response_after_complete_upload(503, Some("0"), PeerBody::HoldWithDeadline, shape)
                .await
                .expect_err("deadline must stop the open response");
        assert_eq!(status.code(), Code::DeadlineExceeded);
    }
}

#[tokio::test]
async fn mapped_http_error_with_open_body_preserves_the_connection() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server).await.expect("peer handshake");
        let mut handlers = tokio::task::JoinSet::new();
        let mut accepted = 0;
        loop {
            tokio::select! {
                completed = handlers.join_next(), if !handlers.is_empty() => {
                    completed
                        .expect("handler result")
                        .expect("raw handler did not panic");
                }
                incoming = connection.accept() => {
                    let Some(incoming) = incoming else {
                        break;
                    };
                    let (request, mut respond) = incoming.expect("request");
                    accepted += 1;
                    let ordinal = accepted;
                    assert!(ordinal <= 2, "unexpected third request");
                    handlers.spawn(async move {
                        let mut body = request.into_body();
                        let mut received = 0;
                        while let Some(chunk) = body.data().await {
                            let chunk = chunk.expect("request DATA");
                            received += chunk.len();
                            body.flow_control()
                                .release_capacity(chunk.len())
                                .expect("request credit");
                        }
                        assert!(received >= 5, "expected a framed request message");
                        assert!(body.trailers().await.expect("request trailers").is_none());
                        if ordinal == 1 {
                            let send = respond
                                .send_response(
                                    http::Response::builder()
                                        .status(503)
                                        .header("content-type", "text/plain")
                                        .body(())
                                        .expect("error headers"),
                                    false,
                                )
                                .expect("send error headers");
                            let held = (send, respond);
                            std::future::pending::<()>().await;
                            drop(held);
                        } else {
                            let mut send = respond
                                .send_response(
                                    http::Response::builder()
                                        .header("content-type", "application/grpc")
                                        .body(())
                                        .expect("success headers"),
                                    false,
                                )
                                .expect("send success headers");
                            send.send_data(
                                Bytes::from_static(b"\0\0\0\0\x07\n\x05reply"),
                                false,
                            )
                            .expect("send reply");
                            let mut trailers = http::HeaderMap::new();
                            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                            send.send_trailers(trailers).expect("send trailers");
                        }
                    });
                }
            }
        }
    });
    let channel = tokio::time::timeout(
        Duration::from_secs(5),
        Channel::from_io(client, "localhost"),
    )
    .await
    .expect("client handshake timed out")
    .expect("client handshake");
    let rejected = tokio::time::timeout(
        Duration::from_secs(5),
        channel.unary::<HelloRequest, HelloReply>(
            "/helloworld.Greeter/SayHello",
            Request::new(HelloRequest::new()),
        ),
    )
    .await
    .expect("error response timed out")
    .expect_err("first call must be rejected");
    assert_eq!(rejected.code(), Code::Unavailable);
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        channel.unary::<HelloRequest, HelloReply>(
            "/helloworld.Greeter/SayHello",
            Request::new(HelloRequest::new()),
        ),
    )
    .await
    .expect("recovery call timed out")
    .expect("connection must remain usable");
    assert_eq!(
        response.into_inner().message().to_str().expect("UTF-8"),
        "reply"
    );
    drop(channel);
    peer.abort();
}
