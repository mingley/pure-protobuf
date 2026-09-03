//! Serving: the [`Service`] trait, per-RPC dispatch through [`Rpc`], and the
//! [`Server`] / [`Router`] accept loops.
//!
//! Generated code implements [`Service`]; you implement the generated service
//! trait. Writing either by hand is supported and documented, because a
//! kernel you cannot drive by hand is a kernel you cannot debug.

use crate::config::{ServerConfig, Wire};
use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::tls::{PeerIdentity, ServerTls};
use crate::wire::{
    check_request, encode_msg, grpc_trailers, gzip_outbound, gzip_stream_frame,
    let_producer_catch_up, read_one_message, reject, reject_request, send_bytes, send_ok_headers,
    send_trailers_only, wrap_timeout, OutBatch, WireStream,
};
use bytes::Bytes;
use h2::RecvStream;
use pbrs::{Parse, Serialize};
use std::collections::HashMap;
use std::future::{poll_fn, Future};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch, Semaphore};

/// A gRPC service that can be served.
///
/// `protoc-gen-pbrs` emits one implementation per `service` in your `.proto`,
/// so the usual path is to implement the generated trait (`Greeter`) and let
/// the generated type (`GreeterServer`) implement this.
///
/// Implementing it by hand takes a name and a `match` on
/// [`Rpc::method`]:
///
/// ```
/// use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Service, Status};
///
/// struct Echo;
///
/// impl Service for Echo {
///     const NAME: &'static str = "demo.Echo";
///
///     async fn call(&self, rpc: Rpc) {
///         match rpc.method() {
///             "Ping" => {
///                 rpc.unary(|req: Request<HelloRequest>| async move {
///                     let mut reply = HelloReply::new();
///                     reply.set_message(req.get_ref().name());
///                     Ok::<_, Status>(Response::new(reply))
///                 })
///                 .await;
///             }
///             _ => rpc.unimplemented(),
///         }
///     }
/// }
/// ```
pub trait Service: Send + Sync + 'static {
    /// Fully qualified proto service name, e.g. `helloworld.Greeter`.
    ///
    /// [`Router`] keys on this, and it is the `<service>` half of the
    /// `/<service>/<method>` request path.
    const NAME: &'static str;

    /// Extra `/<service>/` prefixes [`Router`] also mounts this service at.
    ///
    /// Default is empty. Generated `grpc.reflection.v1.ServerReflection`
    /// aliases `grpc.reflection.v1alpha.ServerReflection` so older grpcurl
    /// that falls back to v1alpha hits the same handler. That is a path
    /// alias, not a second proto and not a second `ServerReflectionServer`.
    /// An interceptor on the v1alpha path sees
    /// [`Rpc::service`] `grpc.reflection.v1alpha.ServerReflection` — Distinct
    /// from the v1 name, which is the path the peer sent.
    /// [`Server`] does not look up [`Self::NAME`] or these aliases: a lone
    /// reflection server already answers a v1alpha path. Distinct from
    /// mounting the same handler twice. Distinct from grpc-web, which is a
    /// second protocol, not a path alias.
    /// A wrapping [`Service`] should forward these like [`Self::NAME`].
    const ALIASES: &'static [&'static str] = &[];

    /// Dispatch one RPC.
    ///
    /// Match on [`Rpc::method`] and consume the [`Rpc`] with the call shape
    /// the method declares. Returning without consuming it resets the stream.
    fn call(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
}

/// Object-safe [`Service`], so [`Router`] can hold a heterogeneous map.
trait DynService: Send + Sync + 'static {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

impl<S: Service> DynService for S {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(self.call(rpc))
    }
}

/// What the accept loop hands each stream. Monomorphic for [`Server`], boxed
/// for [`Router`].
trait Dispatch: Send + Sync + 'static {
    fn dispatch(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
}

/// One [`Incoming::accept`] result: a connection, an error, or `None` if exhausted.
///
/// The `SocketAddr` is the remote peer when the transport has one. Other
/// connection facts go on [`Incoming::peer`], not this tuple.
#[allow(
    clippy::type_complexity,
    reason = "Option<Result<(Io, peer), Status>> is the accept contract"
)]
pub type IncomingAccept<Io> = Option<Result<(Io, Option<SocketAddr>), Status>>;

/// A source of already-accepted byte streams.
///
/// [`TcpListener`] and Unix listeners are served by [`Server::serve_listener`]
/// / [`Server::serve_unix_listener`] so TCP_NODELAY, TCP keepalive, and TLS
/// stay applied. Implement this for a custom acceptor (in-process duplex,
/// vsock, a TLS stack you drove yourself).
///
/// [`IncomingAccept`] stays `(Io, Option<SocketAddr>)`. Override [`Self::peer`]
/// to return a [`ConnectionInfo`] with a local address, mTLS identity, Unix
/// credentials, or a transport `:scheme`. The default copies the accept
/// address and does not probe `Io`.
///
/// Returning `None` means the source is exhausted: the server stops accepting,
/// sends `GOAWAY`, and drains. After the last connection, pending forever is
/// usually what you want, so the live stream is not torn down.
///
/// ```
/// use std::future::Future;
/// use std::net::SocketAddr;
/// use pbrs_grpc::{ConnectionInfo, Incoming, IncomingAccept, PeerCred, PeerIdentity};
///
/// struct One(Option<tokio::net::TcpStream>);
///
/// impl Incoming for One {
///     type Io = tokio::net::TcpStream;
///     fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send {
///         let io = self.0.take();
///         async move { io.map(|io| Ok((io, None))) }
///     }
///     fn peer(&self, io: &Self::Io, remote: Option<SocketAddr>) -> ConnectionInfo {
///         let _ = (self, io, remote);
///         ConnectionInfo::new()
///             .with_remote_addr("192.0.2.1:8".parse().expect("remote"))
///             .with_local_addr("127.0.0.1:9".parse().expect("local"))
///             .with_peer_identity(PeerIdentity::from_der_certs([b"leaf"]).expect("leaf"))
///             .with_peer_cred(PeerCred::new(42, 43, Some(44)))
///             .with_scheme("https")
///     }
/// }
/// ```
pub trait Incoming: Send {
    /// Accepted byte stream. Must be an HTTP/2 prior-knowledge transport;
    /// this crate does not speak HTTP/1.1 or grpc-web.
    type Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static;

    /// Next connection, or `None` when the source is exhausted.
    ///
    /// `SocketAddr` is what [`Rpc::remote_addr`] reports unless
    /// [`Self::peer`] replaces it; use `None` when the transport has no TCP
    /// peer (Unix, in-process). Override [`Self::peer`] to fill
    /// [`Rpc::local_addr`], [`Rpc::peer_identity`], [`Rpc::peer_cred`], or a
    /// transport [`Rpc::scheme`]. The default leaves those unset: only the
    /// TCP accept loop fills the local address, only an mTLS handshake fills
    /// the client certificate, and only the Unix accept loop fills
    /// credentials.
    fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send;

    /// Facts copied onto every RPC on this connection.
    ///
    /// The default keeps the `SocketAddr` from [`Self::accept`] and does not
    /// probe `Io`. [`IncomingAccept`] is unchanged. Override this when you
    /// already know a local address, mTLS identity, Unix credentials, or a
    /// transport `:scheme` (a vsock, a TLS stack you drove, a Unix socket
    /// you accepted yourself). Applies to every call shape on that
    /// connection.
    fn peer(&self, io: &Self::Io, remote: Option<SocketAddr>) -> ConnectionInfo {
        let _ = (self, io);
        ConnectionInfo::from_accept(remote)
    }
}

/// Unix-domain peer credentials from `SO_PEERCRED` (Linux) or
/// `LOCAL_PEERCRED` (macOS / *BSD).
///
/// Present on [`Rpc::peer_cred`] / [`Request::peer_cred`] after
/// [`Server::serve_unix`] / [`Server::serve_unix_until_shutdown`] (and the
/// `*_unlink` / `serve_unix_listener` forms), or when [`Incoming::peer`]
/// supplies them. TCP, TLS, and [`Server::serve_connection`] yield `None`
/// even when the byte stream is a Unix socket — those entry points do not
/// probe `Io`.
///
/// The kernel does not interpret uid/gid against `/etc/passwd`; an
/// interceptor that authorizes by user does that itself. `pid` is `None` on
/// platforms that only report uid/gid.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PeerCred {
    uid: u32,
    gid: u32,
    pid: Option<u32>,
}

impl PeerCred {
    /// Construct credentials. The Unix accept loop fills these from the
    /// socket; an [`Incoming`] implementor that already probed `Io` uses this.
    #[must_use]
    pub const fn new(uid: u32, gid: u32, pid: Option<u32>) -> Self {
        Self { uid, gid, pid }
    }

    /// Effective user id of the connecting process.
    #[must_use]
    pub const fn uid(self) -> u32 {
        self.uid
    }

    /// Effective group id of the connecting process.
    #[must_use]
    pub const fn gid(self) -> u32 {
        self.gid
    }

    /// Process id of the connecting process, when the platform reports one.
    ///
    /// Linux, macOS, and *BSD typically set this. Treat `None` as "unknown",
    /// not "pid 0".
    #[must_use]
    pub const fn pid(self) -> Option<u32> {
        self.pid
    }
}

#[cfg(unix)]
fn peer_cred_of(io: &UnixStream) -> Option<PeerCred> {
    let cred = io.peer_cred().ok()?;
    Some(PeerCred {
        uid: cred.uid(),
        gid: cred.gid(),
        pid: cred.pid().and_then(|pid| u32::try_from(pid).ok()),
    })
}

/// One inbound RPC, before its call shape has been chosen.
///
/// Consume it with exactly one of [`Self::unary`],
/// [`Self::client_streaming`], [`Self::server_streaming`],
/// [`Self::bidi_streaming`], or [`Self::unimplemented`]. Each one owns the
/// full response: headers, message frames, and `grpc-status` trailers.
///
/// An [`crate::Interceptor`] may mutate [`Self::metadata_mut`], cap the
/// deadline with [`Self::set_timeout`], inspect the server overlay with
/// [`Self::rpc_timeout`], attach typed state on
/// [`Self::extensions_mut`], or turn the RPC away with [`Self::reject`].
pub struct Rpc {
    request: http::Request<RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
    config: ServerConfig,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    peer_identity: Option<PeerIdentity>,
    peer_cred: Option<PeerCred>,
    transport_scheme: Option<&'static str>,
    extensions: http::Extensions,
    metadata: Metadata,
    timeout: Option<Duration>,
    response_interceptor: Option<crate::interceptor::ResponseHook>,
}

impl std::fmt::Debug for Rpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rpc")
            .field("authority", &self.authority())
            .field("path", &self.path())
            .field("service", &self.service())
            .field("method", &self.method())
            .field("remote_addr", &self.remote_addr)
            .field("local_addr", &self.local_addr)
            .field("peer_identity", &self.peer_identity)
            .field("peer_cred", &self.peer_cred)
            .field("scheme", &self.scheme())
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("rpc_timeout", &self.rpc_timeout())
            .field("peer_timeout", &self.peer_timeout())
            .field("effective_timeout", &self.effective_timeout())
            .field("deadline", &self.deadline())
            .field("limits", &self.limits())
            .field("accepts_gzip", &self.accepts_gzip())
            .field("compresses_outbound", &self.compresses_outbound())
            .field("gzip_level", &self.gzip_level())
            .field("accepts_compressed", &self.accepts_compressed())
            .field("concurrent_rpc_limit", &self.concurrent_rpc_limit())
            .field("send_buffer_size", &self.send_buffer_size())
            .field("encoding", &self.encoding())
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
    }
}

impl Rpc {
    /// Full request path, e.g. `/helloworld.Greeter/SayHello`.
    ///
    /// Generated handlers see the same value on [`Request::path`]. Bind it
    /// before [`Self::metadata_mut`]: `let path = rpc.path();`. Visible on
    /// every call shape.
    #[must_use]
    pub fn path(&self) -> &str {
        self.request.uri().path()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Generated handlers see the same value on [`Request::service`].
    /// Applies to every call shape.
    #[must_use]
    pub fn service(&self) -> &str {
        split_path(self.path()).0
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Generated handlers see the same value on [`Request::method`].
    /// Applies to every call shape.
    #[must_use]
    pub fn method(&self) -> &str {
        split_path(self.path()).1
    }

    /// HTTP/2 `:authority` the peer sent, e.g. `127.0.0.1:50051` or
    /// `localhost` on a Unix socket.
    ///
    /// TLS uses the client's [`crate::Target`], not SNI, unless
    /// [`crate::Channel::origin`] overrode `:authority` on that clone.
    /// Applies to every call shape.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.request
            .uri()
            .authority()
            .map(http::uri::Authority::as_str)
    }

    /// HTTP/2 `:scheme` for this RPC (`http` on h2c, `https` on TLS).
    ///
    /// On TCP and Unix this is the transport, not whatever the peer wrote:
    /// a cleartext connection reports `http` even if the preface claimed
    /// `https`. The default [`Incoming`] and [`Server::serve_connection`] keep
    /// the peer's `:scheme`. [`Incoming::peer`] can set a transport scheme.
    /// Applies to every call shape.
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.transport_scheme
            .or_else(|| self.request.uri().scheme_str())
    }

