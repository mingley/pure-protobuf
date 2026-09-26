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
use crate::status::{Code, Status};
use crate::telemetry::{CallLabels, CallRole, LifecycleObserver, RejectionEvent, RejectionReason};
use crate::tls::PeerIdentity;
use crate::wire::{check_request, reject, reject_request};
use bytes::Bytes;
use h2::RecvStream;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

pub(crate) async fn serve_one<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    config: ServerConfig,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let result = serve_io(
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
    /// Transport `:scheme` when the accept loop knows it. `None` keeps the
    /// peer's `:scheme` ([`Incoming`] / [`Server::serve_connection`]).
    scheme: Option<&'static str>,
}

impl std::fmt::Debug for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionInfo")
            .field("remote", &self.remote.as_ref().map(|_| "[REDACTED]"))
            .field("local", &self.local.as_ref().map(|_| "[REDACTED]"))
            .field("identity", &self.identity.as_ref().map(|_| "[REDACTED]"))
            .field("cred", &self.cred.as_ref().map(|_| "[REDACTED]"))
            .field("scheme", &self.scheme.map(|_| "[REDACTED]"))
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
            scheme: Some("http"),
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
            scheme: Some("https"),
        }
    }

    pub(crate) fn unix(cred: Option<PeerCred>) -> Self {
        Self {
            remote: None,
            local: None,
            identity: None,
            cred,
            scheme: Some("http"),
        }
    }
}

pub(crate) fn incoming_rpc(
    request: http::Request<RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
    config: ServerConfig,
    peer: ConnectionInfo,
) -> Rpc {
    let metadata = Metadata::from_headers(request.headers());
    Rpc {
        request,
        respond,
        config,
        remote_addr: peer.remote,
        local_addr: peer.local,
        peer_identity: peer.identity,
        peer_cred: peer.cred,
        transport_scheme: peer.scheme,
        extensions: http::Extensions::new(),
        metadata,
        diagnostic_config: None,
        timeout: None,
        response_interceptor: None,
        byte_budget: ByteBudgetTracker::default(),
        observer: None,
    }
}

pub(crate) async fn serve_io<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: ConnectionInfo,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
    rpc_slots: Option<Arc<Semaphore>>,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let handshake = tokio::time::timeout(
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
    let (interval, timeout) = config.keepalive();
    let (age, idle, grace) = config.connection_lifetime();
    let age = age.map(|d| crate::config::jitter_age(d, connection_seed(peer.remote)));
    let dead = crate::keepalive::spawn(conn.ping_pong(), interval, timeout);
    let born = tokio::time::Instant::now();
    let busy = crate::keepalive::Busy::new();
    let mut last_idle = born;
    let mut occupied = false;
    let mut draining = false;
    let mut force_close: Option<tokio::time::Instant> = None;
    loop {
        let in_flight = busy.count();
        if in_flight == 0 {
            if occupied {
                last_idle = tokio::time::Instant::now();
                occupied = false;
            }
        } else {
            occupied = true;
        }
        let age_at = age.map(|d| born + d);
        let idle_at = if in_flight == 0 {
            idle.map(|d| last_idle + d)
        } else {
            None
        };
        tokio::select! {
            biased;
            accepted = std::future::poll_fn(|cx| conn.poll_accept(cx)) => {
                let Some(Ok((request, mut respond))) = accepted else {
                    break;
                };
                occupied = true;
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
                    reject_request(&mut respond, err, config.accepts_compressed());
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
                            reject(
                                &mut respond,
                                status,
                                config.accepts_compressed(),
                            );
                            continue;
                        }
                    },
                };
                let lease = busy.start();
                let queued_at = dispatch
                    .observer()
                    .map(|_| std::time::Instant::now());
                let dispatch = Arc::clone(&dispatch);
                let rpc_peer = peer.clone();
                drop(tokio::spawn(async move {
                    let _lease = lease;
                    let _permit = permit;
                    if let (Some(obs), Some(queued_at)) = (dispatch.observer(), queued_at) {
                        let path = request.uri().path();
                        let authority = request.uri().authority().map(http::uri::Authority::as_str);
                        let labels = CallLabels::new(path, authority, CallRole::Server);
                        obs.on_server_queue_wait(&labels, queued_at.elapsed());
                    }
                    dispatch
                        .dispatch(incoming_rpc(request, respond, config, rpc_peer))
                        .await;
                }));
            }
            _ = busy.notified() => {}
            _ = wait_for_drain(goaway.clone()), if !draining => {
                draining = true;
                force_close = Some(tokio::time::Instant::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(age_at), if !draining => {
                draining = true;
                force_close = Some(tokio::time::Instant::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(idle_at), if !draining => {
                draining = true;
                force_close = Some(tokio::time::Instant::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(force_close) => {
                break;
            }
            _ = crate::keepalive::wait_opt(dead.clone()) => {
                break;
            }
        }
    }
    Ok(())
}

pub(crate) async fn sleep_until_opt(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
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
