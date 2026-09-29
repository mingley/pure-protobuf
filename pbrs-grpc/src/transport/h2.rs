//! h2-backed implementation of the [`super`] transport seam (H2-02).
//!
//! This is the only module that may name `h2::` types. Each newtype fixes
//! h2's buffer generic to [`Bytes`] and implements the [`super`] trait 1:1,
//! so the reroute is behavior-preserving by construction. Call sites import
//! the [`super`] traits for methods and name these types in signatures; a
//! future native engine (H2-04) re-implements the same traits.

use crate::bdp;
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
pub(crate) struct RecvStream {
    inner: ::h2::RecvStream,
    recorder: bdp::Recorder,
}

impl RecvStream {
    fn new(inner: ::h2::RecvStream, recorder: bdp::Recorder) -> Self {
        Self { inner, recorder }
    }
}

impl fmt::Debug for RecvStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl super::RecvStream for RecvStream {
    fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, Error>>> {
        match self.inner.poll_data(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Ok(bytes))) => {
                self.recorder.record_data(bytes.len());
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(Error(error)))),
        }
    }

    fn poll_trailers(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<HeaderMap>, Error>> {
        self.inner.poll_trailers(cx).map(|r| r.map_err(Error))
    }

    fn trailers(&mut self) -> impl Future<Output = Result<Option<HeaderMap>, Error>> {
        let fut = self.inner.trailers();
        async move { fut.await.map_err(Error) }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    /// Owned window handle. h2's handle is a shared refcounted reference to
    /// the stream's window, so cloning it here releases capacity against the
    /// same window as the borrowed `h2::RecvStream::flow_control`.
    fn flow_control(&mut self) -> FlowControl {
        FlowControl(self.inner.flow_control().clone())
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
pub(crate) struct SendRequest {
    inner: ::h2::client::SendRequest<Bytes>,
    recorder: bdp::Recorder,
}

impl SendRequest {
    fn new(inner: ::h2::client::SendRequest<Bytes>) -> Self {
        Self {
            inner,
            recorder: bdp::Recorder::disabled(),
        }
    }

    fn set_recorder(&mut self, recorder: bdp::Recorder) {
        self.recorder = recorder;
    }
}

impl fmt::Debug for SendRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl super::SendRequest for SendRequest {
    fn ready(self) -> ReadySendRequest {
        ReadySendRequest(self.inner.ready())
    }

    fn send_request(
        &mut self,
        request: Request<()>,
        end_of_stream: bool,
    ) -> Result<(ResponseFuture, SendStream), Error> {
        self.inner
            .send_request(request, end_of_stream)
            .map(|(rsp, send)| {
                (
                    ResponseFuture {
                        inner: rsp,
                        recorder: self.recorder.clone(),
                    },
                    SendStream(send),
                )
            })
            .map_err(Error)
    }

    fn current_max_send_streams(&self) -> usize {
        self.inner.current_max_send_streams()
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
            .map(|r| r.map(SendRequest::new).map_err(Error))
    }
}

/// Future that resolves to the response headers of one request.
pub(crate) struct ResponseFuture {
    inner: ::h2::client::ResponseFuture,
    recorder: bdp::Recorder,
}

impl fmt::Debug for ResponseFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseFuture").finish_non_exhaustive()
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response<RecvStream>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let recorder = self.recorder.clone();
        Pin::new(&mut self.inner).poll(cx).map(|r| {
            r.map(|rsp| rsp.map(|recv| RecvStream::new(recv, recorder)))
                .map_err(Error)
        })
    }
}

/// Client connection driver: polled until the connection closes.
pub(crate) struct ClientConnection<IO> {
    inner: ::h2::client::Connection<IO, Bytes>,
    ping: Option<bdp::Driver>,
}

impl<IO> ClientConnection<IO> {
    fn new(inner: ::h2::client::Connection<IO, Bytes>) -> Self {
        Self { inner, ping: None }
    }

    fn install_ping_driver(&mut self, driver: bdp::Driver) {
        self.ping = Some(driver);
    }
}

impl<IO> fmt::Debug for ClientConnection<IO> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConnection").finish_non_exhaustive()
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for ClientConnection<IO> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(driver) = &mut self.ping {
            match driver.poll(cx) {
                Poll::Ready(Ok(bdp::Event::SizeUpdate(window))) => {
                    self.inner.set_target_window_size(window);
                    if let Err(error) = self.inner.set_initial_window_size(window) {
                        return Poll::Ready(Err(Error(error)));
                    }
                }
                Poll::Ready(Ok(bdp::Event::KeepAliveTimedOut)) => return Poll::Ready(Ok(())),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => {}
            }
        }
        Pin::new(&mut self.inner).poll(cx).map_err(Error)
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> super::ClientConnection for ClientConnection<IO> {
    fn ping_pong(&mut self) -> Option<PingPong> {
        if self.ping.is_some() {
            None
        } else {
            self.inner.ping_pong().map(PingPong)
        }
    }
}

/// Builder for client connections.
pub(crate) struct ClientBuilder {
    inner: ::h2::client::Builder,
    adaptive: Option<bdp::Config>,
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder").finish_non_exhaustive()
    }
}

