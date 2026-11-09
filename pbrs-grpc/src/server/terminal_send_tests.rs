//! Terminal writes must report transport failure to server accounting.

#![allow(
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "bounded synchronous test gate and wire-frame witness"
)]

#[cfg(feature = "grpc-web")]
use super::send_web_stream_response;
use super::{CancelFlag, send_stream_response};
use crate::binlog::{BinaryLogFilter, BinaryLogger, EventType, LogRecord, Logger, Sink};
use crate::channelz::Registry;
use crate::compression::CompressionAlgorithm;
use crate::config::Wire;
use crate::limits::{ByteBudgetTracker, MessageLimits};
use crate::request::Response;
use crate::server::rpc::{end_call, start_call};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{CallLabels, LifecycleObserver};
use crate::transport::{ServerBuilder, ServerConnection, h2 as backend};
use bytes::Bytes;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(15);

#[derive(Default)]
struct EndObserver(Arc<Mutex<Vec<Code>>>);

impl Clone for EndObserver {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl LifecycleObserver for EndObserver {
    fn on_server_call_end(&self, _: &CallLabels<'_>, status: &Status, _: Duration) {
        self.0.lock().expect("observer").push(status.code());
    }
}

/// Stop exactly before the response writer attempts its terminal frame.
#[derive(Debug)]
struct TrailerGate {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    released: Mutex<bool>,
    wake: Condvar,
}

impl TrailerGate {
    fn new(entered: oneshot::Sender<()>) -> Self {
        Self {
            entered: Mutex::new(Some(entered)),
            released: Mutex::new(false),
            wake: Condvar::new(),
        }
    }

    fn release(&self) {
        *self.released.lock().expect("gate") = true;
        self.wake.notify_all();
    }
}

impl Sink for TrailerGate {
    fn emit(&self, record: &LogRecord) {
        if record.entry.event != EventType::ServerTrailer {
            return;
        }
        if let Some(entered) = self.entered.lock().expect("gate sender").take() {
            let _sent = entered.send(());
        }
        // The connection driver is a different task. Blocking is bounded and
        // restricted to this test; production sinks must remain nonblocking.
        tokio::task::block_in_place(|| {
            let guard = self.released.lock().expect("gate");
            let (guard, timeout) = self
                .wake
                .wait_timeout_while(guard, Duration::from_secs(30), |released| !*released)
                .expect("gate wait");
            assert!(!timeout.timed_out() || *guard, "terminal gate timed out");
        });
    }
}

struct ReleaseOnDrop(Arc<TrailerGate>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Signal only after a complete client RST_STREAM frame has been written.
struct ResetWitness {
    io: DuplexStream,
    preface_left: usize,
    pending: Vec<u8>,
    reset: Option<oneshot::Sender<()>>,
}

impl ResetWitness {
    fn record(&mut self, mut bytes: &[u8]) {
        if self.reset.is_none() {
            return;
        }
        let prefix = self.preface_left.min(bytes.len());
        self.preface_left -= prefix;
        bytes = &bytes[prefix..];
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 9 {
            let len = (usize::from(self.pending[0]) << 16)
                | (usize::from(self.pending[1]) << 8)
                | usize::from(self.pending[2]);
            if self.pending.len() < 9 + len {
                break;
            }
            let stream =
                u32::from_be_bytes(self.pending[5..9].try_into().expect("frame stream id"))
                    & 0x7fff_ffff;
            if stream == 1 && self.pending[3] == 3 {
                if let Some(reset) = self.reset.take() {
                    let _sent = reset.send(());
                }
            }
            drop(self.pending.drain(..9 + len));
            if self.reset.is_none() {
                self.pending.clear();
                break;
            }
        }
        assert!(
            self.pending.len() < 64 * 1024,
            "unexpected client frame size"
        );
    }
}

impl AsyncRead for ResetWitness {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for ResetWitness {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.io).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self.record(&buf[..n]);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Native,
    #[cfg(feature = "grpc-web")]
    WebBinary,
    #[cfg(feature = "grpc-web")]
    WebText,
}

#[derive(Clone, Copy, Debug)]
enum Ending {
    Empty,
    Metadata,
    ProducerError,
}

async fn terminal_case(shape: Shape, ending: Ending, reset: bool) {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (reset_tx, reset_rx) = oneshot::channel();
    let (respond_tx, respond_rx) = oneshot::channel();
    let (entered_tx, entered_rx) = oneshot::channel();
    let gate = Arc::new(TrailerGate::new(entered_tx));
    let _release = ReleaseOnDrop(Arc::clone(&gate));
    let logger = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), gate);
    let tap = logger
        .start_call("/test.Service/Stream", Logger::Server)
        .expect("logged call");

    // Keep the backend connection driving on a separate task while the
    // writer is stopped at its terminal log event.
    let _server = AbortOnDrop(tokio::spawn(async move {
        let mut conn = backend::ServerBuilder::new()
            .handshake(server_io)
            .await
            .expect("server handshake");
        let (_request, respond) = conn
            .accept()
            .await
            .expect("request")
            .expect("request headers");
        let _sent = respond_tx.send(respond);
        while let Some(next) = conn.accept().await {
            drop(next);
        }
    }));
    let witness = ResetWitness {
        io: client_io,
        preface_left: 24,
        pending: Vec::new(),
        reset: Some(reset_tx),
    };
    let (client, mut connection) = h2::client::handshake(witness)
        .await
        .expect("client handshake");
    let mut ping = connection.ping_pong().expect("ping handle");
    let _client = AbortOnDrop(tokio::spawn(async move {
        let _result = connection.await;
    }));
    let mut client = client.ready().await.expect("client ready");
    let request = http::Request::builder()
        .uri("http://localhost/test.Service/Stream")
        .body(())
        .expect("request");
    let (response, mut send) = client.send_request(request, false).expect("open stream");
    send.send_data(Bytes::new(), true).expect("request end");
    let respond = respond_rx.await.expect("response handle");

