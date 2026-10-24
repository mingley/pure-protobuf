//! Independent registry-h2 checks for the surviving terminal response owner.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_types,
    reason = "bounded independent protocol regression assertions"
)]

use super::{DeadlineEnding, DeadlineResponse, SendResponse};
use crate::transport::{SendResponse as _, SendStream as _};
use bytes::Bytes;
use http::{HeaderMap, Response};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("independent HTTP/2 exchange completed")
}

fn headers(status: Option<&str>) -> Response<()> {
    let mut response = Response::builder()
        .header("content-type", "application/grpc")
        .body(())
        .unwrap();
    if let Some(status) = status {
        response
            .headers_mut()
            .insert("grpc-status", status.parse().unwrap());
    }
    response
}

fn trailers(status: &str) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert("grpc-status", status.parse().unwrap());
    map
}

struct Peer {
    requests: h2::client::SendRequest<Bytes>,
    accepted: mpsc::UnboundedReceiver<crate::h2_backend::server::SendResponse<Bytes>>,
    server: tokio::task::JoinHandle<()>,
    client: tokio::task::JoinHandle<()>,
    flush: Arc<FlushGate>,
}

struct FlushGate {
    paused: AtomicBool,
    blocked: AtomicBool,
    wake: atomic_waker::AtomicWaker,
}

impl FlushGate {
    fn pause(&self) {
        self.blocked.store(false, Ordering::Release);
        self.paused.store(true, Ordering::Release);
    }

    fn release(&self) {
        self.paused.store(false, Ordering::Release);
        self.wake.wake();
    }
}

struct GatedIo {
    inner: tokio::io::DuplexStream,
    flush: Arc<FlushGate>,
}

