//! Crate-internal HTTP/2 transport seam (H2-02).
//!
//! These traits define every transport operation the client and server use:
//! connection handshake, stream open/accept, send headers/data/trailers,
//! receive, flow-control release, reset, and keepalive ping. The [`h2`]
//! module provides the current h2-backed implementation; no `h2::` type may
//! appear outside it. A future native engine (H2-04) implements these same
//! traits, so call sites migrate by swapping the backend import.
//!
//! Value types shared by every backend ([`Error`], [`Reason`]) are
//! re-exported here. Backend machinery (streams, connections, builders)
//! lives in [`h2`] and is imported from there.

use self::h2 as backend;

pub(crate) mod h2;

pub(crate) use self::h2::{Error, Reason};

use bytes::Bytes;
use http::{HeaderMap, Request, Response};
use std::future::Future;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

/// Outbound side of one HTTP/2 stream: capacity, DATA, trailers, reset.
pub(crate) trait SendStream {
    /// Currently available send window (stream + connection minima).
    fn capacity(&self) -> usize;
    /// Ask the peer for more send window via WINDOW_UPDATE.
    fn reserve_capacity(&mut self, capacity: usize);
    /// Poll until `capacity` grows or the stream closes.
    fn poll_capacity(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<usize, Error>>>;
    /// Queue DATA; errors when the frame exceeds `capacity`.
    fn send_data(&mut self, data: Bytes, end_of_stream: bool) -> Result<(), Error>;
    /// Send trailers; the stream must not already be closed.
    fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Error>;
    /// Abort the stream with RST_STREAM. Idempotent; never fails.
    fn send_reset(&mut self, reason: Reason);
    /// Poll for a reset sent by the peer.
    fn poll_reset(&mut self, cx: &mut Context<'_>) -> Poll<Result<Reason, Error>>;
}

/// Inbound side of one HTTP/2 stream: DATA, trailers, flow control.
pub(crate) trait RecvStream {
    /// Poll for the next DATA chunk; `None` at end of stream.
    fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, Error>>>;
    /// Poll for trailers once DATA is exhausted.
    fn poll_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<HeaderMap>, Error>>;
    /// Wait for trailers once DATA is exhausted.
    fn trailers(&mut self) -> impl Future<Output = Result<Option<HeaderMap>, Error>>;
    /// True once the peer closed its send side (headers-only or DATA done).
    fn is_end_stream(&self) -> bool;
    /// Handle to release consumed receive window back to the peer.
    fn flow_control(&mut self) -> backend::FlowControl;
}

/// Receive-window handle for one stream.
pub(crate) trait FlowControl {
    /// Grant the peer `sz` more bytes of window. Errors past the target.
    fn release_capacity(&mut self, sz: usize) -> Result<(), Error>;
}

/// Client handle that opens outbound streams on a connection.
pub(crate) trait SendRequest: Clone {
    /// Wait until the connection can open another stream.
    fn ready(self) -> backend::ReadySendRequest;
    /// Open a stream and send request headers.
    fn send_request(
        &mut self,
        request: Request<()>,
        end_of_stream: bool,
    ) -> Result<(backend::ResponseFuture, backend::SendStream), Error>;
    /// Peer's current MAX_CONCURRENT_STREAMS style send-stream budget.
    fn current_max_send_streams(&self) -> usize;
}

/// Server handle that answers one accepted stream.
pub(crate) trait SendResponse {
    /// Send response headers; returns the stream's send half unless `end_of_stream`.
    fn send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<backend::SendStream, Error>;
    /// Abort the stream with RST_STREAM. Idempotent; never fails.
    #[allow(
        dead_code,
        reason = "seam surface: production answers trailers-only, tests RST refused streams"
    )]
    fn send_reset(&mut self, reason: Reason);
    /// Poll for a reset sent by the peer.
    fn poll_reset(&mut self, cx: &mut Context<'_>) -> Poll<Result<Reason, Error>>;
}

/// Client connection driver: polled until the connection closes.
pub(crate) trait ClientConnection: Future<Output = Result<(), Error>> {
    /// Keepalive ping handle, if the connection still accepts pings.
    fn ping_pong(&mut self) -> Option<backend::PingPong>;
}

/// One accepted inbound stream: request headers plus the response handle.
pub(crate) type Accepted = (Request<backend::RecvStream>, backend::SendResponse);

/// Server connection driver: accepts streams until the connection closes.
pub(crate) trait ServerConnection {
    /// Poll for the next inbound stream: headers plus the response handle.
    fn poll_accept(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Accepted, Error>>>;
    /// Accept the next inbound stream, driving the connection meanwhile.
    fn accept(&mut self) -> impl Future<Output = Option<Result<Accepted, Error>>>;
    /// Keepalive ping handle, if the connection still accepts pings.
    fn ping_pong(&mut self) -> Option<backend::PingPong>;
    /// Send GOAWAY; in-flight streams run to completion.
    fn graceful_shutdown(&mut self);
}

/// Builder for client connections: SETTINGS knobs plus the handshake.
pub(crate) trait ClientBuilder {
    /// Builder with the protocol defaults.
    fn new() -> Self;
    /// Per-stream initial receive window (SETTINGS_INITIAL_WINDOW_SIZE).
    fn initial_window_size(&mut self, size: u32) -> &mut Self;
    /// Connection-level initial receive window.
    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self;
    /// Largest DATA frame the peer may send us.
    fn max_frame_size(&mut self, max: u32) -> &mut Self;
    /// Largest header list the peer may send us, uncompressed bytes.
    fn max_header_list_size(&mut self, max: u32) -> &mut Self;
    /// Cap on concurrent streams opened by the peer.
    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self;
    /// Send-stream budget before the peer's SETTINGS arrives.
    fn initial_max_send_streams(&mut self, initial: usize) -> &mut Self;
    /// Cap on reset streams retained for error reporting.
    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self;
    /// How long a reset stream is retained for error reporting.
    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self;
    /// Cap on outbound data buffered per stream.
    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self;
    /// Cap on reset streams queued before accept.
    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self;
    /// Cap on locally-reset streams retained for error reporting.
    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self;
    /// Whether the peer may push (always disabled for gRPC).
    fn enable_push(&mut self, enabled: bool) -> &mut Self;
    /// HPACK dynamic table size we advertise.
    fn header_table_size(&mut self, size: u32) -> &mut Self;
    /// Outbound DATA frames in flight before backpressure.
    fn data_frame_budget(&mut self, budget: usize) -> &mut Self;
    /// Run the client handshake over `io`.
    fn handshake<IO>(
        &self,
        io: IO,
    ) -> impl Future<Output = Result<(backend::SendRequest, backend::ClientConnection<IO>), Error>>
    + use<IO, Self>
    where
        IO: AsyncRead + AsyncWrite + Unpin;
}

/// Builder for server connections: SETTINGS knobs plus the handshake.
pub(crate) trait ServerBuilder {
    /// Builder with the protocol defaults.
    fn new() -> Self;
    /// Per-stream initial receive window (SETTINGS_INITIAL_WINDOW_SIZE).
    fn initial_window_size(&mut self, size: u32) -> &mut Self;
    /// Connection-level initial receive window.
    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self;
    /// Largest DATA frame the peer may send us.
    fn max_frame_size(&mut self, max: u32) -> &mut Self;
    /// Largest header list the peer may send us, uncompressed bytes.
    fn max_header_list_size(&mut self, max: u32) -> &mut Self;
    /// Cap on concurrent streams opened by the peer.
    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self;
    /// Cap on reset streams retained for error reporting.
    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self;
    /// How long a reset stream is retained for error reporting.
    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self;
    /// Cap on outbound data buffered per stream.
    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self;
    /// Cap on reset streams queued before accept.
    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self;
    /// Cap on locally-reset streams retained for error reporting.
    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self;
    /// HPACK dynamic table size we advertise.
    fn header_table_size(&mut self, size: u32) -> &mut Self;
    /// Outbound DATA frames in flight before backpressure.
    fn data_frame_budget(&mut self, budget: usize) -> &mut Self;
    /// Run the server handshake over `io`.
    fn handshake<IO>(
        &self,
        io: IO,
    ) -> impl Future<Output = Result<backend::ServerConnection<IO>, Error>> + use<IO, Self>
    where
        IO: AsyncRead + AsyncWrite + Unpin;
}

/// Keepalive ping handle for one connection.
pub(crate) trait PingPong {
    /// Send a PING and wait for its ACK.
    fn ping(&mut self, ping: backend::Ping) -> impl Future<Output = Result<backend::Pong, Error>>;
}
