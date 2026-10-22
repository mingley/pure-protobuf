//! Responses and request uploads progress independently under HTTP/2 flow control.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "independent local HTTP/2 peer and regression assertions"
)]
use bytes::Bytes;
use pbrs_grpc::{ByteBudgetTracker, Channel, Code, HelloReply, HelloRequest, Request};
use std::future::{Future, poll_fn};
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const GUARD: Duration = Duration::from_secs(2);
const PAYLOAD: usize = 1024 * 1024;

async fn guarded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(GUARD, future)
        .await
        .expect("operation waited for unavailable request upload credit")
}
fn message(size: usize) -> HelloRequest {
    let mut request = HelloRequest::new();
    request.set_name("x".repeat(size));
    request
}
fn frame(text: &str) -> Bytes {
    assert!(text.len() < 128);
    let mut bytes = vec![0];
    bytes.extend_from_slice(&u32::try_from(text.len() + 2).unwrap().to_be_bytes());
    bytes.extend_from_slice(&[0x0a, u8::try_from(text.len()).unwrap()]);
    bytes.extend_from_slice(text.as_bytes());
    Bytes::from(bytes)
}
fn headers() -> http::Response<()> {
    http::Response::builder()
        .header("content-type", "application/grpc")
        .header("x-initial", "observed")
        .body(())
        .unwrap()
}
fn trailers() -> http::HeaderMap {
    let mut map = http::HeaderMap::new();
    map.insert("grpc-status", http::HeaderValue::from_static("0"));
    map.insert("x-finished", http::HeaderValue::from_static("valid"));
    map
}
async fn read_request(body: &mut h2::RecvStream) -> (usize, bool) {
    let mut received = 0;
    while let Some(data) = body.data().await {
        match data {
            Ok(data) => {
                received += data.len();
                body.flow_control().release_capacity(data.len()).unwrap();
            }
            Err(_) => return (received, false),
        }
    }
    (received, true)
}

