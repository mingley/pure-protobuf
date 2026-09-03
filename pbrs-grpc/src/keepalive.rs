//! HTTP/2 PING so a dead peer is noticed before the next RPC, and the
//! in-flight counter idle close uses to ignore PINGs. Age is wall-clock
//! from handshake; PINGs do not postpone it.
//!
//! TCP `SO_KEEPALIVE` is [`crate::ServerConfig::tcp_keepalive`] /
//! [`crate::ChannelConfig::tcp_keepalive`], applied in `tcp`. Probe interval
//! is [`crate::ServerConfig::tcp_keepalive_interval`] /
//! [`crate::ChannelConfig::tcp_keepalive_interval`] (`TCP_KEEPINTVL`); it does
//! not turn `SO_KEEPALIVE` on by itself. Probe retry count is
//! [`crate::ServerConfig::tcp_keepalive_retries`] /
//! [`crate::ChannelConfig::tcp_keepalive_retries`] (`TCP_KEEPCNT`); it does
//! not turn `SO_KEEPALIVE` on by itself either.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Notify};

/// Drive PINGs until one fails or times out. `None` means keepalive is off.
pub(crate) fn spawn(
    ping_pong: Option<h2::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
) -> Option<watch::Receiver<bool>> {
    let interval = interval?;
    let mut ping_pong = ping_pong?;
    let (tx, rx) = watch::channel(false);
    drop(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is immediate; skip it so we do not PING on connect.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match tokio::time::timeout(timeout, ping_pong.ping(h2::Ping::opaque())).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => {
                    tx.send(true).ok();
                    break;
                }
            }
        }
    }));
    Some(rx)
}

/// Wait until a keepalive task declares the connection dead.
pub(crate) async fn wait(mut dead: watch::Receiver<bool>) {
    dead.wait_for(|v| *v).await.ok();
}

/// [`wait`] when keepalive is optional; pending forever when it is off.
pub(crate) async fn wait_opt(dead: Option<watch::Receiver<bool>>) {
    match dead {
        Some(dead) => wait(dead).await,
        None => std::future::pending().await,
    }
}

/// Outstanding RPCs on one HTTP/2 connection.
///
/// Idle close (server GOAWAY, client driver stop) and client max-connection-age
/// wait for [`Self::count`] to reach zero (age also has a grace bound).
/// Keepalive PINGs do not touch this. Age itself is wall-clock from
/// handshake; PINGs do not postpone it.
pub(crate) struct Busy {
    n: AtomicUsize,
    notify: Notify,
}

/// Decrements [`Busy`] when dropped. Hold it for the whole RPC, including a
/// [`crate::Streaming`] that outlives the [`crate::Call`].
#[must_use]
pub(crate) struct Lease {
    busy: Arc<Busy>,
}

impl Busy {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            n: AtomicUsize::new(0),
            notify: Notify::new(),
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }

    pub(crate) fn start(self: &Arc<Self>) -> Lease {
        let prev = self.n.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            self.notify.notify_waiters();
        }
        Lease {
            busy: Arc::clone(self),
        }
    }

    pub(crate) async fn notified(&self) {
        self.notify.notified().await;
    }

    /// Subscribe first so a 1→0 transition between the check and the wait is
    /// not lost.
    pub(crate) async fn wait_idle(&self) {
        loop {
            let notified = self.notify.notified();
            if self.count() == 0 {
                return;
            }
            notified.await;
        }
    }

    pub(crate) async fn wait_busy(&self) {
        loop {
            let notified = self.notify.notified();
            if self.count() > 0 {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        loop {
            let n = self.busy.n.load(Ordering::SeqCst);
            if n == 0 {
                return;
            }
            if self
                .busy
                .n
                .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if n == 1 {
                    self.busy.notify.notify_waiters();
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Busy;

    #[tokio::test]
    async fn lease_tracks_outstanding_rpcs() {
        let busy = Busy::new();
        assert_eq!(busy.count(), 0);
        let a = busy.start();
        assert_eq!(busy.count(), 1);
        let b = busy.start();
        assert_eq!(busy.count(), 2);
        drop(a);
        assert_eq!(busy.count(), 1);
        drop(b);
        assert_eq!(busy.count(), 0);
    }
}
