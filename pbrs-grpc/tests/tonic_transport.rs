//! Opaque HTTP-body transport policy, lifetime and deadline regressions.

#![cfg(feature = "tonic")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration tests"
)]

use bytes::{Bytes, BytesMut};
use http::{HeaderMap, Request, Response};
use http_body::Body;
use http_body::Frame;
use http_body_util::Full;
use pbrs_grpc::{Channel, ChannelConfig, Code};
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tonic::body::Body as TonicBody;
use tower::ServiceExt;

fn request(path: &'static str, bytes: Bytes) -> Request<TonicBody> {
    Request::post(path)
        .header("content-type", "application/grpc")
        .body(TonicBody::new(Full::new(bytes)))
        .expect("request")
}

async fn body(mut body: TonicBody) -> (Bytes, Option<HeaderMap>) {
    let mut bytes = BytesMut::new();
    let mut trailers = None;
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.expect("body frame");
        match frame.into_data() {
            Ok(data) => bytes.extend_from_slice(&data),
            Err(frame) => trailers = frame.into_trailers().ok(),
        }
    }
    (bytes.freeze(), trailers)
}

async fn echo_peer(
    config: ChannelConfig,
) -> (Channel, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let (client, server) = tokio::io::duplex(128 * 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server).await.expect("handshake");
        while let Some(call) = connection.accept().await {
            let (request, mut respond) = call.expect("request");
            count.fetch_add(1, Ordering::SeqCst);
            drop(tokio::spawn(async move {
                assert_eq!(
                    request.uri().authority().expect("authority").as_str(),
                    "localhost"
                );
                let mut recv = request.into_body();
                let mut data = BytesMut::new();
                while let Some(chunk) = recv.data().await {
                    let chunk = chunk.expect("request data");
                    recv.flow_control()
                        .release_capacity(chunk.len())
                        .expect("release");
                    data.extend_from_slice(&chunk);
                }
                let mut send = respond
                    .send_response(
                        Response::builder()
                            .header("content-type", "application/grpc")
                            .body(())
                            .expect("response"),
                        false,
                    )
                    .expect("headers");
                send.send_data(data.freeze(), false).expect("data");
                let mut trailers = HeaderMap::new();
                trailers.insert("grpc-status", "7".parse().expect("status"));
                trailers.insert(
                    "grpc-message",
                    "denied%20by%20peer".parse().expect("message"),
                );
                trailers.insert("x-terminal", "retained".parse().expect("metadata"));
                send.send_trailers(trailers).expect("trailers");
            }));
        }
    });
    let channel = Channel::from_io_with(client, "localhost", config)
        .await
        .expect("channel");
    (channel, calls, peer)
}

#[tokio::test]
async fn preframed_data_and_terminal_trailers_pass_without_codec_changes() {
    let (channel, calls, peer) = echo_peer(ChannelConfig::default()).await;
    let wire = Bytes::from_static(b"\x00\x00\x00\x00\x04\x08\x80\x01\x00");
    let response = channel
        .oneshot(request("/test.Echo/Call", wire.clone()))
        .await
        .expect("call");
    let (returned, trailers) = body(response.into_body()).await;
    assert_eq!(returned, wire);
    let trailers = trailers.expect("trailers");
    assert_eq!(trailers.get("grpc-status").expect("status"), "7");
    assert_eq!(
        trailers.get("grpc-message").expect("message"),
        "denied%20by%20peer"
    );
    assert_eq!(trailers.get("x-terminal").expect("metadata"), "retained");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    peer.abort();
}

