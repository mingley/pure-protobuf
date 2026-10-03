//! Connection pool: slots, dialing, handshake, and idle/age watches.

use super::retry::RetryStatsRecorder;
use super::streaming::run_server_stream;
use super::{Channel, Target};
use crate::config::ChannelConfig;
use crate::health::{HealthCheckRequest, HealthCheckResponse};
use crate::lb::{
    HealthSignal, LbPolicy, LeastRequest, OutlierDetection, Pick, PickFirst, Priority,
    RandomSubsetting, RingHash, RoundRobin, WeightedRoundRobin, disables_health_check,
    ensure_least_request_registered, ensure_outlier_detection_registered,
    ensure_pick_first_registered, ensure_priority_registered, ensure_random_subsetting_registered,
    ensure_ring_hash_registered, ensure_round_robin_registered,
    ensure_weighted_round_robin_registered, instantiate, select_lb_policy, signal_for,
    transient_backoff,
};
use crate::limits::{ByteBudgetTracker, BytePermit};
use crate::metadata::Metadata;
use crate::orca::{OrcaLoadReport, OrcaLoadReportRequest};
use crate::resolver::{
    Resolution, ResolvedAddress, ResolverConfig, ResolverHandle, ResolverTask, parse_target_uri,
    resolver_for,
};
use crate::service_config::{ServiceConfig, SharedServiceConfig};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{LifecycleObserver, ReconnectEvent};
use crate::tls::ClientTls;
use crate::transport::{ClientBuilder, ClientConnection, SendRequest, h2 as backend};
use http::uri::Authority;
use std::collections::HashMap;
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
    pub(crate) send: Option<backend::SendRequest>,
    /// Stops the connection driver (idle close, age close, lost-race handshake, drop).
    pub(crate) stop: Option<watch::Sender<bool>>,
    /// Outstanding RPCs; `None` when neither idle-close nor age is configured.
    pub(crate) busy: Option<Arc<crate::keepalive::Busy>>,
    /// Pinned address on LB channels; `None` dials the channel endpoint.
    pub(crate) address: Option<ResolvedAddress>,
    /// Channelz socket for the live connection; dropping unregisters.
    /// Taken (unregistering) wherever `send` is cleared.
    pub(crate) channelz_socket: Option<crate::channelz::SocketHandle>,
    /// Channelz subchannel for this entry's address (round_robin-table
    /// entries only; other slots hang sockets directly under the
    /// channel). Lives with the entry across redials.
    pub(crate) channelz_subchannel: Option<crate::channelz::SubchannelHandle>,
}

/// A finished handshake: the sender plus the handles that stop its driver.
pub(crate) struct Dialed {
    pub(crate) send: backend::SendRequest,
    pub(crate) stop: watch::Sender<bool>,
    pub(crate) busy: Option<Arc<crate::keepalive::Busy>>,
    /// Local socket address, when the transport reports one (TCP;
    /// Unix clients are unnamed).
    pub(crate) local_addr: Option<crate::channelz::EndpointAddr>,
    /// Connected peer address, when the transport reports one.
    pub(crate) peer_addr: Option<crate::channelz::EndpointAddr>,
}

#[derive(Debug)]
pub(crate) struct SlotLoad {
    in_flight: AtomicUsize,
    max_streams: AtomicUsize,
}

impl SlotLoad {
    fn new() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            max_streams: AtomicUsize::new(usize::MAX),
        }
    }

    fn snapshot(&self) -> (usize, usize) {
        (
            self.in_flight.load(Ordering::Relaxed),
            self.max_streams.load(Ordering::Relaxed).max(1),
        )
    }

    fn start(self: &Arc<Self>, max_streams: usize) -> SlotLoadGuard {
        self.max_streams
            .store(max_streams.max(1), Ordering::Relaxed);
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        SlotLoadGuard {
            load: Arc::clone(self),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SlotLoadGuard {
    load: Arc<SlotLoad>,
}

impl Drop for SlotLoadGuard {
    fn drop(&mut self) {
        self.load.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A sender taken from a pool slot, plus the generation so a raced `GOAWAY`
/// can discard this slot instead of writing into a reconnect that already
/// landed.
pub(crate) struct LiveConn {
    pub(crate) send: backend::SendRequest,
    pub(crate) load: Option<SlotLoadGuard>,
    pub(crate) lease: Option<crate::keepalive::Lease>,
    /// Clone of the slot's driver-stop sender. Held on a received
    /// [`Streaming`] so dropping the last [`Channel`] does not stop the
    /// connection under an in-flight stream.
    pub(crate) driver: Option<watch::Sender<bool>>,
    pub(crate) slot: usize,
    pub(crate) r#gen: u64,
    /// round_robin subchannel address; `None` uses `slot`/`gen` in
    /// [`ChannelInner::slots`], `Some` discards by address in the
    /// round_robin table.
    pub(crate) rr_addr: Option<ResolvedAddress>,
    /// Channelz socket serving this connection, for stream/message
    /// counters. The registry resolves it per record, so a socket
    /// that unregistered mid-RPC (handoff, discard) silently drops
    /// late increments instead of misattributing them.
    pub(crate) channelz_socket: Option<crate::channelz::SocketId>,
}

/// One subchannel per round_robin address, grown and shrunk with the
/// resolver list. Entries are never reindexed, so an in-flight
/// [`LiveConn`] keeps its address identity across churn.
pub(crate) struct RrTable {
    conns: Mutex<HashMap<ResolvedAddress, Arc<Mutex<ConnSlot>>>>,
}

/// Client-side health checking for one acquire (A17): the watched
/// service name. Resolved per acquire from the channel's service
/// config plus the master switch; subchannels snapshot it at dial.
#[derive(Clone, Debug)]
pub(crate) struct HealthDirective {
    pub(crate) service: String,
}

impl RrTable {
    fn new() -> Self {
        Self {
            conns: Mutex::new(HashMap::new()),
        }
    }
}

/// Backoff between wait-for-ready handshake attempts, in milliseconds.
/// Caps at the last entry; see [`ChannelInner::acquire`].
const WAIT_FOR_READY_BACKOFF_MS: &[u64] = &[20, 40, 80, 160, 320, 640, 1000];

pub(crate) struct ChannelInner {
    pub(crate) slots: Vec<Mutex<ConnSlot>>,
    pub(crate) loads: Vec<Arc<SlotLoad>>,
    pub(crate) next: AtomicUsize,
    pub(crate) authority: Authority,
    pub(crate) endpoint: Endpoint,
    /// Keeps the refresh task alive; subchannels consume the watch in CH-03.
    #[allow(dead_code, reason = "task guard until CH-03 wires subchannels")]
    pub(crate) resolver: Option<ResolverHandle>,
    pub(crate) tls: Option<ClientTls>,
    /// Per-address subchannels for round_robin; `None` on direct and
    /// pick_first channels, which use [`Self::slots`].
    pub(crate) rr: Option<Arc<RrTable>>,
    /// Settings used to dial. Per-clone overlays on [`Channel`] (timeout,
    /// wait-for-ready, send_compressed, gzip_compression_level, message sizes,
    /// stream_buffer, max_send_buffer_size, https_scheme, origin) do not change
    /// how a dead slot is redialed.
    pub(crate) dial: ChannelConfig,
    /// Channelz channel registration; dropping the last [`Channel`]
    /// unregisters the channel (and its direct sockets).
    pub(crate) channelz: crate::channelz::ChannelHandle,
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
        /// LB driver; `None` dials the first snapshot address
        /// (a selected policy with no runtime yet).
        lb: Option<LbPolicy>,
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

/// Register a channelz channel named for its endpoint.
pub(crate) fn register_channel_for(endpoint: &Endpoint) -> crate::channelz::ChannelHandle {
    let target = endpoint.describe();
    crate::channelz::Registry::global_shared().register_channel(target.clone(), target)
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
    let channelz = register_channel_for(&endpoint);
    let channel_id = channelz.id();
    let secure = tls.is_some();
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        live_slots(sends, channel_id, secure),
        None,
        SharedServiceConfig::default(),
        None,
        channelz,
    ))
}

pub(crate) fn connect_lazy_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Tcp(target.authority().to_owned());
    let authority = target.parse()?;
    let channelz = register_channel_for(&endpoint);
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        empty_slots(config.connection_count()),
        None,
        SharedServiceConfig::default(),
        None,
        channelz,
    ))
}