    /// Peer address, when the transport exposed one.
    ///
    /// TCP fills this from accept. [`Incoming`] copies the `SocketAddr` from
    /// [`IncomingAccept`] unless [`Incoming::peer`] replaces it. Unix and
    /// [`Server::serve_connection`] yield `None`. Applies to every call shape.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    ///
    /// On TCP this is `TcpStream::local_addr` (the interface the peer hit),
    /// not the listener bind address if that was `0.0.0.0`. Unix and
    /// [`Server::serve_connection`] yield `None`. The default [`Incoming`]
    /// leaves it unset; [`Incoming::peer`] can fill it. Applies to every call
    /// shape.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    ///
    /// Leaf first, DER-encoded. TLS without client authentication, h2c,
    /// Unix, the default [`Incoming`], and [`Server::serve_connection`] yield
    /// `None`. [`Incoming::peer`] can supply a chain the acceptor already
    /// verified ([`PeerIdentity::from_der_certs`]). The kernel does not parse
    /// X.509. Applies to every call shape.
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.peer_identity.as_ref()
    }

    /// Unix-socket peer credentials (`SO_PEERCRED`), when the accept loop
    /// filled them.
    ///
    /// Same-process tests see this process's uid/gid/`pid`. TCP, TLS, the
    /// default [`Incoming`], and [`Server::serve_connection`] yield `None`.
    /// [`Incoming::peer`] can supply credentials the acceptor already probed.
    /// Applies to every call shape.
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.peer_cred
    }

    /// Request metadata the handler will see.
    ///
    /// Distinct from [`Self::metadata_mut`]: that mutates the inbound map; this borrows it.
    /// Same map as [`Request::metadata`] after an interceptor returns `Ok`.
    /// Bind it if you need more than one lookup: `let md = rpc.metadata()`.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutate inbound metadata the handler will see.
    ///
    /// Distinct from [`Self::metadata`]: that borrows the inbound map; this mutates it.
    /// Insert, or strip with [`Metadata::remove`] / [`Metadata::remove_bin`].
    /// Reserved keys (`grpc-*`, `content-type`, hop-by-hop headers, ...)
    /// stay on the HTTP request for the kernel; they cannot be inserted or
    /// removed here.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Cap this RPC's deadline. Combined with the client's `grpc-timeout` and
    /// [`ServerConfig::timeout`] as the soonest of the three; an interceptor
    /// can only tighten, not extend. Calling this twice keeps the sooner
    /// value. Values below 1 ms are raised to 1 ms. This is the handler's
    /// deadline on every call shape.
    ///
    /// Distinct from [`Self::timeout`]: that reads the interceptor cap; this tightens it.
    pub fn set_timeout(&mut self, timeout: Duration) {
        let timeout = timeout.max(Duration::from_millis(1));
        self.timeout = Some(match self.timeout {
            Some(prev) => prev.min(timeout),
            None => timeout,
        });
    }

    /// Deadline cap an interceptor set with [`Self::set_timeout`], if any.
    ///
    /// Distinct from [`Self::rpc_timeout`]: that is the server overlay; this is the interceptor cap.
    /// This is not the effective deadline: that also includes the client's
    /// `grpc-timeout` and [`ServerConfig::timeout`]. See
    /// [`Self::effective_timeout`]. The server overlay itself is
    /// [`Self::rpc_timeout`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Server [`ServerConfig::timeout`] overlay.
    ///
    /// Distinct from [`Self::timeout`] (an interceptor cap),
    /// [`Self::peer_timeout`] (the client's `grpc-timeout`), and
    /// [`Self::effective_timeout`] (the soonest of the three). This is the
    /// server policy even after [`Self::set_timeout`]. Same value as
    /// [`crate::Server::rpc_timeout`]. Generated handlers see it on
    /// [`Request::rpc_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::rpc_timeout`].
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.config.rpc_timeout()
    }

    /// The client's `grpc-timeout`, if it sent one.
    ///
    /// Independent of [`Self::timeout`], which is only an interceptor cap,
    /// and of [`Self::rpc_timeout`], which is the server overlay.
    /// [`Self::effective_timeout`] is the soonest of this, the server cap,
    /// and the interceptor cap. Generated handlers see the same value on
    /// [`Request::peer_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::peer_timeout`].
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        crate::wire::timeout_from_headers(self.request.headers())
    }

    /// Deadline the handler will run under: the soonest of the client's
    /// `grpc-timeout`, [`ServerConfig::timeout`], and [`Self::set_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::timeout`].
    #[must_use]
    pub fn effective_timeout(&self) -> Option<Duration> {
        crate::wire::soonest(
            crate::wire::effective_timeout(self.request.headers(), self.config.rpc_timeout()),
            self.timeout,
        )
    }

    /// Absolute Instant matching [`Self::effective_timeout`].
    ///
    /// Distinct from [`Self::timeout`]: that is the interceptor duration cap; this Instant is computed when the getter runs.
    /// Computed when you call this, so an interceptor that just tightened
    /// [`Self::set_timeout`] sees the new Instant. The handler's
    /// [`Request::deadline`] is stamped once when dispatch starts. Visible
    /// on every call shape.
    /// Response interceptors see the same Instant as [`Request::deadline`] on [`crate::Response::deadline`].
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.effective_timeout()
            .map(|d| tokio::time::Instant::now() + d)
    }

    /// Effective message caps for this RPC.
    ///
    /// Same value as [`ServerConfig::limits`]: the inbound decode cap and the
    /// outbound encode cap the kernel enforces when it reads and writes
    /// frames. Default is 4 MiB inbound ([`crate::DEFAULT_MAX_DECODING_MESSAGE_SIZE`])
    /// and unlimited outbound. An interceptor cannot raise them; it can only
    /// inspect, or reject before the body is read. Generated handlers see the
    /// same caps on [`Request::limits`].
    /// Same overlay as [`crate::Server::limits`].
    /// Response interceptors see the same caps on [`crate::Response::limits`].
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.config.limits()
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    ///
    /// Same value as [`Request::accepts_gzip`] after dispatch. A handler that
    /// calls [`crate::Response::set_compress`] still only gzips when this is
    /// true: the kernel will not compress a peer that did not ask.
    /// Response interceptors see the same value on [`crate::Response::accepts_gzip`].
    /// Distinct from [`Self::accepts_compressed`]: that is this server overlay, not the peer advertisement.
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        crate::wire::accepts_gzip(self.request.headers())
    }

    /// Whether this server gzips responses when the peer advertised gzip.
    ///
    /// Same overlay as [`crate::Server::compresses_outbound`]. A handler
    /// [`crate::Response::set_compress`]`(false)` opts out; unset follows
    /// this default. Generated handlers see the same value on
    /// [`Request::compresses_outbound`].
    /// Response interceptors see the same value on [`crate::Response::compresses_outbound`].
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.config.compresses_outbound()
    }

    /// Configured outbound gzip deflate level.
    ///
    /// Same overlay as [`crate::Server::gzip_level`].
    /// Generated handlers see the same value on [`Request::gzip_level`].
    /// Distinct from [`Self::compresses_outbound`]: that is on or off; this is deflate effort.
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay.
    /// Response interceptors see the same value on [`crate::Response::gzip_level`].
    /// An interceptor cannot change this; the kernel applies it when encoding.
    /// Applies to every call shape.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.config.gzip_level()
    }

    /// Whether this server inflates inbound gzip. Default `true`.
    ///
    /// Same overlay as [`crate::Server::accepts_compressed`].
    /// Generated handlers see the same value on [`Request::accepts_compressed`].
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this overlay.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay.
    /// Response interceptors see the same value on [`crate::Response::accepts_compressed`].
    /// An interceptor cannot change this; the kernel applies it when decoding.
    /// Applies to every call shape.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.config.accepts_compressed()
    }

    /// Configured process-wide RPC cap, if any.
    ///
    /// Same overlay as [`crate::Server::concurrent_rpc_limit`].
    /// Generated handlers see the same value on [`Request::concurrent_rpc_limit`].
    /// Distinct from [`crate::Outgoing::concurrent_rpc_limit`]: that is a client interceptor overlay.
    /// Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits.
    /// `None` when the server omitted a cap. An interceptor cannot change this; extras are [`Code::ResourceExhausted`] before the handler runs.
    /// Applies to every call shape.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.config.concurrent_rpc_limit()
    }

    /// Configured write-time HTTP/2 send buffer.
    ///
    /// Same overlay as [`crate::Server::send_buffer_size`].
    /// Generated handlers see the same value on [`Request::send_buffer_size`].
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay.
    /// Distinct from [`Self::limits`]: that is uncompressed protobuf bytes, not this HTTP/2 send buffer.
    /// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows: those are handshake SETTINGS, not this write-time threshold.
    /// Response interceptors see the same value on [`crate::Response::send_buffer_size`].
    /// An interceptor cannot change this; the kernel applies it when sending DATA.
    /// Applies to every call shape.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.config.send_buffer_size()
    }

    /// The peer's `grpc-encoding` token, if it sent a non-identity coding.
    ///
    /// Missing, empty, or an explicit `identity` token is `None` — the spec
    /// treats those as the same coding. `"GZIP"` stays `"GZIP"`. Generated
    /// handlers see the same value on [`Request::encoding`]. `grpc-*` keys
    /// are not in [`Self::metadata`]. Bind it before [`Self::metadata_mut`]:
    /// `let enc = rpc.encoding();`.
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this received `grpc-encoding`.
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        crate::wire::grpc_encoding(self.request.headers())
    }

    /// Typed values an interceptor may attach for the handler.
    ///
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values the handler will see; this borrows the map.
    /// Empty until an [`crate::Interceptor`] (or wrapping [`Service`]) inserts
    /// into [`Self::extensions_mut`]. Survives onto the [`Request`] the
    /// handler receives.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values the handler will see on [`Request::extensions`].
    ///
    /// Distinct from [`Self::extensions`]: that borrows the map; this inserts typed values the handler will see.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// Stack `hook` after any [`crate::Server::on_response`] already on this RPC.
    pub(crate) fn push_response_hook(&mut self, hook: crate::interceptor::ResponseHook) {
        self.response_interceptor = Some(match self.response_interceptor.take() {
            None => hook,
            Some(prev) => Arc::new(crate::interceptor::ResponseThen::new(prev, hook)),
        });
    }

    /// Answer with `UNIMPLEMENTED`, naming the path.
    ///
    /// This is the correct default arm of a method `match`: a peer asking for
    /// a method you do not have is a peer error, not a server error.
    pub fn unimplemented(mut self) {
        send_trailers_only(
            &mut self.respond,
            Status::unimplemented(self.request.uri().path().to_string()),
            &Metadata::new(),
        );
    }

    /// Answer with `status` without reading the request body.
    ///
    /// This is how an [`crate::Interceptor`] or a wrapping [`Service`] turns
    /// away an RPC it will not delegate, for example on failed authentication.
    /// Trailing metadata on `status` and `grpc-status-details-bin` (see
    /// [`Status::with_error_details`]) both ship.
    ///
    /// ```
    /// use pbrs_grpc::{Rpc, Service, Status};
    /// use std::sync::Arc;
    ///
    /// /// Requires a bearer token before delegating to `inner`.
    /// struct RequireAuth<S> {
    ///     inner: Arc<S>,
    ///     token: String,
    /// }
    ///
    /// impl<S: Service> Service for RequireAuth<S> {
    ///     const NAME: &'static str = S::NAME;
    ///     const ALIASES: &'static [&'static str] = S::ALIASES;
    ///
    ///     async fn call(&self, mut rpc: Rpc) {
    ///         if rpc.metadata().get("authorization") != Some(self.token.as_str()) {
    ///             return rpc.reject(Status::unauthenticated("bad or missing token"));
    ///         }
    ///         rpc.metadata_mut().remove("authorization");
    ///         self.inner.call(rpc).await;
    ///     }
    /// }
    /// ```
    pub fn reject(mut self, status: Status) {
        send_trailers_only(&mut self.respond, status, &Metadata::new());
    }

    /// Serve a unary method: one request message, one response message.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.unary(|req: Request<HelloRequest>| async move {
    ///     let mut reply = HelloReply::new();
    ///     reply.set_message(req.get_ref().name());
    ///     Ok::<_, Status>(Response::new(reply))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn unary<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default,
        Resp: Serialize,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel,
            path,
            gzip_level,
            deadline,
            timeout,
            peer_timeout,
            rpc_timeout,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
                Ok(response) => {
                    send_unary_response(response, respond, wire, prefer_gzip, peer_accepts_gzip)
                        .await
                }
            }
        })
        .await;
    }

    /// Serve a client-streaming method: many request messages, one response.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.client_streaming(|req: Request<Streaming<HelloRequest>>| async move {
    ///     let mut inbound = req.into_inner();
    ///     while inbound.message().await?.is_some() {}
    ///     Ok::<_, Status>(Response::new(HelloReply::new()))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn client_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default + Send + 'static,
        Resp: Serialize,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel,
            path,
            gzip_level,
            deadline,
            timeout,
            peer_timeout,
            rpc_timeout,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
                Ok(response) => {
                    send_unary_response(response, respond, wire, prefer_gzip, peer_accepts_gzip)
                        .await
                }
            }
        })
        .await;
    }

    /// Serve a server-streaming method: one request message, many responses.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// Spawn the producer before returning the stream. A client RST while
    /// drain waits for the next message aborts the drain so
    /// [`Request::cancelled`] and [`crate::StreamSender::closed`] resolve
    /// without another send.
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.server_streaming(|req: Request<HelloRequest>| async move {
    ///     let (tx, stream) = Streaming::channel(8);
    ///     let mut reply = HelloReply::new();
    ///     reply.set_message(req.get_ref().name());
    ///     tx.send(reply).await.ok();
    ///     Ok::<_, Status>(Response::new(stream))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn server_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default,
        Resp: Serialize + Send,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel,
            path,
            gzip_level,
            timeout,
            peer_timeout,
            rpc_timeout,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
                Ok(response) => {
                    send_stream_response(
                        response,
                        respond,
                        wire,
                        deadline,
                        prefer_gzip,
                        peer_accepts_gzip,
                    )
                    .await
                }
            }
        })
        .await;
    }

    /// Serve a bidirectional-streaming method.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.bidi_streaming(|req: Request<Streaming<HelloRequest>>| async move {
    ///     let (tx, outbound) = Streaming::channel(8);
    ///     let mut inbound = req.into_inner();
    ///     while let Some(msg) = inbound.message().await? {
    ///         let mut reply = HelloReply::new();
    ///         reply.set_message(msg.name());
    ///         if tx.send(reply).await.is_err() {
    ///             break;
    ///         }
    ///     }
    ///     Ok::<_, Status>(Response::new(outbound))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn bidi_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default + Send + 'static,
        Resp: Serialize + Send,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel,
            path,
            gzip_level,
            timeout,
            peer_timeout,
            rpc_timeout,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
                Ok(response) => {
                    send_stream_response(
                        response,
                        respond,
                        wire,
                        deadline,
                        prefer_gzip,
                        peer_accepts_gzip,
                    )
                    .await
                }
            }
        })
        .await;
    }

    /// Read the single request message, then run `handler` under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_unary_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: Parse + Default,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let timeout = self.effective_timeout();
        let authority = self.authority().map(str::to_owned);
        let scheme = self.scheme().map(str::to_owned);
        let path = Some(self.path().to_owned());
        let peer_timeout = self.peer_timeout();
        let rpc_timeout = self.rpc_timeout();
        let peer_accepts_gzip = self.accepts_gzip();
        let encoding = self.encoding().map(str::to_owned);
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            peer_identity,
            peer_cred,
            transport_scheme: _,
            extensions,
            metadata,
            timeout: _,
            response_interceptor: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let prefer_gzip = config.compresses_outbound();
        let mut recv = request.into_body();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let on_reset = cancel_tx.clone();
        let outcome = wrap_timeout(timeout, async {
            let framed =
                read_one_message::<Req>(&mut recv, limits, config.accepts_compressed()).await?;
            let mut req = Request::from_metadata(
                framed.message,
                metadata,
                remote_addr,
                local_addr,
                peer_identity,
            )
            .with_extensions(extensions)
            .with_http(authority, scheme, path.clone());
            req.set_compressed(framed.compressed);
            req.set_peer_cred(peer_cred);
            req.set_limits(limits);
            req.set_peer_timeout(peer_timeout);
            req.set_rpc_timeout(rpc_timeout);
            req.set_accepts_gzip(peer_accepts_gzip);
            req.set_compresses_outbound(prefer_gzip);
            req.set_gzip_level(config.gzip_level());
            req.set_accepts_compressed(config.accepts_compressed());
            req.set_concurrent_rpc_limit(config.concurrent_rpc_limit());
            req.set_send_buffer_size(config.send_buffer_size());
            req.set_encoding(encoding);
            req.set_cancel(cancel_rx);
            if let Some(d) = timeout {
                req.set_timeout(d);
            }
            if let Some(at) = deadline {
                req.set_deadline(at);
            }
            run_handler(&mut respond, on_reset, handler(req)).await
        })
        .await;
        notify_deadline(&outcome, &cancel_tx);
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel: CancelOnDrop(cancel_tx),
            path,
            gzip_level: config.gzip_level(),
            timeout,
            peer_timeout,
            rpc_timeout,
        })
    }

    /// Hand the request stream to `handler`, under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_streaming_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: Parse + Default + Send + 'static,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let timeout = self.effective_timeout();
        let authority = self.authority().map(str::to_owned);
        let scheme = self.scheme().map(str::to_owned);
        let path = Some(self.path().to_owned());
        let peer_timeout = self.peer_timeout();
        let rpc_timeout = self.rpc_timeout();
        let peer_accepts_gzip = self.accepts_gzip();
        let encoding = self.encoding().map(str::to_owned);
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            peer_identity,
            peer_cred,
            transport_scheme: _,
            extensions,
            metadata,
            timeout: _,
            response_interceptor: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let prefer_gzip = config.compresses_outbound();
        let recv = request.into_body();
        // Decoded on the handler's task: no pump task, no queue, and reading
        // is what releases HTTP/2 capacity.
        let stream = Streaming::from_wire(WireStream::<Req>::new(
            recv,
            limits,
            deadline,
            config.accepts_compressed(),
        ));
        let mut req =
            Request::from_metadata(stream, metadata, remote_addr, local_addr, peer_identity)
                .with_extensions(extensions)
                .with_http(authority, scheme, path.clone());
        req.set_peer_cred(peer_cred);
        req.set_limits(limits);
        req.set_peer_timeout(peer_timeout);
        req.set_rpc_timeout(rpc_timeout);
        req.set_accepts_gzip(peer_accepts_gzip);
        req.set_compresses_outbound(prefer_gzip);
        req.set_gzip_level(config.gzip_level());
        req.set_accepts_compressed(config.accepts_compressed());
        req.set_concurrent_rpc_limit(config.concurrent_rpc_limit());
        req.set_send_buffer_size(config.send_buffer_size());
        req.set_encoding(encoding);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        if let Some(at) = deadline {
            req.set_deadline(at);
        }
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let on_reset = cancel_tx.clone();
        req.set_cancel(cancel_rx);
        let outcome = wrap_timeout(timeout, async {
            run_handler(&mut respond, on_reset, handler(req)).await
        })
        .await;
        notify_deadline(&outcome, &cancel_tx);
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            cancel: CancelOnDrop(cancel_tx),
            path,
            gzip_level: config.gzip_level(),
            timeout,
            peer_timeout,
            rpc_timeout,
        })
    }
}

