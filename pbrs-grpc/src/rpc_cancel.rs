#![allow(
    clippy::disallowed_types,
    reason = "short synchronous waker registry locks; no guard crosses an await"
)]

//! Per-RPC client cancel flag.
//!
//! One waiter uses the inline atomic waker. Concurrent upload, response and
//! hedge tasks register additional wakers on a cold path. Finished flags can
//! be reused after the previous call has dropped every handle.

use atomic_waker::AtomicWaker;
use std::cell::Cell;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

struct Inner {
    cancelled: AtomicBool,
    /// Every sender handle is gone. Waiters wake, but this is not a cancel.
    closed: AtomicBool,
    senders: AtomicUsize,
    waker: AtomicWaker,
    primary_claimed: AtomicBool,
    additional: Mutex<Vec<Option<std::task::Waker>>>,
}

impl Inner {
    fn wake_waiters(&self) {
        self.waker.wake();
        // Always synchronize with secondary registration. Skipping this lock
        // using a relaxed waiter count could race a waiter's terminal check.
        let wakers: Vec<_> = self
            .additional
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter_mut()
            .filter_map(Option::take)
            .collect();
        // Executor callbacks may reenter a future; never call them locked.
        for waker in wakers {
            waker.wake();
        }
    }
}

// One finished call's flag. A live call on this thread holds its own clone.
thread_local! {
    static SPARE: Cell<Option<Arc<Inner>>> = const { Cell::new(None) };
}

/// Cloneable cancel handle for one client RPC.
///
/// The flag is always present. `ManuallyDrop` lets `Drop` move the `Arc`
/// out without a discriminant check on every clone and drop.
pub(crate) struct RpcCancel {
    inner: ManuallyDrop<Arc<Inner>>,
}

impl std::fmt::Debug for RpcCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcCancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl RpcCancel {
    pub(crate) fn new() -> Self {
        if let Some(inner) = SPARE.try_with(Cell::take).ok().flatten() {
            inner.cancelled.store(false, Ordering::Release);
            inner.closed.store(false, Ordering::Release);
            inner.senders.store(1, Ordering::Release);
            return Self {
                inner: ManuallyDrop::new(inner),
            };
        }
        Self {
            inner: ManuallyDrop::new(Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                senders: AtomicUsize::new(1),
                waker: AtomicWaker::new(),
                primary_claimed: AtomicBool::new(false),
                additional: Mutex::new(Vec::new()),
            })),
        }
    }

    fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    /// Mark the call cancelled and wake whoever is waiting.
    pub(crate) fn signal(&self) {
        let inner = self.inner();
        inner.cancelled.store(true, Ordering::Release);
        inner.wake_waiters();
    }

    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner().cancelled.load(Ordering::Acquire)
    }

    /// Resolves when the call is cancelled or every sender handle is gone.
    pub(crate) fn wait(&self) -> Wait {
        Wait {
            inner: Arc::clone(self.inner()),
            registration: Registration::None,
        }
    }
}

impl Clone for RpcCancel {
    fn clone(&self) -> Self {
        let inner = self.inner();
        inner.senders.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: ManuallyDrop::new(Arc::clone(inner)),
        }
    }
}

impl Drop for RpcCancel {
    #[allow(unsafe_code, reason = "move the flag out of ManuallyDrop exactly once")]
    fn drop(&mut self) {
        // SAFETY: `inner` is initialized by every constructor and this is the
        // only place it is moved out. `ManuallyDrop` does not drop the `Arc`.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        if inner.senders.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        // Last sender. A waiter still holding the flag must observe close.
        // Nobody waiting: keep the allocation for the next call.
        if Arc::strong_count(&inner) == 1 {
            inner.cancelled.store(false, Ordering::Release);
            inner.closed.store(false, Ordering::Release);
            drop(inner.waker.take());
            SPARE
                .try_with(|cell| {
                    let existing = cell.take();
                    if existing.is_none() {
                        cell.set(Some(inner));
                    } else {
                        cell.set(existing);
                    }
                })
                .unwrap_or_default();
            return;
        }
        inner.closed.store(true, Ordering::Release);
        inner.wake_waiters();
    }
}

/// [`RpcCancel::wait`] future.
pub(crate) struct Wait {
    inner: Arc<Inner>,
    registration: Registration,
}

enum Registration {
    None,
    Primary,
    Additional(usize),
}

impl Wait {
    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }
}

