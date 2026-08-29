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
    accepts_gzip, check_request, encode_msg, grpc_trailers, gzip_outbound, let_producer_catch_up,
    read_one_message, reject, reject_request, send_bytes, send_ok_headers, send_trailers_only,
    wrap_timeout, OutBatch, WireStream,
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
use std::time::Duration;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
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
    /// transport has no TCP peer (Unix, in-process). [`Rpc::local_addr`] is
    /// `None` on this path; only the TCP accept loop fills it.
    fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send;
}

/// One inbound RPC, before its call shape has been chosen.
///
/// Consume it with exactly one of [`Self::unary`],
/// [`Self::client_streaming`], [`Self::server_streaming`],
/// [`Self::bidi_streaming`], or [`Self::unimplemented`]. Each one owns the
/// full response: headers, message frames, and `grpc-status` trailers.
///
/// An [`crate::Interceptor`] may mutate [`Self::metadata_mut`], cap the
/// deadline with [`Self::set_timeout`], attach typed state on
/// [`Self::extensions_mut`], or turn the RPC away with [`Self::reject`].
pub struct Rpc {
    request: http::Request<RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
    config: ServerConfig,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    extensions: http::Extensions,
    metadata: Metadata,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for Rpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rpc")
            .field("authority", &self.authority())
            .field("path", &self.path())
            .field("remote_addr", &self.remote_addr)
            .field("local_addr", &self.local_addr)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("peer_timeout", &self.peer_timeout())
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

