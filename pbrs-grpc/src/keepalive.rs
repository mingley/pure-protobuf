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

use crate::rt::{Interval as _, Runtime, TokioRuntime};
use crate::transport::{PingPong, h2 as backend};
use atomic_waker::AtomicWaker;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Waker;
use std::time::Duration;
use tokio::sync::{Notify, watch};

/// Arm user PINGs only when an interval is configured.
///
/// Taking the ping handle registers a waker on every connection poll. Outlined
/// so the connection task's poll loop does not absorb that branch.
#[inline(never)]
pub(crate) fn spawn_if(
    ping_pong: impl FnOnce() -> Option<backend::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
    activity: Option<Arc<Busy>>,
) -> Option<watch::Receiver<bool>> {
    interval?;
    spawn_with_activity_in::<TokioRuntime>(ping_pong(), interval, timeout, activity)
}

/// [`spawn_if`] on runtime `R`.
#[inline(never)]
pub(crate) fn spawn_if_in<R: Runtime>(
    ping_pong: impl FnOnce() -> Option<backend::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
) -> Option<watch::Receiver<bool>> {
    interval?;
    spawn_in::<R>(ping_pong(), interval, timeout)
}

/// Drive PINGs on runtime `R` until a probe fails or the connection drops.
pub(crate) fn spawn_in<R: Runtime>(
    ping_pong: Option<backend::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
) -> Option<watch::Receiver<bool>> {
    spawn_with_activity_in::<R>(ping_pong, interval, timeout, None)
}

