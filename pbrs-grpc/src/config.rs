//! Transport tuning and resource caps for servers and channels.

use crate::limits::MessageLimits;
use std::time::Duration;

/// Default HTTP/2 stream and connection window: 16 MiB.
///
/// Large enough that a single 4 MiB message never stalls on a
/// `WINDOW_UPDATE` round trip, which is where naive gRPC stacks lose most of
/// their large-payload throughput.
pub const DEFAULT_WINDOW_SIZE: u32 = 16 * 1024 * 1024;

/// Default HTTP/2 `SETTINGS_MAX_FRAME_SIZE`: 1 MiB.
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 1024 * 1024;

/// Default HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`: 256 in-flight RPCs.
pub const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 256;

/// Default HTTP/2 send buffer per connection: 1 MiB.
pub const DEFAULT_MAX_SEND_BUFFER_SIZE: usize = 1024 * 1024;

/// Default HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`: 16 KiB of metadata.
///
/// Caps how much header material a peer can force the HPACK decoder to
/// materialise per RPC.
pub const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 16 * 1024;

/// Default queue depth between a client-streaming caller and the wire.
///
/// Only outbound streams are queued; received streams are decoded on the
/// reading task.
pub const DEFAULT_STREAM_BUFFER: usize = 16;

/// How long to wait for a keepalive PING acknowledgement. Default 20 s.
pub const DEFAULT_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a client dial or a server TLS/HTTP/2 preface may take. Default 20 s.
///
/// Covers TCP (or Unix) connect, optional TLS, and the peer's HTTP/2
/// SETTINGS. A peer that accepts the socket and never speaks is dropped
/// instead of hanging the caller forever.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// After [`ServerConfig::max_connection_age`] or idle fires, how long to wait
/// for in-flight RPCs before dropping the socket. Default 10 s.
pub const DEFAULT_MAX_CONNECTION_AGE_GRACE: Duration = Duration::from_secs(10);

/// HTTP/2 rapid-reset cap: remotely-reset streams waiting in the accept queue.
///
/// h2's default, set explicitly. A peer that opens streams and immediately
/// `RST_STREAM`s them sits in that queue until accepted; exceeding this is
/// `ENHANCE_YOUR_CALM` and the connection is dropped.
pub const DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS: usize = 20;

/// The per-stream settings the wire layer needs: message caps plus how much
/// the connection will buffer before a write has to wait for flow control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wire {
    pub(crate) limits: MessageLimits,
    pub(crate) send_buffer: usize,
}

/// HTTP/2 and resource settings for a server.
///
/// Every field has a safe default; override only what you measured.
///
/// ```
/// use pbrs_grpc::ServerConfig;
///
/// let config = ServerConfig::new()
///     .max_decoding_message_size(1024 * 1024)
///     .max_concurrent_streams(1024);
/// assert_eq!(config.limits().max_decoding(), Some(1024 * 1024));
/// assert_eq!(config.concurrent_streams(), 1024);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    limits: MessageLimits,
    initial_stream_window_size: u32,
    initial_connection_window_size: u32,
    max_frame_size: u32,
    max_concurrent_streams: u32,
    max_send_buffer_size: usize,
    max_header_list_size: u32,
    max_pending_accept_reset_streams: usize,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
    tcp_keepalive: Option<Duration>,
    handshake_timeout: Duration,
    max_connection_age: Option<Duration>,
    max_connection_idle: Option<Duration>,
    max_connection_age_grace: Duration,
    timeout: Option<Duration>,
    max_concurrent_connections: Option<usize>,
    max_concurrent_rpcs: Option<usize>,
    send_compressed: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: MessageLimits::default(),
            initial_stream_window_size: DEFAULT_WINDOW_SIZE,
            initial_connection_window_size: DEFAULT_WINDOW_SIZE,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            max_send_buffer_size: DEFAULT_MAX_SEND_BUFFER_SIZE,
            max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
            max_pending_accept_reset_streams: DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS,
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
            tcp_keepalive: None,
            handshake_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_connection_age: None,
            max_connection_idle: None,
            max_connection_age_grace: DEFAULT_MAX_CONNECTION_AGE_GRACE,
            timeout: None,
            max_concurrent_connections: None,
            max_concurrent_rpcs: None,
            send_compressed: false,
        }
    }
}