    /// HTTP/2 `:authority` the peer sent, e.g. `127.0.0.1:50051` or
    /// `localhost` on a Unix socket.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.request
            .uri()
            .authority()
            .map(http::uri::Authority::as_str)
    }

    /// HTTP/2 `:scheme` the peer sent (`http` on h2c, `https` on TLS).
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.request.uri().scheme_str()
    }

    /// Peer address, when the transport exposed one. TCP only; Unix and
    /// [`Server::serve_connection`] yield `None`.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    ///
    /// On TCP this is `TcpStream::local_addr` (the interface the peer hit),
    /// not the listener bind address if that was `0.0.0.0`. Unix,
    /// [`Incoming`], and [`Server::serve_connection`] yield `None`.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Request metadata the handler will see.
    ///
    /// Same map as [`Request::metadata`] after an interceptor returns `Ok`.
    /// Bind it if you need more than one lookup: `let md = rpc.metadata()`.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutate inbound metadata the handler will see.
    ///
    /// Insert, or strip with [`Metadata::remove`] / [`Metadata::remove_bin`].
    /// Reserved keys (`grpc-*`, `content-type`, hop-by-hop headers, ...)
    /// stay on the HTTP request for the kernel; they cannot be inserted or
    /// removed here.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Cap this RPC's deadline. Combined with the client's `grpc-timeout` and
    /// [`ServerConfig::timeout`] as the soonest of the three; an interceptor
    /// can only tighten, not extend. Calling this twice keeps the sooner
    /// value. Values below 1 ms are raised to 1 ms.
    pub fn set_timeout(&mut self, timeout: Duration) {
        let timeout = timeout.max(Duration::from_millis(1));
        self.timeout = Some(match self.timeout {
            Some(prev) => prev.min(timeout),
            None => timeout,
        });
    }

    /// Deadline cap an interceptor set with [`Self::set_timeout`], if any.
    ///
    /// This is not the effective deadline: that also includes the client's
    /// `grpc-timeout` and [`ServerConfig::timeout`]. See
    /// [`Self::effective_timeout`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// The client's `grpc-timeout`, if it sent one.
    ///
    /// Independent of [`Self::timeout`], which is only an interceptor cap.
    /// [`Self::effective_timeout`] is the soonest of this, the server cap,
    /// and the interceptor cap.
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        crate::wire::timeout_from_headers(self.request.headers())
    }

    /// Deadline the handler will run under: the soonest of the client's
    /// `grpc-timeout`, [`ServerConfig::timeout`], and [`Self::set_timeout`].
    #[must_use]
    pub fn effective_timeout(&self) -> Option<Duration> {
        crate::wire::soonest(
            crate::wire::effective_timeout(self.request.headers(), self.config.rpc_timeout()),
            self.timeout,
        )
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
    /// Trailing metadata on `status` and `grpc-status-details-bin` (see
    /// [`Status::with_error_details`]) both ship.
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
    ///     async fn call(&self, mut rpc: Rpc) {
    ///         if rpc.metadata().get("authorization") != Some(self.token.as_str()) {
    ///             return rpc.reject(Status::unauthenticated("bad or missing token"));
    ///         }
    ///         rpc.metadata_mut().remove("authorization");
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
            prefer_gzip,
            peer_accepts_gzip,
            ..
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => {
                send_unary_response(response, respond, wire, prefer_gzip, peer_accepts_gzip).await
            }
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
            prefer_gzip,
            peer_accepts_gzip,
            ..
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => {
                send_unary_response(response, respond, wire, prefer_gzip, peer_accepts_gzip).await
            }
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
            prefer_gzip,
            peer_accepts_gzip,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => {
                send_stream_response(
                    response,
                    respond,
                    wire,
                    deadline,
                    prefer_gzip,
                    peer_accepts_gzip,
                )
                .await
            }
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
            prefer_gzip,
            peer_accepts_gzip,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        match outcome {
            Err(status) => send_trailers_only(&mut respond, status, &Metadata::new()),
            Ok(response) => {
                send_stream_response(
                    response,
                    respond,
                    wire,
                    deadline,
                    prefer_gzip,
                    peer_accepts_gzip,
                )
                .await
            }
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
        let timeout = self.effective_timeout();
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            extensions,
            metadata,
            timeout: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let peer_accepts_gzip = accepts_gzip(request.headers());
        let prefer_gzip = config.compresses_outbound();
        let mut recv = request.into_body();
        let outcome = wrap_timeout(timeout, async {
            let framed = read_one_message::<Req>(&mut recv, limits).await?;
            let mut req = Request::from_metadata(framed.message, metadata, remote_addr, local_addr)
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
            prefer_gzip,
            peer_accepts_gzip,
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
        let timeout = self.effective_timeout();
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            extensions,
            metadata,
            timeout: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let peer_accepts_gzip = accepts_gzip(request.headers());
        let prefer_gzip = config.compresses_outbound();
        let recv = request.into_body();
        // Decoded on the handler's task: no pump task, no queue, and reading
        // is what releases HTTP/2 capacity.
        let stream = Streaming::from_wire(WireStream::<Req>::new(recv, limits, deadline));
        let mut req = Request::from_metadata(stream, metadata, remote_addr, local_addr)
            .with_extensions(extensions);
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
            prefer_gzip,
            peer_accepts_gzip,
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
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
}

async fn send_unary_response<Resp: Serialize>(
    response: Response<Resp>,
    mut respond: h2::server::SendResponse<Bytes>,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
) {
    let (msg, headers, trailers, compress) = response.split();
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let frame = match encode_msg(&msg, gzip, wire.limits) {
        Ok(frame) => frame,
        Err(status) => {
            send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
    };
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip) else {
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
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
) {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let gzip = gzip_outbound(compress, prefer_gzip, peer_accepts_gzip);
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, gzip) else {
        return;
    };
    let mut status = Status::from_code(Code::Ok);
    *status.metadata_mut() = trailers;
    // The deadline has to cover the whole response, not just the handler
    // future: a producer that stops early because *its* deadline expired must
    // not be reported as a clean end of stream.
    let drained = match deadline {
        None => drain_to_wire(&mut stream, &mut send, wire, prefer_gzip, peer_accepts_gzip).await,
        Some(at) => tokio::time::timeout_at(
            at,
            drain_to_wire(&mut stream, &mut send, wire, prefer_gzip, peer_accepts_gzip),
        )
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
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
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
            let mut item = item.map_err(DrainError::Producer)?;
            item.compressed = gzip_outbound(item.compressed, prefer_gzip, peer_accepts_gzip);
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
/// use pbrs_grpc::Server;
/// # use pbrs_grpc::{Rpc, Service};
/// # struct Echo;
/// # impl Service for Echo {
/// #     const NAME: &'static str = "demo.Echo";
/// #     async fn call(&self, rpc: Rpc) { rpc.unimplemented() }
/// # }
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// Server::new(Echo)
///     .max_concurrent_streams(1024)
///     .serve("127.0.0.1:50051".parse().expect("addr"))
///     .await
/// # }
/// ```
pub struct Server<S> {
    service: Arc<S>,
    config: ServerConfig,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
}

impl<S: Service> std::fmt::Debug for Server<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("service", &S::NAME)
            .field("config", &self.config)
            .field("interceptors", &self.interceptor.is_some())
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
            interceptor: None,
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
            interceptor: None,
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

    /// Cap how many RPCs the process will run at once. See
    /// [`ServerConfig::max_concurrent_rpcs`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_rpcs(n);
        self
    }

    /// Cap how many TCP/Unix connections the accept loop will serve at once.
    /// See [`ServerConfig::max_concurrent_connections`].
    #[must_use]
    pub fn max_concurrent_connections(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_connections(n);
        self
    }

    /// Concurrent RPCs allowed per HTTP/2 connection. See
    /// [`ServerConfig::max_concurrent_streams`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.config = self.config.max_concurrent_streams(streams);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`. See
    /// [`ServerConfig::timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// gzip responses when the client advertises gzip. See
    /// [`ServerConfig::send_compressed`].
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// HTTP/2 PING keepalive. See [`ServerConfig::keep_alive_interval`].
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.config = self.config.keep_alive_interval(interval);
        self
    }

    /// How long to wait for a PING acknowledgement. See
    /// [`ServerConfig::keep_alive_timeout`].
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.keep_alive_timeout(timeout);
        self
    }

    /// TCP `SO_KEEPALIVE`. See [`ServerConfig::tcp_keepalive`].
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.config = self.config.tcp_keepalive(time);
        self
    }

    /// Send GOAWAY this long after accept. See [`ServerConfig::max_connection_age`].
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.config = self.config.max_connection_age(age);
        self
    }

    /// Send GOAWAY after this long with no outstanding RPCs. See
    /// [`ServerConfig::max_connection_idle`].
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.config = self.config.max_connection_idle(idle);
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs. See
    /// [`ServerConfig::max_connection_age_grace`].
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.config = self.config.max_connection_age_grace(grace);
        self
    }

    /// Drop a client that never finishes TLS or the HTTP/2 preface. See
    /// [`ServerConfig::handshake_timeout`].
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.handshake_timeout(timeout);
        self
    }

    /// Run `interceptor` before this service sees any RPC.
    ///
    /// Closures implement [`crate::Interceptor`], so
    /// `server.intercept(|rpc| { ... })` is the usual form. The interceptor
    /// can mutate [`Rpc::metadata_mut`], cap the deadline with
    /// [`Rpc::set_timeout`], inspect [`Rpc::peer_timeout`] /
    /// [`Rpc::effective_timeout`] / [`Rpc::authority`] / [`Rpc::scheme`] /
    /// [`Rpc::remote_addr`] / [`Rpc::local_addr`],
    /// attach typed state on [`Rpc::extensions_mut`], or return `Err`
    /// (including [`Status::with_error_details`]) to reject.
    /// Generated servers expose the same method:
    /// `GreeterServer::new(svc).intercept(auth).serve(addr)`.
    /// Calling this twice stacks: the first interceptor runs first, matching
    /// [`Router::intercept`] and [`crate::Channel::intercept`].
    /// On a [`Router`], call [`Router::intercept`] to cover every mounted
    /// service, or wrap one service with [`crate::Intercepted`].
    #[must_use]
    pub fn intercept<I: crate::Interceptor>(mut self, interceptor: I) -> Self {
        self.interceptor = Some(match self.interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(crate::interceptor::Then::new(prev, interceptor)),
        });
        self
    }

    fn into_single(self) -> (Single<S>, ServerConfig) {
        (
            Single {
                service: self.service,
                interceptor: self.interceptor,
            },
            self.config,
        )
    }

    /// Add a second service, switching to path-based routing.
    #[must_use]
    pub fn add_service<T: Service>(self, service: T) -> Router {
        self.into_router().add_service(service)
    }

    /// Move this service into a [`Router`], keeping the configuration and any
    /// interceptors.
    #[must_use]
    pub fn into_router(self) -> Router {
        let mut router = Router::new().config(self.config).add_arc(self.service);
        router.interceptor = self.interceptor;
        router
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
    /// `listener` must already be bound. Draining stops accepting, sends
    /// `GOAWAY` on every live connection, and waits for in-flight RPCs to
    /// finish. To bind an address and then drain, use
    /// [`Self::serve_until_shutdown`].
    pub async fn serve_with_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        let (dispatch, config) = self.into_single();
        accept_loop(Arc::new(dispatch), listener, config, shutdown, None).await
    }

    /// Bind `addr` and serve until `shutdown` resolves, then drain.
    ///
    /// This is the address form of [`Self::serve_with_shutdown`].
    pub async fn serve_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_with_shutdown(bind(addr).await?, shutdown).await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails.
    ///
    /// `path` must not already be bound. This does not unlink a leftover
    /// socket file; use [`Self::serve_unix_unlink`] after a crash. TLS over a
    /// Unix socket is not supported; use [`Self::serve_tls`] on TCP.
    /// To bind and then drain on a signal, use [`Self::serve_unix_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], after unlinking a crash leftover.
    ///
    /// A crash leaves a socket inode that is not accepting. This unlinks that
    /// leftover and binds. If another process is actually listening on `path`,
    /// the file is left alone and this returns [`Code::Unavailable`].
    /// To unlink, bind, and then drain on a signal, use
    /// [`Self::serve_unix_unlink_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path).await?)
            .await
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
        let (dispatch, config) = self.into_single();
        accept_unix_loop(Arc::new(dispatch), listener, config, shutdown).await
    }

    /// Bind `path` and serve h2c until `shutdown` resolves, then drain.
    ///
    /// This is the path form of [`Self::serve_unix_with_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix(path)?, shutdown)
            .await
    }

    /// [`Self::serve_unix_until_shutdown`], after unlinking a crash leftover.
    /// A live listener is left alone. See [`Self::serve_unix_unlink`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix_unlink(path).await?, shutdown)
            .await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    ///
    /// To bind and then drain on a signal, use [`Self::serve_tls_until_shutdown`].
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
        let (dispatch, config) = self.into_single();
        accept_loop(Arc::new(dispatch), listener, config, shutdown, Some(tls)).await
    }

    /// Bind `addr` and serve over TLS until `shutdown` resolves, then drain.
    ///
    /// This is the address form of [`Self::serve_tls_with_shutdown`].
    pub async fn serve_tls_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, shutdown, tls)
            .await
    }

    /// Serve a single already-accepted byte stream until it closes.
    ///
    /// No accept loop, no TLS, no TCP options. Pair with [`crate::Channel::from_io`].
    /// [`Rpc::remote_addr`] and [`Rpc::local_addr`] are `None`.
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
        let (dispatch, config) = self.into_single();
        serve_one(Arc::new(dispatch), io, None, config).await
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
        let (dispatch, config) = self.into_single();
        accept_incoming(Arc::new(dispatch), incoming, config, shutdown).await
    }
}

