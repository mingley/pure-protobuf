//! A14 entity registry: channels, subchannels, servers, sockets.
//!
//! [`Registry`] hands out process-unique ordered IDs (never reused,
//! never zero) and owns one entry per live entity. Owners hold RAII
//! guards ([`ChannelHandle`], [`SubchannelHandle`], [`ServerHandle`],
//! [`SocketHandle`]); dropping a guard unregisters its entity and
//! detaches it from its parent, so memory is `O(live entities)` plus
//! one bounded [`Trace`](super::Trace) each — flat
//! under churn by construction.
//!
//! Hot paths (call/stream/message counters) are lock-free atomics;
//! timestamps are millis-since-epoch atomics (0 = unset). Registry
//! maps sit behind a std `RwLock`: lookups clone the entry `Arc`
//! under a read lock and touch it after release, so record calls
//! never hold map locks while touching entries. All methods are
//! sync; the async service layer snapshots above them.
//!
//! The entity graph is flat: a channel owns subchannels (one per
//! resolved address with dial activity on resolver-LB channels) or
//! sockets directly (pick_first and direct channels, whose slots are
//! not per-address); a server owns listen sockets, and accepted
//! sockets hang under their server. LB-policy nesting is not
//! modeled. A missing id on any record call is a silent no-op, so
//! stale handles from a previous generation can never corrupt a new
//! entity.

use super::trace::{Trace, TraceChild, TraceSeverity};
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex, OnceLock, RwLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Channel entity id. Positive, never reused within a registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(u64);

/// Subchannel entity id. Positive, never reused within a registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubchannelId(u64);

/// Server entity id. Positive, never reused within a registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerId(u64);

/// Socket entity id. Positive, never reused within a registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SocketId(u64);

macro_rules! entity_id {
    ($name:ident) => {
        impl $name {
            /// The raw id for proto conversion.
            #[must_use]
            pub fn get(self) -> u64 {
                self.0
            }
        }
    };
}

entity_id!(ChannelId);
entity_id!(SubchannelId);
entity_id!(ServerId);
entity_id!(SocketId);

/// Connectivity state, mirroring the proto enum one-to-one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connectivity {
    /// Unknown (proto default; never set, only read back).
    #[default]
    Unknown,
    /// Created, nothing dialed yet.
    Idle,
    /// A dial is in flight.
    Connecting,
    /// At least one connection is live.
    Ready,
    /// Last dial failed and nothing is live.
    TransientFailure,
    /// Closed. Entities unregister instead of lingering here, so
    /// this only appears during the drop itself.
    Shutdown,
}

/// A dialable endpoint for socket address conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndpointAddr {
    /// TCP address (v4 or v6).
    Tcp(std::net::SocketAddr),
    /// Unix-domain socket path, or `@name` for abstract names.
    Uds(String),
    /// Anything else, as human-readable text.
    Other(String),
}

/// Security posture of a socket. Certificates are empty when the
/// handshake did not retain them (unknown, not absent encryption).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SocketSecurity {
    /// Plaintext (or unknown transport): no `Security` emitted.
    #[default]
    None,
    /// TLS session.
    Tls {
        /// Local certificate bytes, empty when not retained.
        local_certificate: Vec<u8>,
        /// Remote certificate bytes, empty when not retained.
        remote_certificate: Vec<u8>,
    },
}

/// What a socket hangs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketParent {
    /// A leaf channel (pick_first, direct).
    Channel(ChannelId),
    /// A per-address subchannel.
    Subchannel(SubchannelId),
    /// A server (accepted or listen socket).
    Server(ServerId),
}

/// Live channel entry. Counters are atomics; child lists and state
/// change rarely and sit behind small mutexes.
#[derive(Debug)]
pub(crate) struct ChannelEntry {
    pub id: ChannelId,
    pub name: String,
    pub target: String,
    state: Mutex<Connectivity>,
    calls_started: AtomicU64,
    calls_succeeded: AtomicU64,
    calls_failed: AtomicU64,
    last_call_started_ms: AtomicU64,
    trace: Trace,
    subchannels: Mutex<Vec<(SubchannelId, String)>>,
    sockets: Mutex<Vec<(SocketId, String)>>,
}

/// Live subchannel entry: one per resolved address with dial
/// activity. Call counters count attempts routed through it (fed by
/// socket stream events, one stream per attempt).
#[derive(Debug)]
pub(crate) struct SubchannelEntry {
    pub id: SubchannelId,
    pub parent: ChannelId,
    pub name: String,
    state: Mutex<Connectivity>,
    calls_started: AtomicU64,
    calls_succeeded: AtomicU64,
    calls_failed: AtomicU64,
    last_call_started_ms: AtomicU64,
    trace: Trace,
    sockets: Mutex<Vec<(SocketId, String)>>,
}

/// Live server entry.
#[derive(Debug)]
pub(crate) struct ServerEntry {
    pub id: ServerId,
    pub name: String,
    calls_started: AtomicU64,
    calls_succeeded: AtomicU64,
    calls_failed: AtomicU64,
    last_call_started_ms: AtomicU64,
    trace: Trace,
    /// (id, name, is_listen): listen sockets plus accepted connections.
    listen_sockets: Mutex<Vec<(SocketId, String, bool)>>,
}

