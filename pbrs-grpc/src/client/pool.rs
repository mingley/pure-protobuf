//! Connection pool: slots, dialing, handshake, and idle/age watches.

use super::{Channel, Target};
use crate::config::ChannelConfig;
use crate::limits::ByteBudgetTracker;
use crate::service_config::SharedServiceConfig;
use crate::status::Status;
use crate::stream::Streaming;
use crate::telemetry::{LifecycleObserver, ReconnectEvent};
use crate::tls::ClientTls;
use bytes::Bytes;
use http::uri::Authority;
use std::future::Future;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, watch};

/// One pooled HTTP/2 client. `gen` changes whenever the slot is redialed, so a
/// grabber that observed the previous generation die does not overwrite a
/// reconnect that already landed. `send` is `None` until the first successful
/// handshake on a lazy channel, and after a dead handle is discarded or the
/// slot idle-closes.
pub(crate) struct ConnSlot {
    pub(crate) r#gen: u64,
    pub(crate) send: Option<h2::client::SendRequest<Bytes>>,
    /// Stops the connection driver (idle close, age close, lost-race handshake, drop).
    pub(crate) stop: Option<watch::Sender<bool>>,
    /// Outstanding RPCs; `None` when neither idle-close nor age is configured.
    pub(crate) busy: Option<Arc<crate::keepalive::Busy>>,
}

/// A finished handshake: the sender plus the handles that stop its driver.
pub(crate) struct Dialed {
    pub(crate) send: h2::client::SendRequest<Bytes>,
    pub(crate) stop: watch::Sender<bool>,
    pub(crate) busy: Option<Arc<crate::keepalive::Busy>>,
}

/// A sender taken from a pool slot, plus the generation so a raced `GOAWAY`
/// can discard this slot instead of writing into a reconnect that already
/// landed.
pub(crate) struct LiveConn {
    pub(crate) send: h2::client::SendRequest<Bytes>,
    pub(crate) lease: Option<crate::keepalive::Lease>,
    /// Clone of the slot's driver-stop sender. Held on a received
    /// [`Streaming`] so dropping the last [`Channel`] does not stop the
    /// connection under an in-flight stream.
    pub(crate) driver: Option<watch::Sender<bool>>,
    pub(crate) slot: usize,
    pub(crate) r#gen: u64,
}

/// Backoff between wait-for-ready handshake attempts, in milliseconds.
/// Caps at the last entry; see [`ChannelInner::acquire`].
const WAIT_FOR_READY_BACKOFF_MS: &[u64] = &[20, 40, 80, 160, 320, 640, 1000];

pub(crate) struct ChannelInner {
    pub(crate) slots: Vec<Mutex<ConnSlot>>,
    pub(crate) next: AtomicUsize,
    pub(crate) endpoint: Endpoint,
    pub(crate) tls: Option<ClientTls>,
    /// Settings used to dial. Per-clone overlays on [`Channel`] (timeout,
    /// wait-for-ready, send_compressed, gzip_compression_level, message sizes,
    /// stream_buffer, max_send_buffer_size, https_scheme, origin) do not change
    /// how a dead slot is redialed.
    pub(crate) dial: ChannelConfig,
}

/// Where a handshake should connect. TCP is `host:port`; Unix is a filesystem
/// path. HTTP/2 `:authority` for a Unix socket is `localhost`. [`Self::Once`]
/// is an already-connected stream that cannot be redialed.
#[derive(Clone)]
pub(crate) enum Endpoint {
    Tcp(String),
    #[cfg(unix)]
    Unix(PathBuf),
    Once,
}

impl Endpoint {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Tcp(host) => host.clone(),
            #[cfg(unix)]
            Self::Unix(path) => path.display().to_string(),
            Self::Once => "once".to_owned(),
        }
    }

    pub(crate) fn can_redial(&self) -> bool {
        !matches!(self, Self::Once)
    }
}

pub(crate) async fn connect_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Tcp(target.authority().to_owned());
    let authority = target.parse()?;
    let n = config.connection_count();
    let mut sends = Vec::with_capacity(n);
    for _ in 0..n {
        sends.push(handshake(&endpoint, config, tls.as_ref()).await?);
    }
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        live_slots(sends),
    ))
}

pub(crate) fn connect_lazy_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Tcp(target.authority().to_owned());
    let authority = target.parse()?;
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        empty_slots(config.connection_count()),
    ))
}

