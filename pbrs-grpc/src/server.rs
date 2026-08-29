//! Serving: the [`Service`] trait, per-RPC dispatch through [`Rpc`], and the
//! [`Server`] / [`Router`] accept loops.
//!
//! Generated code implements [`Service`]; you implement the generated service
//! trait. Writing either by hand is supported and documented, because a
//! kernel you cannot drive by hand is a kernel you cannot debug.

use crate::config::{ServerConfig, Wire};
use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::tls::ServerTls;
use crate::wire::{
    check_request, effective_timeout, encode_msg, grpc_trailers, let_producer_catch_up,
    read_one_message, reject, send_bytes, send_ok_headers, send_trailers_only, wrap_timeout,
    OutBatch, WireStream,
};
use bytes::Bytes;
use h2::RecvStream;
use pbrs::{Parse, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::Poll;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::{mpsc, watch, Semaphore};

/// A gRPC service that can be served.
///
/// `protoc-gen-pbrs` emits one implementation per `service` in your `.proto`,
/// so the usual path is to implement the generated trait (`Greeter`) and let
/// the generated type (`GreeterServer`) implement this.
///
/// Implementing it by hand takes a name and a `match` on
/// [`Rpc::method`]:
///
/// ```
/// use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Service, Status};
///
/// struct Echo;
///
/// impl Service for Echo {
///     const NAME: &'static str = "demo.Echo";
///
///     async fn call(&self, rpc: Rpc) {
///         match rpc.method() {
///             "Ping" => {
///                 rpc.unary(|req: Request<HelloRequest>| async move {
///                     let mut reply = HelloReply::new();
///                     reply.set_message(req.get_ref().name());
///                     Ok::<_, Status>(Response::new(reply))
///                 })
///                 .await;
///             }
///             _ => rpc.unimplemented(),
///         }
///     }
/// }
/// ```
pub trait Service: Send + Sync + 'static {
    /// Fully qualified proto service name, e.g. `helloworld.Greeter`.
    ///
    /// [`Router`] keys on this, and it is the `<service>` half of the
    /// `/<service>/<method>` request path.
    const NAME: &'static str;

    /// Dispatch one RPC.
    ///
    /// Match on [`Rpc::method`] and consume the [`Rpc`] with the call shape
    /// the method declares. Returning without consuming it resets the stream.
    fn call(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
}

/// Object-safe [`Service`], so [`Router`] can hold a heterogeneous map.
trait DynService: Send + Sync + 'static {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

impl<S: Service> DynService for S {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(self.call(rpc))
    }
}

/// What the accept loop hands each stream. Monomorphic for [`Server`], boxed
/// for [`Router`].
trait Dispatch: Send + Sync + 'static {
    fn dispatch(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
}

/// One [`Incoming::accept`] result: a connection, an error, or `None` if exhausted.
#[allow(
    clippy::type_complexity,
    reason = "Option<Result<(Io, peer), Status>> is the accept contract"
)]
pub type IncomingAccept<Io> = Option<Result<(Io, Option<SocketAddr>), Status>>;

/// A source of already-accepted byte streams.
///
/// [`TcpListener`] and Unix listeners are served by [`Server::serve_listener`]
/// / [`Server::serve_unix_listener`] so TCP_NODELAY, TCP keepalive, and TLS
/// stay applied. Implement this for a custom acceptor (in-process duplex,
/// vsock, a TLS stack you drove yourself).
///
/// Returning `None` means the source is exhausted: the server stops accepting,
/// sends `GOAWAY`, and drains. After the last connection, pending forever is
/// usually what you want, so the live stream is not torn down.
///
/// ```no_run
/// use std::future::Future;
/// use pbrs_grpc::{Incoming, IncomingAccept};
///
/// struct One(Option<tokio::net::TcpStream>);
///
/// impl Incoming for One {
///     type Io = tokio::net::TcpStream;
///     fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send {
///         let io = self.0.take();
///         async move { io.map(|io| Ok((io, None))) }
///     }
/// }
/// ```
pub trait Incoming: Send {
    /// Accepted byte stream. Must be an HTTP/2 prior-knowledge transport;
    /// this crate does not speak HTTP/1.1 or grpc-web.
    type Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static;