/// Live socket entry: one per connection (or listen socket).
#[derive(Debug)]
pub(crate) struct SocketEntry {
    pub id: SocketId,
    pub parent: SocketParent,
    pub name: String,
    /// Locally bound address, when the transport reports one.
    pub local: Option<EndpointAddr>,
    pub remote: Option<EndpointAddr>,
    pub remote_name: Option<String>,
    pub security: SocketSecurity,
    streams_started: AtomicU64,
    streams_succeeded: AtomicU64,
    streams_failed: AtomicU64,
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    keep_alives_sent: AtomicU64,
    last_local_stream_ms: AtomicU64,
    last_remote_stream_ms: AtomicU64,
    last_message_sent_ms: AtomicU64,
    last_message_received_ms: AtomicU64,
}

/// Entity registry. See the module docs for the memory and locking contracts.
#[derive(Debug, Default)]
pub struct Registry {
    next_id: AtomicU64,
    channels: RwLock<BTreeMap<u64, Arc<ChannelEntry>>>,
    subchannels: RwLock<BTreeMap<u64, Arc<SubchannelEntry>>>,
    servers: RwLock<BTreeMap<u64, Arc<ServerEntry>>>,
    sockets: RwLock<BTreeMap<u64, Arc<SocketEntry>>>,
}

