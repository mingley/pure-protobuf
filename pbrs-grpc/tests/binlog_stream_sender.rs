//! Client upload failure provenance in binary logs.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "bounded integration-test synchronization and assertions"
)]

use bytes::Bytes;
use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, EventType, LogRecord, Sink, VecSink};
use pbrs_grpc::codec::CodecMessage;
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{CallHandle, Channel, Code, Request, Status, StreamSender};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const PATH: &str = "/probe.Service/Upload";
const WAIT: Duration = Duration::from_secs(10);
type UnitCall = Pin<Box<dyn Future<Output = Result<(), Status>> + Send>>;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(WAIT, future)
        .await
        .expect("bounded operation")
}

fn request() -> HelloRequest {
    let mut request = HelloRequest::new();
    request.set_name("request");
    request
}

fn status(code: Code) -> Status {
    let mut status = Status::with_details(code, "sender failure", vec![2_u8, 3, 5]);
    status
        .metadata_mut()
        .insert("x-proof", "intact")
        .expect("metadata");
    status
}

fn assert_status(status: &Status, code: Code) {
    assert_eq!(status.code(), code);
    assert_eq!(status.message(), "sender failure");
    assert_eq!(status.details(), &[2, 3, 5]);
    assert_eq!(status.metadata().get("x-proof"), Some("intact"));
}

fn cancels(sink: &VecSink) -> usize {
    sink.records()
        .iter()
        .filter(|record| record.entry.event == EventType::Cancel)
        .count()
}

fn logger(sink: Arc<dyn Sink>) -> BinaryLogger {
    BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink)
}

fn upload(
    channel: &Channel,
    bidi: bool,
    request: Request<()>,
) -> (StreamSender<HelloRequest>, UnitCall, CallHandle) {
    if bidi {
        let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(PATH, request);
        let handle = call.handle();
        (
            sender,
            Box::pin(async move { call.await.map(|_| ()) }),
            handle,
        )
    } else {
        let (sender, call) = channel.client_streaming::<HelloRequest, HelloReply>(PATH, request);
        let handle = call.handle();
        (
            sender,
            Box::pin(async move { call.await.map(|_| ()) }),
            handle,
        )
    }
}

/// A peer that either holds the response or sends a successful response when released.
struct Peer {
    channel: Channel,
    sink: Arc<VecSink>,
    accepted: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Peer {
    async fn new(success: bool, zero_window: bool, headers_first: bool) -> Self {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut builder = h2::server::Builder::new();
            if zero_window {
                builder.initial_window_size(0);
            }
            let mut conn = builder.handshake(server).await.expect("peer handshake");
            let mut accepted = Some(accepted_tx);
            let mut release = Some(release_rx);
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    item = conn.accept() => match item {
                        Some(Ok((request, mut respond))) => {
                            let accepted = accepted.take().expect("one request");
                            let release = release.take().expect("one response");
                            handlers.spawn(async move {
                                let body = request.into_body();
                                accepted.send(()).expect("accepted");
                                if headers_first {
                                    let response = http::Response::builder().status(200)
                                        .header("content-type", "application/grpc")
                                        .body(()).expect("response");
                                    let send = respond.send_response(response, false).expect("headers");
                                    std::future::pending::<()>().await;
                                    drop((body, send));
                                    return;
                                }
                                if success {
                                    release.await.expect("response release");
                                    let response = http::Response::builder().status(200)
                                        .header("content-type", "application/grpc")
                                        .body(()).expect("response");
                                    let mut send = respond.send_response(response, false).expect("headers");
                                    send.send_data(Bytes::from_static(b"\0\0\0\0\0"), false).expect("message");
                                    let mut trailers = http::HeaderMap::new();
                                    trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                                    send.send_trailers(trailers).expect("trailers");
                                }
                                // Keep the upload open until the test ends.
                                std::future::pending::<()>().await;
                                drop((body, respond));
                            });
                        }
                        Some(Err(_)) | None => break,
                    },
                    completed = handlers.join_next(), if !handlers.is_empty() => {
                        completed.expect("handler").expect("handler did not panic");
                    }
                }
            }
        });
        let channel = bounded(Channel::from_io(client, "localhost"))
            .await
            .expect("client handshake");
        let sink = Arc::new(VecSink::new());
        Self {
            channel: channel.binary_logger(logger(sink.clone())),
            sink,
            accepted: Some(accepted_rx),
            release: Some(release_tx),
            task,
        }
    }

    async fn accepted(&mut self) {
        bounded(self.accepted.take().expect("accept receiver"))
            .await
            .expect("accepted");
    }

    fn release(&mut self) {
        self.release
            .take()
            .expect("release sender")
            .send(())
            .expect("release peer");
    }
}

