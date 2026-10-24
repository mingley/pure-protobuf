//! Server-owned deadlines must include service waits before shape selection.
//!
//! The peer speaks registry h2 directly. It never interprets grpc-timeout or
//! sends a deadline cancellation, so only the native server can end these calls.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_types,
    reason = "bounded independent-peer regression witnesses"
)]

use bytes::Bytes;
use pbrs_grpc::{
    HelloReply, HelloRequest, Request, Response, Rpc, Server, Service, Status, Streaming,
};
use std::future::{Future, poll_fn};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::watch;

struct Probe {
    entered: AtomicBool,
    dropped: AtomicUsize,
    handlers: AtomicUsize,
    deadlines: Mutex<Vec<Option<tokio::time::Instant>>>,
    gate: watch::Receiver<bool>,
}

struct Owner(Arc<Probe>);

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

struct Delayed {
    probe: Arc<Probe>,
}

impl Service for Delayed {
    const NAME: &'static str = "deadline.Probe";

    async fn call(&self, rpc: Rpc) {
        if rpc.method() == "Recovery" {
            rpc.unary(|_: Request<HelloRequest>| async {
                Ok::<_, Status>(Response::new(HelloReply::new()))
            })
            .await;
            return;
        }
        let _owner = Owner(Arc::clone(&self.probe));
        let mut gate = self.probe.gate.clone();
        self.probe.deadlines.lock().unwrap().push(rpc.deadline());
        if rpc.method() == "DelayedBidi" {
            let probe = Arc::clone(&self.probe);
            // Construction chooses the shape now; polling is deliberately held.
            let response =
                rpc.bidi_streaming(move |_: Request<Streaming<HelloRequest>>| async move {
                    probe.handlers.fetch_add(1, Ordering::SeqCst);
                    let (sender, stream) = Streaming::<HelloReply>::channel(1);
                    drop(sender);
                    Ok::<_, Status>(Response::new(stream))
                });
            self.probe.entered.store(true, Ordering::SeqCst);
            gate.wait_for(|open| *open).await.unwrap();
            response.await;
        } else {
            self.probe.entered.store(true, Ordering::SeqCst);
            gate.wait_for(|open| *open).await.unwrap();
            self.probe.deadlines.lock().unwrap().push(rpc.deadline());
            let probe = Arc::clone(&self.probe);
            rpc.unary(move |_: Request<HelloRequest>| async move {
                probe.handlers.fetch_add(1, Ordering::SeqCst);
                Ok::<_, Status>(Response::new(HelloReply::new()))
            })
            .await;
        }
    }
}

// A pending coordinator wakes itself instead of allowing Tokio's paused clock
// to advance automatically to the next timer. The wall clock is only a watchdog.
async fn drive<T>(future: impl Future<Output = T>) -> T {
    let started = std::time::Instant::now();
    tokio::pin!(future);
    poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        if result.is_pending() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "independent peer stalled"
            );
            cx.waker().wake_by_ref();
        }
        result
    })
    .await
}

async fn turn() {
    for _ in 0..128 {
        tokio::task::yield_now().await;
    }
}

struct Peer {
    send: h2::client::SendRequest<Bytes>,
    server: tokio::task::JoinHandle<()>,
    connection: tokio::task::JoinHandle<()>,
    probe: Arc<Probe>,
    gate: watch::Sender<bool>,
}

impl Peer {
    async fn new() -> Self {
        let (gate, receiver) = watch::channel(false);
        let probe = Arc::new(Probe {
            entered: AtomicBool::new(false),
            dropped: AtomicUsize::new(0),
            handlers: AtomicUsize::new(0),
            deadlines: Mutex::new(Vec::new()),
            gate: receiver,
        });
        let (client_io, server_io) = tokio::io::duplex(4096);
        let service = Delayed {
            probe: Arc::clone(&probe),
        };
        let server = tokio::spawn(async move {
            Server::new(service)
                .max_concurrent_rpcs(1)
                .serve_connection(server_io)
                .await
                .unwrap();
        });
        let (send, connection) = drive(h2::client::handshake(client_io)).await.unwrap();
        let connection = tokio::spawn(async move {
            connection.await.unwrap();
        });
        Self {
            send,
            server,
            connection,
            probe,
            gate,
        }
    }