impl Registry {
    /// New empty registry. Hookups use [`Self::global`]; tests and
    /// embedding use explicit registries.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            channels: RwLock::new(BTreeMap::new()),
            subchannels: RwLock::new(BTreeMap::new()),
            servers: RwLock::new(BTreeMap::new()),
            sockets: RwLock::new(BTreeMap::new()),
        }
    }

    /// Process-global registry backing every channelz hookup.
    pub fn global() -> &'static Registry {
        static GLOBAL: OnceLock<Registry> = OnceLock::new();
        GLOBAL.get_or_init(Self::new)
    }

    /// Process-global registry as shared ownership (for the service).
    #[must_use]
    pub fn global_shared() -> Arc<Registry> {
        static GLOBAL: OnceLock<Arc<Registry>> = OnceLock::new();
        Arc::clone(GLOBAL.get_or_init(|| Arc::new(Self::new())))
    }

    fn alloc(&self) -> u64 {
        // Starts at 1; 0 stays invalid per A14. Wrapping is a
        // non-event (2^64 ids per process lifetime).
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a top-level channel. Traces creation (A3).
    pub fn register_channel(
        self: &Arc<Self>,
        name: impl Into<String>,
        target: impl Into<String>,
    ) -> ChannelHandle {
        let name = name.into();
        let id = ChannelId(self.alloc());
        let entry = Arc::new(ChannelEntry {
            id,
            name: name.clone(),
            target: target.into(),
            state: Mutex::new(Connectivity::Idle),
            calls_started: AtomicU64::new(0),
            calls_succeeded: AtomicU64::new(0),
            calls_failed: AtomicU64::new(0),
            last_call_started_ms: AtomicU64::new(0),
            trace: Trace::new(),
            subchannels: Mutex::new(Vec::new()),
            sockets: Mutex::new(Vec::new()),
        });
        entry.trace.info(format!("Channel created: {name}"));
        if let Ok(mut channels) = self.channels.write() {
            channels.insert(id.0, Arc::clone(&entry));
        }
        ChannelHandle {
            registry: Arc::clone(self),
            id,
        }
    }

    /// Register a subchannel under `channel`. Traces creation on the
    /// subchannel and on the parent with a child ref (A3). Missing
    /// parents still register (orphan); the ref attach is skipped.
    pub fn register_subchannel(
        self: &Arc<Self>,
        channel: ChannelId,
        name: impl Into<String>,
    ) -> SubchannelHandle {
        let name = name.into();
        let id = SubchannelId(self.alloc());
        let entry = Arc::new(SubchannelEntry {
            id,
            parent: channel,
            name: name.clone(),
            state: Mutex::new(Connectivity::Idle),
            calls_started: AtomicU64::new(0),
            calls_succeeded: AtomicU64::new(0),
            calls_failed: AtomicU64::new(0),
            last_call_started_ms: AtomicU64::new(0),
            trace: Trace::new(),
            sockets: Mutex::new(Vec::new()),
        });
        entry.trace.info(format!("Subchannel created: {name}"));
        if let Ok(mut subchannels) = self.subchannels.write() {
            subchannels.insert(id.0, Arc::clone(&entry));
        }
        if let Some(parent) = self.channel(channel) {
            if let Ok(mut children) = parent.subchannels.lock() {
                children.push((id, name.clone()));
            }
            parent.trace.log(
                TraceSeverity::Info,
                format!("Subchannel created: {name}"),
                Some(TraceChild::Subchannel { id: id.0, name }),
            );
        }
        SubchannelHandle {
            registry: Arc::clone(self),
            id,
        }
    }

    /// Register a server. Traces creation (A3).
    pub fn register_server(self: &Arc<Self>, name: impl Into<String>) -> ServerHandle {
        let name = name.into();
        let id = ServerId(self.alloc());
        let entry = Arc::new(ServerEntry {
            id,
            name: name.clone(),
            calls_started: AtomicU64::new(0),
            calls_succeeded: AtomicU64::new(0),
            calls_failed: AtomicU64::new(0),
            last_call_started_ms: AtomicU64::new(0),
            trace: Trace::new(),
            listen_sockets: Mutex::new(Vec::new()),
        });
        entry.trace.info(format!("Server created: {name}"));
        if let Ok(mut servers) = self.servers.write() {
            servers.insert(id.0, Arc::clone(&entry));
        }
        ServerHandle {
            registry: Arc::clone(self),
            id,
        }
    }

    /// Register a socket. Attaches to its parent and traces the
    /// establishment on the parent (A3's TCP-connection event).
    /// Missing parents still register (orphan).
    #[allow(clippy::too_many_arguments, reason = "socket identity is one call")]
    pub fn register_socket(
        self: &Arc<Self>,
        parent: SocketParent,
        local: Option<EndpointAddr>,
        remote: Option<EndpointAddr>,
        remote_name: Option<String>,
        security: SocketSecurity,
        is_listen: bool,
    ) -> SocketHandle {
        let id = SocketId(self.alloc());
        let name = format!(
            "{}→{}",
            local
                .as_ref()
                .map(addr_text)
                .unwrap_or_else(|| "?".to_owned()),
            remote
                .as_ref()
                .map(addr_text)
                .unwrap_or_else(|| "-".to_owned())
        );
        let entry = Arc::new(SocketEntry {
            id,
            parent,
            name: name.clone(),
            local,
            remote,
            remote_name,
            security,
            streams_started: AtomicU64::new(0),
            streams_succeeded: AtomicU64::new(0),
            streams_failed: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            keep_alives_sent: AtomicU64::new(0),
            last_local_stream_ms: AtomicU64::new(0),
            last_remote_stream_ms: AtomicU64::new(0),
            last_message_sent_ms: AtomicU64::new(0),
            last_message_received_ms: AtomicU64::new(0),
        });
        if let Ok(mut sockets) = self.sockets.write() {
            sockets.insert(id.0, Arc::clone(&entry));
        }
        let what = if is_listen { "Listen socket" } else { "Socket" };
        self.attach_socket(
            parent,
            id,
            &name,
            is_listen,
            &format!("{what} established: {name}"),
        );
        SocketHandle {
            registry: Arc::clone(self),
            id,
        }
    }

    /// Attach a socket ref to its parent plus a trace line. Shared
    /// by registration; detach mirrors it.
    fn attach_socket(
        &self,
        parent: SocketParent,
        id: SocketId,
        name: &str,
        is_listen: bool,
        trace: &str,
    ) {
        match parent {
            SocketParent::Channel(channel) => {
                if let Some(entry) = self.channel(channel) {
                    if let Ok(mut sockets) = entry.sockets.lock() {
                        sockets.push((id, name.to_owned()));
                    }
                    entry.trace.info(trace.to_owned());
                }
            }
            SocketParent::Subchannel(subchannel) => {
                if let Some(entry) = self.subchannel(subchannel) {
                    if let Ok(mut sockets) = entry.sockets.lock() {
                        sockets.push((id, name.to_owned()));
                    }
                    entry.trace.info(trace.to_owned());
                }
            }
            SocketParent::Server(server) => {
                if let Some(entry) = self.server(server) {
                    if let Ok(mut sockets) = entry.listen_sockets.lock() {
                        sockets.push((id, name.to_owned(), is_listen));
                    }
                    entry.trace.info(trace.to_owned());
                }
            }
        }
    }

    /// Look up a channel entry. Clones under a read lock; entry
    /// touched after release.
    pub(crate) fn channel(&self, id: ChannelId) -> Option<Arc<ChannelEntry>> {
        self.channels.read().ok()?.get(&id.0).cloned()
    }

    /// Look up a subchannel entry.
    pub(crate) fn subchannel(&self, id: SubchannelId) -> Option<Arc<SubchannelEntry>> {
        self.subchannels.read().ok()?.get(&id.0).cloned()
    }

    /// Look up a server entry.
    pub(crate) fn server(&self, id: ServerId) -> Option<Arc<ServerEntry>> {
        self.servers.read().ok()?.get(&id.0).cloned()
    }

    /// Look up a socket entry.
    pub(crate) fn socket(&self, id: SocketId) -> Option<Arc<SocketEntry>> {
        self.sockets.read().ok()?.get(&id.0).cloned()
    }

    /// Unregister a channel and detach it. Sockets hanging directly
    /// under it go too (their owners dropped with the channel);
    /// subchannels are owned by their own guards and stay until
    /// those drop (orphaned refs resolve as missing, never wrong).
    pub(crate) fn unregister_channel(&self, id: ChannelId) {
        let entry = self
            .channels
            .write()
            .ok()
            .and_then(|mut map| map.remove(&id.0));
        let Some(entry) = entry else { return };
        let sockets = entry
            .sockets
            .lock()
            .ok()
            .map(|sockets| sockets.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        for socket in sockets.into_iter().flatten() {
            self.unregister_socket(socket);
        }
    }

    /// Unregister a subchannel: detach from its parent (tracing
    /// deletion with a child ref, A3) and drop its sockets.
    pub(crate) fn unregister_subchannel(&self, id: SubchannelId) {
        let entry = self
            .subchannels
            .write()
            .ok()
            .and_then(|mut map| map.remove(&id.0));
        let Some(entry) = entry else { return };
        if let Some(parent) = self.channel(entry.parent) {
            if let Ok(mut children) = parent.subchannels.lock() {
                children.retain(|(known, _)| *known != id);
            }
            parent.trace.log(
                TraceSeverity::Info,
                format!("Subchannel deleted: {}", entry.name),
                Some(TraceChild::Subchannel {
                    id: id.0,
                    name: entry.name.clone(),
                }),
            );
        }
        let sockets = entry
            .sockets
            .lock()
            .ok()
            .map(|sockets| sockets.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        for socket in sockets.into_iter().flatten() {
            self.unregister_socket(socket);
        }
    }

    /// Unregister a server and drop its sockets.
    pub(crate) fn unregister_server(&self, id: ServerId) {
        let entry = self
            .servers
            .write()
            .ok()
            .and_then(|mut map| map.remove(&id.0));
        let Some(entry) = entry else { return };
        let sockets = entry
            .listen_sockets
            .lock()
            .ok()
            .map(|sockets| sockets.iter().map(|(id, _, _)| *id).collect::<Vec<_>>());
        for socket in sockets.into_iter().flatten() {
            self.unregister_socket(socket);
        }
    }

    /// Unregister a socket and detach it from its parent, tracing
    /// the close on the parent.
    pub(crate) fn unregister_socket(&self, id: SocketId) {
        let entry = self
            .sockets
            .write()
            .ok()
            .and_then(|mut map| map.remove(&id.0));
        let Some(entry) = entry else { return };
        let trace = format!("Socket closed: {}", entry.name);
        match entry.parent {
            SocketParent::Channel(channel) => {
                if let Some(parent) = self.channel(channel) {
                    if let Ok(mut sockets) = parent.sockets.lock() {
                        sockets.retain(|(known, _)| *known != id);
                    }
                    parent.trace.info(trace);
                }
            }
            SocketParent::Subchannel(subchannel) => {
                if let Some(parent) = self.subchannel(subchannel) {
                    if let Ok(mut sockets) = parent.sockets.lock() {
                        sockets.retain(|(known, _)| *known != id);
                    }
                    parent.trace.info(trace);
                }
            }
            SocketParent::Server(server) => {
                if let Some(parent) = self.server(server) {
                    if let Ok(mut sockets) = parent.listen_sockets.lock() {
                        sockets.retain(|(known, _, _)| *known != id);
                    }
                    parent.trace.info(trace);
                }
            }
        }
    }

    /// Set channel state, tracing transitions (A3). No-op when
    /// unchanged or missing.
    pub fn set_channel_state(&self, id: ChannelId, state: Connectivity) {
        let Some(entry) = self.channel(id) else {
            return;
        };
        let Ok(mut current) = entry.state.lock() else {
            return;
        };
        if *current == state {
            return;
        }
        *current = state;
        entry
            .trace
            .info(format!("Connectivity state change to {state:?}"));
    }

    /// Set subchannel state, tracing transitions (A3), then
    /// re-derive the parent channel state from its subchannels: any
    /// READY wins, else any CONNECTING, else any TRANSIENT_FAILURE,
    /// else the parent keeps its explicitly set state (channels
    /// without subchannels are set directly by their hookups).
    pub fn set_subchannel_state(&self, id: SubchannelId, state: Connectivity) {
        let Some(entry) = self.subchannel(id) else {
            return;
        };
        let changed = {
            let Ok(mut current) = entry.state.lock() else {
                return;
            };
            if *current == state {
                false
            } else {
                *current = state;
                entry
                    .trace
                    .info(format!("Connectivity state change to {state:?}"));
                true
            }
        };
        if changed {
            self.derive_channel_state(entry.parent);
        }
    }

    /// Re-derive a channel's state from its live subchannels.
    fn derive_channel_state(&self, id: ChannelId) {
        let Some(entry) = self.channel(id) else {
            return;
        };
        let children = entry
            .subchannels
            .lock()
            .ok()
            .map(|children| children.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        let mut aggregate: Option<Connectivity> = None;
        for child in children.into_iter().flatten() {
            let Some(sub) = self.subchannel(child) else {
                continue;
            };
            let state = sub.state.lock().ok().map(|state| *state);
            aggregate = Some(match (aggregate, state) {
                (_, Some(Connectivity::Ready)) | (Some(Connectivity::Ready), _) => {
                    Connectivity::Ready
                }
                (_, Some(Connectivity::Connecting)) | (Some(Connectivity::Connecting), _) => {
                    Connectivity::Connecting
                }
                (_, Some(Connectivity::TransientFailure))
                | (Some(Connectivity::TransientFailure), _) => Connectivity::TransientFailure,
                (known, _) => known.unwrap_or(Connectivity::Idle),
            });
            if aggregate == Some(Connectivity::Ready) {
                break;
            }
        }
        if let Some(state) = aggregate {
            self.set_channel_state(id, state);
        }
    }

    /// Trace a line on a channel. Missing ids are silent no-ops.
    pub fn trace_channel(
        &self,
        id: ChannelId,
        severity: TraceSeverity,
        description: impl Into<String>,
    ) {
        if let Some(entry) = self.channel(id) {
            entry.trace.log(severity, description, None);
        }
    }

    /// Trace a line with a child ref on a channel.
    pub fn trace_channel_child(
        &self,
        id: ChannelId,
        severity: TraceSeverity,
        description: impl Into<String>,
        child: TraceChild,
    ) {
        if let Some(entry) = self.channel(id) {
            entry.trace.log(severity, description, Some(child));
        }
    }

    /// Trace a line on a subchannel.
    pub fn trace_subchannel(
        &self,
        id: SubchannelId,
        severity: TraceSeverity,
        description: impl Into<String>,
    ) {
        if let Some(entry) = self.subchannel(id) {
            entry.trace.log(severity, description, None);
        }
    }

    /// Trace a line on a server.
    pub fn trace_server(
        &self,
        id: ServerId,
        severity: TraceSeverity,
        description: impl Into<String>,
    ) {
        if let Some(entry) = self.server(id) {
            entry.trace.log(severity, description, None);
        }
    }

    /// Count one call started on a channel (lock-free).
    pub fn note_call_started(&self, id: ChannelId) {
        if let Some(entry) = self.channel(id) {
            entry.calls_started.fetch_add(1, Ordering::Relaxed);
            entry
                .last_call_started_ms
                .store(now_ms(), Ordering::Relaxed);
        }
    }

    /// Count one call completed on a channel (lock-free).
    pub fn note_call_end(&self, id: ChannelId, ok: bool) {
        if let Some(entry) = self.channel(id) {
            if ok {
                entry.calls_succeeded.fetch_add(1, Ordering::Relaxed);
            } else {
                entry.calls_failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Count one call started on a server (lock-free).
    pub fn note_server_call_started(&self, id: ServerId) {
        if let Some(entry) = self.server(id) {
            entry.calls_started.fetch_add(1, Ordering::Relaxed);
            entry
                .last_call_started_ms
                .store(now_ms(), Ordering::Relaxed);
        }
    }

    /// Count one call completed on a server (lock-free).
    pub fn note_server_call_end(&self, id: ServerId, ok: bool) {
        if let Some(entry) = self.server(id) {
            if ok {
                entry.calls_succeeded.fetch_add(1, Ordering::Relaxed);
            } else {
                entry.calls_failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Count one stream started on a socket, cascading to the parent
    /// subchannel's call counters when there is one (each attempt is
    /// one stream). `local` marks locally-initiated streams.
    pub fn note_stream_started(&self, id: SocketId, local: bool) {
        let Some(entry) = self.socket(id) else { return };
        entry.streams_started.fetch_add(1, Ordering::Relaxed);
        if local {
            entry
                .last_local_stream_ms
                .store(now_ms(), Ordering::Relaxed);
        } else {
            entry
                .last_remote_stream_ms
                .store(now_ms(), Ordering::Relaxed);
        }
        if let SocketParent::Subchannel(parent) = entry.parent {
            if let Some(sub) = self.subchannel(parent) {
                sub.calls_started.fetch_add(1, Ordering::Relaxed);
                sub.last_call_started_ms.store(now_ms(), Ordering::Relaxed);
            }
        }
    }

    /// Count one stream completed on a socket, cascading to the
    /// parent subchannel's call counters when there is one.
    pub fn note_stream_end(&self, id: SocketId, ok: bool) {
        let Some(entry) = self.socket(id) else { return };
        if ok {
            entry.streams_succeeded.fetch_add(1, Ordering::Relaxed);
        } else {
            entry.streams_failed.fetch_add(1, Ordering::Relaxed);
        }
        if let SocketParent::Subchannel(parent) = entry.parent {
            if let Some(sub) = self.subchannel(parent) {
                if ok {
                    sub.calls_succeeded.fetch_add(1, Ordering::Relaxed);
                } else {
                    sub.calls_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Count messages on a socket (lock-free). `sent` selects the
    /// direction; `count` is usually 1.
    pub fn note_messages(&self, id: SocketId, sent: bool, count: u64) {
        let Some(entry) = self.socket(id) else { return };
        if sent {
            entry.messages_sent.fetch_add(count, Ordering::Relaxed);
            entry
                .last_message_sent_ms
                .store(now_ms(), Ordering::Relaxed);
        } else {
            entry.messages_received.fetch_add(count, Ordering::Relaxed);
            entry
                .last_message_received_ms
                .store(now_ms(), Ordering::Relaxed);
        }
    }

    /// Count one keepalive sent on a socket (lock-free).
    pub fn note_keepalive(&self, id: SocketId) {
        if let Some(entry) = self.socket(id) {
            entry.keep_alives_sent.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Live entity counts, for bound tests and debugging.
    #[must_use]
    pub fn len(&self) -> EntityCounts {
        EntityCounts {
            channels: self.channels.read().ok().map_or(0, |map| map.len()),
            subchannels: self.subchannels.read().ok().map_or(0, |map| map.len()),
            servers: self.servers.read().ok().map_or(0, |map| map.len()),
            sockets: self.sockets.read().ok().map_or(0, |map| map.len()),
        }
    }

    /// Whether the registry holds no entities.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == EntityCounts::default()
    }
}

/// One page of entities plus whether the scan reached the end.
#[derive(Debug)]
pub(crate) struct Page<T> {
    /// Page items in ascending id order.
    pub items: Vec<Arc<T>>,
    /// True when no further ids exist past this page.
    pub end: bool,
}

impl Registry {
    /// Channels with id at or above `start`, ascending, capped at `max`.
    pub(crate) fn top_channels(&self, start: i64, max: usize) -> Page<ChannelEntry> {
        let start = u64::try_from(start.max(0)).unwrap_or(u64::MAX);
        let mut items = Vec::new();
        let mut end = true;
        if let Ok(channels) = self.channels.read() {
            for (_, entry) in channels.range(start..) {
                if items.len() >= max {
                    end = false;
                    break;
                }
                items.push(Arc::clone(entry));
            }
        }
        Page { items, end }
    }

    /// Servers with id at or above `start`, ascending, capped at `max`.
    pub(crate) fn servers(&self, start: i64, max: usize) -> Page<ServerEntry> {
        let start = u64::try_from(start.max(0)).unwrap_or(u64::MAX);
        let mut items = Vec::new();
        let mut end = true;
        if let Ok(servers) = self.servers.read() {
            for (_, entry) in servers.range(start..) {
                if items.len() >= max {
                    end = false;
                    break;
                }
                items.push(Arc::clone(entry));
            }
        }
        Page { items, end }
    }

    /// A server's sockets (listen + accepted) with id at or above
    /// `start`, ascending, capped at `max`. `None` when the server is
    /// missing.
    pub(crate) fn server_sockets(
        &self,
        server: i64,
        start: i64,
        max: usize,
    ) -> Option<(Vec<(SocketId, String)>, bool)> {
        let entry = self.server_by_wire_id(server)?;
        let mut children = entry.socket_children();
        children.sort();
        let start = u64::try_from(start.max(0)).unwrap_or(u64::MAX);
        let mut items = Vec::new();
        let mut end = true;
        for (id, name) in children.into_iter().filter(|(id, _)| id.0 >= start) {
            if items.len() >= max {
                end = false;
                break;
            }
            items.push((id, name));
        }
        Some((items, end))
    }

    /// Look up a channel by wire id. Non-positive ids never match.
    pub(crate) fn channel_by_wire_id(&self, id: i64) -> Option<Arc<ChannelEntry>> {
        self.channel(ChannelId(u64::try_from(id).ok()?))
    }

    /// Look up a subchannel by wire id. Non-positive ids never match.
    pub(crate) fn subchannel_by_wire_id(&self, id: i64) -> Option<Arc<SubchannelEntry>> {
        self.subchannel(SubchannelId(u64::try_from(id).ok()?))
    }

    /// Look up a server by wire id. Non-positive ids never match.
    pub(crate) fn server_by_wire_id(&self, id: i64) -> Option<Arc<ServerEntry>> {
        self.server(ServerId(u64::try_from(id).ok()?))
    }

    /// Look up a socket by wire id. Non-positive ids never match.
    pub(crate) fn socket_by_wire_id(&self, id: i64) -> Option<Arc<SocketEntry>> {
        self.socket(SocketId(u64::try_from(id).ok()?))
    }
}

/// Snapshot accessors for the service layer. Each locks at most one
/// entry mutex; the caller must not hold a registry map lock (all
/// lookups clone first).
impl ChannelEntry {
    pub(crate) fn state(&self) -> Connectivity {
        self.state
            .lock()
            .ok()
            .map(|state| *state)
            .unwrap_or_default()
    }

    pub(crate) fn trace_snapshot(&self) -> super::trace::TraceSnapshot {
        self.trace.snapshot()
    }

    pub(crate) fn calls_started(&self) -> u64 {
        self.calls_started.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_succeeded(&self) -> u64 {
        self.calls_succeeded.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_failed(&self) -> u64 {
        self.calls_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn last_call_started_ms(&self) -> u64 {
        self.last_call_started_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn subchannel_children(&self) -> Vec<(SubchannelId, String)> {
        self.subchannels
            .lock()
            .ok()
            .map(|children| children.clone())
            .unwrap_or_default()
    }

    pub(crate) fn socket_children(&self) -> Vec<(SocketId, String)> {
        self.sockets
            .lock()
            .ok()
            .map(|sockets| sockets.clone())
            .unwrap_or_default()
    }
}

impl SubchannelEntry {
    pub(crate) fn state(&self) -> Connectivity {
        self.state
            .lock()
            .ok()
            .map(|state| *state)
            .unwrap_or_default()
    }

    pub(crate) fn trace_snapshot(&self) -> super::trace::TraceSnapshot {
        self.trace.snapshot()
    }

    pub(crate) fn calls_started(&self) -> u64 {
        self.calls_started.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_succeeded(&self) -> u64 {
        self.calls_succeeded.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_failed(&self) -> u64 {
        self.calls_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn last_call_started_ms(&self) -> u64 {
        self.last_call_started_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn socket_children(&self) -> Vec<(SocketId, String)> {
        self.sockets
            .lock()
            .ok()
            .map(|sockets| sockets.clone())
            .unwrap_or_default()
    }
}

impl ServerEntry {
    pub(crate) fn trace_snapshot(&self) -> super::trace::TraceSnapshot {
        self.trace.snapshot()
    }

    pub(crate) fn calls_started(&self) -> u64 {
        self.calls_started.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_succeeded(&self) -> u64 {
        self.calls_succeeded.load(Ordering::Relaxed)
    }

    pub(crate) fn calls_failed(&self) -> u64 {
        self.calls_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn last_call_started_ms(&self) -> u64 {
        self.last_call_started_ms.load(Ordering::Relaxed)
    }

    /// Listen sockets only (for `Server.listen_socket`).
    pub(crate) fn listen_children(&self) -> Vec<(SocketId, String)> {
        self.listen_sockets
            .lock()
            .ok()
            .map(|sockets| {
                sockets
                    .iter()
                    .filter(|(_, _, listen)| *listen)
                    .map(|(id, name, _)| (*id, name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All server sockets (for `GetServerSockets`).
    pub(crate) fn socket_children(&self) -> Vec<(SocketId, String)> {
        self.listen_sockets
            .lock()
            .ok()
            .map(|sockets| {
                sockets
                    .iter()
                    .map(|(id, name, _)| (*id, name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl SocketEntry {
    pub(crate) fn streams_started(&self) -> u64 {
        self.streams_started.load(Ordering::Relaxed)
    }

    pub(crate) fn streams_succeeded(&self) -> u64 {
        self.streams_succeeded.load(Ordering::Relaxed)
    }

    pub(crate) fn streams_failed(&self) -> u64 {
        self.streams_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn messages_sent(&self) -> u64 {
        self.messages_sent.load(Ordering::Relaxed)
    }

    pub(crate) fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    pub(crate) fn keep_alives_sent(&self) -> u64 {
        self.keep_alives_sent.load(Ordering::Relaxed)
    }

    pub(crate) fn last_local_stream_ms(&self) -> u64 {
        self.last_local_stream_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn last_remote_stream_ms(&self) -> u64 {
        self.last_remote_stream_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn last_message_sent_ms(&self) -> u64 {
        self.last_message_sent_ms.load(Ordering::Relaxed)
    }

    pub(crate) fn last_message_received_ms(&self) -> u64 {
        self.last_message_received_ms.load(Ordering::Relaxed)
    }
}

/// Live entity counts (see [`Registry::len`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityCounts {
    /// Live channels.
    pub channels: usize,
    /// Live subchannels.
    pub subchannels: usize,
    /// Live servers.
    pub servers: usize,
    /// Live sockets.
    pub sockets: usize,
}

/// RAII channel registration: dropping unregisters the channel.
#[derive(Debug)]
pub struct ChannelHandle {
    registry: Arc<Registry>,
    id: ChannelId,
}

impl ChannelHandle {
    /// The registered channel id.
    #[must_use]
    pub fn id(&self) -> ChannelId {
        self.id
    }
}

impl Drop for ChannelHandle {
    fn drop(&mut self) {
        self.registry.unregister_channel(self.id);
    }
}

/// RAII subchannel registration: dropping unregisters the subchannel.
#[derive(Debug)]
pub struct SubchannelHandle {
    registry: Arc<Registry>,
    id: SubchannelId,
}

impl SubchannelHandle {
    /// The registered subchannel id.
    #[must_use]
    pub fn id(&self) -> SubchannelId {
        self.id
    }
}

impl Drop for SubchannelHandle {
    fn drop(&mut self) {
        self.registry.unregister_subchannel(self.id);
    }
}

/// RAII server registration: dropping unregisters the server.
#[derive(Debug)]
pub struct ServerHandle {
    registry: Arc<Registry>,
    id: ServerId,
}

impl ServerHandle {
    /// The registered server id.
    #[must_use]
    pub fn id(&self) -> ServerId {
        self.id
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.registry.unregister_server(self.id);
    }
}

/// RAII socket registration: dropping unregisters the socket.
#[derive(Debug)]
pub struct SocketHandle {
    registry: Arc<Registry>,
    id: SocketId,
}

impl SocketHandle {
    /// The registered socket id.
    #[must_use]
    pub fn id(&self) -> SocketId {
        self.id
    }
}

impl Drop for SocketHandle {
    fn drop(&mut self) {
        self.registry.unregister_socket(self.id);
    }
}

/// Millis since the Unix epoch, saturating to `u64::MAX`. Clocks
/// before the epoch read 0 (unset).
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

impl From<&crate::resolver::ResolvedAddress> for EndpointAddr {
    fn from(addr: &crate::resolver::ResolvedAddress) -> Self {
        match addr {
            crate::resolver::ResolvedAddress::Tcp(sock) => Self::Tcp(*sock),
            crate::resolver::ResolvedAddress::Unix(path) => Self::Uds(path.display().to_string()),
            crate::resolver::ResolvedAddress::UnixAbstract(name) => {
                Self::Uds(format!("@{}", String::from_utf8_lossy(name)))
            }
        }
    }
}

/// One-line address text for socket names.
fn addr_text(addr: &EndpointAddr) -> String {
    match addr {
        EndpointAddr::Tcp(sock) => sock.to_string(),
        EndpointAddr::Uds(path) => format!("unix:{path}"),
        EndpointAddr::Other(text) => text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelId, Connectivity, EndpointAddr, Registry, SocketParent, SocketSecurity,
        TraceSeverity,
    };
    use std::sync::Arc;

    fn tcp(port: u16) -> EndpointAddr {
        EndpointAddr::Tcp(format!("127.0.0.1:{port}").parse().expect("addr"))
    }

    #[test]
    fn ids_are_positive_ordered_and_never_reused() {
        let registry = Arc::new(Registry::new());
        let first = registry.register_channel("a", "dns:///a").id().get();
        let second = registry.register_channel("b", "dns:///b").id().get();
        assert!(first >= 1);
        assert!(second > first);
        drop(registry.register_channel("c", "dns:///c"));
        let fourth = registry.register_channel("d", "dns:///d").id().get();
        assert!(
            fourth > second,
            "ids never reused, got {fourth} after {second}"
        );
    }

    #[test]
    fn unregister_cascades_and_detaches() {
        let registry = Arc::new(Registry::new());
        let channel = registry.register_channel("c", "dns:///c");
        let sub = registry.register_subchannel(channel.id(), "10.0.0.1:80");
        let socket = registry.register_socket(
            SocketParent::Subchannel(sub.id()),
            Some(tcp(443)),
            Some(tcp(50051)),
            None,
            SocketSecurity::None,
            false,
        );
        assert_eq!(registry.len().sockets, 1);
        drop(socket);
        assert!(
            registry.len()
                == super::EntityCounts {
                    channels: 1,
                    subchannels: 1,
                    servers: 0,
                    sockets: 0,
                }
        );
        drop(sub);
        assert_eq!(registry.len().subchannels, 0);
        drop(channel);
        assert!(registry.is_empty());
    }

    #[test]
    fn channel_drop_takes_direct_sockets() {
        let registry = Arc::new(Registry::new());
        let channel = registry.register_channel("c", "passthrough:///x");
        let _socket = registry.register_socket(
            SocketParent::Channel(channel.id()),
            Some(tcp(443)),
            Some(tcp(50051)),
            None,
            SocketSecurity::None,
            false,
        );
        drop(channel);
        // The orphaned socket guard drops at scope end; either way the
        // registry ends empty (unregister is idempotent).
        assert!(registry.len().channels == 0);
    }

    #[test]
    fn subchannel_states_derive_parent_channel_state() {
        let registry = Arc::new(Registry::new());
        let channel = registry.register_channel("c", "dns:///c");
        let a = registry.register_subchannel(channel.id(), "a");
        let b = registry.register_subchannel(channel.id(), "b");
        let state = || {
            *registry
                .channel(channel.id())
                .expect("channel")
                .state
                .lock()
                .expect("lock")
        };
        assert_eq!(state(), Connectivity::Idle);
        registry.set_subchannel_state(a.id(), Connectivity::Connecting);
        assert_eq!(state(), Connectivity::Connecting);
        registry.set_subchannel_state(b.id(), Connectivity::TransientFailure);
        assert_eq!(
            state(),
            Connectivity::Connecting,
            "connecting beats failure"
        );
        registry.set_subchannel_state(a.id(), Connectivity::Ready);
        assert_eq!(state(), Connectivity::Ready);
        registry.set_subchannel_state(a.id(), Connectivity::TransientFailure);
        assert_eq!(state(), Connectivity::TransientFailure);
        drop((channel, a, b));
    }

    #[test]
    fn stream_events_cascade_to_subchannel_calls() {
        let registry = Arc::new(Registry::new());
        let channel = registry.register_channel("c", "dns:///c");
        let sub = registry.register_subchannel(channel.id(), "a");
        let socket = registry.register_socket(
            SocketParent::Subchannel(sub.id()),
            Some(tcp(443)),
            Some(tcp(50051)),
            None,
            SocketSecurity::None,
            false,
        );
        registry.note_stream_started(socket.id(), true);
        registry.note_messages(socket.id(), true, 1);
        registry.note_messages(socket.id(), false, 2);
        registry.note_stream_end(socket.id(), true);
        let sub_entry = registry.subchannel(sub.id()).expect("sub");
        use std::sync::atomic::Ordering;
        assert_eq!(sub_entry.calls_started.load(Ordering::Relaxed), 1);
        assert_eq!(sub_entry.calls_succeeded.load(Ordering::Relaxed), 1);
        let socket_entry = registry.socket(socket.id()).expect("socket");
        assert_eq!(socket_entry.streams_started.load(Ordering::Relaxed), 1);
        assert_eq!(socket_entry.messages_sent.load(Ordering::Relaxed), 1);
        assert_eq!(socket_entry.messages_received.load(Ordering::Relaxed), 2);
        // Channel-level calls are counted separately, at call completion.
        let channel_entry = registry.channel(channel.id()).expect("channel");
        assert_eq!(channel_entry.calls_started.load(Ordering::Relaxed), 0);
        drop((channel, sub, socket));
    }

    #[test]
    fn missing_ids_are_silent_noops() {
        let registry = Registry::new();
        let ghost = ChannelId(4242);
        registry.note_call_started(ghost);
        registry.note_call_end(ghost, true);
        registry.set_channel_state(ghost, Connectivity::Ready);
        registry.trace_channel(ghost, TraceSeverity::Info, "nothing");
        registry.unregister_channel(ghost);
        assert!(registry.is_empty());
    }

    #[test]
    fn creation_and_deletion_trace_the_parent() {
        let registry = Arc::new(Registry::new());
        let channel = registry.register_channel("c", "dns:///c");
        let sub = registry.register_subchannel(channel.id(), "a");
        drop(sub);
        let entry = registry.channel(channel.id()).expect("channel");
        let snapshot = entry.trace.snapshot();
        let descriptions: Vec<&str> = snapshot
            .events
            .iter()
            .map(|event| event.description.as_str())
            .collect();
        assert!(
            descriptions
                .iter()
                .any(|d| d.contains("Subchannel created")),
            "creation traced: {descriptions:?}"
        );
        assert!(
            descriptions
                .iter()
                .any(|d| d.contains("Subchannel deleted")),
            "deletion traced: {descriptions:?}"
        );
        drop(channel);
    }
}
