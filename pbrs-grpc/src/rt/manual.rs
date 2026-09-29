//! Test-only [`Runtime`](super::Runtime) with a manual clock.
//!
//! [`ManualRuntime`] implements every timer from a thread-local clock that
//! only moves when a test calls [`ManualRuntime::advance`], so timeout tests
//! never wait on wall time. Only time is faked: [`Runtime::spawn`](super::Runtime::spawn)
//! delegates to Tokio, so tests still run under `#[tokio::test]`.
//!
//! The clock is thread-local, and a task that migrates threads would see a
//! different clock, so every test here runs on the current-thread runtime
//! and holds a [`ManualGuard`] for the whole test. The guard is `!Send`,
//! installs exactly one clock per thread, and clears it on drop. Using the
//! clock without a guard panics rather than silently reading a wrong time.

use super::{Interval, Runtime, TimedOut, TokioRuntime};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::time::Instant;

struct Clock {
    now: Instant,
    wakers: Vec<Waker>,
}

thread_local! {
    static CLOCK: RefCell<Option<Clock>> = const { RefCell::new(None) };
}

fn with_clock<R>(f: impl FnOnce(&mut Clock) -> R) -> R {
    CLOCK.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let clock = borrowed.as_mut().expect(
            "ManualRuntime used without an installed clock; hold a ManualGuard for the test",
        );
        f(clock)
    })
}

/// [`Runtime`](super::Runtime) driven by [`ManualRuntime::advance`].
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ManualRuntime;

impl ManualRuntime {
    /// Move the manual clock forward, waking every parked sleeper.
    ///
    /// Wakers fire after the clock cell is released, so a woken task that
    /// polls its sleeper does not re-enter the borrow.
    pub(crate) fn advance(elapsed: Duration) {
        let wakers = with_clock(|clock| {
            clock.now += elapsed;
            std::mem::take(&mut clock.wakers)
        });
        for waker in wakers {
            waker.wake();
        }
    }
}

impl Runtime for ManualRuntime {
    fn now() -> Instant {
        with_clock(|clock| clock.now)
    }

    fn sleep_until(at: Instant) -> impl Future<Output = ()> + Send {
        SleepUntil(at)
    }

    async fn timeout<F>(duration: Duration, fut: F) -> Result<F::Output, TimedOut>
    where
        F: Future + Send,
    {
        let deadline = Self::now() + duration;
        tokio::select! {
            biased;
            out = fut => Ok(out),
            () = SleepUntil(deadline) => Err(TimedOut),
        }
    }

    fn interval(period: Duration) -> impl Interval {
        ManualInterval {
            period,
            next: Self::now(),
        }
    }

    fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        TokioRuntime::spawn(fut)
    }
}

/// Installs the manual clock for one thread; none of this works without it.
pub(crate) struct ManualGuard {
    _not_send: Rc<()>,
}

impl ManualGuard {
    /// Install `now` as the thread's manual clock. Panics if one is already
    /// installed: nested clocks would make "now" ambiguous.
    pub(crate) fn install(now: Instant) -> Self {
        CLOCK.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            assert!(
                borrowed.is_none(),
                "ManualRuntime clock already installed on this thread"
            );
            *borrowed = Some(Clock {
                now,
                wakers: Vec::new(),
            });
        });
        Self {
            _not_send: Rc::new(()),
        }
    }
}

impl Drop for ManualGuard {
    fn drop(&mut self) {
        CLOCK.with(|cell| *cell.borrow_mut() = None);
    }
}

struct SleepUntil(Instant);

impl Future for SleepUntil {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if with_clock(|clock| clock.now) >= self.0 {
            return Poll::Ready(());
        }
        with_clock(|clock| clock.wakers.push(cx.waker().clone()));
        // Re-check after registering so an advance between the first check
        // and the registration cannot strand this sleeper.
        if with_clock(|clock| clock.now) >= self.0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

struct ManualInterval {
    period: Duration,
    next: Instant,
}

impl Interval for ManualInterval {
    async fn tick(&mut self) -> Instant {
        SleepUntil(self.next).await;
        // Delay semantics: the next tick is a full period after this
        // one completes, never a catch-up burst.
        self.next = ManualRuntime::now() + self.period;
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::{ManualGuard, ManualRuntime};
    use crate::rt::{Runtime, TokioRuntime};
    use std::time::Duration;

    fn install() -> ManualGuard {
        ManualGuard::install(TokioRuntime::now())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sleep_parks_until_the_manual_clock_advances() {
        let now = TokioRuntime::now();
        let _guard = ManualGuard::install(now);
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        ManualRuntime::spawn(async move {
            ManualRuntime::sleep_until(now + Duration::from_secs(10)).await;
            tx.send(()).ok();
        });
        // Let the sleeper park, then prove it is parked: no wall wait, no
        // completion before the clock moves.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        rx.try_recv()
            .expect_err("sleeper must not complete before advance");
        ManualRuntime::advance(Duration::from_secs(10));
        rx.await
            .expect("sleeper must complete once the clock advances");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_expires_without_any_wall_wait() {
        let _guard = install();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        ManualRuntime::spawn(async move {
            let outcome =
                ManualRuntime::timeout(Duration::from_millis(50), std::future::pending::<()>())
                    .await;
            tx.send(outcome).ok();
        });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        rx.try_recv()
            .expect_err("timeout must not fire before advance");
        ManualRuntime::advance(Duration::from_millis(50));
        rx.await
            .expect("timeout task must finish")
            .expect_err("advancing past the deadline must expire the timeout");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_completion_wins_over_an_expired_deadline() {
        let _guard = install();
        // Ready future, zero timeout: biased toward completion, like Tokio.
        let won = ManualRuntime::timeout(Duration::ZERO, async { 7 }).await;
        assert_eq!(won.ok(), Some(7));
        // Unfinished future, already-expired deadline: expires on first poll.
        ManualRuntime::advance(Duration::from_secs(1));
        let lost = ManualRuntime::timeout(Duration::ZERO, std::future::pending::<()>()).await;
        lost.expect_err("expired deadline with a pending future must fail");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interval_first_tick_is_immediate_then_parks() {
        use crate::rt::Interval as _;
        let _guard = install();
        let mut ticker = ManualRuntime::interval(Duration::from_millis(10));
        // First tick is immediate, matching Tokio.
        ticker.tick().await;
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        ManualRuntime::spawn(async move {
            ticker.tick().await;
            tx.send(()).ok();
        });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        rx.try_recv()
            .expect_err("second tick must wait for the clock");
        ManualRuntime::advance(Duration::from_millis(10));
        rx.await
            .expect("second tick must fire once the clock advances");
    }
}