#[cfg(unix)]
pub(crate) async fn connect_unix_inner(
    path: &Path,
    config: ChannelConfig,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Unix(path.to_owned());
    let n = config.connection_count();
    let mut sends = Vec::with_capacity(n);
    for _ in 0..n {
        sends.push(handshake(&endpoint, config, None).await?);
    }
    Ok(finish_channel(
        endpoint,
        unix_authority(),
        config,
        None,
        live_slots(sends),
    ))
}

pub(crate) fn finish_channel(
    endpoint: Endpoint,
    authority: Authority,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    slots: Vec<Mutex<ConnSlot>>,
) -> Channel {
    let https = tls.is_some();
    let inner = Arc::new(ChannelInner {
        slots,
        next: AtomicUsize::new(0),
        endpoint,
        tls,
        dial: config,
    });
    for i in 0..inner.slots.len() {
        spawn_idle_watch(Arc::clone(&inner), i);
        spawn_age_watch(Arc::clone(&inner), i);
    }
    let budget_limit = if config.send_buffer_size() != crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE {
        Some(config.send_buffer_size())
    } else {
        None
    };
    Channel {
        inner,
        config,
        interceptors: Arc::from([]),
        response_interceptors: Arc::from([]),
        rpc_slots: rpc_slots_from(config),
        byte_budget: ByteBudgetTracker::new(budget_limit),
        user_agent: crate::wire::PBRS_GRPC_UA,
        https,
        authority,
        observer: None,
        service_config: SharedServiceConfig::default(),
    }
}

