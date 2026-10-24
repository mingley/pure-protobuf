//! Independent HTTP/2 witnesses for accepted deadline and response ownership.
//!
//! This peer sends grpc-timeout but never implements a client deadline or RST.
#![allow(
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded raw-peer regression fixtures with explicit wire assertions"
)]

use bytes::Bytes;
use pbrs_grpc::telemetry::{CallLabels, LifecycleObserver};
use pbrs_grpc::{
    Code, HelloReply, HelloRequest, Metadata, Parts, Request, Response, Rpc, Server, Service,
    Status, Streaming,
};
use std::future::{Future, pending, poll_fn};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Unary,
    ClientStream,
    ServerStream,
    Bidi,
}

#[derive(Clone, Copy)]
enum Kind {
    Hold,
    LateCap,
    Tighten,
    ReplaceMetadata,
    ConstructorOverrun,
    EmptyUnary,
    CommittedEmptyUnary,
    RetainParts,
    Output(Shape, usize),
}

struct Probe {
    entered: AtomicBool,
    cap_set: AtomicBool,
    dropped: AtomicUsize,
    producer_dropped: AtomicUsize,
    handlers: AtomicUsize,
    constructors: AtomicUsize,
    deadlines: Mutex<Vec<Option<tokio::time::Instant>>>,
    starts: AtomicUsize,
    ends: Mutex<Vec<Code>>,
    gate: watch::Receiver<bool>,
    producers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    retained: Mutex<Option<Parts>>,
}

struct ServiceOwner(Arc<Probe>);

impl Drop for ServiceOwner {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

struct ProducerOwner(Arc<Probe>);

impl Drop for ProducerOwner {
    fn drop(&mut self) {
        self.0.producer_dropped.fetch_add(1, Ordering::SeqCst);
    }
}

struct Observer(Arc<Probe>);

impl LifecycleObserver for Observer {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        if call.method() != "Recovery" {
            self.0.starts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _: Duration) {
        if call.method() != "Recovery" {
            self.0.ends.lock().unwrap().push(status.code());
        }
    }
}

struct Extended {
    probe: Arc<Probe>,
    kind: Kind,
}

fn reply(length: usize) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message("x".repeat(length));
    reply
}

fn output_stream(probe: &Arc<Probe>, length: usize) -> Streaming<HelloReply> {
    let (sender, stream) = Streaming::channel(1);
    let owner = ProducerOwner(Arc::clone(probe));
    let task = tokio::spawn(async move {
        let _owner = owner;
        sender.send(reply(length)).await.unwrap();
        sender.closed().await;
    });
    probe.producers.lock().unwrap().push(task);
    stream
}

impl Service for Extended {
    const NAME: &'static str = "deadline.Extended";