    /// Next connection, or `None` when the source is exhausted.
    ///
    /// `SocketAddr` is what [`Rpc::remote_addr`] reports; use `None` when the
    /// transport has no TCP peer (Unix, in-process).
    fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send;
}

/// One inbound RPC, before its call shape has been chosen.
///
/// Consume it with exactly one of [`Self::unary`],
/// [`Self::client_streaming`], [`Self::server_streaming`],
/// [`Self::bidi_streaming`], or [`Self::unimplemented`]. Each one owns the
/// full response: headers, message frames, and `grpc-status` trailers.
pub struct Rpc {
    request: http::Request<RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
    config: ServerConfig,
    remote_addr: Option<SocketAddr>,
    extensions: http::Extensions,
}

impl std::fmt::Debug for Rpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rpc")
            .field("path", &self.path())
            .field("remote_addr", &self.remote_addr)
            .finish_non_exhaustive()
    }
}

impl Rpc {
    /// Full request path, e.g. `/helloworld.Greeter/SayHello`.
    #[must_use]
    pub fn path(&self) -> &str {
        self.request.uri().path()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    #[must_use]
    pub fn service(&self) -> &str {
        split_path(self.path()).0
    }

    /// Method half of the path, e.g. `SayHello`.
    #[must_use]
    pub fn method(&self) -> &str {
        split_path(self.path()).1
    }

    /// Peer address, when the transport exposed one.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Request headers as gRPC metadata.
    #[must_use]
    pub fn metadata(&self) -> Metadata {
        Metadata::from_headers(self.request.headers())
    }

    /// Effective message caps for this RPC.
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.config.limits()
    }

    /// Typed values an interceptor may attach for the handler.
    ///
    /// Empty until an [`crate::Interceptor`] (or wrapping [`Service`]) inserts
    /// into [`Self::extensions_mut`]. Survives onto the [`Request`] the
    /// handler receives.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values the handler will see on [`Request::extensions`].
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// Answer with `UNIMPLEMENTED`, naming the path.
    ///
    /// This is the correct default arm of a method `match`: a peer asking for
    /// a method you do not have is a peer error, not a server error.
    pub fn unimplemented(mut self) {
        send_trailers_only(
            &mut self.respond,
            Status::unimplemented(self.request.uri().path().to_string()),
            &Metadata::new(),
        );
    }

    /// Answer with `status` without reading the request body.
    ///
    /// This is how an [`crate::Interceptor`] or a wrapping [`Service`] turns
    /// away an RPC it will not delegate, for example on failed authentication.
    /// Any trailing metadata on `status` is delivered.
    ///
    /// ```
    /// use pbrs_grpc::{Rpc, Service, Status};
    /// use std::sync::Arc;
    ///
    /// /// Requires a bearer token before delegating to `inner`.
    /// struct RequireAuth<S> {
    ///     inner: Arc<S>,
    ///     token: String,
    /// }
    ///
    /// impl<S: Service> Service for RequireAuth<S> {
    ///     const NAME: &'static str = S::NAME;
    ///
    ///     async fn call(&self, rpc: Rpc) {
    ///         if rpc.metadata().get("authorization") != Some(self.token.as_str()) {
    ///             return rpc.reject(Status::unauthenticated("bad or missing token"));
    ///         }
    ///         self.inner.call(rpc).await;
    ///     }
    /// }
    /// ```
    pub fn reject(mut self, status: Status) {
        send_trailers_only(&mut self.respond, status, &Metadata::new());
    }

    /// Serve a unary method: one request message, one response message.
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.unary(|req: Request<HelloRequest>| async move {
    ///     let mut reply = HelloReply::new();
    ///     reply.set_message(req.get_ref().name());
    ///     Ok::<_, Status>(Response::new(reply))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn unary<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default,
        Resp: Serialize,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            ..
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => send_unary_response(response, respond, wire).await,
        }
    }