#[tokio::test]
async fn unsupported_native_security_and_resource_policies_fail_before_dispatch() {
    let (channel, calls, peer) = echo_peer(ChannelConfig::default()).await;
    let method =
        r#"{"methodConfig":[{"name":[{"service":"test.Echo"}],"maxResponseMessageBytes":8}]}"#;
    let variants = [
        channel.clone().intercept(|_: &mut pbrs_grpc::Outgoing| {
            Err(pbrs_grpc::Status::unauthenticated("native auth"))
        }),
        channel.clone().max_decoding_message_size(8),
        channel
            .clone()
            .on_response(|_: &mut pbrs_grpc::ResponseParts| Ok(())),
        channel.clone().max_encoding_message_size(8),
        channel.clone().byte_budget(8),
        channel.clone().accept_compressed(false),
        channel.clone().send_compressed(),
        channel
            .clone()
            .service_config(method)
            .expect("service config"),
    ];
    for channel in variants {
        let error = channel
            .oneshot(request("/test.Echo/Call", Bytes::new()))
            .await
            .expect_err("unsupported policy");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }
    let response = channel
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect("default call");
    body(response.into_body()).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the default request dispatched"
    );
    peer.abort();
}

#[tokio::test]
async fn dropping_response_releases_native_rpc_concurrency_slot() {
    let (channel, _, peer) = echo_peer(ChannelConfig::new().max_concurrent_rpcs(1)).await;
    let response = channel
        .clone()
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect("first call");
    let error = channel
        .clone()
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect_err("limit");
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    drop(response);
    let response = channel
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect("released slot");
    body(response.into_body()).await;
    peer.abort();
}

async fn stalled_peer(
    response_headers: bool,
) -> (
    Channel,
    tokio::sync::oneshot::Receiver<h2::Reason>,
    tokio::sync::oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (reset_tx, reset_rx) = tokio::sync::oneshot::channel();
    let (opened_tx, opened_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server).await.expect("handshake");
        let (request, mut respond) = connection.accept().await.expect("call").expect("request");
        opened_tx.send(()).ok();
        let watcher = tokio::spawn(async move {
            let mut recv = request.into_body();
            while let Some(chunk) = recv.data().await {
                let chunk = chunk.expect("data");
                recv.flow_control()
                    .release_capacity(chunk.len())
                    .expect("release");
            }
            let reason = if response_headers {
                let mut send = respond
                    .send_response(
                        Response::builder()
                            .header("content-type", "application/grpc")
                            .body(())
                            .expect("response"),
                        false,
                    )
                    .expect("headers");
                poll_fn(|cx| send.poll_reset(cx)).await.expect("reset")
            } else {
                poll_fn(|cx| respond.poll_reset(cx)).await.expect("reset")
            };
            reset_tx.send(reason).ok();
        });
        while connection.accept().await.is_some() {}
        watcher.await.ok();
    });
    let channel = Channel::from_io(client, "localhost")
        .await
        .expect("channel");
    (channel, reset_rx, opened_rx, peer)
}

#[tokio::test]
async fn grpc_timeout_covers_headers_and_resets_stream() {
    let (channel, reset, _opened, peer) = stalled_peer(false).await;
    let mut req = request("/test.Echo/Call", Bytes::new());
    req.headers_mut()
        .insert("grpc-timeout", "30m".parse().expect("timeout"));
    let error = channel.clone().oneshot(req).await.expect_err("deadline");
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), reset)
            .await
            .expect("reset timeout")
            .expect("reset"),
        h2::Reason::CANCEL
    );
    peer.abort();
}

#[tokio::test]
async fn channel_timeout_covers_stream_body_without_polling_and_resets_peer() {
    let (channel, reset, _opened, peer) = stalled_peer(true).await;
    let response = channel
        .timeout(Duration::from_millis(30))
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect("headers");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), reset)
            .await
            .expect("reset timeout")
            .expect("reset"),
        h2::Reason::CANCEL
    );
    let mut body = response.into_body();
    let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .expect("frame")
        .expect_err("deadline");
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
    peer.abort();
}

