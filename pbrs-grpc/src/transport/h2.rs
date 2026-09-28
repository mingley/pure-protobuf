//! h2-backed implementation of the [`super`] transport seam (H2-02).
//!
//! This is the only module that may name `h2::` types. Each newtype fixes
//! h2's buffer generic to [`Bytes`] and implements the [`super`] trait 1:1,
//! so the reroute is behavior-preserving by construction. Call sites import
//! the [`super`] traits for methods and name these types in signatures; a
//! future native engine (H2-04) re-implements the same traits.

use bytes::Bytes;
use http::{HeaderMap, Request, Response};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Transport error. Forwards `h2::Error` exactly: message, kind probes, and
/// `std::error::Error` cause chain.
pub(crate) struct Error(::h2::Error);

impl Error {
    /// The reset reason carried by this error, if any.
    pub(crate) fn reason(&self) -> Option<Reason> {
        self.0.reason().map(Reason)
    }

    /// True when the error wraps an underlying I/O failure.
    pub(crate) fn is_io(&self) -> bool {
        self.0.is_io()
    }

    /// True when the error reports a GOAWAY frame.
    pub(crate) fn is_go_away(&self) -> bool {
        self.0.is_go_away()
    }

    /// True when the error reports a stream reset.
    pub(crate) fn is_reset(&self) -> bool {
        self.0.is_reset()
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl From<::h2::Error> for Error {
    fn from(err: ::h2::Error) -> Self {
        Self(err)
    }
}

impl From<Reason> for Error {
    fn from(reason: Reason) -> Self {
        Self(::h2::Error::from(reason.0))
    }
}

// ---------------------------------------------------------------------------
// Reason
// ---------------------------------------------------------------------------

/// HTTP/2 error code (RST_STREAM / GOAWAY). Same code points as `h2::Reason`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reason(::h2::Reason);

#[allow(
    dead_code,
    reason = "complete RFC 9113 code space; the crate sends only CANCEL, INTERNAL_ERROR, and REFUSED_STREAM today"
)]
impl Reason {
    pub(crate) const NO_ERROR: Reason = Reason(::h2::Reason::NO_ERROR);
    pub(crate) const PROTOCOL_ERROR: Reason = Reason(::h2::Reason::PROTOCOL_ERROR);
    pub(crate) const INTERNAL_ERROR: Reason = Reason(::h2::Reason::INTERNAL_ERROR);
    pub(crate) const FLOW_CONTROL_ERROR: Reason = Reason(::h2::Reason::FLOW_CONTROL_ERROR);
    pub(crate) const SETTINGS_TIMEOUT: Reason = Reason(::h2::Reason::SETTINGS_TIMEOUT);
    pub(crate) const STREAM_CLOSED: Reason = Reason(::h2::Reason::STREAM_CLOSED);
    pub(crate) const FRAME_SIZE_ERROR: Reason = Reason(::h2::Reason::FRAME_SIZE_ERROR);
    pub(crate) const REFUSED_STREAM: Reason = Reason(::h2::Reason::REFUSED_STREAM);
    pub(crate) const CANCEL: Reason = Reason(::h2::Reason::CANCEL);
    pub(crate) const COMPRESSION_ERROR: Reason = Reason(::h2::Reason::COMPRESSION_ERROR);
    pub(crate) const CONNECT_ERROR: Reason = Reason(::h2::Reason::CONNECT_ERROR);
    pub(crate) const ENHANCE_YOUR_CALM: Reason = Reason(::h2::Reason::ENHANCE_YOUR_CALM);
    pub(crate) const INADEQUATE_SECURITY: Reason = Reason(::h2::Reason::INADEQUATE_SECURITY);
    pub(crate) const HTTP_1_1_REQUIRED: Reason = Reason(::h2::Reason::HTTP_1_1_REQUIRED);
}

impl fmt::Debug for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<u32> for Reason {
    fn from(code: u32) -> Self {
        Self(::h2::Reason::from(code))
    }
}

impl From<Reason> for u32 {
    fn from(reason: Reason) -> u32 {
        u32::from(reason.0)
    }
}

