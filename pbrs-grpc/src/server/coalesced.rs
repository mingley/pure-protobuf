//! Coalesce connection-internal notifications before rescheduling its owner.
//!
//! A ready-set or HTTP/2 stream can wake its connection while that same task
//! is already processing the event. Preserve the notification, then poll the
//! connection again locally. A bounded poll batch still yields to the runtime.

use atomic_waker::AtomicWaker;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll, Wake, Waker};

const POLLING: u8 = 1;
const NOTIFIED: u8 = 2;
const CLOSED: u8 = 4;
const MAX_POLLS: usize = 4;

struct Notifications {
    flags: AtomicU8,
    parent: AtomicWaker,
}

impl Notifications {
    fn close(&self) {
        self.flags.fetch_or(CLOSED, Ordering::AcqRel);
        drop(self.parent.take());
    }
}

impl Wake for Notifications {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let before = self.flags.fetch_or(NOTIFIED, Ordering::AcqRel);
        if before & (POLLING | CLOSED) == 0 {
            self.parent.wake();
        }
    }
}

pin_project_lite::pin_project! {
    /// Connection future with bounded local processing of internal wakes.
    pub(super) struct Coalesced<F> {
        #[pin]
        future: F,
        notifications: Arc<Notifications>,
        proxy: Waker,
    }

    impl<F> PinnedDrop for Coalesced<F> {
        fn drop(this: Pin<&mut Self>) {
            // Release the owner before dropping application/transport state.
            // Their retained proxy wakers must not keep or reschedule it.
            this.project().notifications.close();
        }
    }
}

impl<F> Coalesced<F> {
    pub(super) fn new(future: F) -> Self {
        let notifications = Arc::new(Notifications {
            flags: AtomicU8::new(0),
            parent: AtomicWaker::new(),
        });
        let proxy = Waker::from(Arc::clone(&notifications));
        Self {
            future,
            notifications,
            proxy,
        }
    }
}