fn spawn_with_activity_in<R: Runtime>(
    ping_pong: Option<backend::PingPong>,
    interval: Option<Duration>,
    timeout: Duration,
    activity: Option<Arc<Busy>>,
) -> Option<watch::Receiver<bool>> {
    let interval = interval?;
    let mut ping_pong = ping_pong?;
    let (tx, rx) = watch::channel(false);
    drop(R::spawn(async move {
        let mut ticker = R::interval(interval.max(Duration::from_millis(1)));
        // The first tick is immediate; skip it so we do not PING on connect.
        ticker.tick().await;
        loop {
            if let Some(busy) = &activity {
                tokio::select! {
                    _ = tx.closed() => return,
                    _ = busy.wait_busy() => {}
                }
                tokio::select! {
                    _ = tx.closed() => return,
                    _ = busy.wait_idle() => continue,
                    _ = ticker.tick() => {}
                }
                if busy.count() == 0 {
                    continue;
                }
            } else {
                tokio::select! {
                    _ = tx.closed() => return,
                    _ = ticker.tick() => {}
                }
            }
            let result = tokio::select! {
                _ = tx.closed() => return,
                result = R::timeout(timeout, ping_pong.ping(backend::Ping::opaque())) => result,
            };
            match result {
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
    keepalive_waker: Option<Box<AtomicWaker>>,
}

/// Decrements [`Busy`] when dropped. Hold it for the whole RPC, including a
/// [`crate::Streaming`] that outlives the [`crate::Call`].
#[must_use]
pub(crate) struct Lease {
    busy: Arc<Busy>,
}

impl Busy {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_keepalive_waker(None)
    }

    pub(crate) fn new_for_adaptive_keepalive() -> Arc<Self> {
        Self::with_keepalive_waker(Some(Box::new(AtomicWaker::new())))
    }

    fn with_keepalive_waker(keepalive_waker: Option<Box<AtomicWaker>>) -> Arc<Self> {
        Arc::new(Self {
            n: AtomicUsize::new(0),
            notify: Notify::new(),
            keepalive_waker,
        })
    }

    pub(crate) fn count(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }

    pub(crate) fn start(self: &Arc<Self>) -> Lease {
        let prev = self.n.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            self.notify.notify_waiters();
            if let Some(waker) = &self.keepalive_waker {
                waker.wake();
            }
        }
        Lease {
            busy: Arc::clone(self),
        }
    }

    /// Subscribe the integrated adaptive-window driver before reading count.
    /// This driver is the sole registered poll consumer for one connection.
    pub(crate) fn register_keepalive(&self, waker: &Waker) {
        if let Some(activity) = &self.keepalive_waker {
            activity.register(waker);
        }
    }

    pub(crate) fn clear_keepalive(&self) {
        if let Some(activity) = &self.keepalive_waker {
            drop(activity.take());
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
                    if let Some(waker) = &self.busy.keepalive_waker {
                        waker.wake();
                    }
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "raw HTTP/2 protocol assertions"
    )]
    use super::Busy;

    use crate::hello::{HelloReply, HelloRequest};
    use crate::{Channel, ChannelConfig, Request};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Wake, Waker};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    struct Frame {
        kind: u8,
        flags: u8,
        stream: u32,
        payload: Vec<u8>,
    }

    async fn read_frame(io: &mut DuplexStream) -> Frame {
        let mut header = [0; 9];
        io.read_exact(&mut header).await.expect("frame header");
        let len =
            usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
        let mut payload = vec![0; len];
        io.read_exact(&mut payload).await.expect("frame payload");
        Frame {
            kind: header[3],
            flags: header[4],
            stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff,
            payload,
        }
    }

    async fn write_frame(io: &mut DuplexStream, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
        let len = u32::try_from(payload.len())
            .expect("frame length")
            .to_be_bytes();
        io.write_all(&[len[1], len[2], len[3], kind, flags])
            .await
            .expect("frame prefix");
        io.write_all(&stream.to_be_bytes())
            .await
            .expect("frame stream");
        io.write_all(payload).await.expect("frame body");
    }

    async fn accept_preface(mut peer: DuplexStream) -> DuplexStream {
        let mut preface = [0; 24];
        peer.read_exact(&mut preface).await.expect("client preface");
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        write_frame(&mut peer, 4, 0, 0, &[]).await;
        peer
    }

    async fn channel(config: ChannelConfig) -> (Channel, DuplexStream) {
        let (client, peer) = tokio::io::duplex(16 * 1024);
        let (channel, peer) = tokio::join!(
            Channel::from_io_with(client, "test.invalid:443", config),
            accept_preface(peer),
        );
        (channel.expect("client handshake"), peer)
    }

    async fn next_ping(peer: &mut DuplexStream) -> Vec<u8> {
        tokio::time::timeout(Duration::from_millis(250), async {
            loop {
                let frame = read_frame(peer).await;
                if frame.kind == 6 && frame.flags == 0 {
                    return frame.payload;
                }
            }
        })
        .await
        .expect("keepalive PING")
    }

    async fn no_ping(peer: &mut DuplexStream, duration: Duration) {
        assert!(
            tokio::time::timeout(duration, async {
                loop {
                    let frame = read_frame(peer).await;
                    if frame.kind == 6 && frame.flags == 0 {
                        return;
                    }
                }
            })
            .await
            .is_err(),
            "idle keepalive PING was emitted"
        );
    }

    async fn streaming_response(
        channel: &Channel,
        peer: &mut DuplexStream,
    ) -> crate::Streaming<HelloReply> {
        let call = channel.server_streaming::<HelloRequest, HelloReply>(
            "/test.Service/Stream",
            Request::new(HelloRequest::default()),
        );
        let result = tokio::spawn(async move { call.await.expect("response headers") });
        loop {
            let frame = read_frame(peer).await;
            if frame.kind == 6 && frame.flags == 0 {
                write_frame(peer, 6, 1, 0, &frame.payload).await;
            }
            if frame.kind == 1 {
                // HPACK: indexed :status 200, literal content-type application/grpc.
                let mut headers = vec![0x88, 0x0f, 0x10, 16];
                headers.extend_from_slice(b"application/grpc");
                write_frame(peer, 1, 4, frame.stream, &headers).await;
                break;
            }
        }
        result.await.expect("response task").into_inner()
    }

    #[tokio::test(start_paused = true)]
    async fn idle_policy_defaults_to_pinging_and_false_does_not_enable_an_interval() {
        let (_channel, mut peer) = channel(ChannelConfig::new().keep_alive_while_idle(false)).await;
        no_ping(&mut peer, Duration::from_millis(100)).await;
        let (_channel, mut peer) =
            channel(ChannelConfig::new().keep_alive_interval(Duration::from_millis(20))).await;
        let payload = next_ping(&mut peer).await;
        write_frame(&mut peer, 6, 1, 0, &payload).await;
    }

    #[tokio::test(start_paused = true)]
    async fn response_stream_lifetime_controls_fixed_and_adaptive_idle_pings() {
        for adaptive in [false, true] {
            let config = ChannelConfig::new()
                .keep_alive_interval(Duration::from_millis(20))
                .keep_alive_timeout(Duration::from_millis(80))
                .keep_alive_while_idle(false)
                .adaptive_window(adaptive);
            let (channel, mut peer) = channel(config).await;
            no_ping(&mut peer, Duration::from_millis(100)).await;
            // A completed Call has handed its activity lease to Streaming.
            for _ in 0..2 {
                let stream = streaming_response(&channel, &mut peer).await;
                let payload = next_ping(&mut peer).await;
                write_frame(&mut peer, 6, 1, 0, &payload).await;
                tokio::task::yield_now().await;
                drop(stream);
                no_ping(&mut peer, Duration::from_millis(100)).await;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn adaptive_ping_timeout_is_not_rearmed_by_shorter_intervals() {
        let (channel, mut peer) = channel(
            ChannelConfig::new()
                .adaptive_window(true)
                .keep_alive_interval(Duration::from_millis(20))
                .keep_alive_timeout(Duration::from_millis(80)),
        )
        .await;
        let _unacknowledged = next_ping(&mut peer).await;
        // Poll at each shorter interval, so a buggy driver cannot keep
        // moving the outstanding probe's acknowledgement deadline forward.
        for _ in 0..5 {
            tokio::time::advance(Duration::from_millis(20)).await;
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
        }
        let mut byte = [0; 1];
        // The driver drops the connection on deadline even while Channel lives.
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(5), peer.read(&mut byte))
                .await
                .expect("closed connection")
                .expect("peer read"),
            0
        );
        drop(channel);
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_an_idle_connection_releases_the_keepalive_task() {
        for adaptive in [false, true] {
            let (client, peer) = tokio::io::duplex(16 * 1024);
            let config = ChannelConfig::new()
                .keep_alive_interval(Duration::from_secs(3600))
                .keep_alive_while_idle(false)
                .adaptive_window(adaptive);
            let (dialed, _peer) = tokio::join!(
                crate::client::pool::finish_h2_with_executor(
                    config,
                    client,
                    &crate::executor::TOKIO_EXECUTOR
                ),
                accept_preface(peer)
            );
            let dialed = dialed.expect("handshake");
            let activity = Arc::downgrade(dialed.busy.as_ref().expect("active-only tracking"));
            drop(dialed);
            for _ in 0..12 {
                tokio::task::yield_now().await;
            }
            assert!(
                activity.upgrade().is_none(),
                "idle keepalive task retained the connection activity"
            );
        }
    }

    #[test]
    fn activity_wakes_the_adaptive_driver_and_clear_releases_its_parent() {
        #[derive(Default)]
        struct Counter(AtomicUsize);
        impl Wake for Counter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let busy = Busy::new_for_adaptive_keepalive();
        let parent = Arc::new(Counter::default());
        let waker = Waker::from(Arc::clone(&parent));
        busy.register_keepalive(&waker);
        let lease = busy.start();
        assert_eq!(parent.0.load(Ordering::Relaxed), 1);
        busy.register_keepalive(&waker);
        drop(lease);
        assert_eq!(parent.0.load(Ordering::Relaxed), 2);
        busy.register_keepalive(&waker);
        assert_eq!(Arc::strong_count(&parent), 3);
        busy.clear_keepalive();
        assert_eq!(Arc::strong_count(&parent), 2);
    }

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
