//! One inbound RPC before its call shape is chosen: [`Rpc`].

#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::Incoming;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::Server;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::{IncomingAccept, PeerCred};
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::dispatch::Service;
use super::drain::{
    CancelOnDrop, Prepared, hold_cancel, notify_deadline, run_handler, send_stream_response,
    send_unary_response,
};
#[cfg(feature = "grpc-web")]
use super::drain::{send_web_stream_response, send_web_unary_response};
use super::router::split_path;
use crate::codec::CodecMessage;
use crate::compression::CompressionAlgorithm;
use crate::config::ServerConfig;
use crate::limits::{ByteBudgetTracker, MessageLimits};
use crate::metadata::Metadata;
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{
    CallLabels, CallRole, CancellationEvent, CancellationReason, DiagnosticConfig,
    LifecycleObserver, RejectionEvent, RejectionReason, diagnostic_debug_value,
    diagnostic_identity,
};
use crate::tls::PeerIdentity;
use crate::transport::h2::{RecvStream, SendResponse};
use crate::wire::{
    WireStream, accepts_codec, inbound_codec, read_one_message, send_trailers_only, wrap_timeout,
};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// One inbound RPC, before its call shape has been chosen.
///
/// Consume it with exactly one of [`Self::unary`],
/// [`Self::client_streaming`], [`Self::server_streaming`],
/// [`Self::bidi_streaming`], or [`Self::unimplemented`]. Each one owns the
/// full response: headers, message frames, and `grpc-status` trailers.
///
/// An [`crate::Interceptor`] may mutate [`Self::metadata_mut`], cap the
/// deadline with [`Self::set_timeout`], inspect the server overlay with
/// [`Self::rpc_timeout`], attach typed state on
/// [`Self::extensions_mut`], or turn the RPC away with [`Self::reject`].
pub struct Rpc {
    pub(crate) request: http::Request<RecvStream>,
    pub(crate) respond: SendResponse,
    pub(crate) config: ServerConfig,
    pub(crate) remote_addr: Option<SocketAddr>,
    pub(crate) local_addr: Option<SocketAddr>,
    pub(crate) peer_identity: Option<PeerIdentity>,
    pub(crate) peer_cred: Option<PeerCred>,
    pub(crate) transport_scheme: Option<&'static str>,
    #[cfg(feature = "grpc-web")]
    pub(crate) web: Option<crate::web::Mode>,
    pub(crate) extensions: http::Extensions,
    pub(crate) metadata: Metadata,
    pub(crate) diagnostic_config: Option<DiagnosticConfig>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) response_interceptor: Option<crate::interceptor::ResponseHook>,
    pub(crate) byte_budget: ByteBudgetTracker,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) binlog: Option<crate::binlog::CallLogger>,
    /// Channelz server owning this RPC, for call counters.
    pub(crate) channelz_server: Option<crate::channelz::ServerId>,
    /// Channelz socket serving this RPC, for stream/message counters.
    pub(crate) channelz_socket: Option<crate::channelz::SocketId>,
}

impl std::fmt::Debug for Rpc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config = self.diagnostic_config.as_ref();
        f.debug_struct("Rpc")
            .field(
                "authority",
                &self
                    .authority()
                    .map(|value| diagnostic_identity(value, config)),
            )
            .field("path", &diagnostic_identity(self.path(), config))
            .field("service", &diagnostic_identity(self.service(), config))
            .field("method", &diagnostic_identity(self.method(), config))
            .field(
                "remote_addr",
                &self
                    .remote_addr
                    .as_ref()
                    .map(|value| diagnostic_debug_value(value, config)),
            )
            .field(
                "local_addr",
                &self
                    .local_addr
                    .as_ref()
                    .map(|value| diagnostic_debug_value(value, config)),
            )
            .field(
                "peer_identity",
                &self
                    .peer_identity
                    .as_ref()
                    .map(|value| diagnostic_debug_value(value, config)),
            )
            .field(
                "peer_cred",
                &self
                    .peer_cred
                    .as_ref()
                    .map(|value| diagnostic_debug_value(value, config)),
            )
            .field(
                "scheme",
                &self
                    .scheme()
                    .map(|value| diagnostic_identity(value, config)),
            )
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("rpc_timeout", &self.rpc_timeout())
            .field("peer_timeout", &self.peer_timeout())
            .field("observer", &self.observer.is_some())
            .field("effective_timeout", &self.effective_timeout())
            .field("deadline", &self.deadline())
            .field("limits", &self.limits())
            .field("accepts_gzip", &self.accepts_gzip())
            .field("compresses_outbound", &self.compresses_outbound())
            .field("gzip_level", &self.gzip_level())
            .field("accepts_compressed", &self.accepts_compressed())
            .field("concurrent_rpc_limit", &self.concurrent_rpc_limit())
            .field("send_buffer_size", &self.send_buffer_size())
            .field("byte_budget_allocated", &self.byte_budget.allocated())
            .field(
                "encoding",
                &self
                    .encoding()
                    .map(|value| diagnostic_identity(value, config)),
            )
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
    }
}

