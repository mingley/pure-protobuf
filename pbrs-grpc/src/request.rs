//! RPC envelopes: [`Request`], [`Response`], and the cancellable [`Call`].

use crate::metadata::Metadata;
use crate::status::Status;
use crate::tls::PeerIdentity;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::watch;

/// A message plus the metadata, deadline, and compression choice around it.
///
/// The same type is used to build an outbound request and to read an inbound
/// one, so a proxy can forward what it received. Server interceptors mutate
/// inbound metadata through [`crate::Rpc::metadata_mut`] and attach typed
/// values through [`crate::Rpc::extensions_mut`]; the handler reads both
/// from this type.
///
/// ```
/// use pbrs_grpc::Request;
/// use std::time::Duration;
///
/// let mut req = Request::new("payload");
/// req.set_timeout(Duration::from_secs(5));
/// req.metadata_mut().insert("x-tenant", "acme")?;
/// assert_eq!(req.timeout(), Some(Duration::from_secs(5)));
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
pub struct Request<T> {
    message: T,
    metadata: Metadata,
    timeout: Option<Duration>,
    compress: bool,
    compressed: bool,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    peer_identity: Option<PeerIdentity>,
    wait_for_ready: Option<bool>,
    extensions: http::Extensions,
}

impl<T> Request<T> {
    /// Wrap a message with no metadata and no deadline.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self {
            message,
            metadata: Metadata::new(),
            timeout: None,
            compress: false,
            compressed: false,
            remote_addr: None,
            local_addr: None,
            peer_identity: None,
            wait_for_ready: None,
            extensions: http::Extensions::new(),
        }
    }

    /// Take the message, discarding the envelope.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }

    /// Borrow the message.
    #[must_use]
    pub fn get_ref(&self) -> &T {
        &self.message
    }

    /// Borrow the message mutably.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.message
    }

    /// Split into message and envelope, keeping metadata, deadline, and
    /// compression choice.
    #[must_use]
    pub fn into_message_and_parts(self) -> (T, Parts) {
        (
            self.message,
            Parts {
                metadata: self.metadata,
                timeout: self.timeout,
                compress: self.compress,
                compressed: self.compressed,
                remote_addr: self.remote_addr,
                local_addr: self.local_addr,
                peer_identity: self.peer_identity,
                wait_for_ready: self.wait_for_ready,
                extensions: self.extensions,
            },
        )
    }

    /// Rebuild a request around a different message, keeping the envelope.
    ///
    /// This is how a proxy or interceptor rewrites a payload without losing
    /// the caller's metadata, deadline, or gzip choice.
    #[must_use]
    pub fn from_message_and_parts<U>(message: U, parts: Parts) -> Request<U> {
        Request {
            message,
            metadata: parts.metadata,
            timeout: parts.timeout,
            compress: parts.compress,
            compressed: parts.compressed,
            remote_addr: parts.remote_addr,
            local_addr: parts.local_addr,
            peer_identity: parts.peer_identity,
            wait_for_ready: parts.wait_for_ready,
            extensions: parts.extensions,
        }
    }

    /// Replace the message, keeping metadata, deadline, compression, and
    /// extensions.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Request<U> {
        let (message, parts) = self.into_message_and_parts();
        Request::<U>::from_message_and_parts(f(message), parts)
    }

    /// Request headers, as gRPC metadata.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable request headers.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Set the deadline. Outbound this becomes `grpc-timeout`; inbound it
    /// reports the effective deadline (client, server cap, interceptor).
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Clear a deadline previously set on this request.
    pub fn clear_timeout(&mut self) {
        self.timeout = None;
    }

    /// The deadline, if any.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Queue this RPC until the channel is connected instead of failing
    /// immediately with [`crate::Code::Unavailable`].
    ///
    /// Pair this with a deadline. Without one, a lazy channel whose
    /// peer never comes up waits until cancellation. The usual source
    /// of a not-yet-connected channel is [`crate::Channel::connect_lazy`].
    /// [`crate::Channel::wait_for_ready`] fills this in when the request
    /// omits it; passing `false` here opts out of that default.
    pub fn set_wait_for_ready(&mut self, wait: bool) {
        self.wait_for_ready = Some(wait);
    }

    /// Drop a wait-for-ready choice so a later [`crate::Channel::wait_for_ready`]
    /// or interceptor can fill it in. See [`Self::clear_timeout`].
    pub fn clear_wait_for_ready(&mut self) {
        self.wait_for_ready = None;
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    ///
    /// `false` when unset; the channel default is applied in
    /// [`crate::Channel`] before interceptors run.
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    pub(crate) fn wait_for_ready_is_set(&self) -> bool {
        self.wait_for_ready.is_some()
    }

    /// Typed values an interceptor attached to this RPC.
    ///
    /// Empty on a request you built yourself until something inserts into
    /// [`Self::extensions_mut`]. On the server, this is the map an
    /// [`crate::Interceptor`] filled on the [`crate::Rpc`] before the
    /// handler ran.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values for later handlers or interceptors.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// gzip this request's payload and set the Compressed-Flag.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// Whether this request's payload will be gzipped. Outbound only.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress
    }

    /// Whether the received frame had the Compressed-Flag set.
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compressed
    }

    /// Peer address, when the transport exposed one. Server side only.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    ///
    /// TCP fills this from the accepted socket. Unix, in-process, and
    /// [`crate::Incoming`] paths yield `None`. See [`crate::Rpc::local_addr`].
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    ///
    /// Same value as [`crate::Rpc::peer_identity`]. TLS without a client
    /// certificate, h2c, Unix, and in-process connections yield `None`.
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.peer_identity.as_ref()
    }

    pub(crate) fn into_parts(self) -> (T, Metadata, Option<Duration>, bool) {
        (self.message, self.metadata, self.timeout, self.compress)
    }

    pub(crate) fn from_metadata(
        message: T,
        metadata: Metadata,
        remote_addr: Option<SocketAddr>,
        local_addr: Option<SocketAddr>,
        peer_identity: Option<PeerIdentity>,
    ) -> Self {
        Self {
            message,
            metadata,
            timeout: None,
            compress: false,
            compressed: false,
            remote_addr,
            local_addr,
            peer_identity,
            wait_for_ready: None,
            extensions: http::Extensions::new(),
        }
    }

    pub(crate) fn set_compressed(&mut self, compressed: bool) {
        self.compressed = compressed;
    }

    pub(crate) fn with_extensions(mut self, extensions: http::Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    pub(crate) fn outgoing<'a>(
        &'a mut self,
        path: &'static str,
        authority: &'a str,
    ) -> Outgoing<'a> {
        Outgoing {
            path,
            authority,
            metadata: &mut self.metadata,
            timeout: &mut self.timeout,
            wait_for_ready: &mut self.wait_for_ready,
            compress: &mut self.compress,
            extensions: &mut self.extensions,
        }
    }
}

