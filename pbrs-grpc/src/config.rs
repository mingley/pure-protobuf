//! Transport tuning and resource caps for servers and channels.

use crate::limits::MessageLimits;
use std::net::SocketAddr;
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

/// Default HTTP/2 `SETTINGS_HEADER_TABLE_SIZE`: 4096 octets.
///
/// HPACK dynamic table. Distinct from [`DEFAULT_MAX_HEADER_LIST_SIZE`]
/// (`SETTINGS_MAX_HEADER_LIST_SIZE`, uncompressed header-block cap).
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;

/// Default HTTP/2 small-DATA framing budget: 25,600 bytes of overhead.
///
/// Tiny DATA frames (payload under 256 bytes) consume this budget. h2 Auto
/// (half the connection window) is not used: the 16 MiB default window would
/// otherwise raise this to 8 MiB. Distinct from [`DEFAULT_WINDOW_SIZE`].
pub const DEFAULT_DATA_FRAME_BUDGET: usize = 25_600;

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

/// Default outbound gzip deflate level: 1 (`flate2` fast).
///
/// At gRPC message sizes the extra CPU of higher levels often costs more
/// latency than the saved bytes buy back. 0 stores; 9 is best.
pub const DEFAULT_GZIP_COMPRESSION_LEVEL: u32 = 1;

/// After [`ServerConfig::max_connection_age`] or idle fires, how long to wait
/// for in-flight RPCs before dropping the socket. Default 10 s.
pub const DEFAULT_MAX_CONNECTION_AGE_GRACE: Duration = Duration::from_secs(10);

/// HTTP/2 rapid-reset cap: remotely-reset streams waiting in the accept queue.
///
/// h2's default, set explicitly. A peer that opens streams and immediately
/// `RST_STREAM`s them sits in that queue until accepted; exceeding this is
/// `ENHANCE_YOUR_CALM` and the connection is dropped.
pub const DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS: usize = 20;

/// HTTP/2 protocol-error RST cap: locally-reset streams after an invalid frame.
///
/// h2's default, set explicitly. A peer that forces protocol-error RSTs
/// (invalid frames) increments this count; exceeding it is
/// `ENHANCE_YOUR_CALM` and the connection is dropped. Distinct from
/// [`DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS`] (rapid reset, remote RSTs).
pub const DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS: usize = 1024;

/// HTTP/2 locally-reset stream-ID memory: how many reset IDs this endpoint
/// remembers so late frames are ignored (RFC 9113).
///
/// h2's default, set explicitly. When the cap is reached, the oldest ID
/// is purged from memory, not `ENHANCE_YOUR_CALM`. Frames on a purged ID
/// are a connection `PROTOCOL_ERROR`. Distinct from
/// [`DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS`] (rapid reset, remote RSTs)
/// and [`DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS`] (protocol-error RSTs we
/// send). Zero is allowed.
pub const DEFAULT_MAX_CONCURRENT_RESET_STREAMS: usize = 50;

/// HTTP/2 locally-reset stream-ID memory duration: how long reset IDs
/// this endpoint remembers so late frames are ignored (RFC 9113).
///
/// h2's default, set explicitly. After this duration the ID is forgotten,
/// not `ENHANCE_YOUR_CALM`. Frames on a forgotten ID are a connection
/// `PROTOCOL_ERROR`. Distinct from [`DEFAULT_MAX_CONCURRENT_RESET_STREAMS`]
/// (how many IDs). Zero is allowed.
pub const DEFAULT_RESET_STREAM_DURATION: Duration = Duration::from_secs(1);