impl ServerConfig {
    /// Safe defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cap inbound messages at `limit` uncompressed bytes. Default 4 MiB.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_decoding(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes. Default unlimited.
    /// Applies to every call shape.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_encoding(limit);
        self
    }

    /// Replace both message caps at once. Applies to every call shape.
    ///
    /// [`crate::Server::message_limits`], [`crate::Router::message_limits`],
    /// and generated `FooServer::message_limits` set this without building a
    /// [`ServerConfig`]. Distinct from [`Self::max_decoding_message_size`] /
    /// [`Self::max_encoding_message_size`]. Oversize inbound or outbound is
    /// [`crate::Code::ResourceExhausted`], including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// HTTP/2 per-stream receive window. Default 16 MiB.
    /// Applies to every call shape.
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.initial_stream_window_size = bytes;
        self
    }

    /// HTTP/2 per-connection receive window. Default 16 MiB.
    /// Applies to every call shape.
    /// A well-behaved client still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.initial_connection_window_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Default 1 MiB.
    /// Applies to every call shape.
    /// A well-behaved client splits DATA; every call shape still completes,
    /// including over TLS, mTLS, Unix, and [`crate::Server::serve_connection`].
    /// Distinct from [`Self::max_header_list_size`], which refuses oversize
    /// metadata, and from [`Self::max_concurrent_streams`], which serializes
    /// extra RPCs.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Concurrent RPCs allowed per connection. Default 256.
    /// Applies to every call shape.
    /// HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`. Distinct from
    /// [`Self::max_concurrent_rpcs`], which refuses extras as
    /// [`crate::Code::ResourceExhausted`]. A well-behaved client waits; both
    /// RPCs still complete, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Bytes buffered per connection before writes apply backpressure.
    /// Default 1 MiB. Applies to every call shape.
    /// Write backpressure still completes every call shape, including over
    /// TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from
    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS
    /// minimum, and from [`Self::initial_stream_window_size`], which still
    /// serves at a small receive window.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.max_send_buffer_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`, i.e. the metadata cap.
    /// Default 16 KiB. Applies to every call shape.
    /// Oversize metadata is refused, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`]. Distinct from a raw HTTP/2 peer.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.max_header_list_size = bytes;
        self
    }

    /// Cap remotely-reset HTTP/2 streams waiting in the accept queue.
    /// Applies to every call shape.
    ///
    /// Default 20 ([`DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS`]). A peer that
    /// opens streams and immediately `RST_STREAM`s them sits in that queue
    /// until accepted; exceeding this is `ENHANCE_YOUR_CALM` and the
    /// connection is dropped.
    /// A well-behaved client never fills that queue; every call shape still
    /// completes, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`]. Distinct from a raw HTTP/2 peer.
    ///
    /// [`crate::Server::max_pending_accept_reset_streams`],
    /// [`crate::Router::max_pending_accept_reset_streams`], and generated
    /// `FooServer::max_pending_accept_reset_streams` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, n: usize) -> Self {
        self.max_pending_accept_reset_streams = n;
        self
    }

    /// Send an HTTP/2 PING every `interval` so a dead peer is noticed before
    /// the next RPC. Disabled by default.
    ///
    /// This is not TCP keepalive. PINGs run on Unix sockets and TLS
    /// (including mTLS); they do not reset [`Self::max_connection_idle`]
    /// (idle is outstanding RPCs, not bytes on the wire). For `SO_KEEPALIVE`
    /// on TCP sockets, see [`Self::tcp_keepalive`]. Applies to every call
    /// shape.
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = Some(interval);
        self
    }

    /// How long to wait for a PING acknowledgement before dropping the
    /// connection. Default 20 s. Values below 1 ms are raised to 1 ms.
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Enable TCP `SO_KEEPALIVE` with this idle time before the first probe.
    ///
    /// Disabled by default. Values below 1 ms are raised to 1 ms. Only TCP
    /// sockets are affected; Unix domain sockets and [`crate::Channel::from_io`]
    /// streams are not. Probe interval and retry count stay at the kernel
    /// default.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.tcp_keepalive = Some(time.max(Duration::from_millis(1)));
        self
    }

    /// How long TLS accept (if any) and the HTTP/2 preface may each take.
    /// Default 20 s. Values below 1 ms are raised to 1 ms.
    /// Applies to every call shape, including over TLS, mTLS, and Unix.
    ///
    /// A client that opens a socket and never speaks is dropped, so it cannot
    /// pin a connection task forever. A completed handshake is not subject to
    /// this cap; use [`Self::max_connection_idle`] / [`Self::max_connection_age`]
    /// for live connections.
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Send GOAWAY this long after the connection is accepted. Disabled by
    /// default. Values below 1 ms are raised to 1 ms.
    ///
    /// The actual lifetime is jittered by ±10% so a process with many
    /// connections does not reconnect in lockstep. In-flight RPCs get
    /// [`Self::max_connection_age_grace`] to finish; a [`crate::Channel`] on
    /// the other end redials the next RPC of every call shape, including over
    /// TLS, mTLS, and Unix. Transparent retry of the same in-flight RPC
    /// after GOAWAY is unary and server-streaming after request bytes;
    /// client-streaming and bidi retry before HEADERS.
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.max_connection_age = Some(age.max(Duration::from_millis(1)));
        self
    }

    /// Send GOAWAY once the connection has had no outstanding RPCs for this
    /// long. Disabled by default. Values below 1 ms are raised to 1 ms.
    ///
    /// Idle is measured from accept until the first RPC, and from the moment
    /// the last in-flight RPC finishes thereafter. A long-running stream does
    /// not look idle, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`]. Keepalive PINGs do not count as
    /// activity. The next RPC
    /// of every call shape redials, including over TLS, mTLS, and Unix.
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.max_connection_idle = Some(idle.max(Duration::from_millis(1)));
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs before
    /// dropping the socket. Default 10 s. Values below 1 ms are raised to 1 ms.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// [`crate::Server::max_connection_age_grace`],
    /// [`crate::Router::max_connection_age_grace`], and generated
    /// `FooServer::max_connection_age_grace` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.max_connection_age_grace = grace.max(Duration::from_millis(1));
        self
    }

    /// Cap every RPC to this duration even when the client omits `grpc-timeout`.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// The effective deadline is the soonest of this, the client's, and any
    /// [`crate::Rpc::set_timeout`] from an interceptor. Disabled by default.
    /// Values below 1 ms are raised to 1 ms.
    ///
    /// [`crate::Server::timeout`], [`crate::Router::timeout`], and generated
    /// `FooServer::timeout` set this without building a [`ServerConfig`].
    /// Interceptors and handlers read it on [`crate::Rpc::rpc_timeout`] /
    /// [`crate::Request::rpc_timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout.max(Duration::from_millis(1)));
        self
    }

    /// Cap how many TCP/Unix connections the accept loop will serve at once,
    /// including TLS and mTLS listeners. Applies to every call shape.
    ///
    /// Further accepts are dropped immediately (the peer sees a reset), so an
    /// accept storm cannot pin an unbounded number of handshake tasks.
    /// Disabled by default.
    ///
    /// [`crate::Server::max_concurrent_connections`],
    /// [`crate::Router::max_concurrent_connections`], and generated
    /// `FooServer::max_concurrent_connections` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn max_concurrent_connections(mut self, n: usize) -> Self {
        self.max_concurrent_connections = Some(n);
        self
    }

    /// Cap how many RPCs the process will run at once, across every
    /// connection. Applies to every call shape, including over TLS, mTLS,
    /// Unix, and [`crate::Server::serve_connection`].
    ///
    /// Further RPCs are refused with [`crate::Code::ResourceExhausted`]
    /// before the handler runs. Distinct from
    /// [`Self::max_concurrent_streams`] (per HTTP/2 connection) and
    /// [`Self::max_concurrent_connections`] (accept-loop sockets). Disabled
    /// by default.
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.max_concurrent_rpcs = Some(n.max(1));
        self
    }

    /// gzip responses when the client advertises `gzip` in
    /// `grpc-accept-encoding`. Applies to every call shape.
    ///
    /// Off by default: compression is CPU for bandwidth, and at LAN
    /// latencies identity framing usually wins. A handler can still gzip one
    /// RPC with [`crate::Response::set_compress`]. A response that already
    /// called [`crate::Response::set_compress`] is left alone, including
    /// `set_compress(false)` to opt out of this overlay. A peer that did
    /// not advertise gzip is never sent a compressed frame.
    ///
    /// [`crate::Server::send_compressed`], [`crate::Router::send_compressed`],
    /// and generated `FooServer::send_compressed` enable this without
    /// building a [`ServerConfig`].
    #[must_use]
    pub fn send_compressed(mut self, enable: bool) -> Self {
        self.send_compressed = enable;
        self
    }

    /// Configured message caps. Applies to every call shape.
    #[must_use]
    pub fn limits(self) -> MessageLimits {
        self.limits
    }

    /// Configured per-connection send buffer. Applies to every call shape.
    #[must_use]
    pub fn send_buffer_size(self) -> usize {
        self.max_send_buffer_size
    }

    /// Configured per-RPC timeout, if any. See [`Self::timeout`].
    /// Applies to every call shape.
    /// Interceptors and handlers read this overlay on [`crate::Rpc::rpc_timeout`]
    /// / [`crate::Request::rpc_timeout`].
    #[must_use]
    pub fn rpc_timeout(self) -> Option<Duration> {
        self.timeout
    }

    /// Configured accept-loop connection cap, if any.
    /// See [`Self::max_concurrent_connections`]. Applies to every call shape.
    #[must_use]
    pub fn connection_limit(self) -> Option<usize> {
        self.max_concurrent_connections
    }

    /// Configured process-wide RPC cap, if any. See [`Self::max_concurrent_rpcs`].
    /// Applies to every call shape.
    #[must_use]
    pub fn concurrent_rpc_limit(self) -> Option<usize> {
        self.max_concurrent_rpcs
    }

    /// Whether responses are gzipped when the client accepts gzip.
    /// See [`Self::send_compressed`]. Applies to every call shape.
    #[must_use]
    pub fn compresses_outbound(self) -> bool {
        self.send_compressed
    }

    /// Configured HTTP/2 PING interval, if any. See [`Self::keep_alive_interval`].
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_ping_interval(self) -> Option<Duration> {
        self.keep_alive_interval
    }

    /// How long to wait for a PING acknowledgement. See [`Self::keep_alive_timeout`].
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_ack_timeout(self) -> Duration {
        self.keep_alive_timeout
    }

    /// Configured TCP keepalive idle time, if any. See [`Self::tcp_keepalive`].
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive_period(self) -> Option<Duration> {
        self.tcp_keepalive
    }

    /// HTTP/2 per-stream receive window. See [`Self::initial_stream_window_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn stream_window(self) -> u32 {
        self.initial_stream_window_size
    }

    /// HTTP/2 per-connection receive window. See [`Self::initial_connection_window_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn connection_window(self) -> u32 {
        self.initial_connection_window_size
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. See [`Self::max_frame_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn frame_size(self) -> u32 {
        self.max_frame_size
    }

    /// Concurrent RPCs allowed per connection. See [`Self::max_concurrent_streams`].
    /// Applies to every call shape.
    #[must_use]
    pub fn concurrent_streams(self) -> u32 {
        self.max_concurrent_streams
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. See [`Self::max_header_list_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn header_list_size(self) -> u32 {
        self.max_header_list_size
    }

    /// Remotely-reset HTTP/2 streams waiting in the accept queue.
    /// See [`Self::max_pending_accept_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn pending_accept_reset_streams(self) -> usize {
        self.max_pending_accept_reset_streams
    }

    /// TLS accept and HTTP/2 preface bound. See [`Self::handshake_timeout`].
    /// Applies to every call shape.
    #[must_use]
    pub fn handshake_wait(self) -> Duration {
        self.handshake_timeout
    }

    /// Configured max connection age, if any. The next RPC of every call
    /// shape redials. See [`Self::max_connection_age`].
    #[must_use]
    pub fn connection_age(self) -> Option<Duration> {
        self.max_connection_age
    }

    /// Configured max connection idle, if any. See [`Self::max_connection_idle`].
    /// Applies to every call shape.
    #[must_use]
    pub fn connection_idle(self) -> Option<Duration> {
        self.max_connection_idle
    }

    /// Grace after age or idle. See [`Self::max_connection_age_grace`].
    /// Applies to every call shape.
    #[must_use]
    pub fn age_grace(self) -> Duration {
        self.max_connection_age_grace
    }

    pub(crate) fn keepalive(self) -> (Option<Duration>, Duration) {
        (self.keep_alive_interval, self.keep_alive_timeout)
    }

    pub(crate) fn io_handshake_timeout(self) -> Duration {
        self.handshake_wait()
    }

    pub(crate) fn connection_lifetime(self) -> (Option<Duration>, Option<Duration>, Duration) {
        (
            self.max_connection_age,
            self.max_connection_idle,
            self.max_connection_age_grace,
        )
    }

    pub(crate) fn wire(self) -> Wire {
        Wire {
            limits: self.limits,
            send_buffer: self.max_send_buffer_size,
        }
    }

    pub(crate) fn h2_builder(self) -> h2::server::Builder {
        let mut builder = h2::server::Builder::new();
        builder
            .initial_window_size(self.initial_stream_window_size)
            .initial_connection_window_size(self.initial_connection_window_size)
            .max_frame_size(self.max_frame_size)
            .max_concurrent_streams(self.max_concurrent_streams)
            .max_send_buffer_size(self.max_send_buffer_size)
            .max_header_list_size(self.max_header_list_size)
            .max_pending_accept_reset_streams(self.max_pending_accept_reset_streams);
        builder
    }
}