    /// Serve a client-streaming method: many request messages, one response.
    pub async fn client_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default + Send + 'static,
        Resp: Serialize,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            ..
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => send_unary_response(response, respond, wire).await,
        }
    }

    /// Serve a server-streaming method: one request message, many responses.
    pub async fn server_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default,
        Resp: Serialize + Send,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => send_stream_response(response, respond, wire, deadline).await,
        }
    }

    /// Serve a bidirectional-streaming method.
    pub async fn bidi_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: Parse + Default + Send + 'static,
        Resp: Serialize + Send,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => send_stream_response(response, respond, wire, deadline).await,
        }
    }

    /// Read the single request message, then run `handler` under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_unary_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: Parse + Default,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            extensions,
        } = self;
        let limits = config.limits();
        if let Err(status) = check_request(&request) {
            reject(&mut respond, status);
            return None;
        }
        let timeout = effective_timeout(request.headers(), config.rpc_timeout());
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let (parts, mut recv) = request.into_parts();
        let outcome = wrap_timeout(timeout, async {
            let framed = read_one_message::<Req>(&mut recv, limits).await?;
            let mut req = Request::from_wire(framed.message, parts.headers, remote_addr)
                .with_extensions(extensions);
            req.set_compressed(framed.compressed);
            if let Some(d) = timeout {
                req.set_timeout(d);
            }
            tokio::select! {
                biased;
                result = handler(req) => result,
                gone = wait_client_reset(&mut respond) => Err(gone),
            }
        })
        .await;
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
        })
    }

    /// Hand the request stream to `handler`, under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_streaming_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: Parse + Default + Send + 'static,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            extensions,
        } = self;
        let limits = config.limits();
        if let Err(status) = check_request(&request) {
            reject(&mut respond, status);
            return None;
        }
        let timeout = effective_timeout(request.headers(), config.rpc_timeout());
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let (parts, recv) = request.into_parts();
        // Decoded on the handler's task: no pump task, no queue, and reading
        // is what releases HTTP/2 capacity.
        let stream = Streaming::from_wire(WireStream::<Req>::new(recv, limits, deadline));
        let mut req =
            Request::from_wire(stream, parts.headers, remote_addr).with_extensions(extensions);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        let outcome = wrap_timeout(timeout, async {
            tokio::select! {
                biased;
                result = handler(req) => result,
                gone = wait_client_reset(&mut respond) => Err(gone),
            }
        })
        .await;
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
        })
    }
}

/// Resolve when the client `RST_STREAM`s this RPC.
///
/// Unary handlers that have already read the request (and streaming handlers
/// that are not currently reading) would otherwise run to completion after
/// the caller has gone. `SendResponse::poll_reset` sees the reset without
/// needing the request body.
async fn wait_client_reset(respond: &mut h2::server::SendResponse<Bytes>) -> Status {
    drop(std::future::poll_fn(|cx| respond.poll_reset(cx)).await);
    Status::cancelled()
}

/// A handler result plus the response channel it still has to be written to.
struct Prepared<T> {
    respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    /// The RPC's deadline, shared by the handler, the inbound stream, and the
    /// response writer, so no stage can outlive it.
    deadline: Option<tokio::time::Instant>,
    outcome: Result<T, Status>,
}

async fn send_unary_response<Resp: Serialize>(
    response: Response<Resp>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
) {
    let (msg, headers, trailers, compress) = response.split();
    let frame = match encode_msg(&msg, compress, wire.limits) {
        Ok(frame) => frame,
        Err(status) => {
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, compress) else {
        return;
    };
    send_bytes(&mut send, frame, false, wire.send_buffer)
        .await
        .ok();
    let mut status = Status::new(Code::Ok, "");
    *status.metadata_mut() = trailers;
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

async fn send_stream_response<Resp: Serialize + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
) {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, compress) else {
        return;
    };
    let mut status = Status::from_code(Code::Ok);
    *status.metadata_mut() = trailers;
    // The deadline has to cover the whole response, not just the handler
    // future: a producer that stops early because *its* deadline expired must
    // not be reported as a clean end of stream.
    let drained = match deadline {
        None => drain_to_wire(&mut stream, &mut send, wire).await,
        Some(at) => tokio::time::timeout_at(at, drain_to_wire(&mut stream, &mut send, wire))
            .await
            .unwrap_or_else(|_| Err(DrainError::Producer(Status::deadline_exceeded()))),
    };
    if let Err(err) = drained {
        // A transport failure cannot be reported; a producer failure becomes
        // the stream's trailing status.
        match err {
            DrainError::Transport => return,
            DrainError::Producer(producer) => status = producer,
        }
    }
    // If the deadline elapsed, the RPC did not finish in time, however the
    // drain ended. A handler reading its request stream sees the deadline as an
    // error on the read and will usually just stop producing, which would
    // otherwise be indistinguishable from a clean end of stream.
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
}

