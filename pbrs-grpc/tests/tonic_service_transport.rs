//! Native transport for opaque tonic services: policy and cancellation proof.

#![cfg(feature = "tonic")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "integration tests"
)]

use bytes::{Bytes, BytesMut};
use http::{Request, Response};
use http_body::{Body, Frame};
use pbrs_grpc::tonic_server::{TonicServer, TonicServerExt};
use pbrs_grpc::{Server, ServerConfig};
use std::convert::Infallible;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::Notify;
use tonic::body::Body as TonicBody;
use tonic::server::NamedService;
use tower::Service;

#[derive(Clone, Copy)]
enum Mode {
    Echo,
    PendingReady,
    PendingHandler,
    PendingBody,
    DetachedInput,
    BodyError,
}

#[derive(Default)]
struct State {
    ready: AtomicUsize,
    calls: AtomicUsize,
    entered: Notify,
    dropped: Notify,
    upload_stopped: Notify,
    metadata: AtomicUsize,
}

#[derive(Clone)]
struct Probe {
    mode: Mode,
    state: Arc<State>,
}

impl NamedService for Probe {
    const NAME: &'static str = "test.Opaque";
}

#[derive(Clone)]
struct KernelMarker;

struct Observer;
impl pbrs_grpc::telemetry::LifecycleObserver for Observer {}

struct Dropped(Arc<State>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.dropped.notify_one();
    }
}

impl Service<Request<TonicBody>> for Probe {
    type Response = Response<TonicBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        self.state.ready.fetch_add(1, Ordering::SeqCst);
        if matches!(self.mode, Mode::PendingReady) {
            self.state.entered.notify_one();
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn call(&mut self, request: Request<TonicBody>) -> Self::Future {
        self.state.calls.fetch_add(1, Ordering::SeqCst);
        if request.headers().get("authorization").is_none()
            && request.headers().get_all("x-new").iter().count() == 1
            && request
                .headers()
                .get("x-new")
                .is_some_and(|v| v == "kernel")
            && request.extensions().get::<KernelMarker>().is_some()
        {
            self.state.metadata.fetch_add(1, Ordering::SeqCst);
        }
        let mode = self.mode;
        let state = self.state.clone();
        Box::pin(async move {
            state.entered.notify_one();
            let body = match mode {
                Mode::PendingHandler => {
                    let _guard = Dropped(state);
                    std::future::pending().await
                }
                Mode::PendingBody => TonicBody::new(PendingBody(Dropped(state))),
                Mode::DetachedInput => {
                    let reader_state = state.clone();
                    let mut input = request.into_body();
                    drop(tokio::spawn(async move {
                        let frame = poll_fn(|cx| Pin::new(&mut input).poll_frame(cx)).await;
                        assert!(
                            matches!(frame, Some(Err(status)) if status.code() == tonic::Code::Cancelled)
                        );
                        reader_state.upload_stopped.notify_one();
                    }));
                    TonicBody::new(PendingBody(Dropped(state)))
                }
                Mode::BodyError => TonicBody::new(ErrorBody(false)),
                Mode::Echo | Mode::PendingReady => TonicBody::new(EchoBody {
                    inner: request.into_body(),
                    done: false,
                }),
            };
            Ok(Response::builder()
                .header("content-type", "application/grpc")
                .header("x-initial", "unchanged")
                .body(body)
                .expect("response"))
        })
    }
}

struct PendingBody(Dropped);
impl Body for PendingBody {
    type Data = Bytes;
    type Error = tonic::Status;
    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let _keep_guard = &self.0;
        Poll::Pending
    }
}

struct ErrorBody(bool);
impl Body for ErrorBody {
    type Data = Bytes;
    type Error = tonic::Status;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if std::mem::replace(&mut self.0, true) {
            return Poll::Ready(None);
        }
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-terminal", "local".parse().expect("metadata"));
        Poll::Ready(Some(Err(tonic::Status::with_details_and_metadata(
            tonic::Code::PermissionDenied,
            "local producer error",
            Bytes::from_static(b"details"),
            metadata,
        ))))
    }
}