impl super::ClientBuilder for ClientBuilder {
    fn new() -> Self {
        Self {
            inner: ::h2::client::Builder::new(),
            adaptive: None,
        }
    }

    fn initial_window_size(&mut self, size: u32) -> &mut Self {
        self.inner.initial_window_size(size);
        self
    }

    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self {
        self.inner.initial_connection_window_size(size);
        self
    }

    fn max_frame_size(&mut self, max: u32) -> &mut Self {
        self.inner.max_frame_size(max);
        self
    }

    fn max_header_list_size(&mut self, max: u32) -> &mut Self {
        self.inner.max_header_list_size(max);
        self
    }

    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self {
        self.inner.max_concurrent_streams(max);
        self
    }

    fn initial_max_send_streams(&mut self, initial: usize) -> &mut Self {
        self.inner.initial_max_send_streams(initial);
        self
    }

    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self {
        self.inner.max_concurrent_reset_streams(max);
        self
    }

    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self {
        self.inner.reset_stream_duration(dur);
        self
    }

    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self {
        self.inner.max_send_buffer_size(max);
        self
    }

    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self {
        self.inner.max_pending_accept_reset_streams(max);
        self
    }

    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self {
        self.inner.max_local_error_reset_streams(max);
        self
    }

    fn enable_push(&mut self, enabled: bool) -> &mut Self {
        self.inner.enable_push(enabled);
        self
    }

    fn header_table_size(&mut self, size: u32) -> &mut Self {
        self.inner.header_table_size(size);
        self
    }

    fn data_frame_budget(&mut self, budget: usize) -> &mut Self {
        self.inner.data_frame_budget(budget);
        self
    }

    fn adaptive_window(&mut self, config: Option<bdp::Config>) -> &mut Self {
        self.adaptive = config;
        self
    }