/// The outbound half of an RPC, as a [`crate::ClientInterceptor`] sees it.
///
/// The request message is not here: interceptors run after the caller has
/// already built it, and object-safe interceptors cannot be generic over it.
/// Everything else an interceptor typically stamps — metadata, deadline,
/// wait-for-ready, compression, typed extensions — is. So is the channel's
/// `:authority`, which the interceptor cannot otherwise see. Typed values the
/// caller inserted on [`crate::Request::extensions_mut`] are on this map.
///
/// ```
/// use pbrs_grpc::{Outgoing, Status};
/// use std::time::Duration;
///
/// fn stamp(call: &mut Outgoing<'_>) -> Result<(), Status> {
///     let path = call.path();
///     call.metadata_mut().insert("x-path", path)?;
///     let authority = call.authority();
///     call.metadata_mut().insert("x-authority", authority)?;
///     if call.timeout().is_none() {
///         call.set_timeout(Duration::from_secs(5));
///     }
///     Ok(())
/// }
/// # let _ = stamp;
/// ```
pub struct Outgoing<'a> {
    path: &'static str,
    authority: &'a str,
    metadata: &'a mut Metadata,
    timeout: &'a mut Option<Duration>,
    wait_for_ready: &'a mut Option<bool>,
    compress: &'a mut bool,
    extensions: &'a mut http::Extensions,
}

