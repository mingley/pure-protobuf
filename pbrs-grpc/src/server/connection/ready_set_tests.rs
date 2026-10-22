//! Scheduling work scales with ready RPCs, including after connection I/O wakes.
#![allow(clippy::expect_used, reason = "fail closed on incorrect scheduling")]

use super::poll_admitted;
use atomic_waker::AtomicWaker;
use futures_util::stream::FuturesUnordered;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

struct Gate {
    ready: AtomicBool,
    polls: AtomicUsize,
    drops: AtomicUsize,
    waker: AtomicWaker,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ready: AtomicBool::new(false),
            polls: AtomicUsize::new(0),
            drops: AtomicUsize::new(0),
            waker: AtomicWaker::new(),
        })
    }
    fn release(&self) {
        self.ready.store(true, Ordering::Release);
        self.waker.wake();
    }
}

struct Handler(Arc<Gate>);
impl Future for Handler {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let gate = &self.0;
        gate.polls.fetch_add(1, Ordering::Relaxed);
        gate.waker.register(cx.waker());
        if gate.ready.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
impl Drop for Handler {
    fn drop(&mut self) {
        self.0.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn unrelated_connection_activity_does_not_scan_idle_handlers() {
    const COUNT: usize = 4096;
    let gates: Vec<_> = (0..COUNT).map(|_| Gate::new()).collect();
    let mut handlers = FuturesUnordered::new();
    for gate in &gates {
        handlers.push(Handler(Arc::clone(gate)));
    }
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..COUNT {
        assert!(poll_admitted(&mut handlers, &mut cx).is_pending());
        if gates.iter().all(|g| g.polls.load(Ordering::Relaxed) == 1) {
            break;
        }
    }
    assert!(gates.iter().all(|g| g.polls.load(Ordering::Relaxed) == 1));

    // Connection I/O can wake the owning task many times. It supplies no
    // reason to poll a parked handler whose own waker has not fired.
    for _ in 0..128 {
        assert!(poll_admitted(&mut handlers, &mut cx).is_pending());
    }
    assert!(gates.iter().all(|g| g.polls.load(Ordering::Relaxed) == 1));

    let active = gates.get(COUNT / 2).expect("selected handler");
    active.release();
    assert!(poll_admitted(&mut handlers, &mut cx).is_ready());
    assert_eq!(handlers.len(), COUNT - 1);
    assert_eq!(active.polls.load(Ordering::Relaxed), 2);
    assert_eq!(active.drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        gates
            .iter()
            .map(|g| g.polls.load(Ordering::Relaxed))
            .sum::<usize>(),
        COUNT + 1
    );

    drop(handlers);
    assert!(gates.iter().all(|g| g.drops.load(Ordering::Relaxed) == 1));
}

#[test]
fn queued_wake_survives_other_completion_and_registration() {
    let a = Gate::new();
    let b = Gate::new();
    let c = Gate::new();
    let mut handlers = FuturesUnordered::new();
    handlers.push(Handler(Arc::clone(&a)));
    handlers.push(Handler(Arc::clone(&b)));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(poll_admitted(&mut handlers, &mut cx).is_pending());
    a.release();
    b.release();
    assert!(poll_admitted(&mut handlers, &mut cx).is_ready());
    handlers.push(Handler(Arc::clone(&c)));
    assert!(poll_admitted(&mut handlers, &mut cx).is_ready());
    assert!(poll_admitted(&mut handlers, &mut cx).is_pending());
    assert_eq!(handlers.len(), 1);
    c.release();
    assert!(poll_admitted(&mut handlers, &mut cx).is_ready());
    assert!(handlers.is_empty());
}