struct EchoBody {
    inner: TonicBody,
    done: bool,
}
impl Body for EchoBody {
    type Data = Bytes;
    type Error = tonic::Status;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.inner).poll_frame(cx) {
            Poll::Ready(None) => {
                self.done = true;
                let mut trailers = http::HeaderMap::new();
                trailers.insert("grpc-status", "7".parse().expect("status"));
                trailers.insert("grpc-message", "peer%20terminal".parse().expect("message"));
                trailers.insert("x-terminal", "unchanged".parse().expect("metadata"));
                Poll::Ready(Some(Ok(Frame::trailers(trailers))))
            }
            frame => frame,
        }
    }
}

struct Guard(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Guard {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

fn server(mode: Mode) -> (Server<TonicServer<Probe>>, Arc<State>) {
    let state = Arc::new(State::default());
    (
        Server::new(
            Probe {
                mode,
                state: state.clone(),
            }
            .into_pbrs_service(),
        ),
        state,
    )
}

async fn connect(server: Server<TonicServer<Probe>>) -> (h2::client::SendRequest<Bytes>, Guard) {
    let (client, io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        server.serve_connection(io).await.expect("server");
    });
    let (client, connection) = h2::client::handshake(client).await.expect("handshake");
    let driver = tokio::spawn(async move {
        connection.await.ok();
    });
    (client, Guard(vec![server, driver]))
}

fn request(timeout: Option<&str>) -> Request<()> {
    let mut request = Request::builder()
        .method("POST")
        .uri("http://localhost/test.Opaque/Call")
        .header("content-type", "application/grpc")
        .body(())
        .expect("request");
    if let Some(timeout) = timeout {
        request
            .headers_mut()
            .insert("grpc-timeout", timeout.parse().expect("timeout"));
    }
    request
}

async fn notified(notify: &Notify) {
    tokio::time::timeout(Duration::from_secs(2), notify.notified())
        .await
        .expect("notification");
}

#[tokio::test]
async fn forwards_opaque_data_metadata_and_terminal_trailers() {
    let (server, state) = server(Mode::Echo);
    let (mut client, _guard) = connect(server).await;
    let (response, mut send) = client.send_request(request(None), false).expect("request");
    let bytes = Bytes::from_static(b"\x00\x00\x00\x00\x03abc");
    send.send_data(bytes.clone(), true).expect("upload");
    let response = response.await.expect("response");
    assert_eq!(
        response.headers().get("x-initial").expect("metadata"),
        "unchanged"
    );
    let mut body = response.into_body();
    let mut got = BytesMut::new();
    while let Some(data) = body.data().await {
        let data = data.expect("data");
        body.flow_control()
            .release_capacity(data.len())
            .expect("release");
        got.extend_from_slice(&data);
    }
    assert_eq!(got.freeze(), bytes);
    let trailers = body.trailers().await.expect("trailers").expect("terminal");
    assert_eq!(trailers.get("grpc-status").expect("status"), "7");
    assert_eq!(
        trailers.get("grpc-message").expect("message"),
        "peer%20terminal"
    );
    assert_eq!(trailers.get("x-terminal").expect("metadata"), "unchanged");
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordinary_tower_layer_retains_generated_service_routing_name() {
    let state = Arc::new(State::default());
    let service = Probe {
        mode: Mode::Echo,
        state: state.clone(),
    }
    .into_pbrs_service()
    .layer(tower::limit::ConcurrencyLimitLayer::new(1));
    let (client, io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        Server::new(service)
            .serve_connection(io)
            .await
            .expect("server");
    });
    let (mut client, connection) = h2::client::handshake(client).await.expect("handshake");
    let driver = tokio::spawn(async move {
        connection.await.ok();
    });
    let _guard = Guard(vec![server, driver]);
    let (response, _) = client.send_request(request(None), true).expect("request");
    let mut body = response.await.expect("response").into_body();
    assert!(body.data().await.is_none());
    assert_eq!(
        body.trailers()
            .await
            .expect("trailers")
            .expect("status")
            .get("grpc-status")
            .expect("status"),
        "7"
    );
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn opaque_native_policies_fail_before_tower_readiness_and_dispatch() {
    let defaults = ServerConfig::default();
    let variants = [
        defaults.max_decoding_message_size(8),
        defaults.max_encoding_message_size(8),
        defaults.accept_compressed(false),
        defaults.send_compressed(true),
        defaults.gzip_compression_level(8),
        defaults.compression_codec(pbrs_grpc::compression::Codec::Deflate),
        defaults.max_send_buffer_size(8),
    ];
    for config in variants {
        let (server, state) = server(Mode::Echo);
        let (mut client, _guard) = connect(server.config(config)).await;
        let (response, _) = client.send_request(request(None), true).expect("request");
        let response = response.await.expect("response");
        assert_eq!(response.headers().get("grpc-status").expect("status"), "9");
        assert_eq!(state.ready.load(Ordering::SeqCst), 0);
        assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    }
    for policy in 0..4 {
        let (server, state) = server(Mode::Echo);
        let server = match policy {
            0 => server.byte_budget(8),
            1 => server.on_response(|_: &mut pbrs_grpc::ResponseParts| Ok(())),
            2 => server.observer(Observer),
            _ => server.binary_logger(pbrs_grpc::binlog::BinaryLogger::new(
                pbrs_grpc::binlog::BinaryLogFilter::parse("*").expect("filter"),
                Arc::new(pbrs_grpc::binlog::VecSink::new()),
            )),
        };
        let (mut client, _guard) = connect(server).await;
        let (response, _) = client.send_request(request(None), true).expect("request");
        assert_eq!(
            response
                .await
                .expect("response")
                .headers()
                .get("grpc-status")
                .expect("status"),
            "9"
        );
        assert_eq!(state.ready.load(Ordering::SeqCst), 0);
        assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test]
async fn grpc_web_policy_is_rejected_before_tonic_readiness() {
    let (server, state) = server(Mode::Echo);
    let (mut client, _guard) = connect(server).await;
    let mut req = request(None);
    req.headers_mut().insert(
        "content-type",
        "application/grpc-web+proto"
            .parse()
            .expect("web content type"),
    );
    let (response, _) = client.send_request(req, true).expect("request");
    let mut body = response.await.expect("response").into_body();
    let data = body.data().await.expect("web trailers").expect("data");
    assert!(
        data.windows(b"grpc-status: 9".len())
            .any(|bytes| bytes == b"grpc-status: 9")
    );
    assert_eq!(state.ready.load(Ordering::SeqCst), 0);
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_security_interceptor_mutations_and_extensions_reach_tonic_service() {
    let (server, state) = server(Mode::Echo);
    let (mut client, _guard) = connect(server.intercept(|rpc: &mut pbrs_grpc::Rpc| {
        rpc.metadata_mut().remove("authorization");
        rpc.metadata_mut().set("x-new", "kernel")?;
        rpc.extensions_mut().insert(KernelMarker);
        Ok(())
    }))
    .await;
    let mut req = request(None);
    req.headers_mut()
        .insert("authorization", "private".parse().expect("auth"));
    req.headers_mut()
        .append("x-new", "original".parse().expect("metadata"));
    let (response, _) = client.send_request(req, true).expect("request");
    let mut body = response.await.expect("response").into_body();
    while body.data().await.is_some() {}
    assert_eq!(state.metadata.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn reset_before_response_headers_drops_pending_handler() {
    let (server, state) = server(Mode::PendingHandler);
    let (mut client, _guard) = connect(server).await;
    let (response, mut send) = client.send_request(request(None), true).expect("request");
    notified(&state.entered).await;
    send.send_reset(h2::Reason::CANCEL);
    notified(&state.dropped).await;
    drop(response);
}

#[tokio::test]
async fn reset_after_response_headers_drops_pending_body() {
    let (server, state) = server(Mode::PendingBody);
    let (mut client, _guard) = connect(server).await;
    let (response, mut send) = client.send_request(request(None), true).expect("request");
    let response = response.await.expect("headers");
    send.send_reset(h2::Reason::CANCEL);
    notified(&state.dropped).await;
    drop(response);
}

#[tokio::test]
async fn server_deadline_covers_readiness_and_pending_handler() {
    for mode in [Mode::PendingReady, Mode::PendingHandler] {
        let (server, state) = server(mode);
        let server = server.config(ServerConfig::default().timeout(Duration::from_millis(20)));
        let (mut client, _guard) = connect(server).await;
        let (response, _) = client
            .send_request(request(Some("1S")), true)
            .expect("request");
        let response = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .expect("bounded")
            .expect("response");
        assert_eq!(response.headers().get("grpc-status").expect("status"), "4");
        if matches!(mode, Mode::PendingHandler) {
            notified(&state.dropped).await;
        } else {
            assert_eq!(state.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn peer_deadline_covers_response_body_and_drops_producer() {
    let (server, state) = server(Mode::PendingBody);
    let (mut client, _guard) = connect(server).await;
    let (response, _) = client
        .send_request(request(Some("20m")), true)
        .expect("request");
    let mut body = response.await.expect("headers").into_body();
    assert!(body.data().await.is_none());
    let trailers = body.trailers().await.expect("trailers").expect("status");
    assert_eq!(trailers.get("grpc-status").expect("status"), "4");
    notified(&state.dropped).await;
}

#[tokio::test]
async fn deadline_wakes_detached_incoming_body_reader_with_open_upload() {
    let (server, state) = server(Mode::DetachedInput);
    let (mut client, _guard) = connect(server).await;
    let (response, _upload) = client
        .send_request(request(Some("20m")), false)
        .expect("open upload");
    let mut body = response.await.expect("headers").into_body();
    assert!(body.data().await.is_none());
    assert_eq!(
        body.trailers()
            .await
            .expect("trailers")
            .expect("status")
            .get("grpc-status")
            .expect("status"),
        "4"
    );
    notified(&state.dropped).await;
    notified(&state.upload_stopped).await;
}

#[tokio::test]
async fn local_response_body_status_preserves_details_and_metadata() {
    let (server, _) = server(Mode::BodyError);
    let (mut client, _guard) = connect(server).await;
    let (response, _) = client.send_request(request(None), true).expect("request");
    let mut body = response.await.expect("headers").into_body();
    assert!(body.data().await.is_none());
    let trailers = body.trailers().await.expect("trailers").expect("status");
    assert_eq!(trailers.get("grpc-status").expect("status"), "7");
    assert_eq!(
        trailers.get("grpc-message").expect("message"),
        "local producer error"
    );
    assert_eq!(
        trailers.get("grpc-status-details-bin").expect("details"),
        "ZGV0YWlscw"
    );
    assert_eq!(trailers.get("x-terminal").expect("metadata"), "local");
}

#[tokio::test]
async fn native_concurrency_slot_lives_until_body_drain_and_releases_on_reset() {
    let (server, state) = server(Mode::PendingBody);
    let (mut client, _guard) =
        connect(server.config(ServerConfig::default().max_concurrent_rpcs(1))).await;
    let (response, mut send) = client.send_request(request(None), true).expect("request");
    let first = response.await.expect("headers");
    let (response, _) = client.send_request(request(None), true).expect("second");
    assert_eq!(
        response
            .await
            .expect("rejection")
            .headers()
            .get("grpc-status")
            .expect("status"),
        "8"
    );
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    send.send_reset(h2::Reason::CANCEL);
    notified(&state.dropped).await;
    drop(first);
    // Let the owning dispatch task release its admission permit after drop.
    tokio::task::yield_now().await;
    let (response, mut send) = client.send_request(request(None), true).expect("third");
    let response = response.await.expect("released slot");
    assert!(response.headers().get("grpc-status").is_none());
    assert_eq!(state.calls.load(Ordering::SeqCst), 2);
    send.send_reset(h2::Reason::CANCEL);
    drop(response);
}
