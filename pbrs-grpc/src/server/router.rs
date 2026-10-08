//! Path routing: [`split_path`] and [`Router`].

use super::accept::Incoming;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::IncomingAccept;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::{
    Server, accept_incoming, accept_loop, accept_unix_loop, bind, bind_unix, bind_unix_unlink,
};
use super::connection::serve_one;
use super::dispatch::{Dispatch, DynService, Service};
use super::rpc::Rpc;
use crate::config::ServerConfig;
use crate::limits::{ByteBudgetTracker, MessageLimits};
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::Request;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::status::{Code, Status};
use crate::telemetry::{LifecycleObserver, ObserverChain};
use crate::tls::ServerTls;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;

/// Split `/service/method` without allocating. Unparseable paths yield empty
/// halves, which route to `UNIMPLEMENTED`.
pub(crate) fn split_path(path: &str) -> (&str, &str) {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match rest.rsplit_once('/') {
        Some((service, method)) => (service, method),
        None => ("", ""),
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
/// There is no grpc-go `UnknownServiceHandler` setter: that is a catch-all
/// bidi handler for unregistered services. An unmounted service is
/// [`crate::Code::Unimplemented`], not a fallback [`Service`]. Distinct from
/// [`Server`]: that is one service; an unknown method there is still
/// unimplemented, not a catch-all. Distinct from [`Service::ALIASES`]: that
/// is a known path alias, not an unknown-service handler.
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
pub struct Router {
    routes: HashMap<&'static str, Arc<dyn DynService>>,
    config: ServerConfig,
    pub(crate) interceptor: Option<Arc<dyn crate::Interceptor>>,
    pub(crate) response_interceptor: Option<crate::interceptor::ResponseHook>,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) byte_budget: ByteBudgetTracker,
    pub(crate) binlog: Option<Arc<crate::binlog::BinaryLogger>>,
    /// Channelz server registration, held for the whole serve so the
    /// server (and its sockets) stays listed while serving.
    pub(crate) channelz: Option<crate::channelz::ServerHandle>,
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
            .field("observer", &self.observer.is_some())
            .field("binary_logger", &self.binlog.is_some())
            .finish()
    }
}