/// Newtype so the monomorphic path gets its own [`Dispatch`] impl.
struct Single<S> {
    service: Arc<S>,
    interceptor: Option<Arc<dyn crate::Interceptor>>,
}

impl<S: Service> Dispatch for Single<S> {
    async fn dispatch(&self, mut rpc: Rpc) {
        if let Some(interceptor) = &self.interceptor {
            if let Err(status) = interceptor.intercept(&mut rpc) {
                return rpc.reject(status);
            }
        }
        self.service.call(rpc).await;
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
            .field("interceptors", &self.interceptor.is_some())
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

    /// Cap how many RPCs the process will run at once. See
    /// [`ServerConfig::max_concurrent_rpcs`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_rpcs(n);
        self
    }

    /// Cap how many TCP/Unix connections the accept loop will serve at once.
    /// See [`ServerConfig::max_concurrent_connections`].
    #[must_use]
    pub fn max_concurrent_connections(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_connections(n);
        self
    }

    /// Concurrent RPCs allowed per HTTP/2 connection. See
    /// [`ServerConfig::max_concurrent_streams`].
    #[must_use]
    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {
        self.config = self.config.max_concurrent_streams(streams);
        self
    }

    /// Cap every RPC even when the client omits `grpc-timeout`. See
    /// [`ServerConfig::timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// gzip responses when the client advertises gzip. See
    /// [`ServerConfig::send_compressed`].
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// HTTP/2 PING keepalive. See [`ServerConfig::keep_alive_interval`].
    #[must_use]
    pub fn keep_alive_interval(mut self, interval: Duration) -> Self {
        self.config = self.config.keep_alive_interval(interval);
        self
    }

