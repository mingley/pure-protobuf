//! Server implementation of the HTTP/2 protocol.
//!
//! # Getting started
//!
//! Running an HTTP/2 server requires the caller to manage accepting the
//! connections as well as getting the connections to a state that is ready to
//! begin the HTTP/2 handshake. See [here](../index.html#handshake) for more
//! details.
//!
//! This could be as basic as using Tokio's [`TcpListener`] to accept
//! connections, but usually it means using either ALPN or HTTP/1.1 protocol
//! upgrades.
//!
//! Once a connection is obtained, it is passed to [`handshake`],
//! which will begin the [HTTP/2 handshake]. This returns a future that
//! completes once the handshake process is performed and HTTP/2 streams may
//! be received.
//!
//! [`handshake`] uses default configuration values. There are a number of
//! settings that can be changed by using [`Builder`] instead.
//!
//! # Inbound streams
//!
//! The [`Connection`] instance is used to accept inbound HTTP/2 streams. It
//! does this by implementing [`futures::Stream`]. When a new stream is
//! received, a call to [`Connection::accept`] will return `(request, response)`.
//! The `request` handle (of type [`http::Request<RecvStream>`]) contains the
//! HTTP request head as well as provides a way to receive the inbound data
//! stream and the trailers. The `response` handle (of type [`SendResponse`])
//! allows responding to the request, stream the response payload, send
//! trailers, and send push promises.
//!
//! The send ([`SendStream`]) and receive ([`RecvStream`]) halves of the stream
//! can be operated independently.
//!
//! # Managing the connection
//!
//! The [`Connection`] instance is used to manage connection state. The caller
//! is required to call either [`Connection::accept`] or
//! [`Connection::poll_close`] in order to advance the connection state. Simply
//! operating on [`SendStream`] or [`RecvStream`] will have no effect unless the
//! connection state is advanced.
//!
//! It is not required to call **both** [`Connection::accept`] and
//! [`Connection::poll_close`]. If the caller is ready to accept a new stream,
//! then only [`Connection::accept`] should be called. When the caller **does
//! not** want to accept a new stream, [`Connection::poll_close`] should be
//! called.
//!
//! The [`Connection`] instance should only be dropped once
//! [`Connection::poll_close`] returns `Ready`. Once [`Connection::accept`]
//! returns `Ready(None)`, there will no longer be any more inbound streams. At
//! this point, only [`Connection::poll_close`] should be called.
//!
//! # Shutting down the server
//!
//! Graceful shutdown of the server is [not yet
//! implemented](https://github.com/hyperium/h2/issues/69).
//!
//! # Example
//!
//! A basic HTTP/2 server example that runs over TCP and assumes [prior
//! knowledge], i.e. both the client and the server assume that the TCP socket
//! will use the HTTP/2 protocol without prior negotiation.
//!
//! ```text
//! use h2::server;
//! use http::{Response, StatusCode};
//! use tokio::net::TcpListener;
//!
//! #[tokio::main]
//! pub async fn main() {
//!     let mut listener = TcpListener::bind("127.0.0.1:5928").await.unwrap();
//!
//!     // Accept all incoming TCP connections.
//!     loop {
//!         if let Ok((socket, _peer_addr)) = listener.accept().await {
//!             // Spawn a new task to process each connection.
//!             tokio::spawn(async {
//!                 // Start the HTTP/2 connection handshake
//!                 let mut h2 = server::handshake(socket).await.unwrap();
//!                 // Accept all inbound HTTP/2 streams sent over the
//!                 // connection.
//!                 while let Some(request) = h2.accept().await {
//!                     let (request, mut respond) = request.unwrap();
//!                     println!("Received request: {:?}", request);
//!
//!                     // Build a response with no body
//!                     let response = Response::builder()
//!                         .status(StatusCode::OK)
//!                         .body(())
//!                         .unwrap();
//!
//!                     // Send the response back to the client
//!                     respond.send_response(response, true)
//!                         .unwrap();
//!                 }
//!
//!             });
//!         }
//!     }
//! }
//! ```
//!
//! [prior knowledge]: http://httpwg.org/specs/rfc7540.html#known-http
//! [`handshake`]: fn.handshake.html
//! [HTTP/2 handshake]: http://httpwg.org/specs/rfc7540.html#ConnectionHeader
//! [`Builder`]: struct.Builder.html
//! [`Connection`]: struct.Connection.html
//! [`Connection::poll`]: struct.Connection.html#method.poll
//! [`Connection::poll_close`]: struct.Connection.html#method.poll_close
//! [`futures::Stream`]: https://docs.rs/futures/0.1/futures/stream/trait.Stream.html
//! [`http::Request<RecvStream>`]: ../struct.RecvStream.html
//! [`RecvStream`]: ../struct.RecvStream.html
//! [`SendStream`]: ../struct.SendStream.html
//! [`TcpListener`]: https://docs.rs/tokio-core/0.1/tokio_core/net/struct.TcpListener.html

use crate::h2_backend::codec::{Codec, UserError};
use crate::h2_backend::frame::{self, Pseudo, PushPromiseHeaderError, Reason, Settings, StreamId};
use crate::h2_backend::hpack::BytesStr;
use crate::h2_backend::proto::{self, Config, Error, Prioritized};
use crate::h2_backend::{FlowControl, PingPong, RecvStream, SendStream};

use bytes::{Buf, Bytes};
use http::{HeaderMap, Method, Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{fmt, io};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::instrument::{Instrument, Instrumented};

/// In progress HTTP/2 connection handshake future.
///
/// This type implements `Future`, yielding a `Connection` instance once the
/// handshake has completed.
///
/// The handshake is completed once the connection preface is fully received
/// from the client **and** the initial settings frame is sent to the client.
///
/// The handshake future does not wait for the initial settings frame from the
/// client.
///
/// See [module] level docs for more details.
///
/// [module]: index.html
#[must_use = "futures do nothing unless polled"]
pub struct Handshake<T, B: Buf = Bytes> {
    /// The config to pass to Connection::new after handshake succeeds.
    builder: Builder,
    /// The current state of the handshake.
    state: Handshaking<T, B>,
    /// Span tracking the handshake
    span: tracing::Span,
}

/// Accepts inbound HTTP/2 streams on a connection.
///
/// A `Connection` is backed by an I/O resource (usually a TCP socket) and
/// implements the HTTP/2 server logic for that connection. It is responsible
/// for receiving inbound streams initiated by the client as well as driving the
/// internal state forward.
///
/// `Connection` values are created by calling [`handshake`]. Once a
/// `Connection` value is obtained, the caller must call [`poll`] or
/// [`poll_close`] in order to drive the internal connection state forward.
///
/// See [module level] documentation for more details
///
/// [module level]: index.html
/// [`handshake`]: struct.Connection.html#method.handshake
/// [`poll`]: struct.Connection.html#method.poll
/// [`poll_close`]: struct.Connection.html#method.poll_close
///
/// # Examples
///
/// ```text
/// # use tokio::io::{AsyncRead, AsyncWrite};
/// # use h2::server;
/// # use h2::server::*;
/// #
/// # async fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T) {
/// let mut server = server::handshake(my_io).await.unwrap();
/// while let Some(request) = server.accept().await {
///     tokio::spawn(async move {
///         let (request, respond) = request.unwrap();
///         // Process the request and send the response back to the client
///         // using `respond`.
///     });
/// }
/// # }
/// #
/// # pub fn main() {}
/// ```
#[must_use = "streams do nothing unless polled"]
pub struct Connection<T, B: Buf> {
    connection: proto::Connection<T, Peer, B>,
}

/// Builds server connections with custom configuration values.
///
/// Methods can be chained in order to set the configuration values.
///
/// The server is constructed by calling [`handshake`] and passing the I/O
/// handle that will back the HTTP/2 server.
///
/// New instances of `Builder` are obtained via [`Builder::new`].
///
/// See function level documentation for details on the various server
/// configuration settings.
///
/// [`Builder::new`]: struct.Builder.html#method.new
/// [`handshake`]: struct.Builder.html#method.handshake
///
/// # Examples
///
/// ```text
/// # use tokio::io::{AsyncRead, AsyncWrite};
/// # use h2::server::*;
/// #
/// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
/// # -> Handshake<T>
/// # {
/// // `server_fut` is a future representing the completion of the HTTP/2
/// // handshake.
/// let server_fut = Builder::new()
///     .initial_window_size(1_000_000)
///     .max_concurrent_streams(1000)
///     .handshake(my_io);
/// # server_fut
/// # }
/// #
/// # pub fn main() {}
/// ```
#[derive(Clone, Debug)]
pub struct Builder {
    /// Time to keep locally reset streams around before reaping.
    reset_stream_duration: Duration,

    /// Maximum number of locally reset streams to keep at a time.
    reset_stream_max: usize,

    /// Maximum number of remotely reset streams to allow in the pending
    /// accept queue.
    pending_accept_reset_stream_max: usize,

    /// Initial `Settings` frame to send as part of the handshake.
    settings: Settings,

    /// Initial target window size for new connections.
    initial_target_connection_window_size: Option<u32>,

    /// Maximum amount of bytes to "buffer" for writing per stream.
    max_send_buffer_size: usize,

    /// Maximum number of locally reset streams due to protocol error across
    /// the lifetime of the connection.
    ///
    /// When this gets exceeded, we issue GOAWAYs.
    local_max_error_reset_streams: Option<usize>,

    /// connection-level budget for DATA framing overhead.
    ///
    /// When this gets exhausted, we issue a GOAWAY with `ENHANCE_YOUR_CALM`.
    data_frame_budget: proto::DataFrameBudget,
}

/// Send a response back to the client
///
/// A `SendResponse` instance is provided when receiving a request and is used
/// to send the associated response back to the client. It is also used to
/// explicitly reset the stream with a custom reason.
///
/// It will also be used to initiate push promises linked with the associated
/// stream.
///
/// If the `SendResponse` instance is dropped without sending a response, then
/// the HTTP/2 stream will be reset.
///
/// See [module] level docs for more details.
///
/// [module]: index.html
#[derive(Debug)]
pub struct SendResponse<B: Buf> {
    /// `None` after [`SendResponse::send_response`] moves the handle into
    /// the [`SendStream`]. `StreamRef` starts with an `Arc`, so `Option`
    /// does not grow this value.
    inner: Option<proto::StreamRef<B>>,
}

/// A private terminal owner that survives dropping an application call.
#[derive(Debug)]
pub(crate) struct DeadlineResponse<B: Buf> {
    inner: proto::StreamRef<B>,
}

/// The owner chooses how its application protocol can terminate after headers.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DeadlineEnding {
    /// HTTP/2 trailers can replace an unsent body tail.
    Trailers,
    /// Preserve complete queued application frames and append ending DATA.
    Data,
    /// A partial application frame cannot be repaired; reset the stream.
    Reset,
}