/// Wake spawned work as soon as the server deadline wins, not after trailers.
fn notify_deadline<T>(outcome: &Result<T, Status>, cancel: &watch::Sender<bool>) {
    if matches!(outcome, Err(s) if s.code() == Code::DeadlineExceeded) {
        cancel.send(true).ok();
    }
}

/// Resolve when the client `RST_STREAM`s this RPC.
///
/// Unary handlers that have already read the request (and streaming handlers
/// that are not currently reading) would otherwise run to completion after
/// the caller has gone. `SendResponse::poll_reset` sees the reset without
/// needing the request body.
async fn wait_client_reset(respond: &mut h2::server::SendResponse<Bytes>) -> Status {
    drop(std::future::poll_fn(|cx| respond.poll_reset(cx)).await);
    Status::cancelled()
}

/// Race the handler against a client reset, signalling spawned work on RST.
///
/// After signalling, poll the handler once so a body awaiting
/// [`Request::cancelled`] can finish. A handler that ignores cancel stays
/// `Pending` and is dropped, the same as before.
async fn run_handler<T>(
    respond: &mut h2::server::SendResponse<Bytes>,
    on_reset: watch::Sender<bool>,
    handler: impl Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    tokio::pin!(handler);
    tokio::select! {
        biased;
        result = &mut handler => result,
        gone = wait_client_reset(respond) => {
            on_reset.send(true).ok();
            match poll_fn(|cx| Poll::Ready(handler.as_mut().poll(cx))).await {
                Poll::Ready(result) => result,
                Poll::Pending => Err(gone),
            }
        }
    }
}

/// Marks [`Request::cancelled`] when this RPC is fully written or rejected.
///
/// Lives until the response is on the wire so a streaming producer spawned
/// before the handler returns is not cancelled at return — only when the
/// stream drains, the client resets, or the deadline fires.
struct CancelOnDrop(watch::Sender<bool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.send(true).ok();
    }
}

/// Keep [`CancelOnDrop`] alive across `write`.
///
/// Locals not named in the write future can be dropped at `.await` (NLL).
/// A manual future holds the guard as a field so it cannot drop until `write`
/// completes — `write.await; drop(cancel)` is not enough.
fn hold_cancel<F: Future<Output = ()>>(cancel: CancelOnDrop, write: F) -> HoldCancel<F> {
    HoldCancel {
        write: Box::pin(write),
        cancel: Some(cancel),
    }
}

/// [`hold_cancel`]'s state: poll `write`, drop the guard only when it finishes.
struct HoldCancel<F> {
    cancel: Option<CancelOnDrop>,
    write: Pin<Box<F>>,
}

impl<F: Future<Output = ()>> Future for HoldCancel<F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.write.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.cancel.take();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A handler result plus the response channel it still has to be written to.
struct Prepared<T> {
    respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    /// The RPC's deadline, shared by the handler, the inbound stream, and the
    /// response writer, so no stage can outlive it.
    deadline: Option<tokio::time::Instant>,
    outcome: Result<T, Status>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    cancel: CancelOnDrop,
    /// Kernel-stamped onto the handler [`Response`] before `on_response`.
    path: Option<String>,
    gzip_level: u32,
    timeout: Option<Duration>,
    peer_timeout: Option<Duration>,
    rpc_timeout: Option<Duration>,
}

async fn send_unary_response<Resp: Serialize>(
    response: Response<Resp>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
) {
    let (msg, headers, trailers, compress) = response.split();
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let frame = match encode_msg(&msg, gzip, wire.limits, wire.gzip_level) {
        Ok(frame) => frame,
        Err(status) => {
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip, wire.accept_gzip) else {
        return;
    };
    send_bytes(&mut send, frame, false, wire.send_buffer)
        .await
        .ok();
    let mut status = Status::new(Code::Ok, "");
    *status.metadata_mut() = trailers;
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

async fn send_stream_response<Resp: Serialize + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
) {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip, wire.accept_gzip) else {
        return;
    };
    let mut status = Status::from_code(Code::Ok);
    *status.metadata_mut() = trailers;
    // The deadline has to cover the whole response, not just the handler
    // future: a producer that stops early because *its* deadline expired must
    // not be reported as a clean end of stream.
    let drained = match deadline {
        None => {
            drain_to_wire(
                &mut stream,
                &mut send,
                wire,
                compress,
                prefer_gzip,
                peer_accepts_gzip,
            )
            .await
        }
        Some(at) => tokio::time::timeout_at(
            at,
            drain_to_wire(
                &mut stream,
                &mut send,
                wire,
                compress,
                prefer_gzip,
                peer_accepts_gzip,
            ),
        )
        .await
        .unwrap_or_else(|_| Err(DrainError::Producer(Status::deadline_exceeded()))),
    };
    if let Err(err) = drained {
        // A transport failure cannot be reported; a producer failure becomes
        // the stream's trailing status.
        match err {
            DrainError::Transport => return,
            DrainError::Producer(producer) => status = producer,
        }
    }
    // If the deadline elapsed, the RPC did not finish in time, however the
    // drain ended. A handler reading its request stream sees the deadline as an
    // error on the read and will usually just stop producing, which would
    // otherwise be indistinguishable from a clean end of stream.
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

/// Why a stream stopped before its clean end.
enum DrainError {
    /// The wire is gone, so no status can be delivered.
    Transport,
    /// The handler ended the stream with a status.
    Producer(Status),
}

/// Copy every message from `stream` onto `send`, batching each burst.
async fn drain_to_wire<Resp: Serialize + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut h2::SendStream<Bytes>,
    wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
) -> Result<(), DrainError> {
    let mut batch = OutBatch::new(wire);
    let mut items = Vec::with_capacity(OutBatch::BURST);
    loop {
        items.clear();
        // A client RST while we wait for the next message must abort: a
        // producer that is itself waiting (Health Watch, a timer) will not
        // send, so the write path would never see the reset.
        let n = tokio::select! {
            biased;
            reset = poll_fn(|cx| send.poll_reset(cx)) => {
                drop(reset);
                return Err(DrainError::Transport);
            }
            n = stream.recv_many(&mut items, OutBatch::BURST) => n,
        };
        if n == 0 {
            break;
        }
        // More than one message queued means the producer is running ahead of
        // the network and is bounded by its channel depth, so one scheduling
        // turn lets it top the queue up and doubles the write size. Exactly one
        // means it is not ahead — a request/response stream, say — and must not
        // pay a turn of latency for nothing.
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 {
            let_producer_catch_up().await;
            stream.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let mut item = item.map_err(DrainError::Producer)?;
            item.compressed =
                gzip_stream_frame(item.compressed, envelope, prefer_gzip, peer_accepts_gzip);
            if let Err(status) = batch.encode(item) {
                return Err(DrainError::Producer(status));
            }
            if batch.is_full() {
                batch.flush(send).await.map_err(|_| DrainError::Transport)?;
            }
        }
        if !batch.is_full() {
            batch.flush(send).await.map_err(|_| DrainError::Transport)?;
        }
    }
    batch.flush(send).await.map_err(|_| DrainError::Transport)
}

/// Split `/service/method` without allocating. Unparseable paths yield empty
/// halves, which route to `UNIMPLEMENTED`.
pub(crate) fn split_path(path: &str) -> (&str, &str) {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match rest.rsplit_once('/') {
        Some((service, method)) => (service, method),
        None => ("", ""),
    }
}