/// Spread `age` by ±10% so a fleet does not GOAWAY in lockstep.
///
/// grpc-go hard-codes the same ratio on `MaxConnectionAge`.
pub(crate) fn jitter_age(age: Duration, seed: u64) -> Duration {
    const SPAN: u64 = 201; // 0..=200 thousandths added to 900
    let thousandths = 900 + (seed % SPAN);
    let nanos = age.as_nanos().saturating_mul(u128::from(thousandths)) / 1000;
    let nanos = u64::try_from(nanos).unwrap_or(u64::MAX);
    Duration::from_nanos(nanos).max(Duration::from_millis(1))
}

/// HTTP/2 and resource settings for a [`Channel`](crate::Channel).
///
/// ```
/// use std::time::Duration;
/// use pbrs_grpc::ChannelConfig;
///
/// let config = ChannelConfig::new()
///     .connections(4)
///     .connect_timeout(Duration::from_secs(5))
///     .max_connection_idle(Duration::from_secs(5 * 60));
/// assert_eq!(config.connection_count(), 4);
/// assert_eq!(config.dial_timeout(), Duration::from_secs(5));
/// assert_eq!(config.connection_idle(), Some(Duration::from_secs(5 * 60)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelConfig {
    limits: MessageLimits,
    connections: usize,
    initial_stream_window_size: u32,
    initial_connection_window_size: u32,
    max_frame_size: u32,
    max_concurrent_streams: u32,
    max_send_buffer_size: usize,
    max_header_list_size: u32,
    max_pending_accept_reset_streams: usize,
    stream_buffer: usize,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
    tcp_keepalive: Option<Duration>,
    connect_timeout: Duration,
    max_connection_idle: Option<Duration>,
    send_compressed: bool,
    timeout: Option<Duration>,
    wait_for_ready: bool,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            limits: MessageLimits::default(),
            connections: 1,
            initial_stream_window_size: DEFAULT_WINDOW_SIZE,
            initial_connection_window_size: DEFAULT_WINDOW_SIZE,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            max_send_buffer_size: DEFAULT_MAX_SEND_BUFFER_SIZE,
            max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
            max_pending_accept_reset_streams: DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS,
            stream_buffer: DEFAULT_STREAM_BUFFER,
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
            tcp_keepalive: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_connection_idle: None,
            send_compressed: false,
            timeout: None,
            wait_for_ready: false,
        }
    }
}

