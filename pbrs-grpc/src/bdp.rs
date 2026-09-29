//! Adaptive HTTP/2 receive-window estimation.
//!
//! The estimator follows hyper's BDP-ping shape: sample bytes received during
//! a ping RTT, grow stream and connection receive windows when the sample is at
//! least two thirds of the current target, and back off probe frequency once
//! bandwidth stabilizes. The driver owns the single h2 user [`PingPong`] for a
//! connection whenever adaptive windows are enabled, so optional keepalive
//! pings share that same outstanding PING instead of racing it.
#![allow(
    clippy::disallowed_types,
    reason = "BDP state uses short non-async critical sections from poll_data/poll; no lock is held across await"
)]

use crate::transport::{Error, h2 as backend};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::{Instant, Sleep};

const MIN_WINDOW_SIZE: u32 = 1;

/// h2 enforces the RFC 9113 maximum flow-control window.
pub(crate) const MAX_WINDOW_SIZE: u32 = (1 << 31) - 1;

/// Adaptive-window settings for one HTTP/2 connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Config {
    initial_window: u32,
    max_window: u32,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
}

impl Config {
    /// Build a bounded adaptive-window configuration.
    pub(crate) fn new(
        initial_window: u32,
        max_window: u32,
        keep_alive_interval: Option<Duration>,
        keep_alive_timeout: Duration,
    ) -> Self {
        let max_window = max_window.clamp(MIN_WINDOW_SIZE, MAX_WINDOW_SIZE);
        let initial_window = initial_window.clamp(MIN_WINDOW_SIZE, max_window);
        Self {
            initial_window,
            max_window,
            keep_alive_interval,
            keep_alive_timeout,
        }
    }

    /// Initial receive window advertised while the estimator is learning.
    pub(crate) fn initial_window(self) -> u32 {
        self.initial_window
    }

    fn keep_alive_interval(self) -> Option<Duration> {
        self.keep_alive_interval
    }
}

/// Cheap DATA-byte recorder cloned into every receive stream on a connection.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorder {
    shared: Option<Arc<Mutex<Shared>>>,
}

impl Recorder {
    /// Disabled recorder for fixed-window connections.
    pub(crate) fn disabled() -> Self {
        Self { shared: None }
    }

    /// Record an inbound DATA chunk.
    pub(crate) fn record_data(&self, len: usize) {
        let Some(shared) = &self.shared else {
            return;
        };
        let now = Instant::now();
        let mut shared = lock(shared);
        if let Some(next_bdp_at) = shared.next_bdp_at {
            if now < next_bdp_at {
                return;
            }
            shared.next_bdp_at = None;
        }
        shared.bytes = shared.bytes.saturating_add(len);
        shared.bdp_ping_requested = true;
    }
}

/// Result of polling the shared ping driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// Adaptive estimator chose a larger receive window.
    SizeUpdate(u32),
    /// A keepalive probe timed out.
    KeepAliveTimedOut,
}

/// Shared h2 PING driver for BDP probes and optional keepalive probes.
pub(crate) struct Driver {
    shared: Arc<Mutex<Shared>>,
    bdp: Bdp,
    keep_alive: Option<KeepAlive>,
    wake: Option<PinSleep>,
}

type PinSleep = std::pin::Pin<Box<Sleep>>;

impl Driver {
    /// Build the recorder and driver for a connection.
    pub(crate) fn new(ping_pong: backend::PingPong, config: Config) -> (Recorder, Self) {
        let now = Instant::now();
        let shared = Arc::new(Mutex::new(Shared {
            ping_pong,
            ping_sent_at: None,
            bytes: 0,
            bdp_ping_requested: false,
            next_bdp_at: Some(now),
        }));
        let recorder = Recorder {
            shared: Some(Arc::clone(&shared)),
        };
        let driver = Self {
            shared,
            bdp: Bdp::new(config.initial_window, config.max_window),
            keep_alive: config.keep_alive_interval().map(|interval| KeepAlive {
                interval,
                timeout: config.keep_alive_timeout,
                next_ping_at: now + interval,
                timeout_at: None,
            }),
            wake: None,
        };
        (recorder, driver)
    }

    /// Poll for a BDP size update or keepalive timeout.
    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<Event, Error>> {
        let now = Instant::now();
        if let Some(event) = self.poll_pong(cx, now)? {
            return Poll::Ready(Ok(event));
        }
        if let Some(event) = self.poll_keep_alive(now)? {
            return Poll::Ready(Ok(event));
        }
        if self.send_requested_bdp_ping(now)? {
            if let Some(event) = self.poll_pong(cx, now)? {
                return Poll::Ready(Ok(event));
            }
        }
        self.poll_timer(cx);
        Poll::Pending
    }

    fn poll_pong(&mut self, cx: &mut Context<'_>, now: Instant) -> Result<Option<Event>, Error> {
        let (sent_at, bytes) = {
            let mut shared = lock(&self.shared);
            if shared.ping_sent_at.is_none() {
                return Ok(None);
            }
            match shared.ping_pong.poll_pong(cx) {
                Poll::Pending => return Ok(None),
                Poll::Ready(Err(error)) => return Err(error),
                Poll::Ready(Ok(_pong)) => {
                    let sent_at = shared.ping_sent_at.take();
                    shared.bdp_ping_requested = false;
                    let bytes = shared.bytes;
                    shared.bytes = 0;
                    (sent_at, bytes)
                }
            }
        };
        if let Some(keep_alive) = &mut self.keep_alive {
            keep_alive.on_pong(now);
        }
        let Some(sent_at) = sent_at else {
            return Ok(None);
        };
        let rtt = now.saturating_duration_since(sent_at);
        let update = self.bdp.calculate(bytes, rtt);
        let mut shared = lock(&self.shared);
        shared.next_bdp_at = Some(now + self.bdp.ping_delay);
        Ok(update.map(Event::SizeUpdate))
    }