/// Serves exactly one [`Service`], with no per-RPC dynamic dispatch.
///
/// A hand-written [`Service`] is first-class. Unknown methods are
/// [`crate::Code::Unimplemented`] on every call shape, including over TLS,
/// mTLS, Unix, and [`Server::serve_connection`].
///
/// ```no_run
/// use pbrs_grpc::Server;
/// # use pbrs_grpc::{Rpc, Service};
/// # struct Echo;
/// # impl Service for Echo {
/// #     const NAME: &'static str = "demo.Echo";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// Server::new(Echo)
///     .max_concurrent_streams(1024)
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await
/// # }
/// ```
pub struct Server<S> {
    service: Arc<S>,
    config: ServerConfig,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
    response_interceptor: Option<crate::interceptor::ResponseHook>,
}

impl<S> Clone for Server<S> {
    fn clone(&self) -> Self {
        Self {
            service: Arc::clone(&self.service),
            config: self.config,
            interceptor: self.interceptor.clone(),
            response_interceptor: self.response_interceptor.clone(),
        }
    }
}

impl<S: Service> std::fmt::Debug for Server<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("service", &S::NAME)
            .field("config", &self.config)
            .field("interceptors", &self.interceptor.is_some())
            .field(
                "response_interceptors",
                &self.response_interceptor.is_some(),
            )
            .finish()
    }
}

impl<S: Service> Server<S> {
    /// Wrap an existing `Arc` without adding another layer.
    #[must_use]
    pub fn from_arc(service: Arc<S>) -> Self {
        Self {
            service,
            config: ServerConfig::default(),
            interceptor: None,
            response_interceptor: None,
        }
    }

    /// Take the inner `Arc` back.
    #[must_use]
    pub fn into_inner(self) -> Arc<S> {
        self.service
    }

    /// Serve `service` with default configuration.
    #[must_use]
    pub fn new(service: S) -> Self {
        Self {
            service: Arc::new(service),
            config: ServerConfig::default(),
            interceptor: None,
            response_interceptor: None,
        }
    }

    /// Replace the transport and limit configuration. Applies to every call
    /// shape.
    #[must_use]
    pub fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// The configuration in effect. Applies to every call shape.
    ///
    /// Distinct from [`Self::config`], which replaces it. Same snapshot a
    /// [`crate::Channel::config`] getter returns on the client.
    #[must_use]
    pub fn server_config(&self) -> ServerConfig {
        self.config
    }