/// The per-stream settings the wire layer needs: message caps plus how much
/// the connection will buffer before a write has to wait for flow control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wire {
    pub(crate) limits: MessageLimits,
    pub(crate) send_buffer: usize,
    /// Inflate inbound gzip. Default on; [`ServerConfig::accept_compressed`] /
    /// [`ChannelConfig::accept_compressed`]`(false)` opts out.
    pub(crate) accept_gzip: bool,
    /// Deflate effort for outbound gzip. Default 1.
    pub(crate) gzip_level: u32,
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
    header_table_size: u32,
    data_frame_budget: usize,
    max_pending_accept_reset_streams: usize,
    max_local_error_reset_streams: usize,
    max_concurrent_reset_streams: usize,
    reset_stream_duration: Duration,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
    tcp_keepalive: Option<Duration>,
    tcp_keepalive_interval: Option<Duration>,
    handshake_timeout: Duration,
    max_connection_age: Option<Duration>,
    max_connection_idle: Option<Duration>,
    max_connection_age_grace: Duration,
    timeout: Option<Duration>,
    max_concurrent_connections: Option<usize>,
    max_concurrent_rpcs: Option<usize>,
    send_compressed: bool,
    accept_compressed: bool,
    gzip_compression_level: u32,
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
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            data_frame_budget: DEFAULT_DATA_FRAME_BUDGET,
            max_pending_accept_reset_streams: DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS,
            max_local_error_reset_streams: DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS,
            max_concurrent_reset_streams: DEFAULT_MAX_CONCURRENT_RESET_STREAMS,
            reset_stream_duration: DEFAULT_RESET_STREAM_DURATION,
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
            tcp_keepalive: None,
            tcp_keepalive_interval: None,
            handshake_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_connection_age: None,
            max_connection_idle: None,
            max_connection_age_grace: DEFAULT_MAX_CONNECTION_AGE_GRACE,
            timeout: None,
            max_concurrent_connections: None,
            max_concurrent_rpcs: None,
            send_compressed: false,
            accept_compressed: true,
            gzip_compression_level: DEFAULT_GZIP_COMPRESSION_LEVEL,
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

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic table). Default 4096.
    /// Applies to every call shape.
    /// A well-behaved client still completes every call shape at this table
    /// size, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`]. Distinct from
    /// [`Self::max_header_list_size`], which caps uncompressed header-block
    /// bytes (`SETTINGS_MAX_HEADER_LIST_SIZE`).
    ///
    /// [`crate::Server::header_table_size`],
    /// [`crate::Router::header_table_size`], and generated
    /// `FooServer::header_table_size` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn header_table_size(mut self, bytes: u32) -> Self {
        self.header_table_size = bytes;
        self
    }

    /// HTTP/2 small-DATA framing budget. Default 25600.
    /// Applies to every call shape.
    /// Caps extra memory from tiny DATA frames (payload under 256 bytes).
    /// Exceeding this is `ENHANCE_YOUR_CALM` (`too_many_data_frames`).
    /// Distinct from [`Self::initial_connection_window_size`], which is
    /// flow-control bytes, and from [`Self::max_frame_size`], which caps one
    /// DATA payload. h2 Auto (half the connection window) is not exposed:
    /// the 16 MiB default window would otherwise raise this to 8 MiB.
    /// Empty DATA frames are a separate h2 cap and do not consume this budget.
    /// A well-behaved client still completes every call shape at this framing
    /// budget, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// [`crate::Server::data_frame_budget`],
    /// [`crate::Router::data_frame_budget`], and generated
    /// `FooServer::data_frame_budget` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn data_frame_budget(mut self, bytes: usize) -> Self {
        self.data_frame_budget = bytes;
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

    /// Cap locally-reset HTTP/2 streams caused by a peer protocol error.
    /// Default 1024 ([`DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS`]). Exceeding
    /// this is `ENHANCE_YOUR_CALM` and the connection is dropped.
    /// Distinct from [`Self::max_pending_accept_reset_streams`]: that caps
    /// remotely-reset streams waiting in the accept queue (rapid reset).
    /// This caps RSTs we send after an invalid frame.
    /// h2's `None` disable is not exposed. Applies to every call shape.
    /// A well-behaved client never triggers one; every call shape still
    /// completes, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// [`crate::Server::max_local_error_reset_streams`],
    /// [`crate::Router::max_local_error_reset_streams`], and generated
    /// `FooServer::max_local_error_reset_streams` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, n: usize) -> Self {
        self.max_local_error_reset_streams = n;
        self
    }

    /// Cap remembered locally-reset HTTP/2 stream IDs.
    /// Default 50 ([`DEFAULT_MAX_CONCURRENT_RESET_STREAMS`]).
    /// Applies to every call shape.
    /// After this endpoint sends `RST_STREAM`, the stream ID is remembered
    /// so late frames are ignored (RFC 9113). When the cap is reached, the
    /// oldest ID is purged from memory, not `ENHANCE_YOUR_CALM`.
    /// Frames on a purged ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`Self::max_pending_accept_reset_streams`] (rapid-reset
    /// GOAWAY) and [`Self::max_local_error_reset_streams`] (protocol-error RST
    /// GOAWAY). This memory includes CANCEL after a drop, not only invalid
    /// frames. Zero is allowed (every local reset is immediately forgotten).
    /// A well-behaved client still completes every call shape at this memory cap,
    /// including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// [`crate::Server::max_concurrent_reset_streams`],
    /// [`crate::Router::max_concurrent_reset_streams`], and generated
    /// `FooServer::max_concurrent_reset_streams` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn max_concurrent_reset_streams(mut self, n: usize) -> Self {
        self.max_concurrent_reset_streams = n;
        self
    }

    /// How long locally-reset HTTP/2 stream IDs are remembered.
    /// Default 1 s ([`DEFAULT_RESET_STREAM_DURATION`]).
    /// Applies to every call shape.
    /// After this duration the ID is forgotten, not `ENHANCE_YOUR_CALM`.
    /// Frames on a forgotten ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`Self::max_concurrent_reset_streams`], which is how many
    /// IDs are remembered (count). This is how long (time). Zero is allowed
    /// (every local reset is immediately forgotten).
    /// A well-behaved client still completes every call shape at this reset duration,
    /// including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`].
    ///
    /// [`crate::Server::reset_stream_duration`],
    /// [`crate::Router::reset_stream_duration`], and generated
    /// `FooServer::reset_stream_duration` set this without building a
    /// [`ServerConfig`].
    #[must_use]
    pub fn reset_stream_duration(mut self, dur: Duration) -> Self {
        self.reset_stream_duration = dur;
        self
    }

    /// Send an HTTP/2 PING every `interval` so a dead peer is noticed before
    /// the next RPC. Disabled by default.
    ///
    /// This is not TCP keepalive. PINGs run on Unix sockets and TLS
    /// (including mTLS); they do not reset [`Self::max_connection_idle`]
    /// (idle is outstanding RPCs, not bytes on the wire) and they do not
    /// postpone [`Self::max_connection_age`] (age is wall-clock from
    /// accept). For `SO_KEEPALIVE`
    /// on TCP sockets, see [`Self::tcp_keepalive`]. Applies to every call
    /// shape.
    /// There is no grpc-go `EnforcementPolicy` / `MinTime` setter: inbound
    /// client PINGs are not GOAWAY'd. Distinct from [`Self::data_frame_budget`],
    /// which is `ENHANCE_YOUR_CALM` (`too_many_data_frames`) for tiny DATA,
    /// not PING rate (`too_many_pings`). Distinct from
    /// [`ChannelConfig::keep_alive_interval`]: that sends client PINGs (and
    /// already Distincts tonic `http2_keep_alive_while_idle` / grpc-go
    /// `PermitWithoutStream`).
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
    /// streams are not. Probe interval is [`Self::tcp_keepalive_interval`]
    /// (`TCP_KEEPINTVL`); this idle time does not set it. Probe retry count
    /// stays at the kernel default.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// `TCP_NODELAY` is always on for TCP connect and accept (Nagle off).
    /// There is no `tcp_nodelay(bool)` setter. Distinct from tonic, which
    /// defaults Nagle off but lets you turn it back on. Unix domain sockets
    /// and [`crate::Channel::from_io`] skip TCP socket tuning entirely.
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.tcp_keepalive = Some(time.max(Duration::from_millis(1)));
        self
    }

    /// TCP keepalive probe interval (`TCP_KEEPINTVL`) after idle
    /// [`Self::tcp_keepalive`].
    ///
    /// Disabled by default (kernel default). Values below 1 ms are raised to
    /// 1 ms. Only applied when [`Self::tcp_keepalive`] is also set; this does
    /// not turn `SO_KEEPALIVE` on by itself. Probe retry count stays at the
    /// kernel default. Only TCP sockets are affected; Unix domain sockets and
    /// [`crate::Channel::from_io`] streams are not.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// Distinct from [`Self::tcp_keepalive`], which is idle time before the
    /// first probe (`TCP_KEEPIDLE`).
    /// Applies to every call shape, including over TLS and mTLS.
    #[must_use]
    pub fn tcp_keepalive_interval(mut self, interval: Duration) -> Self {
        self.tcp_keepalive_interval = Some(interval.max(Duration::from_millis(1)));
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

    /// Deflate effort for outbound gzip. Default 1 (`flate2` fast).
    /// Applies to every call shape.
    ///
    /// 0 stores; 9 is best. Values above 9 are clamped to 9. Unused when
    /// outbound compression is off.
    /// Distinct from [`Self::send_compressed`], which is on or off.
    ///
    /// [`crate::Server::gzip_compression_level`], [`crate::Router::gzip_compression_level`],
    /// and generated `FooServer::gzip_compression_level` set this without
    /// building a [`ServerConfig`].
    #[must_use]
    pub fn gzip_compression_level(mut self, level: u32) -> Self {
        self.gzip_compression_level = level.min(9);
        self
    }

    /// Inflate inbound gzip. Default `true`. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`crate::Server::serve_connection`].
    ///
    /// Passing `false` refuses `grpc-encoding: gzip` as
    /// [`crate::Code::Unimplemented`] before a handler runs, advertises
    /// `grpc-accept-encoding: identity` only, and does not inflate a
    /// Compressed-Flag. Distinct from [`Self::send_compressed`], which is
    /// outbound. Distinct from tonic's `accept_compressed`, which starts
    /// opt-in; this kernel starts on so interop gzip keeps working.
    ///
    /// [`crate::Server::accept_compressed`], [`crate::Router::accept_compressed`],
    /// and generated `FooServer::accept_compressed` set this without building
    /// a [`ServerConfig`].
    #[must_use]
    pub fn accept_compressed(mut self, accept: bool) -> Self {
        self.accept_compressed = accept;
        self
    }

    /// Configured message caps. Applies to every call shape.
    /// Server interceptors read this overlay on [`crate::Rpc::limits`] / [`crate::Request::limits`].
    /// Distinct from [`Self::message_limits`], which sets them.
    #[must_use]
    pub fn limits(self) -> MessageLimits {
        self.limits
    }

    /// Configured per-connection send buffer. Applies to every call shape.
    /// Server interceptors read this overlay on [`crate::Rpc::send_buffer_size`] / [`crate::Request::send_buffer_size`].
    /// Distinct from [`Self::max_send_buffer_size`], which sets it.
    #[must_use]
    pub fn send_buffer_size(self) -> usize {
        self.max_send_buffer_size
    }

    /// Configured per-RPC timeout, if any. See [`Self::timeout`].
    /// Applies to every call shape.
    /// Interceptors and handlers read this overlay on [`crate::Rpc::rpc_timeout`]
    /// / [`crate::Request::rpc_timeout`].
    /// Distinct from [`Self::timeout`], which sets it.
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
    /// Distinct from [`Self::max_concurrent_rpcs`], which sets it.
    #[must_use]
    pub fn concurrent_rpc_limit(self) -> Option<usize> {
        self.max_concurrent_rpcs
    }

    /// Whether responses are gzipped when the client accepts gzip.
    /// See [`Self::send_compressed`]. Applies to every call shape.
    /// Distinct from [`Self::send_compressed`], which sets it.
    #[must_use]
    pub fn compresses_outbound(self) -> bool {
        self.send_compressed
    }

    /// Configured outbound gzip deflate level. See [`Self::gzip_compression_level`].
    /// Applies to every call shape.
    /// Distinct from [`Self::gzip_compression_level`], which sets it.
    #[must_use]
    pub fn gzip_level(self) -> u32 {
        self.gzip_compression_level
    }

    /// Whether inbound gzip is inflated. Default `true`.
    /// See [`Self::accept_compressed`]. Applies to every call shape.
    /// Distinct from [`Self::accept_compressed`], which sets it.
    /// Distinct from [`crate::Rpc::accepts_gzip`], which is the peer's
    /// `grpc-accept-encoding`.
    #[must_use]
    pub fn accepts_compressed(self) -> bool {
        self.accept_compressed
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

    /// Configured TCP keepalive probe interval, if any. See
    /// [`Self::tcp_keepalive_interval`]. Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive_probe_interval(self) -> Option<Duration> {
        self.tcp_keepalive_interval
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

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE`. See [`Self::header_table_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn header_table(self) -> u32 {
        self.header_table_size
    }

    /// HTTP/2 small-DATA framing budget. See [`Self::data_frame_budget`].
    /// Applies to every call shape.
    #[must_use]
    pub fn data_budget(self) -> usize {
        self.data_frame_budget
    }

    /// Remotely-reset HTTP/2 streams waiting in the accept queue.
    /// See [`Self::max_pending_accept_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn pending_accept_reset_streams(self) -> usize {
        self.max_pending_accept_reset_streams
    }

    /// Locally-reset HTTP/2 streams caused by a peer protocol error.
    /// See [`Self::max_local_error_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn local_error_reset_streams(self) -> usize {
        self.max_local_error_reset_streams
    }

    /// Remembered locally-reset HTTP/2 stream IDs.
    /// See [`Self::max_concurrent_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn concurrent_reset_streams(self) -> usize {
        self.max_concurrent_reset_streams
    }

    /// Time locally-reset HTTP/2 stream IDs stay in memory.
    /// See [`Self::reset_stream_duration`]. Applies to every call shape.
    #[must_use]
    pub fn reset_stream_ttl(self) -> Duration {
        self.reset_stream_duration
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
            accept_gzip: self.accept_compressed,
            gzip_level: self.gzip_compression_level,
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
            .header_table_size(self.header_table_size)
            .data_frame_budget(self.data_frame_budget)
            .max_pending_accept_reset_streams(self.max_pending_accept_reset_streams)
            .max_local_error_reset_streams(Some(self.max_local_error_reset_streams))
            .max_concurrent_reset_streams(self.max_concurrent_reset_streams)
            .reset_stream_duration(self.reset_stream_duration);
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
///     .max_connection_idle(Duration::from_secs(5 * 60))
///     .max_connection_age(Duration::from_secs(30 * 60));
/// assert_eq!(config.connection_count(), 4);
/// assert_eq!(config.dial_timeout(), Duration::from_secs(5));
/// assert_eq!(config.connection_idle(), Some(Duration::from_secs(5 * 60)));
/// assert_eq!(config.connection_age(), Some(Duration::from_secs(30 * 60)));
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
    header_table_size: u32,
    data_frame_budget: usize,
    max_pending_accept_reset_streams: usize,
    max_local_error_reset_streams: usize,
    max_concurrent_reset_streams: usize,
    reset_stream_duration: Duration,
    stream_buffer: usize,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
    tcp_keepalive: Option<Duration>,
    tcp_keepalive_interval: Option<Duration>,
    local_address: Option<SocketAddr>,
    connect_timeout: Duration,
    max_connection_idle: Option<Duration>,
    max_connection_age: Option<Duration>,
    max_connection_age_grace: Duration,
    send_compressed: bool,
    accept_compressed: bool,
    gzip_compression_level: u32,
    timeout: Option<Duration>,
    wait_for_ready: bool,
    max_concurrent_rpcs: Option<usize>,
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
            header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            data_frame_budget: DEFAULT_DATA_FRAME_BUDGET,
            max_pending_accept_reset_streams: DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS,
            max_local_error_reset_streams: DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS,
            max_concurrent_reset_streams: DEFAULT_MAX_CONCURRENT_RESET_STREAMS,
            reset_stream_duration: DEFAULT_RESET_STREAM_DURATION,
            stream_buffer: DEFAULT_STREAM_BUFFER,
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
            tcp_keepalive: None,
            tcp_keepalive_interval: None,
            local_address: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_connection_idle: None,
            max_connection_age: None,
            max_connection_age_grace: DEFAULT_MAX_CONNECTION_AGE_GRACE,
            send_compressed: false,
            accept_compressed: true,
            gzip_compression_level: DEFAULT_GZIP_COMPRESSION_LEVEL,
            timeout: None,
            wait_for_ready: false,
            max_concurrent_rpcs: None,
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
    /// Distinct from [`Self::max_concurrent_rpcs`], which refuses extras
    /// before the stream opens.
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
    /// [`crate::Channel::max_send_buffer_size`] sets the write-time DATA threshold on a live clone without building a [`ChannelConfig`].
    /// There is no tonic `Endpoint::buffer_size`: that is tower `Buffer` request
    /// slots (default 1024), not these bytes. This kernel is not a tower stack;
    /// clones share the pool without an mpsc of RPCs. Distinct from
    /// [`Self::stream_buffer`]: that is client-streaming/bidi message queue
    /// depth, not this send buffer. Distinct from grpc-go `ReadBufferSize` /
    /// `WriteBufferSize`, which are socket byte buffers (default 32 KiB), not
    /// this HTTP/2 send buffer.
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

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic table). Default 4096.
    /// Applies to every call shape.
    /// Applied at handshake, not as a live overlay.
    /// Distinct from [`ServerConfig::header_table_size`], which still serves
    /// when the server advertises a smaller table. Distinct from
    /// [`Self::max_header_list_size`], which caps uncompressed header-block
    /// bytes (`SETTINGS_MAX_HEADER_LIST_SIZE`). A well-behaved server still
    /// completes every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn header_table_size(mut self, bytes: u32) -> Self {
        self.header_table_size = bytes;
        self
    }

    /// HTTP/2 small-DATA framing budget. Default 25600.
    /// Applies to every call shape.
    /// Applied at handshake, not as a live overlay.
    /// Caps extra memory from tiny DATA frames (payload under 256 bytes).
    /// Exceeding this is `ENHANCE_YOUR_CALM` (`too_many_data_frames`).
    /// Distinct from [`ServerConfig::data_frame_budget`], which still serves
    /// when the server caps small-DATA framing. Distinct from
    /// [`Self::initial_connection_window_size`], which is flow-control bytes,
    /// and from [`Self::max_frame_size`], which caps one DATA payload.
    /// h2 Auto (half the connection window) is not exposed.
    /// Empty DATA frames are a separate h2 cap and do not consume this budget.
    /// A well-behaved server still completes every call shape at this framing
    /// budget, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    #[must_use]
    pub fn data_frame_budget(mut self, bytes: usize) -> Self {
        self.data_frame_budget = bytes;
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

    /// Cap locally-reset HTTP/2 streams caused by a peer protocol error.
    /// Default 1024 ([`DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS`]). Exceeding
    /// this is `ENHANCE_YOUR_CALM` and the connection is dropped.
    /// Applied at handshake, not as a live overlay.
    /// Distinct from [`ServerConfig::max_local_error_reset_streams`], which
    /// still serves when the server caps protocol-error RSTs. Distinct from
    /// [`Self::max_pending_accept_reset_streams`] (rapid reset, remote RSTs).
    /// This caps RSTs we send after an invalid frame.
    /// h2's `None` disable is not exposed.
    /// A well-behaved server never forces this; every call shape still
    /// completes, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_local_error_reset_streams(mut self, n: usize) -> Self {
        self.max_local_error_reset_streams = n;
        self
    }

    /// Cap remembered locally-reset HTTP/2 stream IDs.
    /// Default 50 ([`DEFAULT_MAX_CONCURRENT_RESET_STREAMS`]).
    /// Applies to every call shape.
    /// Applied at handshake, not as a live overlay.
    /// After this endpoint sends `RST_STREAM`, the stream ID is remembered
    /// so late frames are ignored (RFC 9113). When the cap is reached, the
    /// oldest ID is purged from memory, not `ENHANCE_YOUR_CALM`.
    /// Frames on a purged ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`ServerConfig::max_concurrent_reset_streams`], which still
    /// serves when the server remembers fewer reset stream IDs.
    /// Distinct from [`Self::max_pending_accept_reset_streams`] (rapid-reset
    /// GOAWAY) and [`Self::max_local_error_reset_streams`] (protocol-error RST
    /// GOAWAY). This memory includes CANCEL after a drop, not only invalid
    /// frames. Zero is allowed (every local reset is immediately forgotten).
    /// A well-behaved server still completes every call shape at this memory cap,
    /// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    #[must_use]
    pub fn max_concurrent_reset_streams(mut self, n: usize) -> Self {
        self.max_concurrent_reset_streams = n;
        self
    }

    /// How long locally-reset HTTP/2 stream IDs are remembered.
    /// Default 1 s ([`DEFAULT_RESET_STREAM_DURATION`]).
    /// Applies to every call shape.
    /// Applied at handshake, not as a live overlay.
    /// After this duration the ID is forgotten, not `ENHANCE_YOUR_CALM`.
    /// Frames on a forgotten ID are a connection `PROTOCOL_ERROR`.
    /// Distinct from [`ServerConfig::reset_stream_duration`], which still
    /// serves when the server remembers reset stream IDs for less time.
    /// Distinct from [`Self::max_concurrent_reset_streams`], which is how many
    /// IDs are remembered (count). This is how long (time). Zero is allowed
    /// (every local reset is immediately forgotten).
    /// A well-behaved server still completes every call shape at this reset duration,
    /// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    #[must_use]
    pub fn reset_stream_duration(mut self, dur: Duration) -> Self {
        self.reset_stream_duration = dur;
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
    /// [`Self::max_connection_idle`] and they do not postpone
    /// [`Self::max_connection_age`] (age is wall-clock from the handshake).
    /// There is no `http2_keep_alive_while_idle` setter: once this interval
    /// is set, idle connections PING too. Distinct from tonic's
    /// `Endpoint::http2_keep_alive_while_idle`, which defaults off so a
    /// client interval does not PING an idle socket. Distinct from grpc-go
    /// `PermitWithoutStream`, which is that same idle-PING flag (omitted
    /// because this behavior is already on).
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
    /// streams are not. Probe interval is [`Self::tcp_keepalive_interval`]
    /// (`TCP_KEEPINTVL`); this idle time does not set it. Probe retry count
    /// stays at the kernel default.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// `TCP_NODELAY` is always on for TCP connect and accept (Nagle off).
    /// There is no `tcp_nodelay(bool)` setter. Distinct from tonic, which
    /// defaults Nagle off but lets you turn it back on. Unix domain sockets
    /// and [`crate::Channel::from_io`] skip TCP socket tuning entirely.
    /// Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.tcp_keepalive = Some(time.max(Duration::from_millis(1)));
        self
    }

    /// TCP keepalive probe interval (`TCP_KEEPINTVL`) after idle
    /// [`Self::tcp_keepalive`].
    ///
    /// Disabled by default (kernel default). Values below 1 ms are raised to
    /// 1 ms. Only applied when [`Self::tcp_keepalive`] is also set; this does
    /// not turn `SO_KEEPALIVE` on by itself. Probe retry count stays at the
    /// kernel default. Only TCP sockets are affected; Unix domain sockets and
    /// [`crate::Channel::from_io`] streams are not.
    ///
    /// Distinct from [`Self::keep_alive_interval`], which sends HTTP/2 PINGs.
    /// Distinct from [`Self::tcp_keepalive`], which is idle time before the
    /// first probe (`TCP_KEEPIDLE`).
    /// Applies to every call shape, including over TLS and mTLS.
    #[must_use]
    pub fn tcp_keepalive_interval(mut self, interval: Duration) -> Self {
        self.tcp_keepalive_interval = Some(interval.max(Duration::from_millis(1)));
        self
    }

    /// Bind the TCP client to `addr` before connect.
    ///
    /// Port `0` lets the OS pick an ephemeral source port. Applies to every
    /// TCP call shape, including [`crate::Channel::connect`],
    /// [`crate::Channel::connect_with`], [`crate::Channel::connect_tls`],
    /// [`crate::Channel::connect_tls_with`], and the lazy variants. TLS and
    /// mTLS bind the TCP socket first, then handshake. Unix domain sockets
    /// and [`crate::Channel::from_io`] skip this bind: those streams are not
    /// TCP.
    ///
    /// Distinct from [`crate::Rpc::local_addr`] / [`crate::Request::local_addr`]:
    /// those are the accepted interface after the handshake, not this source
    /// bind. Distinct from tonic's `Endpoint::local_address`, which takes an
    /// `IpAddr` and always binds port 0. There is no live
    /// `Channel::local_address` setter: this overlay is handshake-only.
    #[must_use]
    pub fn local_address(mut self, addr: SocketAddr) -> Self {
        self.local_address = Some(addr);
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

    /// Close a connection this long after it was dialed, even while RPCs are
    /// in flight. Disabled by default. Values below 1 ms are raised to 1 ms.
    ///
    /// The actual lifetime is jittered by ±10% so a process with many
    /// connections does not reconnect in lockstep. Distinct from
    /// [`Self::max_connection_idle`]: a long-running stream is not idle, but
    /// it does not postpone age. In-flight RPCs get
    /// [`Self::max_connection_age_grace`] to finish; new RPCs of every call
    /// shape redial, including over TLS, mTLS, and Unix, except on
    /// [`crate::Channel::from_io`], which cannot redial and fails with
    /// [`crate::Code::Unavailable`].
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.max_connection_age = Some(age.max(Duration::from_millis(1)));
        self
    }

    /// After [`Self::max_connection_age`] fires, wait this long for in-flight
    /// RPCs before dropping the socket. Default 10 s. Values below 1 ms are
    /// raised to 1 ms. Applies to every call shape, including over TLS, mTLS,
    /// Unix, and [`crate::Channel::from_io`].
    ///
    /// New RPCs already redial (or fail on `from_io`) when age fires; this
    /// bound is only the in-flight drain.
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.max_connection_age_grace = grace.max(Duration::from_millis(1));
        self
    }

    /// gzip request payloads (and [`crate::StreamSender::send`] on a stream).
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    ///
    /// Off by default. The kernel advertises `identity,gzip` unless
    /// [`Self::accept_compressed`]`(false)` opted out, so a server that
    /// implements gzip will accept these frames. Per-RPC
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

    /// Deflate effort for outbound gzip. Default 1 (`flate2` fast).
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`].
    ///
    /// 0 stores; 9 is best. Values above 9 are clamped to 9. Unused when
    /// outbound compression is off.
    /// Distinct from [`Self::send_compressed`], which is on or off.
    /// Overlay: [`crate::Channel::gzip_compression_level`] and generated
    /// `FooClient::gzip_compression_level` set this without building a
    /// [`ChannelConfig`].
    #[must_use]
    pub fn gzip_compression_level(mut self, level: u32) -> Self {
        self.gzip_compression_level = level.min(9);
        self
    }

    /// Inflate inbound gzip. Default `true`. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    ///
    /// Passing `false` omits gzip from `grpc-accept-encoding` and refuses a
    /// `grpc-encoding: gzip` reply as [`crate::Code::Unimplemented`] without
    /// inflating. Distinct from [`Self::send_compressed`], which is outbound.
    /// Distinct from tonic's `accept_compressed`, which starts opt-in; this
    /// kernel starts on so interop gzip keeps working.
    ///
    /// [`crate::Channel::accept_compressed`] and generated
    /// `FooClient::accept_compressed` set this without building a
    /// [`ChannelConfig`].
    #[must_use]
    pub fn accept_compressed(mut self, accept: bool) -> Self {
        self.accept_compressed = accept;
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
    /// Distinct from [`crate::Channel::connected`]: that is a live snapshot; this overlay still queues when a slot is empty.
    #[must_use]
    pub fn wait_for_ready(mut self, wait: bool) -> Self {
        self.wait_for_ready = wait;
        self
    }

    /// Cap how many RPCs this channel will run at once, across every
    /// pooled connection. Applies to every call shape, including over TLS,
    /// mTLS, Unix, and [`crate::Channel::from_io`].
    ///
    /// Further RPCs are refused with [`crate::Code::ResourceExhausted`]
    /// before the stream opens. Distinct from
    /// [`Self::max_concurrent_streams`] (per HTTP/2 connection SETTINGS;
    /// extras wait) and from [`ServerConfig::max_concurrent_rpcs`] (the
    /// server refuses inbound). Disabled by default.
    /// There is no tonic `Endpoint::rate_limit` setter: that is tower
    /// `RateLimitLayer` (at most N RPCs per duration). This kernel is not a
    /// tower stack. This cap is in-flight slots, not a token bucket.
    /// Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
    ///
    /// [`crate::Channel::max_concurrent_rpcs`] and generated
    /// `FooClient::max_concurrent_rpcs` set this without building a
    /// [`ChannelConfig`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.max_concurrent_rpcs = Some(n.max(1));
        self
    }

    /// Configured message caps. Applies to every call shape.
    /// [`crate::Channel::limits`] reads this overlay on a live clone without building a [`ChannelConfig`].
    /// Distinct from [`Self::message_limits`], which sets them.
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
    /// Distinct from [`Self::stream_buffer`], which sets it.
    #[must_use]
    pub fn stream_buffer_size(self) -> usize {
        self.stream_buffer
    }

    /// Configured per-connection send buffer. Applies to every call shape.
    /// Distinct from [`Self::max_send_buffer_size`], which sets it.
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

    /// Configured TCP keepalive probe interval, if any. See
    /// [`Self::tcp_keepalive_interval`]. Applies to every call shape.
    #[must_use]
    pub fn tcp_keepalive_probe_interval(self) -> Option<Duration> {
        self.tcp_keepalive_interval
    }

    /// Configured TCP source bind, if any. See [`Self::local_address`].
    /// Applies to every TCP call shape; Unix and [`crate::Channel::from_io`]
    /// skip it.
    /// Distinct from [`Self::local_address`], which sets it.
    #[must_use]
    pub fn bound_local_address(self) -> Option<SocketAddr> {
        self.local_address
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

    /// HTTP/2 `SETTINGS_HEADER_TABLE_SIZE`. See [`Self::header_table_size`].
    /// Applies to every call shape.
    #[must_use]
    pub fn header_table(self) -> u32 {
        self.header_table_size
    }

    /// HTTP/2 small-DATA framing budget. See [`Self::data_frame_budget`].
    /// Applies to every call shape.
    #[must_use]
    pub fn data_budget(self) -> usize {
        self.data_frame_budget
    }

    /// Remotely-reset HTTP/2 streams waiting in the accept queue.
    /// See [`Self::max_pending_accept_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn pending_accept_reset_streams(self) -> usize {
        self.max_pending_accept_reset_streams
    }

    /// Locally-reset HTTP/2 streams caused by a peer protocol error.
    /// See [`Self::max_local_error_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn local_error_reset_streams(self) -> usize {
        self.max_local_error_reset_streams
    }

    /// Remembered locally-reset HTTP/2 stream IDs.
    /// See [`Self::max_concurrent_reset_streams`]. Applies to every call shape.
    #[must_use]
    pub fn concurrent_reset_streams(self) -> usize {
        self.max_concurrent_reset_streams
    }

    /// Time locally-reset HTTP/2 stream IDs stay in memory.
    /// See [`Self::reset_stream_duration`]. Applies to every call shape.
    #[must_use]
    pub fn reset_stream_ttl(self) -> Duration {
        self.reset_stream_duration
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

    /// Configured max connection age, if any. See [`Self::max_connection_age`].
    /// Applies to every call shape.
    #[must_use]
    pub fn connection_age(self) -> Option<Duration> {
        self.max_connection_age
    }

    /// Grace after client age. See [`Self::max_connection_age_grace`].
    /// Applies to every call shape.
    #[must_use]
    pub fn age_grace(self) -> Duration {
        self.max_connection_age_grace
    }

    /// Whether request payloads are gzipped. See [`Self::send_compressed`].
    /// Applies to every call shape.
    /// Distinct from [`Self::send_compressed`], which sets it.
    #[must_use]
    pub fn compresses_outbound(self) -> bool {
        self.send_compressed
    }

    /// Configured outbound gzip deflate level. See [`Self::gzip_compression_level`].
    /// Applies to every call shape.
    /// Distinct from [`Self::gzip_compression_level`], which sets it.
    #[must_use]
    pub fn gzip_level(self) -> u32 {
        self.gzip_compression_level
    }

    /// Whether inbound gzip is inflated. Default `true`.
    /// See [`Self::accept_compressed`]. Applies to every call shape.
    /// Distinct from [`Self::accept_compressed`], which sets it.
    /// Distinct from [`crate::Rpc::accepts_gzip`], which is the peer's
    /// `grpc-accept-encoding`.
    #[must_use]
    pub fn accepts_compressed(self) -> bool {
        self.accept_compressed
    }

    /// Configured default per-RPC deadline, if any. See [`Self::timeout`].
    /// Applies to every call shape.
    /// Distinct from [`Self::timeout`], which sets it.
    #[must_use]
    pub fn rpc_timeout(self) -> Option<Duration> {
        self.timeout
    }

    /// Configured default wait-for-ready. See [`Self::wait_for_ready`].
    /// Applies to every call shape.
    /// Distinct from [`Self::wait_for_ready`], which sets it.
    #[must_use]
    pub fn waits_for_ready(self) -> bool {
        self.wait_for_ready
    }

    /// Configured channel-wide RPC cap, if any. See [`Self::max_concurrent_rpcs`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_concurrent_rpcs`], which sets it.
    #[must_use]
    pub fn concurrent_rpc_limit(self) -> Option<usize> {
        self.max_concurrent_rpcs
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
            accept_gzip: self.accept_compressed,
            gzip_level: self.gzip_compression_level,
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
            .header_table_size(self.header_table_size)
            .data_frame_budget(self.data_frame_budget)
            .max_pending_accept_reset_streams(self.max_pending_accept_reset_streams)
            .max_local_error_reset_streams(Some(self.max_local_error_reset_streams))
            .max_concurrent_reset_streams(self.max_concurrent_reset_streams)
            .reset_stream_duration(self.reset_stream_duration)
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
        assert_eq!(config.tcp_keepalive_probe_interval(), None);
        assert_eq!(ChannelConfig::new().tcp_keepalive_probe_interval(), None);
        assert_eq!(ChannelConfig::new().bound_local_address(), None);
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
        assert_eq!(config.header_table(), super::DEFAULT_HEADER_TABLE_SIZE);
        assert_eq!(
            ChannelConfig::new().header_table(),
            super::DEFAULT_HEADER_TABLE_SIZE
        );
        assert_eq!(ServerConfig::new().header_table_size(0).header_table(), 0);
        assert_eq!(ChannelConfig::new().header_table_size(0).header_table(), 0);
        assert_eq!(
            ServerConfig::new().header_table_size(8192).header_table(),
            8192
        );
        assert_eq!(
            ChannelConfig::new().header_table_size(8192).header_table(),
            8192
        );
        assert_eq!(config.data_budget(), super::DEFAULT_DATA_FRAME_BUDGET);
        assert_eq!(
            ChannelConfig::new().data_budget(),
            super::DEFAULT_DATA_FRAME_BUDGET
        );
        assert_eq!(
            ServerConfig::new().data_frame_budget(512).data_budget(),
            512
        );
        assert_eq!(
            ChannelConfig::new().data_frame_budget(512).data_budget(),
            512
        );
        assert_eq!(ServerConfig::new().data_frame_budget(0).data_budget(), 0);
        assert_eq!(
            config.pending_accept_reset_streams(),
            super::DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS
        );
        assert_eq!(
            config.local_error_reset_streams(),
            super::DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS
        );
        assert_eq!(
            ChannelConfig::new().pending_accept_reset_streams(),
            super::DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS
        );
        assert_eq!(
            ChannelConfig::new().local_error_reset_streams(),
            super::DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS
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
        assert_eq!(
            ServerConfig::new()
                .max_local_error_reset_streams(5)
                .local_error_reset_streams(),
            5
        );
        assert_eq!(
            ChannelConfig::new()
                .max_local_error_reset_streams(5)
                .local_error_reset_streams(),
            5
        );
        assert_eq!(
            ServerConfig::new()
                .max_local_error_reset_streams(0)
                .local_error_reset_streams(),
            0
        );
        assert_eq!(
            config.concurrent_reset_streams(),
            super::DEFAULT_MAX_CONCURRENT_RESET_STREAMS
        );
        assert_eq!(
            ChannelConfig::new().concurrent_reset_streams(),
            super::DEFAULT_MAX_CONCURRENT_RESET_STREAMS
        );
        assert_eq!(
            ServerConfig::new()
                .max_concurrent_reset_streams(1)
                .concurrent_reset_streams(),
            1
        );
        assert_eq!(
            ChannelConfig::new()
                .max_concurrent_reset_streams(1)
                .concurrent_reset_streams(),
            1
        );
        assert_eq!(
            ServerConfig::new()
                .max_concurrent_reset_streams(0)
                .concurrent_reset_streams(),
            0
        );
        assert_eq!(
            ChannelConfig::new()
                .max_concurrent_reset_streams(0)
                .concurrent_reset_streams(),
            0
        );
        assert_eq!(
            config.reset_stream_ttl(),
            super::DEFAULT_RESET_STREAM_DURATION
        );
        assert_eq!(
            ChannelConfig::new().reset_stream_ttl(),
            super::DEFAULT_RESET_STREAM_DURATION
        );
        assert_eq!(
            ServerConfig::new()
                .reset_stream_duration(Duration::from_secs(10))
                .reset_stream_ttl(),
            Duration::from_secs(10)
        );
        assert_eq!(
            ChannelConfig::new()
                .reset_stream_duration(Duration::from_secs(10))
                .reset_stream_ttl(),
            Duration::from_secs(10)
        );
        assert_eq!(
            ServerConfig::new()
                .reset_stream_duration(Duration::ZERO)
                .reset_stream_ttl(),
            Duration::ZERO
        );
        assert_eq!(
            ChannelConfig::new()
                .reset_stream_duration(Duration::ZERO)
                .reset_stream_ttl(),
            Duration::ZERO
        );
        assert_eq!(config.handshake_wait(), super::DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(config.connection_age(), None);
        assert_eq!(config.connection_idle(), None);
        assert_eq!(config.age_grace(), super::DEFAULT_MAX_CONNECTION_AGE_GRACE);
        assert!(!config.compresses_outbound());
        assert!(config.accepts_compressed());
        assert_eq!(ChannelConfig::new().connection_idle(), None);
        assert_eq!(ChannelConfig::new().connection_age(), None);
        assert_eq!(
            ChannelConfig::new().age_grace(),
            super::DEFAULT_MAX_CONNECTION_AGE_GRACE
        );
        assert_eq!(ChannelConfig::new().rpc_timeout(), None);
        assert_eq!(ChannelConfig::new().concurrent_rpc_limit(), None);
        assert!(!ChannelConfig::new().waits_for_ready());
        assert!(ChannelConfig::new().wait_for_ready(true).waits_for_ready());
        assert!(ChannelConfig::new().accepts_compressed());
        assert!(!ChannelConfig::new()
            .accept_compressed(false)
            .accepts_compressed());
        assert!(!ServerConfig::new()
            .accept_compressed(false)
            .accepts_compressed());
        assert_eq!(config.gzip_level(), super::DEFAULT_GZIP_COMPRESSION_LEVEL);
        assert_eq!(
            ChannelConfig::new().gzip_level(),
            super::DEFAULT_GZIP_COMPRESSION_LEVEL
        );
        assert_eq!(
            ServerConfig::new().gzip_compression_level(9).gzip_level(),
            9
        );
        assert_eq!(
            ServerConfig::new().gzip_compression_level(0).gzip_level(),
            0
        );
        assert_eq!(
            ServerConfig::new().gzip_compression_level(10).gzip_level(),
            9
        );
        assert_eq!(
            ChannelConfig::new().gzip_compression_level(9).gzip_level(),
            9
        );
        assert_eq!(
            ChannelConfig::new().gzip_compression_level(10).gzip_level(),
            9
        );
        assert_eq!(ServerConfig::new().wire().gzip_level, 1);
        assert_eq!(
            ServerConfig::new()
                .gzip_compression_level(9)
                .wire()
                .gzip_level,
            9
        );
        assert_eq!(
            ChannelConfig::new()
                .gzip_compression_level(0)
                .wire()
                .gzip_level,
            0
        );
    }

    #[test]
    fn channel_rpc_cap_never_zero() {
        assert_eq!(
            ChannelConfig::new()
                .max_concurrent_rpcs(0)
                .concurrent_rpc_limit(),
            Some(1)
        );
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
        assert_eq!(
            ServerConfig::new()
                .tcp_keepalive_interval(Duration::from_millis(0))
                .tcp_keepalive_probe_interval(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            ChannelConfig::new()
                .tcp_keepalive_interval(Duration::from_millis(0))
                .tcp_keepalive_probe_interval(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            ChannelConfig::new()
                .tcp_keepalive_interval(Duration::from_secs(5))
                .tcp_keepalive_period(),
            None
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
        let channel_age = ChannelConfig::new()
            .max_connection_age(Duration::from_millis(0))
            .max_connection_age_grace(Duration::from_millis(0));
        assert_eq!(channel_age.connection_age(), Some(Duration::from_millis(1)));
        assert_eq!(channel_age.age_grace(), Duration::from_millis(1));
    }

    #[test]
    fn http2_knobs_round_trip() {
        let server = ServerConfig::new()
            .initial_stream_window_size(1)
            .initial_connection_window_size(2)
            .max_frame_size(16_384)
            .max_concurrent_streams(8)
            .max_header_list_size(32)
            .header_table_size(2048)
            .data_frame_budget(512)
            .max_pending_accept_reset_streams(3)
            .max_local_error_reset_streams(7)
            .max_concurrent_reset_streams(5)
            .reset_stream_duration(Duration::from_secs(10));
        assert_eq!(server.stream_window(), 1);
        assert_eq!(server.connection_window(), 2);
        assert_eq!(server.frame_size(), 16_384);
        assert_eq!(server.concurrent_streams(), 8);
        assert_eq!(server.header_list_size(), 32);
        assert_eq!(server.header_table(), 2048);
        assert_eq!(server.data_budget(), 512);
        assert_eq!(server.pending_accept_reset_streams(), 3);
        assert_eq!(server.local_error_reset_streams(), 7);
        assert_eq!(server.concurrent_reset_streams(), 5);
        assert_eq!(server.reset_stream_ttl(), Duration::from_secs(10));

        let channel = ChannelConfig::new()
            .initial_stream_window_size(3)
            .initial_connection_window_size(4)
            .max_frame_size(16_384)
            .max_concurrent_streams(9)
            .max_header_list_size(64)
            .header_table_size(1024)
            .data_frame_budget(768)
            .max_pending_accept_reset_streams(11)
            .max_local_error_reset_streams(13)
            .max_concurrent_reset_streams(17)
            .reset_stream_duration(Duration::from_secs(4));
        assert_eq!(channel.stream_window(), 3);
        assert_eq!(channel.connection_window(), 4);
        assert_eq!(channel.frame_size(), 16_384);
        assert_eq!(channel.concurrent_streams(), 9);
        assert_eq!(channel.header_list_size(), 64);
        assert_eq!(channel.header_table(), 1024);
        assert_eq!(channel.data_budget(), 768);
        assert_eq!(channel.pending_accept_reset_streams(), 11);
        assert_eq!(channel.local_error_reset_streams(), 13);
        assert_eq!(channel.concurrent_reset_streams(), 17);
        assert_eq!(channel.reset_stream_ttl(), Duration::from_secs(4));
        assert!(!ChannelConfig::new().compresses_outbound());
        assert!(ChannelConfig::new()
            .send_compressed(true)
            .compresses_outbound());
        assert!(ServerConfig::new()
            .send_compressed(true)
            .compresses_outbound());
        assert!(ChannelConfig::new().wire().accept_gzip);
        assert!(
            !ChannelConfig::new()
                .accept_compressed(false)
                .wire()
                .accept_gzip
        );
        assert!(ServerConfig::new().wire().accept_gzip);
        assert!(
            !ServerConfig::new()
                .accept_compressed(false)
                .wire()
                .accept_gzip
        );
    }

    #[test]
    fn age_jitter_is_plus_or_minus_ten_percent() {
        let age = Duration::from_secs(100);
        assert_eq!(super::jitter_age(age, 0), Duration::from_secs(90));
        assert_eq!(super::jitter_age(age, 200), Duration::from_secs(110));
        assert_ne!(super::jitter_age(age, 1), super::jitter_age(age, 2));
    }

    #[test]
    fn local_address_round_trips() {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 2], 0));
        assert_eq!(
            ChannelConfig::new()
                .local_address(addr)
                .bound_local_address(),
            Some(addr)
        );
    }
}