impl<'a> Outgoing<'a> {
    /// The HTTP/2 `:authority` this channel sends, e.g. `127.0.0.1:50051`
    /// or `localhost` on a Unix socket.
    #[must_use]
    pub fn authority(&self) -> &'a str {
        self.authority
    }

    /// The full gRPC path, `/<package>.<Service>/<Method>`.
    #[must_use]
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// Request headers, as gRPC metadata.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        self.metadata
    }

    /// Mutable request headers.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        self.metadata
    }

    /// The deadline, if any.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        *self.timeout
    }

    /// Set the deadline. Becomes `grpc-timeout` on the wire.
    pub fn set_timeout(&mut self, timeout: Duration) {
        *self.timeout = Some(timeout);
    }

    /// Clear a deadline previously set on the request or by an earlier
    /// interceptor.
    pub fn clear_timeout(&mut self) {
        *self.timeout = None;
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    /// Queue this RPC until the channel is connected.
    pub fn set_wait_for_ready(&mut self, wait: bool) {
        *self.wait_for_ready = Some(wait);
    }

    /// Drop a wait-for-ready choice so a later interceptor or channel
    /// default can fill it in.
    pub fn clear_wait_for_ready(&mut self) {
        *self.wait_for_ready = None;
    }

    /// Whether the request payload will be gzipped.
    #[must_use]
    pub fn compress(&self) -> bool {
        *self.compress
    }

    /// gzip this request's payload and set the Compressed-Flag.
    pub fn set_compress(&mut self, compress: bool) {
        *self.compress = compress;
    }

    /// Typed values earlier interceptors or the caller attached to this RPC.
    ///
    /// The caller inserts on [`crate::Request::extensions_mut`] before the
    /// call; stacked interceptors share the same map. These values are not
    /// sent on the wire.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        self.extensions
    }

    /// Insert typed values for later interceptors.
    ///
    /// Use this to pass a parsed identity or span into the next interceptor
    /// without a metadata round-trip.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        self.extensions
    }
}

impl fmt::Debug for Outgoing<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outgoing")
            .field("path", &self.path)
            .field("authority", &self.authority)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("wait_for_ready", &self.wait_for_ready)
            .field("compress", &self.compress)
            .finish_non_exhaustive()
    }
}

impl<T: fmt::Debug> fmt::Debug for Request<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("message", &self.message)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("compressed", &self.compressed)
            .field("remote_addr", &self.remote_addr)
            .field("local_addr", &self.local_addr)
            .field("peer_identity", &self.peer_identity)
            .field("wait_for_ready", &self.wait_for_ready)
            .finish_non_exhaustive()
    }
}

/// A [`Request`] envelope without its message. See
/// [`Request::into_message_and_parts`].
#[derive(Debug)]
pub struct Parts {
    metadata: Metadata,
    timeout: Option<Duration>,
    compress: bool,
    compressed: bool,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    peer_identity: Option<PeerIdentity>,
    wait_for_ready: Option<bool>,
    extensions: http::Extensions,
}

impl Parts {
    /// Request headers.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable request headers.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// The deadline, if any.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Whether the payload will be gzipped. Outbound only.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    /// Typed values an interceptor attached to this RPC.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values for later handlers or interceptors.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// Peer address, when the transport exposed one.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    /// See [`Request::local_addr`].
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    /// See [`Request::peer_identity`].
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.peer_identity.as_ref()
    }
}

