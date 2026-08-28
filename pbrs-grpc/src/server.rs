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
use crate::wire::{
    check_request, encode_msg, grpc_trailers, let_producer_catch_up, read_one_message, reject,
    send_bytes, send_ok_headers, send_trailers_only, timeout_from_headers, wrap_timeout, OutBatch,
    WireStream,
};
use bytes::Bytes;
use h2::RecvStream;
use pbrs::{Parse, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

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
    /// This is how a wrapping [`Service`] turns away an RPC it will not
    /// delegate, for example on failed authentication. Any trailing metadata on
    /// `status` is delivered.
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
        } = self;
        let limits = config.limits();
        if let Err(status) = check_request(&request) {
            reject(&mut respond, status);
            return None;
        }
        let timeout = timeout_from_headers(request.headers());
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let (parts, mut recv) = request.into_parts();
        let outcome = wrap_timeout(timeout, async {
            let framed = read_one_message::<Req>(&mut recv, limits).await?;
            let mut req = Request::from_wire(framed.message, parts.headers, remote_addr);
            req.set_compressed(framed.compressed);
            if let Some(d) = timeout {
                req.set_timeout(d);
            }
            handler(req).await
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
        } = self;
        let limits = config.limits();
        if let Err(status) = check_request(&request) {
            reject(&mut respond, status);
            return None;
        }
        let timeout = timeout_from_headers(request.headers());
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let (parts, recv) = request.into_parts();
        // Decoded on the handler's task: no pump task, no queue, and reading
        // is what releases HTTP/2 capacity.
        let stream = Streaming::from_wire(WireStream::<Req>::new(recv, limits, deadline));
        let mut req = Request::from_wire(stream, parts.headers, remote_addr);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        let outcome = wrap_timeout(timeout, handler(req)).await;
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
        })
    }
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
        accept_loop(Arc::new(self), listener, config, shutdown).await
    }
}

impl Dispatch for Router {
    async fn dispatch(&self, rpc: Rpc) {
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

/// Accept connections until `shutdown` resolves, then drain in-flight work.
async fn accept_loop<D: Dispatch>(
    dispatch: Arc<D>,
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<(), Status> {
    // Dropping every clone of `drain_tx` is what tells us the last connection
    // task has finished.
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let (goaway_tx, goaway_rx) = watch::channel(false);
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
                let dispatch = Arc::clone(&dispatch);
                let goaway = goaway_rx.clone();
                let drain = drain_tx.clone();
                drop(tokio::spawn(async move {
                    serve_conn(dispatch, tcp, Some(peer), config, goaway).await;
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

async fn serve_conn<D: Dispatch>(
    dispatch: Arc<D>,
    tcp: TcpStream,
    peer: Option<SocketAddr>,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
) {
    // Nagle would coalesce a unary response with the next request's ACK and
    // add a full RTT to every small RPC.
    tcp.set_nodelay(true).ok();
    let Ok(mut conn) = config.h2_builder().handshake(tcp).await else {
        return;
    };
    let drain = std::pin::pin!(wait_for_drain(goaway));
    let mut drain = Some(drain);
    loop {
        // `poll_accept` borrows `conn` only for this statement, so the
        // `graceful_shutdown` call below can borrow it again.
        let mut draining = false;
        let accepted = std::future::poll_fn(|cx| {
            if let Poll::Ready(item) = conn.poll_accept(cx) {
                return Poll::Ready(item);
            }
            if let Some(fut) = drain.as_mut() {
                if fut.as_mut().poll(cx).is_ready() {
                    draining = true;
                    return Poll::Ready(None);
                }
            }
            Poll::Pending
        })
        .await;
        if draining {
            // Stop watching, queue GOAWAY, and keep serving in-flight streams
            // until the peer closes.
            drain = None;
            conn.graceful_shutdown();
            continue;
        }
        let Some(Ok((request, respond))) = accepted else {
            break;
        };
        let dispatch = Arc::clone(&dispatch);
        drop(tokio::spawn(async move {
            dispatch
                .dispatch(Rpc {
                    request,
                    respond,
                    config,
                    remote_addr: peer,
                })
                .await;
        }));
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