    /// Cap inbound messages at `limit` bytes. Default 4 MiB.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_decoding_message_size(limit);
        self
    }

    /// Cap outbound messages at `limit` bytes. Default unlimited.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_encoding_message_size(limit);
        self
    }

    /// Replace both message caps at once. Applies to every call shape.
    /// See [`ServerConfig::message_limits`].
    /// Distinct from [`Self::max_decoding_message_size`] /
    /// [`Self::max_encoding_message_size`]. Oversize inbound or outbound
    /// is [`Code::ResourceExhausted`], including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`].
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.config = self.config.message_limits(limits);
        self
    }

    /// Configured message caps. See [`Self::message_limits`].
    /// Applies to every call shape.
    /// Distinct from [`Self::message_limits`], which sets them.
    /// Distinct from [`Self::send_buffer_size`]: that is the HTTP/2 send buffer, not uncompressed protobuf bytes.
    /// Same overlay as [`crate::Rpc::limits`].
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.config.limits()
    }

    /// Cap how many RPCs the process will run at once.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. See [`ServerConfig::max_concurrent_rpcs`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_rpcs(n);
        self
    }

    /// Configured process-wide RPC cap, if any. See [`Self::max_concurrent_rpcs`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_concurrent_rpcs`], which sets it.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.config.concurrent_rpc_limit()
    }

    /// Cap how many TCP/Unix connections the accept loop will serve at once,
    /// including TLS and mTLS listeners. Applies to every call shape. See
    /// [`ServerConfig::max_concurrent_connections`].
    #[must_use]
    pub fn max_concurrent_connections(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_connections(n);
        self
    }

    /// Concurrent RPCs allowed per HTTP/2 connection. Applies to every call
    /// shape. See [`ServerConfig::max_concurrent_streams`].
    /// HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`. Distinct from
    /// [`Self::max_concurrent_rpcs`], which refuses extras as
    /// [`Code::ResourceExhausted`]. A well-behaved client waits; both RPCs
    /// still complete, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.config = self.config.max_concurrent_streams(streams);
        self
    }

    /// HTTP/2 per-stream receive window. Applies to every call shape.
    /// See [`ServerConfig::initial_stream_window_size`].
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.config = self.config.initial_stream_window_size(bytes);
        self
    }

    /// HTTP/2 per-connection receive window. Applies to every call shape.
    /// See [`ServerConfig::initial_connection_window_size`].
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.config = self.config.initial_connection_window_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Applies to every call shape.
    /// See [`ServerConfig::max_frame_size`].
    /// A well-behaved client splits DATA; every call shape still completes,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct
    /// from [`Self::max_header_list_size`], which refuses oversize metadata,
    /// and from [`Self::max_concurrent_streams`], which serializes extra RPCs.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.config = self.config.max_frame_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. Applies to every call shape.
    /// See [`ServerConfig::max_header_list_size`].
    /// Oversize metadata is refused, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. Distinct from a raw HTTP/2 peer.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.config = self.config.max_header_list_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic table). Default 4096.
    /// Applies to every call shape. See [`ServerConfig::header_table_size`].
    /// A well-behaved client still completes every call shape at this table
    /// size, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Distinct from
    /// [`Self::max_header_list_size`], which caps uncompressed header-block
    /// bytes (`SETTINGS_MAX_HEADER_LIST_SIZE`).
    #[must_use]
    pub fn header_table_size(mut self, bytes: u32) -> Self {
        self.config = self.config.header_table_size(bytes);
        self
    }

    /// HTTP/2 small-DATA framing budget. Default 25600.
    /// Applies to every call shape. See [`ServerConfig::data_frame_budget`].
    /// Caps extra memory from tiny DATA frames. Exceeding this is
    /// `ENHANCE_YOUR_CALM` (`too_many_data_frames`). Distinct from
    /// [`Self::initial_connection_window_size`], which is flow-control bytes,
    /// and from [`Self::max_frame_size`], which caps one DATA payload.
    /// h2 Auto (half the connection window) is not exposed.
    /// A well-behaved client still completes every call shape at this framing
    /// budget, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn data_frame_budget(mut self, bytes: usize) -> Self {
        self.config = self.config.data_frame_budget(bytes);
        self
    }

    /// Per-connection HTTP/2 send buffer. Applies to every call shape.
    /// See [`ServerConfig::max_send_buffer_size`].
    /// Write backpressure still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::initial_stream_window_size`], which still
    /// serves at a small receive window.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_send_buffer_size(bytes);
        self
    }

    /// Configured write-time HTTP/2 send buffer. See [`Self::max_send_buffer_size`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_send_buffer_size`], which sets it.
    /// Distinct from [`Self::message_limits`]: that is uncompressed protobuf bytes, not this send buffer.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.config.send_buffer_size()
    }

    /// Cap remotely-reset HTTP/2 streams waiting in the accept queue.
    /// Applies to every call shape. See
    /// [`ServerConfig::max_pending_accept_reset_streams`].
    /// A well-behaved client never fills that queue; every call shape still
    /// completes, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Distinct from a raw HTTP/2 peer.
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_pending_accept_reset_streams(n);
        self
    }

    /// Cap locally-reset HTTP/2 streams caused by a peer protocol error.
    /// Applies to every call shape. See
    /// [`ServerConfig::max_local_error_reset_streams`].
    /// Exceeding this is `ENHANCE_YOUR_CALM`. Distinct from
    /// [`Self::max_pending_accept_reset_streams`]: that caps remotely-reset
    /// streams (rapid reset). This caps RSTs we send after an invalid frame.
    /// A well-behaved client never triggers one; every call shape still
    /// completes, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_local_error_reset_streams(n);
        self
    }

    /// Cap remembered locally-reset HTTP/2 stream IDs.
    /// Default 50. Applies to every call shape. See
    /// [`ServerConfig::max_concurrent_reset_streams`].
    /// When the cap is reached, the oldest ID is purged from memory, not
    /// `ENHANCE_YOUR_CALM`. Frames on a purged ID are a connection
    /// `PROTOCOL_ERROR`. Distinct from
    /// [`Self::max_pending_accept_reset_streams`] (rapid-reset GOAWAY) and
    /// [`Self::max_local_error_reset_streams`] (protocol-error RST GOAWAY).
    /// A well-behaved client still completes every call shape at this memory cap,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn max_concurrent_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_reset_streams(n);
        self
    }

    /// How long locally-reset HTTP/2 stream IDs are remembered.
    /// Default 1 s. Applies to every call shape. See
    /// [`ServerConfig::reset_stream_duration`].
    /// After this duration the ID is forgotten, not `ENHANCE_YOUR_CALM`.
    /// Frames on a forgotten ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`Self::max_concurrent_reset_streams`], which is how many
    /// IDs are remembered (count). This is how long (time).
    /// A well-behaved client still completes every call shape at this reset duration,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn reset_stream_duration(mut self, dur: Duration) -> Self {
        self.config = self.config.reset_stream_duration(dur);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`. Applies to
    /// every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. See [`ServerConfig::timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// gzip responses when the client advertises gzip. Applies to every call
    /// shape, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// See [`ServerConfig::send_compressed`].
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// Deflate effort for outbound gzip. Default 1 (`flate2` fast).
    /// Applies to every call shape. See
    /// [`ServerConfig::gzip_compression_level`].
    /// Distinct from [`Self::send_compressed`], which is on or off.
    /// 0 stores; 9 is best. A well-behaved client still completes every
    /// call shape, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn gzip_compression_level(mut self, level: u32) -> Self {
        self.config = self.config.gzip_compression_level(level);
        self
    }

    /// Inflate inbound gzip. Default `true`. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Passing `false` refuses `grpc-encoding: gzip` as
    /// [`Code::Unimplemented`] before the handler runs. Distinct from
    /// [`Self::send_compressed`], which is outbound. See
    /// [`ServerConfig::accept_compressed`].
    #[must_use]
    pub fn accept_compressed(mut self, accept: bool) -> Self {
        self.config = self.config.accept_compressed(accept);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`.
    /// Applies to every call shape.
    /// Distinct from [`Self::timeout`], which sets it.
    /// Interceptors and handlers read the same overlay on [`Rpc::rpc_timeout`]
    /// / [`Request::rpc_timeout`].
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.config.rpc_timeout()
    }

    /// Whether responses are gzipped when the client accepts gzip.
    /// Applies to every call shape.
    /// Distinct from [`Self::send_compressed`], which enables it.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.config.compresses_outbound()
    }

    /// Configured outbound gzip deflate level. See [`Self::gzip_compression_level`].
    /// Applies to every call shape.
    /// Distinct from [`Self::gzip_compression_level`], which sets it.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.config.gzip_level()
    }

    /// Whether inbound gzip is inflated. Default `true`.
    /// Applies to every call shape.
    /// Distinct from [`Self::accept_compressed`], which sets it.
    /// Distinct from [`Rpc::accepts_gzip`], which is the peer's
    /// `grpc-accept-encoding`.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.config.accepts_compressed()
    }

    /// HTTP/2 PING keepalive. Applies to every call shape.
    /// See [`ServerConfig::keep_alive_interval`].
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.config = self.config.keep_alive_interval(interval);
        self
    }

    /// How long to wait for a PING acknowledgement. Applies to every call
    /// shape. See [`ServerConfig::keep_alive_timeout`].
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.keep_alive_timeout(timeout);
        self
    }

    /// TCP `SO_KEEPALIVE`. Applies to every call shape.
    /// See [`ServerConfig::tcp_keepalive`].
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.config = self.config.tcp_keepalive(time);
        self
    }

    /// Send GOAWAY this long after accept. The next RPC of every call shape
    /// redials, including over TLS, mTLS, and Unix; transparent retry of the
    /// same in-flight RPC is unary and server-streaming after request bytes,
    /// client-streaming and bidi before HEADERS. See
    /// [`ServerConfig::max_connection_age`].
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.config = self.config.max_connection_age(age);
        self
    }

    /// Send GOAWAY after this long with no outstanding RPCs. The next RPC of
    /// every call shape redials, including over TLS, mTLS, and Unix. See
    /// [`ServerConfig::max_connection_idle`].
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.config = self.config.max_connection_idle(idle);
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Applies to every call shape. See [`ServerConfig::max_connection_age_grace`].
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.config = self.config.max_connection_age_grace(grace);
        self
    }

    /// Drop a client that never finishes TLS or the HTTP/2 preface.
    /// Applies to every call shape, including over TLS, mTLS, and Unix. See
    /// [`ServerConfig::handshake_timeout`].
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.handshake_timeout(timeout);
        self
    }

    /// Run `interceptor` before this service sees any RPC.
    ///
    /// Closures implement [`crate::Interceptor`], so
    /// `server.intercept(|rpc| { ... })` is the usual form. The interceptor
    /// can mutate [`Rpc::metadata_mut`], cap the deadline with
    /// [`Rpc::set_timeout`], inspect [`Rpc::path`] / [`Rpc::service`] /
    /// [`Rpc::method`] / [`Rpc::peer_timeout`] / [`Rpc::rpc_timeout`] /
    /// [`Rpc::effective_timeout`] / [`Rpc::authority`] / [`Rpc::scheme`] /
    /// [`Rpc::remote_addr`] / [`Rpc::local_addr`] / [`Rpc::peer_identity`] /
    /// [`Rpc::peer_cred`] / [`Rpc::limits`] / [`Rpc::accepts_gzip`] /
    /// [`Rpc::encoding`] / [`Rpc::compresses_outbound`] / [`Rpc::gzip_level`] /
    /// [`Rpc::accepts_compressed`] / [`Rpc::concurrent_rpc_limit`] /
    /// [`Rpc::send_buffer_size`],
    /// attach typed state on [`Rpc::extensions_mut`], or return `Err`
    /// (including [`Status::with_error_details`]) to reject before the body
    /// is read. Generated handlers see the same path, peer, caps, client
    /// timeout, server timeout overlay, gzip facts, response-gzip overlay,
    /// deflate effort, inbound-gzip overlay, process RPC cap, and write-time send buffer on [`Request`].
    /// Generated servers expose the same method:
    /// `GreeterServer::new(svc).intercept(auth).serve(addr)`.
    /// Calling this twice stacks: the first interceptor runs first, matching
    /// [`Router::intercept`] and [`crate::Channel::intercept`]. A single
    /// interceptor still rejects before the handler on every call shape,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// On a [`Router`], call [`Router::intercept`] to cover every mounted
    /// service, or wrap one service with [`crate::Intercepted`].
    /// [`Status::from_error_details`] is the typed bag after this Server intercept Err; those trailers reach the client without reading the body.
    /// Distinct from a handler Err: that is after the handler ran; this Server intercept Err is trailers without reading the body.
    /// Distinct from a Server on_response Err: that is trailers-only after handler Ok; this Server intercept Err is trailers without reading the body.
    /// Distinct from an Intercepted on_response Err: that is trailers-only after handler Ok; this Server intercept Err is trailers without reading the body.
    /// Distinct from a Router on_response Err: that is trailers-only after handler Ok; this Server intercept Err is trailers without reading the body.
    /// Distinct from a ServiceExt on_response Err: that is trailers-only after handler Ok; this Server intercept Err is trailers without reading the body.
    /// Distinct from a Channel on_response Err: that fails the Call after a successful receive; this Server intercept Err is trailers without reading the body.
    /// Distinct from a ResponseInterceptor Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Server intercept Err is trailers without reading the body.
    /// Distinct from a method-level on_response Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Server intercept Err is trailers without reading the body.
    /// Distinct from a Channel intercept Err: that is a local reject never opens a stream; this Server intercept Err is trailers without reading the body.
    /// Distinct from a ClientInterceptor Err: that is a local reject never opens a stream; this Server intercept Err is trailers without reading the body.
    /// Distinct from a method-level intercept Err: that is a local reject never opens a stream; this Server intercept Err is trailers without reading the body.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Server intercept Err is trailers without reading the body.
    /// Distinct from [`crate::Channel::intercept`]: that runs on the outbound call before the stream opens; this runs on the inbound RPC before the handler.
    /// Distinct from [`crate::Channel::intercept`]: that runs on the outbound call before the stream opens; this Server intercept runs on the inbound RPC before the handler.
    /// Distinct from [`Self::on_response`]: that runs after the handler returns Ok; this runs on the inbound RPC before the handler.
    /// Distinct from [`Self::on_response`]: that runs after the handler returns Ok; this Server intercept runs on the inbound RPC before the handler.
    /// Distinct from [`Router::intercept`]: that runs on the inbound RPC before every mounted service on that Router; this Server intercept runs on the inbound RPC before the Server's Service.
    ///
    /// ```
    /// # fn demo<S: pbrs_grpc::Service>(server: pbrs_grpc::Server<S>) -> pbrs_grpc::Server<S> {
    /// server.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = (
    ///         rpc.path(),
    ///         rpc.service(),
    ///         rpc.method(),
    ///         rpc.metadata(),
    ///         rpc.timeout(),
    ///         rpc.peer_timeout(),
    ///         rpc.rpc_timeout(),
    ///         rpc.effective_timeout(),
    ///         rpc.deadline(),
    ///         rpc.accepts_gzip(),
    ///         rpc.encoding(),
    ///         rpc.compresses_outbound(),
    ///         rpc.gzip_level(),
    ///         rpc.accepts_compressed(),
    ///         rpc.concurrent_rpc_limit(),
    ///         rpc.send_buffer_size(),
    ///         rpc.limits(),
    ///         rpc.local_addr(),
    ///         rpc.remote_addr(),
    ///         rpc.peer_identity(),
    ///         rpc.peer_cred(),
    ///         rpc.authority(),
    ///         rpc.scheme(),
    ///         rpc.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn intercept<I: crate::Interceptor>(mut self, interceptor: I) -> Self {
        self.interceptor = Some(match self.interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::Then::new(prev, interceptor)),
        });
        self
    }

    /// Run `interceptor` after the handler returns `Ok`.
    ///
    /// Closures implement [`crate::ResponseInterceptor`], so
    /// `server.on_response(|parts| { ... })` is the usual form. The hook
    /// sees [`crate::ResponseParts`]: headers, trailers, compress, and local
    /// [`crate::Response::extensions`]. Those extensions are not on the
    /// wire; stamp [`crate::ResponseParts::metadata_mut`] to send a header.
    /// Calling this twice stacks: the first interceptor runs first, matching
    /// [`Self::intercept`]. Applies to every call shape, including over TLS,
    /// mTLS, Unix, and [`Self::serve_connection`].
    /// `Err` after the handler already ran; that status is sent trailers-only
    /// instead of the response, including [`Status::with_error_details`].
    /// A handler `Err` skips this hook.
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// Distinct from [`crate::Request::path`]: that is the inbound request.
    /// [`crate::ResponseParts::gzip_level`] is the server encode overlay.
    /// Distinct from [`crate::ResponseParts::compress`]: that is on or off.
    /// [`crate::ResponseParts::compresses_outbound`] is the server encode overlay.
    /// Distinct from [`crate::ResponseParts::compress`]: that is the per-RPC Compressed-Flag.
    /// [`crate::ResponseParts::accepts_gzip`] is the peer `grpc-accept-encoding` advertisement.
    /// Distinct from [`crate::ResponseParts::encoding`]: that is received `grpc-encoding`.
    /// [`crate::ResponseParts::deadline`] is kernel-stamped when writing.
    /// Distinct from [`crate::Request::deadline`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::deadline`]: that is computed when that getter runs.
    /// [`crate::ResponseParts::timeout`] is the duration stamped at dispatch.
    /// Distinct from [`crate::ResponseParts::deadline`]: that is the Instant.
    /// [`crate::ResponseParts::limits`] is the encode cap when writing.
    /// Distinct from [`crate::Request::limits`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::limits`]: that is a server interceptor before the handler.
    /// [`crate::ResponseParts::peer_timeout`] is the client's `grpc-timeout`.
    /// Distinct from [`crate::ResponseParts::timeout`]: that is the effective cap.
    /// [`crate::ResponseParts::rpc_timeout`] is the server overlay.
    /// Distinct from [`crate::ResponseParts::timeout`]: that is soonest-of-three, not the overlay.
    /// Distinct from [`crate::ResponseParts::peer_timeout`]: that is the client's `grpc-timeout`.
    /// [`crate::ResponseParts::accepts_compressed`] is the inbound gzip overlay.
    /// Distinct from [`crate::ResponseParts::accepts_gzip`]: that is the peer advertisement.
    /// [`crate::ResponseParts::send_buffer_size`] is the write-time HTTP/2 send buffer overlay.
    /// Distinct from [`crate::ResponseParts::limits`]: that is the encode cap, not this send buffer.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy after this Server on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay after this Server on_response.
    /// [`Status::from_error_details`] is the typed bag after this Server on_response Err; a local reject is trailers-only after handler Ok.
    /// Distinct from a handler Err: that is after the handler ran; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a Server intercept Err: that is trailers without reading the body; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from an Interceptor Err: that is trailers without reading the body; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level Interceptor Err: that is trailers without reading the body; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a Router intercept Err: that is trailers without reading the body; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a ServiceExt intercept Err: that is trailers without reading the body; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a ResponseInterceptor Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level on_response Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a Channel on_response Err: that fails the Call after a successful receive; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a ClientInterceptor Err: that is a local reject never opens a stream; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a Channel intercept Err: that is a local reject never opens a stream; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level intercept Err: that is a local reject never opens a stream; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Server on_response Err is trailers-only after handler Ok.
    /// Distinct from [`Self::intercept`]: that runs on the inbound RPC before the handler; this runs after the handler returns Ok.
    /// Distinct from [`Self::intercept`]: that runs on the inbound RPC before the handler; this Server on_response runs after the handler returns Ok.
    /// Distinct from [`Router::on_response`]: that runs after the handler returns Ok on every mounted service on that Router; this Server on_response runs after the handler returns Ok on the Server's Service.
    /// Generated servers expose the same method:
    /// `GreeterServer::new(svc).on_response(stamp).serve(addr)`.
    /// On a [`Router`], call [`Router::on_response`] to cover every mounted
    /// service.
    ///
    /// ```
    /// # fn demo<S: pbrs_grpc::Service>(server: pbrs_grpc::Server<S>) -> pbrs_grpc::Server<S> {
    /// server.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = (
    ///         parts.path(),
    ///         parts.service(),
    ///         parts.method(),
    ///         parts.metadata(),
    ///         parts.trailers(),
    ///         parts.compress(),
    ///         parts.compress_is_set(),
    ///         parts.encoding(),
    ///         parts.gzip_level(),
    ///         parts.compresses_outbound(),
    ///         parts.accepts_gzip(),
    ///         parts.deadline(),
    ///         parts.timeout(),
    ///         parts.limits(),
    ///         parts.peer_timeout(),
    ///         parts.rpc_timeout(),
    ///         parts.accepts_compressed(),
    ///         parts.send_buffer_size(),
    ///         parts.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn on_response<I: crate::ResponseInterceptor>(mut self, interceptor: I) -> Self {
        self.response_interceptor = Some(match self.response_interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::ResponseThen::new(prev, interceptor)),
        });
        self
    }

    fn into_single(self) -> (Single<S>, ServerConfig) {
        (
            Single {
                service: self.service,
                interceptor: self.interceptor,
                response_interceptor: self.response_interceptor,
            },
            self.config,
        )
    }

    /// Add a second service, switching to path-based routing.
    ///
    /// [`Self::max_decoding_message_size`] and
    /// [`Self::max_encoding_message_size`] stay in effect on every mounted
    /// service, on every call shape of those mounts, including over TLS, mTLS,
    /// Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn add_service<T: Service>(self, service: T) -> Router {
        self.into_router().add_service(service)
    }

    /// Mount `service` when `Some`. `None` is a no-op.
    /// Applies to every call shape.
    ///
    /// Distinct from [`Self::add_service`], which always mounts.
    /// `None` does not replace a service already there.
    /// Services that stay mounted still complete every call shape, including
    /// over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn add_optional_service<T: Service>(self, service: Option<T>) -> Router {
        match service {
            Some(service) => self.add_service(service),
            None => self.into_router(),
        }
    }

    /// Move this service into a [`Router`], keeping the configuration and any
    /// interceptors.
    #[must_use]
    pub fn into_router(self) -> Router {
        let mut router = Router::new().config(self.config).add_arc(self.service);
        router.interceptor = self.interceptor;
        router.response_interceptor = self.response_interceptor;
        router
    }

    /// Bind `addr` and serve until the listener fails.
    /// Applies to every call shape.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Status> {
        self.serve_listener(bind(addr).await?).await
    }

    /// Serve on an existing listener until it fails.
    /// Applies to every call shape.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        self.serve_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve until `shutdown` resolves, then drain. Applies to every call
    /// shape. In-flight RPCs finish; new connections are refused. TLS and
    /// Unix drain the same way (`serve_tls_with_shutdown`,
    /// `serve_unix_with_shutdown`).
    ///
    /// `listener` must already be bound. Draining stops accepting, sends
    /// `GOAWAY` on every live connection, and waits for in-flight RPCs to
    /// finish. To bind an address and then drain, use
    /// [`Self::serve_until_shutdown`].
    pub async fn serve_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (dispatch, config) = self.into_single();
        accept_loop(Arc::new(dispatch), listener, config, shutdown, None).await
    }

    /// Bind `addr` and serve until `shutdown` resolves, then drain.
    /// Applies to every call shape.
    ///
    /// This is the address form of [`Self::serve_with_shutdown`].
    pub async fn serve_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_with_shutdown(bind(addr).await?, shutdown).await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails. Applies to every call shape.
    ///
    /// `path` must not already be bound. This does not unlink a leftover
    /// socket file; use [`Self::serve_unix_unlink`] after a crash. TLS over a
    /// Unix socket is not supported; use [`Self::serve_tls`] on TCP.
    /// Each RPC carries [`Rpc::peer_cred`] from `SO_PEERCRED` / `LOCAL_PEERCRED`.
    /// To bind and then drain on a signal, use [`Self::serve_unix_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], after unlinking a crash leftover.
    /// Applies to every call shape.
    ///
    /// A crash leaves a socket inode that is not accepting. This unlinks that
    /// leftover and binds. If another process is actually listening on `path`,
    /// the file is left alone and this returns [`Code::Unavailable`].
    /// To unlink, bind, and then drain on a signal, use
    /// [`Self::serve_unix_unlink_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path).await?)
            .await
    }

    /// Serve h2c on an existing Unix listener until it fails.
    /// Applies to every call shape.
    #[cfg(unix)]
    pub async fn serve_unix_listener(self, listener: UnixListener) -> Result<(), Status> {
        self.serve_unix_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve h2c on a Unix listener until `shutdown` resolves, then drain.
    /// Applies to every call shape. In-flight RPCs finish; new connections
    /// are refused. See [`Self::serve_with_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_with_shutdown(
        self,
        listener: UnixListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (dispatch, config) = self.into_single();
        accept_unix_loop(Arc::new(dispatch), listener, config, shutdown).await
    }

    /// Bind `path` and serve h2c until `shutdown` resolves, then drain.
    /// Applies to every call shape.
    ///
    /// This is the path form of [`Self::serve_unix_with_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix(path)?, shutdown)
            .await
    }

    /// [`Self::serve_unix_until_shutdown`], after unlinking a crash leftover.
    /// Applies to every call shape. A live listener is left alone. See
    /// [`Self::serve_unix_unlink`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix_unlink(path).await?, shutdown)
            .await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    ///
    /// To bind and then drain on a signal, use [`Self::serve_tls_until_shutdown`].
    /// `:scheme` is `https` on every call shape. mTLS fills
    /// [`Rpc::peer_identity`] on every call shape.
    pub async fn serve_tls(self, addr: SocketAddr, tls: ServerTls) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, std::future::pending(), tls)
            .await
    }

    /// Serve over TLS until `shutdown` resolves, then drain.
    /// Applies to every call shape, including mTLS. In-flight RPCs finish;
    /// new connections are refused.
    pub async fn serve_tls_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        let (dispatch, config) = self.into_single();
        accept_loop(Arc::new(dispatch), listener, config, shutdown, Some(tls)).await
    }

    /// Bind `addr` and serve over TLS until `shutdown` resolves, then drain.
    /// Applies to every call shape.
    ///
    /// This is the address form of [`Self::serve_tls_with_shutdown`].
    pub async fn serve_tls_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, shutdown, tls)
            .await
    }

    /// Serve a single already-accepted byte stream until it closes.
    /// Applies to every call shape.
    ///
    /// No accept loop, no TLS, no TCP options. Pair with [`crate::Channel::from_io`].
    /// [`Rpc::remote_addr`], [`Rpc::local_addr`], [`Rpc::peer_identity`],
    /// and [`Rpc::peer_cred`] are `None`. Generated handlers see the same
    /// empty facts on [`Request`] and [`crate::Parts`]. [`Rpc::scheme`] is the peer's
    /// `:scheme`. Use [`Self::serve_with_incoming`] and [`Incoming::peer`]
    /// when a custom acceptor already knows those facts.
    ///
    /// ```no_run
    /// # async fn run(
    /// #     io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    /// # ) -> Result<(), pbrs_grpc::Status> {
    /// # use pbrs_grpc::{Rpc, Server, Service};
    /// # struct Echo;
    /// # impl Service for Echo {
    /// #     const NAME: &'static str = "demo.Echo";
    /// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
    /// # }
    /// Server::new(Echo).serve_connection(io).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn serve_connection<IO>(self, io: IO) -> Result<(), Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (dispatch, config) = self.into_single();
        serve_one(Arc::new(dispatch), io, None, config).await
    }

    /// Serve connections from `incoming` until it is exhausted or the
    /// listener-side work fails.
    /// Applies to every call shape. See [`Incoming`].
    ///
    /// Override [`Incoming::peer`] to fill [`Rpc::local_addr`],
    /// [`Rpc::peer_identity`], [`Rpc::peer_cred`], or a transport
    /// [`Rpc::scheme`] without changing [`IncomingAccept`].
    pub async fn serve_with_incoming<I: Incoming>(self, incoming: I) -> Result<(), Status> {
        self.serve_with_incoming_shutdown(incoming, std::future::pending())
            .await
    }

    /// [`Self::serve_with_incoming`] until `shutdown` resolves, then drain.
    /// Applies to every call shape.
    pub async fn serve_with_incoming_shutdown<I: Incoming>(
        self,
        incoming: I,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (dispatch, config) = self.into_single();
        accept_incoming(Arc::new(dispatch), incoming, config, shutdown).await
    }
}

