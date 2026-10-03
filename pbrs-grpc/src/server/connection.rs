//! Connection serving: [`serve_io`], [`ConnectionInfo`], drain waits.

#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::Incoming;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::{IncomingAccept, PeerCred};
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::{Server, rpc_slots};
use super::dispatch::Dispatch;
use super::rpc::Rpc;
use crate::config::ServerConfig;
use crate::limits::ByteBudgetTracker;
use crate::metadata::Metadata;
#[cfg(unix)]
use crate::request::UdsConnectInfo;
use crate::request::{TcpConnectInfo, TlsConnectInfo};
use crate::rt::{Runtime, TokioRuntime};
use crate::status::{Code, Status};
use crate::telemetry::{CallLabels, CallRole, LifecycleObserver, RejectionEvent, RejectionReason};
use crate::tls::PeerIdentity;
use crate::transport::h2::{RecvStream, SendResponse};
use crate::transport::{ServerBuilder, ServerConnection};
use crate::wire::{check_request, reject, reject_request};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

pub(crate) fn serve_one<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    config: ServerConfig,
) -> impl std::future::Future<Output = Result<(), Status>>
where
    D: Dispatch,
    IO: crate::rt::Io,
{
    serve_one_in::<TokioRuntime, D, IO>(dispatch, io, peer, config)
}

/// [`serve_one`] on runtime `R`; production callers use Tokio.
pub(crate) async fn serve_one_in<R: Runtime, D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    config: ServerConfig,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: crate::rt::Io,
{
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let result = serve_io_in::<R, _, _>(
        dispatch,
        io,
        ConnectionInfo::from_accept(peer),
        config,
        goaway_rx,
        rpc_slots(config),
    )
    .await;
    drop(goaway_tx);
    result
}

/// Connection-level facts copied onto every RPC on this socket.
///
/// The TCP, TLS, and Unix accept loops fill this themselves.
/// [`Incoming::peer`] is how a custom acceptor supplies a local address,
/// mTLS identity, Unix credentials, or a transport `:scheme`. The default
/// keeps the `SocketAddr` from [`IncomingAccept`] and does not override
/// `:scheme`. [`Server::serve_connection`] leaves every field unset.
/// Applies to every call shape on that connection.
#[derive(Clone, Default)]
pub struct ConnectionInfo {
    remote: Option<SocketAddr>,
    local: Option<SocketAddr>,
    identity: Option<PeerIdentity>,
    cred: Option<PeerCred>,
    #[cfg(unix)]
    uds_peer_addr: Option<Arc<tokio::net::unix::SocketAddr>>,
    /// Transport `:scheme` when the accept loop knows it. `None` keeps the
    /// peer's `:scheme` ([`Incoming`] / [`Server::serve_connection`]).
    scheme: Option<&'static str>,
    // Tonic's TLS fields are private. Capture its exact typed information
    // from the completed handshake, then share its certificate Arc per RPC.
    #[cfg(feature = "tonic")]
    tonic_tls:
        Option<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>,
    #[cfg(all(unix, feature = "tonic"))]
    tonic_uds: Option<tonic::transport::server::UdsConnectInfo>,
}

impl std::fmt::Debug for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("ConnectionInfo");
        dbg.field("remote", &self.remote.as_ref().map(|_| "[REDACTED]"))
            .field("local", &self.local.as_ref().map(|_| "[REDACTED]"))
            .field("identity", &self.identity.as_ref().map(|_| "[REDACTED]"))
            .field("cred", &self.cred.as_ref().map(|_| "[REDACTED]"));
        #[cfg(unix)]
        dbg.field(
            "uds_peer_addr",
            &self.uds_peer_addr.as_ref().map(|_| "[REDACTED]"),
        );
        dbg.field("scheme", &self.scheme.map(|_| "[REDACTED]"))
            .finish()
    }
}

impl ConnectionInfo {
    /// Empty facts: no addresses, no identity, no credentials, no scheme
    /// override. Same as [`Default`].
    /// Distinct from [`Self::from_accept`]: that copies the IncomingAccept tuple.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start from the `SocketAddr` [`Incoming::accept`] returned.
    /// Distinct from [`Self::new`]: that is empty facts, not this accept tuple.
    /// Distinct from [`Self::with_remote_addr`]: that overlays a builder; this starts from IncomingAccept.
    #[must_use]
    pub fn from_accept(remote: Option<SocketAddr>) -> Self {
        Self {
            remote,
            ..Self::default()
        }
    }