impl AsyncRead for GatedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for GatedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.flush.paused.load(Ordering::Acquire) {
            self.flush.wake.register(cx.waker());
            if self.flush.paused.load(Ordering::Acquire) {
                self.flush.blocked.store(true, Ordering::Release);
                return Poll::Pending;
            }
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Peer {
    async fn new(window: u32) -> Self {
        let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
        let (accepted_tx, accepted) = mpsc::unbounded_channel();
        let flush = Arc::new(FlushGate {
            paused: AtomicBool::new(false),
            blocked: AtomicBool::new(false),
            wake: atomic_waker::AtomicWaker::new(),
        });
        let server_io = GatedIo {
            inner: server_io,
            flush: Arc::clone(&flush),
        };
        let server = tokio::spawn(async move {
            let mut connection = crate::h2_backend::server::handshake(server_io)
                .await
                .unwrap();
            let mut requests = Vec::new();
            while let Some(accepted) = connection.accept().await {
                let (request, response) = accepted.unwrap();
                requests.push(request);
                if accepted_tx.send(response).is_err() {
                    break;
                }
            }
        });
        let (requests, connection) = h2::client::Builder::new()
            .initial_window_size(window)
            .handshake(client_io)
            .await
            .unwrap();
        let client = tokio::spawn(async move {
            let _connection_result = connection.await;
        });
        Self {
            requests,
            accepted,
            server,
            client,
            flush,
        }
    }

    async fn open(&mut self) -> (h2::client::ResponseFuture, SendResponse) {
        let mut requests = bounded(self.requests.clone().ready()).await.unwrap();
        let request = http::Request::builder()
            .method("POST")
            .uri("http://localhost/deadline.Test/Call")
            .header("content-type", "application/grpc")
            .body(())
            .unwrap();
        let (response, _upload) = requests.send_request(request, true).unwrap();
        let accepted = bounded(self.accepted.recv()).await.unwrap();
        (response, SendResponse(accepted))
    }

    async fn recover(&mut self) {
        let (response, mut respond) = self.open().await;
        drop(respond.send_response(headers(Some("0")), true).unwrap());
        let response = bounded(response).await.unwrap();
        assert_eq!(response.headers()["grpc-status"], "0");
        assert!(response.body().is_end_stream());
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
        self.client.abort();
    }
}

fn finish_native(owner: &mut DeadlineResponse) -> Result<bool, super::Error> {
    owner.finish(
        headers(Some("4")),
        trailers("4"),
        None,
        DeadlineEnding::Trailers,
    )
}

#[cfg(feature = "tonic")]
async fn assert_tonic_partial_error_replay(data: Bytes, trailers: HeaderMap) {
    struct UnexpectedMessage(Arc<AtomicU8>);
    impl tonic::codec::Decoder for UnexpectedMessage {
        type Item = ();
        type Error = tonic::Status;

        fn decode(
            &mut self,
            _buffer: &mut tonic::codec::DecodeBuf<'_>,
        ) -> Result<Option<Self::Item>, Self::Error> {
            self.0.store(1, Ordering::Release);
            Err(tonic::Status::internal(
                "an incomplete message reached the decoder",
            ))
        }
    }

    // Replay the actual captured wire DATA and trailers through Tonic's codec.
    // This is an oracle replay, not a second live Tonic transport connection.
    let frames = futures_util::stream::iter([
        Ok::<_, std::convert::Infallible>(http_body::Frame::data(data)),
        Ok(http_body::Frame::trailers(trailers)),
    ]);
    let decoded = Arc::new(AtomicU8::new(0));
    let mut response = tonic::Streaming::new_response(
        UnexpectedMessage(Arc::clone(&decoded)),
        http_body_util::StreamBody::new(frames),
        http::StatusCode::OK,
        None,
        None,
    );
    assert_eq!(
        bounded(response.message()).await.unwrap_err().code(),
        tonic::Code::DeadlineExceeded
    );
    assert_eq!(decoded.load(Ordering::Acquire), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_response_preheader_survives_application_drop_and_preserves_completed_result() {
    let mut peer = Peer::new(0).await;
    let (response, respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    drop(respond);
    assert!(finish_native(&mut owner).unwrap());
    assert!(!finish_native(&mut owner).unwrap());
    let response = bounded(response).await.unwrap();
    assert_eq!(response.headers()["grpc-status"], "4");
    assert!(response.body().is_end_stream());

    let (response, mut respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    drop(respond.send_response(headers(Some("0")), true).unwrap());
    // No await intervenes: successful END_STREAM is queued, not necessarily
    // written. A later timer must still leave that committed result alone.
    assert!(!finish_native(&mut owner).unwrap());
    assert_eq!(
        bounded(response).await.unwrap().headers()["grpc-status"],
        "0"
    );
    peer.recover().await;
}

struct Reenter {
    owner: Mutex<Option<DeadlineResponse>>,
    observed: Arc<AtomicU8>,
}

impl Reenter {
    fn probe(&self) {
        let owner = self.owner.lock().unwrap().take();
        let Some(mut owner) = owner else {
            return;
        };
        let (done, finished) = std::sync::mpsc::sync_channel(1);
        // A bounded OS-thread probe detects a held engine lock without making
        // the test itself permanently deadlock. It is a hang guard, not a
        // performance threshold. If the old path holds a lock, the callback
        // returns after the guard and lets that thread finish on unlock.
        let _worker = std::thread::spawn(move || {
            let result = matches!(finish_native(&mut owner), Ok(false));
            let _send_result = done.send(result);
        });
        let unlocked = finished.recv_timeout(Duration::from_secs(3)) == Ok(true);
        self.observed
            .store(if unlocked { 1 } else { 2 }, Ordering::Release);
    }
}

struct OwnedBody {
    bytes: Vec<u8>,
    reenter: Reenter,
}

impl AsRef<[u8]> for OwnedBody {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for OwnedBody {
    fn drop(&mut self) {
        self.reenter.probe();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_response_zero_window_keeps_queued_initial_headers_and_drops_owners_unlocked() {
    let mut peer = Peer::new(0).await;
    let (response, mut respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    let observed = Arc::new(AtomicU8::new(0));
    let body = Bytes::from_owner(OwnedBody {
        bytes: vec![b'x'; 1024],
        reenter: Reenter {
            owner: Mutex::new(respond.deadline_response()),
            observed: Arc::clone(&observed),
        },
    });
    let mut send = respond.send_response(headers(None), false).unwrap();
    send.send_data(body, false).unwrap();
    drop(send);
    drop(respond);
    // Same-thread synchronous calls have not allowed the driver to flush the
    // initial response head. Native termination must preserve it while removing
    // DATA that can never pass the peer's zero receive window.
    assert!(finish_native(&mut owner).unwrap());
    assert_eq!(observed.load(Ordering::Acquire), 1);
    let response = bounded(response).await.unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    let mut body = response.into_body();
    assert!(bounded(body.data()).await.is_none());
    assert_eq!(
        bounded(body.trailers()).await.unwrap().unwrap()["grpc-status"],
        "4"
    );
    peer.recover().await;
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_response_partial_message_discards_inflight_tail_before_error_trailers() {
    let mut peer = Peer::new(4096).await;
    let (response, mut respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    let observed = Arc::new(AtomicU8::new(0));
    let mut bytes = vec![b'x'; 16_389];
    bytes[..5].copy_from_slice(&[0, 0, 0, 64, 0]);
    let body = Bytes::from_owner(OwnedBody {
        bytes,
        reenter: Reenter {
            owner: Mutex::new(respond.deadline_response()),
            observed: Arc::clone(&observed),
        },
    });
    let mut send = respond.send_response(headers(None), false).unwrap();
    let response = bounded(response).await.unwrap();
    // Keep the codec's completed HTTP/2 DATA frame parked behind poll_flush.
    // This forces the unencoded application tail to remain in flight instead
    // of allowing it to be reclaimed into the ordinary pending queue first.
    peer.flush.pause();
    send.send_data(body, false).unwrap();
    let mut body = response.into_body();
    let first = bounded(body.data()).await.unwrap().unwrap();
    assert_eq!(first.len(), 4096);
    assert_eq!(&first[..5], &[0, 0, 0, 64, 0]);
    assert!(peer.flush.blocked.load(Ordering::Acquire));
    assert_eq!(observed.load(Ordering::Acquire), 0);
    // Deliberately retain the receive credit. Only HEADERS can now
    // pass; the rest of the application message must not precede the error.
    drop(send);
    drop(respond);
    assert!(finish_native(&mut owner).unwrap());
    assert_eq!(observed.load(Ordering::Acquire), 0);
    // A codec parked on socket poll_flush owns that DATA frame until the IO
    // waker makes progress. Release only this artificial IO gate; the peer's
    // exhausted HTTP/2 receive window remains withheld. The cancelled tail
    // must then be reclaimed without running its owner under engine locks.
    peer.flush.release();
    bounded(async {
        loop {
            match observed.load(Ordering::Acquire) {
                0 => tokio::task::yield_now().await,
                1 => break,
                _ => panic!("in-flight payload destructor ran under an engine lock"),
            }
        }
    })
    .await;
    assert!(bounded(body.data()).await.is_none());
    let actual_trailers = bounded(body.trailers()).await.unwrap().unwrap();
    assert_eq!(actual_trailers["grpc-status"], "4");
    #[cfg(feature = "tonic")]
    assert_tonic_partial_error_replay(first.clone(), actual_trailers).await;
    assert_eq!(observed.load(Ordering::Acquire), 1);
    body.flow_control().release_capacity(first.len()).unwrap();
    peer.recover().await;
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_response_data_ending_preserves_complete_queued_body_and_reset_is_explicit() {
    let mut peer = Peer::new(1024).await;
    let (response, mut respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    let mut send = respond.send_response(headers(None), false).unwrap();
    send.send_data(Bytes::from_static(b"complete"), false)
        .unwrap();
    drop(send);
    assert!(
        owner
            .finish(
                headers(Some("4")),
                trailers("4"),
                Some(Bytes::from_static(b"ending")),
                DeadlineEnding::Data,
            )
            .unwrap()
    );
    let mut body = bounded(response).await.unwrap().into_body();
    let mut actual = Vec::new();
    while let Some(chunk) = bounded(body.data()).await {
        actual.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(actual, b"completeending");

    let (response, mut respond) = peer.open().await;
    let mut owner = respond.deadline_response().unwrap();
    let mut send = respond.send_response(headers(None), false).unwrap();
    let mut body = bounded(response).await.unwrap().into_body();
    send.send_data(Bytes::from_static(b"unfinished"), false)
        .unwrap();
    assert!(
        owner
            .finish(
                headers(Some("4")),
                trailers("4"),
                None,
                DeadlineEnding::Reset
            )
            .unwrap()
    );
    let error = bounded(body.data()).await.unwrap().unwrap_err();
    assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
    peer.recover().await;
}

impl Wake for Reenter {
    fn wake(self: Arc<Self>) {
        self.probe();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_response_returns_reserved_credit_and_wakes_siblings_after_unlock() {
    let mut peer = Peer::new(65_535).await;
    let (first_response, mut first) = peer.open().await;
    let mut owner = first.deadline_response().unwrap();
    let reenter_owner = first.deadline_response();
    let mut first_send = first.send_response(headers(None), false).unwrap();
    first_send.reserve_capacity(65_535);
    assert_eq!(first_send.capacity(), 65_535);
    let (second_response, mut second) = peer.open().await;
    let mut second_send = second.send_response(headers(None), false).unwrap();
    second_send.reserve_capacity(1);
    let observed = Arc::new(AtomicU8::new(0));
    let waker = Waker::from(Arc::new(Reenter {
        owner: Mutex::new(reenter_owner),
        observed: Arc::clone(&observed),
    }));
    assert!(matches!(
        second_send.poll_capacity(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    assert!(finish_native(&mut owner).unwrap());
    assert_eq!(observed.load(Ordering::Acquire), 1);
    assert!(second_send.capacity() >= 1);
    second_send.send_trailers(trailers("0")).unwrap();
    let mut first_body = bounded(first_response).await.unwrap().into_body();
    assert!(bounded(first_body.data()).await.is_none());
    assert_eq!(
        bounded(first_body.trailers()).await.unwrap().unwrap()["grpc-status"],
        "4"
    );
    let mut second_body = bounded(second_response).await.unwrap().into_body();
    assert!(bounded(second_body.data()).await.is_none());
    assert_eq!(
        bounded(second_body.trailers()).await.unwrap().unwrap()["grpc-status"],
        "0"
    );
    peer.recover().await;
}

// Preparation only: append to transport/h2/deadline_response_tests.rs.
// Uses that module's independent Peer; no timeout or terminal owner is activated.

struct NormalSuccessProbe {
    send: Arc<Mutex<super::SendStream>>,
    drops: std::sync::atomic::AtomicUsize,
    observed: AtomicU8,
    worker: Mutex<Option<std::thread::JoinHandle<usize>>>,
}

impl NormalSuccessProbe {
    fn new(send: Arc<Mutex<super::SendStream>>) -> Arc<Self> {
        Arc::new(Self {
            send,
            drops: std::sync::atomic::AtomicUsize::new(0),
            observed: AtomicU8::new(0),
            worker: Mutex::new(None),
        })
    }

    fn on_drop(&self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
        let send = Arc::clone(&self.send);
        let (done, finished) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            // A read-only query locks the same engine; it neither finishes nor
            // resets the successful response. Main retains another send owner.
            let capacity = send.lock().unwrap().capacity();
            let _send_result = done.send(capacity);
            capacity
        });
        *self.worker.lock().unwrap() = Some(worker);
        // This is a finite deadlock detector, not a latency requirement. If the
        // engine is held, return after three seconds so it can release its lock
        // and the worker can complete. Do not join inside the destructor.
        let outcome = match finished.recv_timeout(Duration::from_secs(3)) {
            Ok(_) => 1,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => 2,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => 3,
        };
        self.observed.store(outcome, Ordering::Release);
    }

    async fn finish_worker(&self) {
        bounded(async {
            loop {
                let finished = self
                    .worker
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(std::thread::JoinHandle::is_finished);
                if finished {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        // is_finished above makes this join nonblocking. It also reports a
        // worker panic instead of leaving a detached lock probe behind.
        let worker = self.worker.lock().unwrap().take().unwrap();
        let _queried_capacity = worker.join().expect("capacity probe returned");
        assert_eq!(self.drops.load(Ordering::Acquire), 1);
    }
}

struct NormalSuccessBody {
    bytes: Vec<u8>,
    probe: Arc<NormalSuccessProbe>,
}

impl AsRef<[u8]> for NormalSuccessBody {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for NormalSuccessBody {
    fn drop(&mut self) {
        self.probe.on_drop();
    }
}

async fn ordinary_success_owner_case(payload_len: usize, unlocked_control: bool) {
    let mut peer = Peer::new(65_535).await;
    let (response, mut respond) = peer.open().await;
    let send = Arc::new(Mutex::new(
        respond.send_response(headers(None), false).unwrap(),
    ));
    let response = bounded(response).await.unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/grpc");
    let mut body = response.into_body();
    let probe = NormalSuccessProbe::new(Arc::clone(&send));
    let mut expected = vec![b'x'; payload_len];
    // One complete native uncompressed gRPC message, including its 5-byte head.
    let message_len = u32::try_from(payload_len - 5).unwrap();
    expected[0] = 0;
    expected[1..5].copy_from_slice(&message_len.to_be_bytes());
    let owned = Bytes::from_owner(NormalSuccessBody {
        bytes: expected.clone(),
        probe: Arc::clone(&probe),
    });
    if unlocked_control {
        // Same query, same live stream, but deliberately destroy this backing
        // owner outside engine work. Then send an ordinary successful response.
        drop(owned);
        assert_eq!(probe.observed.load(Ordering::Acquire), 1);
        let mut send = send.lock().unwrap();
        send.send_data(Bytes::copy_from_slice(&expected), false)
            .unwrap();
        send.send_trailers(trailers("0")).unwrap();
    } else {
        // Pause socket flush only. Writes remain enabled. With the existing
        // non-vectored GatedIo, 64 B is copied and 16,384 B is chained; the latter
        // occupies exactly one default-max HTTP/2 DATA frame with ample credit.
        peer.flush.pause();
        let mut send = send.lock().unwrap();
        send.send_data(owned, false).unwrap();
        send.send_trailers(trailers("0")).unwrap();
    }

    let first = bounded(body.data()).await.unwrap().unwrap();
    assert_eq!(first.as_ref(), expected.as_slice());
    if !unlocked_control {
        assert!(peer.flush.blocked.load(Ordering::Acquire));
        if payload_len == 16_384 {
            // Witness the fully written chained frame still held by the codec
            // behind poll_flush. Reclaim must happen after this gate opens.
            assert_eq!(probe.drops.load(Ordering::Acquire), 0);
            assert_eq!(probe.observed.load(Ordering::Acquire), 0);
        } else {
            // The small copied owner was retired while staging, before flush.
            assert_eq!(probe.drops.load(Ordering::Acquire), 1);
            assert_ne!(probe.observed.load(Ordering::Acquire), 0);
        }
        peer.flush.release();
    }
    body.flow_control().release_capacity(first.len()).unwrap();
    assert!(bounded(body.data()).await.is_none());
    assert_eq!(
        bounded(body.trailers()).await.unwrap().unwrap()["grpc-status"],
        "0"
    );
    probe.finish_worker().await;
    peer.recover().await;
    // Establish complete successful output, exactly-once destruction, worker
    // completion and connection recovery before the causal red/green assertion.
    let observed = probe.observed.load(Ordering::Acquire);
    eprintln!(
        "normal-success owner: payload={payload_len} control={unlocked_control} native_status=0 recovery=ok drops=1 reentry={observed} (1=returned, 2=watchdog, 3=disconnected)"
    );
    assert_eq!(
        observed, 1,
        "ordinary successful DATA owner could not query its engine during Drop"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn normal_success_owner_drop_unlocked_control() {
    ordinary_success_owner_case(64, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn normal_success_small_copied_owner_drop_reenters_unlocked() {
    ordinary_success_owner_case(64, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn normal_success_large_chained_tail_drop_reenters_unlocked() {
    ordinary_success_owner_case(16_384, false).await;
}
