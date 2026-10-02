//! Observable cancellation lifetime and drop order of the response writer.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "explicit completion, abandonment, and panic assertions"
)]

use super::{CancelOnDrop, hold_cancel};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::watch;

type Events = Arc<Mutex<Vec<(&'static str, bool)>>>;

#[derive(Clone, Copy)]
enum Mode {
    Pending,
    Ready,
    Panic,
}

struct Writer {
    mode: Mode,
    cancelled: watch::Receiver<bool>,
    events: Events,
}

impl Future for Writer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        let name = match self.mode {
            Mode::Pending => "pending",
            Mode::Ready => "ready",
            Mode::Panic => "panic",
        };
        self.events
            .lock()
            .expect("events")
            .push((name, *self.cancelled.borrow()));
        match self.mode {
            Mode::Pending => Poll::Pending,
            Mode::Ready => Poll::Ready(()),
            Mode::Panic => panic!("writer poll panic"),
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.events
            .lock()
            .expect("events")
            .push(("drop", *self.cancelled.borrow()));
    }
}

fn prepare(mode: Mode) -> (CancelOnDrop, Writer, watch::Receiver<bool>, Events) {
    let (tx, cancelled) = watch::channel(false);
    let events = Arc::new(Mutex::new(Vec::new()));
    (
        CancelOnDrop(tx),
        Writer {
            mode,
            cancelled: cancelled.clone(),
            events: events.clone(),
        },
        cancelled,
        events,
    )
}

#[test]
fn unpolled_drop_cancels_before_dropping_the_writer() {
    let (cancel, write, cancelled, events) = prepare(Mode::Pending);
    let held = hold_cancel(cancel, write);
    assert!(!*cancelled.borrow());
    drop(held);
    assert!(*cancelled.borrow());
    assert_eq!(*events.lock().expect("events"), [("drop", true)]);
}

#[tokio::test]
async fn pending_drop_keeps_the_guard_until_abandonment() {
    let (cancel, write, cancelled, events) = prepare(Mode::Pending);
    let mut held = Box::pin(hold_cancel(cancel, write));
    poll_fn(|cx| {
        assert!(held.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(!*cancelled.borrow());
    drop(held);
    assert!(*cancelled.borrow());
    assert_eq!(
        *events.lock().expect("events"),
        [("pending", false), ("drop", true)]
    );
}

#[tokio::test]
async fn ready_await_cancels_before_dropping_the_completed_writer() {
    let (cancel, write, cancelled, events) = prepare(Mode::Ready);
    hold_cancel(cancel, write).await;
    assert!(*cancelled.borrow());
    assert_eq!(
        *events.lock().expect("events"),
        [("ready", false), ("drop", true)]
    );
}

#[tokio::test]
async fn panicking_poll_cancels_before_dropping_the_writer() {
    let (cancel, write, cancelled, events) = prepare(Mode::Panic);
    let task = tokio::spawn(async move { hold_cancel(cancel, write).await });
    assert!(task.await.expect_err("writer panicked").is_panic());
    assert!(*cancelled.borrow());
    assert_eq!(
        *events.lock().expect("events"),
        [("panic", false), ("drop", true)]
    );
}
