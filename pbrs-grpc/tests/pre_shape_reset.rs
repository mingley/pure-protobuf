//! A peer reset must release a service that has not selected its call shape.
//!
//! Registry h2 supplies the peer; no client or server RPC timeout is configured.
#![allow(
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded raw-peer ownership and admission regressions"
)]

use bytes::Bytes;
use pbrs_grpc::telemetry::{CallLabels, CancellationEvent, CancellationReason, LifecycleObserver};
use pbrs_grpc::{Code, Router, Rpc, Server, Service, Status};
use std::future::{Future, pending};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

const WATCHDOG: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Probe {
    entered: Notify,
    dropped: Notify,
    drops: AtomicUsize,
    starts: AtomicUsize,
    ends: Mutex<Vec<Code>>,
    cancellations: Mutex<Vec<CancellationReason>>,
}

struct ServiceOwner(Arc<Probe>);

impl Drop for ServiceOwner {
    fn drop(&mut self) {
        self.0.drops.fetch_add(1, Ordering::SeqCst);
        self.0.dropped.notify_one();
    }
}

struct Parked(Arc<Probe>);

impl Service for Parked {
    const NAME: &'static str = "reset.BeforeShape";

    async fn call(&self, rpc: Rpc) {
        if rpc.method() == "Recovery" {
            // No response DATA or request body is needed for this admission proof.
            rpc.reject(Status::unimplemented("recovered"));
            return;
        }
        assert_eq!(rpc.deadline(), None, "the fixture must remain untimed");
        let _owner = ServiceOwner(Arc::clone(&self.0));
        self.0.entered.notify_one();
        pending::<()>().await;
        drop(rpc);
    }
}

struct Observer(Arc<Probe>);

impl LifecycleObserver for Observer {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        if call.method() == "Hold" {
            self.0.starts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _: Duration) {
        if call.method() == "Hold" {
            assert_eq!(
                self.0.drops.load(Ordering::SeqCst),
                1,
                "the polled application future must drop before its lifecycle owner"
            );
            self.0.ends.lock().unwrap().push(status.code());
        }
    }

    fn on_cancellation(&self, event: &CancellationEvent<'_>) {
        if event.call.method() == "Hold" {
            self.0.cancellations.lock().unwrap().push(event.reason);
        }
    }
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(WATCHDOG, future)
        .await
        .expect("independent peer watchdog")
}

struct Peer {
    send: h2::client::SendRequest<Bytes>,
    server: tokio::task::JoinHandle<()>,
    connection: tokio::task::JoinHandle<()>,
    probe: Arc<Probe>,
}

impl Peer {
    async fn new(routed: bool) -> Self {
        let probe = Arc::new(Probe::default());
        let service = Parked(Arc::clone(&probe));
        let observer = Arc::new(Observer(Arc::clone(&probe)));
        let (client_io, server_io) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            if routed {
                Router::new()
                    .add_service(service)
                    .observer(observer)
                    .max_concurrent_rpcs(1)
                    .serve_connection(server_io)
                    .await
                    .unwrap();
            } else {
                Server::new(service)
                    .observer(observer)
                    .max_concurrent_rpcs(1)
                    .serve_connection(server_io)
                    .await
                    .unwrap();
            }
        });
        let (send, connection) = bounded(h2::client::handshake(client_io)).await.unwrap();
        let connection = tokio::spawn(async move {
            connection.await.unwrap();
        });
        Self {
            send,
            server,
            connection,
            probe,
        }
    }

    async fn request(
        &mut self,
        method: &str,
        end_stream: bool,
    ) -> (h2::client::ResponseFuture, h2::SendStream<Bytes>) {
        self.send = bounded(self.send.clone().ready()).await.unwrap();
        self.send
            .send_request(
                http::Request::builder()
                    .method("POST")
                    .uri(format!("http://local/reset.BeforeShape/{method}"))
                    .header("content-type", "application/grpc")
                    .header("te", "trailers")
                    .body(())
                    .unwrap(),
                end_stream,
            )
            .unwrap()
    }

    async fn recovery(&mut self) -> String {
        let (response, _body) = self.request("Recovery", true).await;
        let response = bounded(response).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);
        let initial = response.headers().get("grpc-status").cloned();
        let mut body = response.into_body();
        while let Some(chunk) = bounded(body.data()).await {
            let chunk = chunk.unwrap();
            body.flow_control().release_capacity(chunk.len()).unwrap();
        }
        let trailers = bounded(body.trailers()).await.unwrap();
        initial
            .or_else(|| trailers.and_then(|mut map| map.remove("grpc-status")))
            .expect("explicit recovery grpc-status")
            .to_str()
            .unwrap()
            .to_owned()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        // Failed ownership regressions must not leave parked tasks behind.
        self.connection.abort();
        self.server.abort();
    }
}

async fn reset_before_shape_releases_call(routed: bool, end_stream: bool) {
    let mut peer = Peer::new(routed).await;
    let (response, mut body) = peer.request("Hold", end_stream).await;
    bounded(peer.probe.entered.notified()).await;
    assert_eq!(peer.probe.starts.load(Ordering::SeqCst), 1);
    assert_eq!(peer.probe.drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        peer.recovery().await,
        "8",
        "the pending call owns the sole slot"
    );

    assert!(peer.probe.ends.lock().unwrap().is_empty());
    assert!(peer.probe.cancellations.lock().unwrap().is_empty());
    assert!(!peer.connection.is_finished());

    body.send_reset(h2::Reason::CANCEL);
    let drop_observed = tokio::time::timeout(WATCHDOG, peer.probe.dropped.notified())
        .await
        .is_ok();
    let recovery = peer.recovery().await;
    let drops = peer.probe.drops.load(Ordering::SeqCst);
    let ends = peer.probe.ends.lock().unwrap().clone();
    let cancellations = peer.probe.cancellations.lock().unwrap().clone();
    eprintln!(
        "pre-shape reset witness: routed={routed} request_eos={end_stream} drop_observed={drop_observed} drops={drops} recovery={recovery} ends={ends:?} cancellations={cancellations:?}"
    );
    drop(response);
    assert!(
        drop_observed,
        "a peer reset must destroy the parked service"
    );
    assert_eq!(drops, 1, "the service owner is destroyed exactly once");
    assert_eq!(recovery, "12", "reset must release the sole RPC permit");
    assert_eq!(ends, [Code::Cancelled]);
    assert_eq!(cancellations, [CancellationReason::PeerReset]);
    assert!(
        !peer.connection.is_finished(),
        "the peer connection stays live"
    );
}

#[tokio::test]
async fn reset_before_shape_with_open_request_releases_call() {
    reset_before_shape_releases_call(false, false).await;
}

#[tokio::test]
async fn reset_before_shape_after_request_eos_releases_call() {
    reset_before_shape_releases_call(false, true).await;
}

#[tokio::test]
async fn reset_before_shape_in_router_releases_call() {
    reset_before_shape_releases_call(true, true).await;
}
