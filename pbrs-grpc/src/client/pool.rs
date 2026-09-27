//! Connection pool: slots, dialing, handshake, and idle/age watches.

use super::retry::RetryStatsRecorder;
use super::{Channel, Target};
use crate::config::ChannelConfig;
use crate::lb::{Pick, PickFirst, ensure_registered, select_lb_policy};
use crate::limits::ByteBudgetTracker;
use crate::resolver::{
    Resolution, ResolvedAddress, ResolverConfig, ResolverHandle, ResolverTask, parse_target_uri,
    resolver_for,
};
use crate::service_config::{ServiceConfig, SharedServiceConfig};
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
use tokio::task::JoinSet;

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
    /// Pinned address on LB channels; `None` dials the channel endpoint.
    pub(crate) address: Option<ResolvedAddress>,
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
    /// Keeps the refresh task alive; subchannels consume the watch in CH-03.
    #[allow(dead_code, reason = "task guard until CH-03 wires subchannels")]
    pub(crate) resolver: Option<ResolverHandle>,
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
    /// Resolver-managed target. `display` is the original target URI
    /// (never a resolved IP); `current` is the latest snapshot. Each
    /// dial borrows the current `Arc` without awaiting, so resolver
    /// updates never block picks.
    Resolved {
        display: String,
        current: watch::Receiver<Arc<Resolution>>,
        /// pick_first driver; `None` dials the first snapshot address
        /// (a selected policy with no runtime yet).
        lb: Option<Arc<PickFirst>>,
    },
}

impl Endpoint {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Tcp(host) => host.clone(),
            #[cfg(unix)]
            Self::Unix(path) => path.display().to_string(),
            Self::Once => "once".to_owned(),
            Self::Resolved { display, .. } => display.clone(),
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
        None,
        SharedServiceConfig::default(),
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
        None,
        SharedServiceConfig::default(),
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
        None,
        SharedServiceConfig::default(),
    ))
}

/// Resolve `uri` once, then serve dials from the live snapshot.
/// Fails fast when the target is malformed, the scheme is unknown, or
/// the initial lookup is empty or fails. Slots stay lazy: each dial
/// borrows the current snapshot without awaiting.
pub(crate) async fn connect_uri_inner(
    uri: &str,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    resolver: ResolverConfig,
) -> Result<Channel, Status> {
    let target = parse_target_uri(uri)?;
    if tls.is_some() && matches!(target.scheme.as_str(), "unix" | "unix-abstract") {
        return Err(Status::invalid_argument(
            "TLS over a Unix socket is not supported",
        ));
    }
    let built = resolver_for(&target, &resolver).await?;
    if built.initial.is_empty() {
        return Err(Status::unavailable(format!(
            "resolve {uri}: no addresses in initial snapshot"
        )));
    }
    let authority = match target.scheme.as_str() {
        "unix" | "unix-abstract" => unix_authority(),
        _ => Target::from(target.authority()).parse()?,
    };
    // A21: adopt the initial document, failing the channel when it is
    // invalid and nothing good precedes it.
    let shared = SharedServiceConfig::default();
    let mut adopted = None;
    if let Some(json) = built.initial.service_config() {
        shared.set(ServiceConfig::parse(json)?);
        adopted = Some(json.to_owned());
    }
    // Effective LB policy: explicit selection wins, else pick_first
    // is the default. Anything selected but unrunnable fails fast.
    ensure_registered();
    let doc = shared.get();
    let lb = match doc.as_ref().and_then(|state| {
        if state.config.lb_policies().is_empty() {
            None
        } else {
            Some(select_lb_policy(&state.config))
        }
    }) {
        Some(Some(selected)) if selected.name == "pick_first" => Some(PickFirst::from_config(
            doc.as_ref().map(|state| &state.config),
        )),
        Some(Some(selected)) => {
            return Err(Status::invalid_argument(format!(
                "loadBalancingConfig selected {:?}, which has no runtime in this build",
                selected.name
            )));
        }
        Some(None) => {
            return Err(Status::invalid_argument(
                "loadBalancingConfig lists no registered policy",
            ));
        }
        None => Some(PickFirst::from_config(None)),
    };
    let initial_addrs = built.initial.addresses().to_vec();
    let mut handle = built.into_handle();
    if let Some(policy) = lb.as_ref() {
        policy.update(initial_addrs).await;
        let watch = handle.watch.clone();
        let worker = Arc::clone(policy);
        handle.guard(ResolverTask::new(tokio::spawn(async move {
            let mut rx = watch;
            loop {
                if rx.changed().await.is_err() {
                    return;
                }
                let snapshot = rx.borrow_and_update().clone();
                worker.update(snapshot.addresses().to_vec()).await;
            }
        })));
    }
    let endpoint = Endpoint::Resolved {
        display: uri.to_owned(),
        current: handle.watch.clone(),
        lb,
    };
    // Later documents adopt live; invalid ones keep the last good.
    handle.guard(ResolverTask::new(tokio::spawn(adopt_loop(
        handle.watch.clone(),
        shared.clone(),
        adopted,
    ))));
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        empty_slots(config.connection_count()),
        Some(handle),
        shared,
    ))
}