    /// How long to wait for a PING acknowledgement. See
    /// [`ServerConfig::keep_alive_timeout`].
    #[must_use]
    pub fn keep_alive_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.keep_alive_timeout(timeout);
        self
    }

    /// TCP `SO_KEEPALIVE`. See [`ServerConfig::tcp_keepalive`].
    #[must_use]
    pub fn tcp_keepalive(mut self, time: Duration) -> Self {
        self.config = self.config.tcp_keepalive(time);
        self
    }

    /// Send GOAWAY this long after accept. See [`ServerConfig::max_connection_age`].
    #[must_use]
    pub fn max_connection_age(mut self, age: Duration) -> Self {
        self.config = self.config.max_connection_age(age);
        self
    }

    /// Send GOAWAY after this long with no outstanding RPCs. See
    /// [`ServerConfig::max_connection_idle`].
    #[must_use]
    pub fn max_connection_idle(mut self, idle: Duration) -> Self {
        self.config = self.config.max_connection_idle(idle);
        self
    }

    /// After age or idle fires, wait this long for in-flight RPCs. See
    /// [`ServerConfig::max_connection_age_grace`].
    #[must_use]
    pub fn max_connection_age_grace(mut self, grace: Duration) -> Self {
        self.config = self.config.max_connection_age_grace(grace);
        self
    }

    /// Drop a client that never finishes TLS or the HTTP/2 preface. See
    /// [`ServerConfig::handshake_timeout`].
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.handshake_timeout(timeout);
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

    /// Bind `addr` and serve until `shutdown` resolves, then drain. See
    /// [`Server::serve_until_shutdown`].
    pub async fn serve_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_with_shutdown(bind(addr).await?, shutdown).await
    }

    /// Bind `path` and serve h2c over a Unix domain socket until the listener
    /// fails. See [`Server::serve_unix`].
    #[cfg(unix)]
    pub async fn serve_unix(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix(path)?).await
    }

    /// [`Self::serve_unix`], after unlinking a crash leftover. A live listener
    /// is left alone. See [`Server::serve_unix_unlink`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink(self, path: impl AsRef<std::path::Path>) -> Result<(), Status> {
        self.serve_unix_listener(bind_unix_unlink(path).await?)
            .await
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

    /// Bind `path` and serve h2c until `shutdown` resolves, then drain.
    /// See [`Server::serve_unix_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix(path)?, shutdown)
            .await
    }

    /// [`Self::serve_unix_until_shutdown`], after unlinking a crash leftover.
    /// See [`Server::serve_unix_unlink_until_shutdown`].
    #[cfg(unix)]
    pub async fn serve_unix_unlink_until_shutdown(
        self,
        path: impl AsRef<std::path::Path>,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), Status> {
        self.serve_unix_with_shutdown(bind_unix_unlink(path).await?, shutdown)
            .await
    }

    /// Bind `addr` and serve over TLS until the listener fails.
    ///
    /// To bind and then drain on a signal, use [`Self::serve_tls_until_shutdown`].
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

    /// Bind `addr` and serve over TLS until `shutdown` resolves, then drain.
    /// See [`Server::serve_tls_until_shutdown`].
    pub async fn serve_tls_until_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
        tls: ServerTls,
    ) -> Result<(), Status> {
        self.serve_tls_with_shutdown(bind(addr).await?, shutdown, tls)
            .await
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

/// Bind `path`, unlinking a crash leftover. A live listener is left alone.
#[cfg(unix)]
async fn bind_unix_unlink(path: impl AsRef<std::path::Path>) -> Result<UnixListener, Status> {
    let path = path.as_ref();
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e)
            if e.kind() == std::io::ErrorKind::AddrInUse
                || e.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            if unix_path_has_listener(path).await {
                return Err(Status::unavailable(format!(
                    "unix socket {} is already in use",
                    path.display()
                )));
            }
            std::fs::remove_file(path).map_err(|e| Status::unavailable(e.to_string()))?;
            UnixListener::bind(path).map_err(|e| Status::unavailable(e.to_string()))
        }
        Err(e) => Err(Status::unavailable(e.to_string())),
    }
}