impl ChannelConfig {
    /// Safe defaults: one connection, 4 MiB inbound cap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open `n` independent HTTP/2 connections and spread RPCs round-robin.
    /// Applies to every call shape, including over TLS, mTLS, and Unix.
    /// [`crate::Channel::from_io`] cannot pool: [`crate::Channel::from_io_with`]
    /// forces `connections` to 1.
    /// All of them must succeed: a pool larger than the server's
    /// [`crate::Server::max_concurrent_connections`] fails the dial as
    /// [`crate::Code::Unavailable`].
    ///
    /// One connection means one `h2` driver task, so one core drives all
    /// framing. Raising this is the single biggest throughput lever for
    /// concurrent small RPCs; see [the tuning guide](crate#tuning).
    ///
    /// A slot that later dies is redialed on the next RPC that lands on it;
    /// the other slots keep serving.
    #[must_use]
    pub fn connections(mut self, n: usize) -> Self {
        self.connections = n.max(1);
        self
    }

    /// Cap inbound messages at `limit` uncompressed bytes. Default 4 MiB.
    /// Applies to every call shape, including when set on
    /// [`crate::Channel::connect_tls_with`] / [`crate::Channel::connect_unix_with`]
    /// / [`crate::Channel::from_io_with`]. Distinct from wrapping a live
    /// [`crate::Channel`] with [`crate::Channel::max_decoding_message_size`].
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_decoding(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes. Default unlimited.
    /// Applies to every call shape, including when set on
    /// [`crate::Channel::connect_tls_with`] / [`crate::Channel::connect_unix_with`]
    /// / [`crate::Channel::from_io_with`]. Distinct from wrapping a live
    /// [`crate::Channel`] with [`crate::Channel::max_encoding_message_size`].
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_encoding(limit);
        self
    }