// Observe complete END_STREAM HEADERS actually written to TCP. Queuing a
// response and immediately resetting it can discard headers inside h2, which
// would make a response-versus-reset assertion test a different wire exchange.
#[derive(Default)]
struct WrittenHeaders {
    count: AtomicUsize,
    initial_count: AtomicUsize,
    resets: AtomicUsize,
    changed: tokio::sync::Notify,
}
impl WrittenHeaders {
    async fn wait_reset(&self, count: usize) {
        while self.resets.load(Ordering::Acquire) < count {
            self.changed.notified().await;
        }
    }
    async fn wait_initial(&self) {
        while self.initial_count.load(Ordering::Acquire) == 0 {
            self.changed.notified().await;
        }
    }
    async fn wait_for(&self, count: usize) {
        while self.count.load(Ordering::Acquire) < count {
            self.changed.notified().await;
        }
    }
}
struct ObservedSocket {
    socket: tokio::net::TcpStream,
    pending: Vec<u8>,
    written: Arc<WrittenHeaders>,
}
#[allow(
    clippy::indexing_slicing,
    reason = "the frame length is checked before slicing each complete HTTP/2 frame"
)]
impl ObservedSocket {
    fn record(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 9 {
            let length = (usize::from(self.pending[0]) << 16)
                | (usize::from(self.pending[1]) << 8)
                | usize::from(self.pending[2]);
            if self.pending.len() < length + 9 {
                break;
            }
            if self.pending[3] == 1 {
                self.written.initial_count.fetch_add(1, Ordering::Release);
                self.written.changed.notify_one();
            }
            if self.pending[3] == 1 && self.pending[4] & 1 != 0 {
                self.written.count.fetch_add(1, Ordering::Release);
                self.written.changed.notify_one();
            }
            if self.pending[3] == 3 && length == 4 && self.pending[9..13] == [0; 4] {
                self.written.resets.fetch_add(1, Ordering::Release);
                self.written.changed.notify_one();
            }
            self.pending.drain(..length + 9);
        }
        assert!(self.pending.len() < 64 * 1024, "unexpected peer frame size");
    }
}
impl AsyncRead for ObservedSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}
#[allow(
    clippy::indexing_slicing,
    reason = "successful socket writes return the number of bytes consumed from these buffers"
)]
impl AsyncWrite for ObservedSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = &result {
            self.record(&buf[..*written]);
        }
        result
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write_vectored(cx, bufs);
        if let Poll::Ready(Ok(written)) = &result {
            let mut remaining = *written;
            for buf in bufs {
                let take = remaining.min(buf.len());
                self.record(&buf[..take]);
                remaining -= take;
                if remaining == 0 {
                    break;
                }
            }
        }
        result
    }
    fn is_write_vectored(&self) -> bool {
        self.socket.is_write_vectored()
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

async fn rejection(bidi: bool, reset: bool) {
    let (rejections_checked, wait_rejections_checked) = tokio::sync::oneshot::channel();
    let (reset_written, mut wait_reset_written) = tokio::sync::mpsc::channel(2);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, wait_ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let (socket, _) = listener.accept().await.unwrap();
        let written = Arc::new(WrittenHeaders::default());
        let socket = ObservedSocket {
            socket,
            pending: Vec::new(),
            written: written.clone(),
        };
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let mut ping = connection.ping_pong().unwrap();
        tokio::select! {
            result = ping.ping(h2::Ping::opaque()) => { result.unwrap(); },
            result = connection.accept() => panic!("request before SETTINGS barrier: {result:?}"),
        }
        ready.send(()).unwrap();
        let mut held = Vec::new();
        for index in 1..=2 {
            let (mut request, mut respond) = connection.accept().await.unwrap().unwrap();
            poll_fn(|cx| {
                assert!(request.body_mut().poll_data(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            let mut send = respond
                .send_response(
                    http::Response::builder()
                        .header("content-type", "application/grpc")
                        .header("grpc-status", "8")
                        .header("grpc-message", "busy")
                        .header("x-terminal", "rejected")
                        .header("grpc-status-details-bin", "AQID")
                        .body(())
                        .unwrap(),
                    true,
                )
                .unwrap();
            if reset {
                tokio::select! {
                    () = written.wait_for(index) => {},
                    result = connection.accept() => panic!("request before response write: {result:?}"),
                }
                send.send_reset(h2::Reason::NO_ERROR);
                tokio::select! {
                    () = written.wait_reset(index) => {},
                    result = connection.accept() => panic!("request before reset write: {result:?}"),
                }
                reset_written.send(()).await.unwrap();
            }
            // Keep the request open; no peer timeout or dropped handle can
            // accidentally unblock the upload in the no-reset cases.
            held.push((request, respond, send));
        }
        // Keep both uploads at zero credit until the client has checked both
        // terminal statuses. Granting credit before delivering the rejection
        // can fill connection credit with the retained, unread request bodies.
        tokio::select! {
            biased;
            result = wait_rejections_checked => result.unwrap(),
            result = connection.accept() => panic!("recovery before rejection checks: {result:?}"),
        }
        drop(held);
        connection
            .set_initial_window_size(u32::try_from(2 * PAYLOAD).unwrap())
            .unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        assert_eq!(request.uri().path(), "/test/Recovery");
        let mut body = request.into_body();
        let read = read_request(&mut body);
        tokio::pin!(read);
        tokio::select! {
            result = &mut read => assert!(result.1),
            result = connection.accept() => panic!("unexpected recovery stream: {result:?}"),
        }
        let mut recovered = respond.send_response(headers(), false).unwrap();
        recovered.send_data(frame("recovered"), false).unwrap();
        recovered.send_trailers(trailers()).unwrap();
        while let Some(request) = connection.accept().await {
            request.unwrap();
        }
        drop(recovered);
    });
    let budget = ByteBudgetTracker::with_limit(4 * PAYLOAD);
    let channel = Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024)
        .max_concurrent_rpcs(1)
        .with_byte_budget_tracker(budget.clone());
    guarded(wait_ready).await.unwrap();
    // Repeating the rejected RPC proves admission/byte permits are released.
    for _ in 0..2 {
        let status = if bidi {
            let (sender, call) =
                channel.bidi::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
            sender.send(message(PAYLOAD)).await.unwrap();
            sender.close();
            guarded(call).await.unwrap_err()
        } else {
            let (sender, call) = channel
                .client_streaming::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
            sender.send(message(PAYLOAD)).await.unwrap();
            sender.close();
            guarded(call).await.unwrap_err()
        };
        if reset {
            guarded(wait_reset_written.recv()).await.unwrap();
        }
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert_eq!(status.message(), "busy");
        assert_eq!(status.metadata().get("x-terminal"), Some("rejected"));
        assert_eq!(status.details(), &[1, 2, 3]);
        assert_eq!(budget.allocated(), 0);
        assert_eq!(budget.active_byte_permit_tokens(), 0);
    }
    rejections_checked.send(()).unwrap();
    recovery(&channel).await;
    peer.abort();
}

#[derive(Clone, Copy)]
enum Mode {
    Bidi,
    ClientHeaders,
    EarlyOk,
    TransportError,
    CancelBidi,
}

async fn duplex(mode: Mode, bidi: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (credit, allow_credit) = tokio::sync::oneshot::channel();
    let (ready, wait_ready) = tokio::sync::oneshot::channel();
    let (uploaded, upload) = tokio::sync::oneshot::channel();
    let (sent_headers, headers_ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let written = Arc::new(WrittenHeaders::default());
        let socket = ObservedSocket {
            socket,
            pending: Vec::new(),
            written: written.clone(),
        };
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let mut ping = connection.ping_pong().unwrap();
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
        let mut send = if matches!(mode, Mode::TransportError) {
            respond.send_reset(h2::Reason::INTERNAL_ERROR);
            None
        } else {
            let mut send = respond.send_response(headers(), false).unwrap();
            if !matches!(mode, Mode::ClientHeaders) {
                send.send_data(
                    frame(if matches!(mode, Mode::EarlyOk) {
                        "early"
                    } else {
                        "first"
                    }),
                    false,
                )
                .unwrap();
            }
            if matches!(mode, Mode::EarlyOk) {
                send.send_trailers(trailers()).unwrap();
            }
            Some(send)
        };
        // The headers-only control waits for an actual TCP write, so its
        // assertion cannot succeed merely because the peer queued nothing.
        if matches!(mode, Mode::ClientHeaders) {
            tokio::select! {
                () = written.wait_initial() => {},
                result = connection.accept() => panic!("unexpected stream before headers write: {result:?}"),
            }
        }
        sent_headers.send(()).unwrap();
        tokio::select! {
            biased;
            result = allow_credit => result.unwrap(),
            result = connection.accept() => panic!("unexpected stream before credit: {result:?}"),
        }
        connection
            .set_initial_window_size(u32::try_from(2 * PAYLOAD).unwrap())
            .unwrap();
        let result = {
            let read = read_request(&mut body);
            tokio::pin!(read);
            tokio::select! {
                result = &mut read => result,
                result = connection.accept() => panic!("unexpected stream during upload: {result:?}"),
            }
        };
        if matches!(mode, Mode::Bidi | Mode::ClientHeaders) {
            assert_eq!(result, (PAYLOAD + 9, true));
            let send = send.as_mut().unwrap();
            if matches!(mode, Mode::ClientHeaders) {
                send.send_data(frame("accepted"), false).unwrap();
            }
            send.send_trailers(trailers()).unwrap();
        }
        uploaded.send(result).unwrap();
        let (request, mut respond) = connection.accept().await.unwrap().unwrap();
        assert_eq!(request.uri().path(), "/test/Recovery");
        let mut recovery_body = request.into_body();
        let read = read_request(&mut recovery_body);
        tokio::pin!(read);
        let read = tokio::select! {
            result = &mut read => result,
            result = connection.accept() => panic!("unexpected recovery stream: {result:?}"),
        };
        assert!(read.1);
        let mut recovery = respond.send_response(headers(), false).unwrap();
        recovery.send_data(frame("recovered"), false).unwrap();
        recovery.send_trailers(trailers()).unwrap();
        while let Some(request) = connection.accept().await {
            request.unwrap();
        }
        drop((body, send, recovery));
    });
    let budget = ByteBudgetTracker::with_limit(4 * PAYLOAD);
    let channel = Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024)
        .max_concurrent_rpcs(1)
        .with_byte_budget_tracker(budget.clone());
    guarded(wait_ready).await.unwrap();
    if bidi {
        let (sender, call) =
            channel.bidi::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
        sender.send(message(PAYLOAD)).await.unwrap();
        sender.close();
        let result = guarded(call).await;
        if matches!(mode, Mode::TransportError) {
            assert_ne!(result.unwrap_err().code(), Code::DeadlineExceeded);
        } else {
            let mut response = result.unwrap();
            assert_eq!(response.metadata().get("x-initial"), Some("observed"));
            assert_eq!(
                guarded(response.get_mut().message())
                    .await
                    .unwrap()
                    .unwrap()
                    .message(),
                "first"
            );
            assert!(
                budget.allocated() > 0,
                "blocked encoded tail must retain its byte charge"
            );
            assert!(budget.active_byte_permit_tokens() > 0);
            if matches!(mode, Mode::CancelBidi) {
                drop(response);
                guarded(async {
                    while budget.allocated() != 0 || budget.active_byte_permit_tokens() != 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await;
            } else {
                credit.send(()).unwrap();
                assert_eq!(guarded(upload).await.unwrap(), (PAYLOAD + 9, true));
                assert!(
                    guarded(response.get_mut().message())
                        .await
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    response
                        .get_mut()
                        .trailers()
                        .await
                        .unwrap()
                        .get("x-finished"),
                    Some("valid")
                );
                assert_eq!(budget.allocated(), 0);
                assert_eq!(budget.active_byte_permit_tokens(), 0);
                // The documented concurrency cap follows response ownership.
                drop(response);
                recovery(&channel).await;
                peer.abort();
                return;
            }
        }
    } else {
        let (sender, call) =
            channel.client_streaming::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
        sender.send(message(PAYLOAD)).await.unwrap();
        sender.close();
        if matches!(mode, Mode::ClientHeaders) {
            tokio::pin!(call);
            // Polling drives the request; the peer cannot issue a response
            // message or terminal trailers until credit is explicitly granted.
            tokio::select! {
                result = &mut call => panic!("headers alone completed unary response: {result:?}"),
                result = headers_ready => result.unwrap(),
            }
            assert!(poll_fn(|cx| Poll::Ready(call.as_mut().poll(cx).is_pending())).await);
            credit.send(()).unwrap();
            assert_eq!(guarded(call).await.unwrap().get_ref().message(), "accepted");
            assert_eq!(guarded(upload).await.unwrap(), (PAYLOAD + 9, true));
            assert_eq!(budget.allocated(), 0);
            assert_eq!(budget.active_byte_permit_tokens(), 0);
            recovery(&channel).await;
            peer.abort();
            return;
        }
        let result = guarded(call).await;
        if matches!(mode, Mode::TransportError) {
            assert_ne!(result.unwrap_err().code(), Code::DeadlineExceeded);
        } else {
            let response = result.unwrap();
            assert_eq!(response.metadata().get("x-initial"), Some("observed"));
            assert_eq!(response.get_ref().message(), "early");
        }
    }
    assert_eq!(budget.allocated(), 0);
    assert_eq!(budget.active_byte_permit_tokens(), 0);
    credit.send(()).unwrap();
    // After a terminal response, cancelling the remaining request is valid.
    // The full-upload invariant applies to the live, nonterminal bidi case.
    let uploaded = guarded(upload).await.unwrap();
    if matches!(mode, Mode::CancelBidi | Mode::TransportError) {
        assert!(!uploaded.1, "cancelled upload must terminate with a reset");
    }
    recovery(&channel).await;
    peer.abort();
}

async fn recovery(channel: &Channel) {
    let response = guarded(
        channel.unary::<HelloRequest, HelloReply>("/test/Recovery", Request::new(message(1))),
    )
    .await
    .unwrap();
    assert_eq!(response.get_ref().message(), "recovered");
}

async fn aggregate_budget(bidi: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (ready, wait_ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut builder = h2::server::Builder::new();
        builder.initial_window_size(0);
        let mut connection = builder.handshake::<_, Bytes>(socket).await.unwrap();
        let mut ping = connection.ping_pong().unwrap();
        tokio::select! {
            result = ping.ping(h2::Ping::opaque()) => { result.unwrap(); },
            result = connection.accept() => panic!("request before SETTINGS barrier: {result:?}"),
        }
        ready.send(()).unwrap();
        let mut held = Vec::new();
        loop {
            let (request, mut respond) = connection.accept().await.unwrap().unwrap();
            if request.uri().path() != "/test/Recovery" {
                held.push((request, respond));
                continue;
            }
            connection
                .set_initial_window_size(u32::try_from(2 * PAYLOAD).unwrap())
                .unwrap();
            let mut body = request.into_body();
            let read = read_request(&mut body);
            tokio::pin!(read);
            tokio::select! {
                result = &mut read => assert!(result.1),
                result = connection.accept() => panic!("unexpected recovery stream: {result:?}"),
            }
            let mut send = respond.send_response(headers(), false).unwrap();
            send.send_data(frame("recovered"), false).unwrap();
            send.send_trailers(trailers()).unwrap();
            while let Some(request) = connection.accept().await {
                request.unwrap();
            }
            drop((send, held));
            break;
        }
    });
    // Each encoded batch fits alone; retaining both exceeds the configured
    // budget. Releasing the first charge while retaining its bytes is invalid.
    let budget = ByteBudgetTracker::with_limit(128 * 1024);
    let channel = Channel::connect(address)
        .await
        .unwrap()
        .max_send_buffer_size(16 * 1024)
        .max_concurrent_rpcs(1)
        .with_byte_budget_tracker(budget.clone());
    guarded(wait_ready).await.unwrap();
    let status = if bidi {
        let (sender, call) =
            channel.bidi::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
        sender.send(message(96 * 1024)).await.unwrap();
        sender.send(message(96 * 1024)).await.unwrap();
        sender.close();
        guarded(call).await.unwrap_err()
    } else {
        let (sender, call) =
            channel.client_streaming::<HelloRequest, HelloReply>("/test/Upload", Request::new(()));
        sender.send(message(96 * 1024)).await.unwrap();
        sender.send(message(96 * 1024)).await.unwrap();
        sender.close();
        guarded(call).await.unwrap_err()
    };
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(budget.allocated(), 0);
    assert_eq!(budget.active_byte_permit_tokens(), 0);
    recovery(&channel).await;
    peer.abort();
}

#[tokio::test]
async fn closed_client_upload_returns_terminal_rejection_without_credit() {
    rejection(false, false).await;
}
#[tokio::test]
async fn closed_bidi_upload_returns_terminal_rejection_without_credit() {
    rejection(true, false).await;
}
#[tokio::test]
async fn closed_client_upload_retains_rejection_with_peer_no_error_reset() {
    rejection(false, true).await;
}
#[tokio::test]
async fn closed_bidi_upload_retains_rejection_with_peer_no_error_reset() {
    rejection(true, true).await;
}
#[tokio::test]
async fn closed_client_upload_preserves_headers_and_completes_after_credit() {
    duplex(Mode::ClientHeaders, false).await;
}
#[tokio::test]
async fn closed_bidi_upload_returns_headers_and_message_before_credit() {
    duplex(Mode::Bidi, true).await;
}
#[tokio::test]
async fn closed_client_upload_returns_complete_ok_before_credit() {
    duplex(Mode::EarlyOk, false).await;
}
#[tokio::test]
async fn closed_client_upload_returns_transport_error_without_credit() {
    duplex(Mode::TransportError, false).await;
}
#[tokio::test]
async fn closed_bidi_upload_returns_transport_error_without_credit() {
    duplex(Mode::TransportError, true).await;
}
#[tokio::test]
async fn dropping_closed_bidi_response_releases_blocked_upload_budget() {
    duplex(Mode::CancelBidi, true).await;
}
#[tokio::test]
async fn closed_client_batches_cannot_bypass_aggregate_byte_budget() {
    aggregate_budget(false).await;
}
#[tokio::test]
async fn closed_bidi_batches_cannot_bypass_aggregate_byte_budget() {
    aggregate_budget(true).await;
}