    async fn call(&self, mut rpc: Rpc) {
        if rpc.method() == "Recovery" {
            // A trailers-only recovery needs no DATA credit. Its exact status
            // distinguishes service admission from an exhausted sole RPC slot.
            rpc.reject(Status::unimplemented("recovered"));
            return;
        }
        let _owner = ServiceOwner(Arc::clone(&self.probe));
        self.probe.deadlines.lock().unwrap().push(rpc.deadline());
        self.probe.entered.store(true, Ordering::SeqCst);
        match self.kind {
            Kind::Hold => {
                pending::<()>().await;
                drop(rpc);
            }
            Kind::LateCap | Kind::Tighten => {
                let mut gate = self.probe.gate.clone();
                gate.wait_for(|open| *open).await.unwrap();
                if matches!(self.kind, Kind::LateCap) {
                    rpc.set_timeout(Duration::from_millis(20));
                } else {
                    rpc.set_timeout(Duration::from_millis(80));
                    rpc.set_timeout(Duration::from_millis(200));
                }
                self.probe.deadlines.lock().unwrap().push(rpc.deadline());
                self.probe.cap_set.store(true, Ordering::SeqCst);
                pending::<()>().await;
                drop(rpc);
            }
            Kind::ReplaceMetadata => {
                *rpc.metadata_mut() = Metadata::new();
                rpc.set_timeout(Duration::from_millis(200));
                self.probe.deadlines.lock().unwrap().push(rpc.deadline());
                pending::<()>().await;
                drop(rpc);
            }
            Kind::ConstructorOverrun => {
                let probe = Arc::clone(&self.probe);
                rpc.unary(move |_: Request<HelloRequest>| {
                    probe.constructors.fetch_add(1, Ordering::SeqCst);
                    let before = tokio::time::Instant::now();
                    // The constructor is synchronous. Poll the public paused
                    // clock operation through its clock advance, before its
                    // cooperative yield, then drop that clock-only future.
                    // Assert the time change so a clock-fixture change cannot
                    // silently turn this into an unexpired-handler test.
                    let mut advance =
                        std::pin::pin!(tokio::time::advance(Duration::from_millis(21)));
                    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
                    let _advanced = advance.as_mut().poll(&mut cx);
                    assert_eq!(
                        tokio::time::Instant::now(),
                        before + Duration::from_millis(21)
                    );
                    async move {
                        probe.handlers.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, Status>(Response::new(HelloReply::new()))
                    }
                })
                .await;
            }
            Kind::EmptyUnary | Kind::CommittedEmptyUnary => {
                let probe = Arc::clone(&self.probe);
                rpc.unary(move |_: Request<HelloRequest>| async move {
                    probe.handlers.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await;
            }
            Kind::RetainParts => {
                let probe = Arc::clone(&self.probe);
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    let (_, parts) = request.into_message_and_parts();
                    *probe.retained.lock().unwrap() = Some(parts);
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await;
            }
            Kind::Output(shape, length) => {
                let probe = Arc::clone(&self.probe);
                match shape {
                    Shape::Unary => {
                        rpc.unary(move |_: Request<HelloRequest>| async move {
                            probe.handlers.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, Status>(Response::new(reply(length)))
                        })
                        .await;
                    }
                    Shape::ClientStream => {
                        rpc.client_streaming(
                            move |mut request: Request<Streaming<HelloRequest>>| async move {
                                while request.get_mut().message().await?.is_some() {}
                                probe.handlers.fetch_add(1, Ordering::SeqCst);
                                Ok::<_, Status>(Response::new(reply(length)))
                            },
                        )
                        .await;
                    }
                    Shape::ServerStream => {
                        rpc.server_streaming(move |_: Request<HelloRequest>| async move {
                            probe.handlers.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, Status>(Response::new(output_stream(&probe, length)))
                        })
                        .await;
                    }
                    Shape::Bidi => {
                        rpc.bidi_streaming(
                            move |mut request: Request<Streaming<HelloRequest>>| async move {
                                while request.get_mut().message().await?.is_some() {}
                                probe.handlers.fetch_add(1, Ordering::SeqCst);
                                Ok::<_, Status>(Response::new(output_stream(&probe, length)))
                            },
                        )
                        .await;
                    }
                }
            }
        }
    }
}

// Self-waking prevents paused Tokio time from advancing to an unrelated timer.
// The real five-second bound is solely a hung-fixture watchdog.
async fn drive<T>(future: impl Future<Output = T>) -> T {
    let started = std::time::Instant::now();
    tokio::pin!(future);
    poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        if result.is_pending() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "raw deadline peer did not make progress"
            );
            cx.waker().wake_by_ref();
        }
        result
    })
    .await
}