impl Rpc {
    /// Full request path, e.g. `/helloworld.Greeter/SayHello`.
    ///
    /// Generated handlers see the same value on [`Request::path`]. Bind it
    /// before [`Self::metadata_mut`]: `let path = rpc.path();`. Visible on
    /// every call shape.
    #[must_use]
    pub fn path(&self) -> &str {
        self.request.uri().path()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Generated handlers see the same value on [`Request::service`].
    /// Applies to every call shape.
    #[must_use]
    pub fn service(&self) -> &str {
        split_path(self.path()).0
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Generated handlers see the same value on [`Request::method`].
    /// Applies to every call shape.
    #[must_use]
    pub fn method(&self) -> &str {
        split_path(self.path()).1
    }

    /// HTTP/2 `:authority` the peer sent, e.g. `127.0.0.1:50051` or
    /// `localhost` on a Unix socket.
    ///
    /// TLS uses the client's [`crate::Target`], not SNI, unless
    /// [`crate::Channel::origin`] overrode `:authority` on that clone.
    /// Applies to every call shape.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.request
            .uri()
            .authority()
            .map(http::uri::Authority::as_str)
    }

    /// HTTP/2 `:scheme` for this RPC (`http` on h2c, `https` on TLS).
    ///
    /// On TCP and Unix this is the transport, not whatever the peer wrote:
    /// a cleartext connection reports `http` even if the preface claimed
    /// `https`. The default [`Incoming`] and [`Server::serve_connection`] keep
    /// the peer's `:scheme`. [`Incoming::peer`] can set a transport scheme.
    /// Applies to every call shape.
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.transport_scheme
            .or_else(|| self.request.uri().scheme_str())
    }