/// Newtype so the monomorphic path gets its own [`Dispatch`] impl.
struct Single<S> {
    service: Arc<S>,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
    response_interceptor: Option<crate::interceptor::ResponseHook>,
}

impl<S: Service> Dispatch for Single<S> {
    async fn dispatch(&self, mut rpc: Rpc) {
        rpc.response_interceptor = self.response_interceptor.clone();
        if let Some(interceptor) = &self.interceptor {
            if let Err(status) = interceptor.intercept(&mut rpc) {
                return rpc.reject(status);
            }
        }
        self.service.call(rpc).await;
    }
}

/// Serves several services, routing on the service half of the path.
///
/// Routing is a hash lookup on the `/<service>/` prefix plus one boxed future
/// per RPC. Use [`Server`] when you have a single service and want neither.
///
/// A path whose service is not mounted, or a method a mounted service does
/// not have, is [`crate::Code::Unimplemented`] on every call shape, including
/// over TLS, mTLS, Unix, and [`Server::serve_connection`].
///
/// Generated reflection also mounts `grpc.reflection.v1alpha.ServerReflection`
/// as a [`Service::ALIASES`] path of v1, so older grpcurl still lists.
/// Distinct from a second proto. Distinct from [`Server`], which does not
/// look up the path.
///
/// ```no_run
/// use pbrs_grpc::Router;
/// # use pbrs_grpc::{Rpc, Service};
/// # struct A; struct B;
/// # impl Service for A {
/// #     const NAME: &'static str = "demo.A";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # impl Service for B {
/// #     const NAME: &'static str = "demo.B";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// Router::new()
///     .add_service(A)
///     .add_service(B)
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await
/// # }
/// ```
#[derive(Clone, Default)]
pub struct Router {
    routes: HashMap<&'static str, Arc<dyn DynService>>,
    config: ServerConfig,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
    response_interceptor: Option<crate::interceptor::ResponseHook>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut services: Vec<&str> = self.routes.keys().copied().collect();
        services.sort_unstable();
        f.debug_struct("Router")
            .field("services", &services)
            .field("config", &self.config)
            .field("interceptors", &self.interceptor.is_some())
            .field(
                "response_interceptors",
                &self.response_interceptor.is_some(),
            )
            .finish()
    }
}