impl<B: Buf> DeadlineResponse<B> {
    /// Finish an unfinished response without replacing a completed result.
    ///
    /// An initial DATA-encoded ending can be sent intact. Once body output has
    /// started, such an ending cannot repair an arbitrary partial application
    /// message; that case resets instead of appending ambiguous payload bytes.
    pub(crate) fn finish(
        &mut self,
        response: Response<()>,
        trailers: HeaderMap,
        ending_data: Option<B>,
        post_header: DeadlineEnding,
    ) -> Result<bool, crate::h2_backend::Error> {
        self.inner
            .finish_response(response, trailers, ending_data, post_header)
    }
}

/// Send a response to a promised request
///
/// A `SendPushedResponse` instance is provided when promising a request and is used
/// to send the associated response to the client. It is also used to
/// explicitly reset the stream with a custom reason.
///
/// It can not be used to initiate push promises.
///
/// If the `SendPushedResponse` instance is dropped without sending a response, then
/// the HTTP/2 stream will be reset.
///
/// See [module] level docs for more details.
///
/// [module]: index.html
pub struct SendPushedResponse<B: Buf> {
    inner: SendResponse<B>,
}

// Manual implementation necessary because of rust-lang/rust#26925
impl<B: Buf + fmt::Debug> fmt::Debug for SendPushedResponse<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SendPushedResponse {{ {:?} }}", self.inner)
    }
}

/// Stages of an in-progress handshake.
enum Handshaking<T, B: Buf> {
    /// State 1. Connection is flushing pending SETTINGS frame.
    Flushing(Instrumented<Flush<T, Prioritized<B>>>),
    /// State 2. Connection is waiting for the client preface.
    ReadingPreface(Instrumented<ReadPreface<T, Prioritized<B>>>),
    /// State 3. Handshake is done, polling again would panic.
    Done,
}

/// Flush a Sink
struct Flush<T, B> {
    codec: Option<Codec<T, B>>,
}

/// Read the client connection preface
struct ReadPreface<T, B> {
    codec: Option<Codec<T, B>>,
    pos: usize,
}

#[derive(Debug)]
pub(crate) struct Peer;

const PREFACE: [u8; 24] = *b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Creates a new configured HTTP/2 server with default configuration
/// values backed by `io`.
///
/// It is expected that `io` already be in an appropriate state to commence
/// the [HTTP/2 handshake]. See [Handshake] for more details.
///
/// Returns a future which resolves to the [`Connection`] instance once the
/// HTTP/2 handshake has been completed. The returned [`Connection`]
/// instance will be using default configuration values. Use [`Builder`] to
/// customize the configuration values used by a [`Connection`] instance.
///
/// [HTTP/2 handshake]: http://httpwg.org/specs/rfc7540.html#ConnectionHeader
/// [Handshake]: ../index.html#handshake
/// [`Connection`]: struct.Connection.html
///
/// # Examples
///
/// ```text
/// # use tokio::io::{AsyncRead, AsyncWrite};
/// # use h2::server;
/// # use h2::server::*;
/// #
/// # async fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
/// # {
/// let connection = server::handshake(my_io).await.unwrap();
/// // The HTTP/2 handshake has completed, now use `connection` to
/// // accept inbound HTTP/2 streams.
/// # }
/// #
/// # pub fn main() {}
/// ```
pub fn handshake<T>(io: T) -> Handshake<T, Bytes>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    Builder::new().handshake(io)
}

// ===== impl Connection =====