    let (tx, stream) = Streaming::<crate::HelloReply>::channel(2);
    match ending {
        Ending::ProducerError => tx.fail(Status::resource_exhausted("producer")).await,
        Ending::Empty | Ending::Metadata => drop(tx),
    }
    let mut reply = Response::new(stream);
    if matches!(ending, Ending::Metadata) {
        reply
            .trailers_mut()
            .insert("x-end", "kept")
            .expect("trailing metadata");
    }
    let wire = Wire {
        limits: MessageLimits::default(),
        send_buffer: 1024,
        accept_gzip: true,
        send_codec: CompressionAlgorithm::Gzip,
        gzip_level: 1,
    };
    let budget = ByteBudgetTracker::unlimited();
    let observer = EndObserver::default();
    let captured = observer.clone();
    let flag = CancelFlag::new();
    let registry = Registry::global_shared();
    let server = registry.register_server("terminal-test");
    let server_id = server.id();
    let entry = registry.server(server_id).expect("server entry");
    start_call(&flag, Some(&observer), None, Some(server_id), None);
    let mut writer = AbortOnDrop(tokio::spawn(async move {
        let labels = CallLabels::UNLABELED_SERVER;
        let status = match shape {
            Shape::Native => {
                send_stream_response(
                    reply,
                    respond,
                    wire,
                    None,
                    false,
                    true,
                    false,
                    #[cfg(feature = "zstd")]
                    false,
                    &budget,
                    Some(&observer),
                    &labels,
                    Some(&tap),
                    None,
                    &flag,
                )
                .await
            }
            #[cfg(feature = "grpc-web")]
            Shape::WebBinary | Shape::WebText => {
                let mode = if matches!(shape, Shape::WebBinary) {
                    crate::web::Mode::Binary
                } else {
                    crate::web::Mode::Text
                };
                let web = crate::web::WebContext::new(
                    mode,
                    &http::HeaderMap::new(),
                    crate::web::CorsPolicy::DenyAll,
                    false,
                );
                send_web_stream_response(
                    reply,
                    respond,
                    web,
                    wire,
                    None,
                    false,
                    true,
                    false,
                    #[cfg(feature = "zstd")]
                    false,
                    &budget,
                    Some(&observer),
                    &labels,
                    Some(&tap),
                    None,
                    &flag,
                )
                .await
            }
        };
        end_call(
            &flag,
            Some(&observer),
            Some(&labels),
            Some(server_id),
            None,
            &status,
            || Duration::ZERO,
        );
        status
    }));

    tokio::time::timeout(WAIT, entered_rx)
        .await
        .expect("terminal gate timeout")
        .expect("terminal writer disappeared");
    let response = tokio::time::timeout(WAIT, response)
        .await
        .expect("response headers timeout")
        .expect("response headers");

    if reset {
        send.send_reset(h2::Reason::CANCEL);
        tokio::time::timeout(WAIT, reset_rx)
            .await
            .expect("reset write timeout")
            .expect("reset witness");
        // The client wrote RST_STREAM before PING. An ACK proves the server
        // connection processed the reset before the terminal writer resumes.
        tokio::time::timeout(WAIT, ping.ping(h2::Ping::opaque()))
            .await
            .expect("ping timeout")
            .expect("ping ack");
    }
    _release.0.release();
    let status = tokio::time::timeout(WAIT, &mut writer.0)
        .await
        .expect("writer timeout")
        .expect("writer task");
    let expected = if reset {
        Code::Cancelled
    } else if matches!(ending, Ending::ProducerError) {
        Code::ResourceExhausted
    } else {
        Code::Ok
    };
    assert_eq!(
        status.code(),
        expected,
        "shape={shape:?} ending={ending:?} reset={reset}"
    );
    assert_eq!(*captured.0.lock().expect("observer"), [expected]);
    assert_eq!(entry.calls_started(), 1);
    assert_eq!(
        entry.calls_succeeded(),
        if expected == Code::Ok { 1 } else { 0 }
    );
    assert_eq!(
        entry.calls_failed(),
        if expected == Code::Ok { 0 } else { 1 }
    );
    drop(response);
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_failure_updates_observer_and_channelz() {
    tokio::time::timeout(Duration::from_secs(90), async {
        for ending in [Ending::Empty, Ending::Metadata, Ending::ProducerError] {
            terminal_case(Shape::Native, ending, true).await;
            terminal_case(Shape::Native, ending, false).await;
        }
        #[cfg(feature = "grpc-web")]
        for shape in [Shape::WebBinary, Shape::WebText] {
            terminal_case(shape, Ending::Empty, true).await;
            terminal_case(shape, Ending::Empty, false).await;
            terminal_case(shape, Ending::ProducerError, true).await;
            terminal_case(shape, Ending::ProducerError, false).await;
        }
    })
    .await
    .expect("terminal response scenarios timed out");
}