// ---------------------------------------------------------------------------
// SendStream
// ---------------------------------------------------------------------------

/// Outbound half of one stream.
pub(crate) struct SendStream(::h2::SendStream<Bytes>);

impl fmt::Debug for SendStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendStream").finish_non_exhaustive()
    }
}

impl super::SendStream for SendStream {
    fn capacity(&self) -> usize {
        self.0.capacity()
    }

    fn reserve_capacity(&mut self, capacity: usize) {
        self.0.reserve_capacity(capacity);
    }

    fn poll_capacity(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<usize, Error>>> {
        self.0
            .poll_capacity(cx)
            .map(|opt| opt.map(|r| r.map_err(Error)))
    }

    fn send_data(&mut self, data: Bytes, end_of_stream: bool) -> Result<(), Error> {
        self.0.send_data(data, end_of_stream).map_err(Error)
    }

    fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Error> {
        self.0.send_trailers(trailers).map_err(Error)
    }

    fn send_reset(&mut self, reason: Reason) {
        self.0.send_reset(reason.0);
    }

    fn poll_reset(&mut self, cx: &mut Context<'_>) -> Poll<Result<Reason, Error>> {
        self.0.poll_reset(cx).map(|r| r.map(Reason).map_err(Error))
    }
}

// ---------------------------------------------------------------------------
// RecvStream + FlowControl
// ---------------------------------------------------------------------------

/// Inbound half of one stream.
pub(crate) struct RecvStream(::h2::RecvStream);

impl fmt::Debug for RecvStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl super::RecvStream for RecvStream {
    fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, Error>>> {
        self.0
            .poll_data(cx)
            .map(|opt| opt.map(|r| r.map_err(Error)))
    }

    fn poll_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<HeaderMap>, Error>> {
        self.0.poll_trailers(cx).map(|r| r.map_err(Error))
    }

    fn trailers(&mut self) -> impl Future<Output = Result<Option<HeaderMap>, Error>> {
        let fut = self.0.trailers();
        async move { fut.await.map_err(Error) }
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    /// Owned window handle. h2's handle is a shared refcounted reference to
    /// the stream's window, so cloning it here releases capacity against the
    /// same window as the borrowed `h2::RecvStream::flow_control`.
    fn flow_control(&mut self) -> FlowControl {
        FlowControl(self.0.flow_control().clone())
    }
}

/// Receive-window handle for one stream.
#[derive(Clone)]
pub(crate) struct FlowControl(::h2::FlowControl);

impl fmt::Debug for FlowControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl super::FlowControl for FlowControl {
    fn release_capacity(&mut self, sz: usize) -> Result<(), Error> {
        self.0.release_capacity(sz).map_err(Error)
    }
}

// ---------------------------------------------------------------------------
// Client: SendRequest, ReadySendRequest, ResponseFuture, Connection, Builder
// ---------------------------------------------------------------------------

/// Client handle that opens outbound streams. Cheap to clone.
#[derive(Clone)]
pub(crate) struct SendRequest(::h2::client::SendRequest<Bytes>);

impl fmt::Debug for SendRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl super::SendRequest for SendRequest {
    fn ready(self) -> ReadySendRequest {
        ReadySendRequest(self.0.ready())
    }

    fn send_request(
        &mut self,
        request: Request<()>,
        end_of_stream: bool,
    ) -> Result<(ResponseFuture, SendStream), Error> {
        self.0
            .send_request(request, end_of_stream)
            .map(|(rsp, send)| (ResponseFuture(rsp), SendStream(send)))
            .map_err(Error)
    }

    fn current_max_send_streams(&self) -> usize {
        self.0.current_max_send_streams()
    }
}

/// Future that resolves to a ready [`SendRequest`].
pub(crate) struct ReadySendRequest(::h2::client::ReadySendRequest<Bytes>);

impl fmt::Debug for ReadySendRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadySendRequest").finish_non_exhaustive()
    }
}

impl Future for ReadySendRequest {
    type Output = Result<SendRequest, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|r| r.map(SendRequest).map_err(Error))
    }
}