#[tokio::test]
async fn cancelling_future_before_headers_resets_peer() {
    let (channel, reset, opened, peer) = stalled_peer(false).await;
    let call = tokio::spawn(
        channel
            .clone()
            .oneshot(request("/test.Echo/Call", Bytes::new())),
    );
    tokio::time::timeout(Duration::from_secs(1), opened)
        .await
        .expect("headers timeout")
        .expect("headers reached peer");
    call.abort();
    call.await.ok();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), reset)
            .await
            .expect("reset timeout")
            .expect("reset"),
        h2::Reason::CANCEL
    );
    peer.abort();
}

#[tokio::test]
async fn dropping_response_resets_stalled_peer() {
    let (channel, reset, _opened, peer) = stalled_peer(true).await;
    let response = channel
        .clone()
        .oneshot(request("/test.Echo/Call", Bytes::new()))
        .await
        .expect("headers");
    drop(response);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), reset)
            .await
            .expect("reset timeout")
            .expect("reset"),
        h2::Reason::CANCEL
    );
    peer.abort();
}

struct ObservedUpload {
    data: Option<Bytes>,
    dropped: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Body for ObservedUpload {
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        Poll::Ready(self.data.take().map(|data| Ok(Frame::data(data))))
    }
}

impl Drop for ObservedUpload {
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            dropped.send(()).ok();
        }
    }
}

#[tokio::test]
async fn early_terminal_rejection_preserves_status_details_and_metadata_during_upload() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server).await.expect("handshake");
        let (request, mut respond) = connection.accept().await.expect("call").expect("request");
        let mut send = respond
            .send_response(
                Response::builder()
                    .header("content-type", "application/grpc")
                    .body(())
                    .expect("response"),
                false,
            )
            .expect("headers");
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", "7".parse().expect("status"));
        trailers.insert(
            "grpc-message",
            "early%20rejection".parse().expect("message"),
        );
        trailers.insert(
            "grpc-status-details-bin",
            "cHVibGljIGRldGFpbHM".parse().expect("details"),
        );
        trailers.insert("x-terminal", "preserved".parse().expect("metadata"));
        send.send_trailers(trailers).expect("trailers");
        // Closing the receive half aborts an upload too large for the peer's
        // unrefreshed window, after normal response headers and trailers.
        drop(request);
        drop(send);
        drop(respond);
        while connection.accept().await.is_some() {}
    });
    let channel = Channel::from_io(client, "localhost")
        .await
        .expect("channel");
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let request = Request::post("/test.Echo/Bidi")
        .header("content-type", "application/grpc")
        .body(TonicBody::new(ObservedUpload {
            data: Some(Bytes::from(vec![0; 2 * 1024 * 1024])),
            dropped: Some(dropped_tx),
        }))
        .expect("request");
    let response = channel
        .clone()
        .oneshot(request)
        .await
        .expect("initial headers");
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("upload termination timeout")
        .expect("upload terminated");
    // The upload error is ready before receiving is polled: it must not hide
    // the peer's already received terminal status block.
    let (data, trailers) = body(response.into_body()).await;
    assert!(data.is_empty());
    let trailers = trailers.expect("peer trailers");
    assert_eq!(trailers.get("grpc-status").expect("status"), "7");
    assert_eq!(
        trailers.get("grpc-message").expect("message"),
        "early%20rejection"
    );
    assert_eq!(
        trailers.get("grpc-status-details-bin").expect("details"),
        "cHVibGljIGRldGFpbHM"
    );
    assert_eq!(trailers.get("x-terminal").expect("metadata"), "preserved");
    peer.abort();
}

#[test]
fn tonic_feature_exposes_the_transport_contract() {
    fn assert_transport<
        T: tower::Service<
                Request<TonicBody>,
                Response = Response<TonicBody>,
                Error = tonic::Status,
            > + Clone
            + Send,
    >() {
    }
    assert_transport::<Channel>();
    assert_eq!(
        Code::DeadlineExceeded.to_i32(),
        tonic::Code::DeadlineExceeded as i32
    );
}