async fn witnessed(predicate: impl Fn() -> bool) {
    drive(poll_fn(|_| {
        if predicate() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }))
    .await;
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
    async fn new(kind: Kind, response_window: u32) -> Self {
        let (gate, receiver) = watch::channel(false);
        let probe = Arc::new(Probe {
            entered: AtomicBool::new(false),
            cap_set: AtomicBool::new(false),
            dropped: AtomicUsize::new(0),
            producer_dropped: AtomicUsize::new(0),
            handlers: AtomicUsize::new(0),
            constructors: AtomicUsize::new(0),
            deadlines: Mutex::new(Vec::new()),
            starts: AtomicUsize::new(0),
            ends: Mutex::new(Vec::new()),
            gate: receiver,
            producers: Mutex::new(Vec::new()),
            retained: Mutex::new(None),
        });
        let (client_io, server_io) = tokio::io::duplex(4096);
        let service = Extended {
            probe: Arc::clone(&probe),
            kind,
        };
        let observer = Observer(Arc::clone(&probe));
        let server = tokio::spawn(async move {
            let mut server = Server::new(service)
                .observer(observer)
                .max_concurrent_rpcs(1);
            if matches!(kind, Kind::EmptyUnary) {
                // Deliberately prevent the complete status body from fitting
                // in the send buffer before the deadline. A small peer window
                // alone does not imply the application write is still pending.
                server = server.max_send_buffer_size(8);
            }
            server.serve_connection(server_io).await.unwrap();
        });
        let mut builder = h2::client::Builder::new();
        builder.initial_window_size(response_window);
        let (send, connection) = drive(builder.handshake(client_io)).await.unwrap();
        let connection = tokio::spawn(async move {
            // Connection abortion is covered separately and may close this IO.
            let _result = connection.await;
        });
        Self {
            send,
            server,
            connection,
            probe,
            gate,
        }
    }

    async fn request(
        &mut self,
        method: &str,
        timeout: Option<&str>,
        content_type: &str,
    ) -> h2::client::ResponseFuture {
        let mut request = http::Request::builder()
            .method("POST")
            .uri(format!("http://local/deadline.Extended/{method}"))
            .header("content-type", content_type)
            .header("te", "trailers");
        if let Some(timeout) = timeout {
            request = request.header("grpc-timeout", timeout);
        }
        self.send = drive(self.send.clone().ready()).await.unwrap();
        let (response, mut body) = self
            .send
            .send_request(request.body(()).unwrap(), false)
            .unwrap();
        #[cfg(feature = "grpc-web")]
        let input = if content_type.contains("grpc-web-text") {
            use base64::Engine as _;
            Bytes::from(base64::engine::general_purpose::STANDARD.encode([0; 5]))
        } else {
            Bytes::from_static(&[0, 0, 0, 0, 0])
        };
        #[cfg(not(feature = "grpc-web"))]
        let input = Bytes::from_static(&[0, 0, 0, 0, 0]);
        body.send_data(input, true).unwrap();
        response
    }

    async fn entered(&self) {
        witnessed(|| self.probe.entered.load(Ordering::SeqCst)).await;
    }

    async fn expired(&self) {
        witnessed(|| self.probe.dropped.load(Ordering::SeqCst) == 1).await;
        turn().await;
        assert_eq!(self.probe.starts.load(Ordering::SeqCst), 1);
        assert_eq!(*self.probe.ends.lock().unwrap(), [Code::DeadlineExceeded]);
    }

    async fn recovery(&mut self) {
        let response = self.request("Recovery", None, "application/grpc").await;
        let response = drive(response)
            .await
            .expect("same connection remains usable");
        let (code, bytes) = native_status(response).await;
        assert_eq!(code, "12", "the sole application RPC permit was released");
        assert!(bytes.is_empty());
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.gate.send_replace(true);
        self.connection.abort();
        self.server.abort();
        let tasks = std::mem::take(&mut *self.probe.producers.lock().unwrap());
        for task in tasks {
            task.abort();
        }
    }
}

async fn collect(mut body: h2::RecvStream) -> (Vec<u8>, Option<http::HeaderMap>) {
    let mut bytes = Vec::new();
    while let Some(data) = drive(body.data()).await {
        let data = data.expect("terminal status must not be replaced by a reset");
        bytes.extend_from_slice(&data);
        body.flow_control().release_capacity(data.len()).unwrap();
    }
    let trailers = drive(body.trailers()).await.unwrap();
    (bytes, trailers)
}

async fn native_status(response: http::Response<h2::RecvStream>) -> (String, Vec<u8>) {
    let initial = response.headers().get("grpc-status").cloned();
    let (bytes, trailers) = collect(response.into_body()).await;
    let code = initial
        .or_else(|| trailers.and_then(|headers| headers.get("grpc-status").cloned()))
        .expect("one native gRPC terminal status");
    (code.to_str().unwrap().to_owned(), bytes)
}

