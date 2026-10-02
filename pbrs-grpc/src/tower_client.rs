//! Tower client adapter for [`Channel`].
//!
//! The adapter is opt-in behind the `tower` feature. It does not insert a
//! buffer into the native client path; tower layers wrap the returned service
//! exactly where the caller chooses.
//!
//! With the `tonic` feature, [`Channel`] also implements
//! `Service<http::Request<tonic::body::Body>>`. Unmodified tonic-generated
//! clients can use `Client::new(channel)` or `Client::with_interceptor(channel,
//! interceptor)`. Tonic owns message framing, codecs, compression and message
//! limits; this adapter forwards existing `Bytes` without decoding or copying
//! their payload. The channel supplies pooling, discovery, TLS, HTTP/2 tuning,
//! authority, wait-for-ready, RPC concurrency and deadline enforcement.
//! Tonic 0.14's decoder still copies received DATA into its `BytesMut` decode
//! buffer, as on tonic's own transport. Native prost stubs can instead slice a
//! complete uncompressed frame directly from its DATA chunk. This adapter
//! preserves tonic's codec behavior; it does not claim native-stub copy costs.
//!
//! Native typed-call interceptors, binary logging, finite transport-byte
//! budgets, custom native message/compression settings and matching method
//! service-config policies cannot be applied to opaque tonic bodies. Such
//! configurations are rejected before dispatch instead of bypassed. Configure
//! these policies on the tonic-generated client or its Tower layers. Native
//! per-call telemetry/retry accounting is not emitted by this HTTP adapter.

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

#[cfg(feature = "tonic")]
mod tonic_transport {
    use super::Channel;
    use crate::client::pool::{LiveConn, ingest_call_status, track_least_request};
    use crate::transport::{FlowControl, Reason, RecvStream, SendRequest, SendStream, h2};
    use bytes::Bytes;
    use http::{Request, Response, Uri};
    use http_body::{Body, Frame};
    use std::future::{Future, poll_fn};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::{OwnedSemaphorePermit, oneshot, watch};
    use tokio::time::{Instant, Sleep};
    use tonic::Status;
    use tower::Service;

    impl Service<Request<tonic::body::Body>> for Channel {
        type Response = Response<tonic::body::Body>;
        type Error = Status;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Status>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Status>> {
            // Pool capacity is awaited by call, without holding a slot lock.
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: Request<tonic::body::Body>) -> Self::Future {
            let channel = self.clone();
            let timeout = request
                .headers()
                .get("grpc-timeout")
                .and_then(|value| value.to_str().ok())
                .and_then(crate::timeout::parse_timeout);
            let timeout = match (timeout, self.config.rpc_timeout()) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            // Start the deadline at dispatch, including time waiting for a pool.
            let deadline = crate::timeout::deadline_from(timeout);
            Box::pin(async move {
                match deadline {
                    Some(at) => tokio::select! {
                        biased;
                        () = tokio::time::sleep_until(at) => Err(Status::deadline_exceeded("deadline exceeded")),
                        result = call(channel, request, deadline) => result,
                    },
                    None => call(channel, request, deadline).await,
                }
            })
        }
    }

    fn check_policies(channel: &Channel, path: &str) -> Result<(), Status> {
        let defaults = crate::ChannelConfig::default();
        if !channel.interceptors.is_empty()
            || !channel.response_interceptors.is_empty()
            || channel.binlog.is_some()
            || channel.byte_budget.limit().is_some()
            || channel.config.limits() != defaults.limits()
            || channel.config.compresses_outbound() != defaults.compresses_outbound()
            || channel.config.accepts_compressed() != defaults.accepts_compressed()
            || channel.config.send_codec() != defaults.send_codec()
            || channel.config.gzip_level() != defaults.gzip_level()
            || channel.method_config_for(path).is_some()
        {
            return Err(Status::failed_precondition(
                "native typed-call policies cannot apply to tonic HTTP bodies; configure interceptors, compression, message limits, logging and retries on the tonic client or Tower layers",
            ));
        }
        Ok(())
    }

