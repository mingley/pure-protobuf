//! Accept loops: [`Incoming`], [`Server`], bind and accept helpers.

use super::connection::{ConnectionInfo, serve_io, serve_one, wait_for_drain};
use super::dispatch::{Dispatch, Service, Single};
use super::router::Router;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::rpc::Rpc;
use crate::config::ServerConfig;
use crate::limits::{ByteBudgetTracker, MessageLimits};
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::Request;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::status::{Code, Status};
use crate::telemetry::{LifecycleObserver, ObserverChain};
use crate::tls::ServerTls;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc, watch};

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
pub(crate) fn peer_cred_of(io: &UnixStream) -> Option<PeerCred> {
    let cred = io.peer_cred().ok()?;
    Some(PeerCred {
        uid: cred.uid(),
        gid: cred.gid(),
        pid: cred.pid().and_then(|pid| u32::try_from(pid).ok()),
    })
}

/// Serves exactly one [`Service`], with no per-RPC dynamic dispatch.
///
/// A hand-written [`Service`] is first-class. Unknown methods are
/// [`crate::Code::Unimplemented`] on every call shape, including over TLS,
/// mTLS, Unix, and [`Server::serve_connection`].
/// There is no tonic `Server::trace_fn`: that intercepts inbound headers and
/// installs a `tracing::Span` on each response future. This type has no span
/// installer. Distinct from [`crate::Interceptor`] (envelope mutation, not a
/// span). Distinct from grpc.stats `Handler` (Begin/End/payload). Distinct from
/// binary logging (`grpc.binarylog.v1`). Distinct from OpenTelemetry. Distinct
/// from tonic `Server::layer` (tower).
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
    observer: Option<Arc<dyn LifecycleObserver>>,
    byte_budget: ByteBudgetTracker,
    binlog: Option<Arc<crate::binlog::BinaryLogger>>,
}

impl<S> Clone for Server<S> {
    fn clone(&self) -> Self {
        Self {
            service: Arc::clone(&self.service),
            config: self.config,
            interceptor: self.interceptor.clone(),
            response_interceptor: self.response_interceptor.clone(),
            observer: self.observer.clone(),
            byte_budget: self.byte_budget.clone(),
            binlog: self.binlog.clone(),
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
            .field("observer", &self.observer.is_some())
            .field("binary_logger", &self.binlog.is_some())
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
            observer: None,
            byte_budget: ByteBudgetTracker::default(),
            binlog: None,
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
            observer: None,
            byte_budget: ByteBudgetTracker::default(),
            binlog: None,
        }
    }

    /// Byte budget tracker in effect.
    #[must_use]
    pub fn byte_budget_tracker(&self) -> &ByteBudgetTracker {
        &self.byte_budget
    }

    /// Set a transport byte budget for queued and in-flight buffers.
    #[must_use]
    pub fn byte_budget(mut self, limit: usize) -> Self {
        self.byte_budget = ByteBudgetTracker::with_limit(limit);
        self
    }

    /// Attach an existing [`crate::ByteBudgetTracker`] to this server.
    #[must_use]
    pub fn with_byte_budget_tracker(mut self, tracker: ByteBudgetTracker) -> Self {
        self.byte_budget = tracker;
        self
    }

    /// Number of bytes currently allocated in transport buffers on this server.
    #[must_use]
    pub fn byte_budget_allocated(&self) -> usize {
        self.byte_budget.allocated()
    }

    /// The configured byte budget limit, if any.
    #[must_use]
    pub fn byte_budget_limit(&self) -> Option<usize> {
        self.byte_budget.limit()
    }