impl<T, B> Connection<T, B>
where
    T: AsyncRead + AsyncWrite + Unpin,
    B: Buf,
{
    fn handshake2(io: T, builder: Builder) -> Handshake<T, B> {
        let span = tracing::trace_span!("server_handshake");
        let entered = span.enter();

        // Create the codec.
        let mut codec = Codec::new(io);

        if let Some(max) = builder.settings.max_frame_size() {
            codec.set_max_recv_frame_size(max as usize);
        }

        if let Some(max) = builder.settings.max_header_list_size() {
            codec.set_max_recv_header_list_size(max as usize);
        }

        // Send initial settings frame.
        codec
            .buffer(builder.settings.clone().into())
            .expect("invalid SETTINGS frame");

        // Create the handshake future.
        let state =
            Handshaking::Flushing(Flush::new(codec).instrument(tracing::trace_span!("flush")));

        drop(entered);

        Handshake {
            builder,
            state,
            span,
        }
    }

    /// Accept the next incoming request on this connection.
    pub async fn accept(
        &mut self,
    ) -> Option<Result<(Request<RecvStream>, SendResponse<B>), crate::h2_backend::Error>> {
        crate::h2_backend::poll_fn(move |cx| self.poll_accept(cx)).await
    }

    /// The transport maps this request immediately. Keeping the poll out of
    /// line copied the head into this return slot and then out again.
    #[inline(always)]
    #[doc(hidden)]
    pub fn poll_accept(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<(Request<RecvStream>, SendResponse<B>), crate::h2_backend::Error>>>
    {
        self.poll_accept_with(cx, RecvStream::new)
    }

    /// Accept the next request and attach `wrap`'s body in the same map.
    ///
    /// A second `Request::map` copied the head again. The transport body is
    /// built here so that copy does not happen.
    #[inline(always)]
    pub(crate) fn poll_accept_with<R>(
        &mut self,
        cx: &mut Context<'_>,
        wrap: impl FnOnce(FlowControl) -> R,
    ) -> Poll<Option<Result<(Request<R>, SendResponse<B>), crate::h2_backend::Error>>> {
        // Always try to advance the internal state. Getting Pending also is
        // needed to allow this function to return Pending.
        if self.poll_closed(cx)?.is_ready() {
            // If the socket is closed, don't return anything
            // TODO: drop any pending streams
            return Poll::Ready(None);
        }

        if let Some(inner) = self.connection.next_incoming() {
            tracing::trace!("received incoming");
            // `map` moves the head once. `into_parts` then `from_parts`
            // copied that head through two temporaries.
            let request = inner
                .take_request()
                .map(|()| wrap(FlowControl::new(inner.clone_to_opaque())));
            let respond = SendResponse { inner: Some(inner) };

            return Poll::Ready(Some(Ok((request, respond))));
        }

        Poll::Pending
    }

    /// Sets the target window size for the whole connection.
    ///
    /// If `size` is greater than the current value, then a `WINDOW_UPDATE`
    /// frame will be immediately sent to the remote, increasing the connection
    /// level window by `size - current_value`.
    ///
    /// If `size` is less than the current value, nothing will happen
    /// immediately. However, as window capacity is released by
    /// [`FlowControl`] instances, no `WINDOW_UPDATE` frames will be sent
    /// out until the number of "in flight" bytes drops below `size`.
    ///
    /// The default value is 65,535.
    ///
    /// See [`FlowControl`] documentation for more details.
    ///
    /// [`FlowControl`]: ../struct.FlowControl.html
    /// [library level]: ../index.html#flow-control
    pub fn set_target_window_size(&mut self, size: u32) {
        assert!(size <= proto::MAX_WINDOW_SIZE);
        self.connection.set_target_window_size(size);
    }

    /// Set a new `INITIAL_WINDOW_SIZE` setting (in octets) for stream-level
    /// flow control for received data.
    ///
    /// The `SETTINGS` will be sent to the remote, and only applied once the
    /// remote acknowledges the change.
    ///
    /// This can be used to increase or decrease the window size for existing
    /// streams.
    ///
    /// # Errors
    ///
    /// Returns an error if a previous call is still pending acknowledgement
    /// from the remote endpoint.
    pub fn set_initial_window_size(&mut self, size: u32) -> Result<(), crate::h2_backend::Error> {
        assert!(size <= proto::MAX_WINDOW_SIZE);
        self.connection.set_initial_window_size(size)?;
        Ok(())
    }

    /// Enables the [extended CONNECT protocol].
    ///
    /// [extended CONNECT protocol]: https://datatracker.ietf.org/doc/html/rfc8441#section-4
    ///
    /// # Errors
    ///
    /// Returns an error if a previous call is still pending acknowledgement
    /// from the remote endpoint.
    pub fn enable_connect_protocol(&mut self) -> Result<(), crate::h2_backend::Error> {
        self.connection.set_enable_connect_protocol()?;
        Ok(())
    }

    /// Returns `Ready` when the underlying connection has closed.
    ///
    /// If any new inbound streams are received during a call to `poll_closed`,
    /// they will be queued and returned on the next call to [`poll_accept`].
    ///
    /// This function will advance the internal connection state, driving
    /// progress on all the other handles (e.g. [`RecvStream`] and [`SendStream`]).
    ///
    /// See [here](index.html#managing-the-connection) for more details.
    ///
    /// [`poll_accept`]: struct.Connection.html#method.poll_accept
    /// [`RecvStream`]: ../struct.RecvStream.html
    /// [`SendStream`]: ../struct.SendStream.html
    pub fn poll_closed(&mut self, cx: &mut Context) -> Poll<Result<(), crate::h2_backend::Error>> {
        self.connection.poll(cx).map_err(Into::into)
    }

    /// Sets the connection to a GOAWAY state.
    ///
    /// Does not terminate the connection. Must continue being polled to close
    /// connection.
    ///
    /// After flushing the GOAWAY frame, the connection is closed. Any
    /// outstanding streams do not prevent the connection from closing. This
    /// should usually be reserved for shutting down when something bad
    /// external to `h2` has happened, and open streams cannot be properly
    /// handled.
    ///
    /// For graceful shutdowns, see [`graceful_shutdown`](Connection::graceful_shutdown).
    pub fn abrupt_shutdown(&mut self, reason: Reason) {
        self.connection.go_away_from_user(reason);
    }

    /// Starts a [graceful shutdown][1] process.
    ///
    /// Must continue being polled to close connection.
    ///
    /// It's possible to receive more requests after calling this method, since
    /// they might have been in-flight from the client already. After about
    /// 1 RTT, no new requests should be accepted. Once all active streams
    /// have completed, the connection is closed.
    ///
    /// [1]: http://httpwg.org/specs/rfc7540.html#GOAWAY
    pub fn graceful_shutdown(&mut self) {
        self.connection.go_away_gracefully();
    }

    /// Takes a `PingPong` instance from the connection.
    ///
    /// # Note
    ///
    /// This may only be called once. Calling multiple times will return `None`.
    pub fn ping_pong(&mut self) -> Option<PingPong> {
        self.connection.take_user_pings().map(PingPong::new)
    }

    /// Checks if there are any streams
    pub fn has_streams(&self) -> bool {
        self.connection.has_streams()
    }

    /// Returns the maximum number of concurrent streams that may be initiated
    /// by the server on this connection.
    ///
    /// This limit is configured by the client peer by sending the
    /// [`SETTINGS_MAX_CONCURRENT_STREAMS` parameter][1] in a `SETTINGS` frame.
    /// This method returns the currently acknowledged value received from the
    /// remote.
    ///
    /// [1]: https://tools.ietf.org/html/rfc7540#section-5.1.2
    pub fn max_concurrent_send_streams(&self) -> usize {
        self.connection.max_send_streams()
    }

    /// Returns the maximum number of concurrent streams that may be initiated
    /// by the client on this connection.
    ///
    /// This returns the value of the [`SETTINGS_MAX_CONCURRENT_STREAMS`
    /// parameter][1] sent in a `SETTINGS` frame that has been
    /// acknowledged by the remote peer. The value to be sent is configured by
    /// the [`Builder::max_concurrent_streams`][2] method before handshaking
    /// with the remote peer.
    ///
    /// [1]: https://tools.ietf.org/html/rfc7540#section-5.1.2
    /// [2]: ../struct.Builder.html#method.max_concurrent_streams
    pub fn max_concurrent_recv_streams(&self) -> usize {
        self.connection.max_recv_streams()
    }

    // Could disappear at anytime.
    #[doc(hidden)]
    #[cfg(any())]
    pub fn num_wired_streams(&self) -> usize {
        self.connection.num_wired_streams()
    }
}

#[cfg(any())]
impl<T, B> futures_core::Stream for Connection<T, B>
where
    T: AsyncRead + AsyncWrite + Unpin,
    B: Buf,
{
    type Item = Result<(Request<RecvStream>, SendResponse<B>), crate::h2_backend::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_accept(cx)
    }
}

impl<T, B> fmt::Debug for Connection<T, B>
where
    T: fmt::Debug,
    B: fmt::Debug + Buf,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Connection")
            .field("connection", &self.connection)
            .finish()
    }
}

// ===== impl Builder =====

