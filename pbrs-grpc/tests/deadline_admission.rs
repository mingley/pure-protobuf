//! Requests waiting for peer capacity must stay outside the HTTP/2 stream queue.
//!
//! The original failing fixtures remain byte-for-byte in the evidence archive.
//! These current-thread, paused-clock peers check the actual native/opaque opens.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "bounded raw-peer protocol assertions"
)]

use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Call, CallHandle, Channel, Code, Request};
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const BOUND: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_millis(50);
type ResultFuture = Pin<Box<dyn Future<Output = Result<(), Code>> + Send>>;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Unary,
    ServerStream,
    ClientStream,
    Bidi,
}

const SHAPES: [Shape; 4] = [
    Shape::Unary,
    Shape::ServerStream,
    Shape::ClientStream,
    Shape::Bidi,
];

impl Shape {
    fn path(self) -> &'static str {
        match self {
            Self::Unary => "/admission.Test/Unary",
            Self::ServerStream => "/admission.Test/ServerStream",
            Self::ClientStream => "/admission.Test/ClientStream",
            Self::Bidi => "/admission.Test/Bidi",
        }
    }
}

fn erase<T: Send + 'static>(call: Call<T>) -> (CallHandle, ResultFuture) {
    let handle = call.handle();
    (
        handle,
        Box::pin(async move { call.await.map(|_| ()).map_err(|e| e.code()) }),
    )
}

async fn native_call(
    channel: &Channel,
    shape: Shape,
    timeout: Option<Duration>,
) -> (CallHandle, ResultFuture) {
    let mut envelope = Request::new(());
    if let Some(timeout) = timeout {
        envelope.set_timeout(timeout);
    }
    match shape {
        Shape::Unary | Shape::ServerStream => {
            let mut request = Request::new(HelloRequest::new());
            if let Some(timeout) = timeout {
                request.set_timeout(timeout);
            }
            match shape {
                Shape::Unary => erase(channel.unary::<_, HelloReply>(shape.path(), request)),
                _ => erase(channel.server_streaming::<_, HelloReply>(shape.path(), request)),
            }
        }
        Shape::ClientStream => {
            let (sender, call) =
                channel.client_streaming::<HelloRequest, HelloReply>(shape.path(), envelope);
            sender
                .send(HelloRequest::new())
                .await
                .expect("queue request");
            sender.close();
            erase(call)
        }
        Shape::Bidi => {
            let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(shape.path(), envelope);
            sender
                .send(HelloRequest::new())
                .await
                .expect("queue request");
            sender.close();
            erase(call)
        }
    }
}

async fn pending(future: &mut ResultFuture) {
    poll_fn(|cx| {
        assert!(
            future.as_mut().poll(cx).is_pending(),
            "capacity wait completed early"
        );
        Poll::Ready(())
    })
    .await;
}

#[derive(Debug)]
struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

// Preserve partial reads when quiet() cancels a read. Preserve non-ACK frames
// seen while fencing SETTINGS; otherwise a late request could escape assertion.
struct Peer {
    io: DuplexStream,
    buffered: Vec<u8>,
    observed: VecDeque<Frame>,
}

impl Peer {
    async fn read(&mut self) -> Frame {
        loop {
            if self.buffered.len() >= 9 {
                let n = usize::from(self.buffered[0]) * 65536
                    + usize::from(self.buffered[1]) * 256
                    + usize::from(self.buffered[2]);
                assert!(n <= 16384, "oversized fixture frame");
                if self.buffered.len() >= 9 + n {
                    let frame = Frame {
                        kind: self.buffered[3],
                        flags: self.buffered[4],
                        stream: u32::from_be_bytes(
                            self.buffered[5..9].try_into().expect("stream id"),
                        ) & 0x7fff_ffff,
                        payload: self.buffered[9..9 + n].to_vec(),
                    };
                    self.buffered.drain(..9 + n);
                    return frame;
                }
            }
            let mut chunk = [0; 4096];
            let n = self.io.read(&mut chunk).await.expect("read frame");
            assert!(n != 0, "peer EOF");
            self.buffered.extend_from_slice(&chunk[..n]);
        }
    }