#[tokio::test]
async fn reached_sender_failure_preserves_status_and_logs_once() {
    for bidi in [false, true] {
        for live in [false, true] {
            for code in [Code::Cancelled, Code::InvalidArgument, Code::Ok] {
                let peer = Peer::new(false, false, false).await;
                let (sender, call, handle) = upload(&peer.channel, bidi, Request::new(()));
                let keep = live.then(|| sender.clone());
                bounded(sender.fail(status(code))).await;
                let returned = bounded(call).await.expect_err("sender failure");
                assert_status(&returned, code);
                assert!(
                    !handle.is_cancelled(),
                    "sender failure is not a handle signal"
                );
                assert_eq!(cancels(&peer.sink), 1);
                drop(keep);
            }
        }
    }
}

#[derive(Debug)]
struct FailedEncode(Arc<AtomicUsize>);

impl CodecMessage for FailedEncode {
    fn encoded_len(&self) -> usize {
        0
    }

    fn encode_payload<W: pbrs::WireOut>(&self, _out: &mut W) -> Result<(), Status> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Status::internal("encoder failed"))
    }

    fn decode_payload(_payload: Bytes) -> Result<Self, Status> {
        Err(Status::internal("request-only test message"))
    }

    fn empty() -> Self {
        Self(Arc::default())
    }
}

fn failing_upload(channel: &Channel, bidi: bool) -> (StreamSender<FailedEncode>, UnitCall) {
    if bidi {
        let (sender, call) = channel.bidi::<FailedEncode, HelloReply>(PATH, Request::new(()));
        (sender, Box::pin(async move { call.await.map(|_| ()) }))
    } else {
        let (sender, call) =
            channel.client_streaming::<FailedEncode, HelloReply>(PATH, Request::new(()));
        (sender, Box::pin(async move { call.await.map(|_| ()) }))
    }
}

#[tokio::test]
async fn only_the_first_reached_failure_is_logged() {
    for bidi in [false, true] {
        for live in [false, true] {
            for sender_first in [false, true] {
                let peer = Peer::new(false, false, false).await;
                let (sender, call) = failing_upload(&peer.channel, bidi);
                let mut keep = Some(sender.clone());
                let hits = Arc::new(AtomicUsize::new(0));
                if sender_first {
                    bounded(sender.fail(status(Code::InvalidArgument))).await;
                    bounded(
                        keep.as_ref()
                            .expect("sender")
                            .send(FailedEncode(hits.clone())),
                    )
                    .await
                    .expect("queue encoder");
                } else {
                    bounded(sender.send(FailedEncode(hits.clone())))
                        .await
                        .expect("queue encoder");
                    bounded(sender.fail(status(Code::InvalidArgument))).await;
                }
                assert!(!keep.as_ref().expect("sender").is_closed());
                if !live {
                    drop(keep.take());
                }
                let returned = bounded(call).await.expect_err("first failure");
                if sender_first {
                    assert_status(&returned, Code::InvalidArgument);
                    assert_eq!(hits.load(Ordering::SeqCst), 0);
                    assert_eq!(cancels(&peer.sink), 1);
                } else {
                    assert_eq!(returned.code(), Code::Internal);
                    assert_eq!(returned.message(), "encoder failed");
                    assert_eq!(hits.load(Ordering::SeqCst), 1);
                    assert_eq!(cancels(&peer.sink), 0);
                }
                drop(keep);
            }
        }
    }
}