impl Builder {
    /// Returns a new server builder instance initialized with default
    /// configuration values.
    ///
    /// Configuration methods can be chained on the return value.
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .initial_window_size(1_000_000)
    ///     .max_concurrent_streams(1000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn new() -> Builder {
        Builder {
            reset_stream_duration: Duration::from_secs(proto::DEFAULT_RESET_STREAM_SECS),
            reset_stream_max: proto::DEFAULT_RESET_STREAM_MAX,
            pending_accept_reset_stream_max: proto::DEFAULT_REMOTE_RESET_STREAM_MAX,
            settings: Settings::default(),
            initial_target_connection_window_size: None,
            max_send_buffer_size: proto::DEFAULT_MAX_SEND_BUFFER_SIZE,
            local_max_error_reset_streams: Some(proto::DEFAULT_LOCAL_RESET_COUNT_MAX),
            data_frame_budget: proto::DataFrameBudget::Auto,
        }
    }

    /// Indicates the initial window size (in octets) for stream-level
    /// flow control for received data.
    ///
    /// The initial window of a stream is used as part of flow control. For more
    /// details, see [`FlowControl`].
    ///
    /// The default value is 65,535.
    ///
    /// [`FlowControl`]: ../struct.FlowControl.html
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .initial_window_size(1_000_000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn initial_window_size(&mut self, size: u32) -> &mut Self {
        self.settings.set_initial_window_size(Some(size));
        self
    }

    /// Indicates the initial window size (in octets) for connection-level flow control
    /// for received data.
    ///
    /// The initial window of a connection is used as part of flow control. For more details,
    /// see [`FlowControl`].
    ///
    /// The default value is 65,535.
    ///
    /// [`FlowControl`]: ../struct.FlowControl.html
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .initial_connection_window_size(1_000_000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn initial_connection_window_size(&mut self, size: u32) -> &mut Self {
        self.initial_target_connection_window_size = Some(size);
        self
    }

    /// Indicates the size (in octets) of the largest HTTP/2 frame payload that the
    /// configured server is able to accept.
    ///
    /// The sender may send data frames that are **smaller** than this value,
    /// but any data larger than `max` will be broken up into multiple `DATA`
    /// frames.
    ///
    /// The value **must** be between 16,384 and 16,777,215. The default value is 16,384.
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .max_frame_size(1_000_000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    ///
    /// # Panics
    ///
    /// This function panics if `max` is not within the legal range specified
    /// above.
    pub fn max_frame_size(&mut self, max: u32) -> &mut Self {
        self.settings.set_max_frame_size(Some(max));
        self
    }

    /// Sets the max size of received header frames.
    ///
    /// This advisory setting informs a peer of the maximum size of header list
    /// that the sender is prepared to accept, in octets. The value is based on
    /// the uncompressed size of header fields, including the length of the name
    /// and value in octets plus an overhead of 32 octets for each header field.
    ///
    /// This setting is also used to limit the maximum amount of data that is
    /// buffered to decode HEADERS frames.
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .max_header_list_size(16 * 1024)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn max_header_list_size(&mut self, max: u32) -> &mut Self {
        self.settings.set_max_header_list_size(Some(max));
        self
    }

    /// Sets the header table size.
    ///
    /// This setting informs the peer of the maximum size of the header compression
    /// table used to encode header blocks, in octets. The encoder may select any value
    /// equal to or less than the header table size specified by the sender.
    ///
    /// The default value is 4,096.
    pub fn header_table_size(&mut self, size: u32) -> &mut Self {
        self.settings.set_header_table_size(Some(size));
        self
    }

    /// Sets the maximum number of concurrent streams.
    ///
    /// The maximum concurrent streams setting only controls the maximum number
    /// of streams that can be initiated by the remote peer. In other words,
    /// when this setting is set to 100, this does not limit the number of
    /// concurrent streams that can be created by the caller.
    ///
    /// It is recommended that this value be no smaller than 100, so as to not
    /// unnecessarily limit parallelism. However, any value is legal, including
    /// 0. If `max` is set to 0, then the remote will not be permitted to
    /// initiate streams.
    ///
    /// Note that streams in the reserved state, i.e., push promises that have
    /// been reserved but the stream has not started, do not count against this
    /// setting.
    ///
    /// Also note that if the remote *does* exceed the value set here, it is not
    /// a protocol level error. Instead, the `h2` library will immediately reset
    /// the stream.
    ///
    /// See [Section 5.1.2] in the HTTP/2 spec for more details.
    ///
    /// [Section 5.1.2]: https://http2.github.io/http2-spec/#rfc.section.5.1.2
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .max_concurrent_streams(1000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn max_concurrent_streams(&mut self, max: u32) -> &mut Self {
        self.settings.set_max_concurrent_streams(Some(max));
        self
    }

    /// Sets the maximum number of concurrent locally reset streams.
    ///
    /// When a stream is explicitly reset by either calling
    /// [`SendResponse::send_reset`] or by dropping a [`SendResponse`] instance
    /// before completing the stream, the HTTP/2 specification requires that
    /// any further frames received for that stream must be ignored for "some
    /// time".
    ///
    /// In order to satisfy the specification, internal state must be maintained
    /// to implement the behavior. This state grows linearly with the number of
    /// streams that are locally reset.
    ///
    /// The `max_concurrent_reset_streams` setting configures sets an upper
    /// bound on the amount of state that is maintained. When this max value is
    /// reached, the oldest reset stream is purged from memory.
    ///
    /// Once the stream has been fully purged from memory, any additional frames
    /// received for that stream will result in a connection level protocol
    /// error, forcing the connection to terminate.
    ///
    /// The default value is currently 50.
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .max_concurrent_reset_streams(1000)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn max_concurrent_reset_streams(&mut self, max: usize) -> &mut Self {
        self.reset_stream_max = max;
        self
    }

    /// Sets the maximum number of local resets due to protocol errors made by the remote end.
    ///
    /// Invalid frames and many other protocol errors will lead to resets being generated for those streams.
    /// Too many of these often indicate a malicious client, and there are attacks which can abuse this to DOS servers.
    /// This limit protects against these DOS attacks by limiting the amount of resets we can be forced to generate.
    ///
    /// When the number of local resets exceeds this threshold, the server will issue GOAWAYs with an error code of
    /// `ENHANCE_YOUR_CALM` to the client.
    ///
    /// If you really want to disable this, supply [`Option::None`] here.
    /// Disabling this is not recommended and may expose you to DOS attacks.
    ///
    /// The default value is currently 1024, but could change.
    pub fn max_local_error_reset_streams(&mut self, max: Option<usize>) -> &mut Self {
        self.local_max_error_reset_streams = max;
        self
    }

    /// Sets the maximum number of pending-accept remotely-reset streams.
    ///
    /// Streams that have been received by the peer, but not accepted by the
    /// user, can also receive a RST_STREAM. This is a legitimate pattern: one
    /// could send a request and then shortly after, realize it is not needed,
    /// sending a CANCEL.
    ///
    /// However, since those streams are now "closed", they don't count towards
    /// the max concurrent streams. So, they will sit in the accept queue,
    /// using memory.
    ///
    /// When the number of remotely-reset streams sitting in the pending-accept
    /// queue reaches this maximum value, a connection error with the code of
    /// `ENHANCE_YOUR_CALM` will be sent to the peer, and returned by the
    /// `Future`.
    ///
    /// The default value is currently 20, but could change.
    ///
    /// # Examples
    ///
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .max_pending_accept_reset_streams(100)
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn max_pending_accept_reset_streams(&mut self, max: usize) -> &mut Self {
        self.pending_accept_reset_stream_max = max;
        self
    }

    /// Sets the maximum send buffer size per stream.
    ///
    /// Once a stream has buffered up to (or over) the maximum, the stream's
    /// flow control will not "poll" additional capacity. Once bytes for the
    /// stream have been written to the connection, the send buffer capacity
    /// will be freed up again.
    ///
    /// The default is currently ~400KB, but may change.
    ///
    /// # Panics
    ///
    /// This function panics if `max` is larger than `u32::MAX`.
    pub fn max_send_buffer_size(&mut self, max: usize) -> &mut Self {
        assert!(max <= u32::MAX as usize);
        self.max_send_buffer_size = max;
        self
    }

    /// Sets the maximum number of concurrent locally reset streams.
    ///
    /// When a stream is explicitly reset by either calling
    /// [`SendResponse::send_reset`] or by dropping a [`SendResponse`] instance
    /// before completing the stream, the HTTP/2 specification requires that
    /// any further frames received for that stream must be ignored for "some
    /// time".
    ///
    /// In order to satisfy the specification, internal state must be maintained
    /// to implement the behavior. This state grows linearly with the number of
    /// streams that are locally reset.
    ///
    /// The `reset_stream_duration` setting configures the max amount of time
    /// this state will be maintained in memory. Once the duration elapses, the
    /// stream state is purged from memory.
    ///
    /// Once the stream has been fully purged from memory, any additional frames
    /// received for that stream will result in a connection level protocol
    /// error, forcing the connection to terminate.
    ///
    /// The default value is currently 1 second.
    ///
    /// # Examples
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// # use std::time::Duration;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .reset_stream_duration(Duration::from_secs(10))
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn reset_stream_duration(&mut self, dur: Duration) -> &mut Self {
        self.reset_stream_duration = dur;
        self
    }

    /// Enables the [extended CONNECT protocol].
    ///
    /// [extended CONNECT protocol]: https://datatracker.ietf.org/doc/html/rfc8441#section-4
    pub fn enable_connect_protocol(&mut self) -> &mut Self {
        self.settings.set_enable_connect_protocol(Some(1));
        self
    }

    /// Sets a connection-level budget for limiting memory overhead from
    /// received small DATA frames.
    ///
    /// HTTP/2 flow control accounts for DATA payload bytes, but not the
    /// additional memory required to buffer each DATA frame. An excessive
    /// number of small frames may therefore consume disproportionate memory.
    ///
    /// Small DATA frames consume this budget. The budget is restored when
    /// buffered frames are consumed by the application, while sufficiently
    /// large frames may also restore budget. Empty DATA frames are limited
    /// separately and do not consume this budget.
    ///
    /// When this budget is exhausted, the connection is closed with
    /// `ENHANCE_YOUR_CALM`.
    ///
    /// By default, the budget is half the initial connection window, with a
    /// minimum of 25,600 bytes. Increasing the connection window therefore
    /// also increases the permitted framing overhead.
    pub fn data_frame_budget(&mut self, budget: usize) -> &mut Self {
        self.data_frame_budget = proto::DataFrameBudget::Configured(budget);
        self
    }

    /// Creates a new configured HTTP/2 server backed by `io`.
    ///
    /// It is expected that `io` already be in an appropriate state to commence
    /// the [HTTP/2 handshake]. See [Handshake] for more details.
    ///
    /// Returns a future which resolves to the [`Connection`] instance once the
    /// HTTP/2 handshake has been completed.
    ///
    /// This function also allows the caller to configure the send payload data
    /// type. See [Outbound data type] for more details.
    ///
    /// [HTTP/2 handshake]: http://httpwg.org/specs/rfc7540.html#ConnectionHeader
    /// [Handshake]: ../index.html#handshake
    /// [`Connection`]: struct.Connection.html
    /// [Outbound data type]: ../index.html#outbound-data-type.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut = Builder::new()
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    ///
    /// Configures the send-payload data type. In this case, the outbound data
    /// type will be `&'static [u8]`.
    ///
    /// ```text
    /// # use tokio::io::{AsyncRead, AsyncWrite};
    /// # use h2::server::*;
    /// #
    /// # fn doc<T: AsyncRead + AsyncWrite + Unpin>(my_io: T)
    /// # -> Handshake<T, &'static [u8]>
    /// # {
    /// // `server_fut` is a future representing the completion of the HTTP/2
    /// // handshake.
    /// let server_fut: Handshake<_, &'static [u8]> = Builder::new()
    ///     .handshake(my_io);
    /// # server_fut
    /// # }
    /// #
    /// # pub fn main() {}
    /// ```
    pub fn handshake<T, B>(&self, io: T) -> Handshake<T, B>
    where
        T: AsyncRead + AsyncWrite + Unpin,
        B: Buf,
    {
        Connection::handshake2(io, self.clone())
    }
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

// ===== impl SendResponse =====

impl<B: Buf> SendResponse<B> {
    /// Keep one terminal owner while the application consumes this handle.
    pub(crate) fn deadline_response(&self) -> Option<DeadlineResponse<B>> {
        self.inner.as_ref().map(|inner| DeadlineResponse {
            inner: inner.clone(),
        })
    }

    /// Send an interim informational response (1xx status codes)
    ///
    /// This method can be called multiple times before calling `send_response()`
    /// to send the final response. Only 1xx status codes are allowed.
    ///
    /// Interim informational responses are used to provide early feedback to the client
    /// before the final response is ready. Common examples include:
    /// - 100 Continue: Indicates the client should continue with the request
    /// - 103 Early Hints: Provides early hints about resources to preload
    ///
    /// # Arguments
    /// * `response` - HTTP response with 1xx status code and headers
    ///
    /// # Returns
    /// * `Ok(())` - Interim Informational response sent successfully
    /// * `Err(Error)` - Failed to send (invalid status code, connection error, etc.)
    ///
    /// # Examples
    /// ```text
    /// use h2::server;
    /// use http::{Response, StatusCode};
    ///
    /// # async fn example(mut send_response: h2::server::SendResponse<bytes::Bytes>) -> Result<(), h2::Error> {
    /// // Send 100 Continue before processing request body
    /// let continue_response = Response::builder()
    ///     .status(StatusCode::CONTINUE)
    ///     .body(())
    ///     .unwrap();
    /// send_response.send_informational(continue_response)?;
    ///
    /// // Later send the final response
    /// let final_response = Response::builder()
    ///     .status(StatusCode::OK)
    ///     .body(())
    ///     .unwrap();
    /// let _stream = send_response.send_response(final_response, false)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// This method will return an error if:
    /// - The response status code is not in the 1xx range
    /// - The final response has already been sent
    /// - There is a connection-level error
    pub fn send_informational(
        &mut self,
        response: Response<()>,
    ) -> Result<(), crate::h2_backend::Error> {
        let Some(inner) = self.inner.as_mut() else {
            return Err(UserError::PollResetAfterSendResponse.into());
        };
        let stream_id = inner.stream_id();
        let status = response.status();

        tracing::trace!(
            "send_informational called with status: {} on stream: {:?}",
            status,
            stream_id
        );

        // Validate that this is an informational response (1xx status code)
        if !response.status().is_informational() {
            tracing::trace!(
                "invalid informational status code: {} on stream: {:?}",
                status,
                stream_id
            );
            return Err(crate::h2_backend::Error::from(
                UserError::InvalidInformationalStatusCode,
            ));
        }

        tracing::trace!(
            "converting informational response to HEADERS frame without END_STREAM flag for stream: {:?}",
            stream_id
        );

        let frame = *Peer::convert_send_message(
            stream_id, response, false, // NOT end_of_stream for informational responses
        );

        tracing::trace!(
            "sending interim informational headers frame for stream: {:?}",
            stream_id
        );

        // Use the proper H2 streams API for sending interim informational headers
        // This bypasses the normal response flow and allows multiple informational responses
        let result = inner.send_informational_headers(frame).map_err(Into::into);

        match &result {
            Ok(()) => tracing::trace!(
                "Successfully sent informational headers for stream: {:?}",
                stream_id
            ),
            Err(e) => tracing::trace!(
                "Failed to send informational headers for stream: {:?}: {:?}",
                stream_id,
                e
            ),
        }

        result
    }

    /// Send a response to a client request.
    ///
    /// On success, a [`SendStream`] instance is returned. This instance can be
    /// used to stream the response body and send trailers.
    ///
    /// If a body or trailers will be sent on the returned [`SendStream`]
    /// instance, then `end_of_stream` must be set to `false` when calling this
    /// function.
    ///
    /// The [`SendResponse`] instance is already associated with a received
    /// request.  This function may only be called once per instance and only if
    /// [`send_reset`] has not been previously called.
    ///
    /// [`SendResponse`]: #
    /// [`SendStream`]: ../struct.SendStream.html
    /// [`send_reset`]: #method.send_reset
    pub fn send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<SendStream<B>, crate::h2_backend::Error> {
        self.inner
            .as_mut()
            .ok_or(UserError::PollResetAfterSendResponse)?
            .send_response(response, end_of_stream)
            .map_err(crate::h2_backend::Error::from)?;
        // The caller keeps this `SendResponse`. The stream ref itself moves
        // into the `SendStream`, so Drop does not release a second handle.
        let inner = self.inner.take().expect("send handle still present");
        Ok(SendStream::new(inner))
    }

    /// Stock HTTP 200. The header block is replayed, so there is no response
    /// head to copy.
    pub fn send_stock_response(
        &mut self,
        end_of_stream: bool,
    ) -> Result<SendStream<B>, crate::h2_backend::Error> {
        self.inner
            .as_mut()
            .ok_or(UserError::PollResetAfterSendResponse)?
            .send_stock_response(end_of_stream)
            .map_err(crate::h2_backend::Error::from)?;
        let inner = self.inner.take().expect("send handle still present");
        Ok(SendStream::new(inner))
    }

    /// Push a request and response to the client
    ///
    /// On success, a [`SendResponse`] instance is returned.
    ///
    /// [`SendResponse`]: #
    pub fn push_request(
        &mut self,
        request: Request<()>,
    ) -> Result<SendPushedResponse<B>, crate::h2_backend::Error> {
        self.inner
            .as_mut()
            .ok_or(UserError::PollResetAfterSendResponse)?
            .send_push_promise(request)
            .map(|inner| SendPushedResponse {
                inner: SendResponse { inner: Some(inner) },
            })
            .map_err(Into::into)
    }

    /// Send a stream reset to the peer.
    ///
    /// This essentially cancels the stream, including any inbound or outbound
    /// data streams.
    ///
    /// If this function is called before [`send_response`], a call to
    /// [`send_response`] will result in an error.
    ///
    /// If this function is called while a [`SendStream`] instance is active,
    /// any further use of the instance will result in an error.
    ///
    /// This function should only be called once.
    ///
    /// [`send_response`]: #method.send_response
    /// [`SendStream`]: ../struct.SendStream.html
    pub fn send_reset(&mut self, reason: Reason) {
        if let Some(inner) = self.inner.as_mut() {
            inner.send_reset(reason);
        }
    }

    /// Polls to be notified when the client resets this stream.
    ///
    /// If stream is still open, this returns `Poll::Pending`, and
    /// registers the task to be notified if a `RST_STREAM` is received.
    ///
    /// If a `RST_STREAM` frame is received for this stream, calling this
    /// method will yield the `Reason` for the reset.
    ///
    /// # Error
    ///
    /// Calling this method after having called `send_response` will return
    /// a user error.
    pub fn poll_reset(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Reason, crate::h2_backend::Error>> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(Err(UserError::PollResetAfterSendResponse.into()));
        };
        inner.poll_reset(cx, proto::PollReset::AwaitingHeaders)
    }

    /// Returns the stream ID of the response stream.
    ///
    /// # Panics
    ///
    /// If the lock on the stream store has been poisoned.
    pub fn stream_id(&self) -> crate::h2_backend::StreamId {
        crate::h2_backend::StreamId::from_internal(
            self.inner
                .as_ref()
                .expect("stream id after send_response")
                .stream_id(),
        )
    }
}

// ===== impl SendPushedResponse =====

impl<B: Buf> SendPushedResponse<B> {
    /// Send a response to a promised request.
    ///
    /// On success, a [`SendStream`] instance is returned. This instance can be
    /// used to stream the response body and send trailers.
    ///
    /// If a body or trailers will be sent on the returned [`SendStream`]
    /// instance, then `end_of_stream` must be set to `false` when calling this
    /// function.
    ///
    /// The [`SendPushedResponse`] instance is associated with a promised
    /// request.  This function may only be called once per instance and only if
    /// [`send_reset`] has not been previously called.
    ///
    /// [`SendPushedResponse`]: #
    /// [`SendStream`]: ../struct.SendStream.html
    /// [`send_reset`]: #method.send_reset
    pub fn send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<SendStream<B>, crate::h2_backend::Error> {
        self.inner.send_response(response, end_of_stream)
    }

    /// Send a stream reset to the peer.
    ///
    /// This essentially cancels the stream, including any inbound or outbound
    /// data streams.
    ///
    /// If this function is called before [`send_response`], a call to
    /// [`send_response`] will result in an error.
    ///
    /// If this function is called while a [`SendStream`] instance is active,
    /// any further use of the instance will result in an error.
    ///
    /// This function should only be called once.
    ///
    /// [`send_response`]: #method.send_response
    /// [`SendStream`]: ../struct.SendStream.html
    pub fn send_reset(&mut self, reason: Reason) {
        self.inner.send_reset(reason)
    }

    /// Polls to be notified when the client resets this stream.
    ///
    /// If stream is still open, this returns `Poll::Pending`, and
    /// registers the task to be notified if a `RST_STREAM` is received.
    ///
    /// If a `RST_STREAM` frame is received for this stream, calling this
    /// method will yield the `Reason` for the reset.
    ///
    /// # Error
    ///
    /// Calling this method after having called `send_response` will return
    /// a user error.
    pub fn poll_reset(
        &mut self,
        cx: &mut Context,
    ) -> Poll<Result<Reason, crate::h2_backend::Error>> {
        self.inner.poll_reset(cx)
    }

    /// Returns the stream ID of the response stream.
    ///
    /// # Panics
    ///
    /// If the lock on the stream store has been poisoned.
    pub fn stream_id(&self) -> crate::h2_backend::StreamId {
        self.inner.stream_id()
    }
}

// ===== impl Flush =====

impl<T, B: Buf> Flush<T, B> {
    fn new(codec: Codec<T, B>) -> Self {
        Flush { codec: Some(codec) }
    }
}

impl<T, B> Future for Flush<T, B>
where
    T: AsyncWrite + Unpin,
    B: Buf,
{
    type Output = Result<Codec<T, B>, crate::h2_backend::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Flush the codec
        ready!(self.codec.as_mut().unwrap().flush(cx))
            .map_err(crate::h2_backend::Error::from_io)?;

        // Return the codec
        Poll::Ready(Ok(self.codec.take().unwrap()))
    }
}

impl<T, B: Buf> ReadPreface<T, B> {
    fn new(codec: Codec<T, B>) -> Self {
        ReadPreface {
            codec: Some(codec),
            pos: 0,
        }
    }

    fn inner_mut(&mut self) -> &mut T {
        self.codec.as_mut().unwrap().get_mut()
    }
}

impl<T, B> Future for ReadPreface<T, B>
where
    T: AsyncRead + Unpin,
    B: Buf,
{
    type Output = Result<Codec<T, B>, crate::h2_backend::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut buf = [0; 24];
        let mut rem = PREFACE.len() - self.pos;

        while rem > 0 {
            let mut buf = ReadBuf::new(&mut buf[..rem]);
            ready!(Pin::new(self.inner_mut()).poll_read(cx, &mut buf))
                .map_err(crate::h2_backend::Error::from_io)?;
            let n = buf.filled().len();
            if n == 0 {
                return Poll::Ready(Err(crate::h2_backend::Error::from_io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before reading preface",
                ))));
            }

            if &PREFACE[self.pos..self.pos + n] != buf.filled() {
                proto_err!(conn: "read_preface: invalid preface");
                // TODO: Should this just write the GO_AWAY frame directly?
                return Poll::Ready(Err(Error::library_go_away(Reason::PROTOCOL_ERROR).into()));
            }

            self.pos += n;
            rem -= n; // TODO test
        }

        Poll::Ready(Ok(self.codec.take().unwrap()))
    }
}