    async fn call(
        channel: Channel,
        request: Request<tonic::body::Body>,
        deadline: Option<Instant>,
    ) -> Result<Response<tonic::body::Body>, Status> {
        let (mut parts, body) = request.into_parts();
        check_policies(&channel, parts.uri.path())?;
        let rpc_permit = channel
            .rpc_slots
            .as_ref()
            .map(|slots| {
                slots.clone().try_acquire_owned().map_err(|_| {
                    Status::resource_exhausted("channel RPC concurrency limit exceeded")
                })
            })
            .transpose()?;
        let metadata = crate::Metadata::from_headers(&parts.headers);
        let hash = channel.ring_request_hash(&metadata);
        let mut live = channel
            .inner
            .acquire(
                channel.config.waits_for_ready(),
                channel.observer.as_deref(),
                channel.health_directive(),
                hash,
            )
            .await
            .map_err(status)?;
        let load = track_least_request(&channel.inner.endpoint, live.rr_addr.as_ref()).await;
        parts.uri = Uri::builder()
            .scheme(channel.scheme())
            .authority(channel.authority.clone())
            .path_and_query(parts.uri.path_and_query().cloned().ok_or_else(|| {
                Status::invalid_argument("gRPC request URI requires a method path")
            })?)
            .build()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        parts.version = http::Version::HTTP_2;
        if let Some(remaining) = crate::timeout::remaining_timeout(deadline).map_err(status)? {
            let value = http::HeaderValue::from_str(&crate::timeout::encode_timeout(remaining))
                .map_err(|error| Status::internal(error.to_string()))?;
            parts.headers.insert("grpc-timeout", value);
        }
        if !parts.headers.contains_key(http::header::USER_AGENT) {
            parts
                .headers
                .insert(http::header::USER_AGENT, channel.user_agent.clone());
        }
        let end_stream = body.is_end_stream();
        let (response, send) = match live
            .send
            .send_request(Request::from_parts(parts, ()), end_stream)
        {
            Ok(stream) => stream,
            Err(error) => {
                channel
                    .inner
                    .discard_conn(live.slot, live.r#gen, live.rr_addr.as_ref())
                    .await;
                return Err(status(crate::Status::from_h2_pre_headers(error)));
            }
        };
        let (cancel, cancelled) = watch::channel(false);
        let guard = CancelOnDrop(cancel);
        let (upload_tx, mut upload_error) = oneshot::channel();
        // One forwarding task per HTTP request, no extra message queue. It is
        // cancelled even if the caller disappears before response headers.
        drop(tokio::spawn(forward_body(
            body,
            send,
            end_stream,
            channel.config.send_buffer_size(),
            deadline,
            cancelled,
            upload_tx,
        )));
        tokio::pin!(response);
        let mut upload_pending = true;
        let mut transport_upload_error = None;
        let response = tokio::select! {
            biased;
            response = &mut response => match response {
                Ok(response) => response,
                Err(error) => {
                    // A reset caused by a local tonic encoder failure must
                    // retain that failure's status (e.g. message-size limits).
                    if let Ok(error) = upload_error.try_recv() {
                        return Err(error.status);
                    }
                    return Err(status(crate::Status::from_h2_post_dispatch(error)));
                }
            },
            error = &mut upload_error => {
                upload_pending = false;
                if let Ok(error) = error {
                    if !error.transport {
                        return Err(error.status);
                    }
                    // A peer can close the upload half after either a
                    // trailers-only rejection or normal headers followed by
                    // terminal trailers. Retain that response for tonic.
                    match response.await {
                        Ok(response) => {
                            transport_upload_error = Some(error.status);
                            response
                        }
                        Err(_) => return Err(error.status),
                    }
                } else {
                    response.await.map_err(|error| status(crate::Status::from_h2_post_dispatch(error)))?
                }
            }
        };
        let (parts, recv) = response.into_parts();
        let initial_status = parts
            .headers
            .contains_key("grpc-status")
            .then(|| crate::wire::status_from(&parts.headers, None));
        Ok(Response::from_parts(
            parts,
            tonic::body::Body::new(ResponseBody {
                recv,
                deadline: deadline.map(|at| Box::pin(tokio::time::sleep_until(at))),
                upload_error: upload_pending.then_some(upload_error),
                transport_upload_error,
                data_done: false,
                done: false,
                guard: Some(guard),
                channel,
                initial_status,
                reported: false,
                live: Some(live),
                rpc_permit,
                load: Some(load),
            }),
        ))
    }

    fn status(status: crate::Status) -> Status {
        Status::new(
            tonic::Code::from_i32(status.code().to_i32()),
            status.message(),
        )
    }

    struct CancelOnDrop(watch::Sender<bool>);

    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.send_replace(true);
        }
    }

    struct UploadError {
        status: Status,
        transport: bool,
    }

    impl UploadError {
        fn local(status: Status) -> Self {
            Self {
                status,
                transport: false,
            }
        }

        fn native(error: crate::Status) -> Self {
            Self {
                // Send/reset/window errors are transport failures even when
                // their native public status is INTERNAL. A zero send budget
                // is the local configuration error from send_bytes instead.
                transport: error.code() != crate::Code::InvalidArgument,
                status: status(error),
            }
        }
    }

    async fn forward_body(
        mut body: tonic::body::Body,
        mut send: h2::SendStream,
        mut ended: bool,
        send_buffer: usize,
        deadline: Option<Instant>,
        mut cancelled: watch::Receiver<bool>,
        upload_error: oneshot::Sender<UploadError>,
    ) {
        let forward = async {
            if !ended {
                while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
                    let frame = frame.map_err(UploadError::local)?;
                    match frame.into_data() {
                        Ok(bytes) => {
                            crate::wire::send::send_bytes(&mut send, bytes, false, send_buffer)
                                .await
                                .map_err(UploadError::native)?
                        }
                        Err(frame) => {
                            if let Ok(trailers) = frame.into_trailers() {
                                send.send_trailers(trailers).map_err(|error| {
                                    UploadError::native(crate::Status::from_h2_send(error))
                                })?;
                                ended = true;
                                break;
                            }
                        }
                    }
                }
                if !ended {
                    send.send_data(Bytes::new(), true)
                        .map_err(|error| UploadError::native(crate::Status::from_h2_send(error)))?;
                }
            }
            Ok::<(), UploadError>(())
        };
        let result = tokio::select! {
            biased;
            _ = cancelled.wait_for(|cancel| *cancel) => Err(UploadError::local(Status::cancelled("RPC cancelled"))),
            () = until_deadline(deadline) => Err(UploadError::local(Status::deadline_exceeded("deadline exceeded"))),
            result = forward => result,
        };
        if let Err(error) = result {
            if !error.transport {
                send.send_reset(Reason::CANCEL);
            }
            upload_error.send(error).ok();
            return;
        }
        // Retain the send half to reset an idle/half-closed RPC when its
        // response is dropped or the receiving deadline expires.
        tokio::select! {
            biased;
            _ = cancelled.wait_for(|cancel| *cancel) => send.send_reset(Reason::CANCEL),
            () = until_deadline(deadline) => {
                send.send_reset(Reason::CANCEL);
                upload_error.send(UploadError::local(Status::deadline_exceeded("deadline exceeded"))).ok();
                return;
            },
            _ = poll_fn(|cx| send.poll_reset(cx)) => {},
        }
        drop(upload_error);
    }

    async fn until_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(at) => tokio::time::sleep_until(at).await,
            None => std::future::pending().await,
        }
    }

    struct ResponseBody {
        recv: h2::RecvStream,
        deadline: Option<Pin<Box<Sleep>>>,
        upload_error: Option<oneshot::Receiver<UploadError>>,
        transport_upload_error: Option<Status>,
        data_done: bool,
        done: bool,
        guard: Option<CancelOnDrop>,
        channel: Channel,
        initial_status: Option<crate::Status>,
        reported: bool,
        live: Option<LiveConn>,
        rpc_permit: Option<OwnedSemaphorePermit>,
        load: Option<crate::lb::LrTrack>,
    }

    impl ResponseBody {
        fn report(&mut self, status: &crate::Status) {
            if !self.reported {
                ingest_call_status(
                    &self.channel.inner.endpoint,
                    self.live.as_ref().and_then(|live| live.rr_addr.as_ref()),
                    status,
                );
                self.reported = true;
            }
        }

        fn finish(&mut self) {
            self.done = true;
            self.guard.take();
            self.live.take();
            self.rpc_permit.take();
            self.load.take();
        }

        fn upload_failure(&mut self) -> Poll<Option<Result<Frame<Bytes>, Status>>> {
            match self.transport_upload_error.take() {
                Some(error) => {
                    self.report(&crate::Status::new(
                        crate::Code::from_i32(error.code() as i32),
                        error.message().to_owned(),
                    ));
                    self.finish();
                    Poll::Ready(Some(Err(error)))
                }
                None => Poll::Pending,
            }
        }
    }

    impl Body for ResponseBody {
        type Data = Bytes;
        type Error = Status;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Status>>> {
            if self.done {
                return Poll::Ready(None);
            }
            if self
                .deadline
                .as_mut()
                .is_some_and(|sleep| sleep.as_mut().poll(cx).is_ready())
            {
                self.report(&crate::Status::deadline_exceeded());
                self.finish();
                return Poll::Ready(Some(Err(Status::deadline_exceeded("deadline exceeded"))));
            }
            if let Some(error) = &mut self.upload_error {
                match Pin::new(error).poll(cx) {
                    Poll::Ready(Ok(error)) => {
                        self.upload_error = None;
                        if error.transport {
                            self.transport_upload_error = Some(error.status);
                        } else {
                            self.report(&crate::Status::new(
                                crate::Code::from_i32(error.status.code() as i32),
                                error.status.message().to_owned(),
                            ));
                            self.finish();
                            return Poll::Ready(Some(Err(error.status)));
                        }
                    }
                    Poll::Ready(Err(_)) => self.upload_error = None,
                    Poll::Pending => {}
                }
            }
            if !self.data_done {
                match self.recv.poll_data(cx) {
                    Poll::Pending => return self.upload_failure(),
                    Poll::Ready(Some(Ok(bytes))) => {
                        if let Err(error) = self.recv.flow_control().release_capacity(bytes.len()) {
                            let error = crate::Status::from_h2(error);
                            self.report(&error);
                            self.finish();
                            return Poll::Ready(Some(Err(status(error))));
                        }
                        return Poll::Ready(Some(Ok(Frame::data(bytes))));
                    }
                    Poll::Ready(Some(Err(error))) => {
                        let error = crate::Status::from_h2_post_dispatch(error);
                        self.report(&error);
                        self.finish();
                        return Poll::Ready(Some(Err(status(error))));
                    }
                    Poll::Ready(None) => self.data_done = true,
                }
            }
            match self.recv.poll_trailers(cx) {
                Poll::Pending => self.upload_failure(),
                Poll::Ready(trailers) => {
                    let terminal_status = match &trailers {
                        Ok(Some(trailers)) => crate::wire::status_from(trailers, None),
                        Ok(None) => self
                            .initial_status
                            .clone()
                            .unwrap_or_else(|| crate::Status::unknown("missing grpc-status")),
                        Err(error) => crate::Status::unavailable(error.to_string()),
                    };
                    if terminal_status.code() == crate::Code::Ok
                        && self.transport_upload_error.is_some()
                    {
                        // A successful response cannot acknowledge an upload
                        // that failed before its declared end of stream.
                        return self.upload_failure();
                    }
                    self.report(&terminal_status);
                    self.finish();
                    Poll::Ready(match trailers {
                        Ok(Some(trailers)) => Some(Ok(Frame::trailers(trailers))),
                        Ok(None) => None,
                        Err(error) => {
                            Some(Err(status(crate::Status::from_h2_post_dispatch(error))))
                        }
                    })
                }
            }
        }

        fn is_end_stream(&self) -> bool {
            self.done
        }
    }

    impl Drop for ResponseBody {
        fn drop(&mut self) {
            if let Some(status) = self.initial_status.clone() {
                self.report(&status);
            } else if !self.done {
                self.report(&crate::Status::cancelled());
            }
        }
    }
}
