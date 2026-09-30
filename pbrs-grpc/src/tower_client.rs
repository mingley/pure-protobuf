//! Tower client adapter for [`Channel`].
//!
//! The adapter is opt-in behind the `tower` feature. It does not insert a
//! buffer into the native client path; tower layers wrap the returned service
//! exactly where the caller chooses.

use crate::codec::CodecMessage;
use crate::{Call, Channel, Request, Response, Status, StreamSender, Streaming};
use futures_core::Stream;
use std::future::poll_fn;
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

macro_rules! streaming_service {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        #[derive(Debug)]
        pub struct $name<Req, Resp> {
            channel: Channel,
            path: &'static str,
            _types: PhantomData<fn(Req) -> Resp>,
        }

        impl<Req, Resp> Clone for $name<Req, Resp> {
            fn clone(&self) -> Self {
                Self::new(self.channel.clone(), self.path)
            }
        }

        impl<Req, Resp> $name<Req, Resp> {
            /// Wrap `channel` for calls to `path`.
            #[must_use]
            pub fn new(channel: Channel, path: &'static str) -> Self {
                Self { channel, path, _types: PhantomData }
            }

            /// The gRPC path this service calls.
            #[must_use]
            pub fn path(&self) -> &'static str { self.path }

            /// The underlying channel.
            #[must_use]
            pub fn channel(&self) -> &Channel { &self.channel }

            /// Recover the underlying channel.
            #[must_use]
            pub fn into_inner(self) -> Channel { self.channel }
        }
    };
}

streaming_service!(
    /// A Tower service for one server-streaming gRPC method.
    ///
    /// The response contains the native [`Streaming`], including trailers,
    /// deadline enforcement and cancellation when dropped before EOF.
    ServerStreamingService
);
streaming_service!(
    /// A Tower service for one client-streaming gRPC method.
    ///
    /// Accepts a [`Request`] containing any sendable [`Stream`] of
    /// `Result<Req, Status>`, including [`Streaming::channel`] streams.
    /// A bounded forwarding task feeds the native request sender and exits
    /// when the input ends, the RPC ends or the [`Call`] is cancelled.
    /// Live streams cannot be cloned for automatic middleware retries.
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloRequest, HelloReply, Request, Streaming};
    /// # use tower::ServiceExt;
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let (tx, input) = Streaming::channel(4);
    /// let service = channel.tower_client_streaming::<HelloRequest, HelloReply>(
    ///     "/helloworld.Greeter/ClientHello",
    /// );
    /// let send = async move {
    ///     let mut message = HelloRequest::new();
    ///     message.set_name("hello");
    ///     tx.send(message).await?;
    ///     tx.close();
    ///     Ok::<_, pbrs_grpc::Status>(())
    /// };
    /// let (sent, response) = tokio::join!(send, service.oneshot(Request::new(input)));
    /// sent?;
    /// let reply = response?.into_inner();
    /// # let _ = reply;
    /// # Ok(())
    /// # }
    /// ```
    ClientStreamingService
);
streaming_service!(
    /// A Tower service for one bidirectional gRPC method.
    ///
    /// Accepts the same request streams as [`ClientStreamingService`].
    /// Request forwarding continues after response headers arrive. Dropping
    /// the received [`Streaming`] cancels the RPC and stops forwarding, even
    /// when the request producer is idle. Native trailers and deadlines apply.
    BidiService
);

impl Channel {
    /// Expose one server-streaming method as a Tower service.
    #[must_use]
    pub fn tower_server_streaming<Req, Resp>(
        &self,
        path: &'static str,
    ) -> ServerStreamingService<Req, Resp> {
        ServerStreamingService::new(self.clone(), path)
    }

    /// Expose one client-streaming method as a Tower service.
    ///
    /// Requests contain a [`Stream`] of `Result<Req, Status>`. Metadata,
    /// compression settings and the timeout are preserved on the native RPC.
    #[must_use]
    pub fn tower_client_streaming<Req, Resp>(
        &self,
        path: &'static str,
    ) -> ClientStreamingService<Req, Resp> {
        ClientStreamingService::new(self.clone(), path)
    }

    /// Expose one bidirectional method as a Tower service.
    ///
    /// Requests contain a [`Stream`] of `Result<Req, Status>`. The returned
    /// [`Call`] resolves at response headers; request production can continue.
    #[must_use]
    pub fn tower_bidi<Req, Resp>(&self, path: &'static str) -> BidiService<Req, Resp> {
        BidiService::new(self.clone(), path)
    }
}

impl<Req, Resp> Service<Request<Req>> for ServerStreamingService<Req, Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
{
    type Response = Response<Streaming<Resp>>;
    type Error = Status;
    type Future = Call<Self::Response>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Req>) -> Self::Future {
        self.channel.server_streaming(self.path, request)
    }
}

impl<Req, Resp, S> Service<Request<S>> for ClientStreamingService<Req, Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
    S: Stream<Item = Result<Req, Status>> + Send + 'static,
{
    type Response = Response<Resp>;
    type Error = Status;
    type Future = Call<Self::Response>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<S>) -> Self::Future {
        let (stream, parts) = request.into_message_and_parts();
        let (sender, call) = self
            .channel
            .client_streaming(self.path, Request::<()>::from_message_and_parts((), parts));
        forward_requests(stream, sender);
        call
    }
}

impl<Req, Resp, S> Service<Request<S>> for BidiService<Req, Resp>
where
    Req: CodecMessage + Send + 'static,
    Resp: CodecMessage + Send + 'static,
    S: Stream<Item = Result<Req, Status>> + Send + 'static,
{
    type Response = Response<Streaming<Resp>>;
    type Error = Status;
    type Future = Call<Self::Response>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<S>) -> Self::Future {
        let (stream, parts) = request.into_message_and_parts();
        let (sender, call) = self
            .channel
            .bidi(self.path, Request::<()>::from_message_and_parts((), parts));
        forward_requests(stream, sender);
        call
    }
}

fn forward_requests<Req, S>(stream: S, sender: StreamSender<Req>)
where
    Req: CodecMessage + Send + 'static,
    S: Stream<Item = Result<Req, Status>> + Send + 'static,
{
    // No unbounded queue: send waits on the native channel's stream buffer.
    // Watch the receiver as well as the input so an idle producer cannot
    // keep this task alive after native deadline/cancellation/early response.
    drop(tokio::spawn(async move {
        tokio::pin!(stream);
        loop {
            let next = tokio::select! {
                biased;
                () = sender.closed() => return,
                next = poll_fn(|cx| stream.as_mut().poll_next(cx)) => next,
            };
            match next {
                Some(Ok(message)) => {
                    if sender.send(message).await.is_err() {
                        return;
                    }
                }
                Some(Err(status)) => {
                    sender.fail(status).await;
                    return;
                }
                None => return,
            }
        }
    }));
}