/// A reply: message, initial headers, and trailing metadata.
///
/// Trailing metadata set here survives on the OK path; to attach metadata to
/// an error, put it on the [`Status`] instead.
///
/// ```
/// use pbrs_grpc::Response;
///
/// let mut resp = Response::new(42);
/// resp.metadata_mut().insert("x-cache", "miss")?;
/// resp.trailers_mut().insert("x-rows-scanned", "17")?;
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
pub struct Response<T> {
    message: T,
    metadata: Metadata,
    trailers: Metadata,
    compress: bool,
}

impl<T> Response<T> {
    /// Wrap a message with no headers and no trailers.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self {
            message,
            metadata: Metadata::new(),
            trailers: Metadata::new(),
            compress: false,
        }
    }

    /// Take the message.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }

    /// Borrow the message.
    #[must_use]
    pub fn get_ref(&self) -> &T {
        &self.message
    }

    /// Borrow the message mutably.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.message
    }

    /// Replace the message, keeping headers and trailers.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Response<U> {
        Response {
            message: f(self.message),
            metadata: self.metadata,
            trailers: self.trailers,
            compress: self.compress,
        }
    }

    /// Initial headers, sent before the first message.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable initial headers.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Trailing metadata, sent alongside `grpc-status`.
    #[must_use]
    pub fn trailers(&self) -> &Metadata {
        &self.trailers
    }

    /// Mutable trailing metadata.
    pub fn trailers_mut(&mut self) -> &mut Metadata {
        &mut self.trailers
    }

    /// gzip this payload and set the Compressed-Flag.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// Whether this payload is gzipped.
    ///
    /// On a response you build, this is [`Self::set_compress`]. On a received
    /// unary response, it is the Compressed-Flag from the wire. Streaming
    /// payloads report the flag on each [`crate::Framed`] instead.
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compress
    }

    pub(crate) fn from_parts(message: T, metadata: Metadata, trailers: Metadata) -> Self {
        Self {
            message,
            metadata,
            trailers,
            compress: false,
        }
    }

    pub(crate) fn from_parts_compress(
        message: T,
        metadata: Metadata,
        trailers: Metadata,
        compress: bool,
    ) -> Self {
        Self {
            message,
            metadata,
            trailers,
            compress,
        }
    }

    pub(crate) fn split(self) -> (T, Metadata, Metadata, bool) {
        (self.message, self.metadata, self.trailers, self.compress)
    }
}

impl<T: fmt::Debug> fmt::Debug for Response<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("message", &self.message)
            .field("metadata", &self.metadata)
            .field("trailers", &self.trailers)
            .field("compress", &self.compress)
            .finish()
    }
}

/// An RPC in flight.
///
/// Await it for the result. Dropping it without awaiting resets the HTTP/2
/// stream so the server drops the handler, the same as [`Self::cancel`].
/// Cancel while you still hold the future if you need the await to resolve
/// with [`Code::Cancelled`](crate::Code::Cancelled) rather than being dropped.
#[must_use = "an RPC does nothing until awaited"]
pub struct Call<T> {
    fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,
    cancel: watch::Sender<bool>,
    /// Set when [`Future::poll`] returns `Ready`, so [`Drop`] does not RST a
    /// finished RPC.
    done: bool,
}

impl<T> Call<T> {
    pub(crate) fn new(
        cancel: watch::Sender<bool>,
        fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,
    ) -> Self {
        Self {
            fut,
            cancel,
            done: false,
        }
    }

    /// Reset the stream and resolve with [`Code::Cancelled`](crate::Code::Cancelled).
    pub fn cancel(&self) {
        self.cancel.send(true).ok();
    }

    /// A cancel handle that can be moved to another task.
    ///
    /// ```no_run
    /// # async fn demo(call: pbrs_grpc::Call<u32>) {
    /// let handle = call.handle();
    /// tokio::spawn(async move {
    ///     tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    ///     handle.cancel();
    /// });
    /// let _ = call.await;
    /// # }
    /// ```
    #[must_use]
    pub fn handle(&self) -> CallHandle {
        CallHandle {
            cancel: self.cancel.clone(),
        }
    }
}