#[derive(Debug)]
struct Gate {
    entered: mpsc::UnboundedSender<()>,
    open: Mutex<bool>,
    ready: Condvar,
}

impl Gate {
    fn wait(&self) {
        self.entered.send(()).expect("gate entered");
        let open = self.open.lock().expect("gate");
        let (open, timeout) = self
            .ready
            .wait_timeout_while(open, WAIT, |open| !*open)
            .expect("gate wait");
        assert!(*open && !timeout.timed_out(), "gate timed out");
    }

    fn release(&self) {
        *self.open.lock().expect("gate") = true;
        self.ready.notify_all();
    }
}

fn gate() -> (Gate, mpsc::UnboundedReceiver<()>) {
    let (entered, receiver) = mpsc::unbounded_channel();
    (
        Gate {
            entered,
            open: Mutex::new(false),
            ready: Condvar::new(),
        },
        receiver,
    )
}

#[derive(Debug)]
struct ReentrantSink {
    handle: Mutex<Option<CallHandle>>,
    gate: Gate,
    calls: AtomicUsize,
}

impl Sink for ReentrantSink {
    fn emit(&self, record: &LogRecord) {
        if record.entry.event == EventType::Cancel {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let handle = self
                .handle
                .lock()
                .expect("handle")
                .take()
                .expect("retained handle");
            handle.cancel();
            assert!(handle.is_cancelled());
            self.gate.wait();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reentrant_cancel_callback_preserves_the_selected_status() {
    for bidi in [false, true] {
        for live in [false, true] {
            let mut peer = Peer::new(false, false, false).await;
            let (gate, mut entered) = gate();
            let sink = Arc::new(ReentrantSink {
                handle: Mutex::new(None),
                gate,
                calls: AtomicUsize::new(0),
            });
            let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
            let (sender, call, handle) = upload(&channel, bidi, Request::new(()));
            *sink.handle.lock().expect("handle") = Some(handle.clone());
            let keep = live.then(|| sender.clone());
            let task = if live {
                let task = tokio::spawn(call);
                peer.accepted().await;
                bounded(sender.fail(status(Code::InvalidArgument))).await;
                task
            } else {
                bounded(sender.fail(status(Code::InvalidArgument))).await;
                tokio::spawn(call)
            };
            bounded(entered.recv()).await.expect("cancel callback");
            assert!(handle.is_cancelled());
            sink.gate.release();
            let returned = bounded(task)
                .await
                .expect("call task")
                .expect_err("sender failure");
            assert_status(&returned, Code::InvalidArgument);
            assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
            drop(keep);
        }
    }
}

#[derive(Debug)]
struct PanicPayload;

impl Drop for PanicPayload {
    fn drop(&mut self) {
        panic!("panic payload drop");
    }
}

#[derive(Debug)]
struct PanicSink {
    payload_drop_panics: bool,
    calls: AtomicUsize,
}

impl Sink for PanicSink {
    fn emit(&self, record: &LogRecord) {
        if record.entry.event == EventType::Cancel {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.payload_drop_panics {
                std::panic::panic_any(PanicPayload);
            }
            panic!("cancel sink");
        }
    }
}

#[tokio::test]
async fn cancel_sink_panics_do_not_replace_sender_failure() {
    for bidi in [false, true] {
        for payload_drop_panics in [false, true] {
            let mut peer = Peer::new(false, false, false).await;
            let sink = Arc::new(PanicSink {
                payload_drop_panics,
                calls: AtomicUsize::new(0),
            });
            let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
            let (sender, call, handle) = upload(&channel, bidi, Request::new(()));
            let task = tokio::spawn(call);
            peer.accepted().await;
            bounded(sender.fail(status(Code::InvalidArgument))).await;
            let returned = bounded(task)
                .await
                .expect("call task")
                .expect_err("sender failure");
            assert_status(&returned, Code::InvalidArgument);
            assert!(!handle.is_cancelled());
            assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[derive(Debug)]
struct HoldSink {
    gate: Gate,
    panic_after_release: bool,
    calls: AtomicUsize,
}

impl Sink for HoldSink {
    fn emit(&self, record: &LogRecord) {
        if record.entry.event == EventType::Cancel {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.gate.wait();
            assert!(!self.panic_after_release, "cancel sink");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn selected_sender_failure_survives_deadline_during_callback() {
    for bidi in [false, true] {
        for code in [Code::Unavailable, Code::Cancelled] {
            for panic_after_release in [false, true] {
                let mut peer = Peer::new(false, false, false).await;
                let (gate, mut entered) = gate();
                let sink = Arc::new(HoldSink {
                    gate,
                    panic_after_release,
                    calls: AtomicUsize::new(0),
                });
                let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
                let mut request = Request::new(());
                request.set_timeout(Duration::from_secs(2));
                let (sender, call, _) = upload(&channel, bidi, request);
                let task = tokio::spawn(call);
                peer.accepted().await;
                bounded(sender.fail(status(code))).await;
                bounded(entered.recv()).await.expect("failure selected");
                tokio::time::sleep(Duration::from_millis(2100)).await;
                sink.gate.release();
                let returned = bounded(task)
                    .await
                    .expect("call task")
                    .expect_err("sender failure");
                assert_status(&returned, code);
                assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
            }
        }
    }
}

#[derive(Debug)]
struct MessageGate {
    gate: Gate,
    events: Mutex<Vec<EventType>>,
}

impl Sink for MessageGate {
    fn emit(&self, record: &LogRecord) {
        self.events.lock().expect("events").push(record.entry.event);
        if record.entry.event == EventType::ClientMessage {
            self.gate.wait();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accepted_failure_can_lose_to_terminal_response() {
    for bidi in [false, true] {
        for code in [Code::Cancelled, Code::InvalidArgument, Code::Ok] {
            let mut peer = Peer::new(true, true, false).await;
            let (gate, mut entered) = gate();
            let sink = Arc::new(MessageGate {
                gate,
                events: Mutex::new(Vec::new()),
            });
            let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
            let (sender, task) = if bidi {
                let (sender, call) =
                    channel.bidi::<HelloRequest, HelloReply>(PATH, Request::new(()));
                let task = tokio::spawn(async move {
                    let mut response = call.await?.into_inner();
                    assert!(bounded(response.message()).await?.is_some());
                    assert!(bounded(response.message()).await?.is_none());
                    Ok::<(), Status>(())
                });
                (sender, task)
            } else {
                let (sender, call) =
                    channel.client_streaming::<HelloRequest, HelloReply>(PATH, Request::new(()));
                (sender, tokio::spawn(async move { call.await.map(|_| ()) }))
            };
            let mut large = HelloRequest::new();
            large.set_name("x".repeat(2 * 1024 * 1024));
            bounded(sender.send(large)).await.expect("first message");
            bounded(entered.recv()).await.expect("message callback");
            let keep = sender.clone();
            bounded(sender.fail(status(code))).await;
            assert!(!keep.is_closed(), "failure was accepted");
            sink.gate.release();
            peer.accepted().await;
            peer.release();
            bounded(task)
                .await
                .expect("call task")
                .expect("successful response");
            bounded(keep.closed()).await;
            let events = sink.events.lock().expect("events");
            assert_eq!(
                events
                    .iter()
                    .filter(|event| **event == EventType::ClientMessage)
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| **event == EventType::Cancel)
                    .count(),
                0
            );
        }
    }
}

#[cfg(feature = "tower")]
#[tokio::test]
async fn forwarded_tower_errors_remain_unclassified() {
    use tower::ServiceExt;

    for bidi in [false, true] {
        for compat in [false, true] {
            let peer = Peer::new(false, false, false).await;
            let returned = if compat {
                let stream = pbrs_grpc::compat::iter([Err::<HelloRequest, Status>(status(
                    Code::InvalidArgument,
                ))]);
                let stream = pbrs_grpc::compat::response_stream(pbrs_grpc::Response::new(stream))
                    .into_inner();
                if bidi {
                    bounded(
                        peer.channel
                            .tower_bidi::<HelloRequest, HelloReply>(PATH)
                            .oneshot(Request::new(stream)),
                    )
                    .await
                    .expect_err("forwarded error")
                } else {
                    bounded(
                        peer.channel
                            .tower_client_streaming::<HelloRequest, HelloReply>(PATH)
                            .oneshot(Request::new(stream)),
                    )
                    .await
                    .expect_err("forwarded error")
                }
            } else {
                let stream = pbrs_grpc::compat::iter([Err::<HelloRequest, Status>(status(
                    Code::InvalidArgument,
                ))]);
                if bidi {
                    bounded(
                        peer.channel
                            .tower_bidi::<HelloRequest, HelloReply>(PATH)
                            .oneshot(Request::new(stream)),
                    )
                    .await
                    .expect_err("input error")
                } else {
                    bounded(
                        peer.channel
                            .tower_client_streaming::<HelloRequest, HelloReply>(PATH)
                            .oneshot(Request::new(stream)),
                    )
                    .await
                    .expect_err("input error")
                }
            };
            assert_status(&returned, Code::InvalidArgument);
            assert_eq!(cancels(&peer.sink), 0);
        }
    }
}

#[tokio::test]
async fn failure_waits_for_a_full_upload_queue() {
    for bidi in [false, true] {
        let peer = Peer::new(false, false, false).await;
        let channel = peer.channel.clone().stream_buffer(1);
        let (sender, call, handle) = upload(&channel, bidi, Request::new(()));
        bounded(sender.send(request())).await.expect("fill queue");
        let keep = sender.clone();
        let mut failure = Box::pin(sender.fail(status(Code::InvalidArgument)));
        let first_poll =
            std::future::poll_fn(|cx| std::task::Poll::Ready(failure.as_mut().poll(cx))).await;
        assert!(first_poll.is_pending(), "failure waits for queue capacity");
        assert!(
            peer.sink.records().is_empty(),
            "the unpolled call has not logged"
        );
        let task = tokio::spawn(call);
        bounded(failure).await;
        let returned = bounded(task)
            .await
            .expect("call task")
            .expect_err("sender failure");
        assert_status(&returned, Code::InvalidArgument);
        assert!(!handle.is_cancelled());
        let records = peer.sink.records();
        let message = records
            .iter()
            .position(|record| record.entry.event == EventType::ClientMessage)
            .expect("queued message");
        let cancel = records
            .iter()
            .position(|record| record.entry.event == EventType::Cancel)
            .expect("sender failure");
        assert!(message < cancel);
        assert_eq!(cancels(&peer.sink), 1);
        drop(keep);
    }
}

#[tokio::test]
async fn post_header_sink_panic_does_not_strand_bidi_reset() {
    let peer = Peer::new(false, false, true).await;
    let sink = Arc::new(PanicSink {
        payload_drop_panics: true,
        calls: AtomicUsize::new(0),
    });
    let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
    let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(PATH, Request::new(()));
    let mut response = bounded(call).await.expect("response headers").into_inner();
    bounded(sender.fail(status(Code::InvalidArgument))).await;
    assert_eq!(
        bounded(response.message())
            .await
            .expect_err("response reset")
            .code(),
        Code::Cancelled
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_header_reentrant_sink_does_not_strand_bidi_reset() {
    let peer = Peer::new(false, false, true).await;
    let (gate, mut entered) = gate();
    let sink = Arc::new(ReentrantSink {
        handle: Mutex::new(None),
        gate,
        calls: AtomicUsize::new(0),
    });
    let channel = peer.channel.clone().binary_logger(logger(sink.clone()));
    let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(PATH, Request::new(()));
    let handle = call.handle();
    *sink.handle.lock().expect("handle") = Some(handle.clone());
    let mut response = bounded(call).await.expect("response headers").into_inner();
    bounded(sender.fail(status(Code::InvalidArgument))).await;
    bounded(entered.recv()).await.expect("cancel callback");
    assert!(handle.is_cancelled());
    sink.gate.release();
    assert_eq!(
        bounded(response.message())
            .await
            .expect_err("response reset")
            .code(),
        Code::Cancelled
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}
