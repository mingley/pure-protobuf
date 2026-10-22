//! A failed upload task must terminate its HTTP/2 stream and release resources.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "independent HTTP/2 peer and deliberate application-panic regression assertions"
)]
use bytes::Bytes;
use pbrs_grpc::{
    ByteBudgetTracker, Channel, Code, CodecMessage, HelloReply, HelloRequest, Request, Status,
};
use std::future::{Future, poll_fn};
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::oneshot;

const GUARD: Duration = Duration::from_secs(2);

async fn guarded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(GUARD, future)
        .await
        .expect("upload task failed to terminate its stream")
}

struct PanicMessage {
    panicked: Arc<AtomicBool>,
    budget: ByteBudgetTracker,
}
impl CodecMessage for PanicMessage {
    fn encoded_len(&self) -> usize {
        1
    }
    fn encode_payload<W: pbrs::WireOut>(&self, _out: &mut W) -> Result<(), Status> {
        // The panic must occur after the upload owns a real budget charge.
        assert!(self.budget.allocated() > 0);
        assert!(self.budget.active_byte_permit_tokens() > 0);
        self.panicked.store(true, Ordering::SeqCst);
        panic!("deliberate application codec panic in spawned upload");
    }
    fn decode_payload(_payload: Bytes) -> Result<Self, Status> {
        Err(Status::internal(
            "PanicMessage is only an outbound test codec",
        ))
    }
    fn empty() -> Self {
        Self {
            panicked: Arc::new(AtomicBool::new(false)),
            budget: ByteBudgetTracker::with_limit(128 * 1024),
        }
    }
}

struct Peer {
    address: SocketAddr,
    ready: oneshot::Receiver<()>,
    accepted: oneshot::Receiver<()>,
    reset: oneshot::Receiver<h2::Reason>,
    task: tokio::task::JoinHandle<()>,
}