/// Why a stream stopped before its clean end.
enum DrainError {
    /// The wire is gone, so no status can be delivered.
    Transport,
    /// The handler ended the stream with a status.
    Producer(Status),
}

/// Copy every message from `stream` onto `send`, batching each burst.
async fn drain_to_wire<Resp: Serialize + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut h2::SendStream<Bytes>,
    wire: Wire,
) -> Result<(), DrainError> {
    let mut batch = OutBatch::new(wire);
    let mut items = Vec::with_capacity(OutBatch::BURST);
    loop {
        items.clear();
        if stream.recv_many(&mut items, OutBatch::BURST).await == 0 {
            break;
        }
        // More than one message queued means the producer is running ahead of
        // the network and is bounded by its channel depth, so one scheduling
        // turn lets it top the queue up and doubles the write size. Exactly one
        // means it is not ahead — a request/response stream, say — and must not
        // pay a turn of latency for nothing.
        let room = OutBatch::BURST - items.len();
        if items.len() > 1 && room > 0 {
            let_producer_catch_up().await;
            stream.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let item = item.map_err(DrainError::Producer)?;
            batch
                .push(send, item)
                .await
                .map_err(|_| DrainError::Transport)?;
        }
        if !batch.is_full() {
            batch.flush(send).await.map_err(|_| DrainError::Transport)?;
        }
    }
    batch.flush(send).await.map_err(|_| DrainError::Transport)
}

/// Split `/service/method` without allocating. Unparseable paths yield empty
/// halves, which route to `UNIMPLEMENTED`.
fn split_path(path: &str) -> (&str, &str) {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match rest.rsplit_once('/') {
        Some((service, method)) => (service, method),
        None => ("", ""),
    }
}

/// Serves exactly one [`Service`], with no per-RPC dynamic dispatch.
///
/// ```no_run
/// use pbrs_grpc::{Server, ServerConfig};
/// # use pbrs_grpc::{Rpc, Service};
/// # struct Echo;
/// # impl Service for Echo {
/// #     const NAME: &'static str = "demo.Echo";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// Server::new(Echo)
///     .config(ServerConfig::new().max_concurrent_streams(1024))
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await
/// # }
/// ```
pub struct Server<S> {
    service: Arc<S>,
    config: ServerConfig,
}

impl<S: Service> std::fmt::Debug for Server<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("service", &S::NAME)
            .field("config", &self.config)
            .finish()
    }
}

impl<S: Service> Server<S> {
    /// Wrap an existing `Arc` without adding another layer.
    #[must_use]
    pub fn from_arc(service: Arc<S>) -> Self {
        Self {
            service,
            config: ServerConfig::default(),
        }
    }

    /// Take the inner `Arc` back.
    #[must_use]
    pub fn into_inner(self) -> Arc<S> {
        self.service
    }

    /// Serve `service` with default configuration.
    #[must_use]
    pub fn new(service: S) -> Self {
        Self {
            service: Arc::new(service),
            config: ServerConfig::default(),
        }
    }