// ===== impl Handshake =====

impl<T, B: Buf> Future for Handshake<T, B>
where
    T: AsyncRead + AsyncWrite + Unpin,
    B: Buf,
{
    type Output = Result<Connection<T, B>, crate::h2_backend::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let span = self.span.clone(); // XXX(eliza): T_T
        let _e = span.enter();
        tracing::trace!(state = ?self.state);

        loop {
            match &mut self.state {
                Handshaking::Flushing(flush) => {
                    // We're currently flushing a pending SETTINGS frame. Poll the
                    // flush future, and, if it's completed, advance our state to wait
                    // for the client preface.
                    let codec = match Pin::new(flush).poll(cx)? {
                        Poll::Pending => {
                            tracing::trace!(flush.poll = %"Pending");
                            return Poll::Pending;
                        }
                        Poll::Ready(flushed) => {
                            tracing::trace!(flush.poll = %"Ready");
                            flushed
                        }
                    };
                    self.state = Handshaking::ReadingPreface(
                        ReadPreface::new(codec).instrument(tracing::trace_span!("read_preface")),
                    );
                }
                Handshaking::ReadingPreface(read) => {
                    let codec = ready!(Pin::new(read).poll(cx)?);

                    self.state = Handshaking::Done;

                    let connection = proto::Connection::new(
                        codec,
                        Config {
                            next_stream_id: 2.into(),
                            // Server does not need to locally initiate any streams
                            initial_max_send_streams: 0,
                            max_send_buffer_size: self.builder.max_send_buffer_size,
                            reset_stream_duration: self.builder.reset_stream_duration,
                            reset_stream_max: self.builder.reset_stream_max,
                            remote_reset_stream_max: self.builder.pending_accept_reset_stream_max,
                            local_error_reset_streams_max: self
                                .builder
                                .local_max_error_reset_streams,
                            settings: self.builder.settings.clone(),
                            data_frame_budget: self
                                .builder
                                .data_frame_budget
                                .resolve(self.builder.initial_target_connection_window_size),
                        },
                    );

                    tracing::trace!("connection established!");
                    let mut c = Connection { connection };
                    if let Some(sz) = self.builder.initial_target_connection_window_size {
                        c.set_target_window_size(sz);
                    }

                    return Poll::Ready(Ok(c));
                }
                Handshaking::Done => {
                    panic!("Handshaking::poll() called again after handshaking was complete")
                }
            }
        }
    }
}

