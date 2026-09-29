//! Tower client adapter for [`Channel`].
//!
//! The adapter is opt-in behind the `tower` feature. It does not insert a
//! buffer into the native client path; tower layers wrap the returned service
//! exactly where the caller chooses.

use crate::codec::CodecMessage;
use crate::{Call, Channel, Request, Response, Status};
use std::marker::PhantomData;
use std::task::{Context, Poll};
use tower::Service;

/// A tower service for one unary gRPC method on a [`Channel`].
#[derive(Debug)]
pub struct UnaryService<Req, Resp> {
    channel: Channel,
    path: &'static str,
    _types: PhantomData<fn(Req) -> Resp>,
}

impl<Req, Resp> Clone for UnaryService<Req, Resp> {
    fn clone(&self) -> Self {
        Self {
            channel: self.channel.clone(),
            path: self.path,
            _types: PhantomData,
        }
    }
}

impl<Req, Resp> UnaryService<Req, Resp> {
    /// Wrap `channel` for calls to `path`.
    #[must_use]
    pub fn new(channel: Channel, path: &'static str) -> Self {
        Self {
            channel,
            path,
            _types: PhantomData,
        }
    }

    /// The gRPC path this service calls.
    #[must_use]
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// The underlying channel.
    #[must_use]
    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    /// Recover the underlying channel.
    #[must_use]
    pub fn into_inner(self) -> Channel {
        self.channel
    }
}

impl Channel {
    /// Expose one unary method on this channel as a tower service.
    #[must_use]
    pub fn tower_unary<Req, Resp>(&self, path: &'static str) -> UnaryService<Req, Resp> {
        UnaryService::new(self.clone(), path)
    }
}

impl<Req, Resp> Service<Request<Req>> for UnaryService<Req, Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    type Response = Response<Resp>;
    type Error = Status;
    type Future = Call<Response<Resp>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Req>) -> Self::Future {
        self.channel.unary(self.path, request)
    }
}