    /// Replace the transport and limit configuration.
    #[must_use]
    pub fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// Cap inbound messages at `limit` bytes. Default 4 MiB.
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_decoding_message_size(limit);
        self
    }

    /// Cap outbound messages at `limit` bytes. Default unlimited.
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_encoding_message_size(limit);
        self
    }

    /// Run `interceptor` before this service sees any RPC.
    ///
    /// Closures implement [`crate::Interceptor`], so
    /// `server.intercept(|rpc| { ... })` is the usual form. Generated servers
    /// expose the same method: `GreeterServer::new(svc).intercept(auth).serve(addr)`.
    /// On a [`Router`], call [`Router::intercept`] to cover every mounted
    /// service, or wrap one service with [`crate::Intercepted`].
    #[must_use]
    pub fn intercept<I: crate::Interceptor>(
        self,
        interceptor: I,
    ) -> Server<crate::Intercepted<S, I>> {
        Server {
            service: Arc::new(crate::Intercepted::from_arc(self.service, interceptor)),
            config: self.config,
        }
    }

    /// Add a second service, switching to path-based routing.
    #[must_use]
    pub fn add_service<T: Service>(self, service: T) -> Router {
        self.into_router().add_service(service)
    }

    /// Move this service into a [`Router`], keeping the configuration.
    #[must_use]
    pub fn into_router(self) -> Router {
        Router::new().config(self.config).add_arc(self.service)
    }

    /// Bind `addr` and serve until the listener fails.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Status> {
        self.serve_listener(bind(addr).await?).await
    }

    /// Serve on an existing listener until it fails.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        self.serve_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve until `shutdown` resolves, then drain.
    ///
    /// Draining stops accepting, sends `GOAWAY` on every live connection, and
    /// waits for in-flight RPCs to finish.
    pub async fn serve_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        accept_loop(
            Arc::new(Single(self.service)),
            listener,
            self.config,
            shutdown,
            None,
        )
        .await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails.
    ///
    /// `path` must not already be bound. This does not unlink a leftover
    /// socket file; use [`Self::serve_unix_unlink`] after a crash. TLS over a
    /// Unix socket is not supported; use [`Self::serve_tls`] on TCP.
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], removing a leftover socket file first if bind
    /// fails with address-in-use.
    ///
    /// If another process is actually listening on `path`, this steals it.
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path)?).await
    }

    /// Serve h2c on an existing Unix listener until it fails.
    #[cfg(unix)]
    pub async fn serve_unix_listener(self, listener: UnixListener) -> Result<(), Status> {
        self.serve_unix_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve h2c on a Unix listener until `shutdown` resolves, then drain.
    /// See [`Self::serve_with_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_with_shutdown(
        self,
        listener: UnixListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        accept_unix_loop(
            Arc::new(Single(self.service)),
            listener,
            self.config,
            shutdown,
        )
        .await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    pub async fn serve_tls(self, addr: SocketAddr, tls: ServerTls) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, std::future::pending(), tls)
            .await
    }

    /// Serve over TLS until `shutdown` resolves, then drain.
    pub async fn serve_tls_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        accept_loop(
            Arc::new(Single(self.service)),
            listener,
            self.config,
            shutdown,
            Some(tls),
        )
        .await
    }

    /// Serve a single already-accepted byte stream until it closes.
    ///
    /// No accept loop, no TLS, no TCP options. Pair with [`crate::Channel::from_io`].
    /// [`Rpc::remote_addr`] is `None`.
    ///
    /// ```no_run
    /// # async fn run(
    /// #     io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    /// # ) -> Result<(), pbrs_grpc::Status> {
    /// # use pbrs_grpc::{Rpc, Server, Service};
    /// # struct Echo;
    /// # impl Service for Echo {
    /// #     const NAME: &'static str = "demo.Echo";
    /// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
    /// # }
    /// Server::new(Echo).serve_connection(io).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn serve_connection<IO>(self, io: IO) -> Result<(), Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        serve_one(Arc::new(Single(self.service)), io, None, self.config).await
    }

    /// Serve connections from `incoming` until it is exhausted or the
    /// listener-side work fails. See [`Incoming`].
    pub async fn serve_with_incoming<I: Incoming>(self, incoming: I) -> Result<(), Status> {
        self.serve_with_incoming_shutdown(incoming, std::future::pending())
            .await
    }

    /// [`Self::serve_with_incoming`] until `shutdown` resolves, then drain.
    pub async fn serve_with_incoming_shutdown<I: Incoming>(
        self,
        incoming: I,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        accept_incoming(
            Arc::new(Single(self.service)),
            incoming,
            self.config,
            shutdown,
        )
        .await
    }
}

/// Newtype so the monomorphic path gets its own [`Dispatch`] impl.
struct Single<S>(Arc<S>);

impl<S: Service> Dispatch for Single<S> {
    fn dispatch(&self, rpc: Rpc) -> impl Future<Output = ()> + Send {
        self.0.call(rpc)
    }
}