/// Future that resolves to the response headers of one request.
pub(crate) struct ResponseFuture(::h2::client::ResponseFuture);

impl fmt::Debug for ResponseFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseFuture").finish_non_exhaustive()
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response<RecvStream>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|r| r.map(|rsp| rsp.map(RecvStream)).map_err(Error))
    }
}

/// Client connection driver: polled until the connection closes.
pub(crate) struct ClientConnection<IO>(::h2::client::Connection<IO, Bytes>);

impl<IO> fmt::Debug for ClientConnection<IO> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConnection").finish_non_exhaustive()
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for ClientConnection<IO> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map_err(Error)
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> super::ClientConnection for ClientConnection<IO> {
    fn ping_pong(&mut self) -> Option<PingPong> {
        self.0.ping_pong().map(PingPong)
    }
}

/// Builder for client connections.
pub(crate) struct ClientBuilder(::h2::client::Builder);

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder").finish_non_exhaustive()
    }
}

impl super::ClientBuilder for ClientBuilder {
    fn new() -> Self {
        Self(::h2::client::Builder::new())
    }

    fn initial_window_size(&mut self, size: u32) -> &mut Self {
        self.0.initial_window_size(size);
        self
    }

    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self {
        self.0.initial_connection_window_size(size);
        self
    }

    fn max_frame_size(&mut self, max: u32) -> &mut Self {
        self.0.max_frame_size(max);
        self
    }

    fn max_header_list_size(&mut self, max: u32) -> &mut Self {
        self.0.max_header_list_size(max);
        self
    }

    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self {
        self.0.max_concurrent_streams(max);
        self
    }

    fn initial_max_send_streams(&mut self, initial: usize) -> &mut Self {
        self.0.initial_max_send_streams(initial);
        self
    }

    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self {
        self.0.max_concurrent_reset_streams(max);
        self
    }

    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self {
        self.0.reset_stream_duration(dur);
        self
    }

    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self {
        self.0.max_send_buffer_size(max);
        self
    }

    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self {
        self.0.max_pending_accept_reset_streams(max);
        self
    }

    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self {
        self.0.max_local_error_reset_streams(max);
        self
    }

    fn enable_push(&mut self, enabled: bool) -> &mut Self {
        self.0.enable_push(enabled);
        self
    }

    fn header_table_size(&mut self, size: u32) -> &mut Self {
        self.0.header_table_size(size);
        self
    }

    fn data_frame_budget(&mut self, budget: usize) -> &mut Self {
        self.0.data_frame_budget(budget);
        self
    }

    fn handshake<IO>(
        &self,
        io: IO,
    ) -> impl Future<Output = Result<(SendRequest, ClientConnection<IO>), Error>> + use<IO>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        // Box `io` so the async block captures 8 bytes, not the whole
        // stream: coroutine layouts keep dead upvar storage, and h2's
        // handshake future owns `io` again, so an unboxed upvar would pay
        // for it twice (a TLS stream is ~1KB). One setup-time alloc.
        let builder = self.0.clone();
        let io = Box::new(io);
        async move {
            builder
                .handshake::<IO, Bytes>(*io)
                .await
                .map(|(send, conn)| (SendRequest(send), ClientConnection(conn)))
                .map_err(Error)
        }
    }
}

// ---------------------------------------------------------------------------
// Server: SendResponse, Connection, Builder
// ---------------------------------------------------------------------------

/// Server handle that answers one accepted stream.
pub(crate) struct SendResponse(::h2::server::SendResponse<Bytes>);

impl fmt::Debug for SendResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendResponse").finish_non_exhaustive()
    }
}

impl super::SendResponse for SendResponse {
    fn send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<SendStream, Error> {
        self.0
            .send_response(response, end_of_stream)
            .map(SendStream)
            .map_err(Error)
    }

    fn send_reset(&mut self, reason: Reason) {
        self.0.send_reset(reason.0);
    }

    fn poll_reset(&mut self, cx: &mut Context<'_>) -> Poll<Result<Reason, Error>> {
        self.0.poll_reset(cx).map(|r| r.map(Reason).map_err(Error))
    }
}

