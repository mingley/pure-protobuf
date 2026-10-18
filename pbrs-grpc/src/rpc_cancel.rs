//! Per-RPC client cancel flag.
//!
//! A `watch` channel builds eight notify lists. This flag is one atomic and
//! one waker, reused by the next call on the thread after the previous call
//! has dropped every handle.

use atomic_waker::AtomicWaker;
use std::cell::Cell;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

struct Inner {
    cancelled: AtomicBool,
    /// Every sender handle is gone. Waiters wake, but this is not a cancel.
    closed: AtomicBool,
    senders: AtomicUsize,
    waker: AtomicWaker,
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
        inner.waker.wake();
    }

    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner().cancelled.load(Ordering::Acquire)
    }

    /// Resolves when the call is cancelled or every sender handle is gone.
    pub(crate) fn wait(&self) -> Wait {
        Wait {
            inner: Arc::clone(self.inner()),
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
            let _ = SPARE.try_with(|cell| {
                let existing = cell.take();
                if existing.is_none() {
                    cell.set(Some(inner));
                } else {
                    cell.set(existing);
                }
            });
            return;
        }
        inner.closed.store(true, Ordering::Release);
        inner.waker.wake();
    }
}

/// [`RpcCancel::wait`] future.
pub(crate) struct Wait {
    inner: Arc<Inner>,
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
        if self.inner.cancelled.load(Ordering::Acquire) || self.inner.closed.load(Ordering::Acquire)
        {
            return Poll::Ready(());
        }
        self.inner.waker.register(cx.waker());
        if self.inner.cancelled.load(Ordering::Acquire) || self.inner.closed.load(Ordering::Acquire)
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
