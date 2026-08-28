//! RPC envelopes: [`Request`], [`Response`], and the cancellable [`Call`].

use crate::metadata::Metadata;
use crate::status::Status;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::watch;

/// A message plus the metadata, deadline, and compression choice around it.
///
/// The same type is used to build an outbound request and to read an inbound
/// one, so a proxy can forward what it received.
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

    /// Split into message and envelope, keeping metadata and deadline.
    #[must_use]
    pub fn into_message_and_parts(self) -> (T, Parts) {
        (
            self.message,
            Parts {
                metadata: self.metadata,
                timeout: self.timeout,
                compressed: self.compressed,
                remote_addr: self.remote_addr,
            },
        )
    }

    /// Rebuild a request around a different message, keeping the envelope.
    ///
    /// This is how a proxy or interceptor rewrites a payload without losing
    /// the caller's metadata or deadline.
    #[must_use]
    pub fn from_message_and_parts<U>(message: U, parts: Parts) -> Request<U> {
        Request {
            message,
            metadata: parts.metadata,
            timeout: parts.timeout,
            compress: false,
            compressed: parts.compressed,
            remote_addr: parts.remote_addr,
        }
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
    /// reports the deadline the peer asked for.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// The deadline, if any.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// gzip this request's payload and set the Compressed-Flag.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
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

    pub(crate) fn into_parts(self) -> (T, Metadata, Option<Duration>, bool) {
        (self.message, self.metadata, self.timeout, self.compress)
    }

    /// Build a server-side request straight from the received header map.
    pub(crate) fn from_wire(
        message: T,
        headers: http::HeaderMap,
        remote_addr: Option<SocketAddr>,
    ) -> Self {
        Self {
            message,
            metadata: Metadata::from_owned_headers(headers),
            timeout: None,
            compress: false,
            compressed: false,
            remote_addr,
        }
    }

    pub(crate) fn set_compressed(&mut self, compressed: bool) {
        self.compressed = compressed;
    }
}

/// A [`Request`] envelope without its message. See
/// [`Request::into_message_and_parts`].
pub struct Parts {
    metadata: Metadata,
    timeout: Option<Duration>,
    compressed: bool,
    remote_addr: Option<SocketAddr>,
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

    /// Peer address, when the transport exposed one.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
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

    /// Whether the received frame had the Compressed-Flag set.
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

/// An RPC in flight.
///
/// Await it for the result. Dropping it without awaiting abandons the RPC;
/// dropping it after [`Self::cancel`] resets the HTTP/2 stream so the server
/// stops working on it.
#[must_use = "an RPC does nothing until awaited"]
pub struct Call<T> {
    fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,
    cancel: watch::Sender<bool>,
}

impl<T> Call<T> {
    pub(crate) fn new(
        cancel: watch::Sender<bool>,
        fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,
    ) -> Self {
        Self { fut, cancel }
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
        self.fut.as_mut().poll(cx)
    }
}

/// A cancel signal for an in-flight [`Call`], detached from the call itself.
#[derive(Clone)]
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
        req.metadata_mut().insert("k", "v").expect("insert");
        let (message, parts) = req.into_message_and_parts();
        assert_eq!(message, 1);
        let rebuilt = Request::<u32>::from_message_and_parts("swapped", parts);
        assert_eq!(rebuilt.timeout(), Some(Duration::from_millis(7)));
        assert_eq!(rebuilt.metadata().get("k"), Some("v"));
        assert_eq!(rebuilt.into_inner(), "swapped");
    }

    #[test]
    fn response_map_keeps_metadata() {
        let mut resp = Response::new(2u32);
        resp.trailers_mut().insert("t", "1").expect("insert");
        let mapped = resp.map(|n| n * 21);
        assert_eq!(mapped.trailers().get("t"), Some("1"));
        assert_eq!(mapped.into_inner(), 42);
    }
}