impl Future for Wait {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.inner.cancelled.load(Ordering::Acquire) || this.inner.closed.load(Ordering::Acquire)
        {
            return Poll::Ready(());
        }
        if matches!(this.registration, Registration::None) {
            if this
                .inner
                .primary_claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                this.registration = Registration::Primary;
            } else {
                // Cloning or dropping a waker can run executor code. Keep
                // both outside the registry lock so callbacks can cancel.
                let waker = cx.waker().clone();
                let mut waiters = this
                    .inner
                    .additional
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let index = if let Some((index, slot)) = waiters
                    .iter_mut()
                    .enumerate()
                    .find(|(_, slot)| slot.is_none())
                {
                    *slot = Some(waker);
                    index
                } else {
                    waiters.push(Some(waker));
                    waiters.len() - 1
                };
                this.registration = Registration::Additional(index);
            }
        }
        match this.registration {
            Registration::Primary => this.inner.waker.register(cx.waker()),
            Registration::Additional(index) => {
                let needs_update = {
                    let waiters = this
                        .inner
                        .additional
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    waiters
                        .get(index)
                        .and_then(Option::as_ref)
                        .is_some_and(|waker| !waker.will_wake(cx.waker()))
                };
                if needs_update {
                    let mut replacement = Some(cx.waker().clone());
                    let previous = {
                        let mut waiters = this
                            .inner
                            .additional
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        match waiters.get_mut(index) {
                            Some(slot) if slot.is_some() => {
                                std::mem::replace(slot, replacement.take())
                            }
                            _ => None,
                        }
                    };
                    drop(previous);
                    // Cancellation may have removed the registration while
                    // the replacement was cloned. Drop an unused clone here.
                    drop(replacement);
                }
            }
            Registration::None => {}
        }
        if this.inner.cancelled.load(Ordering::Acquire) || this.inner.closed.load(Ordering::Acquire)
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        match self.registration {
            Registration::None => {}
            Registration::Primary => {
                drop(self.inner.waker.take());
                self.inner.primary_claimed.store(false, Ordering::Release);
            }
            Registration::Additional(index) => {
                let waker = self
                    .inner
                    .additional
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_mut(index)
                    .and_then(Option::take);
                drop(waker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "cancellation regressions"
    )]
    use super::RpcCancel;
    use std::future::Future;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Wake, Waker};

    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn waker() -> (Arc<Count>, Waker) {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        (count.clone(), Waker::from(count))
    }

    #[test]
    fn replacing_secondary_waker_allows_its_drop_to_cancel() {
        struct CancelOnDrop(RpcCancel);
        #[allow(
            clippy::manual_noop_waker,
            reason = "the waker's final owner must run a reentrant cancellation destructor"
        )]
        impl Wake for CancelOnDrop {
            fn wake(self: Arc<Self>) {}
        }
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.signal();
            }
        }

        let cancel = RpcCancel::new();
        let mut primary = Box::pin(cancel.wait());
        assert!(
            primary
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let mut secondary = Box::pin(cancel.wait());
        let waker = Waker::from(Arc::new(CancelOnDrop(cancel.clone())));
        assert!(
            secondary
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);

        let (sent, received) = std::sync::mpsc::channel();
        let task = std::thread::spawn(move || {
            assert!(
                secondary
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_ready()
            );
            sent.send(()).expect("test receiver exists");
        });
        received
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("waker drop must run outside the registry lock");
        task.join().expect("poll completes");
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn cancellation_wakes_every_registered_task() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        cancel.signal();
        assert_eq!(a.0.load(Ordering::SeqCst), 1);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_ready()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_ready()
        );
    }

    #[test]
    fn dropping_primary_and_replacing_it_preserves_other_waiters() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let (c, wc) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        drop(first);
        let mut third = Box::pin(cancel.wait());
        assert!(
            third
                .as_mut()
                .poll(&mut Context::from_waker(&wc))
                .is_pending()
        );
        cancel.signal();
        assert_eq!(a.0.load(Ordering::SeqCst), 0);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert_eq!(c.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn final_sender_closure_wakes_waiters_without_cancelling() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        drop(cancel);
        assert_eq!(a.0.load(Ordering::SeqCst), 1);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_ready()
        );
        assert!(!first.is_cancelled());
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_ready()
        );
        assert!(!second.is_cancelled());
    }

    #[test]
    fn racing_registration_with_signal_never_loses_a_pending_waiter() {
        for additional in [false, true] {
            for _ in 0..32 {
                let cancel = RpcCancel::new();
                let mut primary = additional.then(|| Box::pin(cancel.wait()));
                if let Some(wait) = &mut primary {
                    assert!(
                        wait.as_mut()
                            .poll(&mut Context::from_waker(Waker::noop()))
                            .is_pending()
                    );
                }
                let peer = cancel.clone();
                let start = Arc::new(std::sync::Barrier::new(2));
                let ready = start.clone();
                let (count, wake) = waker();
                let task = std::thread::spawn(move || {
                    let mut wait = Box::pin(peer.wait());
                    ready.wait();
                    let pending = wait
                        .as_mut()
                        .poll(&mut Context::from_waker(&wake))
                        .is_pending();
                    // Keep the pending future alive until signal has finished.
                    ready.wait();
                    pending
                });
                start.wait();
                cancel.signal();
                start.wait();
                if task.join().expect("waiter") {
                    assert!(count.0.load(Ordering::SeqCst) > 0);
                }
            }
        }
    }
}
