//! A closed request producer must not hide an already-received peer rejection.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "integration test"
)]

use bytes::Bytes;
use pbrs_grpc::codec::CodecMessage;
use pbrs_grpc::hello::HelloReply;
use pbrs_grpc::{Channel, ChannelConfig, Code, Request, Status, StreamSender};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(15);

struct GateState {
    entered: Option<oneshot::Sender<()>>,
    released: bool,
}

struct EncodeGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl EncodeGate {
    fn new(entered: oneshot::Sender<()>) -> Self {
        Self {
            state: Mutex::new(GateState {
                entered: Some(entered),
                released: false,
            }),
            changed: Condvar::new(),
        }
    }

    fn block_once(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entered) = state.entered.take() {
            let _ = entered.send(());
            let (next, timeout) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(30), |state| !state.released)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            assert!(!timeout.timed_out() || state.released, "encode gate timed out");
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.released = true;
        self.changed.notify_all();
    }
}

// Unblock the worker even when a timeout or assertion aborts the test.
struct ReleaseOnDrop(Arc<EncodeGate>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct GateMessage {
    gate: Arc<EncodeGate>,
    fail_encode: bool,
}

impl CodecMessage for GateMessage {
    fn encoded_len(&self) -> usize {
        tokio::task::block_in_place(|| self.gate.block_once());
        0
    }

    fn encode_payload<W: pbrs::WireOut>(&self, _out: &mut W) -> Result<(), Status> {
        if self.fail_encode {
            Err(Status::invalid_argument("local encoder"))
        } else {
            Ok(())
        }
    }

    fn decode_payload(_payload: Bytes) -> Result<Self, Status> {
        Ok(Self::empty())
    }

    fn empty() -> Self {
        let (entered, _) = oneshot::channel();
        Self {
            gate: Arc::new(EncodeGate::new(entered)),
            fail_encode: false,
        }
    }
}

/// Observe complete HEADERS and RST_STREAM frames written by the peer.
struct PeerWriteWitness {
    io: DuplexStream,
    pending: Vec<u8>,
    headers: Option<oneshot::Sender<()>>,
    reset: Option<oneshot::Sender<()>>,
}

impl PeerWriteWitness {
    fn record(&mut self, bytes: &[u8]) {
        if self.headers.is_none() && self.reset.is_none() {
            return;
        }
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 9 {
            let len = (usize::from(self.pending[0]) << 16)
                | (usize::from(self.pending[1]) << 8)
                | usize::from(self.pending[2]);
            if self.pending.len() < 9 + len {
                break;
            }
            let stream = u32::from_be_bytes(self.pending[5..9].try_into().unwrap()) & 0x7fff_ffff;
            if stream == 1 && self.pending[3] == 1 && self.pending[4] & 4 != 0 {
                if let Some(headers) = self.headers.take() {
                    let _ = headers.send(());
                }
            }
            if stream == 1 && self.pending[3] == 3 {
                if let Some(reset) = self.reset.take() {
                    let _ = reset.send(());
                }
            }
            if self.headers.is_none() && self.reset.is_none() {
                self.pending.clear();
                return;
            }
            self.pending.drain(..9 + len);
        }
        assert!(self.pending.len() < 64 * 1024, "unexpected peer frame size");
    }
}

impl AsyncRead for PeerWriteWitness {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for PeerWriteWitness {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.io).poll_write(cx, bytes);
        if let Poll::Ready(Ok(count)) = result {
            self.record(&bytes[..count]);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    HttpError,
    GrpcError,
    HttpInitialOk,
    GrpcInitialOk,
    Refused,
    NoError,
    LocalProducer,
    LocalBudget,
    LocalEncode,
    CallerCancel,
    DeadlineAndCancel,
}

impl Case {
    fn no_headers(self) -> bool {
        matches!(self, Self::Refused)
    }

    fn grpc_response(self) -> bool {
        matches!(self, Self::GrpcError | Self::GrpcInitialOk | Self::NoError)
    }

    fn initial_ok(self) -> bool {
        matches!(self, Self::HttpInitialOk | Self::GrpcInitialOk)
    }

    fn reset_reason(self) -> h2::Reason {
        match self {
            Self::Refused => h2::Reason::REFUSED_STREAM,
            Self::NoError => h2::Reason::NO_ERROR,
            _ => h2::Reason::CANCEL,
        }
    }
}

type TestCall = Pin<Box<dyn Future<Output = Result<(), Status>> + Send>>;

async fn exercise_closed_upload(bidi: bool, case: Case) {
    let (client_io, peer_io) = tokio::io::duplex(64 * 1024);
    let (headers_tx, headers_rx) = oneshot::channel();
    let (reset_tx, reset_rx) = oneshot::channel();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (prepared_tx, prepared_rx) = oneshot::channel();
    let gate = Arc::new(EncodeGate::new(entered_tx));
    let _release_on_drop = ReleaseOnDrop(gate.clone());
    let peer_io = PeerWriteWitness {
        io: peer_io,
        pending: Vec::new(),
        headers: if case.no_headers() { None } else { Some(headers_tx) },
        reset: Some(reset_tx),
    };

    let peer = tokio::spawn(async move {
        tokio::time::timeout(WAIT, async {
            let mut connection = h2::server::handshake(peer_io).await.expect("handshake");
            let mut ping = connection.ping_pong().expect("PING handle");
            let (request, mut respond) = connection
                .accept()
                .await
                .expect("request")
                .expect("request headers");
            entered_rx.await.expect("closed encoder entered");

            let send = if case.no_headers() {
                respond.send_reset(case.reset_reason());
                None
            } else {
                let mut response = http::Response::builder()
                    .status(if case.grpc_response() { 200 } else { 503 })
                    .header("content-type", "application/grpc")
                    .header("x-terminal", "peer");
                if case.grpc_response() || case.initial_ok() {
                    response = response.header(
                        "grpc-status",
                        if case.initial_ok() { "0" } else { "8" },
                    );
                }
                let mut send = respond
                    .send_response(response.body(()).unwrap(), case.grpc_response())
                    .expect("response headers");
                tokio::select! {
                    result = headers_rx => result.expect("HEADERS write witness"),
                    incoming = connection.accept() => {
                        panic!("unexpected request before HEADERS: {incoming:?}");
                    }
                }
                send.send_reset(case.reset_reason());
                Some(send)
            };

            tokio::select! {
                result = reset_rx => result.expect("RST_STREAM write witness"),
                incoming = connection.accept() => {
                    panic!("unexpected request before reset: {incoming:?}");
                }
            }
            // HEADERS and RST_STREAM were written before this PING. Its ACK
            // confirms that the client connection processed both frames.
            tokio::select! {
                result = ping.ping(h2::Ping::opaque()) => {
                    result.expect("PING acknowledgement");
                }
                incoming = connection.accept() => {
                    panic!("unexpected request before acknowledgement: {incoming:?}");
                }
            }
            prepared_tx.send(()).expect("release test");
            let held = (request, respond, send);
            if let Some(Ok(_)) = connection.accept().await {
                panic!("unexpected second request");
            }
            drop(held);
        })
        .await
        .expect("peer synchronization timed out");
    });

    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        // The framed request is five bytes, so 1024 forces try_send_data.
        ChannelConfig::new().stream_buffer(4).max_send_buffer_size(1024),
    )
    .await
    .expect("channel");
    let channel = if matches!(case, Case::LocalBudget) {
        channel.byte_budget(1)
    } else {
        channel
    };
    let mut request = Request::new(());
    if matches!(case, Case::DeadlineAndCancel) {
        request.set_timeout(Duration::from_secs(2));
    }
    let (sender, handle, call): (StreamSender<GateMessage>, _, TestCall) = if bidi {
        let (sender, call) =
            channel.bidi::<GateMessage, HelloReply>("/test.Service/Bidi", request);
        let handle = call.handle();
        (sender, handle, Box::pin(async move { call.await.map(|_| ()) }))
    } else {
        let (sender, call) =
            channel.client_streaming::<GateMessage, HelloReply>("/test.Service/Upload", request);
        let handle = call.handle();
        (sender, handle, Box::pin(async move { call.await.map(|_| ()) }))
    };
    tokio::time::timeout(
        WAIT,
        sender.send(GateMessage {
            gate: gate.clone(),
            fail_encode: matches!(case, Case::LocalEncode),
        }),
    )
    .await
    .expect("producer send timed out")
    .expect("producer send");
    if matches!(case, Case::LocalProducer) {
        tokio::time::timeout(WAIT, sender.fail(Status::invalid_argument("local producer")))
            .await
            .expect("producer failure timed out");
    } else {
        drop(sender);
    }
    let call = tokio::spawn(call);
    tokio::time::timeout(WAIT, prepared_rx)
        .await
        .expect("peer preparation timed out")
        .expect("peer preparation failed");

    if matches!(case, Case::DeadlineAndCancel) {
        // The call deadline was created before the codec entered. Waiting one
        // full request timeout after this barrier makes both signals ready.
        tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(2)).await;
        handle.cancel();
    } else if matches!(case, Case::CallerCancel) {
        handle.cancel();
    }
    gate.release();

    let status = tokio::time::timeout(WAIT, call)
        .await
        .expect("call timed out")
        .expect("call task")
        .expect_err("the request must fail");
    let expected = match case {
        Case::HttpError | Case::Refused => Code::Unavailable,
        Case::GrpcError | Case::NoError | Case::LocalBudget => Code::ResourceExhausted,
        Case::LocalProducer | Case::LocalEncode => Code::InvalidArgument,
        Case::HttpInitialOk | Case::GrpcInitialOk | Case::CallerCancel => Code::Cancelled,
        Case::DeadlineAndCancel => Code::DeadlineExceeded,
    };
    assert_eq!(status.code(), expected, "bidi={bidi}, case={case:?}");
    assert_eq!(
        channel.byte_budget_allocated(),
        0,
        "budget must be released after failure; bidi={bidi}, case={case:?}"
    );
    match case {
        Case::HttpError | Case::GrpcError | Case::NoError => {
            assert_eq!(status.metadata().get("x-terminal"), Some("peer"));
        }
        Case::LocalProducer => assert_eq!(status.message(), "local producer"),
        Case::LocalEncode => assert_eq!(status.message(), "local encoder"),
        Case::CallerCancel => assert_eq!(status.message(), "cancelled"),
        _ => {}
    }
    peer.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_upload_prefers_queued_rejection_without_masking_local_failures() {
    for bidi in [false, true] {
        for case in [
            Case::HttpError,
            Case::GrpcError,
            Case::HttpInitialOk,
            Case::GrpcInitialOk,
            Case::Refused,
            Case::NoError,
            Case::LocalProducer,
            Case::LocalBudget,
            Case::LocalEncode,
            Case::CallerCancel,
        ] {
            exercise_closed_upload(bidi, case).await;
        }
    }
    // The bidi fast path also accepts a deadline; both signals must resolve
    // to the deadline when it has already expired.
    exercise_closed_upload(true, Case::DeadlineAndCancel).await;
}