/// Adopt resolver-delivered service configs (A21): valid documents
/// replace the lineage's config, invalid ones are ignored.
async fn adopt_loop(
    mut watch: tokio::sync::watch::Receiver<std::sync::Arc<Resolution>>,
    shared: SharedServiceConfig,
    mut adopted: Option<String>,
) {
    loop {
        if watch.changed().await.is_err() {
            return;
        }
        let snapshot = watch.borrow_and_update().clone();
        let Some(json) = snapshot.service_config() else {
            continue;
        };
        if adopted.as_deref() == Some(json) {
            continue;
        }
        if let Ok(parsed) = ServiceConfig::parse(json) {
            shared.set(parsed);
            adopted = Some(json.to_owned());
        }
    }
}

pub(crate) fn finish_channel(
    endpoint: Endpoint,
    authority: Authority,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    slots: Vec<Mutex<ConnSlot>>,
    resolver: Option<ResolverHandle>,
    service_config: SharedServiceConfig,
) -> Channel {
    let https = tls.is_some();
    let inner = Arc::new(ChannelInner {
        slots,
        next: AtomicUsize::new(0),
        endpoint,
        resolver,
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
        service_config,
        retry_stats: Arc::new(RetryStatsRecorder::new()),
        binlog: None,
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
                address: None,
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
                address: None,
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
        if let Endpoint::Resolved {
            lb: Some(policy), ..
        } = &self.endpoint
        {
            return self.acquire_lb(policy, wait_for_ready, observer).await;
        }
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

    /// Acquire through pick_first: one sticky slot whose address tracks
    /// the policy. A live connection to the picked address is reused;
    /// otherwise the address is dialed and stored, displacing a
    /// different address with a graceful handoff (in-flight streams on
    /// the old connection finish within the age grace; they are never
    /// migrated). Raced against the RPC deadline by the caller, like
    /// [`Self::acquire`].
    async fn acquire_lb(
        self: &Arc<Self>,
        policy: &PickFirst,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let mut updates = policy.watch();
        let mut attempt = 0usize;
        loop {
            let addr = match policy.pick().await {
                Pick::Use(addr) => addr,
                Pick::Wait => {
                    if !wait_for_ready {
                        return Err(Status::unavailable(format!(
                            "resolve {display}: no ready address"
                        )));
                    }
                    updates.changed().await.ok();
                    continue;
                }
                Pick::Fail(status) => {
                    if !wait_for_ready {
                        return Err(status);
                    }
                    // Backoff expiry bumps nothing, so re-poll as well
                    // as watching for policy movement.
                    tokio::select! {
                        _ = updates.changed() => {}
                        () = tokio::time::sleep(Duration::from_millis(20)) => {}
                    }
                    continue;
                }
            };
            let (handle, lease, r#gen, driver, slot_addr) = {
                let slot = self.slot(0)?.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    slot.address.clone(),
                )
            };
            if slot_addr.as_ref() == Some(&addr) {
                if let Some(handle) = handle {
                    if let Ok(ready) = handle.ready().await {
                        return Ok(LiveConn {
                            send: ready,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                        });
                    }
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            let plan = policy.race_plan().await;
            let tried = plan.len();
            match dial_racing(&display, plan, self.dial, self.tls.as_ref()).await {
                Ok((winner, dialed)) => {
                    let addr = winner;
                    policy.note_success(&addr).await;
                    if let Some(obs) = observer {
                        if r#gen > 0 || attempt > 0 {
                            let attempt_u32 =
                                u32::try_from(attempt).unwrap_or(u32::MAX).saturating_add(1);
                            obs.on_reconnect(&ReconnectEvent {
                                target: &display,
                                attempt: attempt_u32,
                                duration: dial_start.elapsed(),
                                status: None,
                            });
                        }
                    }
                    let mut slot = self.slot(0)?.lock().await;
                    if slot.r#gen == r#gen {
                        let displaced = if slot.address.as_ref() != Some(&addr) {
                            slot.address = Some(addr);
                            replace_for_handoff(&mut slot, dialed)
                        } else {
                            store_dialed(&mut slot, dialed);
                            None
                        };
                        // LB slots always track busyness: handoff drains
                        // wait on it even without idle/age configured.
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let send = slot.send.clone().ok_or_else(|| {
                            Status::unavailable("connection vanished after store")
                        })?;
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        drop(slot);
                        if let Some(old) = displaced {
                            spawn_handoff_drain(old, self.dial.age_grace());
                        }
                        spawn_idle_watch(Arc::clone(self), 0);
                        spawn_age_watch(Arc::clone(self), 0);
                        return Ok(LiveConn {
                            send,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_round_failed(tried, status.clone()).await;
                    if let Some(obs) = observer {
                        if r#gen > 0 || attempt > 0 {
                            let attempt_u32 =
                                u32::try_from(attempt).unwrap_or(u32::MAX).saturating_add(1);
                            obs.on_reconnect(&ReconnectEvent {
                                target: &display,
                                attempt: attempt_u32,
                                duration: dial_start.elapsed(),
                                status: Some(status.code()),
                            });
                        }
                    }
                    if wait_for_ready {
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

/// A displaced connection awaiting its drain: in-flight streams finish
/// within the grace, then the driver stops.
struct Displaced {
    stop: watch::Sender<bool>,
    busy: Option<Arc<crate::keepalive::Busy>>,
}

/// Store `dialed`, returning the displaced connection for graceful
/// drain (instead of stopping it like [`store_dialed`]).
fn replace_for_handoff(slot: &mut ConnSlot, dialed: Dialed) -> Option<Displaced> {
    let old = match (slot.stop.take(), slot.busy.take()) {
        (Some(stop), busy) => Some(Displaced { stop, busy }),
        (None, _) => None,
    };
    slot.r#gen = slot.r#gen.wrapping_add(1);
    slot.send = Some(dialed.send.clone());
    slot.stop = Some(dialed.stop);
    slot.busy = dialed.busy;
    old
}

fn spawn_handoff_drain(old: Displaced, grace: Duration) {
    drop(tokio::spawn(async move {
        if let Some(busy) = old.busy {
            tokio::select! {
                () = busy.wait_idle() => {}
                () = tokio::time::sleep(grace) => {}
            }
        }
        old.stop.send(true).ok();
    }));
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
            finish_tcp(tcp, config, tls).await
        }
        Endpoint::Resolved {
            display, current, ..
        } => {
            let snapshot = current.borrow().clone();
            let addr = snapshot.addresses().first().cloned().ok_or_else(|| {
                Status::unavailable(format!(
                    "resolve {display}: no addresses in current snapshot"
                ))
            })?;
            dial_resolved(display, addr, config, tls).await
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

/// Tune a connected TCP stream, then run TLS (when configured) and the
/// HTTP/2 handshake. Shared by plain and resolver-managed dials.
async fn finish_tcp(
    tcp: tokio::net::TcpStream,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
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

/// Dial one resolver-chosen address. Unix paths reject TLS, matching
/// the plain Unix dial; abstract names dial on Linux only.
async fn dial_resolved(
    display: &str,
    addr: ResolvedAddress,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
    match addr {
        ResolvedAddress::Tcp(sock) => {
            let tcp = crate::tcp::connect_addr(sock, config.bound_local_address())
                .await
                .map_err(|e| Status::unavailable(format!("connect {display} [{sock}]: {e}")))?;
            finish_tcp(tcp, config, tls).await
        }
        ResolvedAddress::Unix(path) => {
            #[cfg(unix)]
            {
                if tls.is_some() {
                    return Err(Status::invalid_argument(
                        "TLS over a Unix socket is not supported",
                    ));
                }
                let io = UnixStream::connect(path)
                    .await
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                finish_h2(config, io).await
            }
            #[cfg(not(unix))]
            {
                let _ = (path, tls);
                Err(Status::unavailable(format!(
                    "connect {display}: unix: is not supported on this platform"
                )))
            }
        }
        ResolvedAddress::UnixAbstract(name) => {
            #[cfg(target_os = "linux")]
            {
                if tls.is_some() {
                    return Err(Status::invalid_argument(
                        "TLS over a Unix socket is not supported",
                    ));
                }
                let addr = tokio::net::unix::SocketAddr::from_abstract_name(name)
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                let io = UnixStream::connect_addr(&addr)
                    .await
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                finish_h2(config, io).await
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (name, tls);
                Err(Status::unavailable(format!(
                    "connect {display}: unix-abstract: is Linux-only"
                )))
            }
        }
    }
}

/// Happy-Eyeballs connection-attempt delay (A61, RFC 8305 §5):
/// start the next address when the previous attempt neither
/// succeeds nor fails within 250ms. A61 allows a channel arg in
/// [100ms, 2s]; no knob yet, so the default is fixed.
const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);

/// Race full dials over the policy's ordered plan (A61): attempts
/// start staggered by [`HAPPY_EYEBALLS_DELAY`], a fast failure starts
/// the next address immediately, and the first fully READY dial wins
/// (TCP, TLS, and HTTP/2 all complete). Losers are aborted and any
/// already-completed loser connection is shut down, so no duplicate
/// established connection leaks. Each attempt is bounded by the
/// handshake timeout, matching the direct dial path. All-failed
/// returns the last error.
async fn dial_racing(
    display: &str,
    plan: Vec<ResolvedAddress>,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<(ResolvedAddress, Dialed), Status> {
    let mut rest = plan.into_iter();
    let Some(first) = rest.next() else {
        return Err(Status::unavailable(format!(
            "resolve {display}: no ready address"
        )));
    };
    let timeout = config.handshake_timeout();
    let mut pending: JoinSet<Result<(ResolvedAddress, Dialed), Status>> = JoinSet::new();
    spawn_dial_attempt(&mut pending, display, first, config, tls, timeout);
    let mut stagger = Box::pin(tokio::time::sleep(HAPPY_EYEBALLS_DELAY));
    let mut last_error: Option<Status> = None;
    loop {
        let has_more = rest.len() > 0;
        tokio::select! {
            () = &mut stagger, if has_more && !pending.is_empty() => {
                if let Some(next) = rest.next() {
                    spawn_dial_attempt(&mut pending, display, next, config, tls, timeout);
                }
                stagger.as_mut().reset(tokio::time::Instant::now() + HAPPY_EYEBALLS_DELAY);
            }
            result = pending.join_next() => {
                match result {
                    Some(Ok(Ok(won))) => {
                        pending.abort_all();
                        while let Some(done) = pending.join_next().await {
                            if let Ok(Ok((_, dialed))) = done {
                                dialed.stop.send(true).ok();
                            }
                        }
                        return Ok(won);
                    }
                    other => {
                        match other {
                            Some(Ok(Err(status))) => last_error = Some(status),
                            Some(Err(_)) if last_error.is_none() => {
                                last_error = Some(Status::unavailable(format!(
                                    "connect {display}: dial task ended"
                                )));
                            }
                            Some(Ok(Ok(_))) | Some(Err(_)) | None => {}
                        }
                        if pending.is_empty() {
                            if let Some(next) = rest.next() {
                                spawn_dial_attempt(&mut pending, display, next, config, tls, timeout);
                                stagger
                                    .as_mut()
                                    .reset(tokio::time::Instant::now() + HAPPY_EYEBALLS_DELAY);
                            } else {
                                return Err(last_error.unwrap_or_else(|| {
                                    Status::unavailable(format!(
                                        "connect {display}: no addresses answered"
                                    ))
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Spawn one bounded dial attempt onto the race set.
fn spawn_dial_attempt(
    pending: &mut JoinSet<Result<(ResolvedAddress, Dialed), Status>>,
    display: &str,
    addr: ResolvedAddress,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
    timeout: Duration,
) {
    let display = display.to_owned();
    let owned_tls = tls.cloned();
    pending.spawn(async move {
        let dial = dial_resolved(&display, addr.clone(), config, owned_tls.as_ref());
        match tokio::time::timeout(timeout, dial).await {
            Ok(Ok(dialed)) => Ok((addr, dialed)),
            Ok(Err(status)) => Err(status),
            Err(_) => Err(Status::unavailable(format!(
                "connect {display} [{addr:?}]: timed out after {timeout:?}"
            ))),
        }
    });
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