impl<F: Future> Future for Coalesced<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        this.notifications.parent.register(cx.waker());
        this.notifications.flags.fetch_or(POLLING, Ordering::AcqRel);
        let mut inner_cx = Context::from_waker(this.proxy);
        for index in 0..MAX_POLLS {
            // A prior notification is satisfied by this poll. Wakes during
            // the poll set it again, including wakes from another thread.
            this.notifications
                .flags
                .fetch_and(!NOTIFIED, Ordering::AcqRel);
            if let Poll::Ready(output) = this.future.as_mut().poll(&mut inner_cx) {
                this.notifications.close();
                return Poll::Ready(output);
            }
            let before = this
                .notifications
                .flags
                .fetch_and(!POLLING, Ordering::AcqRel);
            // A wake before this RMW is observed here. A wake afterwards sees
            // POLLING clear and schedules the parent itself, so returning
            // Pending cannot lose an external notification.
            if before & NOTIFIED == 0 {
                return Poll::Pending;
            }
            if index + 1 == MAX_POLLS || !tokio::task::coop::has_budget_remaining() {
                this.notifications.parent.wake();
                return Poll::Pending;
            }
            this.notifications.flags.fetch_or(POLLING, Ordering::AcqRel);
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    #[derive(Default)]
    struct CountWake(AtomicUsize);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn consumed_self_notification_does_not_reschedule_the_parent() {
        let parent = Arc::new(CountWake::default());
        let waker = Waker::from(Arc::clone(&parent));
        let mut cx = Context::from_waker(&waker);
        let mut calls = 0;
        let future = poll_fn(|cx| {
            calls += 1;
            if calls == 1 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(19)
            }
        });
        let mut future = Box::pin(Coalesced::new(future));
        assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(19));
        assert_eq!(Arc::strong_count(&parent), 2);
        drop(future);
        assert_eq!(calls, 2);
        assert_eq!(parent.0.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn continuously_ready_work_has_a_finite_poll_budget() {
        let parent = Arc::new(CountWake::default());
        let waker = Waker::from(Arc::clone(&parent));
        let mut cx = Context::from_waker(&waker);
        let calls = AtomicUsize::new(0);
        let future = poll_fn(|cx| {
            calls.fetch_add(1, Ordering::Relaxed);
            cx.waker().wake_by_ref();
            Poll::<()>::Pending
        });
        let mut future = Box::pin(Coalesced::new(future));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(calls.load(Ordering::Relaxed), MAX_POLLS);
        assert_eq!(parent.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn external_wake_during_poll_is_not_lost() {
        let parent = Arc::new(CountWake::default());
        let waker = Waker::from(Arc::clone(&parent));
        let mut cx = Context::from_waker(&waker);
        let ready = Arc::new(AtomicBool::new(false));
        let mut first = true;
        let future = poll_fn(|cx| {
            if ready.load(Ordering::Acquire) {
                return Poll::Ready(());
            }
            assert!(first);
            first = false;
            let notify = cx.waker().clone();
            let ready = Arc::clone(&ready);
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    ready.store(true, Ordering::Release);
                    notify.wake();
                });
            });
            Poll::Pending
        });
        let mut future = Box::pin(Coalesced::new(future));
        assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(()));
        assert_eq!(parent.0.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn idle_wakes_follow_the_latest_parent_and_drop_releases_it() {
        let first = Arc::new(CountWake::default());
        let second = Arc::new(CountWake::default());
        let first_waker = Waker::from(Arc::clone(&first));
        let second_waker = Waker::from(Arc::clone(&second));
        let mut held = None;
        let future = poll_fn(|cx| {
            held = Some(cx.waker().clone());
            Poll::<()>::Pending
        });
        let mut future = Box::pin(Coalesced::new(future));
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(&first_waker))
                .is_pending()
        );
        // Retrieve the proxy independently of the closure's captured borrow.
        let proxy = future.proxy.clone();
        proxy.wake_by_ref();
        assert_eq!(first.0.load(Ordering::Relaxed), 1);
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending()
        );
        proxy.wake_by_ref();
        assert_eq!(first.0.load(Ordering::Relaxed), 1);
        assert_eq!(second.0.load(Ordering::Relaxed), 1);
        assert!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending()
        );
        assert_eq!(Arc::strong_count(&second), 3);
        drop(future);
        assert_eq!(Arc::strong_count(&second), 2);
        proxy.wake_by_ref();
        assert_eq!(second.0.load(Ordering::Relaxed), 1);
        drop(held);
    }

    #[test]
    #[allow(
        clippy::expect_used,
        reason = "the proxy is installed before the thread barrier"
    )]
    fn racing_the_pending_boundary_always_leaves_progress_or_a_parent_wake() {
        for _ in 0..256 {
            let parent = Arc::new(CountWake::default());
            let waker = Waker::from(Arc::clone(&parent));
            let mut cx = Context::from_waker(&waker);
            let ready = Arc::new(AtomicBool::new(false));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let proxy = Arc::new(std::sync::OnceLock::<Waker>::new());
            let mut first = true;
            let future = poll_fn(|cx| {
                if ready.load(Ordering::Acquire) {
                    return Poll::Ready(());
                }
                if first {
                    first = false;
                    assert!(proxy.set(cx.waker().clone()).is_ok());
                    barrier.wait();
                }
                Poll::Pending
            });
            let mut future = Box::pin(Coalesced::new(future));
            std::thread::scope(|scope| {
                let ready = Arc::clone(&ready);
                let barrier = Arc::clone(&barrier);
                let proxy = Arc::clone(&proxy);
                let thread = scope.spawn(move || {
                    barrier.wait();
                    ready.store(true, Ordering::Release);
                    proxy.get().expect("installed proxy").wake_by_ref();
                });
                let result = future.as_mut().poll(&mut cx);
                assert!(thread.join().is_ok());
                if result.is_pending() {
                    assert!(parent.0.load(Ordering::Relaxed) > 0);
                    assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(()));
                }
            });
        }
    }

    #[test]
    #[allow(
        clippy::panic,
        reason = "exercise registered parent cleanup after a poll panic"
    )]
    fn drop_after_a_poll_panic_releases_the_parent() {
        let parent = Arc::new(CountWake::default());
        let waker = Waker::from(Arc::clone(&parent));
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(Coalesced::new(poll_fn(|_| -> Poll<()> {
            panic!("injected poll failure");
        })));
        let proxy = future.proxy.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            future.as_mut().poll(&mut cx)
        }));
        assert!(result.is_err());
        drop(future);
        assert_eq!(Arc::strong_count(&parent), 2);
        proxy.wake_by_ref();
        assert_eq!(parent.0.load(Ordering::Relaxed), 0);
    }
}
