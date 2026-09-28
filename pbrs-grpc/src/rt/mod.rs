//! The async-runtime seam: task spawn, timer creation, and socket IO bounds.
//!
//! Protocol code in `client/call`, `server/connection`, `keepalive`, and
//! `timeout` goes through [`Runtime`] instead of calling Tokio directly, so
//! an alternative runtime can be slotted in behind this one trait. Timers
//! stay on [`tokio::time::Instant`] and spawned tasks still return
//! [`tokio::task::JoinHandle`]; the seam names the operations, not new clock
//! or task types.
//!
//! [`TokioRuntime`] is the production implementation. Every method compiles
//! to the direct Tokio call, so routing through the seam costs nothing: the
//! future types are identical, not wrappers.
//!
//! The call shapes stay concrete on purpose. Functions keep their existing
//! names and signatures and delegate to a generic `*_in` core instantiated
//! with [`TokioRuntime`]; the generic cores take the runtime as a type
//! parameter `R` first. Callers that never name `R` keep compiling
//! unchanged, and a test names [`manual::ManualRuntime`] explicitly to get
//! deterministic time.
//!
//! What is deliberately NOT behind the seam: `tokio::sync` channels
//! (`watch`, `Notify`, semaphores), `tokio::select!`, and wall-clock
//! `std::time::Instant` latency reads are runtime-agnostic vocabulary and
//! stay direct. The dial pools, accept loops, routers, and drain paths keep
//! their own Tokio timers; they migrate when their cards come up.

#[cfg(test)]
#[forbid(unsafe_code)]
pub(crate) mod manual;
pub(crate) mod per_core;

use std::future::Future;
use std::time::Duration;

/// Spawn, timers, and the clock, as static operations on a runtime.
///
/// Everything is static so call sites name the runtime as a type parameter
/// (`R::sleep_until(at)`) without threading a handle through signatures
/// that predate the seam. No instance is ever needed.
pub(crate) trait Runtime {
    /// The current time on this runtime's clock.
    fn now() -> tokio::time::Instant;

    /// A future that completes at `at`.
    fn sleep_until(at: tokio::time::Instant) -> impl Future<Output = ()> + Send;

    /// Poll `fut` to completion, or [`TimedOut`] if `duration` elapses first.
    ///
    /// A future that is already ready wins over an already-expired
    /// deadline: completion is checked first.
    fn timeout<F>(
        duration: Duration,
        fut: F,
    ) -> impl Future<Output = Result<F::Output, TimedOut>> + Send
    where
        F: Future + Send;

    /// A ticker that yields every `period`, with missed ticks delayed rather
    /// than bursty (Tokio's `MissedTickBehavior::Delay`).
    fn interval(period: Duration) -> impl Interval;

    /// Spawn a task; the handle type stays Tokio's.
    fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static;
}

/// A periodic ticker; [`Runtime::interval`] constructs these.
pub(crate) trait Interval: Send {
    /// Wait for the next tick, yielding the tick time. The first tick
    /// completes immediately.
    fn tick(&mut self) -> impl Future<Output = tokio::time::Instant> + Send;
}

impl Interval for tokio::time::Interval {
    fn tick(&mut self) -> impl Future<Output = tokio::time::Instant> + Send {
        tokio::time::Interval::tick(self)
    }
}

/// The socket IO every served connection runs on.
///
/// Today this is exactly the Tokio IO bound, named so the server seam has
/// one bound to target. An alternative runtime provides its own IO types
/// and implements this trait for them.
pub(crate) trait Io:
    tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static
{
}

impl<T> Io for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {}

/// [`Runtime::timeout`] expired before the future completed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimedOut;

impl std::fmt::Display for TimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("deadline elapsed")
    }
}

impl std::error::Error for TimedOut {}

/// [`Runtime`] for production: every method is the direct Tokio call.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TokioRuntime;

impl Runtime for TokioRuntime {
    #[inline]
    fn now() -> tokio::time::Instant {
        tokio::time::Instant::now()
    }

    #[inline]
    fn sleep_until(at: tokio::time::Instant) -> impl Future<Output = ()> + Send {
        tokio::time::sleep_until(at)
    }

    #[inline]
    async fn timeout<F>(duration: Duration, fut: F) -> Result<F::Output, TimedOut>
    where
        F: Future + Send,
    {
        tokio::time::timeout(duration, fut)
            .await
            .map_err(|_| TimedOut)
    }

    #[inline]
    fn interval(period: Duration) -> impl Interval {
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker
    }

    #[inline]
    fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(fut)
    }
}
