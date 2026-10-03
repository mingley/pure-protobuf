use crate::h2_backend as h2;
use bytes::Bytes;
use h2::client::SendRequest;
use http::Request;
use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

type Opened = (h2::client::ResponseFuture, h2::SendStream<Bytes>);

#[derive(Debug)]
struct Frame {
    kind: u8,
    flags: u8,
    id: u32,
    payload: Vec<u8>,
}

async fn frame(peer: &mut DuplexStream) -> Frame {
    let mut header = [0; 9];
    peer.read_exact(&mut header).await.unwrap();
    let length =
        usize::from(header[0]) * 65536 + usize::from(header[1]) * 256 + usize::from(header[2]);
    assert!(length <= 16384);
    let mut payload = vec![0; length];
    peer.read_exact(&mut payload).await.unwrap();
    Frame {
        kind: header[3],
        flags: header[4],
        id: u32::from_be_bytes(header[5..9].try_into().unwrap()) & 0x7fff_ffff,
        payload,
    }
}

async fn settings(peer: &mut DuplexStream, limit: u32) {
    let mut bytes = vec![0, 0, 6, 4, 0, 0, 0, 0, 0, 0, 3];
    bytes.extend_from_slice(&limit.to_be_bytes());
    peer.write_all(&bytes).await.unwrap();
    peer.flush().await.unwrap();
    loop {
        let next = frame(peer).await;
        if next.kind == 4 && next.flags == 1 {
            break;
        }
    }
}

async fn reset(peer: &mut DuplexStream, id: u32) {
    let mut bytes = vec![0, 0, 4, 3, 0];
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(&8u32.to_be_bytes());
    peer.write_all(&bytes).await.unwrap();
    peer.flush().await.unwrap();
}

async fn quiet(peer: &mut DuplexStream) {
    while let Ok(next) = tokio::time::timeout(Duration::from_millis(30), frame(peer)).await {
        assert_eq!(next.id, 0, "unexpected stream frame: {next:?}");
    }
}

async fn headers(peer: &mut DuplexStream) -> u32 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let next = frame(peer).await;
            if next.kind == 1 {
                return next.id;
            }
        }
    })
    .await
    .expect("HEADERS stalled")
}

async fn connection(
    limit: Option<u32>,
) -> (
    SendRequest<Bytes>,
    tokio::task::JoinHandle<()>,
    DuplexStream,
) {
    let (io, mut peer) = tokio::io::duplex(65536);
    let (send, conn) = h2::client::Builder::new()
        .initial_max_send_streams(0)
        .handshake(io)
        .await
        .unwrap();
    let driver = tokio::spawn(async move {
        let _ = conn.await;
    });
    let mut preface = [0; 24];
    peer.read_exact(&mut preface).await.unwrap();
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    assert_eq!(frame(&mut peer).await.kind, 4);
    if let Some(limit) = limit {
        settings(&mut peer, limit).await;
    }
    (send, driver, peer)
}

fn request() -> Request<()> {
    Request::builder()
        .uri("http://localhost/probe")
        .body(())
        .unwrap()
}

async fn pending_request(
    send: &SendRequest<Bytes>,
) -> tokio::task::JoinHandle<Result<Opened, h2::Error>> {
    let mut send = send.clone();
    let (armed, wait) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut armed = Some(armed);
        let mut pending = Box::pin(send.send_request_when_ready(request(), true));
        std::future::poll_fn(|cx| {
            let outcome = pending.as_mut().poll(cx);
            if outcome.is_pending() {
                if let Some(armed) = armed.take() {
                    armed.send(()).unwrap();
                }
            }
            outcome
        })
        .await
    });
    wait.await.expect("future did not register while full");
    task
}

struct DropGuard(Arc<AtomicUsize>);
impl Drop for DropGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn expired_pending_request_is_untouched_and_never_allocates_stream() {
    let (mut send, driver, mut peer) = connection(Some(1)).await;
    let first = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut second = request();
    second
        .extensions_mut()
        .insert(Arc::new(DropGuard(dropped.clone())));
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            send.send_request_when_ready(second, true)
        )
        .await
        .is_err()
    );
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "pending request retained after deadline"
    );
    settings(&mut peer, 2).await;
    quiet(&mut peer).await;
    let next = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(
        headers(&mut peer).await,
        3,
        "expired request allocated a stream ID"
    );
    drop((first, next));
    driver.abort();
}