    async fn next(&mut self) -> Frame {
        match self.observed.pop_front() {
            Some(frame) => frame,
            None => self.read().await,
        }
    }

    async fn write(&mut self, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
        let length = u32::try_from(payload.len()).expect("fixture frame length");
        let mut bytes = Vec::with_capacity(9 + payload.len());
        bytes.extend_from_slice(&length.to_be_bytes()[1..]);
        bytes.extend_from_slice(&[kind, flags]);
        bytes.extend_from_slice(&stream.to_be_bytes());
        bytes.extend_from_slice(payload);
        self.io.write_all(&bytes).await.expect("write frame");
        self.io.flush().await.expect("flush frame");
    }

    async fn settings(&mut self, streams: u32) {
        let mut bytes = vec![0, 3];
        bytes.extend_from_slice(&streams.to_be_bytes());
        self.write(4, 0, 0, &bytes).await;
        tokio::time::timeout(BOUND, async {
            loop {
                let frame = self.read().await;
                if frame.kind == 4 && frame.flags == 1 {
                    assert_eq!(frame.stream, 0);
                    assert!(frame.payload.is_empty());
                    break;
                }
                self.observed.push_back(frame);
            }
        })
        .await
        .expect("SETTINGS ACK stalled");
    }

    async fn request_eof(&mut self, stream: u32) {
        tokio::time::timeout(BOUND, async {
            let mut headers = false;
            loop {
                let frame = self.next().await;
                if frame.stream == 0 {
                    continue;
                }
                assert_eq!(frame.stream, stream, "unexpected request stream: {frame:?}");
                if frame.kind == 1 {
                    headers = true;
                }
                if frame.kind == 0 && frame.flags & 1 != 0 {
                    assert!(headers, "DATA before HEADERS");
                    assert!(!frame.payload.is_empty(), "request has an encoded message");
                    return;
                }
            }
        })
        .await
        .expect("request EOF stalled");
    }

    async fn quiet_after_abort(&mut self) {
        while let Ok(frame) = tokio::time::timeout(Duration::from_millis(5), self.next()).await {
            eprintln!("after queued abort: {frame:?}");
            assert!(
                frame.stream <= 1,
                "aborted request emitted a stream frame: {frame:?}"
            );
        }
    }

    async fn reply(&mut self, stream: u32) {
        // Static :status 200; literal content-type application/grpc.
        self.write(
            1,
            4,
            stream,
            b"\x88\x00\x0ccontent-type\x10application/grpc",
        )
        .await;
        self.write(0, 0, stream, b"\x00\x00\x00\x00\x00").await;
        // Literal grpc-status 0, END_HEADERS | END_STREAM.
        self.write(1, 5, stream, b"\x00\x0bgrpc-status\x010").await;
    }
}

async fn connected() -> (Channel, Peer) {
    let (client, io) = tokio::io::duplex(65536);
    let connect = tokio::spawn(Channel::from_io(client, "localhost"));
    let mut peer = Peer {
        io,
        buffered: Vec::new(),
        observed: VecDeque::new(),
    };
    let mut preface = [0; 24];
    peer.io.read_exact(&mut preface).await.expect("preface");
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    assert_eq!(peer.read().await.kind, 4);
    peer.settings(1).await;
    let channel = connect.await.expect("connect task").expect("channel");
    (channel, peer)
}

#[derive(Clone, Copy)]
enum Abort {
    Deadline,
    Cancel,
    Drop,
    DeadlineAtRelease,
    CancelAtRelease,
    BothAtRelease,
}
#[derive(Clone, Copy)]
enum Release {
    Grow,
    Reset,
    ZeroThenGrow,
}

async fn release(peer: &mut Peer, release: Release) {
    match release {
        Release::Grow | Release::ZeroThenGrow => peer.settings(2).await,
        Release::Reset => peer.write(3, 0, 1, &8u32.to_be_bytes()).await,
    }
    peer.quiet_after_abort().await;
}

async fn later_good_call(channel: &Channel, peer: &mut Peer) {
    let (_, good) = native_call(channel, Shape::Unary, None).await;
    let good = tokio::spawn(good);
    // No abandoned request may consume stream id 3 or evict this connection.
    peer.request_eof(3).await;
    peer.reply(3).await;
    assert_eq!(
        tokio::time::timeout(BOUND, good)
            .await
            .expect("good call stalled")
            .expect("good task"),
        Ok(())
    );
}