    /// Replace both message caps at once. Applies to every call shape.
    ///
    /// [`crate::Channel::message_limits`] and generated `FooClient::message_limits`
    /// set this without building a [`ChannelConfig`].
    /// Dial-time overlay on [`crate::Channel::connect_tls_with`] /
    /// [`crate::Channel::connect_unix_with`] / [`crate::Channel::from_io_with`].
    /// Distinct from [`Self::max_encoding_message_size`] /
    /// [`Self::max_decoding_message_size`].
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// HTTP/2 per-stream receive window. Default 16 MiB.
    /// Applies to every call shape.
    /// HTTP/2 stream receive window the client advertises. Distinct from
    /// [`ServerConfig::initial_stream_window_size`], which still serves when
    /// the server advertises a small window. A well-behaved server still
    /// completes every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.initial_stream_window_size = bytes;
        self
    }

    /// HTTP/2 per-connection receive window. Default 16 MiB.
    /// Applies to every call shape.
    /// HTTP/2 connection receive window the client advertises. Distinct from
    /// [`ServerConfig::initial_connection_window_size`], which still serves when
    /// the server advertises a small window. A well-behaved server still
    /// completes every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.initial_connection_window_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Default 1 MiB.
    /// Applies to every call shape.
    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE` the client advertises. Distinct
    /// from [`ServerConfig::max_frame_size`], which still serves every call
    /// shape when the server advertises a small cap. A well-behaved server
    /// splits DATA, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Concurrent RPCs allowed per connection. Default 256.
    /// Applies to every call shape.
    /// HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS` the client advertises. Distinct
    /// from [`ServerConfig::max_concurrent_streams`], which serializes extra
    /// RPCs on the server. Push is disabled, including over TLS, mTLS, Unix,
    /// and [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Bytes buffered per connection before writes apply backpressure.
    /// Default 1 MiB. Applies to every call shape.
    /// HTTP/2 send buffer the client applies on outbound frames. Distinct from
    /// [`ServerConfig::max_send_buffer_size`], which still serves when the
    /// server advertises a small buffer. A well-behaved server still completes
    /// every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.max_send_buffer_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`, i.e. the metadata cap the
    /// client will accept from the peer. Default 16 KiB. Applies to every
    /// call shape.
    /// Oversize response headers or trailers are refused, including over TLS,
    /// mTLS, Unix, and [`crate::Channel::from_io`]. Distinct from
    /// [`ServerConfig::max_header_list_size`], which caps inbound request
    /// metadata.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.max_header_list_size = bytes;
        self
    }

    /// Cap remotely-reset HTTP/2 streams waiting in the accept queue.
    /// Applies to every call shape.
    ///
    /// Default 20 ([`DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS`]). A peer that
    /// opens streams and immediately `RST_STREAM`s them sits in that queue
    /// until accepted; exceeding this is `ENHANCE_YOUR_CALM` and the
    /// connection is dropped. Applied at handshake, not as a live overlay.
    /// Distinct from [`ServerConfig::max_pending_accept_reset_streams`], which
    /// still serves when the server caps that queue. A well-behaved server
    /// never fills this client queue; every call shape still completes,
    /// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_pending_accept_reset_streams(mut self, n: usize) -> Self {
        self.max_pending_accept_reset_streams = n;
        self
    }

    /// Messages queued between a client-streaming caller and the wire.
    /// Default 16. Applies to client-streaming and bidi request streams.
    ///
    /// The wire layer sends whatever is queued as one batch, so deeper means
    /// fewer and larger writes at the cost of memory. Received streams are
    /// decoded inline and are not queued, so this does not affect them.
    /// [`crate::Channel::stream_buffer`] sets this on a live clone without
    /// building a [`ChannelConfig`].
    #[must_use]
    pub fn stream_buffer(mut self, messages: usize) -> Self {
        self.stream_buffer = messages.max(1);
        self
    }

    /// Send an HTTP/2 PING every `interval` so a dead peer is noticed before
    /// the next RPC. Disabled by default. PINGs are sent while idle as well
    /// as while RPCs are in flight. They do not reset
    /// [`Self::max_connection_idle`].
    ///
    /// This is not TCP keepalive. PINGs run on Unix sockets, TLS (including
    /// mTLS), and [`crate::Channel::from_io`]. For `SO_KEEPALIVE` on TCP
    /// sockets, see [`Self::tcp_keepalive`]. Applies to every call shape.
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = Some(interval);
        self
    }

    /// How long to wait for a PING acknowledgement before dropping the
    /// connection. Default 20 s. Values below 1 ms are raised to 1 ms.
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Enable TCP `SO_KEEPALIVE` with this idle time before the first probe.
    ///
    /// Disabled by default. Values below 1 ms are raised to 1 ms. Only TCP
    /// sockets are affected; Unix domain sockets and [`crate::Channel::from_io`]
    /// streams are not. Probe interval and retry count stay at the kernel
    /// default.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.tcp_keepalive = Some(time.max(Duration::from_millis(1)));
        self
    }

    /// How long a dial may take: TCP (or Unix) connect, optional TLS, and
    /// the peer's HTTP/2 SETTINGS. Default 20 s. Values below 1 ms are raised
    /// to 1 ms.
    ///
    /// This is a dial bound, not an RPC overlay. Every call shape uses the
    /// same bound when the channel actually dials (eager `connect`, a lazy
    /// first RPC, or a reconnect). Applies to every call shape once that
    /// dial happens.
    ///
    /// Always on. A peer that accepts the socket and never speaks HTTP/2
    /// (or never finishes TLS, including mTLS) fails with
    /// [`crate::Code::Unavailable`] instead of hanging [`crate::Channel::connect`]
    /// / [`crate::Channel::connect_tls`] / [`crate::Channel::connect_unix`] forever.
    /// Connection refused still fails immediately on those dialers; this bound is
    /// for the hang, not the bounce.
    ///
    /// Wait-for-ready treats the timeout as `UNAVAILABLE` and retries with
    /// backoff. An RPC deadline still races the dial.
    #[must_use]
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Close a connection that has had no outstanding RPCs for this long.
    /// Disabled by default. Values below 1 ms are raised to 1 ms.
    ///
    /// The socket is actually torn down (the HTTP/2 driver stops), not merely
    /// skipped on the next RPC. A long-running stream does not look idle.
    /// Keepalive PINGs do not count as activity. The next RPC of every call
    /// shape redials, including over TLS, mTLS, and Unix, except on
    /// [`crate::Channel::from_io`], which cannot redial and fails with
    /// [`crate::Code::Unavailable`].
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.max_connection_idle = Some(idle.max(Duration::from_millis(1)));
        self
    }

    /// gzip request payloads (and [`crate::StreamSender::send`] on a stream).
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    ///
    /// Off by default. The kernel always advertises `identity,gzip`, so a
    /// server that implements gzip will accept these frames. Per-RPC
    /// [`crate::Request::set_compress`] still works when this is off. A
    /// request that already called [`crate::Request::set_compress`] is left
    /// alone, including `set_compress(false)` to opt out of this overlay.
    /// A later interceptor can still set or clear it, including
    /// `clear_compress` then `set_compress(compresses_outbound())` to
    /// reapply.
    #[must_use]
    pub fn send_compressed(mut self, enable: bool) -> Self {
        self.send_compressed = enable;
        self
    }

    /// Default per-RPC deadline when the request omits `grpc-timeout`.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    ///
    /// Distinct from [`Self::connect_timeout`], which bounds the dial. Disabled
    /// by default. Values below 1 ms are raised to 1 ms. A request that already
    /// has a deadline is left alone; a later interceptor can still
    /// [`crate::Outgoing::set_timeout`] or [`crate::Outgoing::clear_timeout`].
    ///
    /// [`crate::Channel::timeout`] and generated `FooClient::timeout` set this
    /// without building a [`ChannelConfig`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout.max(Duration::from_millis(1)));
        self
    }

    /// Default wait-for-ready when the request omits it. Applies to every
    /// call shape.
    ///
    /// Off by default (gRPC fail-fast). A request that already called
    /// [`crate::Request::set_wait_for_ready`] is left alone; a later interceptor
    /// can still set or clear it.
    ///
    /// [`crate::Channel::wait_for_ready`] and generated `FooClient::wait_for_ready`
    /// set this without building a [`ChannelConfig`].
    #[must_use]
    pub fn wait_for_ready(mut self, wait: bool) -> Self {
        self.wait_for_ready = wait;
        self
    }

    /// Configured message caps. Applies to every call shape.
    #[must_use]
    pub fn limits(self) -> MessageLimits {
        self.limits
    }

    /// Configured connection count. Applies to every call shape.
    #[must_use]
    pub fn connection_count(self) -> usize {
        self.connections
    }

    /// Configured outbound streaming queue depth. Applies to client-streaming
    /// and bidi request streams. See [`Self::stream_buffer`].
    #[must_use]
    pub fn stream_buffer_size(self) -> usize {
        self.stream_buffer
    }

    /// Configured per-connection send buffer. Applies to every call shape.
    #[must_use]
    pub fn send_buffer_size(self) -> usize {
        self.max_send_buffer_size
    }

    /// Configured HTTP/2 PING interval, if any. See [`Self::keep_alive_interval`].
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_ping_interval(self) -> Option<Duration> {
        self.keep_alive_interval
    }

    /// How long to wait for a PING acknowledgement. See [`Self::keep_alive_timeout`].
    /// Applies to every call shape.
    #[must_use]
    pub fn keep_alive_ack_timeout(self) -> Duration {
        self.keep_alive_timeout
    }

    /// Configured TCP keepalive idle time, if any. See [`Self::tcp_keepalive`].
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive_period(self) -> Option<Duration> {
        self.tcp_keepalive
    }

    /// HTTP/2 per-stream receive window. See [`Self::initial_stream_window_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn stream_window(self) -> u32 {
        self.initial_stream_window_size
    }

    /// HTTP/2 per-connection receive window. See [`Self::initial_connection_window_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn connection_window(self) -> u32 {
        self.initial_connection_window_size
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. See [`Self::max_frame_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn frame_size(self) -> u32 {
        self.max_frame_size
    }

    /// Concurrent RPCs allowed per connection. See [`Self::max_concurrent_streams`].
    /// Applies to every call shape.
    #[must_use]
    pub fn concurrent_streams(self) -> u32 {
        self.max_concurrent_streams
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. See [`Self::max_header_list_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn header_list_size(self) -> u32 {
        self.max_header_list_size
    }

    /// Remotely-reset HTTP/2 streams waiting in the accept queue.
    /// See [`Self::max_pending_accept_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn pending_accept_reset_streams(self) -> usize {
        self.max_pending_accept_reset_streams
    }

    /// Dial bound: TCP/Unix connect, optional TLS, peer SETTINGS.
    /// See [`Self::connect_timeout`]. Applies to every call shape once that
    /// dial happens.
    #[must_use]
    pub fn dial_timeout(self) -> Duration {
        self.connect_timeout
    }

    /// Configured max connection idle, if any. See [`Self::max_connection_idle`].
    /// Applies to every call shape.
    #[must_use]
    pub fn connection_idle(self) -> Option<Duration> {
        self.max_connection_idle
    }

    /// Whether request payloads are gzipped. See [`Self::send_compressed`].
    /// Applies to every call shape.
    #[must_use]
    pub fn compresses_outbound(self) -> bool {
        self.send_compressed
    }

    /// Configured default per-RPC deadline, if any. See [`Self::timeout`].
    /// Applies to every call shape.
    #[must_use]
    pub fn rpc_timeout(self) -> Option<Duration> {
        self.timeout
    }

    /// Configured default wait-for-ready. See [`Self::wait_for_ready`].
    /// Applies to every call shape.
    #[must_use]
    pub fn waits_for_ready(self) -> bool {
        self.wait_for_ready
    }

    pub(crate) fn keepalive(self) -> (Option<Duration>, Duration) {
        (self.keep_alive_interval, self.keep_alive_timeout)
    }

    /// Bound used by the client handshake. Named apart from
    /// [`Self::connect_timeout`] so the setter stays the gRPC name.
    pub(crate) fn handshake_timeout(self) -> Duration {
        self.dial_timeout()
    }

    pub(crate) fn wire(self) -> Wire {
        Wire {
            limits: self.limits,
            send_buffer: self.max_send_buffer_size,
        }
    }

    pub(crate) fn h2_builder(self) -> h2::client::Builder {
        let mut builder = h2::client::Builder::new();
        builder
            .initial_window_size(self.initial_stream_window_size)
            .initial_connection_window_size(self.initial_connection_window_size)
            .max_frame_size(self.max_frame_size)
            .max_concurrent_streams(self.max_concurrent_streams)
            .max_send_buffer_size(self.max_send_buffer_size)
            .max_header_list_size(self.max_header_list_size)
            .max_pending_accept_reset_streams(self.max_pending_accept_reset_streams)
            .enable_push(false)
            // h2's handshake future returns after writing the client preface,
            // before the peer speaks. Starting send capacity at 0 lets
            // `finish_h2` wait until `current_max_send_streams` leaves 0,
            // which is when the peer's SETTINGS has been applied.
            .initial_max_send_streams(0);
        builder
    }
}