/// Server connection driver: accepts streams until the connection closes.
pub(crate) struct ServerConnection<IO>(::h2::server::Connection<IO, Bytes>);

impl<IO> fmt::Debug for ServerConnection<IO> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConnection").finish_non_exhaustive()
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> super::ServerConnection for ServerConnection<IO> {
    fn poll_accept(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<super::Accepted, Error>>> {
        self.0.poll_accept(cx).map(|opt| {
            opt.map(|r| {
                r.map(|(req, rsp)| (req.map(RecvStream), SendResponse(rsp)))
                    .map_err(Error)
            })
        })
    }

    fn accept(&mut self) -> impl Future<Output = Option<Result<super::Accepted, Error>>> {
        std::future::poll_fn(|cx| self.poll_accept(cx))
    }

    fn ping_pong(&mut self) -> Option<PingPong> {
        self.0.ping_pong().map(PingPong)
    }

    fn graceful_shutdown(&mut self) {
        self.0.graceful_shutdown();
    }
}

/// Builder for server connections.
pub(crate) struct ServerBuilder(::h2::server::Builder);

impl fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerBuilder").finish_non_exhaustive()
    }
}

impl super::ServerBuilder for ServerBuilder {
    fn new() -> Self {
        Self(::h2::server::Builder::new())
    }

    fn initial_window_size(&mut self, size: u32) -> &mut Self {
        self.0.initial_window_size(size);
        self
    }

    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self {
        self.0.initial_connection_window_size(size);
        self
    }

    fn max_frame_size(&mut self, max: u32) -> &mut Self {
        self.0.max_frame_size(max);
        self
    }

    fn max_header_list_size(&mut self, max: u32) -> &mut Self {
        self.0.max_header_list_size(max);
        self
    }

    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self {
        self.0.max_concurrent_streams(max);
        self
    }

    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self {
        self.0.max_concurrent_reset_streams(max);
        self
    }

    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self {
        self.0.reset_stream_duration(dur);
        self
    }

    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self {
        self.0.max_send_buffer_size(max);
        self
    }

    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self {
        self.0.max_pending_accept_reset_streams(max);
        self
    }

    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self {
        self.0.max_local_error_reset_streams(max);
        self
    }

    fn header_table_size(&mut self, size: u32) -> &mut Self {
        self.0.header_table_size(size);
        self
    }

    fn data_frame_budget(&mut self, budget: usize) -> &mut Self {
        self.0.data_frame_budget(budget);
        self
    }

    fn handshake<IO>(
        &self,
        io: IO,
    ) -> impl Future<Output = Result<ServerConnection<IO>, Error>> + use<IO>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        // Box `io` so the async block captures 8 bytes, not the whole
        // stream: coroutine layouts keep dead upvar storage, and h2's
        // handshake future owns `io` again, so an unboxed upvar would pay
        // for it twice (a TLS stream is ~1KB). One setup-time alloc.
        let builder = self.0.clone();
        let io = Box::new(io);
        async move {
            builder
                .handshake::<IO, Bytes>(*io)
                .await
                .map(ServerConnection)
                .map_err(Error)
        }
    }
}

// ---------------------------------------------------------------------------
// Ping / Pong / PingPong
// ---------------------------------------------------------------------------

/// Keepalive ping handle for one connection.
pub(crate) struct PingPong(::h2::PingPong);

impl fmt::Debug for PingPong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl super::PingPong for PingPong {
    fn ping(&mut self, ping: Ping) -> impl Future<Output = Result<Pong, Error>> {
        let fut = self.0.ping(ping.0);
        async move { fut.await.map(Pong).map_err(Error) }
    }
}

/// Outbound PING frame. Payload is always empty (`opaque`), as in h2.
pub(crate) struct Ping(::h2::Ping);

impl fmt::Debug for Ping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl Ping {
    pub(crate) fn opaque() -> Self {
        Self(::h2::Ping::opaque())
    }
}

/// Acknowledgement of a [`Ping`].
pub(crate) struct Pong(::h2::Pong);

impl fmt::Debug for Pong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