    /// Peer address reported as [`Rpc::remote_addr`].
    /// Distinct from [`Self::from_accept`]: that starts from IncomingAccept; this overlays a builder.
    #[must_use]
    pub fn with_remote_addr(mut self, addr: SocketAddr) -> Self {
        self.remote = Some(addr);
        self
    }

    /// Local address reported as [`Rpc::local_addr`].
    #[must_use]
    pub fn with_local_addr(mut self, addr: SocketAddr) -> Self {
        self.local = Some(addr);
        self
    }

    /// Client certificate chain reported as [`Rpc::peer_identity`].
    ///
    /// Build one with [`PeerIdentity::from_der_certs`] when the acceptor
    /// already verified TLS; the kernel does not parse X.509.
    #[must_use]
    pub fn with_peer_identity(mut self, identity: PeerIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Unix credentials reported as [`Rpc::peer_cred`].
    #[must_use]
    pub fn with_peer_cred(mut self, cred: PeerCred) -> Self {
        self.cred = Some(cred);
        self
    }

    /// Transport `:scheme` (`http` or `https`). `None` (the default on this
    /// type) keeps whatever the peer sent.
    #[must_use]
    pub fn with_scheme(mut self, scheme: &'static str) -> Self {
        self.scheme = Some(scheme);
        self
    }

    /// Peer address, if set.
    /// Distinct from [`Self::with_remote_addr`], which sets it.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote
    }

    /// Local address, if set.
    /// Distinct from [`Self::with_local_addr`], which sets it.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local
    }

    /// mTLS client certificate chain, if set.
    /// Distinct from [`Self::with_peer_identity`], which sets it.
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.identity.as_ref()
    }

    /// Unix credentials, if set.
    /// Distinct from [`Self::with_peer_cred`], which sets it.
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.cred
    }

    /// Transport `:scheme` override, if set.
    /// Distinct from [`Self::with_scheme`], which sets it.
    #[must_use]
    pub fn scheme(&self) -> Option<&'static str> {
        self.scheme
    }

    pub(crate) fn tcp(remote: SocketAddr, local: Option<SocketAddr>) -> Self {
        Self {
            remote: Some(remote),
            local,
            identity: None,
            cred: None,
            #[cfg(unix)]
            uds_peer_addr: None,
            scheme: Some("http"),
            #[cfg(feature = "tonic")]
            tonic_tls: None,
            #[cfg(all(unix, feature = "tonic"))]
            tonic_uds: None,
        }
    }

    pub(crate) fn tls(
        remote: SocketAddr,
        local: Option<SocketAddr>,
        identity: Option<PeerIdentity>,
    ) -> Self {
        Self {
            remote: Some(remote),
            local,
            identity,
            cred: None,
            #[cfg(unix)]
            uds_peer_addr: None,
            scheme: Some("https"),
            #[cfg(feature = "tonic")]
            tonic_tls: None,
            #[cfg(all(unix, feature = "tonic"))]
            tonic_uds: None,
        }
    }

    /// Retain exact tonic TLS information from a custom accepted stream.
    ///
    /// Obtain `info` with [`tonic::transport::server::Connected::connect_info`]
    /// on the actual stream after its TLS handshake and verification. Built-in
    /// TLS accept loops fill this automatically. Custom [`Incoming::peer`]
    /// implementations can attach it alongside the native address/identity
    /// builders; this method preserves those native fields. Certificate bytes
    /// alone cannot construct tonic's private TLS information.
    ///
    /// [`Server::serve_connection`] has no peer-info input; use
    /// [`Server::serve_with_incoming`] for custom connection facts.
    #[cfg(feature = "tonic")]
    #[must_use]
    pub fn with_tonic_tls(
        mut self,
        info: tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>,
    ) -> Self {
        self.tonic_tls = Some(info);
        self
    }

    /// Retain exact tonic Unix information from a custom accepted socket.
    ///
    /// Obtain `info` with [`tonic::transport::server::Connected::connect_info`]
    /// on that socket. Built-in Unix accept loops fill this automatically.
    /// Custom [`Incoming::peer`] implementations can attach it alongside
    /// [`Self::with_peer_cred`]; this method preserves native fields and does
    /// not manufacture Tokio's private credential value. Use
    /// [`Server::serve_with_incoming`] when supplying custom connection facts.
    #[cfg(all(unix, feature = "tonic"))]
    #[must_use]
    pub fn with_tonic_uds(mut self, info: tonic::transport::server::UdsConnectInfo) -> Self {
        self.tonic_uds = Some(info);
        self
    }

    #[cfg(unix)]
    pub(crate) fn unix(
        cred: Option<PeerCred>,
        peer_addr: Option<tokio::net::unix::SocketAddr>,
    ) -> Self {
        Self {
            remote: None,
            local: None,
            identity: None,
            cred,
            uds_peer_addr: peer_addr.map(Arc::new),
            scheme: Some("http"),
            #[cfg(feature = "tonic")]
            tonic_tls: None,
            #[cfg(feature = "tonic")]
            tonic_uds: None,
        }
    }
}

