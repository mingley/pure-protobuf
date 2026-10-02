//! Request-phase timeout ownership and handler/reset priority.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "explicit request-phase lifetime, timeout and panic assertions"
)]

use super::{notify_deadline, run_handler};
use crate::status::{Code, Status};
use crate::transport::h2 as backend;
use crate::transport::{
    ClientBuilder, Reason, SendRequest, SendResponse, SendStream, ServerBuilder, ServerConnection,
};
use crate::wire::wrap_timeout;
use std::future::{Future, poll_fn};
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::watch;

#[expect(
    clippy::disallowed_types,
    reason = "synchronous poll/drop event recording; each lock guard ends within its statement before any await"
)]
type Events = Arc<std::sync::Mutex<Vec<(&'static str, bool)>>>;

#[derive(Clone)]
struct State {
    cancelled: watch::Receiver<bool>,
    events: Events,
    drops: Arc<AtomicUsize>,
}

impl State {
    fn record(&self, name: &'static str) {
        self.events
            .lock()
            .expect("events")
            .push((name, *self.cancelled.borrow()));
    }

    fn snapshot(&self) -> Vec<(&'static str, bool)> {
        self.events.lock().expect("events").clone()
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Pending,
    Ready,
    ReadyOnCancel,
    Panic,
}

struct Phase {
    state: State,
    mode: Mode,
    _pinned: PhantomPinned,
}

impl Future for Phase {
    type Output = Result<(), Status>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_ref().get_ref();
        match this.mode {
            Mode::Ready => {
                this.state.record("ready");
                Poll::Ready(Ok(()))
            }
            Mode::ReadyOnCancel if *this.state.cancelled.borrow() => {
                this.state.record("ready");
                Poll::Ready(Ok(()))
            }
            Mode::Pending | Mode::ReadyOnCancel => {
                this.state.record("pending");
                Poll::Pending
            }
            Mode::Panic => {
                this.state.record("panic");
                panic!("request phase panic");
            }
        }
    }
}

impl Drop for Phase {
    fn drop(&mut self) {
        self.state.record("drop");
        assert_eq!(self.state.drops.fetch_add(1, Ordering::SeqCst), 0);
    }
}

fn prepare(mode: Mode) -> (Phase, watch::Sender<bool>, State) {
    let (cancel, cancelled) = watch::channel(false);
    let state = State {
        cancelled,
        events: Events::default(),
        drops: Arc::new(AtomicUsize::new(0)),
    };
    (
        Phase {
            state: state.clone(),
            mode,
            _pinned: PhantomPinned,
        },
        cancel,
        state,
    )
}

#[derive(Clone, Copy)]
enum Shape {
    Owned,
    Borrowed,
}

async fn through_timeout<T>(
    shape: Shape,
    timeout: Option<Duration>,
    phase: impl Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    match shape {
        Shape::Owned => wrap_timeout(timeout, phase).await,
        Shape::Borrowed => {
            tokio::pin!(phase);
            wrap_timeout(timeout, phase.as_mut()).await
        }
    }
}

async fn finish_phase(
    shape: Shape,
    timeout: Option<Duration>,
    phase: Phase,
    cancel: watch::Sender<bool>,
    state: State,
) -> Result<(), Status> {
    let outcome = through_timeout(shape, timeout, phase).await;
    state.record("before_notify");
    notify_deadline(&outcome, &cancel);
    state.record("after_notify");
    outcome
}

#[tokio::test]
async fn unpolled_timeout_phase_drops_once_without_running_notifications() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        for timeout in [None, Some(Duration::from_secs(1))] {
            let (phase, cancel, state) = prepare(Mode::Pending);
            let future = finish_phase(shape, timeout, phase, cancel, state.clone());
            assert!(state.snapshot().is_empty());
            drop(future);
            assert_eq!(state.snapshot(), [("drop", false)]);
        }
    }
}

#[tokio::test]
async fn pending_timeout_phase_drops_once_before_any_notification() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        for timeout in [None, Some(Duration::from_secs(1))] {
            let (phase, cancel, state) = prepare(Mode::Pending);
            let mut future = Box::pin(finish_phase(shape, timeout, phase, cancel, state.clone()));
            poll_fn(|cx| {
                assert!(future.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            assert_eq!(state.snapshot(), [("pending", false)]);
            drop(future);
            assert_eq!(state.snapshot(), [("pending", false), ("drop", false)]);
        }
    }
}

#[tokio::test]
async fn completed_timeout_phase_drops_before_notify_and_prepared_state() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        for timeout in [None, Some(Duration::from_secs(1)), Some(Duration::ZERO)] {
            let (phase, cancel, state) = prepare(Mode::Ready);
            finish_phase(shape, timeout, phase, cancel, state.clone())
                .await
                .expect("ready phase wins even against elapsed timeout");
            assert_eq!(
                state.snapshot(),
                [
                    ("ready", false),
                    ("drop", false),
                    ("before_notify", false),
                    ("after_notify", false),
                ]
            );
        }
    }
}