impl<T> Future for Call<T> {
    type Output = Result<T, Status>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let poll = self.fut.as_mut().poll(cx);
        if poll.is_ready() {
            self.done = true;
        }
        poll
    }
}

impl<T> Drop for Call<T> {
    fn drop(&mut self) {
        if !self.done {
            // Wakes a client-streaming pump that still holds the send half;
            // unary RSTs when the boxed future (and its `SendStream`) drops.
            self.cancel.send(true).ok();
        }
    }
}

impl<T> fmt::Debug for Call<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("cancelled", &*self.cancel.borrow())
            .finish_non_exhaustive()
    }
}

/// A cancel signal for an in-flight [`Call`], detached from the call itself.
#[derive(Clone, Debug)]
pub struct CallHandle {
    cancel: watch::Sender<bool>,
}

impl CallHandle {
    /// Same as [`Call::cancel`].
    pub fn cancel(&self) {
        self.cancel.send(true).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{Request, Response};
    use std::time::Duration;

    #[test]
    fn envelope_survives_a_message_swap() {
        let mut req = Request::new(1u32);
        req.set_timeout(Duration::from_millis(7));
        req.set_wait_for_ready(true);
        req.set_compress(true);
        req.extensions_mut().insert(7u8);
        req.metadata_mut().insert("k", "v").expect("insert");
        let (message, parts) = req.into_message_and_parts();
        assert_eq!(message, 1);
        assert!(parts.wait_for_ready());
        assert!(parts.compress());
        assert_eq!(parts.extensions().get::<u8>().copied(), Some(7));
        let rebuilt = Request::<u32>::from_message_and_parts("swapped", parts);
        assert_eq!(rebuilt.timeout(), Some(Duration::from_millis(7)));
        assert_eq!(rebuilt.metadata().get("k"), Some("v"));
        assert!(rebuilt.wait_for_ready());
        assert!(rebuilt.compress());
        assert_eq!(rebuilt.extensions().get::<u8>().copied(), Some(7));
        assert_eq!(rebuilt.into_inner(), "swapped");
        let mut cleared = Request::new(0u32);
        cleared.set_timeout(Duration::from_secs(1));
        cleared.clear_timeout();
        assert_eq!(cleared.timeout(), None);
        let mut inherit = Request::new(0u32);
        inherit.set_wait_for_ready(false);
        inherit.clear_wait_for_ready();
        assert!(!inherit.wait_for_ready());
        assert!(!inherit.wait_for_ready_is_set());
    }

    #[test]
    fn request_map_keeps_metadata() {
        let mut req = Request::new(1u32);
        req.set_timeout(Duration::from_millis(7));
        req.set_compress(true);
        req.metadata_mut().insert("k", "v").expect("insert");
        let mapped = req.map(|n| n + 1);
        assert_eq!(mapped.timeout(), Some(Duration::from_millis(7)));
        assert_eq!(mapped.metadata().get("k"), Some("v"));
        assert!(mapped.compress());
        assert_eq!(mapped.into_inner(), 2);
    }

    #[test]
    fn response_map_keeps_metadata() {
        let mut resp = Response::new(2u32);
        resp.trailers_mut().insert("t", "1").expect("insert");
        let mapped = resp.map(|n| n * 21);
        assert_eq!(mapped.trailers().get("t"), Some("1"));
        assert_eq!(mapped.into_inner(), 42);
    }

    #[test]
    fn outgoing_debug_names_path_authority_and_user_metadata() {
        let mut req = Request::new(());
        req.metadata_mut().insert("x-trace", "abc").expect("insert");
        req.set_timeout(Duration::from_secs(1));
        let call = req.outgoing("/svc/Method", "127.0.0.1:1");
        let shown = format!("{call:?}");
        assert!(shown.contains("/svc/Method"), "{shown}");
        assert!(shown.contains("127.0.0.1:1"), "{shown}");
        assert!(shown.contains("x-trace"), "{shown}");
        assert!(shown.contains("abc"), "{shown}");
    }
}