pub(crate) fn incoming_rpc(
    request: http::Request<RecvStream>,
    respond: SendResponse,
    config: ServerConfig,
    peer: ConnectionInfo,
    byte_budget: ByteBudgetTracker,
    channelz_server: Option<crate::channelz::ServerId>,
    channelz_socket: Option<crate::channelz::SocketId>,
) -> Rpc {
    let metadata = Metadata::from_headers(request.headers());
    #[cfg(feature = "grpc-web")]
    let web = crate::web::Mode::from_headers(request.headers());
    let mut extensions = http::Extensions::new();
    let tcp_info = TcpConnectInfo {
        local_addr: peer.local,
        remote_addr: peer.remote,
    };
    if peer.scheme == Some("https") {
        extensions.insert(TlsConnectInfo::new(tcp_info, peer.identity.clone()));
    } else if peer.remote.is_some() || peer.local.is_some() {
        extensions.insert(tcp_info);
    }
    #[cfg(feature = "tonic")]
    if let Some(tls_info) = peer.tonic_tls {
        extensions.insert(tls_info);
    } else if peer.scheme != Some("https") && (peer.remote.is_some() || peer.local.is_some()) {
        extensions.insert(tonic::transport::server::TcpConnectInfo {
            local_addr: peer.local,
            remote_addr: peer.remote,
        });
    }
    #[cfg(unix)]
    if peer.uds_peer_addr.is_some() || peer.cred.is_some() {
        extensions.insert(UdsConnectInfo {
            peer_addr: peer.uds_peer_addr.clone(),
            peer_cred: peer.cred,
        });
    }
    #[cfg(all(unix, feature = "tonic"))]
    if let Some(uds_info) = peer.tonic_uds {
        extensions.insert(uds_info);
    }
    Rpc {
        request,
        respond,
        config,
        remote_addr: peer.remote,
        local_addr: peer.local,
        peer_identity: peer.identity,
        peer_cred: peer.cred,
        transport_scheme: peer.scheme,
        #[cfg(feature = "grpc-web")]
        web,
        extensions,
        metadata,
        diagnostic_config: None,
        timeout: None,
        response_interceptor: None,
        byte_budget,
        observer: None,
        binlog: None,
        channelz_server,
        channelz_socket,
    }
}

/// Channelz for a request rejected before dispatch (bad headers or the
/// concurrency cap): a started-and-failed call and stream, like the
/// observer's start/reject/end triple. No messages flowed.
fn note_rejected_call(
    server: Option<crate::channelz::ServerId>,
    socket: Option<crate::channelz::SocketId>,
) {
    let global = crate::channelz::Registry::global();
    if let Some(server) = server {
        global.note_server_call_started(server);
        global.note_server_call_end(server, false);
    }
    if let Some(socket) = socket {
        global.note_stream_started(socket, false);
        global.note_stream_end(socket, false);
    }
}

pub(crate) fn serve_io<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: ConnectionInfo,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
    rpc_slots: Option<Arc<Semaphore>>,
) -> impl std::future::Future<Output = Result<(), Status>>
where
    D: Dispatch,
    IO: crate::rt::Io,
{
    serve_io_in::<TokioRuntime, D, IO>(dispatch, io, peer, config, goaway, rpc_slots)
}