/// Serves several services, routing on the service half of the path.
///
/// Routing is a hash lookup on the `/<service>/` prefix plus one boxed future
/// per RPC. Use [`Server`] when you have a single service and want neither.
///
/// ```no_run
/// use pbrs_grpc::Router;
/// # use pbrs_grpc::{Rpc, Service};
/// # struct A; struct B;
/// # impl Service for A {
/// #     const NAME: &'static str = "demo.A";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # impl Service for B {
/// #     const NAME: &'static str = "demo.B";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// Router::new()
///     .add_service(A)
///     .add_service(B)
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await
/// # }
/// ```
#[derive(Default)]
pub struct Router {
    routes: HashMap<&'static str, Arc<dyn DynService>>,
    config: ServerConfig,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut services: Vec<&str> = self.routes.keys().copied().collect();
        services.sort_unstable();
        f.debug_struct("Router")
            .field("services", &services)
            .field("config", &self.config)
            .finish()
    }
}

impl Router {
    /// An empty router with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            config: ServerConfig::default(),
            interceptor: None,
        }
    }

    /// Replace the transport and limit configuration.
    #[must_use]
    pub fn config(mut self, config: ServerConfig) -> Self {
        self.config = config;
        self
    }

    /// Mount `service` at `S::NAME`, replacing any service already there.
    #[must_use]
    pub fn add_service<S: Service>(self, service: S) -> Self {
        self.add_arc(Arc::new(service))
    }

    /// Run `interceptor` before every mounted service. Calling this twice
    /// stacks: the first interceptor runs first.
    #[must_use]
    pub fn intercept<I: crate::Interceptor>(mut self, interceptor: I) -> Self {
        self.interceptor = Some(match self.interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::Then::new(prev, interceptor)),
        });
        self
    }

    fn add_arc<S: Service>(mut self, service: Arc<S>) -> Self {
        self.routes.insert(S::NAME, service);
        self
    }

    /// Mounted service names, in unspecified order.
    pub fn service_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.routes.keys().copied()
    }

    /// Bind `addr` and serve until the listener fails.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Status> {
        self.serve_listener(bind(addr).await?).await
    }

    /// Serve on an existing listener until it fails.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        self.serve_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve until `shutdown` resolves, then drain. See
    /// [`Server::serve_with_shutdown`].
    pub async fn serve_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_loop(Arc::new(self), listener, config, shutdown, None).await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails. See [`Server::serve_unix`].
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], removing a leftover socket file first if bind
    /// fails with address-in-use. See [`Server::serve_unix_unlink`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path)?).await
    }

    /// Serve h2c on an existing Unix listener until it fails.
    #[cfg(unix)]
    pub async fn serve_unix_listener(self, listener: UnixListener) -> Result<(), Status> {
        self.serve_unix_with_shutdown(listener, std::future::pending())
            .await
    }

    /// Serve h2c on a Unix listener until `shutdown` resolves, then drain.
    #[cfg(unix)]
    pub async fn serve_unix_with_shutdown(
        self,
        listener: UnixListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_unix_loop(Arc::new(self), listener, config, shutdown).await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    pub async fn serve_tls(self, addr: SocketAddr, tls: ServerTls) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, std::future::pending(), tls)
            .await
    }

    /// Serve over TLS until `shutdown` resolves, then drain. See
    /// [`Server::serve_with_shutdown`].
    pub async fn serve_tls_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_loop(Arc::new(self), listener, config, shutdown, Some(tls)).await
    }

    /// Serve a single already-accepted byte stream until it closes.
    /// See [`Server::serve_connection`].
    pub async fn serve_connection<IO>(self, io: IO) -> Result<(), Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let config = self.config;
        serve_one(Arc::new(self), io, None, config).await
    }

    /// Serve connections from `incoming` until it is exhausted.
    /// See [`Server::serve_with_incoming`].
    pub async fn serve_with_incoming<I: Incoming>(self, incoming: I) -> Result<(), Status> {
        self.serve_with_incoming_shutdown(incoming, std::future::pending())
            .await
    }

    /// [`Self::serve_with_incoming`] until `shutdown` resolves, then drain.
    pub async fn serve_with_incoming_shutdown<I: Incoming>(
        self,
        incoming: I,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let config = self.config;
        accept_incoming(Arc::new(self), incoming, config, shutdown).await
    }
}