    async fn request(&mut self, method: &str, timed: bool) -> h2::client::ResponseFuture {
        let mut request = http::Request::builder()
            .method("POST")
            .uri(format!("http://local/deadline.Probe/{method}"))
            .header("content-type", "application/grpc")
            .header("te", "trailers");
        if timed {
            request = request.header("grpc-timeout", "20m");
        }
        let request = request.body(()).unwrap();
        self.send = drive(self.send.clone().ready()).await.unwrap();
        let (response, mut body) = self.send.send_request(request, false).unwrap();
        body.send_data(Bytes::from_static(&[0, 0, 0, 0, 0]), true)
            .unwrap();
        response
    }

    async fn entered(&self) {
        drive(poll_fn(|_| {
            if self.probe.entered.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
        .await;
    }

    async fn recovery(&mut self) {
        let response = self.request("Recovery", false).await;
        assert_eq!(
            status(response).await.0,
            "0",
            "deadline must release the sole RPC permit"
        );
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        // Cleanup must never require releasing the service gate in production.
        // Closing it here only prevents a failed regression from leaking tasks.
        self.gate.send_replace(true);
        self.connection.abort();
        self.server.abort();
    }
}

async fn status(response: h2::client::ResponseFuture) -> (String, usize) {
    let response = drive(response)
        .await
        .expect("valid HTTP/2 response, not a reset");
    let initial = response.headers().get("grpc-status").cloned();
    let mut body = response.into_body();
    let mut bytes = 0;
    while let Some(data) = drive(body.data()).await {
        let data = data.expect("deadline trailers must not reset a live peer");
        bytes += data.len();
        body.flow_control().release_capacity(data.len()).unwrap();
    }
    let trailers = drive(body.trailers()).await.unwrap();
    let code = initial
        .or_else(|| trailers.and_then(|map| map.get("grpc-status").cloned()))
        .unwrap();
    (code.to_str().unwrap().to_owned(), bytes)
}

#[tokio::test(start_paused = true)]
async fn pre_shape_service_wait_expires_without_releasing_gate() {
    let mut peer = Peer::new().await;
    let response = peer.request("Hold", true).await;
    peer.entered().await;
    tokio::time::advance(Duration::from_millis(21)).await;
    turn().await;
    assert_eq!(
        peer.probe.dropped.load(Ordering::SeqCst),
        1,
        "expired pre-shape service owner must be destroyed"
    );
    assert_eq!(peer.probe.handlers.load(Ordering::SeqCst), 0);
    assert_eq!(status(response).await, ("4".to_owned(), 0));
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn elapsed_service_wait_does_not_renew_reported_deadline_or_enter_handler() {
    let mut peer = Peer::new().await;
    let response = peer.request("Hold", true).await;
    peer.entered().await;
    let accepted = peer
        .probe
        .deadlines
        .lock()
        .unwrap()
        .first()
        .copied()
        .expect("observed service admission deadline");
    tokio::time::advance(Duration::from_millis(21)).await;
    peer.gate.send_replace(true);
    turn().await;
    let deadlines = peer.probe.deadlines.lock().unwrap().clone();
    // The full outer owner may cancel before the second getter is reached.
    assert!(
        deadlines.iter().all(|at| *at == accepted),
        "Rpc::deadline must remain fixed after waiting"
    );
    assert_eq!(
        peer.probe.handlers.load(Ordering::SeqCst),
        0,
        "expired handler must never be polled"
    );
    assert_eq!(status(response).await, ("4".to_owned(), 0));
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn delayed_bidi_shape_first_poll_does_not_start_another_timeout() {
    let mut peer = Peer::new().await;
    let response = peer.request("DelayedBidi", true).await;
    peer.entered().await;
    tokio::time::advance(Duration::from_millis(21)).await;
    peer.gate.send_replace(true);
    turn().await;
    assert_eq!(
        peer.probe.handlers.load(Ordering::SeqCst),
        0,
        "constructed bidi future cannot renew expiry at first poll"
    );
    assert_eq!(status(response).await, ("4".to_owned(), 0));
    peer.recovery().await;
}
