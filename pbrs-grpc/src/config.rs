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

/// After [`ServerConfig::max_connection_age`] or idle fires, how long to wait
/// for in-flight RPCs before dropping the socket. Default 10 s.
pub const DEFAULT_MAX_CONNECTION_AGE_GRACE: Duration = Duration::from_secs(10);

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
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
    max_connection_age: Option<Duration>,
    max_connection_idle: Option<Duration>,
    max_connection_age_grace: Duration,
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
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
            max_connection_age: None,
            max_connection_idle: None,
            max_connection_age_grace: DEFAULT_MAX_CONNECTION_AGE_GRACE,
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
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_decoding(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes. Default unlimited.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_encoding(limit);
        self
    }

    /// Replace both message caps at once.
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// HTTP/2 per-stream receive window. Default 16 MiB.
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.initial_stream_window_size = bytes;
        self
    }

    /// HTTP/2 per-connection receive window. Default 16 MiB.
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.initial_connection_window_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Default 1 MiB.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Concurrent RPCs allowed per connection. Default 256.
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Bytes buffered per connection before writes apply backpressure.
    /// Default 1 MiB.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.max_send_buffer_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`, i.e. the metadata cap.
    /// Default 16 KiB.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.max_header_list_size = bytes;
        self
    }

    /// Send an HTTP/2 PING every `interval` so a dead peer is noticed before
    /// the next RPC. Disabled by default.
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = Some(interval);
        self
    }

    /// How long to wait for a PING acknowledgement before dropping the
    /// connection. Default 20 s. Values below 1 ms are raised to 1 ms.
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Send GOAWAY this long after the connection is accepted. Disabled by
    /// default. Values below 1 ms are raised to 1 ms.
    ///
    /// In-flight RPCs get [`Self::max_connection_age_grace`] to finish; a
    /// [`crate::Channel`] on the other end redials the next RPC.
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.max_connection_age = Some(age.max(Duration::from_millis(1)));
        self
    }

    /// Send GOAWAY if no new RPC has arrived for this long. Disabled by
    /// default. Values below 1 ms are raised to 1 ms. Keepalive PINGs do not
    /// count as activity.
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.max_connection_idle = Some(idle.max(Duration::from_millis(1)));
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs before
    /// dropping the socket. Default 10 s. Values below 1 ms are raised to 1 ms.
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.max_connection_age_grace = grace.max(Duration::from_millis(1));
        self
    }

    /// Configured message caps.
    #[must_use]
    pub fn limits(self) -> MessageLimits {
        self.limits
    }

    /// Configured per-connection send buffer.
    #[must_use]
    pub fn send_buffer_size(self) -> usize {
        self.max_send_buffer_size
    }

    pub(crate) fn keepalive(self) -> (Option<Duration>, Duration) {
        (self.keep_alive_interval, self.keep_alive_timeout)
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
            .max_header_list_size(self.max_header_list_size);
        builder
    }
}

/// HTTP/2 and resource settings for a [`Channel`](crate::Channel).
///
/// ```
/// use pbrs_grpc::ChannelConfig;
///
/// let config = ChannelConfig::new().connections(4);
/// assert_eq!(config.connection_count(), 4);
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
    stream_buffer: usize,
    keep_alive_interval: Option<Duration>,
    keep_alive_timeout: Duration,
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
            stream_buffer: DEFAULT_STREAM_BUFFER,
            keep_alive_interval: None,
            keep_alive_timeout: DEFAULT_KEEP_ALIVE_TIMEOUT,
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
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_decoding(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes. Default unlimited.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.limits = self.limits.with_max_encoding(limit);
        self
    }

    /// Replace both message caps at once.
    #[must_use]
    pub fn message_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// HTTP/2 per-stream receive window. Default 16 MiB.
    #[must_use]
    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {
        self.initial_stream_window_size = bytes;
        self
    }

    /// HTTP/2 per-connection receive window. Default 16 MiB.
    #[must_use]
    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {
        self.initial_connection_window_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Default 1 MiB.
    #[must_use]
    pub fn max_frame_size(mut self, bytes: u32) -> Self {
        self.max_frame_size = bytes;
        self
    }

    /// Concurrent RPCs allowed per connection. Default 256.
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.max_concurrent_streams = streams;
        self
    }

    /// Bytes buffered per connection before writes apply backpressure.
    /// Default 1 MiB.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.max_send_buffer_size = bytes;
        self
    }

    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`, i.e. the metadata cap.
    /// Default 16 KiB.
    #[must_use]
    pub fn max_header_list_size(mut self, bytes: u32) -> Self {
        self.max_header_list_size = bytes;
        self
    }

    /// Messages queued between a client-streaming caller and the wire.
    /// Default 16.
    ///
    /// The wire layer sends whatever is queued as one batch, so deeper means
    /// fewer and larger writes at the cost of memory. Received streams are
    /// decoded inline and are not queued, so this does not affect them.
    #[must_use]
    pub fn stream_buffer(mut self, messages: usize) -> Self {
        self.stream_buffer = messages.max(1);
        self
    }

    /// Send an HTTP/2 PING every `interval` so a dead peer is noticed before
    /// the next RPC. Disabled by default. PINGs are sent while idle as well
    /// as while RPCs are in flight.
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = Some(interval);
        self
    }

    /// How long to wait for a PING acknowledgement before dropping the
    /// connection. Default 20 s. Values below 1 ms are raised to 1 ms.
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.keep_alive_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Configured message caps.
    #[must_use]
    pub fn limits(self) -> MessageLimits {
        self.limits
    }

    /// Configured connection count.
    #[must_use]
    pub fn connection_count(self) -> usize {
        self.connections
    }

    /// Configured outbound streaming queue depth.
    #[must_use]
    pub fn stream_buffer_size(self) -> usize {
        self.stream_buffer
    }

    /// Configured per-connection send buffer.
    #[must_use]
    pub fn send_buffer_size(self) -> usize {
        self.max_send_buffer_size
    }

    pub(crate) fn keepalive(self) -> (Option<Duration>, Duration) {
        (self.keep_alive_interval, self.keep_alive_timeout)
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
            .enable_push(false);
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
    fn connection_age_never_zero() {
        let config = ServerConfig::new()
            .max_connection_age(Duration::from_millis(0))
            .max_connection_idle(Duration::from_millis(0))
            .max_connection_age_grace(Duration::from_millis(0));
        let (age, idle, grace) = config.connection_lifetime();
        assert_eq!(age, Some(Duration::from_millis(1)));
        assert_eq!(idle, Some(Duration::from_millis(1)));
        assert_eq!(grace, Duration::from_millis(1));
    }
}