impl Dispatch for Router {
    async fn dispatch(&self, mut rpc: Rpc) {
        if let Some(interceptor) = &self.interceptor {
            if let Err(status) = interceptor.intercept(&mut rpc) {
                return rpc.reject(status);
            }
        }
        match self.routes.get(rpc.service()) {
            Some(service) => service.dispatch(rpc).await,
            None => rpc.unimplemented(),
        }
    }
}

async fn bind(addr: SocketAddr) -> Result<TcpListener, Status> {
    TcpListener::bind(addr)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))
}

#[cfg(unix)]
fn bind_unix(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    UnixListener::bind(path.as_ref()).map_err(|e| Status::unavailable(e.to_string()))
}

#[cfg(unix)]
fn bind_unix_unlink(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    let path = path.as_ref();
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e)
            if e.kind() == std::io::ErrorKind::AddrInUse
                || e.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            std::fs::remove_file(path).map_err(|e| Status::unavailable(e.to_string()))?;
            UnixListener::bind(path).map_err(|e| Status::unavailable(e.to_string()))
        }
        Err(e) => Err(Status::unavailable(e.to_string())),
    }
}

fn connection_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .connection_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// `None` means refuse this peer. `Some(None)` means unlimited. `Some(Some(p))`
/// is a live slot held until the connection task drops it.
fn take_connection_slot(
    slots: &Option<Arc<Semaphore>>,
) -> Option<Option<tokio::sync::OwnedSemaphorePermit>> {
    match slots {
        None => Some(None),
        Some(sem) => sem.clone().try_acquire_owned().ok().map(Some),
    }
}