fn connect_per_core_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    cores: usize,
) -> Result<Vec<Channel>, Status> {
    if cores == 0 {
        return Err(Status::invalid_argument(
            "per-core connect needs at least one core",
        ));
    }
    let mut channels = Vec::with_capacity(cores);
    for _ in 0..cores {
        channels.push(connect_lazy_inner(target.clone(), config, tls.clone())?);
    }
    Ok(channels)
}

// Per-core constructors live with the pool rather than on `Channel`'s home
// module: each channel owns an independent pool, and this is where pools
// are built.
impl Channel {
    /// Connect one lazy channel per core, each with an independent pool.
    ///
    /// Drive `channels[i]` from core `i`'s thread and no pool mutex is ever
    /// contended cross-core. Channels dial on first use, on whatever
    /// runtime drives them, so construct (or first call) each channel from
    /// its own core's runtime to keep dials and connection drivers local
    /// too. Applies to every call shape.
    ///
    /// Returns `invalid_argument` when `cores` is zero.
    pub fn connect_per_core(
        target: impl Into<Target>,
        cores: usize,
    ) -> Result<Vec<Channel>, Status> {
        Self::connect_per_core_with(target, ChannelConfig::default(), cores)
    }

    /// [`Self::connect_per_core`] with `config`. Applies to every call shape.
    pub fn connect_per_core_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        cores: usize,
    ) -> Result<Vec<Channel>, Status> {
        connect_per_core_inner(target.into(), config, None, cores)
    }

    /// [`Self::connect_per_core`] over TLS. Applies to every call shape.
    pub fn connect_tls_per_core(
        target: impl Into<Target>,
        cores: usize,
        tls: ClientTls,
    ) -> Result<Vec<Channel>, Status> {
        Self::connect_tls_per_core_with(target, ChannelConfig::default(), cores, tls)
    }

    /// [`Self::connect_tls_per_core`] with `config`. Applies to every call
    /// shape.
    pub fn connect_tls_per_core_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        cores: usize,
        tls: ClientTls,
    ) -> Result<Vec<Channel>, Status> {
        connect_per_core_inner(target.into(), config, Some(tls), cores)
    }
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
    let channelz = register_channel_for(&endpoint);
    let channel_id = channelz.id();
    Ok(finish_channel(
        endpoint,
        unix_authority(),
        config,
        None,
        live_slots(sends, channel_id, false),
        None,
        SharedServiceConfig::default(),
        None,
        channelz,
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
    ensure_least_request_registered();
    ensure_outlier_detection_registered();
    ensure_pick_first_registered();
    ensure_priority_registered();
    ensure_random_subsetting_registered();
    ensure_ring_hash_registered();
    ensure_round_robin_registered();
    ensure_weighted_round_robin_registered();
    let doc = shared.get();
    let (lb, lb_name) = match doc.as_ref().and_then(|state| {
        if state.config.lb_policies().is_empty() {
            None
        } else {
            Some(select_lb_policy(&state.config))
        }
    }) {
        Some(Some(selected)) => match instantiate(&selected.policy) {
            Ok(lb) => (Some(lb), selected.name.clone()),
            Err(_) => {
                return Err(Status::invalid_argument(format!(
                    "loadBalancingConfig selected {:?}, which has no runtime in this build",
                    selected.name
                )));
            }
        },
        Some(None) => {
            return Err(Status::invalid_argument(
                "loadBalancingConfig lists no registered policy",
            ));
        }
        None => (
            Some(LbPolicy::PickFirst(PickFirst::from_config(None))),
            "pick_first (default)".to_owned(),
        ),
    };
    let rr = matches!(
        lb,
        Some(LbPolicy::RoundRobin(_))
            | Some(LbPolicy::WeightedRoundRobin(_))
            | Some(LbPolicy::RingHash(_))
            | Some(LbPolicy::LeastRequest(_))
            | Some(LbPolicy::RandomSubsetting(_))
            | Some(LbPolicy::Priority(_))
            | Some(LbPolicy::OutlierDetection(_))
    )
    .then(|| Arc::new(RrTable::new()));
    let initial_addrs = built.initial.addresses().to_vec();
    let mut handle = built.into_handle();
    // Channelz registers before the resolver tasks spawn so they can
    // trace into it; the handle moves into the channel below.
    let channelz = crate::channelz::Registry::global_shared().register_channel(uri, uri);
    let channel_id = channelz.id();
    let global = crate::channelz::Registry::global();
    global.trace_channel(
        channel_id,
        crate::channelz::TraceSeverity::Info,
        format!(
            "Initial resolution: {} addresses (LB policy: {lb_name})",
            initial_addrs.len()
        ),
    );
    if let Some(policy) = lb.clone() {
        policy.update(initial_addrs.clone()).await;
        let watch = handle.watch.clone();
        let age_grace = config.age_grace();
        let table = rr.clone();
        let mut prev = initial_addrs.len();
        handle.guard(ResolverTask::new(tokio::spawn(async move {
            let mut rx = watch;
            loop {
                if rx.changed().await.is_err() {
                    return;
                }
                let snapshot = rx.borrow_and_update().clone();
                let addrs = snapshot.addresses().to_vec();
                policy.update(addrs.clone()).await;
                if let Some(table) = table.as_ref() {
                    reconcile_rr(table, &addrs, age_grace).await;
                }
                // A3: interesting resolution events (0/N edges stand out).
                if addrs.len() != prev {
                    let global = crate::channelz::Registry::global();
                    if prev == 0 {
                        global.trace_channel(
                            channel_id,
                            crate::channelz::TraceSeverity::Info,
                            format!("Address list repopulated: {} addresses", addrs.len()),
                        );
                    } else if addrs.is_empty() {
                        global.trace_channel(
                            channel_id,
                            crate::channelz::TraceSeverity::Warning,
                            "Address list emptied: 0 addresses".to_owned(),
                        );
                    } else {
                        global.trace_channel(
                            channel_id,
                            crate::channelz::TraceSeverity::Info,
                            format!(
                                "Address list updated: {} addresses (was {prev})",
                                addrs.len()
                            ),
                        );
                    }
                    prev = addrs.len();
                }
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
        channel_id,
    ))));
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        empty_slots(config.connection_count()),
        Some(handle),
        shared,
        rr,
        channelz,
    ))
}

/// Adopt resolver-delivered service configs (A21): valid documents
/// replace the lineage's config, invalid ones are ignored. Adoptions
/// trace on the channel (A3).
async fn adopt_loop(
    mut watch: tokio::sync::watch::Receiver<std::sync::Arc<Resolution>>,
    shared: SharedServiceConfig,
    mut adopted: Option<String>,
    channel_id: crate::channelz::ChannelId,
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
            crate::channelz::Registry::global().trace_channel(
                channel_id,
                crate::channelz::TraceSeverity::Info,
                "Service config changed (adopted new document)".to_owned(),
            );
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "channel assembly: endpoint, authority, config, tls, slots, resolver, service config, rr table, channelz"
)]
pub(crate) fn finish_channel(
    endpoint: Endpoint,
    authority: Authority,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    slots: Vec<Mutex<ConnSlot>>,
    resolver: Option<ResolverHandle>,
    service_config: SharedServiceConfig,
    rr: Option<Arc<RrTable>>,
    channelz: crate::channelz::ChannelHandle,
) -> Channel {
    let https = tls.is_some();
    let loads = (0..slots.len())
        .map(|_| Arc::new(SlotLoad::new()))
        .collect();
    let inner = Arc::new(ChannelInner {
        slots,
        loads,
        next: AtomicUsize::new(0),
        authority: authority.clone(),
        endpoint,
        resolver,
        tls,
        rr,
        dial: config,
        channelz,
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

pub(crate) fn live_slots(
    dialed: Vec<Dialed>,
    channel: crate::channelz::ChannelId,
    tls: bool,
) -> Vec<Mutex<ConnSlot>> {
    dialed
        .into_iter()
        .map(|d| {
            // Channelz: eagerly dialed connections register their
            // sockets here (the grab path reuses them and never runs
            // `note_dial_ok`). Direct slots hang under the channel.
            let security = if tls {
                crate::channelz::SocketSecurity::Tls {
                    local_certificate: Vec::new(),
                    remote_certificate: Vec::new(),
                }
            } else {
                crate::channelz::SocketSecurity::None
            };
            let global = crate::channelz::Registry::global();
            global.set_channel_state(channel, crate::channelz::Connectivity::Ready);
            let guard = crate::channelz::Registry::global_shared().register_socket(
                crate::channelz::SocketParent::Channel(channel),
                d.local_addr,
                d.peer_addr,
                None,
                security,
                false,
            );
            Mutex::new(ConnSlot {
                r#gen: 0,
                send: Some(d.send),
                stop: Some(d.stop),
                busy: d.busy,
                address: None,
                channelz_socket: Some(guard),
                channelz_subchannel: None,
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
                channelz_socket: None,
                channelz_subchannel: None,
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
            return Ok(0);
        }
        let start = self.next.fetch_add(1, Ordering::Relaxed) % n;
        let mut best = None;
        let mut best_load = usize::MAX;
        let mut fallback = start;
        let mut fallback_load = usize::MAX;
        for offset in 0..n {
            let idx = (start + offset) % n;
            let Some(load) = self.loads.get(idx) else {
                continue;
            };
            let (in_flight, max_streams) = load.snapshot();
            if in_flight < fallback_load {
                fallback = idx;
                fallback_load = in_flight;
            }
            if in_flight < max_streams && in_flight < best_load {
                best = Some(idx);
                best_load = in_flight;
            }
        }
        Ok(best.unwrap_or(fallback))
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
        health: Option<HealthDirective>,
        hash: Option<u64>,
    ) -> Result<LiveConn, Status> {
        if let Endpoint::Resolved {
            lb: Some(policy), ..
        } = &self.endpoint
        {
            match policy {
                LbPolicy::PickFirst(pick) => {
                    return self
                        .acquire_lb(pick, wait_for_ready, observer, health)
                        .await;
                }
                LbPolicy::RoundRobin(rr) => {
                    return self.acquire_rr(rr, wait_for_ready, observer, health).await;
                }
                LbPolicy::WeightedRoundRobin(wrr) => {
                    return self
                        .acquire_wrr(wrr, wait_for_ready, observer, health)
                        .await;
                }
                LbPolicy::RingHash(ring) => {
                    return self
                        .acquire_rh(ring, hash, wait_for_ready, observer, health)
                        .await;
                }
                LbPolicy::LeastRequest(lr) => {
                    return self.acquire_lr(lr, wait_for_ready, observer, health).await;
                }
                LbPolicy::RandomSubsetting(subset) => {
                    return self
                        .acquire_subset(subset, hash, wait_for_ready, observer, health)
                        .await;
                }
                LbPolicy::Priority(priority) => {
                    return self
                        .acquire_priority(priority, hash, wait_for_ready, observer, health)
                        .await;
                }
                LbPolicy::OutlierDetection(outlier) => {
                    return self
                        .acquire_outlier(outlier, hash, wait_for_ready, observer, health)
                        .await;
                }
            }
        }
        let i = self.pick()?;
        let load = self
            .loads
            .get(i)
            .cloned()
            .ok_or_else(|| Status::unavailable("empty connection pool"))?;
        let mut attempt = 0usize;
        loop {
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = self.slot(i)?.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    let load = Some(load.start(ready.current_max_send_streams()));
                    return Ok(LiveConn {
                        send: ready,
                        load,
                        lease,
                        driver,
                        slot: i,
                        r#gen,
                        rr_addr: None,
                        channelz_socket,
                    });
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_channel_dial_start();
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
                        let dial_local = dialed.local_addr.clone();
                        let dial_peer = dialed.peer_addr.clone();
                        let send = store_dialed(&mut slot, dialed);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket =
                            Some(self.note_dial_ok(&mut slot, None, dial_peer, dial_local));
                        drop(slot);
                        spawn_idle_watch(Arc::clone(self), i);
                        spawn_age_watch(Arc::clone(self), i);
                        let load = Some(load.start(send.current_max_send_streams()));
                        return Ok(LiveConn {
                            send,
                            load,
                            lease,
                            driver,
                            slot: i,
                            r#gen,
                            rr_addr: None,
                            channelz_socket,
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
                    self.note_dial_err(None, None, &status).await;
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
        policy: &Arc<PickFirst>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::PickFirst(Arc::clone(policy));
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
            let (handle, lease, r#gen, driver, slot_addr, channelz_socket) = {
                let slot = self.slot(0)?.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    slot.address.clone(),
                    socket_id_of(&slot),
                )
            };
            if slot_addr.as_ref() == Some(&addr) {
                if let Some(handle) = handle {
                    if let Ok(ready) = handle.ready().await {
                        if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await
                        {
                            return Ok(LiveConn {
                                send: ready,
                                load: None,
                                lease,
                                driver,
                                slot: 0,
                                r#gen,
                                rr_addr: None,
                                channelz_socket,
                            });
                        }
                        continue;
                    }
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            let plan = policy.race_plan().await;
            if plan.is_empty() {
                // The picked address went unhealthy or pending between
                // pick and plan: re-pick instead of racing nothing.
                continue;
            }
            let tried = plan.len();
            self.note_channel_dial_start();
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
                        let dial_local = dialed.local_addr.clone();
                        let displaced = if slot.address.as_ref() != Some(&addr) {
                            slot.address = Some(addr.clone());
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
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            None,
                            Some(crate::channelz::EndpointAddr::from(&addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if let Some(old) = displaced {
                            spawn_handoff_drain(old, self.dial.age_grace());
                        }
                        spawn_idle_watch(Arc::clone(self), 0);
                        spawn_age_watch(Arc::clone(self), 0);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy (failover already advanced) or
                            // died while waiting: re-pick.
                            continue;
                        }
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: None,
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_round_failed(tried, status.clone()).await;
                    self.note_dial_err(None, None, &status).await;
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

    /// Acquire through round_robin: rotate over ready addresses, one
    /// subchannel each. A live connection to the picked address is
    /// reused; otherwise the address alone is dialed (no cross-address
    /// race: the policy already chose) and stored in its table entry.
    /// Failed dials mark the address down with backoff; removed
    /// addresses drain via [`reconcile_rr`]. Raced against the RPC
    /// deadline by the caller, like [`Self::acquire`].
    async fn acquire_rr(
        self: &Arc<Self>,
        policy: &Arc<RoundRobin>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::RoundRobin(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no round_robin table"
            )));
        };
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: rotate on.
                            continue;
                        }
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through weighted_round_robin: EDF-schedule over ready
    /// addresses, one subchannel each. Connection handling matches
    /// [`Self::acquire_rr`]; only the pick order is weight-driven.
    async fn acquire_wrr(
        self: &Arc<Self>,
        policy: &Arc<WeightedRoundRobin>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::WeightedRoundRobin(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no weighted_round_robin table"
            )));
        };
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        // OOB reports are advisory and never gate reuse.
                        spawn_oob_watch(self, &lb, &addr, &ready, &driver).await;
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: schedule on.
                            continue;
                        }
                        // OOB reports are advisory and never gate dials.
                        spawn_oob_watch(self, &lb, &addr, &send, &driver).await;
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through ring_hash: hash to one subchannel, one
    /// connection each. Connection handling matches
    /// [`Self::acquire_rr`]; only the pick is hash-driven, and a
    /// missing hash fails fast (A76) instead of queuing.
    async fn acquire_rh(
        self: &Arc<Self>,
        policy: &Arc<RingHash>,
        hash: Option<u64>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::RingHash(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no ring_hash table"
            )));
        };
        let mut updates = policy.watch();
        let mut attempt = 0usize;
        loop {
            let addr = match policy.pick_hash(hash).await {
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: re-hash on.
                            continue;
                        }
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through least_request: sample candidates and take
    /// the least-loaded subchannel, one connection each. Connection
    /// handling matches [`Self::acquire_rr`]; in-flight counts move
    /// via guards owned by the caller, not here.
    async fn acquire_lr(
        self: &Arc<Self>,
        policy: &Arc<LeastRequest>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::LeastRequest(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no least_request table"
            )));
        };
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: resample on.
                            continue;
                        }
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through random_subsetting: pick through the child
    /// policy over the rendezvous subset, one connection each.
    /// Connection handling matches [`Self::acquire_rr`]; the hash
    /// passes through for ring children. OOB pumps run when the
    /// child wants them.
    async fn acquire_subset(
        self: &Arc<Self>,
        policy: &Arc<RandomSubsetting>,
        hash: Option<u64>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::RandomSubsetting(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no random_subsetting table"
            )));
        };
        let mut updates = policy.watch();
        let mut attempt = 0usize;
        loop {
            let addr = match policy.pick_hash(hash).await {
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        // OOB reports are advisory and never gate reuse.
                        spawn_oob_watch(self, &lb, &addr, &ready, &driver).await;
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: repick on.
                            continue;
                        }
                        // OOB reports are advisory and never gate dials.
                        spawn_oob_watch(self, &lb, &addr, &send, &driver).await;
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through priority failover: the policy picks the
    /// current priority's child address (hashing through ring
    /// children); one subchannel per address shared across
    /// priorities. Connection handling matches
    /// [`Self::acquire_subset`].
    async fn acquire_priority(
        self: &Arc<Self>,
        policy: &Arc<Priority>,
        hash: Option<u64>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::Priority(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no priority table"
            )));
        };
        let mut updates = policy.watch();
        let mut attempt = 0usize;
        loop {
            let addr = match policy.pick_hash(hash).await {
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
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        // OOB reports are advisory and never gate reuse.
                        spawn_oob_watch(self, &lb, &addr, &ready, &driver).await;
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: repick on.
                            continue;
                        }
                        // OOB reports are advisory and never gate dials.
                        spawn_oob_watch(self, &lb, &addr, &send, &driver).await;
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Acquire through outlier_detection: the policy picks over the
    /// live (non-ejected) set, hashing through ring children; one
    /// subchannel per address. Connection handling matches
    /// [`Self::acquire_subset`]; call outcomes flow back via
    /// `note_call_status` on completion (see the unary/streaming
    /// paths).
    async fn acquire_outlier(
        self: &Arc<Self>,
        policy: &Arc<OutlierDetection>,
        hash: Option<u64>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let display = self.endpoint.describe();
        let lb = LbPolicy::OutlierDetection(Arc::clone(policy));
        let Some(table) = self.rr.clone() else {
            return Err(Status::unavailable(format!(
                "resolve {display}: no outlier_detection table"
            )));
        };
        let mut updates = policy.watch();
        let mut attempt = 0usize;
        loop {
            let addr = match policy.pick_hash(hash).await {
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
                    // Backoff expiry and sweep timers bump nothing, so
                    // re-poll as well as watching for policy movement.
                    tokio::select! {
                        _ = updates.changed() => {}
                        () = tokio::time::sleep(Duration::from_millis(20)) => {}
                    }
                    continue;
                }
            };
            let entry = {
                let mut conns = table.conns.lock().await;
                Arc::clone(conns.entry(addr.clone()).or_insert_with(|| {
                    Arc::new(Mutex::new(ConnSlot {
                        r#gen: 0,
                        send: None,
                        stop: None,
                        busy: None,
                        address: Some(addr.clone()),
                        channelz_socket: None,
                        channelz_subchannel: None,
                    }))
                }))
            };
            let (handle, lease, r#gen, driver, channelz_socket) = {
                let slot = entry.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (
                    slot.send.clone(),
                    lease,
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    if reuse_health_ok(self, &lb, &addr, &ready, &driver, health.as_ref()).await {
                        // OOB reports are advisory and never gate reuse.
                        spawn_oob_watch(self, &lb, &addr, &ready, &driver).await;
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            let dial_start = tokio::time::Instant::now();
            self.note_dial_start(&entry, &addr).await;
            match dial_racing(&display, vec![addr.clone()], self.dial, self.tls.as_ref()).await {
                Ok((won_addr, dialed)) => {
                    let dial_local = dialed.local_addr.clone();
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
                    let mut slot = entry.lock().await;
                    if slot.r#gen == r#gen {
                        let send = store_dialed(&mut slot, dialed);
                        slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let driver = slot.stop.clone();
                        let r#gen = slot.r#gen;
                        let channelz_socket = Some(self.note_dial_ok(
                            &mut slot,
                            Some(&addr),
                            Some(crate::channelz::EndpointAddr::from(&won_addr)),
                            dial_local,
                        ));
                        drop(slot);
                        if !reuse_health_ok(self, &lb, &addr, &send, &driver, health.as_ref()).await
                        {
                            // Unhealthy or died while waiting: repick on.
                            continue;
                        }
                        // OOB reports are advisory and never gate dials.
                        spawn_oob_watch(self, &lb, &addr, &send, &driver).await;
                        return Ok(LiveConn {
                            send,
                            load: None,
                            lease,
                            driver,
                            slot: 0,
                            r#gen,
                            rr_addr: Some(addr),
                            channelz_socket,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(status) => {
                    policy.note_failure(&addr, status.clone()).await;
                    self.note_dial_err(Some(&entry), Some(&addr), &status).await;
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

    /// Channelz: a dial is starting on a round_robin-table entry.
    /// Ensures the address subchannel, moves it to CONNECTING
    /// (deriving the channel state), and traces the attempt (A3).
    async fn note_dial_start(&self, entry: &Arc<Mutex<ConnSlot>>, addr: &ResolvedAddress) {
        let registry = crate::channelz::Registry::global_shared();
        let mut slot = entry.lock().await;
        let sub = slot.channelz_subchannel.get_or_insert_with(|| {
            registry.register_subchannel(self.channelz.id(), addr_text(addr))
        });
        let id = sub.id();
        drop(slot);
        let global = crate::channelz::Registry::global();
        global.set_subchannel_state(id, crate::channelz::Connectivity::Connecting);
        global.trace_subchannel(
            id,
            crate::channelz::TraceSeverity::Info,
            format!("Starting TCP connection to {}", addr_text(addr)),
        );
    }

    /// Channelz: a dial is starting on a pick_first/direct slot
    /// (sockets hang directly under the channel there).
    fn note_channel_dial_start(&self) {
        let global = crate::channelz::Registry::global();
        global.set_channel_state(
            self.channelz.id(),
            crate::channelz::Connectivity::Connecting,
        );
        global.trace_channel(
            self.channelz.id(),
            crate::channelz::TraceSeverity::Info,
            "Starting TCP connection",
        );
    }

    /// Channelz: a dial succeeded. Moves the subchannel (or channel)
    /// to READY, registers the socket under it, and stores the guard
    /// in the slot (replacing any previous generation, which
    /// unregisters). Returns the new socket id for the [`LiveConn`].
    /// Call only on the winning generation: lost-race dials must not
    /// disturb the winner's registration.
    fn note_dial_ok(
        &self,
        slot: &mut ConnSlot,
        sub_addr: Option<&ResolvedAddress>,
        remote: Option<crate::channelz::EndpointAddr>,
        local: Option<crate::channelz::EndpointAddr>,
    ) -> crate::channelz::SocketId {
        let registry = crate::channelz::Registry::global_shared();
        let global = crate::channelz::Registry::global();
        let parent = match sub_addr {
            Some(addr) if self.rr.is_some() => {
                let sub = slot.channelz_subchannel.get_or_insert_with(|| {
                    registry.register_subchannel(self.channelz.id(), addr_text(addr))
                });
                let id = sub.id();
                global.set_subchannel_state(id, crate::channelz::Connectivity::Ready);
                crate::channelz::SocketParent::Subchannel(id)
            }
            _ => {
                global.set_channel_state(self.channelz.id(), crate::channelz::Connectivity::Ready);
                crate::channelz::SocketParent::Channel(self.channelz.id())
            }
        };
        let security = if self.tls.is_some() {
            crate::channelz::SocketSecurity::Tls {
                local_certificate: Vec::new(),
                remote_certificate: Vec::new(),
            }
        } else {
            crate::channelz::SocketSecurity::None
        };
        let guard = registry.register_socket(parent, local, remote, None, security, false);
        let id = guard.id();
        slot.channelz_socket = Some(guard);
        id
    }

    /// Channelz: a dial failed. Moves the subchannel (or channel) to
    /// TRANSIENT_FAILURE and traces the error (A3).
    async fn note_dial_err(
        &self,
        entry: Option<&Arc<Mutex<ConnSlot>>>,
        addr: Option<&ResolvedAddress>,
        status: &Status,
    ) {
        let global = crate::channelz::Registry::global();
        let sub = match (entry, addr) {
            (Some(entry), Some(addr)) if self.rr.is_some() => {
                let mut slot = entry.lock().await;
                let sub = slot.channelz_subchannel.get_or_insert_with(|| {
                    crate::channelz::Registry::global_shared()
                        .register_subchannel(self.channelz.id(), addr_text(addr))
                });
                Some(sub.id())
            }
            _ => None,
        };
        match sub {
            Some(id) => {
                global.set_subchannel_state(id, crate::channelz::Connectivity::TransientFailure);
                global.trace_subchannel(
                    id,
                    crate::channelz::TraceSeverity::Error,
                    format!("TCP connection failed: {status}"),
                );
            }
            None => {
                global.set_channel_state(
                    self.channelz.id(),
                    crate::channelz::Connectivity::TransientFailure,
                );
                global.trace_channel(
                    self.channelz.id(),
                    crate::channelz::TraceSeverity::Error,
                    format!("TCP connection failed: {status}"),
                );
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
        slot.channelz_socket = None;
        slot.busy = None;
        if let Some(stop) = slot.stop.take() {
            stop.send(true).ok();
        }
        slot.r#gen = slot.r#gen.wrapping_add(1);
    }

    /// Discard a connection handed out as [`LiveConn`]: by
    /// round_robin address when `rr_addr` is set, else by pool slot.
    pub(crate) async fn discard_conn(
        &self,
        slot: usize,
        r#gen: u64,
        rr_addr: Option<&ResolvedAddress>,
    ) {
        if let Some(addr) = rr_addr {
            self.discard_rr(addr, r#gen).await;
        } else {
            self.discard(slot, r#gen).await;
        }
    }

    /// Drop a dead round_robin subchannel generation by address, so
    /// the next acquire redials it. Same generation guard as
    /// [`Self::discard`]; unknown addresses are already gone.
    pub(crate) async fn discard_rr(&self, addr: &ResolvedAddress, r#gen: u64) {
        let Some(table) = self.rr.as_ref() else {
            return;
        };
        let entry = table.conns.lock().await.get(addr).cloned();
        let Some(entry) = entry else {
            return;
        };
        let mut slot = entry.lock().await;
        if slot.r#gen != r#gen {
            return;
        }
        slot.send = None;
        slot.channelz_socket = None;
        slot.busy = None;
        if let Some(stop) = slot.stop.take() {
            stop.send(true).ok();
        }
        slot.r#gen = slot.r#gen.wrapping_add(1);
    }
}

/// Drop round_robin subchannels for removed addresses. Live
/// connections drain gracefully within `grace`; in-flight streams
/// are never migrated. New addresses connect lazily on first pick.
async fn reconcile_rr(table: &RrTable, addrs: &[ResolvedAddress], grace: Duration) {
    let removed: Vec<Arc<Mutex<ConnSlot>>> = {
        let mut conns = table.conns.lock().await;
        let gone: Vec<ResolvedAddress> = conns
            .keys()
            .filter(|addr| !addrs.contains(addr))
            .cloned()
            .collect();
        gone.into_iter()
            .filter_map(|addr| conns.remove(&addr))
            .collect()
    };
    for entry in removed {
        let mut slot = entry.lock().await;
        if let Some(stop) = slot.stop.take() {
            let busy = slot.busy.take();
            slot.send = None;
            slot.r#gen = slot.r#gen.wrapping_add(1);
            // The socket guard moves into the drain (staying
            // registered until in-flight streams finish); the
            // subchannel guard drops with the evicted entry.
            let channelz_socket = slot.channelz_socket.take();
            drop(slot);
            spawn_handoff_drain(
                Displaced {
                    stop,
                    busy,
                    channelz_socket,
                },
                grace,
            );
        }
    }
}

/// Channelz socket id currently serving a slot, if any.
fn socket_id_of(slot: &ConnSlot) -> Option<crate::channelz::SocketId> {
    slot.channelz_socket.as_ref().map(|guard| guard.id())
}

/// One-line address text for channelz subchannel names.
fn addr_text(addr: &ResolvedAddress) -> String {
    match addr {
        ResolvedAddress::Tcp(sock) => sock.to_string(),
        ResolvedAddress::Unix(path) => format!("unix:{}", path.display()),
        ResolvedAddress::UnixAbstract(name) => {
            format!("unix-abstract:@{}", String::from_utf8_lossy(name))
        }
    }
}

fn store_dialed(slot: &mut ConnSlot, dialed: Dialed) -> backend::SendRequest {
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
    /// Socket stays registered until the drain ends, so in-flight
    /// streams keep attributing to a live entity.
    channelz_socket: Option<crate::channelz::SocketHandle>,
}

/// Store `dialed`, returning the displaced connection for graceful
/// drain (instead of stopping it like [`store_dialed`]).
fn replace_for_handoff(slot: &mut ConnSlot, dialed: Dialed) -> Option<Displaced> {
    let old = match (slot.stop.take(), slot.busy.take()) {
        (Some(stop), busy) => Some(Displaced {
            stop,
            busy,
            channelz_socket: slot.channelz_socket.take(),
        }),
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
        // Hold the old socket's registration until its drain completes so
        // in-flight streams still attribute to a live socket.
        drop(old.channelz_socket);
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
        slot.channelz_socket = None;
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
                slot.channelz_socket = None;
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
    // Captured before the stream moves into the handshake: channelz
    // socket addresses.
    let local_addr = tcp
        .local_addr()
        .ok()
        .map(crate::channelz::EndpointAddr::Tcp);
    let peer_addr = tcp.peer_addr().ok().map(crate::channelz::EndpointAddr::Tcp);
    let mut dialed = match tls {
        None => finish_h2(config, tcp).await?,
        Some(tls) => {
            let tls_stream = tls.connect(tcp).await?;
            finish_h2(config, tls_stream).await.map_err(|e| {
                if e.to_string().contains("connection closed") {
                    Status::unauthenticated("tls: peer closed after handshake")
                } else {
                    e
                }
            })?
        }
    };
    dialed.local_addr = local_addr;
    dialed.peer_addr = peer_addr;
    Ok(dialed)
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
                use std::os::unix::ffi::OsStrExt as _;

                if tls.is_some() {
                    return Err(Status::invalid_argument(
                        "TLS over a Unix socket is not supported",
                    ));
                }
                // tokio has no abstract-namespace constructor; socket2
                // builds one from a leading-NUL path instead.
                let mut raw = Vec::with_capacity(name.len() + 1);
                raw.push(0u8);
                raw.extend_from_slice(&name);
                let addr = socket2::SockAddr::unix(std::path::Path::new(
                    std::ffi::OsStr::from_bytes(&raw),
                ))
                .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                let socket =
                    socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
                        .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                socket
                    .set_nonblocking(true)
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                // EINPROGRESS means "wait for writable"; anything else
                // fails fast. (`ErrorKind::InProgress` is still
                // unstable, so match the errno: 36 on every Linux arch.)
                const EINPROGRESS: i32 = 36;
                match socket.connect(&addr) {
                    Ok(()) => {}
                    Err(e) if e.raw_os_error() == Some(EINPROGRESS) => {}
                    Err(e) => {
                        return Err(Status::unavailable(format!("connect {display}: {e}")));
                    }
                }
                let std_stream: std::os::unix::net::UnixStream = socket.into();
                let io = UnixStream::from_std(std_stream)
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                io.ready(tokio::io::Interest::WRITABLE)
                    .await
                    .map_err(|e| Status::unavailable(format!("connect {display}: {e}")))?;
                // A failed nonblocking connect surfaces here, not above.
                io.peer_addr()
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

/// One A17 Watch loop for a subchannel connection: streams
/// `grpc.health.v1.Health/Watch` on the subchannel's own connection
/// and reports [`HealthSignal`] to the LB policy. Only `SERVING` is
/// healthy; `UNIMPLEMENTED` disables watching (treat as healthy);
/// other failures report unhealthy and retry with backoff, reset by
/// any received message. The loop ends silently when the
/// subconnection's stop channel fires (drain, discard, shutdown).
struct HealthWatch {
    send: backend::SendRequest,
    authority: Authority,
    https: bool,
    wire: crate::config::Wire,
    frame: crate::wire::SegFrame,
    addr: ResolvedAddress,
    policy: LbPolicy,
    stop: watch::Receiver<bool>,
    backoff_rounds: u32,
    reported: Option<HealthSignal>,
}

/// How one Watch call ended.
enum WatchEnd {
    /// Backend has no health service: report healthy once, stop.
    Disabled,
    /// Subchannel drained or shut down: clear the record, stop.
    Stopped,
    /// Retryable end: report unhealthy, back off, retry.
    Retry,
}

impl HealthWatch {
    async fn run(mut self) {
        loop {
            match self.once().await {
                WatchEnd::Disabled => {
                    self.report(HealthSignal::Healthy).await;
                    return;
                }
                WatchEnd::Stopped => {
                    self.policy.note_health_gone(&self.addr).await;
                    return;
                }
                WatchEnd::Retry => {
                    self.report(HealthSignal::Unhealthy).await;
                    let delay = transient_backoff(self.backoff_rounds);
                    self.backoff_rounds = self.backoff_rounds.saturating_add(1);
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        _ = self.stop.changed() => {
                            self.policy.note_health_gone(&self.addr).await;
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn once(&mut self) -> WatchEnd {
        if *self.stop.borrow() {
            return WatchEnd::Stopped;
        }
        let md = Metadata::new();
        let response = run_server_stream::<HealthCheckResponse>(
            self.send.clone(),
            &self.authority,
            "/grpc.health.v1.Health/Watch",
            &md,
            None,
            None,
            false,
            self.frame.clone(),
            self.stop.clone(),
            self.wire,
            crate::wire::PBRS_GRPC_UA,
            self.https,
            BytePermit::empty(),
            None,
            // Channelz: LB-internal maintenance stream, not attributed to
            // the socket's data counters.
            None,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(status) if disables_health_check(&status) => return WatchEnd::Disabled,
            Err(status) if status.code() == Code::Cancelled => return WatchEnd::Stopped,
            Err(_) => return WatchEnd::Retry,
        };
        let mut stream = response.into_inner();
        loop {
            tokio::select! {
                biased;
                _ = self.stop.changed() => return WatchEnd::Stopped,
                message = stream.message() => {
                    match message {
                        Ok(Some(body)) => {
                            // Any message resets the retry backoff.
                            self.backoff_rounds = 0;
                            self.report(signal_for(body.status())).await;
                        }
                        // A clean end is unexpected (Watch is
                        // infinite): retry like a failure.
                        Ok(None) => return WatchEnd::Retry,
                        Err(status) if disables_health_check(&status) => {
                            return WatchEnd::Disabled;
                        }
                        Err(status) if status.code() == Code::Cancelled => {
                            return WatchEnd::Stopped;
                        }
                        Err(_) => return WatchEnd::Retry,
                    }
                }
            }
        }
    }

    /// Report on change only; the first report always goes out (it
    /// clears the pending record and wakes initial waiters).
    async fn report(&mut self, signal: HealthSignal) {
        if self.reported != Some(signal) {
            self.reported = Some(signal);
            self.policy.note_health(&self.addr, signal).await;
        }
    }
}

/// One OOB ORCA loop for a subchannel connection: streams
/// `xds.service.orca.v3.OpenRcaService/StreamCoreMetrics` on the
/// subchannel's own connection and folds reports into the WRR
/// policy's weights. `UNIMPLEMENTED` stops silently (A51: never
/// retry OOB on a connection whose backend lacks the service);
/// other failures back off and retry, reset by any received
/// message. Reports are advisory: OOB failures never mark the
/// address unhealthy. The loop ends silently when the
/// subconnection's stop channel fires (drain, discard, shutdown).
struct OobWatch {
    send: backend::SendRequest,
    authority: Authority,
    https: bool,
    wire: crate::config::Wire,
    frame: crate::wire::SegFrame,
    addr: ResolvedAddress,
    policy: LbPolicy,
    stop: watch::Receiver<bool>,
    backoff_rounds: u32,
}

/// How one OOB call ended.
enum OobEnd {
    /// Backend has no OOB service: release the pump slot, stop.
    Disabled,
    /// Subchannel drained or shut down: release the slot, stop.
    Stopped,
    /// Retryable end: back off, retry.
    Retry,
}

impl OobWatch {
    async fn run(mut self) {
        loop {
            match self.once().await {
                OobEnd::Disabled | OobEnd::Stopped => {
                    self.policy.note_oob_gone(&self.addr).await;
                    return;
                }
                OobEnd::Retry => {
                    let delay = transient_backoff(self.backoff_rounds);
                    self.backoff_rounds = self.backoff_rounds.saturating_add(1);
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        _ = self.stop.changed() => {
                            self.policy.note_oob_gone(&self.addr).await;
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn once(&mut self) -> OobEnd {
        if *self.stop.borrow() {
            return OobEnd::Stopped;
        }
        let md = Metadata::new();
        let response = run_server_stream::<OrcaLoadReport>(
            self.send.clone(),
            &self.authority,
            "/xds.service.orca.v3.OpenRcaService/StreamCoreMetrics",
            &md,
            None,
            None,
            false,
            self.frame.clone(),
            self.stop.clone(),
            self.wire,
            crate::wire::PBRS_GRPC_UA,
            self.https,
            BytePermit::empty(),
            None,
            // Channelz: LB-internal maintenance stream, not attributed to
            // the socket's data counters.
            None,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(status) if status.code() == Code::Unimplemented => return OobEnd::Disabled,
            Err(status) if status.code() == Code::Cancelled => return OobEnd::Stopped,
            Err(_) => return OobEnd::Retry,
        };
        let mut stream = response.into_inner();
        loop {
            tokio::select! {
                biased;
                _ = self.stop.changed() => return OobEnd::Stopped,
                message = stream.message() => {
                    match message {
                        Ok(Some(report)) => {
                            // Any message resets the retry backoff.
                            self.backoff_rounds = 0;
                            self.policy.note_orca_report(&self.addr, &report, true).await;
                        }
                        // A clean end is unexpected (OOB is
                        // infinite): retry like a failure.
                        Ok(None) => return OobEnd::Retry,
                        Err(status) if status.code() == Code::Unimplemented => {
                            return OobEnd::Disabled;
                        }
                        Err(status) if status.code() == Code::Cancelled => {
                            return OobEnd::Stopped;
                        }
                        Err(_) => return OobEnd::Retry,
                    }
                }
            }
        }
    }
}

/// Spawn the OOB pump for a live subchannel connection, once per
/// address. No-ops unless the policy (or subset child) enables OOB,
/// the address is known, or no pump runs yet. Unlike health, OOB
/// never gates acquisition: reports are advisory and weights start
/// absent.
async fn spawn_oob_watch(
    inner: &ChannelInner,
    lb: &LbPolicy,
    addr: &ResolvedAddress,
    send: &backend::SendRequest,
    driver: &Option<watch::Sender<bool>>,
) {
    let Some(period) = lb.wants_oob().await else {
        return;
    };
    let Some(stop) = driver.as_ref().map(watch::Sender::subscribe) else {
        return;
    };
    if !lb.note_oob_started(addr).await {
        return;
    }
    let mut request = OrcaLoadReportRequest::new();
    request
        .report_interval_mut()
        .set_seconds(i64::try_from(period.as_secs()).unwrap_or(i64::MAX));
    request
        .report_interval_mut()
        .set_nanos(i32::try_from(period.subsec_nanos()).unwrap_or(i32::MAX));
    let wire = inner.dial.wire();
    let frame = match crate::wire::encode_msg(&request, None, wire.limits, wire.gzip_level) {
        Ok(frame) => frame,
        Err(_) => {
            lb.note_oob_gone(addr).await;
            return;
        }
    };
    let pump = OobWatch {
        send: send.clone(),
        authority: inner.authority.clone(),
        https: inner.tls.is_some(),
        wire,
        frame,
        addr: addr.clone(),
        policy: lb.clone(),
        stop,
        backoff_rounds: 0,
    };
    drop(tokio::spawn(pump.run()));
}

/// Ensure a Watch runs for a live subchannel connection and wait for
/// its first report (A17 CONNECTING). Returns the first signal, or
/// `None` when the connection died while waiting (caller re-picks).
/// With no directive, returns `Some(Healthy)` without watching.
async fn ensure_health_watch(
    inner: &ChannelInner,
    policy: &LbPolicy,
    addr: &ResolvedAddress,
    send: backend::SendRequest,
    stop: watch::Receiver<bool>,
    directive: Option<&HealthDirective>,
) -> Option<HealthSignal> {
    let Some(directive) = directive else {
        return Some(HealthSignal::Healthy);
    };
    if policy.note_health_pending(addr).await {
        let wire = inner.dial.wire();
        let mut request = HealthCheckRequest::new();
        request.set_service(directive.service.clone());
        let frame = match crate::wire::encode_msg(&request, None, wire.limits, wire.gzip_level) {
            Ok(frame) => frame,
            Err(_) => {
                policy.note_health_gone(addr).await;
                return None;
            }
        };
        let watch = HealthWatch {
            send,
            authority: inner.authority.clone(),
            https: inner.tls.is_some(),
            wire,
            frame,
            addr: addr.clone(),
            policy: policy.clone(),
            stop: stop.clone(),
            backoff_rounds: 0,
            reported: None,
        };
        drop(tokio::spawn(watch.run()));
    }
    wait_for_health(policy, addr, stop).await
}

/// Gate connection reuse on health: with no directive the pooled
/// connection is used as-is; otherwise ensure a Watch runs and wait
/// for its first report. Returns false when the caller must re-pick
/// (unhealthy, or the connection died while waiting).
async fn reuse_health_ok(
    inner: &ChannelInner,
    policy: &LbPolicy,
    addr: &ResolvedAddress,
    send: &backend::SendRequest,
    driver: &Option<watch::Sender<bool>>,
    directive: Option<&HealthDirective>,
) -> bool {
    let Some(directive) = directive else {
        return true;
    };
    let Some(stop) = driver.as_ref().map(watch::Sender::subscribe) else {
        return true;
    };
    matches!(
        ensure_health_watch(inner, policy, addr, send.clone(), stop, Some(directive)).await,
        Some(HealthSignal::Healthy)
    )
}

/// Ingest one attempt's ORCA report (A58 per-call): parse the
/// `endpoint-load-metrics-bin` trailer once and hand it to the LB
/// policy. No-ops without a per-address attempt, without a resolver
/// LB policy, or without a decodable report. Error statuses carry
/// their trailers in [`Status::metadata`], so failed attempts feed
/// weights too; the report's own eps/qps captures the errors.
pub(crate) async fn ingest_orca_report(
    endpoint: &Endpoint,
    addr: Option<&ResolvedAddress>,
    trailers: &Metadata,
) {
    let (Some(addr), Endpoint::Resolved { lb: Some(lb), .. }) = (addr, endpoint) else {
        return;
    };
    let Some(report) = crate::orca::report_from_trailers(trailers) else {
        return;
    };
    lb.note_orca_report(addr, &report, false).await;
}

/// Record one attempt's outcome for outlier detection (A50).
/// No-ops without a per-address attempt or without a resolver LB
/// policy. Sync and non-blocking; only outlier (sub)policies
/// consume the outcome. Unary and hedged attempts report;
/// streaming reports once streaming gains per-call hooks (the same
/// coverage as per-call ORCA today).
pub(crate) fn ingest_call_status(
    endpoint: &Endpoint,
    addr: Option<&ResolvedAddress>,
    status: &Status,
) {
    let (Some(addr), Endpoint::Resolved { lb: Some(lb), .. }) = (addr, endpoint) else {
        return;
    };
    lb.note_call_status(addr, status);
}

/// Start least-request tracking for one unary attempt (A48): an
/// RAII guard that releases the address's in-flight count on drop,
/// so every completion path — including hedged-task aborts —
/// balances. Empty (counting nothing) without a per-address
/// attempt or without a least-request (sub)policy.
pub(crate) async fn track_least_request(
    endpoint: &Endpoint,
    addr: Option<&ResolvedAddress>,
) -> crate::lb::LrTrack {
    let (Some(addr), Endpoint::Resolved { lb: Some(lb), .. }) = (addr, endpoint) else {
        return crate::lb::LrTrack::empty();
    };
    lb.track_start(addr).await
}

/// Wait for an address's first Watch report: `None` while no Watch
/// has reported, `None` (no signal) when the connection dies first.
async fn wait_for_health(
    policy: &LbPolicy,
    addr: &ResolvedAddress,
    mut stop: watch::Receiver<bool>,
) -> Option<HealthSignal> {
    if *stop.borrow() {
        return None;
    }
    let mut updates = policy.watch();
    loop {
        if let Some(signal) = policy.health_of(addr).await {
            return Some(signal);
        }
        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() {
                    return None;
                }
            }
            _ = stop.changed() => return None,
        }
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
    #[cfg(feature = "copy-counts")]
    crate::copy_counts::note_spawn(crate::copy_counts::SpawnSite::ClientConnectionDriver);
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
        local_addr: None,
        peer_addr: None,
    })
}

pub(crate) fn attach_conn<T>(
    response: crate::request::Response<Streaming<T>>,
    lease: Option<crate::keepalive::Lease>,
    driver: Option<watch::Sender<bool>>,
    reset: Option<watch::Sender<bool>>,
    rpc_slot: Option<OwnedSemaphorePermit>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> crate::request::Response<Streaming<T>> {
    response.map(|stream| {
        stream
            .bind_conn(lease, driver, reset)
            .bind_rpc_slot(rpc_slot)
            .bind_channelz_socket(channelz_socket)
    })
}

#[cfg(test)]
#[path = "cl08_future_sizes.rs"]
mod cl08_future_sizes;

#[cfg(test)]
mod tests {
    use super::{Channel, ConnSlot, RrTable, reconcile_rr};
    use crate::resolver::ResolvedAddress;
    use crate::status::Code;
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Clone)]
    struct BackoffPeer {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        retry_entered: Arc<tokio::sync::Notify>,
        release_retry: Arc<tokio::sync::Notify>,
    }

    impl BackoffPeer {
        async fn execute(
            &self,
            request: &crate::Request<crate::HelloRequest>,
        ) -> Result<(), crate::Status> {
            let execution = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if execution == 0 {
                return Err(crate::Status::unavailable("eligible rejection"));
            }
            if request.get_ref().name() == "retrying" {
                self.retry_entered.notify_one();
                self.release_retry.notified().await;
            }
            Ok(())
        }
    }

    impl crate::Greeter for BackoffPeer {
        async fn say_hello(
            &self,
            request: crate::Request<crate::HelloRequest>,
        ) -> Result<crate::Response<crate::HelloReply>, crate::Status> {
            self.execute(&request).await?;
            Ok(crate::Response::new(crate::HelloReply::new()))
        }

        async fn server_hello(
            &self,
            request: crate::Request<crate::HelloRequest>,
        ) -> Result<crate::Response<crate::Streaming<crate::HelloReply>>, crate::Status> {
            self.execute(&request).await?;
            let (tx, stream) = crate::Streaming::channel(1);
            drop(tx);
            Ok(crate::Response::new(stream))
        }
    }

    struct BackoffObserver(Arc<tokio::sync::Notify>);

    impl crate::LifecycleObserver for BackoffObserver {
        fn on_attempt_end(
            &self,
            _: &crate::AttemptLabels<'_>,
            status: &crate::Status,
            _: Duration,
        ) {
            if status.code() == Code::Unavailable {
                self.0.notify_one();
            }
        }
    }

    struct BackoffDns(std::net::SocketAddr);

    impl crate::resolver::DnsLookup for BackoffDns {
        fn lookup(
            &self,
            _: &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<std::net::SocketAddr>, std::io::Error>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(vec![self.0]) })
        }
    }

    struct BackoffTxt(&'static str);

    impl crate::resolver::TxtLookup for BackoffTxt {
        fn fetch_txt(
            &self,
            _: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<String>, std::io::Error>> + Send + '_>,
        > {
            Box::pin(async { Ok(vec![self.0.to_owned()]) })
        }
    }

    struct AbortBackoffPeer(tokio::task::JoinHandle<()>);

    impl Drop for AbortBackoffPeer {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn attempt_loads(channel: &Channel) -> (usize, usize, u64) {
        let pool = channel
            .inner
            .loads
            .iter()
            .map(|load| load.snapshot().0)
            .sum();
        let mut busy = 0;
        if let Some(table) = &channel.inner.rr {
            for slot in table.conns.lock().await.values() {
                busy += slot
                    .lock()
                    .await
                    .busy
                    .as_ref()
                    .map_or(0, |busy| busy.count());
            }
        } else {
            for slot in &channel.inner.slots {
                busy += slot
                    .lock()
                    .await
                    .busy
                    .as_ref()
                    .map_or(0, |busy| busy.count());
            }
        }
        let least_request =
            if let super::Endpoint::Resolved { lb: Some(lb), .. } = &channel.inner.endpoint {
                let crate::lb::LbPolicy::LeastRequest(policy) = lb else {
                    panic!("expected least_request")
                };
                policy
                    .loads_snapshot()
                    .await
                    .iter()
                    .map(|(_, load)| load)
                    .sum()
            } else {
                0
            };
        (pool, busy, least_request)
    }

    async fn qualify_backoff_load(server_stream: bool, resolved: bool, scheme: &'static str) {
        use std::sync::atomic::Ordering;
        for exit in ["cancel", "deadline", "retry"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("address");
            let peer = BackoffPeer {
                calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                retry_entered: Arc::new(tokio::sync::Notify::new()),
                release_retry: Arc::new(tokio::sync::Notify::new()),
            };
            let service = peer.clone();
            let _server = AbortBackoffPeer(tokio::spawn(async move {
                crate::GreeterServer::new(service)
                    .serve_listener(listener)
                    .await
                    .expect("serve");
            }));
            let config = crate::ChannelConfig::new()
                .connections(1)
                .max_concurrent_rpcs(1)
                .max_connection_idle(Duration::from_secs(30));
            let policy = r#"{"loadBalancingConfig":[{"least_request":{"choiceCount":2}}],
                "methodConfig":[{"name":[{}],"retryPolicy":{"maxAttempts":3,
                "initialBackoff":"1s","maxBackoff":"1s","backoffMultiplier":1,
                "retryableStatusCodes":["UNAVAILABLE"]}}]}"#;
            let channel = if resolved {
                let bounds = crate::resolver::DnsConfig::new(
                    Duration::from_millis(50),
                    Duration::from_secs(1),
                    Duration::from_millis(100),
                    Duration::from_millis(50),
                    Duration::from_millis(200),
                    Duration::from_secs(1),
                )
                .expect("DNS bounds");
                let resolver = crate::resolver::ResolverConfig::with_dns_provider(
                    bounds,
                    Arc::new(BackoffDns(addr)),
                )
                .with_txt_provider(Arc::new(BackoffTxt(policy)));
                Channel::connect_uri_with(&format!("dns:///{scheme}.invalid:443"), config, resolver)
                    .await
                    .unwrap_or_else(|status| panic!("resolved channel: {status}"))
            } else {
                Channel::connect_with(addr, config)
                    .await
                    .expect("channel")
                    .service_config(policy)
                    .expect("policy")
            };
            let ended = Arc::new(tokio::sync::Notify::new());
            let channel = channel
                .byte_budget(1024)
                .observer(BackoffObserver(ended.clone()));
            let mut message = crate::HelloRequest::new();
            message.set_name("retrying");
            let mut request = crate::Request::new(message);
            request.set_timeout(if exit == "deadline" {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(5)
            });
            let (handle, task) = if server_stream {
                let call = channel.server_streaming::<_, crate::HelloReply>(
                    "/helloworld.Greeter/ServerHello",
                    request,
                );
                (
                    call.handle(),
                    tokio::spawn(
                        async move { call.await.map(|response| Some(response.into_inner())) },
                    ),
                )
            } else {
                let call =
                    channel.unary::<_, crate::HelloReply>("/helloworld.Greeter/SayHello", request);
                (
                    call.handle(),
                    tokio::spawn(async move { call.await.map(|_| None) }),
                )
            };
            tokio::time::timeout(Duration::from_secs(2), ended.notified())
                .await
                .expect("first attempt ended");
            assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                attempt_loads(&channel).await,
                (0, 0, 0),
                "{scheme}, {exit}: completed attempt must release routing and connection load before backoff"
            );
            assert_eq!(channel.byte_budget_allocated(), 0);
            let blocked = channel
                .unary::<_, crate::HelloReply>(
                    "/helloworld.Greeter/SayHello",
                    crate::Request::new(crate::HelloRequest::new()),
                )
                .await
                .expect_err("backoff retains RPC-level admission");
            assert_eq!(blocked.code(), Code::ResourceExhausted);
            if exit == "cancel" {
                handle.cancel();
            }
            if exit == "retry" {
                tokio::time::timeout(Duration::from_secs(2), peer.retry_entered.notified())
                    .await
                    .expect("retry entered handler");
                assert_eq!(
                    attempt_loads(&channel).await,
                    if resolved {
                        (0, 1, u64::from(!server_stream))
                    } else {
                        (1, 1, 0)
                    },
                    "next attempt reacquires load"
                );
                peer.release_retry.notify_one();
            }
            let result = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("call ended")
                .expect("call task");
            if exit == "retry" {
                if let Some(mut stream) = result.expect("retry success") {
                    assert_eq!(
                        attempt_loads(&channel).await.1,
                        1,
                        "successful stream owns connection lease"
                    );
                    assert!(stream.message().await.expect("EOF").is_none());
                    drop(stream);
                }
                assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
            } else {
                assert_eq!(
                    result.expect_err("terminal signal").code(),
                    if exit == "cancel" {
                        Code::Cancelled
                    } else {
                        Code::DeadlineExceeded
                    }
                );
                assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
            }
            assert_eq!(attempt_loads(&channel).await, (0, 0, 0));
            channel
                .unary::<_, crate::HelloReply>(
                    "/helloworld.Greeter/SayHello",
                    crate::Request::new(crate::HelloRequest::new()),
                )
                .await
                .expect("follow-up after admission release");
            assert_eq!(attempt_loads(&channel).await, (0, 0, 0));
            assert_eq!(channel.byte_budget_allocated(), 0);
        }
    }

    #[tokio::test]
    async fn unary_backoff_releases_pool_load_and_connection_lease() {
        qualify_backoff_load(false, false, "qgpoolunary").await;
    }

    #[tokio::test]
    async fn server_stream_backoff_releases_pool_load_and_connection_lease() {
        qualify_backoff_load(true, false, "qgpoolstream").await;
    }

    #[tokio::test]
    async fn unary_backoff_releases_least_request_and_connection_lease() {
        qualify_backoff_load(false, true, "qglrunary").await;
    }

    #[tokio::test]
    async fn server_stream_backoff_releases_resolved_connection_lease() {
        qualify_backoff_load(true, true, "qglrstream").await;
    }

    #[tokio::test]
    async fn per_core_channels_own_independent_pools() {
        let channels = Channel::connect_per_core("127.0.0.1:1", 4).expect("channels");
        assert_eq!(channels.len(), 4);
        for pair in channels.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                !Arc::ptr_eq(&a.inner, &b.inner),
                "per-core channels must not share a pool"
            );
        }
        let err = Channel::connect_per_core("127.0.0.1:1", 0).expect_err("zero cores");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    #[tokio::test]
    async fn reconcile_drops_removed_and_stops_live_conns() {
        let table = RrTable::new();
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        {
            let mut conns = table.conns.lock().await;
            conns.insert(
                tcp(1),
                Arc::new(tokio::sync::Mutex::new(ConnSlot {
                    r#gen: 3,
                    send: None,
                    stop: Some(stop_tx),
                    busy: None,
                    address: Some(tcp(1)),
                    channelz_socket: None,
                    channelz_subchannel: None,
                })),
            );
            conns.insert(
                tcp(2),
                Arc::new(tokio::sync::Mutex::new(ConnSlot {
                    r#gen: 0,
                    send: None,
                    stop: None,
                    busy: None,
                    address: Some(tcp(2)),
                    channelz_socket: None,
                    channelz_subchannel: None,
                })),
            );
        }
        reconcile_rr(&table, &[tcp(2)], Duration::from_millis(1)).await;
        // Removed entry is gone; the survivor is untouched.
        let conns = table.conns.lock().await;
        assert!(!conns.contains_key(&tcp(1)));
        assert!(conns.contains_key(&tcp(2)));
        drop(conns);
        // The live connection's driver was stopped: no leak.
        tokio::time::timeout(Duration::from_secs(5), stop_rx.changed())
            .await
            .expect("stop arrives")
            .expect("sender alive");
        assert!(*stop_rx.borrow());
    }

    #[tokio::test]
    async fn reconcile_ignores_never_connected_entries() {
        let table = RrTable::new();
        {
            let mut conns = table.conns.lock().await;
            conns.insert(
                tcp(9),
                Arc::new(tokio::sync::Mutex::new(ConnSlot {
                    r#gen: 0,
                    send: None,
                    stop: None,
                    busy: None,
                    address: Some(tcp(9)),
                    channelz_socket: None,
                    channelz_subchannel: None,
                })),
            );
        }
        reconcile_rr(&table, &[], Duration::from_millis(1)).await;
        assert!(table.conns.lock().await.is_empty());
    }
}