pub(crate) fn rpc_slots_from(config: ChannelConfig) -> Option<Arc<Semaphore>> {
    config
        .concurrent_rpc_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

pub(crate) fn live_slots(dialed: Vec<Dialed>) -> Vec<Mutex<ConnSlot>> {
    dialed
        .into_iter()
        .map(|d| {
            Mutex::new(ConnSlot {
                r#gen: 0,
                send: Some(d.send),
                stop: Some(d.stop),
                busy: d.busy,
            })
        })
        .collect()
}

pub(crate) fn empty_slots(n: usize) -> Vec<Mutex<ConnSlot>> {
    (0..n)
        .map(|_| {
            Mutex::new(ConnSlot {
                r#gen: 0,
                send: None,
                stop: None,
                busy: None,
            })
        })
        .collect()
}

#[cfg(unix)]
pub(crate) fn unix_authority() -> Authority {
    Authority::from_static("localhost")
}

impl ChannelInner {
    pub(crate) fn pick(&self) -> Result<usize, Status> {
        let n = self.slots.len();
        if n == 0 {
            return Err(Status::unavailable("empty connection pool"));
        }
        if n == 1 {
            Ok(0)
        } else {
            Ok(self.next.fetch_add(1, Ordering::Relaxed) % n)
        }
    }

    pub(crate) fn slot(&self, i: usize) -> Result<&Mutex<ConnSlot>, Status> {
        self.slots
            .get(i)
            .ok_or_else(|| Status::unavailable("empty connection pool"))
    }

    /// Clone a live sender for this slot, redialing only when `ready` reports
    /// the connection is gone or the slot has never been dialed. `ready`
    /// waiting on stream capacity is not treated as death: that wait happens
    /// without holding the slot lock. Handshake and wait-for-ready backoff
    /// also run without the lock, so a down peer cannot stall other RPCs on
    /// the same slot. A `GOAWAY` that races after `ready` is handled by
    /// discarding that generation and retrying once on unary and
    /// server-streaming.
    pub(crate) async fn acquire(
        self: &Arc<Self>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
    ) -> Result<LiveConn, Status> {
        let i = self.pick()?;
        let mut attempt = 0usize;
        loop {
            let (handle, lease, r#gen, driver) = {
                let slot = self.slot(i)?.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (slot.send.clone(), lease, slot.r#gen, slot.stop.clone())
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    return Ok(LiveConn {
                        send: ready,
                        lease,
                        driver,
                        slot: i,
                        r#gen,
                    });
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            match handshake(&self.endpoint, self.dial, self.tls.as_ref()).await {
                Ok(dialed) => {
                    if let Some(obs) = observer {
                        if r#gen > 0 || attempt > 0 {
                            let target_desc = self.endpoint.describe();
                            let attempt_u32 =
                                u32::try_from(attempt).unwrap_or(u32::MAX).saturating_add(1);
                            obs.on_reconnect(&ReconnectEvent {
                                target: &target_desc,
                                attempt: attempt_u32,
                                duration: dial_start.elapsed(),
                                status: None,
                            });
                        }
                    }
                    let mut slot = self.slot(i)?.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        drop(slot);
                        spawn_idle_watch(Arc::clone(self), i);
                        spawn_age_watch(Arc::clone(self), i);
                        return Ok(LiveConn {
                            send,
                            lease,
                            driver,
                            slot: i,
                            r#gen,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    if let Some(obs) = observer {
                        if r#gen > 0 || attempt > 0 {
                            let target_desc = self.endpoint.describe();
                            let attempt_u32 =
                                u32::try_from(attempt).unwrap_or(u32::MAX).saturating_add(1);
                            obs.on_reconnect(&ReconnectEvent {
                                target: &target_desc,
                                attempt: attempt_u32,
                                duration: dial_start.elapsed(),
                                status: Some(status.code()),
                            });
                        }
                    }
                    if wait_for_ready && self.endpoint.can_redial() {
                        let delay_ms = WAIT_FOR_READY_BACKOFF_MS
                            .get(attempt)
                            .copied()
                            .unwrap_or(1000);
                        attempt = attempt.saturating_add(1);
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        return Err(status);
                    }
                }
            }
        }
    }

    /// Drop a dead generation so the next [`Self::acquire`] redials.
    ///
    /// A raced `GOAWAY` can land after `ready` succeeded. Without this, the
    /// same dying sender would be handed out again. A reconnect that already
    /// stored a newer `gen` is left alone.
    pub(crate) async fn discard(&self, i: usize, r#gen: u64) {
        let Ok(lock) = self.slot(i) else {
            return;
        };
        let mut slot = lock.lock().await;
        if slot.r#gen != r#gen {
            return;
        }
        slot.send = None;
        slot.busy = None;
        if let Some(stop) = slot.stop.take() {
            stop.send(true).ok();
        }
        slot.r#gen = slot.r#gen.wrapping_add(1);
    }
}

fn store_dialed(slot: &mut ConnSlot, dialed: Dialed) -> h2::client::SendRequest<Bytes> {
    if let Some(stop) = slot.stop.take() {
        stop.send(true).ok();
    }
    slot.r#gen = slot.r#gen.wrapping_add(1);
    slot.send = Some(dialed.send.clone());
    slot.stop = Some(dialed.stop);
    slot.busy = dialed.busy;
    dialed.send
}

fn spawn_idle_watch(inner: Arc<ChannelInner>, i: usize) {
    let Some(idle) = inner.dial.connection_idle() else {
        return;
    };
    drop(tokio::spawn(async move {
        let (r#gen, busy) = {
            let Ok(slot) = inner.slot(i) else {
                return;
            };
            let slot = slot.lock().await;
            match slot.busy.as_ref() {
                Some(busy) => (slot.r#gen, Arc::clone(busy)),
                None => return,
            }
        };
        idle_watch(inner, i, r#gen, busy, idle).await;
    }));
}

fn spawn_age_watch(inner: Arc<ChannelInner>, i: usize) {
    let Some(age) = inner.dial.connection_age() else {
        return;
    };
    let grace = inner.dial.age_grace();
    drop(tokio::spawn(async move {
        let r#gen = {
            let Ok(slot) = inner.slot(i) else {
                return;
            };
            let slot = slot.lock().await;
            // Lazy slots have no socket yet; age starts at handshake.
            if slot.send.is_none() {
                return;
            }
            slot.r#gen
        };
        let seed = (i as u64).wrapping_shl(32).wrapping_add(r#gen);
        tokio::time::sleep(crate::config::jitter_age(age, seed)).await;
        age_close(inner, i, r#gen, grace).await;
    }));
}

async fn age_close(inner: Arc<ChannelInner>, i: usize, r#gen: u64, grace: Duration) {
    let (old_stop, old_busy) = {
        let Ok(lock) = inner.slot(i) else {
            return;
        };
        let mut slot = lock.lock().await;
        if slot.r#gen != r#gen {
            return;
        }
        slot.send = None;
        let busy = slot.busy.take();
        let stop = slot.stop.take();
        slot.r#gen = slot.r#gen.wrapping_add(1);
        (stop, busy)
    };
    if let Some(busy) = old_busy {
        tokio::select! {
            () = busy.wait_idle() => {}
            () = tokio::time::sleep(grace) => {}
        }
    }
    if let Some(stop) = old_stop {
        stop.send(true).ok();
    }
}

async fn idle_watch(
    inner: Arc<ChannelInner>,
    i: usize,
    r#gen: u64,
    busy: Arc<crate::keepalive::Busy>,
    idle: Duration,
) {
    loop {
        busy.wait_idle().await;
        tokio::select! {
            () = tokio::time::sleep(idle) => {
                let Ok(slot) = inner.slot(i) else {
                    return;
                };
                let mut slot = slot.lock().await;
                if slot.r#gen != r#gen {
                    return;
                }
                if busy.count() != 0 {
                    continue;
                }
                slot.send = None;
                slot.busy = None;
                if let Some(stop) = slot.stop.take() {
                    stop.send(true).ok();
                }
                slot.r#gen = slot.r#gen.wrapping_add(1);
                return;
            }
            () = busy.wait_busy() => {}
        }
    }
}

async fn handshake(
    endpoint: &Endpoint,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
    let timeout = config.handshake_timeout();
    match tokio::time::timeout(timeout, handshake_io(endpoint, config, tls)).await {
        Ok(result) => result,
        Err(_) => Err(Status::unavailable(format!(
            "connect {}: timed out after {timeout:?}",
            endpoint.describe()
        ))),
    }
}

async fn handshake_io(
    endpoint: &Endpoint,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
    match endpoint {
        Endpoint::Tcp(host) => {
            let tcp = crate::tcp::connect(host, config.bound_local_address())
                .await
                .map_err(|e| Status::unavailable(format!("connect {host}: {e}")))?;
            crate::tcp::tune(
                &tcp,
                config.tcp_keepalive_period(),
                config.tcp_keepalive_probe_interval(),
                config.tcp_keepalive_probe_retries(),
            )
            .map_err(|e| Status::unavailable(e.to_string()))?;
            match tls {
                None => finish_h2(config, tcp).await,
                Some(tls) => {
                    let tls_stream = tls.connect(tcp).await?;
                    finish_h2(config, tls_stream).await.map_err(|e| {
                        if e.to_string().contains("connection closed") {
                            Status::unauthenticated("tls: peer closed after handshake")
                        } else {
                            e
                        }
                    })
                }
            }
        }
        #[cfg(unix)]
        Endpoint::Unix(path) => {
            if tls.is_some() {
                return Err(Status::invalid_argument(
                    "TLS over a Unix socket is not supported",
                ));
            }
            let io = UnixStream::connect(path).await.map_err(|e| {
                Status::unavailable(format!("connect {}: {e}", endpoint.describe()))
            })?;
            finish_h2(config, io).await
        }
        Endpoint::Once => Err(Status::unavailable("channel has no address to redial")),
    }
}

pub(crate) async fn finish_h2<IO>(config: ChannelConfig, io: IO) -> Result<Dialed, Status>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (send, mut conn) = config
        .h2_builder()
        .handshake(io)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let (interval, timeout) = config.keepalive();
    let dead = crate::keepalive::spawn(conn.ping_pong(), interval, timeout);
    // `SendRequest::ready` does not wait for SETTINGS. Drive the connection
    // until send capacity leaves 0, which is when the peer's preface has
    // been applied. Dropping this future on connect_timeout drops `conn`.
    std::future::poll_fn(|cx| {
        if send.current_max_send_streams() > 0 {
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut conn).poll(cx) {
            Poll::Ready(result) => {
                drop(result);
                Poll::Ready(Err(Status::unavailable(
                    "http/2 preface: connection closed",
                )))
            }
            Poll::Pending => {
                if send.current_max_send_streams() > 0 {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }
    })
    .await?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let busy = (config.connection_idle().is_some() || config.connection_age().is_some())
        .then(crate::keepalive::Busy::new);
    drop(tokio::spawn(async move {
        tokio::select! {
            r = conn => {
                drop(r);
            }
            _ = crate::keepalive::wait_opt(dead) => {}
            _ = crate::keepalive::wait(stop_rx) => {}
        }
    }));
    Ok(Dialed {
        send,
        stop: stop_tx,
        busy,
    })
}

pub(crate) fn attach_conn<T>(
    response: crate::request::Response<Streaming<T>>,
    lease: Option<crate::keepalive::Lease>,
    driver: Option<watch::Sender<bool>>,
    reset: Option<watch::Sender<bool>>,
    rpc_slot: Option<OwnedSemaphorePermit>,
) -> crate::request::Response<Streaming<T>> {
    response.map(|stream| {
        stream
            .bind_conn(lease, driver, reset)
            .bind_rpc_slot(rpc_slot)
    })
}