/// Accept connections until `shutdown` resolves, then drain in-flight work.
async fn accept_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    tls: Option<ServerTls>,
) -> Result<(), Status> {
    // Dropping every clone of `drain_tx` is what tells us the last connection
    // task has finished.
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(listener.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((tcp, peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(tcp);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                let tls = tls.clone();
                drop(tokio::spawn(async move {
                    crate::tcp::tune(&tcp, config.tcp_keepalive_period()).ok();
                    match tls {
                        None => {
                            drop(serve_io(dispatch, tcp, Some(peer), config, goaway).await);
                        }
                        Some(tls) => {
                            let accept = tokio::time::timeout(
                                config.io_handshake_timeout(),
                                tls.accept(tcp),
                            );
                            if let Ok(Ok(io)) = accept.await {
                                drop(serve_io(dispatch, io, Some(peer), config, goaway).await);
                            }
                        }
                    }
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(Status::unavailable(e.to_string()));
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    // Resolves once every connection task has dropped its `drain` clone.
    while drain_rx.recv().await.is_some() {}
    result
}

/// Unix-domain accept loop. Same drain/GOAWAY contract as the TCP accept
/// loop, without TLS or `TCP_NODELAY` (neither applies).
#[cfg(unix)]
async fn accept_unix_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: UnixListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(listener.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((io, _peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(io);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                drop(tokio::spawn(async move {
                    drop(serve_io(dispatch, io, None, config, goaway).await);
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(Status::unavailable(e.to_string()));
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    while drain_rx.recv().await.is_some() {}
    result
}

/// Accept from a custom [`Incoming`] until it is exhausted or `shutdown`
/// resolves, then drain. No TLS, no TCP options — the acceptor already
/// holds a byte stream.
async fn accept_incoming<D: Dispatch, I: Incoming>(
    dispatch: Arc<D>,
    mut incoming: I,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let slots = connection_slots(config);
    let shutdown = std::pin::pin!(shutdown);
    let mut shutdown = Some(shutdown);
    let mut result = Ok(());
    loop {
        let accepted = {
            let accept = std::pin::pin!(incoming.accept());
            let mut accept = Some(accept);
            std::future::poll_fn(|cx| {
                if let Some(fut) = accept.as_mut() {
                    if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                        return Poll::Ready(Some(res));
                    }
                }
                if let Some(fut) = shutdown.as_mut() {
                    if fut.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            })
            .await
        };
        let Some(accepted) = accepted else {
            break;
        };
        let Some(accepted) = accepted else {
            break;
        };
        match accepted {
            Ok((io, peer)) => {
                let Some(permit) = take_connection_slot(&slots) else {
                    drop(io);
                    continue;
                };
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                drop(tokio::spawn(async move {
                    drop(serve_io(dispatch, io, peer, config, goaway).await);
                    drop(permit);
                    drop(drain);
                }));
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    goaway_tx.send(true).ok();
    drop(goaway_tx);
    drop(drain_tx);
    while drain_rx.recv().await.is_some() {}
    result
}

async fn serve_one<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    config: ServerConfig,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (goaway_tx, goaway_rx) = watch::channel(false);
    let result = serve_io(dispatch, io, peer, config, goaway_rx).await;
    drop(goaway_tx);
    result
}

async fn serve_io<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
) -> Result<(), Status>
where
    D: Dispatch,
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut conn = match tokio::time::timeout(
        config.io_handshake_timeout(),
        config.h2_builder().handshake(io),
    )
    .await
    {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => return Err(Status::unavailable(e.to_string())),
        Err(_) => return Err(Status::unavailable("http/2 preface timed out")),
    };
    let (interval, timeout) = config.keepalive();
    let (age, idle, grace) = config.connection_lifetime();
    let age = age.map(|d| crate::config::jitter_age(d, connection_seed(peer)));
    let dead = crate::keepalive::spawn(conn.ping_pong(), interval, timeout);
    let born = tokio::time::Instant::now();
    let mut last_rpc = born;
    let mut draining = false;
    let mut force_close: Option<tokio::time::Instant> = None;
    loop {
        let age_at = age.map(|d| born + d);
        let idle_at = idle.map(|d| last_rpc + d);
        tokio::select! {
            biased;
            accepted = std::future::poll_fn(|cx| conn.poll_accept(cx)) => {
                let Some(Ok((request, respond))) = accepted else {
                    break;
                };
                last_rpc = tokio::time::Instant::now();
                let dispatch = Arc::clone(&dispatch);
                drop(tokio::spawn(async move {
                    dispatch
                        .dispatch(Rpc {
                            request,
                            respond,
                            config,
                            remote_addr: peer,
                            extensions: http::Extensions::new(),
                        })
                        .await;
                }));
            }
            _ = wait_for_drain(goaway.clone()), if !draining => {
                draining = true;
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(age_at), if !draining => {
                draining = true;
                force_close = Some(tokio::time::Instant::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(idle_at), if !draining => {
                draining = true;
                force_close = Some(tokio::time::Instant::now() + grace);
                conn.graceful_shutdown();
            }
            _ = sleep_until_opt(force_close) => {
                break;
            }
            _ = wait_dead(dead.clone()) => {
                break;
            }
        }
    }
    Ok(())
}

async fn sleep_until_opt(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

fn connection_seed(peer: Option<SocketAddr>) -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    match peer {
        Some(SocketAddr::V4(addr)) => u64::from(u32::from(*addr.ip()))
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(u64::from(addr.port()))
            .wrapping_add(n),
        Some(SocketAddr::V6(addr)) => {
            let mut h = n;
            for b in addr.ip().octets() {
                h = h.wrapping_mul(16_777_619).wrapping_add(u64::from(b));
            }
            h.wrapping_add(u64::from(addr.port()))
        }
        None => n,
    }
}

async fn wait_dead(dead: Option<watch::Receiver<bool>>) {
    match dead {
        Some(dead) => crate::keepalive::wait(dead).await,
        None => std::future::pending().await,
    }
}

async fn wait_for_drain(mut goaway: watch::Receiver<bool>) {
    // A dropped sender also means "stop accepting": the accept loop is gone.
    goaway.wait_for(|v| *v).await.ok();
}

#[cfg(test)]
mod tests {
    use super::split_path;

    #[test]
    fn splits_service_and_method() {
        assert_eq!(
            split_path("/helloworld.Greeter/SayHello"),
            ("helloworld.Greeter", "SayHello")
        );
        assert_eq!(split_path("/a.B/C"), ("a.B", "C"));
    }

    #[test]
    fn unparseable_paths_route_nowhere() {
        assert_eq!(split_path("/"), ("", ""));
        assert_eq!(split_path(""), ("", ""));
        assert_eq!(split_path("/nomethod"), ("", ""));
    }
}