/// [`serve_io`] on runtime `R`; production callers use Tokio.
pub(crate) async fn serve_io_in<R: Runtime, D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: ConnectionInfo,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
    rpc_slots: Option<Arc<Semaphore>>,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: crate::rt::Io,
{
    let handshake = R::timeout(
        config.io_handshake_timeout(),
        config.h2_builder().handshake(io),
    );
    let mut conn = tokio::select! {
        res = handshake => match res {
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => return Err(Status::unavailable(e.to_string())),
            Err(_) => return Err(Status::unavailable("http/2 preface timed out")),
        },
        _ = wait_for_drain(goaway.clone()) => {
            return Ok(());
        }
    };
    // Channelz: one socket per accepted connection, held (via
    // `_channelz_socket`) until the connection task ends so in-flight
    // RPCs attribute to a live socket.
    let channelz_server = dispatch.channelz_server();
    let _channelz_socket = channelz_server.map(|server| {
        let local = peer.local.map(crate::channelz::EndpointAddr::Tcp);
        let remote = peer.remote.map(crate::channelz::EndpointAddr::Tcp);
        let security = if peer.scheme == Some("https") || peer.identity.is_some() {
            crate::channelz::SocketSecurity::Tls {
                local_certificate: Vec::new(),
                remote_certificate: Vec::new(),
            }
        } else {
            crate::channelz::SocketSecurity::None
        };
        crate::channelz::Registry::global_shared().register_socket(
            crate::channelz::SocketParent::Server(server),
            local,
            remote,
            None,
            security,
            false,
        )
    });
    let channelz_socket_id = _channelz_socket.as_ref().map(|handle| handle.id());
    let (interval, timeout) = config.keepalive();
    // In-flight tracking only feeds the idle timer; without one, skip the
    // lease, the wakeups, and the idle bookkeeping entirely.
    let (age, idle, grace) = config.connection_lifetime();
    let track_busy = idle.is_some();
    let age = age.map(|d| crate::config::jitter_age(d, connection_seed(peer.remote)));
    let dead = crate::keepalive::spawn_in::<R>(conn.ping_pong(), interval, timeout);
    let born = R::now();
    let busy = crate::keepalive::Busy::new();
    let mut last_idle = born;
    let mut occupied = false;
    let mut draining = false;
    let mut force_close: Option<tokio::time::Instant> = None;
    // Hoisted one-shot waits: re-creating them every round re-registers a
    // waiter per RPC. Each fires once (`drain` arms `draining`, `dead`
    // breaks), so they are never polled after completion.
    let drain_fut = wait_for_drain(goaway.clone());
    tokio::pin!(drain_fut);
    let dead_fut = crate::keepalive::wait_opt(dead.clone());
    tokio::pin!(dead_fut);
    loop {
        let in_flight = if track_busy {
            let n = busy.count();
            if n == 0 {
                if occupied {
                    last_idle = R::now();
                    occupied = false;
                }
            } else {
                occupied = true;
            }
            n
        } else {
            0
        };
        let age_at = age.map(|d| born + d);
        let idle_at = if in_flight == 0 {
            idle.map(|d| last_idle + d)
        } else {
            None
        };
        tokio::select! {
            biased;
            accepted = conn.accept() => {
                let Some(Ok((request, mut respond))) = accepted else {
                    break;
                };
                if track_busy {
                    occupied = true;
                }
                #[cfg(feature = "grpc-web")]
                if crate::web::send_cors_preflight(&request, &mut respond, config.grpc_web_cors()) {
                    continue;
                }
                if let Err(err) = check_request(&request, config.accepts_compressed()) {
                    if let Some(obs) = dispatch.observer() {
                        let path = request.uri().path();
                        let authority = request.uri().authority().map(http::uri::Authority::as_str);
                        let labels = CallLabels::new(path, authority, CallRole::Server);
                        let status = match &err {
                            crate::wire::RequestReject::Grpc(s) => s.clone(),
                            crate::wire::RequestReject::Http(c) => {
                                Status::unknown(format!("http {c}"))
                            }
                        };
                        obs.on_server_call_start(&labels);
                        obs.on_rejection(&RejectionEvent {
                            call: labels,
                            reason: RejectionReason::InvalidRequest,
                            code: status.code(),
                        });
                        obs.on_server_call_end(&labels, &status, Duration::ZERO);
                    }
                    note_rejected_call(channelz_server, channelz_socket_id);
                    #[cfg(feature = "grpc-web")]
                    if let (Some(web), crate::wire::RequestReject::Grpc(status)) =
                        (crate::web::Mode::from_headers(request.headers()), &err)
                    {
                        crate::web::send_trailers_only(
                            &mut respond,
                            web,
                            status.clone(),
                            &Metadata::new(),
                        );
                        continue;
                    }
                    reject_request(&mut respond, err, config.accepts_compressed());
                    continue;
                }
                if crate::wire::effective_timeout(request.headers(), config.rpc_timeout())
                    .is_some_and(|timeout| timeout.is_zero())
                {
                    let status = Status::deadline_exceeded();
                    if let Some(obs) = dispatch.observer() {
                        let path = request.uri().path();
                        let authority = request.uri().authority().map(http::uri::Authority::as_str);
                        let labels = CallLabels::new(path, authority, CallRole::Server);
                        obs.on_server_call_start(&labels);
                        obs.on_rejection(&RejectionEvent {
                            call: labels,
                            reason: RejectionReason::Other,
                            code: Code::DeadlineExceeded,
                        });
                        obs.on_server_call_end(&labels, &status, Duration::ZERO);
                    }
                    note_rejected_call(channelz_server, channelz_socket_id);
                    #[cfg(feature = "grpc-web")]
                    if let Some(web) = crate::web::Mode::from_headers(request.headers()) {
                        crate::web::send_trailers_only(
                            &mut respond,
                            web,
                            status,
                            &Metadata::new(),
                        );
                        continue;
                    }
                    reject(
                        &mut respond,
                        status,
                        config.accepts_compressed(),
                    );
                    continue;
                }
                let permit = match &rpc_slots {
                    None => None,
                    Some(slots) => match slots.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            let status = Status::resource_exhausted("too many concurrent RPCs");
                            if let Some(obs) = dispatch.observer() {
                                let path = request.uri().path();
                                let authority = request.uri().authority().map(http::uri::Authority::as_str);
                                let labels = CallLabels::new(path, authority, CallRole::Server);
                                obs.on_server_call_start(&labels);
                                obs.on_rejection(&RejectionEvent {
                                    call: labels,
                                    reason: RejectionReason::ConcurrencyLimit,
                                    code: Code::ResourceExhausted,
                                });
                                obs.on_server_call_end(&labels, &status, Duration::ZERO);
                            }
                            note_rejected_call(channelz_server, channelz_socket_id);
                            #[cfg(feature = "grpc-web")]
                            if let Some(web) = crate::web::Mode::from_headers(request.headers()) {
                                crate::web::send_trailers_only(
                                    &mut respond,
                                    web,
                                    status,
                                    &Metadata::new(),
                                );
                                continue;
                            }
                            reject(
                                &mut respond,
                                status,
                                config.accepts_compressed(),
                            );
                            continue;
                        }
                    },
                };
                let lease = track_busy.then(|| busy.start());
                let queued_at = dispatch
                    .observer()
                    .map(|_| std::time::Instant::now());
                let dispatch = Arc::clone(&dispatch);
                let rpc_peer = peer.clone();
                let byte_budget = dispatch.byte_budget();
                #[cfg(feature = "copy-counts")]
                crate::copy_counts::note_spawn(crate::copy_counts::SpawnSite::ServerRpcDispatch);
                drop(R::spawn(async move {
                    let _lease = lease;
                    let _permit = permit;
                    if let (Some(obs), Some(queued_at)) = (dispatch.observer(), queued_at) {
                        let path = request.uri().path();
                        let authority = request.uri().authority().map(http::uri::Authority::as_str);
                        let labels = CallLabels::new(path, authority, CallRole::Server);
                        obs.on_server_queue_wait(&labels, queued_at.elapsed());
                    }
                    dispatch
                        .dispatch(incoming_rpc(
                            request,
                            respond,
                            config,
                            rpc_peer,
                            byte_budget,
                            channelz_server,
                            channelz_socket_id,
                        ))
                        .await;
                }));
            }
            _ = busy.notified(), if track_busy => {}
            _ = &mut drain_fut, if !draining => {
                draining = true;
                force_close = Some(R::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt::<R>(age_at), if !draining && age_at.is_some() => {
                draining = true;
                force_close = Some(R::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt::<R>(idle_at), if !draining && idle_at.is_some() => {
                draining = true;
                force_close = Some(R::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt::<R>(force_close), if force_close.is_some() => {
                break;
            }
            _ = &mut dead_fut => {
                break;
            }
        }
    }
    Ok(())
}

/// Sleep until `at` on runtime `R`, or pend forever when there is none.
pub(crate) async fn sleep_until_opt<R: Runtime>(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => R::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

pub(crate) fn connection_seed(peer: Option<SocketAddr>) -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    match peer {
        Some(SocketAddr::V4(addr)) => u64::from(u32::from(*addr.ip()))
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(u64::from(addr.port()))
            .wrapping_add(n),
        Some(SocketAddr::V6(addr)) => {
            let mut h = n;
            for b in addr.ip().octets() {
                h = h.wrapping_mul(16_777_619).wrapping_add(u64::from(b));
            }
            h.wrapping_add(u64::from(addr.port()))
        }
        None => n,
    }
}

pub(crate) async fn wait_for_drain(mut goaway: watch::Receiver<bool>) {
    // A dropped sender also means "stop accepting": the accept loop is gone.
    goaway.wait_for(|v| *v).await.ok();
}
