//! Responses received before a flow-control-stalled request finishes uploading.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "local HTTP/2 peer and regression assertions"
)]

use bytes::Bytes;
use pbrs_grpc::{Channel, Code, HelloReply, HelloRequest, Request, Status};
use std::time::Duration;

async fn rejected(http_status: u16, grpc_status: Option<&'static str>, streaming: bool) -> Status {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let (socket, _) = listener.accept().await.unwrap();
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        let mut response = http::Response::builder()
            .status(http_status)
            .header("content-type", "application/grpc");
        if let Some(status) = grpc_status {
            response = response
                .header("grpc-status", status)
                .header("grpc-message", "busy")
                .header("x-terminal", "rejected");
        }
        let send = respond
            .send_response(response.body(()).unwrap(), true)
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
    let mut message = HelloRequest::new();
    message.set_name("x".repeat(1024 * 1024));
    let mut request = Request::new(message);
    request.set_timeout(if http_status == 200 && grpc_status == Some("8") {
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
            (503, Some("8")),
        ] {
            assert_eq!(
                rejected(http_status, grpc_status, streaming).await.code(),
                Code::DeadlineExceeded,
                "http={http_status}, grpc={grpc_status:?}, streaming={streaming}"
            );
        }
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