async fn check_native(abort: Abort, released: Release) {
    for shape in SHAPES {
        let (channel, mut peer) = connected().await;
        let (_, first) = native_call(&channel, Shape::Unary, None).await;
        let first = tokio::spawn(first);
        peer.request_eof(1).await;
        if matches!(released, Release::ZeroThenGrow) {
            peer.settings(0).await;
        }
        let (handle, mut waiting) = native_call(&channel, shape, Some(TIMEOUT)).await;
        pending(&mut waiting).await;
        match abort {
            Abort::Deadline => {
                tokio::time::advance(TIMEOUT).await;
                assert_eq!(waiting.await, Err(Code::DeadlineExceeded), "{shape:?}");
            }
            Abort::Cancel => {
                handle.cancel();
                assert_eq!(waiting.await, Err(Code::Cancelled), "{shape:?}");
            }
            Abort::Drop => drop(waiting),
            Abort::DeadlineAtRelease | Abort::BothAtRelease => {
                tokio::time::advance(TIMEOUT).await;
                if matches!(abort, Abort::BothAtRelease) {
                    handle.cancel();
                }
                // Leave the call unpolled while the connection driver applies
                // fresh capacity. Its next possible admission poll must still
                // reject the request. Preserve prefer_deadline's status policy.
                release(&mut peer, released).await;
                assert_eq!(waiting.await, Err(Code::DeadlineExceeded), "{shape:?}");
            }
            Abort::CancelAtRelease => {
                handle.cancel();
                release(&mut peer, released).await;
                assert_eq!(waiting.await, Err(Code::Cancelled), "{shape:?}");
            }
        }
        if !matches!(
            abort,
            Abort::DeadlineAtRelease | Abort::CancelAtRelease | Abort::BothAtRelease
        ) {
            release(&mut peer, released).await;
        }
        later_good_call(&channel, &mut peer).await;
        first.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn all_shapes_expired_before_capacity_growth_emit_no_request() {
    check_native(Abort::Deadline, Release::Grow).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_expired_before_stream_release_emit_no_request() {
    check_native(Abort::Deadline, Release::Reset).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_cancelled_before_capacity_growth_emit_no_request() {
    check_native(Abort::Cancel, Release::Grow).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_cancelled_before_stream_release_emit_no_request() {
    check_native(Abort::Cancel, Release::Reset).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_dropped_before_capacity_growth_emit_no_request() {
    check_native(Abort::Drop, Release::Grow).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_dropped_before_stream_release_emit_no_request() {
    check_native(Abort::Drop, Release::Reset).await;
}
#[tokio::test(start_paused = true)]
async fn all_shapes_abort_while_peer_capacity_is_zero() {
    for abort in [Abort::Deadline, Abort::Cancel, Abort::Drop] {
        check_native(abort, Release::ZeroThenGrow).await;
    }
}

#[tokio::test(start_paused = true)]
async fn all_shapes_recheck_cancel_and_deadline_after_capacity_wakes_them() {
    for abort in [
        Abort::DeadlineAtRelease,
        Abort::CancelAtRelease,
        Abort::BothAtRelease,
    ] {
        check_native(abort, Release::Grow).await;
    }
}

// Use the independent registry h2 peer to decode HPACK and inspect the remote
// deadline. The raw peer above independently checks forbidden wire emissions.
struct HeaderPeer {
    channel: Channel,
    release: Option<tokio::sync::oneshot::Sender<()>>,
    header: tokio::sync::oneshot::Receiver<Duration>,
    first: tokio::task::JoinHandle<Result<(), Code>>,
    driver: tokio::task::JoinHandle<()>,
}

impl HeaderPeer {
    async fn new() -> Self {
        let (client, server) = tokio::io::duplex(65536);
        let (release, released) = tokio::sync::oneshot::channel();
        let (held, occupied) = tokio::sync::oneshot::channel();
        let (header, received) = tokio::sync::oneshot::channel();
        let driver = tokio::spawn(async move {
            let mut connection = h2::server::Builder::new()
                .max_concurrent_streams(1)
                .handshake::<_, bytes::Bytes>(server)
                .await
                .expect("header peer handshake");
            let (first_request, mut first_response) = connection
                .accept()
                .await
                .expect("first request")
                .expect("valid first request");
            held.send(()).expect("first request fence");
            tokio::select! {
                biased;
                result = released => result.expect("release first stream"),
                result = connection.accept() => panic!("second request bypassed peer quota: {result:?}"),
            }
            first_response.send_reset(h2::Reason::CANCEL);
            drop(first_request);
            let (request, mut response) = connection
                .accept()
                .await
                .expect("second request")
                .expect("valid second request");
            let timeout = request
                .headers()
                .get("grpc-timeout")
                .expect("timeout header")
                .to_str()
                .expect("ASCII timeout");
            header
                .send(pbrs_grpc::timeout::parse_timeout(timeout).expect("valid timeout"))
                .expect("timeout receiver");
            let mut send = response
                .send_response(
                    http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .expect("response"),
                    false,
                )
                .expect("response headers");
            send.send_data(bytes::Bytes::from_static(b"\x00\x00\x00\x00\x00"), false)
                .expect("response message");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
            send.send_trailers(trailers).expect("response trailers");
            while let Some(result) = connection.accept().await {
                drop(result.expect("connection remains valid"));
            }
        });
        let channel = Channel::from_io(client, "localhost")
            .await
            .expect("header channel");
        let (_, first) = native_call(&channel, Shape::Unary, None).await;
        let first = tokio::spawn(first);
        tokio::time::timeout(BOUND, occupied)
            .await
            .expect("occupied fence stalled")
            .expect("occupied fence");
        Self {
            channel,
            release: Some(release),
            header: received,
            first,
            driver,
        }
    }

    async fn finish(&mut self, waiting: ResultFuture) -> Duration {
        self.release
            .take()
            .expect("one release")
            .send(())
            .expect("header peer alive");
        let waiting = tokio::spawn(waiting);
        let header = tokio::time::timeout(BOUND, &mut self.header)
            .await
            .expect("header stalled")
            .expect("header received");
        assert_eq!(
            tokio::time::timeout(BOUND, waiting)
                .await
                .expect("request stalled")
                .expect("request task"),
            Ok(())
        );
        header
    }
}

impl Drop for HeaderPeer {
    fn drop(&mut self) {
        self.first.abort();
        self.driver.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn all_shapes_refresh_remote_timeout_on_the_actual_admission_poll() {
    for shape in SHAPES {
        let mut peer = HeaderPeer::new().await;
        let (_, mut waiting) =
            native_call(&peer.channel, shape, Some(Duration::from_secs(1))).await;
        pending(&mut waiting).await;
        tokio::time::advance(Duration::from_millis(140)).await;
        pending(&mut waiting).await;
        tokio::time::advance(Duration::from_millis(140)).await;
        assert_eq!(
            peer.finish(waiting).await,
            Duration::from_millis(720),
            "{shape:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn native_timeout_preserves_the_existing_twenty_millisecond_boundary() {
    for (wait, expected) in [(10, 1000), (20, 980)] {
        let mut peer = HeaderPeer::new().await;
        let (_, mut waiting) =
            native_call(&peer.channel, Shape::Unary, Some(Duration::from_secs(1))).await;
        pending(&mut waiting).await;
        tokio::time::advance(Duration::from_millis(wait)).await;
        assert_eq!(peer.finish(waiting).await, Duration::from_millis(expected));
    }
}

#[cfg(feature = "tonic")]
mod opaque {
    use super::*;
    use bytes::Bytes;
    use http_body::{Body, Frame};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::Context;
    use tower::ServiceExt;

    enum OpaqueAbort {
        Deadline,
        Drop,
    }

    struct CountBody {
        polls: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
        data: Option<Bytes>,
    }
    impl Body for CountBody {
        type Data = Bytes;
        type Error = std::convert::Infallible;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(self.data.take().map(|data| Ok(Frame::data(data))))
        }
        fn is_end_stream(&self) -> bool {
            self.data.is_none()
        }
    }
    impl Drop for CountBody {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    fn call(
        channel: &Channel,
        shape: Shape,
        timeout: Duration,
    ) -> (ResultFuture, Arc<AtomicUsize>, Arc<AtomicBool>) {
        let polls = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        let body = tonic::body::Body::new(CountBody {
            polls: polls.clone(),
            dropped: dropped.clone(),
            data: Some(Bytes::from_static(b"\x00\x00\x00\x00\x00")),
        });
        let request = http::Request::post(shape.path())
            .header("content-type", "application/grpc")
            .header("grpc-timeout", pbrs_grpc::timeout::encode_timeout(timeout))
            .body(body)
            .expect("opaque request");
        let future = channel.clone().oneshot(request);
        (
            Box::pin(async move {
                future
                    .await
                    .map(|_| ())
                    .map_err(|e| Code::from_i32(e.code() as i32))
            }),
            polls,
            dropped,
        )
    }

    async fn check(abort: OpaqueAbort, released: Release) {
        for shape in SHAPES {
            let (channel, mut peer) = connected().await;
            let (_, first) = native_call(&channel, Shape::Unary, None).await;
            let first = tokio::spawn(first);
            peer.request_eof(1).await;
            if matches!(released, Release::ZeroThenGrow) {
                peer.settings(0).await;
            }
            let (mut waiting, polls, dropped) = call(&channel, shape, TIMEOUT);
            pending(&mut waiting).await;
            assert_eq!(
                polls.load(Ordering::SeqCst),
                0,
                "body started before admission"
            );
            match abort {
                OpaqueAbort::Deadline => {
                    tokio::time::advance(TIMEOUT).await;
                    assert_eq!(waiting.await, Err(Code::DeadlineExceeded));
                }
                // Opaque Service futures are cancelled by dropping the future.
                OpaqueAbort::Drop => drop(waiting),
            }
            assert!(
                dropped.load(Ordering::SeqCst),
                "queued body retained after abort"
            );
            assert_eq!(polls.load(Ordering::SeqCst), 0, "queued body was consumed");
            release(&mut peer, released).await;
            later_good_call(&channel, &mut peer).await;
            first.abort();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn all_opaque_paths_expired_before_capacity_growth_emit_no_request() {
        check(OpaqueAbort::Deadline, Release::Grow).await;
    }
    #[tokio::test(start_paused = true)]
    async fn all_opaque_paths_expired_before_stream_release_emit_no_request() {
        check(OpaqueAbort::Deadline, Release::Reset).await;
    }
    #[tokio::test(start_paused = true)]
    async fn all_opaque_paths_dropped_before_capacity_growth_emit_no_request() {
        check(OpaqueAbort::Drop, Release::Grow).await;
    }
    #[tokio::test(start_paused = true)]
    async fn all_opaque_paths_dropped_before_stream_release_emit_no_request() {
        check(OpaqueAbort::Drop, Release::Reset).await;
    }
    #[tokio::test(start_paused = true)]
    async fn opaque_abort_while_peer_capacity_is_zero() {
        for abort in [OpaqueAbort::Deadline, OpaqueAbort::Drop] {
            check(abort, Release::ZeroThenGrow).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn all_opaque_paths_refresh_exact_remote_timeout_on_admission() {
        for shape in SHAPES {
            let mut peer = HeaderPeer::new().await;
            let (mut waiting, polls, _) = call(&peer.channel, shape, Duration::from_secs(1));
            pending(&mut waiting).await;
            tokio::time::advance(Duration::from_millis(140)).await;
            pending(&mut waiting).await;
            tokio::time::advance(Duration::from_millis(140)).await;
            assert_eq!(polls.load(Ordering::SeqCst), 0, "body started while queued");
            assert_eq!(peer.finish(waiting).await, Duration::from_millis(720));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn opaque_timeout_does_not_use_native_twenty_millisecond_rounding() {
        let mut peer = HeaderPeer::new().await;
        let (mut waiting, _, _) = call(&peer.channel, Shape::Unary, Duration::from_secs(1));
        pending(&mut waiting).await;
        tokio::time::advance(Duration::from_millis(10)).await;
        assert_eq!(peer.finish(waiting).await, Duration::from_millis(990));
    }
}