    /// Peer address, when the transport exposed one.
    ///
    /// TCP fills this from accept. [`Incoming`] copies the `SocketAddr` from
    /// [`IncomingAccept`] unless [`Incoming::peer`] replaces it. Unix and
    /// [`Server::serve_connection`] yield `None`. Applies to every call shape.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    ///
    /// On TCP this is `TcpStream::local_addr` (the interface the peer hit),
    /// not the listener bind address if that was `0.0.0.0`. Unix and
    /// [`Server::serve_connection`] yield `None`. The default [`Incoming`]
    /// leaves it unset; [`Incoming::peer`] can fill it. Applies to every call
    /// shape.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    ///
    /// Leaf first, DER-encoded. TLS without client authentication, h2c,
    /// Unix, the default [`Incoming`], and [`Server::serve_connection`] yield
    /// `None`. [`Incoming::peer`] can supply a chain the acceptor already
    /// verified ([`PeerIdentity::from_der_certs`]). The kernel does not parse
    /// X.509. Applies to every call shape.
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.peer_identity.as_ref()
    }

    /// Unix-socket peer credentials (`SO_PEERCRED`), when the accept loop
    /// filled them.
    ///
    /// Same-process tests see this process's uid/gid/`pid`. TCP, TLS, the
    /// default [`Incoming`], and [`Server::serve_connection`] yield `None`.
    /// [`Incoming::peer`] can supply credentials the acceptor already probed.
    /// Applies to every call shape.
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.peer_cred
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

    /// Attach diagnostic configuration to this RPC and its handler request.
    ///
    /// Debug still masks identity unless both consent and raw identity are
    /// enabled; opted-in identity values obey the configured byte limit.
    pub fn set_diagnostic_config(&mut self, config: DiagnosticConfig) -> &mut Self {
        self.diagnostic_config = Some(config);
        self
    }

    /// Cap this RPC's deadline. Combined with the client's `grpc-timeout` and
    /// [`ServerConfig::timeout`] as the soonest of the three; an interceptor
    /// can only tighten, not extend. Calling this twice keeps the sooner
    /// value. Values below 1 ms are raised to 1 ms. This is the handler's
    /// deadline on every call shape.
    ///
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
    /// [`Self::effective_timeout`]. The server overlay itself is
    /// [`Self::rpc_timeout`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Server [`ServerConfig::timeout`] overlay.
    ///
    /// Distinct from [`Self::timeout`] (an interceptor cap),
    /// [`Self::peer_timeout`] (the client's `grpc-timeout`), and
    /// [`Self::effective_timeout`] (the soonest of the three). This is the
    /// server policy even after [`Self::set_timeout`]. Same value as
    /// [`crate::Server::rpc_timeout`]. Generated handlers see it on
    /// [`Request::rpc_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::rpc_timeout`].
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.config.rpc_timeout()
    }

    /// The client's `grpc-timeout`, if it sent one.
    ///
    /// Independent of [`Self::timeout`], which is only an interceptor cap,
    /// and of [`Self::rpc_timeout`], which is the server overlay.
    /// [`Self::effective_timeout`] is the soonest of this, the server cap,
    /// and the interceptor cap. Generated handlers see the same value on
    /// [`Request::peer_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::peer_timeout`].
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        crate::wire::timeout_from_headers(self.request.headers())
    }

    /// Deadline the handler will run under: the soonest of the client's
    /// `grpc-timeout`, [`ServerConfig::timeout`], and [`Self::set_timeout`].
    /// Response interceptors see the same duration on [`crate::Response::timeout`].
    #[must_use]
    pub fn effective_timeout(&self) -> Option<Duration> {
        crate::wire::soonest(
            crate::wire::effective_timeout(self.request.headers(), self.config.rpc_timeout()),
            self.timeout,
        )
    }

    /// Absolute Instant matching [`Self::effective_timeout`].
    ///
    /// Computed when you call this, so an interceptor that just tightened
    /// [`Self::set_timeout`] sees the new Instant. The handler's
    /// [`Request::deadline`] is stamped once when dispatch starts. Visible
    /// on every call shape.
    /// Response interceptors see the same Instant as [`Request::deadline`] on [`crate::Response::deadline`].
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.effective_timeout()
            .map(|d| tokio::time::Instant::now() + d)
    }

    /// Effective message caps for this RPC.
    ///
    /// Same value as [`ServerConfig::limits`]: the inbound decode cap and the
    /// outbound encode cap the kernel enforces when it reads and writes
    /// frames. Default is 4 MiB inbound ([`crate::DEFAULT_MAX_DECODING_MESSAGE_SIZE`])
    /// and unlimited outbound. An interceptor cannot raise them; it can only
    /// inspect, or reject before the body is read. Generated handlers see the
    /// same caps on [`Request::limits`].
    /// Same overlay as [`crate::Server::limits`].
    /// Response interceptors see the same caps on [`crate::Response::limits`].
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.config.limits()
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    ///
    /// Same value as [`Request::accepts_gzip`] after dispatch. A handler that
    /// calls [`crate::Response::set_compress`] still only gzips when this is
    /// true: the kernel will not compress a peer that did not ask.
    /// Response interceptors see the same value on [`crate::Response::accepts_gzip`].
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        crate::wire::accepts_gzip(self.request.headers())
    }

    /// Whether this server gzips responses when the peer advertised gzip.
    ///
    /// Same overlay as [`crate::Server::compresses_outbound`]. A handler
    /// [`crate::Response::set_compress`]`(false)` opts out; unset follows
    /// this default. Generated handlers see the same value on
    /// [`Request::compresses_outbound`].
    /// Response interceptors see the same value on [`crate::Response::compresses_outbound`].
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.config.compresses_outbound()
    }

    /// Configured outbound gzip deflate level.
    ///
    /// Same overlay as [`crate::Server::gzip_level`].
    /// Generated handlers see the same value on [`Request::gzip_level`].
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay.
    /// Response interceptors see the same value on [`crate::Response::gzip_level`].
    /// An interceptor cannot change this; the kernel applies it when encoding.
    /// Applies to every call shape.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.config.gzip_level()
    }

    /// Whether this server inflates inbound gzip. Default `true`.
    ///
    /// Same overlay as [`crate::Server::accepts_compressed`].
    /// Generated handlers see the same value on [`Request::accepts_compressed`].
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay.
    /// Response interceptors see the same value on [`crate::Response::accepts_compressed`].
    /// An interceptor cannot change this; the kernel applies it when decoding.
    /// Applies to every call shape.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.config.accepts_compressed()
    }

    /// Configured process-wide RPC cap, if any.
    ///
    /// Same overlay as [`crate::Server::concurrent_rpc_limit`].
    /// Generated handlers see the same value on [`Request::concurrent_rpc_limit`].
    /// Distinct from [`crate::Outgoing::concurrent_rpc_limit`]: that is a client interceptor overlay.
    /// Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits.
    /// `None` when the server omitted a cap. An interceptor cannot change this; extras are [`Code::ResourceExhausted`] before the handler runs.
    /// Applies to every call shape.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.config.concurrent_rpc_limit()
    }

    /// Configured write-time HTTP/2 send buffer.
    ///
    /// Same overlay as [`crate::Server::send_buffer_size`].
    /// Generated handlers see the same value on [`Request::send_buffer_size`].
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay.
    /// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows: those are handshake SETTINGS, not this write-time threshold.
    /// Response interceptors see the same value on [`crate::Response::send_buffer_size`].
    /// An interceptor cannot change this; the kernel applies it when sending DATA.
    /// Applies to every call shape.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.config.send_buffer_size()
    }

    /// The byte budget tracker in effect for this RPC.
    #[must_use]
    pub fn byte_budget(&self) -> &ByteBudgetTracker {
        &self.byte_budget
    }

    /// Number of bytes currently allocated in transport buffers on this server.
    #[must_use]
    pub fn byte_budget_allocated(&self) -> usize {
        self.byte_budget.allocated()
    }

    /// The peer's `grpc-encoding` token, if it sent a non-identity coding.
    ///
    /// Missing, empty, or an explicit `identity` token is `None` — the spec
    /// treats those as the same coding. `"GZIP"` stays `"GZIP"`. Generated
    /// handlers see the same value on [`Request::encoding`]. `grpc-*` keys
    /// are not in [`Self::metadata`]. Bind it before [`Self::metadata_mut`]:
    /// `let enc = rpc.encoding();`.
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        crate::wire::grpc_encoding(self.request.headers())
    }

    /// Typed values an interceptor may attach for the handler.
    ///
    /// Starts with tonic-style transport connect info when the server has it,
    /// then any [`crate::Interceptor`] (or wrapping [`Service`]) may insert
    /// more values through [`Self::extensions_mut`]. Survives onto the
    /// [`Request`] the handler receives.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values the handler will see on [`Request::extensions`].
    ///
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// Stack `hook` after any [`crate::Server::on_response`] already on this RPC.
    pub(crate) fn push_response_hook(&mut self, hook: crate::interceptor::ResponseHook) {
        self.response_interceptor = Some(match self.response_interceptor.take() {
            None => hook,
            Some(prev) => Arc::new(crate::interceptor::ResponseThen::new(prev, hook)),
        });
    }

    /// Answer with `UNIMPLEMENTED`, naming the path.
    ///
    /// This is the correct default arm of a method `match`: a peer asking for
    /// a method you do not have is a peer error, not a server error.
    pub fn unimplemented(mut self) {
        let status = Status::unimplemented(self.request.uri().path().to_string());
        channelz_start(self.channelz_server, self.channelz_socket);
        if let Some(obs) = &self.observer {
            let labels = CallLabels::new(self.path(), self.authority(), CallRole::Server);
            obs.on_server_call_start(&labels);
            obs.on_rejection(&RejectionEvent {
                call: labels,
                reason: RejectionReason::Unimplemented,
                code: Code::Unimplemented,
            });
            obs.on_server_call_end(&labels, &status, Duration::ZERO);
        }
        if let Some(tap) = &self.binlog {
            tap.log_trailer(&Metadata::new(), &status);
        }
        channelz_end(self.channelz_server, self.channelz_socket, false);
        #[cfg(feature = "grpc-web")]
        if let Some(web) = self.web {
            crate::web::send_trailers_only(&mut self.respond, web, status, &Metadata::new());
            return;
        }
        send_trailers_only(&mut self.respond, status, &Metadata::new());
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
    ///     const ALIASES: &'static [&'static str] = S::ALIASES;
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
        channelz_start(self.channelz_server, self.channelz_socket);
        if let Some(obs) = &self.observer {
            let labels = CallLabels::new(self.path(), self.authority(), CallRole::Server);
            obs.on_server_call_start(&labels);
            obs.on_rejection(&RejectionEvent {
                call: labels,
                reason: RejectionReason::ServerInterceptor,
                code: status.code(),
            });
            obs.on_server_call_end(&labels, &status, Duration::ZERO);
        }
        if let Some(tap) = &self.binlog {
            tap.log_trailer(&Metadata::new(), &status);
        }
        let ok = status.is_ok();
        channelz_end(self.channelz_server, self.channelz_socket, ok);
        #[cfg(feature = "grpc-web")]
        if let Some(web) = self.web {
            crate::web::send_trailers_only(&mut self.respond, web, status, &Metadata::new());
            return;
        }
        send_trailers_only(&mut self.respond, status, &Metadata::new());
    }

    /// Serve a unary method: one request message, one response message.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
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
        Req: CodecMessage,
        Resp: CodecMessage,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel,
            path,
            gzip_level,
            deadline,
            timeout,
            peer_timeout,
            rpc_timeout,
            budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
            web,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            let path_for_labels = observer.as_ref().and_then(|_| path.clone());
            let call_labels = CallLabels::new(
                path_for_labels.as_deref().unwrap_or(""),
                None,
                CallRole::Server,
            );
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => {
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &status, call_start.elapsed());
                    }
                    if let Some(tap) = &binlog {
                        tap.log_trailer(&Metadata::new(), &status);
                    }
                    channelz_end(channelz_server, channelz_socket, status.is_ok());
                    #[cfg(feature = "grpc-web")]
                    if let Some(web) = web {
                        crate::web::send_trailers_only(&mut respond, web, status, &Metadata::new());
                    } else {
                        send_trailers_only(&mut respond, status, &Metadata::new());
                    }
                    #[cfg(not(feature = "grpc-web"))]
                    send_trailers_only(&mut respond, status, &Metadata::new());
                }
                Ok(response) => {
                    #[cfg(feature = "grpc-web")]
                    if let Some(web) = web {
                        send_web_unary_response(
                            response,
                            respond,
                            web,
                            wire,
                            prefer_gzip,
                            peer_accepts_gzip,
                            peer_accepts_deflate,
                            #[cfg(feature = "zstd")]
                            peer_accepts_zstd,
                            &budget,
                            observer.as_deref(),
                            &call_labels,
                            binlog.as_ref(),
                            channelz_socket,
                        )
                        .await;
                    } else {
                        send_unary_response(
                            response,
                            respond,
                            wire,
                            prefer_gzip,
                            peer_accepts_gzip,
                            peer_accepts_deflate,
                            #[cfg(feature = "zstd")]
                            peer_accepts_zstd,
                            &budget,
                            observer.as_deref(),
                            &call_labels,
                            binlog.as_ref(),
                            channelz_socket,
                        )
                        .await;
                    }
                    #[cfg(not(feature = "grpc-web"))]
                    send_unary_response(
                        response,
                        respond,
                        wire,
                        prefer_gzip,
                        peer_accepts_gzip,
                        peer_accepts_deflate,
                        #[cfg(feature = "zstd")]
                        peer_accepts_zstd,
                        &budget,
                        observer.as_deref(),
                        &call_labels,
                        binlog.as_ref(),
                        channelz_socket,
                    )
                    .await;
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &Status::ok(), call_start.elapsed());
                    }
                    channelz_end(channelz_server, channelz_socket, true);
                }
            }
        })
        .await;
    }

    /// Serve a client-streaming method: many request messages, one response.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.client_streaming(|req: Request<Streaming<HelloRequest>>| async move {
    ///     let mut inbound = req.into_inner();
    ///     while inbound.message().await?.is_some() {}
    ///     Ok::<_, Status>(Response::new(HelloReply::new()))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn client_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Resp>, Status>>,
    {
        #[cfg(feature = "grpc-web")]
        if self.web.is_some() {
            self.reject(Status::unimplemented(
                "gRPC-Web supports unary and server-streaming methods",
            ));
            return;
        }
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel,
            path,
            gzip_level,
            deadline,
            timeout,
            peer_timeout,
            rpc_timeout,
            budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
                web: _,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            let path_for_labels = observer.as_ref().and_then(|_| path.clone());
            let call_labels = CallLabels::new(
                path_for_labels.as_deref().unwrap_or(""),
                None,
                CallRole::Server,
            );
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => {
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &status, call_start.elapsed());
                    }
                    if let Some(tap) = &binlog {
                        tap.log_trailer(&Metadata::new(), &status);
                    }
                    channelz_end(channelz_server, channelz_socket, status.is_ok());
                    send_trailers_only(&mut respond, status, &Metadata::new())
                }
                Ok(response) => {
                    send_unary_response(
                        response,
                        respond,
                        wire,
                        prefer_gzip,
                        peer_accepts_gzip,
                        peer_accepts_deflate,
                        #[cfg(feature = "zstd")]
                        peer_accepts_zstd,
                        &budget,
                        observer.as_deref(),
                        &call_labels,
                        binlog.as_ref(),
                        channelz_socket,
                    )
                    .await;
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &Status::ok(), call_start.elapsed());
                    }
                    channelz_end(channelz_server, channelz_socket, true);
                }
            }
        })
        .await;
    }

    /// Serve a server-streaming method: one request message, many responses.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// Spawn the producer before returning the stream. A client RST while
    /// drain waits for the next message aborts the drain so
    /// [`Request::cancelled`] and [`crate::StreamSender::closed`] resolve
    /// without another send.
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.server_streaming(|req: Request<HelloRequest>| async move {
    ///     let (tx, stream) = Streaming::channel(8);
    ///     let mut reply = HelloReply::new();
    ///     reply.set_message(req.get_ref().name());
    ///     tx.send(reply).await.ok();
    ///     Ok::<_, Status>(Response::new(stream))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn server_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: CodecMessage,
        Resp: CodecMessage + Send,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel,
            path,
            gzip_level,
            timeout,
            peer_timeout,
            rpc_timeout,
            budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
            web,
        }) = self.run_unary_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            let path_for_labels = observer.as_ref().and_then(|_| path.clone());
            let call_labels = CallLabels::new(
                path_for_labels.as_deref().unwrap_or(""),
                None,
                CallRole::Server,
            );
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => {
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &status, call_start.elapsed());
                    }
                    if let Some(tap) = &binlog {
                        tap.log_trailer(&Metadata::new(), &status);
                    }
                    channelz_end(channelz_server, channelz_socket, status.is_ok());
                    #[cfg(feature = "grpc-web")]
                    if let Some(web) = web {
                        crate::web::send_trailers_only(&mut respond, web, status, &Metadata::new());
                    } else {
                        send_trailers_only(&mut respond, status, &Metadata::new());
                    }
                    #[cfg(not(feature = "grpc-web"))]
                    send_trailers_only(&mut respond, status, &Metadata::new());
                }
                Ok(response) => {
                    #[cfg(feature = "grpc-web")]
                    let final_status = if let Some(web) = web {
                        send_web_stream_response(
                            response,
                            respond,
                            web,
                            wire,
                            deadline,
                            prefer_gzip,
                            peer_accepts_gzip,
                            peer_accepts_deflate,
                            #[cfg(feature = "zstd")]
                            peer_accepts_zstd,
                            &budget,
                            observer.as_deref(),
                            &call_labels,
                            binlog.as_ref(),
                            channelz_socket,
                        )
                        .await
                    } else {
                        send_stream_response(
                            response,
                            respond,
                            wire,
                            deadline,
                            prefer_gzip,
                            peer_accepts_gzip,
                            peer_accepts_deflate,
                            #[cfg(feature = "zstd")]
                            peer_accepts_zstd,
                            &budget,
                            observer.as_deref(),
                            &call_labels,
                            binlog.as_ref(),
                            channelz_socket,
                        )
                        .await
                    };
                    #[cfg(not(feature = "grpc-web"))]
                    let final_status = send_stream_response(
                        response,
                        respond,
                        wire,
                        deadline,
                        prefer_gzip,
                        peer_accepts_gzip,
                        peer_accepts_deflate,
                        #[cfg(feature = "zstd")]
                        peer_accepts_zstd,
                        &budget,
                        observer.as_deref(),
                        &call_labels,
                        binlog.as_ref(),
                        channelz_socket,
                    )
                    .await;
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &final_status, call_start.elapsed());
                    }
                    channelz_end(channelz_server, channelz_socket, final_status.is_ok());
                }
            }
        })
        .await;
    }

    /// Serve a bidirectional-streaming method.
    ///
    /// Interceptor extensions inserted on this [`Rpc`] are visible on the
    /// handler [`Request`].
    ///
    /// ```
    /// # use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Status, Streaming};
    /// # async fn dispatch(rpc: Rpc) {
    /// rpc.bidi_streaming(|req: Request<Streaming<HelloRequest>>| async move {
    ///     let (tx, outbound) = Streaming::channel(8);
    ///     let mut inbound = req.into_inner();
    ///     while let Some(msg) = inbound.message().await? {
    ///         let mut reply = HelloReply::new();
    ///         reply.set_message(msg.name());
    ///         if tx.send(reply).await.is_err() {
    ///             break;
    ///         }
    ///     }
    ///     Ok::<_, Status>(Response::new(outbound))
    /// })
    /// .await;
    /// # }
    /// ```
    pub async fn bidi_streaming<Req, Resp, F, Fut>(self, handler: F)
    where
        Req: CodecMessage + Send + 'static,
        Resp: CodecMessage + Send,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<Response<Streaming<Resp>>, Status>>,
    {
        #[cfg(feature = "grpc-web")]
        if self.web.is_some() {
            self.reject(Status::unimplemented(
                "gRPC-Web supports unary and server-streaming methods",
            ));
            return;
        }
        let hook = self.response_interceptor.clone();
        let Some(Prepared {
            mut respond,
            wire,
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel,
            path,
            gzip_level,
            timeout,
            peer_timeout,
            rpc_timeout,
            budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
                web: _,
        }) = self.run_streaming_request(handler).await
        else {
            return;
        };
        hold_cancel(cancel, async move {
            let path_for_labels = observer.as_ref().and_then(|_| path.clone());
            let call_labels = CallLabels::new(
                path_for_labels.as_deref().unwrap_or(""),
                None,
                CallRole::Server,
            );
            match outcome.and_then(|response| {
                crate::interceptor::intercept_response(
                    response
                        .with_path(path)
                        .with_gzip_level(gzip_level)
                        .with_compresses_outbound(prefer_gzip)
                        .with_accepts_gzip(peer_accepts_gzip)
                        .with_accepts_compressed(wire.accept_gzip)
                        .with_deadline(deadline)
                        .with_timeout(timeout)
                        .with_peer_timeout(peer_timeout)
                        .with_rpc_timeout(rpc_timeout)
                        .with_limits(Some(wire.limits))
                        .with_send_buffer_size(Some(wire.send_buffer)),
                    hook.as_deref(),
                )
            }) {
                Err(status) => {
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &status, call_start.elapsed());
                    }
                    if let Some(tap) = &binlog {
                        tap.log_trailer(&Metadata::new(), &status);
                    }
                    channelz_end(channelz_server, channelz_socket, status.is_ok());
                    send_trailers_only(&mut respond, status, &Metadata::new())
                }
                Ok(response) => {
                    let final_status = send_stream_response(
                        response,
                        respond,
                        wire,
                        deadline,
                        prefer_gzip,
                        peer_accepts_gzip,
                        peer_accepts_deflate,
                        #[cfg(feature = "zstd")]
                        peer_accepts_zstd,
                        &budget,
                        observer.as_deref(),
                        &call_labels,
                        binlog.as_ref(),
                        channelz_socket,
                    )
                    .await;
                    if let Some(obs) = &observer {
                        obs.on_server_call_end(&call_labels, &final_status, call_start.elapsed());
                    }
                    channelz_end(channelz_server, channelz_socket, final_status.is_ok());
                }
            }
        })
        .await;
    }

    /// Read the single request message, then run `handler` under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_unary_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: CodecMessage,
        F: FnOnce(Request<Req>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let timeout = self.effective_timeout();
        let authority = self.authority().map(str::to_owned);
        let scheme = self.scheme().map(str::to_owned);
        let path = Some(self.path().to_owned());
        let peer_timeout = self.peer_timeout();
        let rpc_timeout = self.rpc_timeout();
        let peer_accepts_gzip = self.accepts_gzip();
        let peer_accepts_deflate =
            accepts_codec(self.request.headers(), CompressionAlgorithm::Deflate);
        #[cfg(feature = "zstd")]
        let peer_accepts_zstd = accepts_codec(self.request.headers(), CompressionAlgorithm::Zstd);
        let request_codec = inbound_codec(self.request.headers());
        let encoding = self.encoding().map(str::to_owned);
        let observer = self.observer.clone();
        let channelz_server = self.channelz_server;
        let channelz_socket = self.channelz_socket;
        #[cfg(feature = "grpc-web")]
        let web = self.web;
        let call_start = tokio::time::Instant::now();
        let owned_labels = observer.as_ref().map(|_| {
            CallLabels::new(
                path.as_deref().unwrap_or(""),
                authority.as_deref(),
                CallRole::Server,
            )
            .to_owned()
        });
        if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
            obs.on_server_call_start(&labels.as_borrowed());
        }
        channelz_start(channelz_server, channelz_socket);
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            peer_identity,
            peer_cred,
            transport_scheme: _,
            #[cfg(feature = "grpc-web")]
                web: _,
            extensions,
            metadata,
            diagnostic_config,
            timeout: _,
            response_interceptor: _,
            byte_budget,
            observer: _,
            binlog,
            channelz_server: _,
            channelz_socket: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let prefer_gzip = config.compresses_outbound();
        let mut recv = request.into_body();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let on_reset = cancel_tx.clone();
        let obs_clone = observer.clone();
        let labels_clone = owned_labels.clone();
        let outcome = wrap_timeout(timeout, async {
            #[cfg(feature = "grpc-web")]
            let framed = if web.is_some_and(crate::web::Mode::is_text) {
                crate::web::read_one_text_message::<Req>(
                    &mut recv,
                    limits,
                    config.accepts_compressed(),
                    request_codec,
                    binlog.as_ref(),
                )
                .await?
            } else {
                read_one_message::<Req>(
                    &mut recv,
                    limits,
                    config.accepts_compressed(),
                    request_codec,
                    binlog.as_ref(),
                    false,
                )
                .await?
            };
            #[cfg(not(feature = "grpc-web"))]
            let framed = read_one_message::<Req>(
                &mut recv,
                limits,
                config.accepts_compressed(),
                request_codec,
                binlog.as_ref(),
                false,
            )
            .await?;
            if let Some(socket) = channelz_socket {
                crate::channelz::Registry::global().note_messages(socket, false, 1);
            }
            if let (Some(obs), Some(labels)) = (&obs_clone, &labels_clone) {
                obs.on_bytes_received(&labels.as_borrowed(), 0);
            }
            let mut req = Request::from_metadata(
                framed.message,
                metadata,
                remote_addr,
                local_addr,
                peer_identity,
            )
            .with_extensions(extensions)
            .with_http(authority, scheme, path.clone());
            if let Some(config) = diagnostic_config {
                req.set_diagnostic_config(config);
            }
            req.set_compressed(framed.compressed);
            req.set_peer_cred(peer_cred);
            req.set_limits(limits);
            req.set_peer_timeout(peer_timeout);
            req.set_rpc_timeout(rpc_timeout);
            req.set_accepts_gzip(peer_accepts_gzip);
            req.set_compresses_outbound(prefer_gzip);
            req.set_gzip_level(config.gzip_level());
            req.set_accepts_compressed(config.accepts_compressed());
            req.set_concurrent_rpc_limit(config.concurrent_rpc_limit());
            req.set_send_buffer_size(config.send_buffer_size());
            req.set_encoding(encoding);
            req.set_cancel(cancel_rx);
            if let Some(d) = timeout {
                req.set_timeout(d);
            }
            if let Some(at) = deadline {
                req.set_deadline(at);
            }
            run_handler(&mut respond, on_reset, handler(req), binlog.as_ref()).await
        })
        .await;
        notify_deadline(&outcome, &cancel_tx);
        if matches!(&outcome, Err(s) if s.code() == Code::DeadlineExceeded) {
            if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
                obs.on_cancellation(&CancellationEvent {
                    call: labels.as_borrowed(),
                    reason: CancellationReason::DeadlineExceeded,
                });
            }
        } else if matches!(&outcome, Err(s) if s.code() == Code::Cancelled) {
            if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
                obs.on_cancellation(&CancellationEvent {
                    call: labels.as_borrowed(),
                    reason: CancellationReason::PeerReset,
                });
            }
        }
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel: CancelOnDrop(cancel_tx),
            path,
            gzip_level: config.gzip_level(),
            timeout,
            peer_timeout,
            rpc_timeout,
            budget: byte_budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
            web,
        })
    }

    /// Hand the request stream to `handler`, under the deadline.
    ///
    /// `None` means the request was rejected and already answered.
    async fn run_streaming_request<Req, T, F, Fut>(self, handler: F) -> Option<Prepared<T>>
    where
        Req: CodecMessage + Send + 'static,
        F: FnOnce(Request<Streaming<Req>>) -> Fut,
        Fut: Future<Output = Result<T, Status>>,
    {
        let timeout = self.effective_timeout();
        let authority = self.authority().map(str::to_owned);
        let scheme = self.scheme().map(str::to_owned);
        let path = Some(self.path().to_owned());
        let peer_timeout = self.peer_timeout();
        let rpc_timeout = self.rpc_timeout();
        let peer_accepts_gzip = self.accepts_gzip();
        let peer_accepts_deflate =
            accepts_codec(self.request.headers(), CompressionAlgorithm::Deflate);
        #[cfg(feature = "zstd")]
        let peer_accepts_zstd = accepts_codec(self.request.headers(), CompressionAlgorithm::Zstd);
        let request_codec = inbound_codec(self.request.headers());
        let encoding = self.encoding().map(str::to_owned);
        let observer = self.observer.clone();
        let channelz_server = self.channelz_server;
        let channelz_socket = self.channelz_socket;
        #[cfg(feature = "grpc-web")]
        let web = self.web;
        let call_start = tokio::time::Instant::now();
        let owned_labels = observer.as_ref().map(|_| {
            CallLabels::new(
                path.as_deref().unwrap_or(""),
                authority.as_deref(),
                CallRole::Server,
            )
            .to_owned()
        });
        if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
            obs.on_server_call_start(&labels.as_borrowed());
        }
        channelz_start(channelz_server, channelz_socket);
        let Self {
            request,
            mut respond,
            config,
            remote_addr,
            local_addr,
            peer_identity,
            peer_cred,
            transport_scheme: _,
            #[cfg(feature = "grpc-web")]
                web: _,
            extensions,
            metadata,
            diagnostic_config,
            timeout: _,
            response_interceptor: _,
            byte_budget,
            observer: _,
            binlog,
            channelz_server: _,
            channelz_socket: _,
        } = self;
        let limits = config.limits();
        let deadline = timeout.map(|d| tokio::time::Instant::now() + d);
        let prefer_gzip = config.compresses_outbound();
        let recv = request.into_body();
        // Decoded on the handler's task: no pump task, no queue, and reading
        // is what releases HTTP/2 capacity.
        let stream = Streaming::from_wire(WireStream::<Req>::new(
            recv,
            limits,
            deadline,
            config.accepts_compressed(),
            request_codec,
            binlog.clone(),
        ))
        // Channelz: count received messages through the handler's polls.
        // Count-only: the terminal arms below own the stream end.
        .bind_channelz_socket_count_only(channelz_socket);
        let mut req =
            Request::from_metadata(stream, metadata, remote_addr, local_addr, peer_identity)
                .with_extensions(extensions)
                .with_http(authority, scheme, path.clone());
        if let Some(config) = diagnostic_config {
            req.set_diagnostic_config(config);
        }
        req.set_peer_cred(peer_cred);
        req.set_limits(limits);
        req.set_peer_timeout(peer_timeout);
        req.set_rpc_timeout(rpc_timeout);
        req.set_accepts_gzip(peer_accepts_gzip);
        req.set_compresses_outbound(prefer_gzip);
        req.set_gzip_level(config.gzip_level());
        req.set_accepts_compressed(config.accepts_compressed());
        req.set_concurrent_rpc_limit(config.concurrent_rpc_limit());
        req.set_send_buffer_size(config.send_buffer_size());
        req.set_encoding(encoding);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        if let Some(at) = deadline {
            req.set_deadline(at);
        }
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let on_reset = cancel_tx.clone();
        req.set_cancel(cancel_rx);
        let outcome = wrap_timeout(timeout, async {
            run_handler(&mut respond, on_reset, handler(req), binlog.as_ref()).await
        })
        .await;
        notify_deadline(&outcome, &cancel_tx);
        if matches!(&outcome, Err(s) if s.code() == Code::DeadlineExceeded) {
            if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
                obs.on_cancellation(&CancellationEvent {
                    call: labels.as_borrowed(),
                    reason: CancellationReason::DeadlineExceeded,
                });
            }
        } else if matches!(&outcome, Err(s) if s.code() == Code::Cancelled) {
            if let (Some(obs), Some(labels)) = (&observer, &owned_labels) {
                obs.on_cancellation(&CancellationEvent {
                    call: labels.as_borrowed(),
                    reason: CancellationReason::PeerReset,
                });
            }
        }
        Some(Prepared {
            respond,
            wire: config.wire(),
            deadline,
            outcome,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            cancel: CancelOnDrop(cancel_tx),
            path,
            gzip_level: config.gzip_level(),
            timeout,
            peer_timeout,
            rpc_timeout,
            budget: byte_budget,
            observer,
            call_start,
            binlog,
            channelz_server,
            channelz_socket,
            #[cfg(feature = "grpc-web")]
            web,
        })
    }
}

/// Channelz: a dispatched RPC started. The socket stream is
/// remote-initiated (`local: false`).
fn channelz_start(
    server: Option<crate::channelz::ServerId>,
    socket: Option<crate::channelz::SocketId>,
) {
    let global = crate::channelz::Registry::global();
    if let Some(server) = server {
        global.note_server_call_started(server);
    }
    if let Some(socket) = socket {
        global.note_stream_started(socket, false);
    }
}

/// Channelz: a dispatched RPC ended with `ok`.
fn channelz_end(
    server: Option<crate::channelz::ServerId>,
    socket: Option<crate::channelz::SocketId>,
    ok: bool,
) {
    let global = crate::channelz::Registry::global();
    if let Some(server) = server {
        global.note_server_call_end(server, ok);
    }
    if let Some(socket) = socket {
        global.note_stream_end(socket, ok);
    }
}