#[tokio::test]
async fn panicking_timeout_phase_drops_once_without_notification() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        for timeout in [None, Some(Duration::from_secs(1))] {
            let (phase, cancel, state) = prepare(Mode::Panic);
            let task = tokio::spawn(finish_phase(shape, timeout, phase, cancel, state.clone()));
            assert!(task.await.expect_err("phase panicked").is_panic());
            assert_eq!(state.snapshot(), [("panic", false), ("drop", false)]);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn elapsed_timeout_drops_the_phase_before_notifying_deadline() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        let (phase, cancel, state) = prepare(Mode::Pending);
        let status = finish_phase(
            shape,
            Some(Duration::from_millis(5)),
            phase,
            cancel,
            state.clone(),
        )
        .await
        .expect_err("deadline elapsed");
        assert_eq!(status.code(), Code::DeadlineExceeded);
        let events = state.snapshot();
        assert!(events.starts_with(&[("pending", false)]));
        assert!(events.ends_with(&[
            ("drop", false),
            ("before_notify", false),
            ("after_notify", true),
        ]));
        assert_eq!(state.drops.load(Ordering::SeqCst), 1);
    }
}

struct ResetPeer {
    respond: backend::SendResponse,
    _send: backend::SendStream,
    _request: http::Request<backend::RecvStream>,
    _response: backend::ResponseFuture,
    _sender: backend::SendRequest,
    client: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for ResetPeer {
    fn drop(&mut self) {
        self.client.abort();
        self.server.abort();
    }
}

async fn reset_peer() -> ResetPeer {
    let (client_io, server_io) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move {
        backend::ServerBuilder::new()
            .handshake(server_io)
            .await
            .expect("server handshake")
    });
    let (sender, connection) = backend::ClientBuilder::new()
        .handshake(client_io)
        .await
        .expect("client handshake");
    let client = tokio::spawn(async move { drop(connection.await) });
    let mut connection = server.await.expect("server task");
    let mut sender = sender.ready().await.expect("ready");
    let (response, mut send) = sender
        .send_request(
            http::Request::builder()
                .uri("http://localhost/sv09/Read")
                .body(())
                .expect("headers"),
            false,
        )
        .expect("send request");
    let (request, mut respond) = connection
        .accept()
        .await
        .expect("request")
        .expect("accepted headers");
    let server = tokio::spawn(async move {
        while let Some(result) = connection.accept().await {
            result.expect("drive connection");
        }
    });
    send.send_reset(Reason::CANCEL);
    assert_eq!(
        poll_fn(|cx| respond.poll_reset(cx))
            .await
            .expect("observe peer reset"),
        Reason::CANCEL
    );
    ResetPeer {
        respond,
        _send: send,
        _request: request,
        _response: response,
        _sender: sender,
        client,
        server,
    }
}

#[tokio::test]
async fn handler_ready_wins_when_peer_reset_is_already_ready() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut peer = reset_peer().await;
            let (phase, cancel, state) = prepare(Mode::Ready);
            through_timeout(
                shape,
                None,
                run_handler(&mut peer.respond, cancel, phase, None),
            )
            .await
            .expect("biased handler readiness wins");
            assert_eq!(state.snapshot(), [("ready", false), ("drop", false)]);
        })
        .await
        .expect("ready/reset fixture stalled");
    }
}

#[tokio::test]
async fn peer_reset_signals_then_polls_a_pending_handler_once() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut peer = reset_peer().await;
            let (phase, cancel, state) = prepare(Mode::Pending);
            let status = through_timeout(
                shape,
                None,
                run_handler(&mut peer.respond, cancel, phase, None),
            )
            .await
            .expect_err("pending handler cancelled");
            assert_eq!(status.code(), Code::Cancelled);
            assert_eq!(
                state.snapshot(),
                [("pending", false), ("pending", true), ("drop", true)]
            );
        })
        .await
        .expect("pending/reset fixture stalled");
    }
}

#[tokio::test]
async fn peer_reset_retains_the_final_cooperative_handler_result() {
    for shape in [Shape::Owned, Shape::Borrowed] {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut peer = reset_peer().await;
            let (phase, cancel, state) = prepare(Mode::ReadyOnCancel);
            through_timeout(
                shape,
                None,
                run_handler(&mut peer.respond, cancel, phase, None),
            )
            .await
            .expect("cooperative handler finishes after cancellation");
            assert_eq!(
                state.snapshot(),
                [("pending", false), ("ready", true), ("drop", true)]
            );
        })
        .await
        .expect("cooperative/reset fixture stalled");
    }
}