impl Router {
    /// An empty router with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            config: ServerConfig::default(),
            interceptor: None,
            response_interceptor: None,
        }
    }

    /// Replace the transport and limit configuration. Applies to every call
    /// shape.
    #[must_use]
    pub fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// The configuration in effect. Applies to every call shape.
    ///
    /// Distinct from [`Self::config`], which replaces it. Same snapshot a
    /// [`crate::Channel::config`] getter returns on the client.
    #[must_use]
    pub fn server_config(&self) -> ServerConfig {
        self.config
    }

    /// Cap inbound messages at `limit` bytes. Default 4 MiB.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_decoding_message_size(limit);
        self
    }

    /// Cap outbound messages at `limit` bytes. Default unlimited.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_encoding_message_size(limit);
        self
    }

    /// Replace both message caps at once. Applies to every call shape.
    /// See [`ServerConfig::message_limits`].
    /// Distinct from [`Self::max_decoding_message_size`] /
    /// [`Self::max_encoding_message_size`]. Oversize inbound or outbound
    /// is [`Code::ResourceExhausted`], including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`].
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.config = self.config.message_limits(limits);
        self
    }

    /// Configured message caps. See [`Self::message_limits`].
    /// Applies to every call shape.
    /// Distinct from [`Self::message_limits`], which sets them.
    /// Distinct from [`Self::send_buffer_size`]: that is the HTTP/2 send buffer, not uncompressed protobuf bytes.
    /// Same overlay as [`crate::Rpc::limits`].
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.config.limits()
    }

    /// Cap how many RPCs the process will run at once.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. See [`ServerConfig::max_concurrent_rpcs`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_rpcs(n);
        self
    }

    /// Configured process-wide RPC cap, if any. See [`Self::max_concurrent_rpcs`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_concurrent_rpcs`], which sets it.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.config.concurrent_rpc_limit()
    }

    /// Cap how many TCP/Unix connections the accept loop will serve at once,
    /// including TLS and mTLS listeners. Applies to every call shape. See
    /// [`ServerConfig::max_concurrent_connections`].
    #[must_use]
    pub fn max_concurrent_connections(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_connections(n);
        self
    }

    /// Concurrent RPCs allowed per HTTP/2 connection. Applies to every call
    /// shape. See [`ServerConfig::max_concurrent_streams`].
    /// HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`. Distinct from
    /// [`Self::max_concurrent_rpcs`], which refuses extras as
    /// [`Code::ResourceExhausted`]. A well-behaved client waits; both RPCs
    /// still complete, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.config = self.config.max_concurrent_streams(streams);
        self
    }

    /// HTTP/2 per-stream receive window. Applies to every call shape.
    /// See [`ServerConfig::initial_stream_window_size`].
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.config = self.config.initial_stream_window_size(bytes);
        self
    }

    /// HTTP/2 per-connection receive window. Applies to every call shape.
    /// See [`ServerConfig::initial_connection_window_size`].
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.config = self.config.initial_connection_window_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Applies to every call shape.
    /// See [`ServerConfig::max_frame_size`].
    /// A well-behaved client splits DATA; every call shape still completes,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct
    /// from [`Self::max_header_list_size`], which refuses oversize metadata,
    /// and from [`Self::max_concurrent_streams`], which serializes extra RPCs.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.config = self.config.max_frame_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. Applies to every call shape.
    /// See [`ServerConfig::max_header_list_size`].
    /// Oversize metadata is refused, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. Distinct from a raw HTTP/2 peer.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.config = self.config.max_header_list_size(bytes);
        self
    }

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic table). Default 4096.
    /// Applies to every call shape. See [`ServerConfig::header_table_size`].
    /// A well-behaved client still completes every call shape at this table
    /// size, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Distinct from
    /// [`Self::max_header_list_size`], which caps uncompressed header-block
    /// bytes (`SETTINGS_MAX_HEADER_LIST_SIZE`).
    #[must_use]
    pub fn header_table_size(mut self, bytes: u32) -> Self {
        self.config = self.config.header_table_size(bytes);
        self
    }

    /// HTTP/2 small-DATA framing budget. Default 25600.
    /// Applies to every call shape. See [`ServerConfig::data_frame_budget`].
    /// Caps extra memory from tiny DATA frames. Exceeding this is
    /// `ENHANCE_YOUR_CALM` (`too_many_data_frames`). Distinct from
    /// [`Self::initial_connection_window_size`], which is flow-control bytes,
    /// and from [`Self::max_frame_size`], which caps one DATA payload.
    /// h2 Auto (half the connection window) is not exposed.
    /// A well-behaved client still completes every call shape at this framing
    /// budget, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn data_frame_budget(mut self, bytes: usize) -> Self {
        self.config = self.config.data_frame_budget(bytes);
        self
    }

    /// Per-connection HTTP/2 send buffer. Applies to every call shape.
    /// See [`ServerConfig::max_send_buffer_size`].
    /// Write backpressure still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::initial_stream_window_size`], which still
    /// serves at a small receive window.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_send_buffer_size(bytes);
        self
    }

    /// Configured write-time HTTP/2 send buffer. See [`Self::max_send_buffer_size`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_send_buffer_size`], which sets it.
    /// Distinct from [`Self::message_limits`]: that is uncompressed protobuf bytes, not this send buffer.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.config.send_buffer_size()
    }

    /// Cap remotely-reset HTTP/2 streams waiting in the accept queue.
    /// Applies to every call shape. See
    /// [`ServerConfig::max_pending_accept_reset_streams`].
    /// A well-behaved client never fills that queue; every call shape still
    /// completes, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Distinct from a raw HTTP/2 peer.
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_pending_accept_reset_streams(n);
        self
    }

    /// Cap locally-reset HTTP/2 streams caused by a peer protocol error.
    /// Applies to every call shape. See
    /// [`ServerConfig::max_local_error_reset_streams`].
    /// Exceeding this is `ENHANCE_YOUR_CALM`. Distinct from
    /// [`Self::max_pending_accept_reset_streams`]: that caps remotely-reset
    /// streams (rapid reset). This caps RSTs we send after an invalid frame.
    /// A well-behaved client never triggers one; every call shape still
    /// completes, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_local_error_reset_streams(n);
        self
    }

    /// Cap remembered locally-reset HTTP/2 stream IDs.
    /// Default 50. Applies to every call shape. See
    /// [`ServerConfig::max_concurrent_reset_streams`].
    /// When the cap is reached, the oldest ID is purged from memory, not
    /// `ENHANCE_YOUR_CALM`. Frames on a purged ID are a connection
    /// `PROTOCOL_ERROR`. Distinct from
    /// [`Self::max_pending_accept_reset_streams`] (rapid-reset GOAWAY) and
    /// [`Self::max_local_error_reset_streams`] (protocol-error RST GOAWAY).
    /// A well-behaved client still completes every call shape at this memory cap,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn max_concurrent_reset_streams(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_reset_streams(n);
        self
    }

    /// How long locally-reset HTTP/2 stream IDs are remembered.
    /// Default 1 s. Applies to every call shape. See
    /// [`ServerConfig::reset_stream_duration`].
    /// After this duration the ID is forgotten, not `ENHANCE_YOUR_CALM`.
    /// Frames on a forgotten ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`Self::max_concurrent_reset_streams`], which is how many
    /// IDs are remembered (count). This is how long (time).
    /// A well-behaved client still completes every call shape at this reset duration,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn reset_stream_duration(mut self, dur: Duration) -> Self {
        self.config = self.config.reset_stream_duration(dur);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`. Applies to
    /// every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::serve_connection`]. See [`ServerConfig::timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// gzip responses when the client advertises gzip. Applies to every call
    /// shape, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// See [`ServerConfig::send_compressed`].
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// Deflate effort for outbound gzip. Default 1 (`flate2` fast).
    /// Applies to every call shape. See
    /// [`ServerConfig::gzip_compression_level`].
    /// Distinct from [`Self::send_compressed`], which is on or off.
    /// 0 stores; 9 is best. A well-behaved client still completes every
    /// call shape, including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn gzip_compression_level(mut self, level: u32) -> Self {
        self.config = self.config.gzip_compression_level(level);
        self
    }

    /// Inflate inbound gzip. Default `true`. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Passing `false` refuses `grpc-encoding: gzip` as
    /// [`Code::Unimplemented`] before the handler runs. Distinct from
    /// [`Self::send_compressed`], which is outbound. See
    /// [`ServerConfig::accept_compressed`].
    #[must_use]
    pub fn accept_compressed(mut self, accept: bool) -> Self {
        self.config = self.config.accept_compressed(accept);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`.
    /// Applies to every call shape.
    /// Distinct from [`Self::timeout`], which sets it.
    /// Interceptors and handlers read the same overlay on [`Rpc::rpc_timeout`]
    /// / [`Request::rpc_timeout`].
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.config.rpc_timeout()
    }

    /// Whether responses are gzipped when the client accepts gzip.
    /// Applies to every call shape.
    /// Distinct from [`Self::send_compressed`], which enables it.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.config.compresses_outbound()
    }

    /// Configured outbound gzip deflate level. See [`Self::gzip_compression_level`].
    /// Applies to every call shape.
    /// Distinct from [`Self::gzip_compression_level`], which sets it.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.config.gzip_level()
    }

    /// Whether inbound gzip is inflated. Default `true`.
    /// Applies to every call shape.
    /// Distinct from [`Self::accept_compressed`], which sets it.
    /// Distinct from [`Rpc::accepts_gzip`], which is the peer's
    /// `grpc-accept-encoding`.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.config.accepts_compressed()
    }

    /// HTTP/2 PING keepalive. Applies to every call shape.
    /// See [`ServerConfig::keep_alive_interval`].
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.config = self.config.keep_alive_interval(interval);
        self
    }

    /// How long to wait for a PING acknowledgement. Applies to every call
    /// shape. See [`ServerConfig::keep_alive_timeout`].
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.keep_alive_timeout(timeout);
        self
    }

    /// TCP `SO_KEEPALIVE`. Applies to every call shape.
    /// See [`ServerConfig::tcp_keepalive`].
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.config = self.config.tcp_keepalive(time);
        self
    }

    /// Send GOAWAY this long after accept. The next RPC of every call shape
    /// redials, including over TLS, mTLS, and Unix; transparent retry of the
    /// same in-flight RPC is unary and server-streaming after request bytes,
    /// client-streaming and bidi before HEADERS. See
    /// [`ServerConfig::max_connection_age`].
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.config = self.config.max_connection_age(age);
        self
    }

    /// Send GOAWAY after this long with no outstanding RPCs. The next RPC of
    /// every call shape redials, including over TLS, mTLS, and Unix. See
    /// [`ServerConfig::max_connection_idle`].
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.config = self.config.max_connection_idle(idle);
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs,
    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`].
    /// Applies to every call shape. See [`ServerConfig::max_connection_age_grace`].
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.config = self.config.max_connection_age_grace(grace);
        self
    }

    /// Drop a client that never finishes TLS or the HTTP/2 preface.
    /// Applies to every call shape, including over TLS, mTLS, and Unix. See
    /// [`ServerConfig::handshake_timeout`].
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.handshake_timeout(timeout);
        self
    }

    /// Mount `service` at `S::NAME`, replacing any service already there.
    ///
    /// [`Service::ALIASES`] are mounted the same way (last mount wins on
    /// each name). Generated reflection aliases
    /// `grpc.reflection.v1alpha.ServerReflection` onto the v1 handler so a
    /// Router with greeter + health + reflection still answers older grpcurl.
    /// Distinct from a second proto: messages are the v1 types. Distinct from
    /// [`Server::new`], which does not look up the path.
    ///
    /// The last mount is the one that serves, on every call shape, including
    /// over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn add_service<S: Service>(self, service: S) -> Self {
        self.add_arc(Arc::new(service))
    }

    /// Mount `service` when `Some`. `None` is a no-op.
    /// Applies to every call shape.
    ///
    /// Distinct from [`Self::add_service`], which always mounts.
    /// `None` does not replace a service already there.
    /// Services that stay mounted still complete every call shape, including
    /// over TLS, mTLS, Unix, and [`Self::serve_connection`].
    #[must_use]
    pub fn add_optional_service<S: Service>(self, service: Option<S>) -> Self {
        match service {
            Some(service) => self.add_service(service),
            None => self,
        }
    }

    /// Run `interceptor` before every mounted service. Calling this twice
    /// stacks: the first interceptor runs first. Same inspect/reject surface
    /// as [`Server::intercept`]. Applies to every call shape.
    /// [`Status::from_error_details`] is the typed bag after this Router intercept Err; those trailers reach the client without reading the body.
    /// Distinct from a handler Err: that is after the handler ran; this Router intercept Err is trailers without reading the body.
    /// Distinct from a Router on_response Err: that is trailers-only after handler Ok; this Router intercept Err is trailers without reading the body.
    /// Distinct from an Intercepted on_response Err: that is trailers-only after handler Ok; this Router intercept Err is trailers without reading the body.
    /// Distinct from a Server on_response Err: that is trailers-only after handler Ok; this Router intercept Err is trailers without reading the body.
    /// Distinct from a ServiceExt on_response Err: that is trailers-only after handler Ok; this Router intercept Err is trailers without reading the body.
    /// Distinct from a Channel on_response Err: that fails the Call after a successful receive; this Router intercept Err is trailers without reading the body.
    /// Distinct from a ResponseInterceptor Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Router intercept Err is trailers without reading the body.
    /// Distinct from a method-level on_response Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Router intercept Err is trailers without reading the body.
    /// Distinct from a Channel intercept Err: that is a local reject never opens a stream; this Router intercept Err is trailers without reading the body.
    /// Distinct from a ClientInterceptor Err: that is a local reject never opens a stream; this Router intercept Err is trailers without reading the body.
    /// Distinct from a method-level intercept Err: that is a local reject never opens a stream; this Router intercept Err is trailers without reading the body.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Router intercept Err is trailers without reading the body.
    /// Distinct from [`crate::Channel::intercept`]: that runs on the outbound call before the stream opens; this Router intercept runs on the inbound RPC before the handler.
    /// Distinct from [`Self::on_response`]: that runs after the handler returns Ok; this Router intercept runs on the inbound RPC before the handler.
    /// Distinct from [`Server::intercept`]: that runs on the inbound RPC before the Server's Service; this Router intercept runs on the inbound RPC before every mounted service on this Router.
    ///
    /// ```
    /// # fn demo(router: pbrs_grpc::Router) -> pbrs_grpc::Router {
    /// router.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = (
    ///         rpc.path(),
    ///         rpc.service(),
    ///         rpc.method(),
    ///         rpc.metadata(),
    ///         rpc.timeout(),
    ///         rpc.peer_timeout(),
    ///         rpc.rpc_timeout(),
    ///         rpc.effective_timeout(),
    ///         rpc.deadline(),
    ///         rpc.accepts_gzip(),
    ///         rpc.encoding(),
    ///         rpc.compresses_outbound(),
    ///         rpc.gzip_level(),
    ///         rpc.accepts_compressed(),
    ///         rpc.concurrent_rpc_limit(),
    ///         rpc.send_buffer_size(),
    ///         rpc.limits(),
    ///         rpc.local_addr(),
    ///         rpc.remote_addr(),
    ///         rpc.peer_identity(),
    ///         rpc.peer_cred(),
    ///         rpc.authority(),
    ///         rpc.scheme(),
    ///         rpc.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn intercept<I: crate::Interceptor>(mut self, interceptor: I) -> Self {
        self.interceptor = Some(match self.interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::Then::new(prev, interceptor)),
        });
        self
    }

    /// Run `interceptor` after the handler returns `Ok`.
    ///
    /// Closures implement [`crate::ResponseInterceptor`]. The hook sees
    /// [`crate::ResponseParts`]: headers, trailers, compress, and local
    /// [`crate::Response::extensions`]. Those extensions are not on the
    /// wire; stamp [`crate::ResponseParts::metadata_mut`] to send a header.
    /// Calling this twice stacks: the first interceptor runs first, matching
    /// [`Self::intercept`]. Applies to every call shape, including over TLS,
    /// mTLS, Unix, and [`Server::serve_connection`].
    /// `Err` after the handler already ran; that status is sent trailers-only
    /// instead of the response, including [`Status::with_error_details`].
    /// A handler `Err` skips this hook.
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// Distinct from [`crate::Request::path`]: that is the inbound request.
    /// [`crate::ResponseParts::gzip_level`] is the server encode overlay.
    /// Distinct from [`crate::ResponseParts::compress`]: that is on or off.
    /// [`crate::ResponseParts::compresses_outbound`] is the server encode overlay.
    /// Distinct from [`crate::ResponseParts::compress`]: that is the per-RPC Compressed-Flag.
    /// [`crate::ResponseParts::accepts_gzip`] is the peer `grpc-accept-encoding` advertisement.
    /// Distinct from [`crate::ResponseParts::encoding`]: that is received `grpc-encoding`.
    /// [`crate::ResponseParts::deadline`] is kernel-stamped when writing.
    /// Distinct from [`crate::Request::deadline`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::deadline`]: that is computed when that getter runs.
    /// [`crate::ResponseParts::timeout`] is the duration stamped at dispatch.
    /// Distinct from [`crate::ResponseParts::deadline`]: that is the Instant.
    /// [`crate::ResponseParts::limits`] is the encode cap when writing.
    /// Distinct from [`crate::Request::limits`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::limits`]: that is a server interceptor before the handler.
    /// [`crate::ResponseParts::peer_timeout`] is the client's `grpc-timeout`.
    /// Distinct from [`crate::ResponseParts::timeout`]: that is the effective cap.
    /// [`crate::ResponseParts::rpc_timeout`] is the server overlay.
    /// Distinct from [`crate::ResponseParts::timeout`]: that is soonest-of-three, not the overlay.
    /// Distinct from [`crate::ResponseParts::peer_timeout`]: that is the client's `grpc-timeout`.
    /// [`crate::ResponseParts::accepts_compressed`] is the inbound gzip overlay.
    /// Distinct from [`crate::ResponseParts::accepts_gzip`]: that is the peer advertisement.
    /// [`crate::ResponseParts::send_buffer_size`] is the write-time HTTP/2 send buffer overlay.
    /// Distinct from [`crate::ResponseParts::limits`]: that is the encode cap, not this send buffer.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy after this Router on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay after this Router on_response.
    /// [`Status::from_error_details`] is the typed bag after this Router on_response Err; a local reject is trailers-only after handler Ok.
    /// Distinct from a handler Err: that is after the handler ran; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a Router intercept Err: that is trailers without reading the body; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from an Interceptor Err: that is trailers without reading the body; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level Interceptor Err: that is trailers without reading the body; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a Server intercept Err: that is trailers without reading the body; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a ServiceExt intercept Err: that is trailers without reading the body; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a ResponseInterceptor Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level on_response Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a Channel on_response Err: that fails the Call after a successful receive; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a ClientInterceptor Err: that is a local reject never opens a stream; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a Channel intercept Err: that is a local reject never opens a stream; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a method-level intercept Err: that is a local reject never opens a stream; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Router on_response Err is trailers-only after handler Ok.
    /// Distinct from [`Self::intercept`]: that runs on the inbound RPC before the handler; this Router on_response runs after the handler returns Ok.
    /// Distinct from [`Server::on_response`]: that runs after the handler returns Ok on the Server's Service; this Router on_response runs after the handler returns Ok on every mounted service on this Router.
    /// Same surface as [`Server::on_response`].
    ///
    /// ```
    /// # fn demo(router: pbrs_grpc::Router) -> pbrs_grpc::Router {
    /// router.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = (
    ///         parts.path(),
    ///         parts.service(),
    ///         parts.method(),
    ///         parts.metadata(),
    ///         parts.trailers(),
    ///         parts.compress(),
    ///         parts.compress_is_set(),
    ///         parts.encoding(),
    ///         parts.gzip_level(),
    ///         parts.compresses_outbound(),
    ///         parts.accepts_gzip(),
    ///         parts.deadline(),
    ///         parts.timeout(),
    ///         parts.limits(),
    ///         parts.peer_timeout(),
    ///         parts.rpc_timeout(),
    ///         parts.accepts_compressed(),
    ///         parts.send_buffer_size(),
    ///         parts.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn on_response<I: crate::ResponseInterceptor>(mut self, interceptor: I) -> Self {
        self.response_interceptor = Some(match self.response_interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::ResponseThen::new(prev, interceptor)),
        });
        self
    }

    fn add_arc<S: Service>(mut self, service: Arc<S>) -> Self {
        let service: Arc<dyn DynService> = service;
        for &alias in S::ALIASES {
            self.routes.insert(alias, Arc::clone(&service));
        }
        self.routes.insert(S::NAME, service);
        self
    }

    /// Mounted service names, in unspecified order, including [`Service::ALIASES`].
    /// Distinct from reflection `list_services`, which reports
    /// `FILE_DESCRIPTOR_SET` names, not these route keys.
    pub fn service_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.routes.keys().copied()
    }

    /// Bind `addr` and serve until the listener fails.
    /// Applies to every call shape.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Status> {
        self.serve_listener(bind(addr).await?).await
    }

    /// Serve on an existing listener until it fails.
    /// Applies to every call shape.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        self.serve_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve until `shutdown` resolves, then drain. Applies to every call
    /// shape. In-flight RPCs finish; new connections are refused. TLS and
    /// Unix drain the same way (`serve_tls_with_shutdown`,
    /// `serve_unix_with_shutdown`). See [`Server::serve_with_shutdown`].
    pub async fn serve_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_loop(Arc::new(self), listener, config, shutdown, None).await
    }

    /// Bind `addr` and serve until `shutdown` resolves, then drain.
    /// Applies to every call shape. See [`Server::serve_until_shutdown`].
    pub async fn serve_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_with_shutdown(bind(addr).await?, shutdown).await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails. Applies to every call shape. See [`Server::serve_unix`].
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], after unlinking a crash leftover.
    /// Applies to every call shape. A live listener is left alone. See
    /// [`Server::serve_unix_unlink`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path).await?)
            .await
    }

    /// Serve h2c on an existing Unix listener until it fails.
    /// Applies to every call shape.
    #[cfg(unix)]
    pub async fn serve_unix_listener(self, listener: UnixListener) -> Result<(), Status> {
        self.serve_unix_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve h2c on a Unix listener until `shutdown` resolves, then drain.
    /// Applies to every call shape. In-flight RPCs finish; new connections
    /// are refused. See [`Server::serve_with_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_with_shutdown(
        self,
        listener: UnixListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_unix_loop(Arc::new(self), listener, config, shutdown).await
    }

    /// Bind `path` and serve h2c until `shutdown` resolves, then drain.
    /// Applies to every call shape. See [`Server::serve_unix_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix(path)?, shutdown)
            .await
    }

    /// [`Self::serve_unix_until_shutdown`], after unlinking a crash leftover.
    /// Applies to every call shape. See [`Server::serve_unix_unlink_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix_unlink(path).await?, shutdown)
            .await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    ///
    /// To bind and then drain on a signal, use [`Self::serve_tls_until_shutdown`].
    /// `:scheme` is `https` on every call shape. mTLS fills
    /// [`Rpc::peer_identity`] on every call shape.
    pub async fn serve_tls(self, addr: SocketAddr, tls: ServerTls) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, std::future::pending(), tls)
            .await
    }

    /// Serve over TLS until `shutdown` resolves, then drain.
    /// Applies to every call shape, including mTLS. In-flight RPCs finish;
    /// new connections are refused. See [`Server::serve_with_shutdown`].
    pub async fn serve_tls_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_loop(Arc::new(self), listener, config, shutdown, Some(tls)).await
    }

    /// Bind `addr` and serve over TLS until `shutdown` resolves, then drain.
    /// Applies to every call shape. See [`Server::serve_tls_until_shutdown`].
    pub async fn serve_tls_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, shutdown, tls)
            .await
    }

    /// Serve a single already-accepted byte stream until it closes.
    /// Applies to every call shape.
    /// See [`Server::serve_connection`].
    pub async fn serve_connection<IO>(self, io: IO) -> Result<(), Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let config = self.config;
        serve_one(Arc::new(self), io, None, config).await
    }

    /// Serve connections from `incoming` until it is exhausted.
    /// Applies to every call shape. See [`Server::serve_with_incoming`].
    ///
    /// Override [`Incoming::peer`] to fill [`Rpc::local_addr`],
    /// [`Rpc::peer_identity`], [`Rpc::peer_cred`], or a transport
    /// [`Rpc::scheme`] without changing [`IncomingAccept`].
    pub async fn serve_with_incoming<I: Incoming>(self, incoming: I) -> Result<(), Status> {
        self.serve_with_incoming_shutdown(incoming, std::future::pending())
            .await
    }

    /// [`Self::serve_with_incoming`] until `shutdown` resolves, then drain.
    /// Applies to every call shape.
    pub async fn serve_with_incoming_shutdown<I: Incoming>(
        self,
        incoming: I,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_incoming(Arc::new(self), incoming, config, shutdown).await
    }
}