impl Clone for Router {
    fn clone(&self) -> Self {
        // The channelz server registers at serve time, so a clone never
        // carries a registration.
        Self {
            routes: self.routes.clone(),
            config: self.config,
            interceptor: self.interceptor.clone(),
            response_interceptor: self.response_interceptor.clone(),
            observer: self.observer.clone(),
            byte_budget: self.byte_budget.clone(),
            binlog: self.binlog.clone(),
            channelz: None,
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

fn path_matches_service_or_alias<S: Service>(path: &str) -> bool {
    Router::path_matches_service(path, S::NAME)
        || S::ALIASES
            .iter()
            .any(|alias| Router::path_matches_service(path, alias))
}

macro_rules! impl_tuple_service {
    ($($idx:tt : $ty:ident => $variant:ident),+ $(,)?) => {
        impl<$($ty: Service),+> Service for ($($ty,)+) {
            const NAME: &'static str = "pbrs_grpc.StaticRouter";

            fn call(&self, rpc: Rpc) -> impl Future<Output = ()> + Send {
                enum Route {
                    $($variant,)+
                    Unknown,
                }

                let path = rpc.path();
                let route = if false {
                    Route::Unknown
                } $(
                    else if path_matches_service_or_alias::<$ty>(path) {
                        Route::$variant
                    }
                )+ else {
                    Route::Unknown
                };

                async move {
                    match route {
                        $(Route::$variant => self.$idx.call(rpc).await,)+
                        Route::Unknown => rpc.unimplemented(),
                    }
                }
            }
        }
    };
}

impl_tuple_service!(0: A => A);
impl_tuple_service!(0: A => A, 1: B => B);
impl_tuple_service!(0: A => A, 1: B => B, 2: C => C);
impl_tuple_service!(0: A => A, 1: B => B, 2: C => C, 3: D => D);
impl_tuple_service!(0: A => A, 1: B => B, 2: C => C, 3: D => D, 4: E => E);
impl_tuple_service!(
    0: A => A,
    1: B => B,
    2: C => C,
    3: D => D,
    4: E => E,
    5: F => F
);

impl Router {
    /// An empty router with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            config: ServerConfig::default(),
            interceptor: None,
            response_interceptor: None,
            observer: None,
            byte_budget: ByteBudgetTracker::default(),
            binlog: None,
            channelz: None,
        }
    }

    /// Whether `path` is under `/<service>/` without allocating or hashing.
    ///
    /// Generated static routers use this helper before dispatching directly
    /// to a concrete [`Service`]. It deliberately accepts an empty method half:
    /// the service then returns `UNIMPLEMENTED`, matching dynamic [`Router`]
    /// behavior for `/<service>/`.
    #[doc(hidden)]
    #[must_use]
    pub fn path_matches_service(path: &str, service: &str) -> bool {
        let Some(rest) = path.strip_prefix('/') else {
            return false;
        };
        let Some(rest) = rest.strip_prefix(service) else {
            return false;
        };
        rest.starts_with('/')
    }

    /// Register the channelz server entity for this serve, named for
    /// the mounted services. Every `serve_*` choke point calls this
    /// before entering its accept loop.
    fn with_channelz(mut self) -> Self {
        let mut services: Vec<&str> = self.routes.keys().copied().collect();
        services.sort_unstable();
        let name = if services.is_empty() {
            "router".to_owned()
        } else {
            services.join(",")
        };
        self.channelz = Some(crate::channelz::Registry::global_shared().register_server(name));
        self
    }

    /// Set a transport byte budget for queued and in-flight buffers.
    #[must_use]
    pub fn byte_budget(mut self, limit: usize) -> Self {
        self.byte_budget = ByteBudgetTracker::with_limit(limit);
        self
    }

    /// Attach an existing [`crate::ByteBudgetTracker`] to this router.
    #[must_use]
    pub fn with_byte_budget_tracker(mut self, tracker: ByteBudgetTracker) -> Self {
        self.byte_budget = tracker;
        self
    }

    /// Byte budget tracker in effect.
    #[must_use]
    pub fn byte_budget_tracker(&self) -> &ByteBudgetTracker {
        &self.byte_budget
    }

    /// Number of bytes currently allocated in transport buffers on this router.
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

    /// Enforce a gRFC A43 authorization policy on every mounted service.
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

    /// Register a lifecycle telemetry observer.
    ///
    /// The observer receives lifecycle events for all routed RPCs:
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
    /// Applies to every call shape on every mounted service. Methods the
    /// logger's filter excludes cost one branch and log nothing. Credential
    /// headers are omitted and sensitive metadata values masked; see
    /// [`BinaryLogger`](crate::binlog::BinaryLogger).
    /// Distinct from [`Self::observer`]: that reports lifecycle events to
    /// telemetry; this records wire-faithful RPC logs to a sink.
    #[must_use]
    pub fn binary_logger(mut self, logger: crate::binlog::BinaryLogger) -> Self {
        self.binlog = Some(Arc::new(logger));
        self
    }

    pub(crate) fn add_arc<S: Service>(mut self, service: Arc<S>) -> Self {
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
        accept_loop(
            Arc::new(self.with_channelz()),
            listener,
            config,
            shutdown,
            None,
        )
        .await
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
        accept_unix_loop(Arc::new(self.with_channelz()), listener, config, shutdown).await
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
        accept_loop(
            Arc::new(self.with_channelz()),
            listener,
            config,
            shutdown,
            Some(tls),
        )
        .await
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
        serve_one(Arc::new(self.with_channelz()), io, None, config).await
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
        accept_incoming(Arc::new(self.with_channelz()), incoming, config, shutdown).await
    }
}

impl Dispatch for Router {
    async fn dispatch(&self, mut rpc: Rpc) {
        rpc.response_interceptor = self.response_interceptor.clone();
        rpc.observer = self.observer.clone();
        if let Some(tap) = self
            .binlog
            .as_ref()
            .and_then(|binlog| binlog.start_call(rpc.path(), crate::binlog::Logger::Server))
        {
            if let Some(peer) = rpc.remote_addr() {
                tap.set_peer(peer);
            }
            tap.log_client_header(
                rpc.metadata(),
                rpc.path(),
                rpc.authority().unwrap_or(""),
                rpc.effective_timeout(),
            );
            rpc.binlog = Some(tap);
        }
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

    fn observer(&self) -> Option<&Arc<dyn LifecycleObserver>> {
        self.observer.as_ref()
    }

    fn byte_budget(&self) -> ByteBudgetTracker {
        self.byte_budget.clone()
    }

    fn channelz_server(&self) -> Option<crate::channelz::ServerId> {
        self.channelz.as_ref().map(|handle| handle.id())
    }
}