#[tokio::test(start_paused = true)]
async fn initial_settings_zero_and_growth_wake_pending_request() {
    let (send, driver, mut peer) = connection(None).await;
    let task = pending_request(&send).await;
    quiet(&mut peer).await;
    settings(&mut peer, 0).await;
    quiet(&mut peer).await;
    assert!(!task.is_finished());
    settings(&mut peer, 1).await;
    let opened = task.await.unwrap().unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    drop(opened);
    driver.abort();
}

#[tokio::test(start_paused = true)]
async fn shrink_below_active_count_does_not_admit_another_request() {
    let (mut send, driver, mut peer) = connection(Some(2)).await;
    let first = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    let second = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 3);
    let task = pending_request(&send).await;
    settings(&mut peer, 1).await;
    reset(&mut peer, 1).await;
    quiet(&mut peer).await;
    assert!(!task.is_finished(), "one active stream still consumes cap1");
    reset(&mut peer, 3).await;
    let next = task.await.unwrap().unwrap();
    assert_eq!(headers(&mut peer).await, 5);
    drop((first, second, next));
    driver.abort();
}

#[tokio::test(start_paused = true)]
async fn many_waiters_wake_without_oversubscribing_capacity() {
    let (mut send, driver, mut peer) = connection(Some(1)).await;
    let first = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    let mut tasks = Vec::new();
    for _ in 0..8 {
        tasks.push(pending_request(&send).await);
    }
    settings(&mut peer, 3).await;
    let mut ids = vec![headers(&mut peer).await, headers(&mut peer).await];
    quiet(&mut peer).await;
    assert_eq!(tasks.iter().filter(|t| t.is_finished()).count(), 2);
    settings(&mut peer, 9).await;
    for _ in 0..6 {
        ids.push(headers(&mut peer).await);
    }
    ids.sort_unstable();
    assert_eq!(ids, (3..=17).step_by(2).collect::<Vec<_>>());
    let mut opened = vec![first];
    for task in tasks {
        opened.push(task.await.unwrap().unwrap());
    }
    drop(opened);
    driver.abort();
}

#[tokio::test(start_paused = true)]
async fn connection_death_wakes_even_a_zero_capacity_waiter() {
    let (send, driver, peer) = connection(Some(0)).await;
    let pending = pending_request(&send).await;
    drop(peer);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    driver.abort();
}

struct WakeGuard;
#[allow(
    clippy::manual_noop_waker,
    reason = "unique Arc ownership measures canceled task retention; a static noop waker cannot provide that oracle"
)]
impl Wake for WakeGuard {
    fn wake(self: Arc<Self>) {}
}

#[tokio::test(start_paused = true)]
async fn canceled_future_releases_task_waker_while_sender_remains_live() {
    let (mut send, driver, mut peer) = connection(Some(0)).await;
    let waker = Arc::new(WakeGuard);
    let weak = Arc::downgrade(&waker);
    let waker = Waker::from(waker);
    let mut pending = Box::pin(send.send_request_when_ready(request(), true));
    {
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(pending.as_mut().poll(&mut cx), Poll::Pending));
    }
    drop(waker);
    drop(pending);
    assert!(
        weak.upgrade().is_none(),
        "canceled registration retained its task"
    );
    settings(&mut peer, 1).await;
    let opened = send.send_request_when_ready(request(), true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    drop(opened);
    driver.abort();
}

#[tokio::test(start_paused = true)]
async fn extension_drop_can_reenter_sender_without_deadlocking_stream_lock() {
    let (mut send, driver, mut peer) = connection(Some(1)).await;
    let mut req = request();
    req.extensions_mut().insert(Arc::new(send.clone()));
    let opened = send.send_request_when_ready(req, true).await.unwrap();
    assert_eq!(headers(&mut peer).await, 1);
    drop(opened);
    driver.abort();
}

#[path = "admission_additional_tests.rs"]
mod additional;
