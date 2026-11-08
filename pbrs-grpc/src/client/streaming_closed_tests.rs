//! Unit coverage for closed producer transport failures and budget ownership.

use super::{
    ClosedUpload, closed_upload_transport_failure, prefer_closed_deadline, write_closed_upload,
};
use crate::compression::CompressionAlgorithm;
use crate::config::Wire;
use crate::hello::HelloRequest;
use crate::limits::{ByteBudgetTracker, MessageLimits};
use crate::rpc_cancel::RpcCancel;
use crate::status::{Code, Status};
use crate::stream::Framed;
use crate::transport::{ClientBuilder, SendRequest, SendStream, h2 as backend};
use std::time::Duration;

#[tokio::test]
async fn closed_upload_preserves_reset_reason_and_budget() {
    tokio::time::timeout(Duration::from_secs(30), async {
        for nonempty in [false, true] {
            for (reason, expected_reason) in [
                (h2::Reason::CANCEL, crate::transport::Reason::CANCEL),
                (
                    h2::Reason::REFUSED_STREAM,
                    crate::transport::Reason::REFUSED_STREAM,
                ),
                (h2::Reason::NO_ERROR, crate::transport::Reason::NO_ERROR),
            ] {
                let (client_io, peer_io) = tokio::io::duplex(64 * 1024);
                let peer = tokio::spawn(async move {
                    let mut connection = h2::server::handshake(peer_io)
                        .await
                        .expect("server handshake");
                    let (_, mut respond) = connection
                        .accept()
                        .await
                        .expect("request")
                        .expect("request headers");
                    respond.send_reset(reason);
                    while let Some(Ok(_)) = connection.accept().await {}
                });
                let (sender, connection) = backend::ClientBuilder::new()
                    .handshake(client_io)
                    .await
                    .expect("client handshake");
                let driver = tokio::spawn(async move { drop(connection.await) });
                let mut sender = sender.ready().await.expect("sender ready");
                let (response, mut send) = sender
                    .send_request(
                        http::Request::builder()
                            .uri("http://localhost/closed-empty")
                            .body(())
                            .expect("request"),
                        false,
                    )
                    .expect("open stream");
                let observed = std::future::poll_fn(|cx| send.poll_reset(cx))
                    .await
                    .expect("peer reset");
                assert_eq!(observed, expected_reason);

                let wire = Wire {
                    limits: MessageLimits::default(),
                    send_buffer: 1024,
                    accept_gzip: true,
                    send_codec: CompressionAlgorithm::Gzip,
                    gzip_level: 1,
                };
                let budget = ByteBudgetTracker::unlimited();
                let items = if nonempty {
                    vec![Ok(Framed::new(HelloRequest::new()))]
                } else {
                    Vec::new()
                };
                let outcome = write_closed_upload::<HelloRequest>(
                    &mut send,
                    items,
                    wire,
                    &budget,
                    &None,
                    None,
                );
                if reason == h2::Reason::NO_ERROR {
                    assert!(matches!(outcome, Ok(ClosedUpload::PeerFinished)));
                    assert_eq!(budget.allocated(), 0);
                } else {
                    let (status, permit) = match outcome {
                        Ok(ClosedUpload::TransportFailed { status, permit }) => (status, permit),
                        _ => panic!("expected a transport failure from END_STREAM"),
                    };
                    assert_eq!(budget.allocated(), if nonempty { 5 } else { 0 });
                    let status = closed_upload_transport_failure(
                        response,
                        &mut send,
                        &RpcCancel::new(),
                        None,
                        status,
                        permit,
                    );
                    assert_eq!(
                        status.code(),
                        if reason == h2::Reason::CANCEL {
                            Code::Cancelled
                        } else {
                            Code::Unavailable
                        }
                    );
                    assert_eq!(
                        status.is_transparent_retryable(),
                        reason == h2::Reason::REFUSED_STREAM
                    );
                    assert_eq!(budget.allocated(), 0);
                }
                driver.abort();
                peer.abort();
            }
        }
    })
    .await
    .expect("closed upload reset test timed out");
}

// This isolates the final preference rule; the integration test covers
// signals already ready at the response-probe boundary.
#[tokio::test(start_paused = true)]
async fn selected_application_error_survives_late_deadline() {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    tokio::time::advance(Duration::from_secs(2)).await;
    let status = prefer_closed_deadline(Status::resource_exhausted("peer"), Some(deadline));
    assert_eq!(status.code(), Code::ResourceExhausted);
    let status = prefer_closed_deadline(Status::unavailable("transport"), Some(deadline));
    assert_eq!(status.code(), Code::DeadlineExceeded);
    let status = prefer_closed_deadline(Status::cancelled(), Some(deadline));
    assert_eq!(status.code(), Code::DeadlineExceeded);
}