#[tokio::test(start_paused = true)]
async fn late_cap_on_initially_untimed_service_consumes_earlier_wait() {
    let mut peer = Peer::new(Kind::LateCap, 65535).await;
    let accepted = tokio::time::Instant::now();
    let response = peer.request("Call", None, "application/grpc").await;
    peer.entered().await;
    assert_eq!(*peer.probe.deadlines.lock().unwrap().first().unwrap(), None);
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.gate.send_replace(true);
    witnessed(|| peer.probe.cap_set.load(Ordering::SeqCst)).await;
    assert_eq!(
        *peer.probe.deadlines.lock().unwrap().get(1).unwrap(),
        Some(accepted + Duration::from_millis(20)),
        "late cap is anchored to acceptance, not setter time"
    );
    peer.expired().await;
    assert_eq!(peer.probe.handlers.load(Ordering::SeqCst), 0);
    assert_eq!(native_status(drive(response).await.unwrap()).await.0, "4");
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn repeated_caps_tighten_without_renewing_time() {
    let mut peer = Peer::new(Kind::Tighten, 65535).await;
    let accepted = tokio::time::Instant::now();
    let response = peer.request("Call", Some("100m"), "application/grpc").await;
    peer.entered().await;
    tokio::time::advance(Duration::from_millis(50)).await;
    peer.gate.send_replace(true);
    witnessed(|| peer.probe.cap_set.load(Ordering::SeqCst)).await;
    assert_eq!(
        *peer.probe.deadlines.lock().unwrap(),
        [
            Some(accepted + Duration::from_millis(100)),
            Some(accepted + Duration::from_millis(80))
        ]
    );
    tokio::time::advance(Duration::from_millis(29)).await;
    turn().await;
    assert_eq!(peer.probe.dropped.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_millis(2)).await;
    peer.expired().await;
    assert_eq!(native_status(drive(response).await.unwrap()).await.0, "4");
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn replacing_metadata_does_not_remove_the_accepted_peer_deadline() {
    let mut peer = Peer::new(Kind::ReplaceMetadata, 65535).await;
    let response = peer.request("Call", Some("20m"), "application/grpc").await;
    peer.entered().await;
    let deadlines = peer.probe.deadlines.lock().unwrap().clone();
    let first = deadlines.first().unwrap();
    assert!(first.is_some());
    assert_eq!(first, deadlines.get(1).unwrap());
    tokio::time::advance(Duration::from_millis(21)).await;
    peer.expired().await;
    assert_eq!(native_status(drive(response).await.unwrap()).await.0, "4");
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn handler_constructor_overrun_cannot_poll_its_returned_future() {
    let mut peer = Peer::new(Kind::ConstructorOverrun, 65535).await;
    let response = peer.request("Call", Some("20m"), "application/grpc").await;
    witnessed(|| peer.probe.constructors.load(Ordering::SeqCst) == 1).await;
    peer.expired().await;
    assert_eq!(
        peer.probe.handlers.load(Ordering::SeqCst),
        0,
        "check expiry after the synchronous FnOnce constructor, before its future is polled"
    );
    assert_eq!(native_status(drive(response).await.unwrap()).await.0, "4");
    peer.recovery().await;
}

#[tokio::test(start_paused = true)]
async fn retained_request_parts_do_not_prevent_connection_completion() {
    let mut peer = Peer::new(Kind::RetainParts, 65535).await;
    let accepted = tokio::time::Instant::now();
    let response = peer.request("Call", Some("20m"), "application/grpc").await;
    let (status, bytes) = native_status(drive(response).await.unwrap()).await;
    assert_eq!(status, "0");
    assert_eq!(bytes, [0, 0, 0, 0, 0]);
    witnessed(|| peer.probe.dropped.load(Ordering::SeqCst) == 1).await;
    turn().await;
    {
        let retained = peer.probe.retained.lock().unwrap();
        let parts = retained
            .as_ref()
            .expect("application retained request parts");
        assert!(
            parts.is_cancelled(),
            "completed call cancellation remains observable"
        );
        assert_eq!(parts.deadline(), Some(accepted + Duration::from_millis(20)));
    }
    // Keep the parts (and their cancellation Arc) alive while the actual
    // independent peer driver and IO close. This is a behavioral drain check,
    // not a claim about internal Arc counts or total retained memory.
    peer.connection.abort();
    assert!(
        drive(&mut peer.connection)
            .await
            .unwrap_err()
            .is_cancelled()
    );
    drive(&mut peer.server)
        .await
        .expect("server connection completes with retained request parts");
    assert!(peer.probe.retained.lock().unwrap().is_some());
    assert_eq!(peer.probe.starts.load(Ordering::SeqCst), 1);
    assert_eq!(*peer.probe.ends.lock().unwrap(), [Code::Ok]);
}

async fn native_blocked_response(shape: Shape) {
    for window in [0, 7] {
        let mut peer = Peer::new(Kind::Output(shape, 1024 * 1024), window).await;
        let response = peer.request("Call", Some("20m"), "application/grpc").await;
        let response = drive(response)
            .await
            .expect("committed native response headers");
        assert!(response.headers().get("grpc-status").is_none());
        let mut body = response.into_body();
        if window != 0 {
            let mut first = Vec::new();
            while first.len() < 7 {
                let data = drive(body.data()).await.unwrap().unwrap();
                first.extend_from_slice(&data);
                // No credit is returned, regardless of DATA packet boundaries.
            }
            assert_eq!(
                first.len(),
                7,
                "{shape:?}: exhaust the exact small response window"
            );
            assert_eq!(first.first(), Some(&0));
            let declared = u32::from_be_bytes(first.get(1..5).unwrap().try_into().unwrap());
            assert!(declared > 1024 * 1024);
            // Keep all seven bytes charged: no WINDOW_UPDATE can unblock DATA.
        }
        tokio::time::advance(Duration::from_millis(21)).await;
        peer.expired().await;
        let (remaining, trailers) = collect(body).await;
        assert!(
            remaining.is_empty(),
            "{shape:?}: unsent DATA must be discarded"
        );
        assert_eq!(trailers.unwrap().get("grpc-status").unwrap(), "4");
        assert_eq!(peer.probe.handlers.load(Ordering::SeqCst), 1);
        if matches!(shape, Shape::ServerStream | Shape::Bidi) {
            witnessed(|| peer.probe.producer_dropped.load(Ordering::SeqCst) == 1).await;
        }
        peer.recovery().await;
    }
}

#[tokio::test(start_paused = true)]
async fn unary_deadline_ends_a_flow_blocked_response() {
    native_blocked_response(Shape::Unary).await;
}

#[tokio::test(start_paused = true)]
async fn client_stream_deadline_ends_a_flow_blocked_response() {
    native_blocked_response(Shape::ClientStream).await;
}

#[tokio::test(start_paused = true)]
async fn server_stream_deadline_ends_a_flow_blocked_response() {
    native_blocked_response(Shape::ServerStream).await;
}

#[tokio::test(start_paused = true)]
async fn bidi_deadline_ends_a_flow_blocked_response() {
    native_blocked_response(Shape::Bidi).await;
}

#[tokio::test(start_paused = true)]
async fn connection_abort_ends_the_started_call_once() {
    let mut peer = Peer::new(Kind::Hold, 65535).await;
    let _response = peer.request("Call", None, "application/grpc").await;
    peer.entered().await;
    assert_eq!(
        peer.probe.starts.load(Ordering::SeqCst),
        1,
        "admitted pre-shape service is part of the server call lifecycle"
    );
    peer.server.abort();
    witnessed(|| peer.probe.dropped.load(Ordering::SeqCst) == 1).await;
    turn().await;
    assert_eq!(peer.probe.starts.load(Ordering::SeqCst), 1);
    assert_eq!(peer.probe.ends.lock().unwrap().len(), 1);
    turn().await;
    assert_eq!(peer.probe.ends.lock().unwrap().len(), 1);
}

#[cfg(feature = "grpc-web")]
const WEB_TYPES: [&str; 4] = [
    "application/grpc-web",
    "application/grpc-web+proto",
    "application/grpc-web-text",
    "application/grpc-web-text+proto",
];

#[cfg(feature = "grpc-web")]
fn web_frames_with_status(
    content_type: &str,
    encoded: &[u8],
    expected_status: &str,
) -> Vec<Vec<u8>> {
    use base64::Engine as _;
    let decoded;
    let mut bytes = if content_type.contains("-text") {
        decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("complete independently decoded base64, including final carry");
        decoded.as_slice()
    } else {
        encoded
    };
    let mut messages = Vec::new();
    let mut ended = false;
    let expected_line = format!("grpc-status: {expected_status}");
    while !bytes.is_empty() {
        assert!(!ended, "no body records after terminal trailers");
        assert!(bytes.len() >= 5, "complete gRPC-Web record prefix");
        let (&[flag, a, b, c, d], body) = bytes.split_first_chunk::<5>().unwrap();
        let len = usize::try_from(u32::from_be_bytes([a, b, c, d])).unwrap();
        assert!(
            bytes.len() >= 5 + len,
            "status bytes cannot fill a partial message"
        );
        let (payload, remaining) = body.split_at_checked(len).unwrap();
        match flag {
            0 => messages.push(payload.to_vec()),
            0x80 => {
                let block = std::str::from_utf8(payload).unwrap();
                assert!(block.lines().any(|line| line == expected_line));
                ended = true;
            }
            other => panic!("unexpected gRPC-Web record flag {other}"),
        }
        bytes = remaining;
    }
    assert!(ended, "one complete body-encoded terminal status");
    messages
}

#[cfg(feature = "grpc-web")]
fn web_frames(content_type: &str, encoded: &[u8]) -> Vec<Vec<u8>> {
    web_frames_with_status(content_type, encoded, "4")
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_pre_shape_deadlines_emit_body_status_in_every_mode() {
    for content_type in WEB_TYPES {
        let mut peer = Peer::new(Kind::Hold, 65535).await;
        let response = peer.request("Call", Some("20m"), content_type).await;
        peer.entered().await;
        tokio::time::advance(Duration::from_millis(21)).await;
        peer.expired().await;
        let response = drive(response)
            .await
            .expect("valid pre-header web deadline response");
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            content_type
        );
        assert!(response.headers().get("grpc-status").is_none());
        let (body, trailers) = collect(response.into_body()).await;
        assert!(trailers.is_none());
        assert!(web_frames(content_type, &body).is_empty());
        peer.recovery().await;
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_boundary_deadlines_preserve_complete_messages_and_text_carry() {
    for content_type in WEB_TYPES {
        // The complete 5-byte frame + tag/length + payload has raw lengths
        // 9,10,11, exercising text carries of 0,1,2 bytes respectively.
        for length in [2, 3, 4] {
            let mut peer = Peer::new(Kind::Output(Shape::ServerStream, length), 65535).await;
            let response = peer.request("Call", Some("20m"), content_type).await;
            let response = drive(response).await.expect("web response headers");
            assert_eq!(
                response.headers().get("content-type").unwrap(),
                content_type
            );
            let mut body = response.into_body();
            let frame_length = 7 + length;
            let emitted = if content_type.contains("-text") {
                (frame_length / 3) * 4
            } else {
                frame_length
            };
            let mut encoded = Vec::new();
            while encoded.len() < emitted {
                let data = drive(body.data()).await.unwrap().unwrap();
                encoded.extend_from_slice(&data);
                body.flow_control().release_capacity(data.len()).unwrap();
            }
            assert_eq!(encoded.len(), emitted);
            turn().await;
            assert_eq!(peer.probe.dropped.load(Ordering::SeqCst), 0);
            tokio::time::advance(Duration::from_millis(21)).await;
            peer.expired().await;
            let (tail, trailers) = collect(body).await;
            encoded.extend_from_slice(&tail);
            assert!(trailers.is_none());
            let messages = web_frames(content_type, &encoded);
            let mut expected = vec![0x0a, u8::try_from(length).unwrap()];
            expected.extend(std::iter::repeat_n(b'x', length));
            assert_eq!(messages, [expected]);
            witnessed(|| peer.probe.producer_dropped.load(Ordering::SeqCst) == 1).await;
            peer.recovery().await;
        }
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_partial_message_deadlines_reset_instead_of_fabricating_body_status() {
    for content_type in WEB_TYPES {
        let mut peer = Peer::new(Kind::Output(Shape::ServerStream, 1024 * 1024), 7).await;
        let response = peer.request("Call", Some("20m"), content_type).await;
        let response = drive(response).await.expect("web response headers");
        let mut body = response.into_body();
        let mut received = 0;
        while received < 7 {
            let data = drive(body.data()).await.unwrap().unwrap();
            received += data.len();
            // Withhold every byte of flow-control credit.
        }
        assert_eq!(received, 7);
        tokio::time::advance(Duration::from_millis(21)).await;
        peer.expired().await;
        let error = drive(body.data())
            .await
            .expect("a partial web frame cannot end as valid successful body framing")
            .expect_err("no credit for additional web DATA");
        assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
        witnessed(|| peer.probe.producer_dropped.load(Ordering::SeqCst) == 1).await;
        peer.recovery().await;
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_partial_trailer_deadlines_reset_instead_of_appending_another_status() {
    for content_type in WEB_TYPES {
        let mut peer = Peer::new(Kind::EmptyUnary, 8).await;
        let response = peer.request("Call", Some("20m"), content_type).await;
        let response = drive(response).await.expect("web response headers");
        let mut body = response.into_body();
        let mut received = Vec::new();
        while received.len() < 8 {
            let data = drive(body.data()).await.unwrap().unwrap();
            received.extend_from_slice(&data);
            // Preserve zero remaining stream credit, including trailer bytes.
        }
        assert_eq!(received.len(), 8);
        if content_type.contains("-text") {
            use base64::Engine as _;
            let raw = base64::engine::general_purpose::STANDARD
                .decode(&received)
                .expect("two complete base64 quanta");
            assert_eq!(raw, [0, 0, 0, 0, 0, 0x80]);
        } else {
            assert_eq!(received.get(..5).unwrap(), [0, 0, 0, 0, 0]);
            assert_eq!(
                received.get(5),
                Some(&0x80),
                "the blocked record is the trailer itself"
            );
        }
        tokio::time::advance(Duration::from_millis(21)).await;
        peer.expired().await;
        let error = drive(body.data())
            .await
            .expect("a partial trailer cannot become a valid complete status")
            .expect_err("no credit for additional trailer DATA");
        assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
        peer.recovery().await;
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_committed_success_is_not_replaced_after_its_deadline() {
    for content_type in WEB_TYPES {
        let mut peer = Peer::new(Kind::CommittedEmptyUnary, 8).await;
        let response = peer.request("Call", Some("20m"), content_type).await;
        let response = drive(response).await.expect("web response headers");
        let mut body = response.into_body();
        let mut encoded = Vec::new();
        while encoded.len() < 8 {
            let data = drive(body.data()).await.unwrap().unwrap();
            encoded.extend_from_slice(&data);
        }
        assert_eq!(encoded.len(), 8);
        // Unlike the bounded-trailer case, the complete body and EOS fit in
        // the ordinary send buffer. Witness logical success before advancing
        // time; withheld wire credit alone cannot renew or fail that result.
        witnessed(|| {
            peer.probe.dropped.load(Ordering::SeqCst) == 1
                && *peer.probe.ends.lock().unwrap() == [Code::Ok]
        })
        .await;
        tokio::time::advance(Duration::from_millis(21)).await;
        turn().await;
        assert_eq!(peer.probe.starts.load(Ordering::SeqCst), 1);
        assert_eq!(*peer.probe.ends.lock().unwrap(), [Code::Ok]);
        body.flow_control().release_capacity(8).unwrap();
        let (tail, trailers) = collect(body).await;
        assert!(trailers.is_none());
        encoded.extend_from_slice(&tail);
        assert_eq!(
            web_frames_with_status(content_type, &encoded, "0"),
            [Vec::<u8>::new()]
        );
        peer.recovery().await;
    }
}
