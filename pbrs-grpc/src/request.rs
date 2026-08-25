//! Request, response, and cancellable RPC call.

use crate::metadata::Metadata;
use crate::status::Status;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::watch;

/// Outbound or inbound RPC envelope.
pub struct Request<T> {
    message: T,
    metadata: Metadata,
    timeout: Option<Duration>,
    compress: bool,
    compressed: bool,
}

impl<T> Request<T> {
    /// Wrap a message with empty metadata and no timeout.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self {
            message,
            metadata: Metadata::new(),
            timeout: None,
            compress: false,
            compressed: false,
        }
    }

    /// Take the message.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }

    /// Metadata (HTTP/2 headers).
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Set `grpc-timeout` / local deadline.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Configured timeout.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Compress this request's protobuf payload with gzip (Compressed-Flag 1).
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// Whether the inbound message had Compressed-Flag 1.
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compressed
    }

    pub(crate) fn into_parts(self) -> (T, Metadata, Option<Duration>, bool) {
        (self.message, self.metadata, self.timeout, self.compress)
    }

    pub(crate) fn set_metadata(&mut self, metadata: Metadata) {
        self.metadata = metadata;
    }

    pub(crate) fn set_compressed(&mut self, compressed: bool) {
        self.compressed = compressed;
    }
}

/// RPC reply: message, initial headers, and trailing metadata.
pub struct Response<T> {
    message: T,
    metadata: Metadata,
    trailers: Metadata,
    compress: bool,
}

impl<T> Response<T> {
    /// Wrap a message with empty headers and trailers.
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

    /// Initial headers (not trailers).
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable initial headers.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Trailing metadata (`grpc-status` is not stored here).
    #[must_use]
    pub fn trailers(&self) -> &Metadata {
        &self.trailers
    }

    /// Mutable trailing metadata. Survives on the OK path.
    pub fn trailers_mut(&mut self) -> &mut Metadata {
        &mut self.trailers
    }

    /// Compress this response payload with gzip.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// Whether the inbound payload had Compressed-Flag 1.
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

/// In-flight RPC. Dropping without cancel leaves the HTTP/2 stream to finish.
#[must_use = "calls do nothing unless awaited"]
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

    /// Ask the client to reset the stream and resolve with [`crate::Code::Cancelled`].
    pub fn cancel(&self) {
        self.cancel.send(true).ok();
    }

    /// Cloneable cancel handle for another task.
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

/// Cloneable cancel signal for an in-flight [`Call`].
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