async fn peer(initial_headers: bool) -> Peer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, wait_ready) = oneshot::channel();
    let (accepted, wait_accepted) = oneshot::channel();
    let (reset, wait_reset) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let mut ping = connection.ping_pong().unwrap();
        // The ACK follows the peer's first-frame zero-window SETTINGS, so
        // no initial request credit can hide the blocked-upload lifecycle.
        tokio::select! {
            result = ping.ping(h2::Ping::opaque()) => { result.unwrap(); },
            result = connection.accept() => panic!("request before SETTINGS barrier: {result:?}"),
        }
        ready.send(()).unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        let mut body = request.into_body();
        poll_fn(|cx| {
            assert!(body.poll_data(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let response = initial_headers.then(|| {
            respond
                .send_response(
                    http::Response::builder()
                        .header("content-type", "application/grpc")
                        .header("x-initial", "observed")
                        .body(())
                        .unwrap(),
                    false,
                )
                .unwrap()
        });
        accepted.send(()).unwrap();
        let reason = {
            let read = async {
                match body.data().await {
                    Some(Err(error)) => error
                        .reason()
                        .expect("request must end with a reset reason"),
                    other => panic!("zero-credit request must reset, got {other:?}"),
                }
            };
            tokio::pin!(read);
            tokio::select! {
                reason = &mut read => reason,
                result = connection.accept() => panic!("new request before failed upload reset: {result:?}"),
            }
        };
        reset.send(reason).unwrap();
        // Both handles stayed alive until the actual client reset arrived.
        drop((body, response, respond));
        connection.set_initial_window_size(2 * 1024 * 1024).unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        assert_eq!(request.uri().path(), "/test/Recovery");
        let read = async {
            let mut body = request.into_body();
            while let Some(data) = body.data().await {
                let data = data.unwrap();
                body.flow_control().release_capacity(data.len()).unwrap();
            }
        };
        tokio::pin!(read);
        tokio::select! {
            () = &mut read => {},
            result = connection.accept() => panic!("unexpected recovery request: {result:?}"),
        }
        let mut send = respond
            .send_response(
                http::Response::builder()
                    .header("content-type", "application/grpc")
                    .body(())
                    .unwrap(),
                false,
            )
            .unwrap();
        send.send_data(Bytes::from_static(b"\0\0\0\0\x0b\x0a\x09recovered"), false)
            .unwrap();
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        send.send_trailers(trailers).unwrap();
        while let Some(request) = connection.accept().await {
            request.unwrap();
        }
        drop(send);
    });
    Peer {
        address,
        ready: wait_ready,
        accepted: wait_accepted,
        reset: wait_reset,
        task,
    }
}

async fn channel(address: SocketAddr, budget: &ByteBudgetTracker) -> Channel {
    Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024)
        .max_concurrent_rpcs(1)
        .with_byte_budget_tracker(budget.clone())
}
async fn quiescent(budget: &ByteBudgetTracker) {
    guarded(async {
        while budget.allocated() != 0 || budget.active_byte_permit_tokens() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
}
async fn recovery(channel: &Channel) {
    let response = guarded(
        channel
            .unary::<HelloRequest, HelloReply>("/test/Recovery", Request::new(HelloRequest::new())),
    )
    .await
    .unwrap();
    assert_eq!(response.get_ref().message(), "recovered");
}

async fn panic_cleanup(after_headers: bool) {
    let peer = peer(after_headers).await;
    let budget = ByteBudgetTracker::with_limit(128 * 1024);
    let channel = channel(peer.address, &budget).await;
    guarded(peer.ready).await.unwrap();
    let panicked = Arc::new(AtomicBool::new(false));
    let (sender, call) = channel.bidi::<PanicMessage, HelloReply>("/test/Panic", Request::new(()));
    let message = PanicMessage {
        panicked: panicked.clone(),
        budget: budget.clone(),
    };
    // Keep the producer open so encoding runs in the independent upload task.
    // No RPC timeout: this tests the task's cleanup, not deadline recovery.
    if after_headers {
        let mut response = guarded(call).await.unwrap();
        assert_eq!(response.metadata().get("x-initial"), Some("observed"));
        guarded(peer.accepted).await.unwrap();
        sender.send(message).await.unwrap();
        let status = guarded(response.get_mut().message()).await.unwrap_err();
        assert_ne!(status.code(), Code::DeadlineExceeded);
        drop(response);
    } else {
        let mut call = Box::pin(call);
        guarded(async {
            tokio::select! {
                result = &mut call => panic!("peer has sent no response: {result:?}"),
                result = peer.accepted => result.unwrap(),
            }
        })
        .await;
        // Only now trigger the codec fault: resetting a stream before its
        // HEADERS were written would validly prevent peer acceptance.
        sender.send(message).await.unwrap();
        let status = guarded(call).await.unwrap_err();
        assert_ne!(status.code(), Code::DeadlineExceeded);
    }
    assert!(
        panicked.load(Ordering::SeqCst),
        "application panic witness did not run"
    );
    assert_eq!(guarded(peer.reset).await.unwrap(), h2::Reason::CANCEL);
    quiescent(&budget).await;
    assert!(sender.is_closed());
    recovery(&channel).await;
    peer.task.abort();
}

async fn cancellation_cleanup(explicit_cancel: bool) {
    let peer = peer(false).await;
    let budget = ByteBudgetTracker::with_limit(4 * 1024 * 1024);
    let channel = channel(peer.address, &budget).await;
    guarded(peer.ready).await.unwrap();
    let (sender, call) =
        channel.bidi::<HelloRequest, HelloReply>("/test/Blocked", Request::new(()));
    let mut message = HelloRequest::new();
    message.set_name("x".repeat(1024 * 1024));
    sender.send(message).await.unwrap();
    sender.close();
    let handle = call.handle();
    let mut call = Box::pin(call);
    guarded(async {
        tokio::select! {
            result = &mut call => panic!("peer has sent no response: {result:?}"),
            result = peer.accepted => result.unwrap(),
        }
    })
    .await;
    assert!(budget.allocated() > 0);
    assert!(budget.active_byte_permit_tokens() > 0);
    if explicit_cancel {
        handle.cancel();
        assert_eq!(guarded(call).await.unwrap_err().code(), Code::Cancelled);
    } else {
        drop(call);
    }
    assert_eq!(guarded(peer.reset).await.unwrap(), h2::Reason::CANCEL);
    quiescent(&budget).await;
    recovery(&channel).await;
    peer.task.abort();
}

#[tokio::test]
async fn upload_task_panic_before_headers_resets_and_releases_resources() {
    panic_cleanup(false).await;
}
#[tokio::test]
async fn upload_task_panic_after_headers_resets_and_releases_resources() {
    panic_cleanup(true).await;
}
#[tokio::test]
async fn dropping_closed_upload_call_before_headers_releases_resources() {
    cancellation_cleanup(false).await;
}
#[tokio::test]
async fn cancelling_closed_upload_call_before_headers_releases_resources() {
    cancellation_cleanup(true).await;
}