/// `true` if some process owns this inode. A crash leftover fails connect
/// with `ConnectionRefused`. A live listener accepts, a full backlog returns
/// `WouldBlock`, and a stuck accept loop times out — all of those are live,
/// so we do not steal.
#[cfg(unix)]
async fn unix_path_has_listener(path: &std::path::Path) -> bool {
    match tokio::time::timeout(
        std::time::Duration::from_millis(50),
        UnixStream::connect(path),
    )
    .await
    {
        Ok(Ok(_stream)) => true,
        Ok(Err(e)) => !unix_connect_means_stale(&e),
        Err(_elapsed) => true,
    }
}

#[cfg(unix)]
fn unix_connect_means_stale(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::AddrNotAvailable
    )
}

fn connection_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .connection_limit()
        .map(|n| Arc::new(Semaphore::new(n)))
}

/// Returns `None` when `max_concurrent_rpcs` is unset. Otherwise a semaphore
/// of that many permits, created once per accept loop so every connection
/// shares the process-wide budget.
fn rpc_slots(config: ServerConfig) -> Option<Arc<Semaphore>> {
    config
        .concurrent_rpc_limit()
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
    let rpcs = rpc_slots(config);
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
                let rpcs = rpcs.clone();
                drop(tokio::spawn(async move {
                    crate::tcp::tune(&tcp, config.tcp_keepalive_period()).ok();
                    let local = tcp.local_addr().ok();
                    match tls {
                        None => {
                            drop(
                                serve_io(dispatch, tcp, Some(peer), local, config, goaway, rpcs)
                                    .await,
                            );
                        }
                        Some(tls) => {
                            let accept = tokio::time::timeout(
                                config.io_handshake_timeout(),
                                tls.accept(tcp),
                            );
                            if let Ok(Ok(io)) = accept.await {
                                drop(
                                    serve_io(dispatch, io, Some(peer), local, config, goaway, rpcs)
                                        .await,
                                );
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
    let rpcs = rpc_slots(config);
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
                let rpcs = rpcs.clone();
                drop(tokio::spawn(async move {
                    drop(serve_io(dispatch, io, None, None, config, goaway, rpcs).await);
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
    let rpcs = rpc_slots(config);
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
                let rpcs = rpcs.clone();
                drop(tokio::spawn(async move {
                    drop(serve_io(dispatch, io, peer, None, config, goaway, rpcs).await);
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
    let result = serve_io(
        dispatch,
        io,
        peer,
        None,
        config,
        goaway_rx,
        rpc_slots(config),
    )
    .await;
    drop(goaway_tx);
    result
}

fn incoming_rpc(
    request: http::Request<RecvStream>,
    respond: h2::server::SendResponse<Bytes>,
    config: ServerConfig,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
) -> Rpc {
    let metadata = Metadata::from_headers(request.headers());
    Rpc {
        request,
        respond,
        config,
        remote_addr,
        local_addr,
        extensions: http::Extensions::new(),
        metadata,
        timeout: None,
    }
}

async fn serve_io<D, IO>(
    dispatch: Arc<D>,
    io: IO,
    peer: Option<SocketAddr>,
    local: Option<SocketAddr>,
    config: ServerConfig,
    goaway: watch::Receiver<bool>,
    rpc_slots: Option<Arc<Semaphore>>,
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
    let busy = crate::keepalive::Busy::new();
    let mut last_idle = born;
    let mut occupied = false;
    let mut draining = false;
    let mut force_close: Option<tokio::time::Instant> = None;
    loop {
        let in_flight = busy.count();
        if in_flight == 0 {
            if occupied {
                last_idle = tokio::time::Instant::now();
                occupied = false;
            }
        } else {
            occupied = true;
        }
        let age_at = age.map(|d| born + d);
        let idle_at = if in_flight == 0 {
            idle.map(|d| last_idle + d)
        } else {
            None
        };
        tokio::select! {
            biased;
            accepted = std::future::poll_fn(|cx| conn.poll_accept(cx)) => {
                let Some(Ok((request, mut respond))) = accepted else {
                    break;
                };
                occupied = true;
                if let Err(err) = check_request(&request) {
                    reject_request(&mut respond, err);
                    continue;
                }
                let permit = match &rpc_slots {
                    None => None,
                    Some(slots) => match slots.clone().try_acquire_owned() {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            reject(
                                &mut respond,
                                Status::resource_exhausted("too many concurrent RPCs"),
                            );
                            continue;
                        }
                    },
                };
                let lease = busy.start();
                let dispatch = Arc::clone(&dispatch);
                drop(tokio::spawn(async move {
                    let _lease = lease;
                    let _permit = permit;
                    dispatch
                        .dispatch(incoming_rpc(request, respond, config, peer, local))
                        .await;
                }));
            }
            _ = busy.notified() => {}
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
            _ = crate::keepalive::wait_opt(dead.clone()) => {
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