impl Dispatch for Router {
    async fn dispatch(&self, mut rpc: Rpc) {
        rpc.response_interceptor = self.response_interceptor.clone();
        if let Some(interceptor) = &self.interceptor {
            if let Err(status) = interceptor.intercept(&mut rpc) {
                return rpc.reject(status);
            }
        }
        match self.routes.get(rpc.service()) {
            Some(service) => service.dispatch(rpc).await,
            None => rpc.unimplemented(),
        }
    }
}

async fn bind(addr: SocketAddr) -> Result<TcpListener, Status> {
    TcpListener::bind(addr)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))
}

#[cfg(unix)]
fn bind_unix(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    UnixListener::bind(path.as_ref()).map_err(|e| Status::unavailable(e.to_string()))
}

/// Bind `path`, unlinking a crash leftover. A live listener is left alone.
#[cfg(unix)]
async fn bind_unix_unlink(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    let path = path.as_ref();
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e)
            if e.kind() == std::io::ErrorKind::AddrInUse
                || e.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            if unix_path_has_listener(path).await {
                return Err(Status::unavailable(format!(
                    "unix socket {} is already in use",
                    path.display()
                )));
            }
            std::fs::remove_file(path).map_err(|e| Status::unavailable(e.to_string()))?;
            UnixListener::bind(path).map_err(|e| Status::unavailable(e.to_string()))
        }
        Err(e) => Err(Status::unavailable(e.to_string())),
    }
}

/// `true` if some process owns this inode. A crash leftover fails connect
/// with `ConnectionRefused`. A live listener accepts, a full backlog returns
/// `WouldBlock`, and a stuck accept loop times out — all of those are live,
/// so we do not steal.
#[cfg(unix)]
async fn unix_path_has_listener(path: &std::path::Path) -> bool {
    match tokio::time::timeout(
        std::time::Duration::from_millis(50),
        UnixStream::connect(path),
    )
    .await
    {
        Ok(Ok(_stream)) => true,
        Ok(Err(e)) => !unix_connect_means_stale(&e),
        Err(_elapsed) => true,
    }
}

#[cfg(unix)]
fn unix_connect_means_stale(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::AddrNotAvailable
    )
}

fn connection_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .connection_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// Returns `None` when `max_concurrent_rpcs` is unset. Otherwise a semaphore
/// of that many permits, created once per accept loop so every connection
/// shares the process-wide budget.
fn rpc_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .concurrent_rpc_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// `None` means refuse this peer. `Some(None)` means unlimited. `Some(Some(p))`
/// is a live slot held until the connection task drops it.
fn take_connection_slot(
    slots: &Option<Arc<Semaphore>>,
) -> Option<Option<tokio::sync::OwnedSemaphorePermit>> {
    match slots {
        None => Some(None),
        Some(sem) => sem.clone().try_acquire_owned().ok().map(Some),
    }
}

/// Accept connections until `shutdown` resolves, then drain in-flight work.
async fn accept_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    tls: Option<ServerTls>,
) -> Result<(), Status> {
    // Dropping every clone of `drain_tx` is what tells us the last connection
    // task has finished.
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let rpcs = rpc_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(listener.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((tcp, peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(tcp);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                let tls = tls.clone();
                let rpcs = rpcs.clone();
                drop(tokio::spawn(async move {
                    crate::tcp::tune(&tcp, config.tcp_keepalive_period()).ok();
                    let local = tcp.local_addr().ok();
                    match tls {
                        None => {
                            drop(
                                serve_io(
                                    dispatch,
                                    tcp,
                                    ConnectionInfo::tcp(peer, local),
                                    config,
                                    goaway,
                                    rpcs,
                                )
                                .await,
                            );
                        }
                        Some(tls) => {
                            let accept = tokio::time::timeout(
                                config.io_handshake_timeout(),
                                tls.accept(tcp),
                            );
                            if let Ok(Ok(io)) = accept.await {
                                let identity = crate::tls::peer_identity_of(&io);
                                drop(
                                    serve_io(
                                        dispatch,
                                        io,
                                        ConnectionInfo::tls(peer, local, identity),
                                        config,
                                        goaway,
                                        rpcs,
                                    )
                                    .await,
                                );
                            }
                        }
                    }
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(Status::unavailable(e.to_string()));
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    // Resolves once every connection task has dropped its `drain` clone.
    while drain_rx.recv().await.is_some() {}
    result
}

/// Unix-domain accept loop. Same drain/GOAWAY contract as the TCP accept
/// loop, without TLS or `TCP_NODELAY` (neither applies).
#[cfg(unix)]
async fn accept_unix_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: UnixListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let rpcs = rpc_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(listener.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((io, _peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(io);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                let rpcs = rpcs.clone();
                let cred = peer_cred_of(&io);
                drop(tokio::spawn(async move {
                    drop(
                        serve_io(
                            dispatch,
                            io,
                            ConnectionInfo::unix(cred),
                            config,
                            goaway,
                            rpcs,
                        )
                        .await,
                    );
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(Status::unavailable(e.to_string()));
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    while drain_rx.recv().await.is_some() {}
    result
}

/// Accept from a custom [`Incoming`] until it is exhausted or `shutdown`
/// resolves, then drain. No TLS, no TCP options — the acceptor already
/// holds a byte stream.
async fn accept_incoming<D: Dispatch, I: Incoming>(
    dispatch: Arc<D>,
    mut incoming: I,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let rpcs = rpc_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(incoming.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((io, peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(io);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                let rpcs = rpcs.clone();
                let info = incoming.peer(&io, peer);
                drop(tokio::spawn(async move {
                    drop(serve_io(dispatch, io, info, config, goaway, rpcs).await);
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    while drain_rx.recv().await.is_some() {}
    result
}

async fn serve_one<D, IO>(
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
#[derive(Clone, Debug, Default)]
pub struct ConnectionInfo {
    remote: Option<SocketAddr>,
    local: Option<SocketAddr>,
    identity: Option<PeerIdentity>,
    cred: Option<PeerCred>,
    /// Transport `:scheme` when the accept loop knows it. `None` keeps the
    /// peer's `:scheme` ([`Incoming`] / [`Server::serve_connection`]).
    scheme: Option<&'static str>,
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

fn incoming_rpc(
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
        timeout: None,
        response_interceptor: None,
    }
}

async fn serve_io<D, IO>(
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
    let mut conn = match tokio::time::timeout(
        config.io_handshake_timeout(),
        config.h2_builder().handshake(io),
    )
    .await
    {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => return Err(Status::unavailable(e.to_string())),
        Err(_) => return Err(Status::unavailable("http/2 preface timed out")),
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
                    reject_request(&mut respond, err, config.accepts_compressed());
                    continue;
                }
                let permit = match &rpc_slots {
                    None => None,
                    Some(slots) => match slots.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            reject(
                                &mut respond,
                                Status::resource_exhausted("too many concurrent RPCs"),
                                config.accepts_compressed(),
                            );
                            continue;
                        }
                    },
                };
                let lease = busy.start();
                let dispatch = Arc::clone(&dispatch);
                let rpc_peer = peer.clone();
                drop(tokio::spawn(async move {
                    let _lease = lease;
                    let _permit = permit;
                    dispatch
                        .dispatch(incoming_rpc(request, respond, config, rpc_peer))
                        .await;
                }));
            }
            _ = busy.notified() => {}
            _ = wait_for_drain(goaway.clone()), if !draining => {
                draining = true;
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

async fn sleep_until_opt(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn connection_seed(peer: Option<SocketAddr>) -> u64 {
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

async fn wait_for_drain(mut goaway: watch::Receiver<bool>) {
    // A dropped sender also means "stop accepting": the accept loop is gone.
    goaway.wait_for(|v| *v).await.ok();
}

#[cfg(test)]
mod tests {
    use super::split_path;

    #[test]
    fn splits_service_and_method() {
        assert_eq!(
            split_path("/helloworld.Greeter/SayHello"),
            ("helloworld.Greeter", "SayHello")
        );
        assert_eq!(split_path("/a.B/C"), ("a.B", "C"));
    }

    #[test]
    fn unparseable_paths_route_nowhere() {
        assert_eq!(split_path("/"), ("", ""));
        assert_eq!(split_path(""), ("", ""));
        assert_eq!(split_path("/nomethod"), ("", ""));
    }
}