    fn poll_keep_alive(&mut self, now: Instant) -> Result<Option<Event>, Error> {
        let Some(keep_alive) = &mut self.keep_alive else {
            return Ok(None);
        };
        if keep_alive
            .timeout_at
            .is_some_and(|deadline| now >= deadline)
        {
            return Ok(Some(Event::KeepAliveTimedOut));
        }
        if now < keep_alive.next_ping_at {
            return Ok(None);
        }
        keep_alive.next_ping_at = now + keep_alive.interval;
        keep_alive.timeout_at = Some(now + keep_alive.timeout);
        let mut shared = lock(&self.shared);
        if !shared.is_ping_sent() {
            shared.send_ping(now)?;
        }
        Ok(None)
    }

    fn send_requested_bdp_ping(&mut self, now: Instant) -> Result<bool, Error> {
        let mut shared = lock(&self.shared);
        if !shared.bdp_ping_requested || shared.is_ping_sent() {
            return Ok(false);
        }
        shared.bdp_ping_requested = false;
        shared.send_ping(now)?;
        Ok(true)
    }

    fn poll_timer(&mut self, cx: &mut Context<'_>) {
        let Some(deadline) = self.next_timer_deadline() else {
            self.wake = None;
            return;
        };
        let replace = self
            .wake
            .as_ref()
            .is_none_or(|sleep| sleep.deadline() != deadline);
        if replace {
            self.wake = Some(Box::pin(tokio::time::sleep_until(deadline)));
        }
        if let Some(sleep) = &mut self.wake {
            if sleep.as_mut().poll(cx).is_ready() {
                self.wake = None;
                cx.waker().wake_by_ref();
            }
        }
    }

    fn next_timer_deadline(&self) -> Option<Instant> {
        let keep_alive = self.keep_alive.as_ref()?;
        match keep_alive.timeout_at {
            Some(timeout_at) => Some(timeout_at.min(keep_alive.next_ping_at)),
            None => Some(keep_alive.next_ping_at),
        }
    }
}

#[derive(Debug)]
struct Shared {
    ping_pong: backend::PingPong,
    ping_sent_at: Option<Instant>,
    bytes: usize,
    bdp_ping_requested: bool,
    next_bdp_at: Option<Instant>,
}

impl Shared {
    fn is_ping_sent(&self) -> bool {
        self.ping_sent_at.is_some()
    }

    fn send_ping(&mut self, now: Instant) -> Result<(), Error> {
        self.ping_pong.send_ping(backend::Ping::opaque())?;
        self.ping_sent_at = Some(now);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct Bdp {
    bdp: u32,
    cap: u32,
    max_bandwidth: f64,
    rtt: f64,
    ping_delay: Duration,
    stable_count: u32,
}

impl Bdp {
    fn new(initial_window: u32, cap: u32) -> Self {
        Self {
            bdp: initial_window,
            cap,
            max_bandwidth: 0.0,
            rtt: 0.0,
            ping_delay: Duration::from_millis(100),
            stable_count: 0,
        }
    }

    fn calculate(&mut self, bytes: usize, rtt: Duration) -> Option<u32> {
        if bytes == 0 || self.bdp >= self.cap {
            self.stabilize_delay();
            return None;
        }

        let rtt = seconds(rtt).max(f64::EPSILON);
        if self.rtt == 0.0 {
            self.rtt = rtt;
        } else {
            self.rtt += (rtt - self.rtt) * 0.125;
        }

        let bandwidth = (bytes as f64) / (self.rtt * 1.5);
        if bandwidth < self.max_bandwidth {
            self.stabilize_delay();
            return None;
        }
        self.max_bandwidth = bandwidth;

        if bytes < (self.bdp as usize).saturating_mul(2) / 3 {
            self.stabilize_delay();
            return None;
        }

        let next = bytes.saturating_mul(2).min(self.cap as usize);
        if next <= self.bdp as usize {
            self.stabilize_delay();
            return None;
        }
        self.bdp = match u32::try_from(next) {
            Ok(next) => next,
            Err(_) => self.cap,
        };
        self.stable_count = 0;
        self.ping_delay /= 2;
        Some(self.bdp)
    }

    fn stabilize_delay(&mut self) {
        if self.ping_delay < Duration::from_secs(10) {
            self.stable_count += 1;
            if self.stable_count >= 2 {
                self.ping_delay *= 4;
                self.stable_count = 0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct KeepAlive {
    interval: Duration,
    timeout: Duration,
    next_ping_at: Instant,
    timeout_at: Option<Instant>,
}

impl KeepAlive {
    fn on_pong(&mut self, now: Instant) {
        self.next_ping_at = now + self.interval;
        self.timeout_at = None;
    }
}

fn seconds(duration: Duration) -> f64 {
    const NANOS_PER_SEC: f64 = 1_000_000_000.0;
    duration.as_secs() as f64 + f64::from(duration.subsec_nanos()) / NANOS_PER_SEC
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::Bdp;
    use std::time::Duration;

    #[test]
    fn bdp_grows_within_cap() {
        let mut bdp = Bdp::new(64 * 1024, 1024 * 1024);
        assert_eq!(
            bdp.calculate(64 * 1024, Duration::from_millis(20)),
            Some(128 * 1024)
        );
        assert_eq!(
            bdp.calculate(1024 * 1024, Duration::from_millis(20)),
            Some(1024 * 1024)
        );
        assert_eq!(bdp.calculate(1024 * 1024, Duration::from_millis(20)), None);
    }
}