    fn handshake<IO>(
        self,
        io: IO,
    ) -> impl Future<Output = Result<(SendRequest, ClientConnection<IO>), Error>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        // Box `io` so the async block captures 8 bytes, not the whole
        // stream: coroutine layouts keep dead upvar storage, and h2's
        // handshake future owns `io` again, so an unboxed upvar would pay
        // for it twice (a TLS stream is ~1KB). One setup-time alloc.
        let builder = self.inner;
        let adaptive = self.adaptive;
        let io = Box::new(io);
        async move {
            let (inner_send, inner_conn) =
                builder.handshake::<IO, Bytes>(*io).await.map_err(Error)?;
            let mut send = SendRequest::new(inner_send);
            let mut conn = ClientConnection::new(inner_conn);
            if let Some(config) = adaptive {
                if let Some(ping_pong) = conn.inner.ping_pong().map(PingPong) {
                    let (recorder, driver) = bdp::Driver::new(ping_pong, config);
                    send.set_recorder(recorder);
                    conn.install_ping_driver(driver);
                }
            }
            Ok((send, conn))
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
pub(crate) struct ServerConnection<IO> {
    inner: ::h2::server::Connection<IO, Bytes>,
    recorder: bdp::Recorder,
    ping: Option<bdp::Driver>,
    ping_timed_out: bool,
}

impl<IO> ServerConnection<IO> {
    fn new(inner: ::h2::server::Connection<IO, Bytes>) -> Self {
        Self {
            inner,
            recorder: bdp::Recorder::disabled(),
            ping: None,
            ping_timed_out: false,
        }
    }

    fn install_ping_driver(&mut self, recorder: bdp::Recorder, driver: bdp::Driver) {
        self.recorder = recorder;
        self.ping = Some(driver);
    }

    fn poll_ping(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        let Some(driver) = &mut self.ping else {
            return Poll::Pending;
        };
        match driver.poll(cx) {
            Poll::Ready(Ok(bdp::Event::SizeUpdate(window))) => {
                self.inner.set_target_window_size(window);
                match self.inner.set_initial_window_size(window) {
                    Ok(()) => Poll::Ready(Ok(())),
                    Err(error) => Poll::Ready(Err(Error(error))),
                }
            }
            Poll::Ready(Ok(bdp::Event::KeepAliveTimedOut)) => {
                self.ping_timed_out = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

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
        if self.ping_timed_out {
            return Poll::Ready(None);
        }
        if let Poll::Ready(Err(error)) = self.poll_ping(cx) {
            return Poll::Ready(Some(Err(error)));
        }
        let recorder = self.recorder.clone();
        self.inner.poll_accept(cx).map(|opt| {
            opt.map(|r| {
                r.map(|(req, rsp)| {
                    (
                        req.map(|recv| RecvStream::new(recv, recorder)),
                        SendResponse(rsp),
                    )
                })
                .map_err(Error)
            })
        })
    }

    fn accept(&mut self) -> impl Future<Output = Option<Result<super::Accepted, Error>>> {
        std::future::poll_fn(|cx| self.poll_accept(cx))
    }

    fn ping_pong(&mut self) -> Option<PingPong> {
        if self.ping.is_some() {
            None
        } else {
            self.inner.ping_pong().map(PingPong)
        }
    }

    fn graceful_shutdown(&mut self) {
        self.inner.graceful_shutdown();
    }
}

/// Builder for server connections.
pub(crate) struct ServerBuilder {
    inner: ::h2::server::Builder,
    adaptive: Option<bdp::Config>,
}

impl fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerBuilder").finish_non_exhaustive()
    }
}

impl super::ServerBuilder for ServerBuilder {
    fn new() -> Self {
        Self {
            inner: ::h2::server::Builder::new(),
            adaptive: None,
        }
    }

    fn initial_window_size(&mut self, size: u32) -> &mut Self {
        self.inner.initial_window_size(size);
        self
    }

    fn initial_connection_window_size(&mut self, size: u32) -> &mut Self {
        self.inner.initial_connection_window_size(size);
        self
    }

    fn max_frame_size(&mut self, max: u32) -> &mut Self {
        self.inner.max_frame_size(max);
        self
    }

    fn max_header_list_size(&mut self, max: u32) -> &mut Self {
        self.inner.max_header_list_size(max);
        self
    }

    fn max_concurrent_streams(&mut self, max: u32) -> &mut Self {
        self.inner.max_concurrent_streams(max);
        self
    }

    fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self {
        self.inner.max_concurrent_reset_streams(max);
        self
    }

    fn reset_stream_duration(&mut self, dur: std::time::Duration) -> &mut Self {
        self.inner.reset_stream_duration(dur);
        self
    }

    fn max_send_buffer_size(&mut self, max: usize) -> &mut Self {
        self.inner.max_send_buffer_size(max);
        self
    }

    fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self {
        self.inner.max_pending_accept_reset_streams(max);
        self
    }

    fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self {
        self.inner.max_local_error_reset_streams(max);
        self
    }

    fn header_table_size(&mut self, size: u32) -> &mut Self {
        self.inner.header_table_size(size);
        self
    }

    fn data_frame_budget(&mut self, budget: usize) -> &mut Self {
        self.inner.data_frame_budget(budget);
        self
    }

    fn adaptive_window(&mut self, config: Option<bdp::Config>) -> &mut Self {
        self.adaptive = config;
        self
    }

    fn handshake<IO>(self, io: IO) -> impl Future<Output = Result<ServerConnection<IO>, Error>>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        // Box `io` so the async block captures 8 bytes, not the whole
        // stream: coroutine layouts keep dead upvar storage, and h2's
        // handshake future owns `io` again, so an unboxed upvar would pay
        // for it twice (a TLS stream is ~1KB). One setup-time alloc.
        let builder = self.inner;
        let adaptive = self.adaptive;
        let io = Box::new(io);
        async move {
            let inner = builder.handshake::<IO, Bytes>(*io).await.map_err(Error)?;
            let mut conn = ServerConnection::new(inner);
            if let Some(config) = adaptive {
                if let Some(ping_pong) = conn.inner.ping_pong().map(PingPong) {
                    let (recorder, driver) = bdp::Driver::new(ping_pong, config);
                    conn.install_ping_driver(recorder, driver);
                }
            }
            Ok(conn)
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

impl PingPong {
    pub(crate) fn send_ping(&mut self, ping: Ping) -> Result<(), Error> {
        self.0.send_ping(ping.0).map_err(Error)
    }

    pub(crate) fn poll_pong(&mut self, cx: &mut Context<'_>) -> Poll<Result<Pong, Error>> {
        self.0.poll_pong(cx).map(|r| r.map(Pong).map_err(Error))
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