    /// Whether transport buffer allocation is currently zero.
    #[must_use]
    pub fn is_byte_budget_quiescent(&self) -> bool {
        self.byte_budget.is_quiescent()
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
    /// Preserves the aggregate byte budget; set it separately with [`Self::byte_budget`].
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

    /// TCP `TCP_KEEPINTVL` probe interval. Applies to every call shape.
    /// Only applied when [`Self::tcp_keepalive`] is also set; this does not
    /// turn `SO_KEEPALIVE` on by itself. Distinct from
    /// [`Self::keep_alive_interval`], which sends HTTP/2 PINGs. See
    /// [`ServerConfig::tcp_keepalive_interval`].
    #[must_use]
    pub fn tcp_keepalive_interval(mut self, interval: Duration) -> Self {
        self.config = self.config.tcp_keepalive_interval(interval);
        self
    }

    /// TCP `TCP_KEEPCNT` probe count. Applies to every call shape.
    /// Only applied when [`Self::tcp_keepalive`] is also set; this does not
    /// turn `SO_KEEPALIVE` on by itself. Distinct from
    /// [`Self::tcp_keepalive_interval`], which is probe spacing. See
    /// [`ServerConfig::tcp_keepalive_retries`].
    #[must_use]
    pub fn tcp_keepalive_retries(mut self, retries: u32) -> Self {
        self.config = self.config.tcp_keepalive_retries(retries);
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

    /// Enforce a gRFC A43 authorization policy on every call.
    ///
    /// Denied calls fail with [`Code::PermissionDenied`] before the
    /// handler runs, on every call shape, including over TLS, mTLS, Unix,
    /// and [`Self::serve_connection`]. Stacks with [`Self::intercept`]:
    /// earlier interceptors run first.
    #[must_use]
    pub fn authorization_policy(self, provider: impl Into<crate::authz::Provider>) -> Self {
        self.intercept(crate::authz::AuthzInterceptor::new(provider))
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

    /// Register a lifecycle telemetry observer.
    ///
    /// The observer receives lifecycle events for incoming RPCs:
    /// server call start/end, server queue wait, payload bytes, rejections, and cancellations.
    /// Peer-supplied paths and authorities are raw identity. For metrics,
    /// register a [`crate::telemetry::BoundedMetricObserver`] with a reviewed
    /// [`crate::telemetry::MetricLabelPolicy`].
    ///
    /// Calling this twice stacks observers: the first registered observer runs first.
    #[must_use]
    pub fn observer<O: LifecycleObserver>(mut self, observer: O) -> Self {
        self.observer = Some(match self.observer {
            None => Arc::new(observer),
            Some(prev) => Arc::new(ObserverChain::new(prev, Arc::new(observer))),
        });
        self
    }

    /// Record RPCs as `grpc.binarylog.v1` entries via `logger`.
    ///
    /// Applies to every call shape. Methods the logger's filter excludes
    /// cost one branch and log nothing. Credential headers are omitted and
    /// sensitive metadata values masked; see
    /// [`BinaryLogger`](crate::binlog::BinaryLogger).
    /// Distinct from [`Self::observer`]: that reports lifecycle events to
    /// telemetry; this records wire-faithful RPC logs to a sink.
    #[must_use]
    pub fn binary_logger(mut self, logger: crate::binlog::BinaryLogger) -> Self {
        self.binlog = Some(Arc::new(logger));
        self
    }

    fn into_single(self) -> (Single<S>, ServerConfig) {
        // Channelz: one server entity per serve, named for the service.
        let channelz = Some(crate::channelz::Registry::global_shared().register_server(S::NAME));
        (
            Single {
                service: self.service,
                interceptor: self.interceptor,
                response_interceptor: self.response_interceptor,
                observer: self.observer,
                byte_budget: self.byte_budget,
                binlog: self.binlog,
                channelz,
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
        router.observer = self.observer;
        router.byte_budget = self.byte_budget;
        router.binlog = self.binlog;
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
    /// There is no grpc-go `WaitForHandlers` setter: grpc-go `Stop` can
    /// return before handlers exit; this drain always waits for in-flight
    /// RPCs. Distinct from [`ServerConfig::max_connection_age_grace`]: that
    /// is GOAWAY then force-close, not this process-wide drain. Distinct
    /// from [`crate::health::HealthReporter::shutdown`]: that flips serving
    /// status, it does not drain. [`Self::serve_connection`] is one duplex:
    /// no accept loop to refuse.
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

    /// Serve `cores` shards of this service on `addr` until `shutdown`
    /// resolves, then drain. Applies to every call shape.
    ///
    /// Each shard is its own OS thread running its own current-thread
    /// runtime, pinned to its core on Linux, accepting on its own
    /// `SO_REUSEPORT` listener. A connection is accepted, served, and
    /// drained on one thread: no task migration, no work stealing, no
    /// cross-core synchronization on the RPC path.
    ///
    /// Shutdown and drain match [`Self::serve_with_shutdown`]: in-flight
    /// RPCs finish on every shard before this returns, and a shard whose
    /// accept loop fails stops the rest, like the single loop failing.
    /// Limits: the connection budget is shared across shards (exact
    /// aggregate; the accept path is cold), while `max_concurrent_rpcs`
    /// divides into `max(1, limit / cores)` per shard. Keep the RPC limit
    /// at least `cores` for an exact aggregate.
    ///
    /// Returns `invalid_argument` when `cores` is zero. Pass a shard count
    /// near the machine's CPU count; oversubscribing only adds threads.
    /// Unix sockets have no `SO_REUSEPORT` equivalent; this mode is TCP
    /// only. Pair [`Self::bind_per_core`] with [`Self::serve_per_core_on`]
    /// when the bound address is needed up front (port `0`).
    pub async fn serve_per_core(
        self,
        addr: SocketAddr,
        cores: usize,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (_, listeners) = self.bind_per_core(addr, cores)?;
        self.serve_per_core_on(listeners, shutdown).await
    }

    /// [`Self::serve_per_core`] over TLS. Applies to every call shape,
    /// including mTLS.
    pub async fn serve_tls_per_core(
        self,
        addr: SocketAddr,
        cores: usize,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        let (_, listeners) = self.bind_per_core(addr, cores)?;
        self.serve_tls_per_core_on(listeners, shutdown, tls).await
    }

    /// Bind `cores` `SO_REUSEPORT` shards on `addr`, resolving port `0` to
    /// the address the kernel chose. Serve them with
    /// [`Self::serve_per_core_on`].
    ///
    /// The shards are std listeners on purpose: each shard converts its
    /// own to Tokio on its own thread, so readiness never depends on the
    /// reactor that called this. Returns `invalid_argument` when `cores`
    /// is zero. A bad address fails here, before any thread starts.
    pub fn bind_per_core(
        &self,
        addr: SocketAddr,
        cores: usize,
    ) -> Result<(SocketAddr, Vec<std::net::TcpListener>), Status> {
        bind_reuseport_shards(addr, cores)
    }

    /// [`Self::serve_per_core`] on pre-bound shards from
    /// [`Self::bind_per_core`]. The shard count is the listener count.
    /// Applies to every call shape.
    pub async fn serve_per_core_on(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_per_core_inner(listeners, shutdown, None).await
    }

    /// [`Self::serve_per_core_on`] over TLS. Applies to every call shape,
    /// including mTLS.
    pub async fn serve_tls_per_core_on(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_per_core_inner(listeners, shutdown, Some(tls))
            .await
    }

    async fn serve_per_core_inner(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
        tls: Option<ServerTls>,
    ) -> Result<(), Status> {
        let config = self.server_config();
        let mut dispatches = Vec::with_capacity(listeners.len());
        for _ in 0..listeners.len() {
            let (dispatch, _) = self.clone().into_single();
            dispatches.push(Arc::new(dispatch));
        }
        serve_per_core_shards(dispatches, config, listeners, shutdown, tls).await
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

pub(crate) async fn bind(addr: SocketAddr) -> Result<TcpListener, Status> {
    TcpListener::bind(addr)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))
}

#[cfg(unix)]
pub(crate) fn bind_unix(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    UnixListener::bind(path.as_ref()).map_err(|e| Status::unavailable(e.to_string()))
}

/// Bind `path`, unlinking a crash leftover. A live listener is left alone.
#[cfg(unix)]
pub(crate) async fn bind_unix_unlink(
    path: impl AsRef<std::path::Path>,
) -> Result<UnixListener, Status> {
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
pub(crate) async fn unix_path_has_listener(path: &std::path::Path) -> bool {
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
pub(crate) fn unix_connect_means_stale(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::AddrNotAvailable
    )
}

pub(crate) fn connection_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .connection_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// Bind `cores` `SO_REUSEPORT` shards on `addr`, resolving port `0` to the
/// address the kernel chose. Shared by [`Server::bind_per_core`] and
/// [`Router::bind_per_core`].
fn bind_reuseport_shards(
    addr: SocketAddr,
    cores: usize,
) -> Result<(SocketAddr, Vec<std::net::TcpListener>), Status> {
    if cores == 0 {
        return Err(Status::invalid_argument(
            "per-core serve needs at least one core",
        ));
    }
    let first = crate::rt::per_core::reuseport_listener(addr)
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let bound = first
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let mut listeners = Vec::with_capacity(cores);
    listeners.push(first);
    for _ in 1..cores {
        let shard = crate::rt::per_core::reuseport_listener(bound)
            .map_err(|e| Status::unavailable(e.to_string()))?;
        listeners.push(shard);
    }
    Ok((bound, listeners))
}

/// Supervise one pinned current-thread shard per listener, each running
/// [`accept_loop_with_slots`] on its own dispatch. Shared by
/// [`Server`] and [`Router`] per-core serve; the only difference is how
/// each builds its per-shard dispatch. `dispatches` must pair one to one
/// with `listeners`.
async fn serve_per_core_shards<D: Dispatch>(
    dispatches: Vec<Arc<D>>,
    config: ServerConfig,
    listeners: Vec<std::net::TcpListener>,
    shutdown: impl Future<Output = ()> + Send,
    tls: Option<ServerTls>,
) -> Result<(), Status> {
    let cores = listeners.len();
    if cores == 0 || dispatches.len() != cores {
        return Err(Status::invalid_argument(
            "per-core serve needs at least one core",
        ));
    }
    let conn_slots = connection_slots(config);
    let rpc_each = config.concurrent_rpc_limit().map(|n| (n / cores).max(1));
    // Stateful broadcast: a shard that boots after shutdown still sees
    // it, which a `Notify` would miss.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (done_tx, mut done_rx) = mpsc::channel::<Result<(), Status>>(cores);
    let mut joins = Vec::with_capacity(cores);
    for ((core, listener), dispatch) in listeners.into_iter().enumerate().zip(dispatches) {
        let stopped = shutdown_rx.clone();
        let finished = done_tx.clone();
        let tls = tls.clone();
        let conn_slots = conn_slots.clone();
        let rpc_slots = rpc_each.map(|n| Arc::new(Semaphore::new(n)));
        let worker = std::thread::Builder::new()
            .name(format!("pbrs-per-core-{core}"))
            .spawn(move || {
                // A refused pin only costs placement; the shard still
                // serves correctly, just unpinned.
                crate::rt::per_core::pin_current_thread_to(core);
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let outcome = match runtime {
                    Ok(runtime) => runtime.block_on(async move {
                        // Convert on the shard thread: `from_std`
                        // registers with the ambient reactor, which must
                        // be this shard's, not the caller's.
                        let listener = match TcpListener::from_std(listener) {
                            Ok(listener) => listener,
                            Err(e) => {
                                return Err(Status::unavailable(e.to_string()));
                            }
                        };
                        accept_loop_with_slots(
                            dispatch,
                            listener,
                            config,
                            async move {
                                let mut stopped = stopped;
                                stopped.wait_for(|stop| *stop).await.ok();
                            },
                            tls,
                            conn_slots,
                            rpc_slots,
                        )
                        .await
                    }),
                    Err(e) => Err(Status::unavailable(e.to_string())),
                };
                finished.blocking_send(outcome).ok();
            });
        match worker {
            Ok(join) => joins.push(join),
            Err(e) => {
                // Stop the shards that did start; nothing serves half-built.
                shutdown_tx.send(true).ok();
                for join in joins {
                    drop(join.join());
                }
                return Err(Status::unavailable(e.to_string()));
            }
        }
    }
    drop(done_tx);
    let mut shutdown = std::pin::pin!(shutdown);
    let mut stopping = false;
    let mut results = Vec::with_capacity(cores);
    while results.len() < cores {
        tokio::select! {
            biased;
            _ = &mut shutdown, if !stopping => {
                stopping = true;
                shutdown_tx.send(true).ok();
            }
            recvd = done_rx.recv() => {
                match recvd {
                    Some(outcome) => {
                        if outcome.is_err() {
                            // Shard parity with the single loop: one
                            // failed accept loop stops the serve.
                            stopping = true;
                            shutdown_tx.send(true).ok();
                        }
                        results.push(outcome);
                    }
                    None => break,
                }
            }
        }
    }
    // Reap off the executor: every shard already reported, so the joins
    // return at once, but a block is still a block.
    let reaped = tokio::task::spawn_blocking(move || {
        for join in joins {
            drop(join.join());
        }
    })
    .await;
    if let Err(e) = reaped {
        return Err(Status::internal(e.to_string()));
    }
    let mut result = Ok(());
    for outcome in results {
        if result.is_ok() {
            result = outcome;
        }
    }
    result
}

impl Router {
    /// Opt-in thread-per-core serve: `cores` pinned current-thread
    /// shards on `SO_REUSEPORT` listeners, each owning its connections.
    /// Same contract as [`Server::serve_per_core`], for multi-service
    /// routers. The connection budget is shared across shards (exact
    /// aggregate), while `max_concurrent_rpcs` divides into
    /// `max(1, limit / cores)` per shard. Applies to every call shape.
    ///
    /// Returns `invalid_argument` when `cores` is zero. TCP only.
    /// Pair [`Self::bind_per_core`] with [`Self::serve_per_core_on`]
    /// when the bound address is needed up front (port `0`).
    pub async fn serve_per_core(
        self,
        addr: SocketAddr,
        cores: usize,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (_, listeners) = self.bind_per_core(addr, cores)?;
        self.serve_per_core_on(listeners, shutdown).await
    }

    /// [`Self::serve_per_core`] over TLS. Applies to every call shape,
    /// including mTLS.
    pub async fn serve_tls_per_core(
        self,
        addr: SocketAddr,
        cores: usize,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        let (_, listeners) = self.bind_per_core(addr, cores)?;
        self.serve_tls_per_core_on(listeners, shutdown, tls).await
    }

    /// Bind `cores` `SO_REUSEPORT` shards on `addr`, resolving port `0` to
    /// the address the kernel chose. Serve them with
    /// [`Self::serve_per_core_on`].
    ///
    /// The shards are std listeners on purpose: each shard converts its
    /// own to Tokio on its own thread, so readiness never depends on the
    /// reactor that called this. Returns `invalid_argument` when `cores`
    /// is zero. A bad address fails here, before any thread starts.
    pub fn bind_per_core(
        &self,
        addr: SocketAddr,
        cores: usize,
    ) -> Result<(SocketAddr, Vec<std::net::TcpListener>), Status> {
        bind_reuseport_shards(addr, cores)
    }

    /// [`Self::serve_per_core`] on pre-bound shards from
    /// [`Self::bind_per_core`]. The shard count is the listener count.
    /// Applies to every call shape.
    pub async fn serve_per_core_on(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_per_core_inner(listeners, shutdown, None).await
    }

    /// [`Self::serve_per_core_on`] over TLS. Applies to every call shape,
    /// including mTLS.
    pub async fn serve_tls_per_core_on(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_per_core_inner(listeners, shutdown, Some(tls))
            .await
    }

    async fn serve_per_core_inner(
        self,
        listeners: Vec<std::net::TcpListener>,
        shutdown: impl Future<Output = ()> + Send,
        tls: Option<ServerTls>,
    ) -> Result<(), Status> {
        // Channelz: one server entity per shard, named like
        // `with_channelz` (one entity per serve there). Each shard owns
        // its dispatch, so each registers its own.
        let mut services: Vec<&str> = self.service_names().collect();
        services.sort_unstable();
        let name = if services.is_empty() {
            "router".to_owned()
        } else {
            services.join(",")
        };
        let config = self.server_config();
        let mut dispatches = Vec::with_capacity(listeners.len());
        for _ in 0..listeners.len() {
            let mut shard = self.clone();
            shard.channelz =
                Some(crate::channelz::Registry::global_shared().register_server(name.clone()));
            dispatches.push(Arc::new(shard));
        }
        serve_per_core_shards(dispatches, config, listeners, shutdown, tls).await
    }
}

/// Returns `None` when `max_concurrent_rpcs` is unset. Otherwise a semaphore
/// of that many permits, created once per accept loop so every connection
/// shares the process-wide budget.
pub(crate) fn rpc_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .concurrent_rpc_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// `None` means refuse this peer. `Some(None)` means unlimited. `Some(Some(p))`
/// is a live slot held until the connection task drops it.
pub(crate) fn take_connection_slot(
    slots: &Option<Arc<Semaphore>>,
) -> Option<Option<tokio::sync::OwnedSemaphorePermit>> {
    match slots {
        None => Some(None),
        Some(sem) => sem.clone().try_acquire_owned().ok().map(Some),
    }
}

/// Accept connections until `shutdown` resolves, then drain in-flight work.
pub(crate) async fn accept_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    tls: Option<ServerTls>,
) -> Result<(), Status> {
    let slots = connection_slots(config);
    let rpcs = rpc_slots(config);
    accept_loop_with_slots(dispatch, listener, config, shutdown, tls, slots, rpcs).await
}

/// [`accept_loop`] with caller-supplied limit semaphores, so the per-core
/// mode can share the connection budget across shards while dividing the
/// per-RPC budget.
pub(crate) async fn accept_loop_with_slots<D: Dispatch>(
    dispatch: Arc<D>,
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    tls: Option<ServerTls>,
    slots: Option<Arc<Semaphore>>,
    rpcs: Option<Arc<Semaphore>>,
) -> Result<(), Status> {
    // Channelz: the listen socket, held for the whole accept loop.
    let _channelz_listen = dispatch.channelz_server().map(|server| {
        let local = listener
            .local_addr()
            .ok()
            .map(crate::channelz::EndpointAddr::Tcp);
        let security = if tls.is_some() {
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
            None,
            None,
            security,
            true,
        )
    });
    // Dropping every clone of `drain_tx` is what tells us the last connection
    // task has finished.
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
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
                    crate::tcp::tune(
                        &tcp,
                        config.tcp_keepalive_period(),
                        config.tcp_keepalive_probe_interval(),
                        config.tcp_keepalive_probe_retries(),
                    )
                    .ok();
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
                            let io = tokio::select! {
                                res = accept => match res {
                                    Ok(Ok(io)) => Some(io),
                                    _ => None,
                                },
                                _ = wait_for_drain(goaway.clone()) => None,
                            };
                            if let Some(io) = io {
                                let identity = crate::tls::peer_identity_of(&io);
                                let connection = ConnectionInfo::tls(peer, local, identity);
                                #[cfg(feature = "tonic")]
                                let connection = connection.with_tonic_tls(
                                    tonic::transport::server::Connected::connect_info(&io),
                                );
                                drop(
                                    serve_io(dispatch, io, connection, config, goaway, rpcs).await,
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
pub(crate) async fn accept_unix_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: UnixListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    // Channelz: the listen socket, held for the whole accept loop.
    let _channelz_listen = dispatch.channelz_server().map(|server| {
        let local = listener.local_addr().ok().and_then(|addr| {
            addr.as_pathname()
                .map(|path| crate::channelz::EndpointAddr::Uds(path.display().to_string()))
        });
        crate::channelz::Registry::global_shared().register_socket(
            crate::channelz::SocketParent::Server(server),
            local,
            None,
            None,
            crate::channelz::SocketSecurity::None,
            true,
        )
    });
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
            Ok((io, peer_addr)) => {
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
                    let connection = ConnectionInfo::unix(cred, Some(peer_addr));
                    #[cfg(feature = "tonic")]
                    let connection = connection
                        .with_tonic_uds(tonic::transport::server::Connected::connect_info(&io));
                    drop(serve_io(dispatch, io, connection, config, goaway, rpcs).await);
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
pub(crate) async fn accept_incoming<D: Dispatch, I: Incoming>(
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