impl<T, B> fmt::Debug for Handshake<T, B>
where
    T: AsyncRead + AsyncWrite + fmt::Debug,
    B: fmt::Debug + Buf,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "server::Handshake")
    }
}

impl Peer {
    pub fn convert_send_message(
        id: StreamId,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Box<frame::Headers> {
        use http::response::Parts;

        // Extract the components of the HTTP request
        let (
            Parts {
                status, headers, ..
            },
            _,
        ) = response.into_parts();

        // `:status` is the only pseudo-header. Writing it into the allocation
        // skips the 160-byte `Pseudo::response` temporary.
        let mut frame = frame::Headers::response_boxed(id, status, headers);
        if crate::h2_backend::header_pool::take_next_stock(
            crate::h2_backend::header_pool::STOCK_RESPONSE,
        ) {
            frame.preset_stock_response();
        }

        if end_of_stream {
            frame.set_end_stream()
        }

        frame
    }

    pub fn convert_push_message(
        stream_id: StreamId,
        promised_id: StreamId,
        request: Request<()>,
    ) -> Result<frame::PushPromise, UserError> {
        use http::request::Parts;

        if let Err(e) = frame::PushPromise::validate_request(&request) {
            use PushPromiseHeaderError::*;
            match e {
                NotSafeAndCacheable => tracing::debug!(
                    ?promised_id,
                    "convert_push_message: method {} is not safe and cacheable",
                    request.method(),
                ),
                InvalidContentLength(e) => tracing::debug!(
                    ?promised_id,
                    "convert_push_message; promised request has invalid content-length {:?}",
                    e,
                ),
            }
            return Err(UserError::MalformedHeaders);
        }

        // Extract the components of the HTTP request
        let (
            Parts {
                method,
                uri,
                headers,
                ..
            },
            _,
        ) = request.into_parts();

        let pseudo = Pseudo::request(method, uri, None);

        Ok(frame::PushPromise::new(
            stream_id,
            promised_id,
            pseudo,
            headers,
        ))
    }
}

/// POST with `:scheme` http(s), `:authority`, and a non-empty `:path`.
///
/// Anything else, including a scheme or path the cache cannot parse, is
/// returned unchanged for the general builder.
///
/// Inlined so the 224-byte request is built in the caller's return slot
/// instead of being copied out of this function.
#[inline(always)]
fn simple_post_request(pseudo: &Pseudo, fields: HeaderMap) -> Result<Request<()>, HeaderMap> {
    let Some(method) = pseudo.method.as_ref() else {
        return Err(fields);
    };
    if method != Method::POST || pseudo.protocol.is_some() || pseudo.status.is_some() {
        return Err(fields);
    }
    let (Some(authority), Some(scheme), Some(path)) = (
        pseudo.authority.as_ref(),
        pseudo.scheme.as_ref(),
        pseudo.path.as_ref(),
    ) else {
        return Err(fields);
    };
    if path.is_empty() {
        return Err(fields);
    }
    let Ok(authority) = cached_authority(authority) else {
        return Err(fields);
    };
    let scheme = match scheme.as_str() {
        "https" => http::uri::Scheme::HTTPS,
        "http" => http::uri::Scheme::HTTP,
        _ => return Err(fields),
    };
    let Ok(path) = cached_path_and_query(path) else {
        return Err(fields);
    };
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(scheme);
    parts.authority = Some(authority);
    parts.path_and_query = Some(path);
    let Ok(uri) = http::uri::Uri::from_parts(parts) else {
        return Err(fields);
    };
    let mut request = Request::new(());
    *request.method_mut() = Method::POST;
    // Same static `Uri::default()` as the client stamp. Nothing to free.
    let _default_uri = std::mem::ManuallyDrop::new(std::mem::replace(request.uri_mut(), uri));
    *request.version_mut() = http::Version::HTTP_2;
    // A stock request carries its headers in the shared map. `fields` is the
    // empty map `Request::new` already installed, so swapping them is a copy.
    if fields.capacity() == 0 {
        let _empty = std::mem::ManuallyDrop::new(fields);
    } else {
        crate::h2_backend::header_pool::replace_map(request.headers_mut(), fields);
    }
    Ok(request)
}

/// `Some(https)` when this is a cache-hit POST with an empty header map.
///
/// The request itself is built later, in the accept slot, so the 224-byte
/// value is not copied into the receive queue.
pub(crate) fn stock_post_https(pseudo: &Pseudo, fields: &HeaderMap) -> Option<bool> {
    if fields.capacity() != 0 || pseudo.protocol.is_some() || pseudo.status.is_some() {
        return None;
    }
    stock_https_of(
        pseudo.method.as_ref(),
        pseudo.scheme.as_ref(),
        pseudo.authority.as_ref(),
        pseudo.path.as_ref(),
    )
}

/// Scheme of a saved stock POST, before its pseudo-headers are cloned.
pub(crate) fn stock_https_of(
    method: Option<&Method>,
    scheme: Option<&BytesStr>,
    authority: Option<&BytesStr>,
    path: Option<&BytesStr>,
) -> Option<bool> {
    let method = method?;
    if method != Method::POST {
        return None;
    }
    let (scheme, authority, path) = (scheme?, authority?, path?);
    if path.is_empty() || !authority_cached(authority) || !path_cached(path) {
        return None;
    }
    match scheme.as_str() {
        "https" => Some(true),
        "http" => Some(false),
        _ => None,
    }
}

/// POST built from the process-wide authority and path cache.
static HTTP_POST: std::sync::OnceLock<Request<()>> = std::sync::OnceLock::new();

/// Authority text leaked for the plain HTTP stock request.
static LEAKED_AUTHORITY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Path text leaked for the plain HTTP stock request.
static LEAKED_PATH: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Empty HTTP 200. Stock responses copy this instead of building it.
static OK_RESPONSE: std::sync::OnceLock<http::Response<()>> = std::sync::OnceLock::new();

#[allow(
    unsafe_code,
    reason = "a stock OK response holds an empty map and no extensions"
)]
pub(crate) fn copy_stock_ok_response() -> http::Response<()> {
    let proto = OK_RESPONSE.get_or_init(|| {
        let mut res = http::Response::new(());
        *res.status_mut() = http::StatusCode::OK;
        res
    });
    // SAFETY: status and version are `Copy`. The header map is empty and
    // extensions are absent, so a bitwise copy does not alias a heap buffer.
    unsafe { std::ptr::read(proto) }
}