#[cfg(test)]
mod tests {
    use super::{ChannelConfig, ServerConfig};
    use std::time::Duration;

    #[test]
    fn server_defaults_are_safe() {
        let config = ServerConfig::new();
        assert_eq!(config.limits().max_decoding(), Some(4 * 1024 * 1024));
        assert_eq!(config.send_buffer_size(), 1024 * 1024);
        assert_eq!(config.rpc_timeout(), None);
        assert_eq!(config.connection_limit(), None);
        assert_eq!(config.concurrent_rpc_limit(), None);
        assert_eq!(config.tcp_keepalive_period(), None);
        assert_eq!(config.keep_alive_ping_interval(), None);
        assert_eq!(config.stream_window(), super::DEFAULT_WINDOW_SIZE);
        assert_eq!(config.connection_window(), super::DEFAULT_WINDOW_SIZE);
        assert_eq!(config.frame_size(), super::DEFAULT_MAX_FRAME_SIZE);
        assert_eq!(
            config.concurrent_streams(),
            super::DEFAULT_MAX_CONCURRENT_STREAMS
        );
        assert_eq!(
            config.header_list_size(),
            super::DEFAULT_MAX_HEADER_LIST_SIZE
        );
        assert_eq!(
            config.pending_accept_reset_streams(),
            super::DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS
        );
        assert_eq!(
            ChannelConfig::new().pending_accept_reset_streams(),
            super::DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS
        );
        assert_eq!(
            ServerConfig::new()
                .max_pending_accept_reset_streams(5)
                .pending_accept_reset_streams(),
            5
        );
        assert_eq!(
            ChannelConfig::new()
                .max_pending_accept_reset_streams(5)
                .pending_accept_reset_streams(),
            5
        );
        assert_eq!(
            ServerConfig::new()
                .max_pending_accept_reset_streams(0)
                .pending_accept_reset_streams(),
            0
        );
        assert_eq!(config.handshake_wait(), super::DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(config.connection_age(), None);
        assert_eq!(config.connection_idle(), None);
        assert_eq!(config.age_grace(), super::DEFAULT_MAX_CONNECTION_AGE_GRACE);
        assert!(!config.compresses_outbound());
        assert_eq!(ChannelConfig::new().connection_idle(), None);
        assert_eq!(ChannelConfig::new().rpc_timeout(), None);
        assert!(!ChannelConfig::new().waits_for_ready());
        assert!(ChannelConfig::new().wait_for_ready(true).waits_for_ready());
    }

    #[test]
    fn server_timeout_and_connection_cap_round_trip() {
        let config = ServerConfig::new()
            .timeout(Duration::from_millis(0))
            .max_concurrent_connections(4)
            .max_concurrent_rpcs(0);
        assert_eq!(config.rpc_timeout(), Some(Duration::from_millis(1)));
        assert_eq!(config.connection_limit(), Some(4));
        assert_eq!(config.concurrent_rpc_limit(), Some(1));
    }

    #[test]
    fn channel_timeout_never_zero() {
        assert_eq!(
            ChannelConfig::new()
                .timeout(Duration::from_millis(0))
                .rpc_timeout(),
            Some(Duration::from_millis(1))
        );
    }

    #[test]
    fn tcp_keepalive_never_zero() {
        assert_eq!(
            ServerConfig::new()
                .tcp_keepalive(Duration::from_millis(0))
                .tcp_keepalive_period(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            ChannelConfig::new()
                .tcp_keepalive(Duration::from_millis(0))
                .tcp_keepalive_period(),
            Some(Duration::from_millis(1))
        );
    }

    #[test]
    fn channel_connections_never_zero() {
        assert_eq!(ChannelConfig::new().connections(0).connection_count(), 1);
    }

    #[test]
    fn stream_buffer_never_zero() {
        assert_eq!(
            ChannelConfig::new().stream_buffer(0).stream_buffer_size(),
            1
        );
    }

    #[test]
    fn keep_alive_timeout_never_zero() {
        assert_eq!(
            ServerConfig::new()
                .keep_alive_timeout(Duration::from_millis(0))
                .keepalive()
                .1,
            Duration::from_millis(1)
        );
        assert_eq!(
            ChannelConfig::new()
                .keep_alive_timeout(Duration::from_millis(0))
                .keepalive()
                .1,
            Duration::from_millis(1)
        );
    }

    #[test]
    fn connect_timeout_never_zero() {
        assert_eq!(
            ChannelConfig::new()
                .connect_timeout(Duration::from_millis(0))
                .dial_timeout(),
            Duration::from_millis(1)
        );
        assert_eq!(
            ServerConfig::new()
                .handshake_timeout(Duration::from_millis(0))
                .handshake_wait(),
            Duration::from_millis(1)
        );
    }

    #[test]
    fn connection_age_never_zero() {
        let config = ServerConfig::new()
            .max_connection_age(Duration::from_millis(0))
            .max_connection_idle(Duration::from_millis(0))
            .max_connection_age_grace(Duration::from_millis(0));
        let (age, idle, grace) = config.connection_lifetime();
        assert_eq!(age, Some(Duration::from_millis(1)));
        assert_eq!(idle, Some(Duration::from_millis(1)));
        assert_eq!(grace, Duration::from_millis(1));
        assert_eq!(config.connection_age(), Some(Duration::from_millis(1)));
        assert_eq!(config.connection_idle(), Some(Duration::from_millis(1)));
        assert_eq!(config.age_grace(), Duration::from_millis(1));
        assert_eq!(
            ChannelConfig::new()
                .max_connection_idle(Duration::from_millis(0))
                .connection_idle(),
            Some(Duration::from_millis(1))
        );
    }

    #[test]
    fn http2_knobs_round_trip() {
        let server = ServerConfig::new()
            .initial_stream_window_size(1)
            .initial_connection_window_size(2)
            .max_frame_size(16_384)
            .max_concurrent_streams(8)
            .max_header_list_size(32)
            .max_pending_accept_reset_streams(3);
        assert_eq!(server.stream_window(), 1);
        assert_eq!(server.connection_window(), 2);
        assert_eq!(server.frame_size(), 16_384);
        assert_eq!(server.concurrent_streams(), 8);
        assert_eq!(server.header_list_size(), 32);
        assert_eq!(server.pending_accept_reset_streams(), 3);

        let channel = ChannelConfig::new()
            .initial_stream_window_size(3)
            .initial_connection_window_size(4)
            .max_frame_size(16_384)
            .max_concurrent_streams(9)
            .max_header_list_size(64)
            .max_pending_accept_reset_streams(11);
        assert_eq!(channel.stream_window(), 3);
        assert_eq!(channel.connection_window(), 4);
        assert_eq!(channel.frame_size(), 16_384);
        assert_eq!(channel.concurrent_streams(), 9);
        assert_eq!(channel.header_list_size(), 64);
        assert_eq!(channel.pending_accept_reset_streams(), 11);
        assert!(!ChannelConfig::new().compresses_outbound());
        assert!(ChannelConfig::new()
            .send_compressed(true)
            .compresses_outbound());
        assert!(ServerConfig::new()
            .send_compressed(true)
            .compresses_outbound());
    }

    #[test]
    fn age_jitter_is_plus_or_minus_ten_percent() {
        let age = Duration::from_secs(100);
        assert_eq!(super::jitter_age(age, 0), Duration::from_secs(90));
        assert_eq!(super::jitter_age(age, 200), Duration::from_secs(110));
        assert_ne!(super::jitter_age(age, 1), super::jitter_age(age, 2));
    }
}
