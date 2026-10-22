//! Reset status controls use an independent registry h2 peer and real errors.

use super::{Code, Status};
use bytes::Bytes;
use std::error::Error as _;
use std::future::poll_fn;
use std::time::Duration;

struct ResetControl {
    error: Option<crate::h2_backend::Error>,
    send: crate::h2_backend::SendStream<Bytes>,
    _requests: crate::h2_backend::client::SendRequest<Bytes>,
    client_driver: tokio::task::JoinHandle<()>,
    peer_driver: tokio::task::JoinHandle<()>,
}

impl Drop for ResetControl {
    fn drop(&mut self) {
        self.client_driver.abort();
        self.peer_driver.abort();
    }
}

async fn reset_control(reason: u32) -> ResetControl {
    let (client_io, peer_io) = tokio::io::duplex(8192);
    let peer_driver = tokio::spawn(async move {
        let mut connection = ::h2::server::Builder::new()
            .handshake::<_, Bytes>(peer_io)
            .await
            .expect("independent peer handshake");
        let (_request, mut response) = connection
            .accept()
            .await
            .expect("request")
            .expect("valid headers");
        response.send_reset(::h2::Reason::from(reason));
        // Drive the connection until the reset is written. Keeping it alive
        // avoids mistaking a dropped peer for the reset under test.
        while connection.accept().await.is_some() {}
    });
    let (mut requests, connection) = crate::h2_backend::client::Builder::new()
        .handshake::<_, Bytes>(client_io)
        .await
        .expect("native client handshake");
    let client_driver = tokio::spawn(async move {
        connection.await.ok();
    });
    let request = http::Request::builder()
        .uri("http://localhost/reset-control")
        .body(())
        .expect("request headers");
    let (response, mut send) = requests.send_request(request, false).expect("open stream");
    let error = tokio::time::timeout(Duration::from_secs(2), response)
        .await
        .expect("bounded response reset")
        .expect_err("peer resets without response headers");
    assert!(error.is_reset());
    assert!(error.is_remote());
    assert_eq!(error.reason().map(u32::from), Some(reason));
    // Witness the stored reset before testing synchronous sends; this is not
    // a race-dependent sleep or a fabricated bare Reason conversion.
    let observed = tokio::time::timeout(Duration::from_secs(2), poll_fn(|cx| send.poll_reset(cx)))
        .await
        .expect("bounded send-side reset")
        .expect("stored reset reason");
    assert_eq!(u32::from(observed), reason);
    ResetControl {
        error: Some(error),
        send,
        _requests: requests,
        client_driver,
        peer_driver,
    }
}

fn assert_mapping(status: Status, reason: u32, expected: Code) {
    assert_eq!(status.code(), expected, "RST_STREAM reason {reason}");
    assert_eq!(status.is_transparent_retryable(), reason == 7);
    if reason == 7 {
        assert_eq!(
            status.transport_evidence(),
            Some(super::TransportEvidence::RefusedStream)
        );
    }
    let cause = status
        .source()
        .expect("reset cause retained")
        .downcast_ref::<crate::transport::Error>()
        .expect("typed transport cause");
    assert!(cause.is_reset());
    assert_eq!(cause.reason().map(u32::from), Some(reason));
}

#[tokio::test]
async fn real_peer_resets_keep_protocol_status_and_cause_on_reads_and_sends() {
    // gRPC PROTOCOL-HTTP2.md Errors table. STREAM_CLOSED has no application
    // mapping; HTTP_1_1_REQUIRED and extension values are unspecified.
    // FRAME_SIZE_ERROR is INTERNAL even though tonic 0.14.6 returns UNKNOWN.
    let cases = [
        (0, Code::Internal),
        (1, Code::Internal),
        (2, Code::Internal),
        (3, Code::Internal),
        (4, Code::Internal),
        (5, Code::Unknown),
        (6, Code::Internal),
        (7, Code::Unavailable),
        (8, Code::Cancelled),
        (9, Code::Internal),
        (10, Code::Internal),
        (11, Code::ResourceExhausted),
        (12, Code::PermissionDenied),
        (13, Code::Unknown),
        (u32::MAX, Code::Unknown),
    ];
    for (reason, expected) in cases {
        let mut control = reset_control(reason).await;
        let error = control.error.take().expect("one response reset");
        assert_mapping(
            crate::wire::frame_reader::h2_error(error.into()),
            reason,
            expected,
        );
        for (bytes, end) in [
            (Bytes::from_static(b"data after reset"), false),
            (Bytes::new(), true),
        ] {
            let error = control
                .send
                .send_data(bytes, end)
                .expect_err("send after witnessed reset");
            assert!(
                error.is_reset(),
                "stored reset must beat inactive-stream user error"
            );
            assert!(error.is_remote());
            assert_mapping(Status::from_h2_send(error), reason, expected);
        }
        for (bytes, end) in [
            (Bytes::from_static(b"try after reset"), false),
            (Bytes::new(), true),
        ] {
            let error = control
                .send
                .try_send_data(bytes, end)
                .expect("reset is an error, not a full send buffer")
                .expect_err("try-send after witnessed reset");
            assert!(error.is_reset());
            assert_mapping(Status::from_h2_send(error), reason, expected);
        }
        let error = control
            .send
            .send_trailers(http::HeaderMap::new())
            .expect_err("trailers after reset");
        assert!(error.is_reset());
        assert_mapping(Status::from_h2_send(error), reason, expected);
    }
}

#[tokio::test]
async fn clean_local_half_close_still_reports_user_misuse_as_internal() {
    let (io, _peer) = tokio::io::duplex(8192);
    let (mut requests, _unpolled_driver) = crate::h2_backend::client::Builder::new()
        .handshake::<_, Bytes>(io)
        .await
        .expect("client preface");
    let request = http::Request::builder()
        .uri("http://localhost/local-end")
        .body(())
        .expect("headers");
    let (_response, mut send) = requests.send_request(request, false).expect("stream");
    send.send_data(Bytes::new(), true)
        .expect("clean half-close");
    let error = send
        .send_data(Bytes::from_static(b"after local end"), false)
        .expect_err("local misuse");
    assert!(!error.is_reset());
    assert!(error.reason().is_none());
    let status = Status::from_h2_send(error);
    assert_eq!(status.code(), Code::Internal);
    assert!(!status.is_transport());
    assert!(!status.is_transparent_retryable());
    let error = send
        .try_send_data(Bytes::new(), false)
        .expect("not a full buffer")
        .expect_err("try-send after local end");
    assert!(!error.is_reset());
    assert_eq!(Status::from_h2_send(error).code(), Code::Internal);
    let error = send
        .send_trailers(http::HeaderMap::new())
        .expect_err("trailers after local end");
    assert!(!error.is_reset());
    assert_eq!(Status::from_h2_send(error).code(), Code::Internal);
}