thread_local! {
    // 0 = nothing popped, 1 = stock http, 2 = stock https.
    static POPPED_STOCK: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

pub(crate) fn stash_popped_stock(https: Option<bool>) {
    POPPED_STOCK.set(match https {
        Some(false) => 1,
        Some(true) => 2,
        None => 0,
    });
}

pub(crate) fn take_popped_stock() -> Option<bool> {
    match POPPED_STOCK.replace(0) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// The accept poll maps this head immediately. Keeping the copy out of line
/// wrote it into a return slot and then copied it again.
#[inline(always)]
pub(crate) fn cached_simple_post(https: bool) -> Request<()> {
    if !https {
        if let Some(cached) = HTTP_POST.get() {
            return copy_static_request(cached);
        }
    }
    cached_simple_post_miss(https)
}

#[cold]
#[inline(never)]
fn cached_simple_post_miss(https: bool) -> Request<()> {
    // `stock_post_https` only returns after both caches are filled.
    let (Some(authority), Some(path)) = (auth_cache::PARSED.get(), path_cache::PARSED.get()) else {
        return Request::new(());
    };
    let scheme = if https {
        http::uri::Scheme::HTTPS
    } else {
        http::uri::Scheme::HTTP
    };
    // The plain `http` request is copied bitwise after the first build.
    // That copy is sound only when every URI byte is static.
    let authority = if https {
        authority.clone()
    } else {
        let leaked: &'static str = Box::leak(authority.as_str().to_owned().into_boxed_str());
        let _ = LEAKED_AUTHORITY.set(leaked);
        http::uri::Authority::from_static(leaked)
    };
    let path = if https {
        path.clone()
    } else {
        let leaked: &'static str = Box::leak(path.as_str().to_owned().into_boxed_str());
        let _ = LEAKED_PATH.set(leaked);
        http::uri::PathAndQuery::from_static(leaked)
    };
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(scheme);
    parts.authority = Some(authority);
    parts.path_and_query = Some(path);
    let Ok(uri) = http::uri::Uri::from_parts(parts) else {
        return Request::new(());
    };
    let mut request = Request::new(());
    *request.method_mut() = Method::POST;
    let _default_uri = std::mem::ManuallyDrop::new(std::mem::replace(request.uri_mut(), uri));
    *request.version_mut() = http::Version::HTTP_2;
    if !https {
        let _ = HTTP_POST.set(copy_static_request(&request));
    }
    request
}

/// `Some` when `s` is the leaked plain-HTTP authority.
pub(crate) fn leaked_authority(s: &str) -> Option<&'static str> {
    leaked_text(s, LEAKED_AUTHORITY.get().copied())
}

/// `Some` when `s` is the leaked plain-HTTP path.
pub(crate) fn leaked_path(s: &str) -> Option<&'static str> {
    leaked_text(s, LEAKED_PATH.get().copied())
}

fn leaked_text(s: &str, leaked: Option<&'static str>) -> Option<&'static str> {
    let leaked = leaked?;
    if s.len() == leaked.len() && (s.as_ptr() == leaked.as_ptr() || s == leaked) {
        Some(leaked)
    } else {
        None
    }
}

#[allow(
    unsafe_code,
    reason = "a stock http request holds only static URI bytes and an empty map"
)]
fn copy_static_request(request: &Request<()>) -> Request<()> {
    // SAFETY: the cached request is `Method::POST`, HTTP/2, an empty header
    // map, and a URI built from `Authority::from_static` and
    // `PathAndQuery::from_static`. Dropping a bitwise copy does not free
    // those bytes or a header allocation.
    unsafe { std::ptr::read(request) }
}

fn authority_cached(raw: &BytesStr) -> bool {
    auth_cache::PARSED
        .get()
        .is_some_and(|parsed| parsed.as_str().as_bytes() == raw.as_ref())
}

fn path_cached(raw: &BytesStr) -> bool {
    path_cache::PARSED
        .get()
        .is_some_and(|parsed| parsed.as_str().as_bytes() == raw.as_ref())
}

fn cached_authority(raw: &BytesStr) -> Result<http::uri::Authority, http::uri::InvalidUri> {
    if let Some(parsed) = auth_cache::PARSED.get() {
        if parsed.as_str().as_bytes() == raw.as_ref() {
            return Ok(parsed.clone());
        }
    }
    cached_authority_miss(raw)
}

/// Worker slot for an authority that is not the process-wide one.
#[cold]
#[inline(never)]
fn cached_authority_miss(raw: &BytesStr) -> Result<http::uri::Authority, http::uri::InvalidUri> {
    use std::cell::RefCell;
    thread_local! {
        static LAST: RefCell<Option<(Bytes, http::uri::Authority)>> =
            const { RefCell::new(None) };
    }
    LAST.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((previous, parsed)) = slot.as_ref() {
            if previous.as_ref() == raw.as_ref() {
                return Ok(parsed.clone());
            }
        }
        let bytes = raw.clone().into_inner();
        let parsed = http::uri::Authority::from_maybe_shared(bytes.clone())?;
        *slot = Some((bytes, parsed.clone()));
        let _ = auth_cache::PARSED.set(parsed.clone());
        Ok(parsed)
    })
}

mod auth_cache {
    pub(super) static PARSED: std::sync::OnceLock<http::uri::Authority> =
        std::sync::OnceLock::new();
}

fn cached_path_and_query(raw: &BytesStr) -> Result<http::uri::PathAndQuery, http::uri::InvalidUri> {
    // Decoded buffers are released between calls and their addresses can be
    // reused for a different method. Only the live text identifies a hit.
    if let Some(parsed) = path_cache::PARSED.get() {
        if parsed.as_str().as_bytes() == raw.as_ref() {
            return Ok(parsed.clone());
        }
    }
    cached_path_and_query_miss(raw)
}

#[cold]
#[inline(never)]
fn cached_path_and_query_miss(
    raw: &BytesStr,
) -> Result<http::uri::PathAndQuery, http::uri::InvalidUri> {
    use std::cell::RefCell;
    thread_local! {
        static LAST: RefCell<Option<(Bytes, http::uri::PathAndQuery)>> =
            const { RefCell::new(None) };
    }
    LAST.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((previous, parsed)) = slot.as_ref() {
            if previous.as_ref() == raw.as_ref() {
                return Ok(parsed.clone());
            }
        }
        let bytes = raw.clone().into_inner();
        let parsed = http::uri::PathAndQuery::from_maybe_shared(bytes.clone())?;
        *slot = Some((bytes, parsed.clone()));
        let _ = path_cache::PARSED.set(parsed.clone());
        Ok(parsed)
    })
}

mod path_cache {
    pub(super) static PARSED: std::sync::OnceLock<http::uri::PathAndQuery> =
        std::sync::OnceLock::new();
}

impl proto::Peer for Peer {
    type Poll = Request<()>;

    const NAME: &'static str = "Server";

    /*
    fn is_server() -> bool {
        true
    }
    */

    #[inline]
    fn r#dyn() -> proto::DynPeer {
        proto::DynPeer::Server
    }

    #[inline(always)]
    fn convert_poll_message(
        pseudo: &Pseudo,
        fields: HeaderMap,
        stream_id: StreamId,
    ) -> Result<Self::Poll, Error> {
        let fields = match simple_post_request(pseudo, fields) {
            Ok(request) => return Ok(request),
            Err(fields) => fields,
        };
        convert_poll_message_general(pseudo, fields, stream_id)
    }
}

#[cold]
#[inline(never)]
fn convert_poll_message_general(
    pseudo: &Pseudo,
    fields: HeaderMap,
    stream_id: StreamId,
) -> Result<Request<()>, Error> {
    use http::{Version, uri};

    macro_rules! malformed {
        ($($arg:tt)*) => {{
            tracing::debug!($($arg)*);
            return Err(Error::library_reset(stream_id, Reason::PROTOCOL_ERROR));
        }}
    }

    let is_connect;
    let method = if let Some(method) = pseudo.method.clone() {
        is_connect = method == Method::CONNECT;
        method
    } else {
        malformed!("malformed headers: missing method");
    };

    let has_protocol = pseudo.protocol.is_some();
    if has_protocol && !is_connect {
        malformed!("malformed headers: :protocol on non-CONNECT request");
    }

    if pseudo.status.is_some() {
        malformed!("malformed headers: :status field on request");
    }

    // Convert the URI
    let mut parts = uri::Parts::default();

    // A request translated from HTTP/1 must not include the :authority
    // header
    if let Some(authority) = pseudo.authority.as_ref() {
        let maybe_authority = cached_authority(authority);
        parts.authority = Some(maybe_authority.or_else(|why| {
            malformed!(
                "malformed headers: malformed authority ({:?}): {}",
                authority,
                why,
            )
        })?);
    }

    // A :scheme is required, except CONNECT.
    if let Some(scheme) = pseudo.scheme.as_ref() {
        if is_connect && !has_protocol {
            malformed!("malformed headers: :scheme in CONNECT");
        }
        let maybe_scheme = scheme.parse();
        let scheme = maybe_scheme.or_else(|why| {
            malformed!(
                "malformed headers: malformed scheme ({:?}): {}",
                scheme,
                why,
            )
        })?;

        // It's not possible to build an `Uri` from a scheme and path. So,
        // after validating is was a valid scheme, we just have to drop it
        // if there isn't an :authority.
        if parts.authority.is_some() {
            parts.scheme = Some(scheme);
        }
    } else if !is_connect || has_protocol {
        malformed!("malformed headers: missing scheme");
    }

    if let Some(path) = pseudo.path.as_ref() {
        if is_connect && !has_protocol {
            malformed!("malformed headers: :path in CONNECT");
        }

        // This cannot be empty
        if path.is_empty() {
            malformed!("malformed headers: missing path");
        }

        let maybe_path = cached_path_and_query(path);
        parts.path_and_query = Some(maybe_path.or_else(|why| {
            malformed!("malformed headers: malformed path ({:?}): {}", path, why,)
        })?);
    } else if is_connect && has_protocol {
        malformed!("malformed headers: missing path in extended CONNECT");
    }

    let uri = match uri::Uri::from_parts(parts) {
        Ok(uri) => uri,
        Err(e) => {
            proto_err!(stream: "error building request: {}; stream={:?}", e, stream_id);
            return Err(Error::library_reset(stream_id, Reason::PROTOCOL_ERROR));
        }
    };
    // The builder's method/uri/version closures were copies on every
    // request. The headers map is the one HPACK already decoded.
    let mut request = Request::new(());
    *request.method_mut() = method;
    *request.uri_mut() = uri;
    *request.version_mut() = Version::HTTP_2;
    if let Some(protocol) = pseudo.protocol.clone() {
        request.extensions_mut().insert(protocol);
    }
    crate::h2_backend::header_pool::replace_map(request.headers_mut(), fields);

    Ok(request)
}

// ===== impl Handshaking =====

impl<T, B> fmt::Debug for Handshaking<T, B>
where
    B: Buf,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Handshaking::Flushing(_) => f.write_str("Flushing(_)"),
            Handshaking::ReadingPreface(_) => f.write_str("ReadingPreface(_)"),
            Handshaking::Done => f.write_str("Done"),
        }
    }
}

#[cfg(test)]
mod cache_tests {
    #![allow(clippy::expect_used, reason = "request path cache regression")]
    use super::*;

    #[test]
    fn reused_path_buffer_does_not_select_cached_method() {
        let original = BytesStr::from_static("/cache/first");
        assert_eq!(
            cached_path_and_query(&original).expect("path").as_str(),
            "/cache/first"
        );
        let mut buffer = bytes::BytesMut::from(original.as_str().as_bytes());
        let repeated = BytesStr::try_from(buffer.split().freeze()).expect("UTF-8");
        let address = repeated.as_ref().as_ptr();
        assert_eq!(
            cached_path_and_query(&repeated).expect("path").as_str(),
            original.as_str()
        );
        drop(repeated);
        assert!(buffer.try_reclaim(original.len()));
        buffer.extend_from_slice(b"/cache/other");
        let changed = BytesStr::try_from(buffer.freeze()).expect("UTF-8");
        assert_eq!(
            changed.as_ref().as_ptr(),
            address,
            "reuse the same allocation"
        );
        assert!(
            !path_cached(&changed),
            "a recycled address does not identify a method"
        );
        assert_eq!(
            cached_path_and_query(&changed).expect("path").as_str(),
            "/cache/other"
        );
    }
}
