//! RPC envelopes: [`Request`], [`Response`], and the cancellable [`Call`].

use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::server::{split_path, PeerCred};
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
/// one, so a proxy can forward what it received — including the method path
/// ([`Self::path`] / [`Self::service`] / [`Self::method`]). Server interceptors
/// mutate inbound metadata through [`crate::Rpc::metadata_mut`] and attach typed
/// values through [`crate::Rpc::extensions_mut`]; the handler reads both
/// from this type. A request you build to send has no path: the channel
/// stamps it from the generated method.
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
#[derive(Clone)]
pub struct Request<T> {
    message: T,
    metadata: Metadata,
    timeout: Option<Duration>,
    compress: Option<bool>,
    compressed: bool,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    peer_identity: Option<PeerIdentity>,
    peer_cred: Option<PeerCred>,
    authority: Option<String>,
    scheme: Option<String>,
    path: Option<String>,
    deadline: Option<tokio::time::Instant>,
    wait_for_ready: Option<bool>,
    limits: Option<MessageLimits>,
    peer_timeout: Option<Duration>,
    accepts_gzip: bool,
    encoding: Option<String>,
    cancel: Option<watch::Receiver<bool>>,
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
            compress: None,
            compressed: false,
            remote_addr: None,
            local_addr: None,
            peer_identity: None,
            peer_cred: None,
            authority: None,
            scheme: None,
            path: None,
            deadline: None,
            wait_for_ready: None,
            limits: None,
            peer_timeout: None,
            accepts_gzip: false,
            encoding: None,
            cancel: None,
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

    /// Split into message and envelope, keeping metadata, deadline,
    /// compression choice, and method path.
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
                peer_cred: self.peer_cred,
                authority: self.authority,
                scheme: self.scheme,
                path: self.path,
                deadline: self.deadline,
                wait_for_ready: self.wait_for_ready,
                limits: self.limits,
                peer_timeout: self.peer_timeout,
                accepts_gzip: self.accepts_gzip,
                encoding: self.encoding,
                cancel: self.cancel,
                extensions: self.extensions,
            },
        )
    }

    /// Rebuild a request around a different message, keeping the envelope.
    ///
    /// This is how a proxy or interceptor rewrites a payload without losing
    /// the caller's metadata, deadline, gzip choice, or method path.
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
            peer_cred: parts.peer_cred,
            authority: parts.authority,
            scheme: parts.scheme,
            path: parts.path,
            deadline: parts.deadline,
            wait_for_ready: parts.wait_for_ready,
            limits: parts.limits,
            peer_timeout: parts.peer_timeout,
            accepts_gzip: parts.accepts_gzip,
            encoding: parts.encoding,
            cancel: parts.cancel,
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

    /// Set the relative timeout. Outbound this becomes `grpc-timeout`.
    ///
    /// Inbound, the kernel stamps the effective remaining duration at
    /// dispatch (client, server cap, interceptor). That value does not
    /// shrink as the handler runs; see [`Self::deadline`] for the absolute
    /// Instant a downstream RPC should inherit.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on this request.
    pub fn clear_timeout(&mut self) {
        self.timeout = None;
    }

    /// Relative timeout stamped at dispatch, if any. See [`Self::set_timeout`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Absolute deadline the server is enforcing, if any.
    ///
    /// Present only on inbound RPCs that have a timeout. Unlike
    /// [`Self::timeout`], this Instant does not depend on how long the
    /// handler has already run, so forwarding
    /// `deadline.saturating_duration_since(tokio::time::Instant::now())`
    /// onto a downstream call preserves the remaining budget.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
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

    /// Whether [`Self::set_wait_for_ready`] has been called.
    ///
    /// Distinct from [`Self::wait_for_ready`], which is `false` when unset.
    /// [`crate::Channel::wait_for_ready`] fills only when this is `false`.
    #[must_use]
    pub fn wait_for_ready_is_set(&self) -> bool {
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
    ///
    /// Passing `false` opts out of a later [`crate::Channel::send_compressed`]
    /// overlay. [`Self::clear_compress`] drops the choice so that overlay
    /// can fill it in.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later [`crate::Channel::send_compressed`]
    /// or interceptor can fill it in. See [`Self::clear_wait_for_ready`].
    pub fn clear_compress(&mut self) {
        self.compress = None;
    }

    /// Whether this request's payload will be gzipped. Outbound only.
    ///
    /// `false` when unset; the channel default is applied in
    /// [`crate::Channel`] before interceptors run.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called.
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset.
    /// [`crate::Channel::send_compressed`] fills only when this is `false`.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// Whether the received unary first frame had the Compressed-Flag set.
    ///
    /// True after inbound unary dispatch when that frame was gzipped. Always
    /// `false` on a client-/bidi-streaming request: each message's flag is
    /// on [`crate::Framed`]. Whether the call itself used gzip is
    /// [`Self::encoding`]. A request you built to send stays `false`.
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
    /// TCP fills this from the accepted socket. Unix, in-process, and the
    /// default [`crate::Incoming`] yield `None`. [`crate::Incoming::peer`]
    /// can fill it. See [`crate::Rpc::local_addr`].
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    ///
    /// Same value as [`crate::Rpc::peer_identity`]. TLS without a client
    /// certificate, h2c, Unix, in-process connections, and the default
    /// [`crate::Incoming`] yield `None`. [`crate::Incoming::peer`] can supply
    /// a chain via [`crate::PeerIdentity::from_der_certs`].
    #[must_use]
    pub fn peer_identity(&self) -> Option<&PeerIdentity> {
        self.peer_identity.as_ref()
    }

    /// Unix-socket peer credentials (`SO_PEERCRED`), when the accept loop
    /// filled them.
    ///
    /// Same value as [`crate::Rpc::peer_cred`]. Same-process tests see this
    /// process's uid/gid/`pid`. TCP, TLS, the default [`crate::Incoming`],
    /// and [`crate::Server::serve_connection`] yield `None`.
    /// [`crate::Incoming::peer`] can supply credentials the acceptor already
    /// probed.
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.peer_cred
    }

    /// The client's own `grpc-timeout`, when the kernel dispatched this call.
    ///
    /// Distinct from [`timeout`](Self::timeout). After interceptors run, that
    /// method is the *effective* cap — the tighter of the client's header and
    /// any interceptor overlay. This method is the client's original duration
    /// so a handler or proxy can log "the client asked 30s, we run under 5s"
    /// or forward the original header. `None` on a request you built to send,
    /// and `None` when the client omitted `grpc-timeout`.
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    ///
    /// `true` after inbound dispatch when the client listed gzip. Kernel
    /// clients always advertise gzip, so a handler talking to
    /// [`crate::Channel`] sees `true` even when the request body itself is
    /// uncompressed. `false` on a request you built to send.
    ///
    /// [`crate::Response::set_compress`] is honoured only when this is
    /// `true`; the kernel silently drops the flag otherwise. Read this
    /// before setting the flag if a handler wants to know whether gzip will
    /// actually go on the wire.
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        self.accepts_gzip
    }

    /// The `grpc-encoding` token the peer used on this call, if any.
    ///
    /// `Some("gzip")` when the request body (unary) or stream (client/bidi)
    /// is gzip-compressed. `None` means identity encoding, or a request you
    /// built to send. Distinct from [`compressed`](Self::compressed): that
    /// is the per-message Compressed-Flag on a unary first frame; this is
    /// the HTTP header that applies to the whole call. Bind it before
    /// [`Self::metadata_mut`]: `let enc = request.encoding();`.
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Whether this inbound RPC has been cancelled.
    ///
    /// True after the client resets the stream, the deadline fires, or the
    /// response has been written (the stream drained, for streaming RPCs).
    /// Always `false` on a request you built to send.
    /// Spawned work should await [`Self::cancelled`] rather than polling this.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|rx| *rx.borrow())
    }

    /// Resolves when this inbound RPC is cancelled.
    ///
    /// The kernel drops the handler future on client RST and on deadline;
    /// work the handler `tokio::spawn`ed keeps running unless it awaits this.
    /// Resolves when the RPC ends: after the response is written (unary) or
    /// the stream drains (streaming), not when the handler function returns.
    /// A server-streaming producer spawned before `Ok(Response::new(stream))`
    /// stays live until that drain. On a request you built to send this never
    /// resolves.
    #[must_use = "cancelled does nothing unless awaited"]
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static {
        when_cancelled(self.cancel.clone())
    }

    pub(crate) fn set_cancel(&mut self, rx: watch::Receiver<bool>) {
        self.cancel = Some(rx);
    }

    /// HTTP/2 `:authority` the peer sent, e.g. `127.0.0.1:50051`.
    ///
    /// Same value as [`crate::Rpc::authority`]. Outbound requests you build
    /// yourself have `None` until the channel stamps its authority on the
    /// wire; this is a server-side field.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// HTTP/2 `:scheme` for this RPC (`http` on h2c, `https` on TLS).
    ///
    /// On TCP and Unix the kernel reports the transport, so a peer cannot
    /// claim `https` on cleartext. The default [`crate::Incoming`] and
    /// [`crate::Server::serve_connection`] keep whatever the peer sent.
    /// [`crate::Incoming::peer`] can set a transport scheme. Same value as
    /// [`crate::Rpc::scheme`].
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// Full gRPC path, e.g. `/helloworld.Greeter/SayHello`.
    ///
    /// Same value as [`crate::Rpc::path`] on an inbound server request.
    /// `None` on a request you built to send: the channel stamps the path
    /// on the wire from the generated method, not from this envelope. Bind it
    /// before [`Self::metadata_mut`]: `let path = request.path();`.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Same split as [`crate::Rpc::service`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`.
    #[must_use]
    pub fn service(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).0)
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Same split as [`crate::Rpc::method`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`.
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).1)
    }

    /// Message caps the kernel is enforcing on this RPC.
    ///
    /// Same value as [`crate::Rpc::limits`] on an inbound server request.
    /// `None` on a request you built to send: the channel's
    /// [`crate::Channel::message_limits`] applies at send time and is not
    /// stored here.
    #[must_use]
    pub fn limits(&self) -> Option<MessageLimits> {
        self.limits
    }

    pub(crate) fn into_parts(self) -> (T, Metadata, Option<Duration>, bool) {
        let compress = self.compress.unwrap_or(false);
        (self.message, self.metadata, self.timeout, compress)
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
            compress: None,
            compressed: false,
            remote_addr,
            local_addr,
            peer_identity,
            peer_cred: None,
            authority: None,
            scheme: None,
            path: None,
            deadline: None,
            wait_for_ready: None,
            limits: None,
            peer_timeout: None,
            accepts_gzip: false,
            encoding: None,
            cancel: None,
            extensions: http::Extensions::new(),
        }
    }

    pub(crate) fn with_http(
        mut self,
        authority: Option<String>,
        scheme: Option<String>,
        path: Option<String>,
    ) -> Self {
        self.authority = authority;
        self.scheme = scheme;
        self.path = path;
        self
    }

    pub(crate) fn set_deadline(&mut self, at: tokio::time::Instant) {
        self.deadline = Some(at);
    }

    pub(crate) fn set_peer_cred(&mut self, cred: Option<PeerCred>) {
        self.peer_cred = cred;
    }

    pub(crate) fn set_limits(&mut self, limits: MessageLimits) {
        self.limits = Some(limits);
    }

    pub(crate) fn set_compressed(&mut self, compressed: bool) {
        self.compressed = compressed;
    }

    pub(crate) fn set_peer_timeout(&mut self, timeout: Option<Duration>) {
        self.peer_timeout = timeout;
    }

    pub(crate) fn set_accepts_gzip(&mut self, accepts: bool) {
        self.accepts_gzip = accepts;
    }

    pub(crate) fn set_encoding(&mut self, encoding: Option<String>) {
        self.encoding = encoding;
    }

    pub(crate) fn with_extensions(mut self, extensions: http::Extensions) -> Self {
        self.extensions = extensions;
        self
    }

    pub(crate) fn outgoing<'a>(
        &'a mut self,
        path: &'static str,
        authority: &'a str,
        https: bool,
        user_agent: &'a str,
        limits: MessageLimits,
    ) -> Outgoing<'a> {
        Outgoing {
            path,
            authority,
            scheme: if https { "https" } else { "http" },
            user_agent,
            limits,
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
/// `:authority`, `:scheme`, `user-agent`, message caps, and the service/method
/// halves of the path, which the interceptor cannot otherwise see. Typed values the
/// caller inserted on [`crate::Request::extensions_mut`] are on this map.
///
/// ```
/// use pbrs_grpc::{Outgoing, Status};
/// use std::time::Duration;
///
/// fn stamp(call: &mut Outgoing<'_>) -> Result<(), Status> {
///     let path = call.path();
///     call.metadata_mut().insert("x-path", path)?;
///     let service = call.service();
///     call.metadata_mut().set("x-service", service)?;
///     let method = call.method();
///     call.metadata_mut().set("x-method", method)?;
///     let authority = call.authority();
///     call.metadata_mut().insert("x-authority", authority)?;
///     let scheme = call.scheme();
///     call.metadata_mut().set("x-scheme", scheme)?;
///     let user_agent = call.user_agent();
///     call.metadata_mut().set("x-ua", user_agent)?;
///     if call.timeout().is_none() {
///         call.set_timeout(Duration::from_secs(5));
///     }
///     if !call.wait_for_ready_is_set() {
///         call.set_wait_for_ready(true);
///     }
///     if !call.compress_is_set() {
///         call.set_compress(true);
///     }
///     Ok(())
/// }
/// # let _ = stamp;
/// ```
pub struct Outgoing<'a> {
    path: &'static str,
    authority: &'a str,
    scheme: &'static str,
    user_agent: &'a str,
    limits: MessageLimits,
    metadata: &'a mut Metadata,
    timeout: &'a mut Option<Duration>,
    wait_for_ready: &'a mut Option<bool>,
    compress: &'a mut Option<bool>,
    extensions: &'a mut http::Extensions,
}

impl<'a> Outgoing<'a> {
    /// The HTTP/2 `:authority` this channel sends, e.g. `127.0.0.1:50051`
    /// or `localhost` on a Unix socket.
    #[must_use]
    pub fn authority(&self) -> &'a str {
        self.authority
    }

    /// HTTP/2 `:scheme` this channel sends.
    ///
    /// Same string as [`crate::Channel::scheme`]: `https` when the channel was
    /// built with [`crate::ClientTls`], or when a [`crate::Channel::from_io`]
    /// clone called [`crate::Channel::https_scheme`]. Otherwise `http`
    /// (cleartext TCP, Unix, and `from_io` without that overlay). Matches
    /// what the kernel writes on the request.
    #[must_use]
    pub fn scheme(&self) -> &'static str {
        self.scheme
    }

    /// The `user-agent` this channel sends, including the kernel suffix.
    ///
    /// Same value as [`crate::Channel::grpc_user_agent`]. A prefix set with
    /// [`crate::Channel::user_agent`] is visible here. Inserting `user-agent`
    /// into metadata succeeds — that name is not reserved — but the kernel
    /// overwrites it after user metadata, so a smuggled value cannot win.
    #[must_use]
    pub fn user_agent(&self) -> &'a str {
        self.user_agent
    }

    /// Message caps this channel will enforce on this RPC.
    ///
    /// Same value as [`crate::ChannelConfig::limits`] after overlays
    /// ([`crate::Channel::message_limits`],
    /// [`crate::Channel::max_decoding_message_size`],
    /// [`crate::Channel::max_encoding_message_size`]). An interceptor cannot
    /// raise them; the kernel applies them when encoding and decoding.
    /// Distinct from [`crate::Request::limits`], which is `None` on a request
    /// you built to send.
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.limits
    }

    /// The full gRPC path, `/<package>.<Service>/<Method>`.
    #[must_use]
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Same split as [`crate::Rpc::service`]. Unparseable paths yield `""`.
    /// Bind it before [`Self::metadata_mut`]: `let svc = call.service();`.
    #[must_use]
    pub fn service(&self) -> &'static str {
        split_path(self.path).0
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Same split as [`crate::Rpc::method`]. Unparseable paths yield `""`.
    /// Bind it before [`Self::metadata_mut`]: `let method = call.method();`.
    #[must_use]
    pub fn method(&self) -> &'static str {
        split_path(self.path).1
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

    /// Relative timeout that becomes `grpc-timeout` on the wire.
    ///
    /// `None` when neither the request nor a channel overlay set one. Fill
    /// that case with [`Self::set_timeout`]. The matching Instant is
    /// [`Self::deadline`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        *self.timeout
    }

    /// Absolute Instant matching [`Self::timeout`].
    ///
    /// Computed when you call this, so an interceptor that just set
    /// [`Self::set_timeout`] sees the new Instant. Same contract as
    /// [`crate::Rpc::deadline`].
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.timeout.map(|d| tokio::time::Instant::now() + d)
    }

    /// Set the relative timeout. Becomes `grpc-timeout` on the wire.
    pub fn set_timeout(&mut self, timeout: Duration) {
        *self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on the request or by an earlier
    /// interceptor.
    pub fn clear_timeout(&mut self) {
        *self.timeout = None;
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    ///
    /// `false` when unset. Use [`Self::wait_for_ready_is_set`] to tell
    /// `None` from an explicit `false`.
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    /// Whether [`Self::set_wait_for_ready`] has been called, including a
    /// channel overlay.
    ///
    /// Distinct from [`Self::wait_for_ready`], which is `false` when unset.
    /// Fill a default only when this is `false`, the same pattern as
    /// [`Self::timeout`] being `None`.
    #[must_use]
    pub fn wait_for_ready_is_set(&self) -> bool {
        self.wait_for_ready.is_some()
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
    ///
    /// `false` when unset. Use [`Self::compress_is_set`] to tell `None`
    /// from an explicit `false`.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called, including a
    /// channel overlay.
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset.
    /// Fill a default only when this is `false`, the same pattern as
    /// [`Self::timeout`] being `None`.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// gzip this request's payload and set the Compressed-Flag.
    ///
    /// Passing `false` opts out of a channel [`crate::Channel::send_compressed`]
    /// overlay.
    pub fn set_compress(&mut self, compress: bool) {
        *self.compress = Some(compress);
    }

    /// Drop a compression choice so a later interceptor or channel
    /// default can fill it in.
    pub fn clear_compress(&mut self) {
        *self.compress = None;
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
            .field("service", &split_path(self.path).0)
            .field("method", &split_path(self.path).1)
            .field("authority", &self.authority)
            .field("scheme", &self.scheme)
            .field("user_agent", &self.user_agent)
            .field("limits", &self.limits)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline())
            .field("wait_for_ready", &self.wait_for_ready)
            .field("compress", &self.compress)
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
    }
}

impl<T: fmt::Debug> fmt::Debug for Request<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("message", &self.message)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline)
            .field("compress", &self.compress)
            .field("compressed", &self.compressed)
            .field("remote_addr", &self.remote_addr)
            .field("local_addr", &self.local_addr)
            .field("peer_identity", &self.peer_identity)
            .field("peer_cred", &self.peer_cred)
            .field("authority", &self.authority)
            .field("scheme", &self.scheme)
            .field("path", &self.path())
            .field("service", &self.service())
            .field("method", &self.method())
            .field("wait_for_ready", &self.wait_for_ready)
            .field("limits", &self.limits)
            .field("peer_timeout", &self.peer_timeout)
            .field("accepts_gzip", &self.accepts_gzip)
            .field("encoding", &self.encoding)
            .field("cancelled", &self.is_cancelled())
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
    }
}

/// A [`Request`] envelope without its message. See
/// [`Request::into_message_and_parts`].
#[derive(Clone)]
pub struct Parts {
    metadata: Metadata,
    timeout: Option<Duration>,
    compress: Option<bool>,
    compressed: bool,
    remote_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    peer_identity: Option<PeerIdentity>,
    peer_cred: Option<PeerCred>,
    authority: Option<String>,
    scheme: Option<String>,
    path: Option<String>,
    deadline: Option<tokio::time::Instant>,
    wait_for_ready: Option<bool>,
    limits: Option<MessageLimits>,
    peer_timeout: Option<Duration>,
    accepts_gzip: bool,
    encoding: Option<String>,
    cancel: Option<watch::Receiver<bool>>,
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

    /// Relative timeout stamped at dispatch, if any. See [`Request::timeout`].
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Set the relative timeout. Outbound this becomes `grpc-timeout`.
    ///
    /// Same as [`Request::set_timeout`]. A proxy that split the envelope
    /// with [`Request::into_message_and_parts`] can tighten the deadline
    /// here without rebuilding a [`Request`] first.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on this envelope.
    /// See [`Request::clear_timeout`].
    pub fn clear_timeout(&mut self) {
        self.timeout = None;
    }

    /// Absolute deadline the server is enforcing, if any. See [`Request::deadline`].
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Whether the payload will be gzipped. Outbound only.
    /// See [`Request::compress`].
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called.
    /// See [`Request::compress_is_set`].
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// gzip this request's payload and set the Compressed-Flag.
    /// See [`Request::set_compress`].
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later channel default can fill it in.
    /// See [`Request::clear_compress`].
    pub fn clear_compress(&mut self) {
        self.compress = None;
    }

    /// Whether the received unary first frame had the Compressed-Flag set.
    /// See [`Request::compressed`].
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compressed
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    /// See [`Request::wait_for_ready`].
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    /// Whether [`Self::set_wait_for_ready`] has been called.
    /// See [`Request::wait_for_ready_is_set`].
    #[must_use]
    pub fn wait_for_ready_is_set(&self) -> bool {
        self.wait_for_ready.is_some()
    }

    /// Queue this RPC until the channel is connected.
    /// See [`Request::set_wait_for_ready`].
    pub fn set_wait_for_ready(&mut self, wait: bool) {
        self.wait_for_ready = Some(wait);
    }

    /// Drop a wait-for-ready choice so a later channel default can fill it in.
    /// See [`Request::clear_wait_for_ready`].
    pub fn clear_wait_for_ready(&mut self) {
        self.wait_for_ready = None;
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

    /// Unix-socket peer credentials, when the accept loop filled them.
    /// See [`Request::peer_cred`].
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.peer_cred
    }

    /// The client's own `grpc-timeout`, when the kernel dispatched this call.
    /// See [`Request::peer_timeout`].
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    /// See [`Request::accepts_gzip`].
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        self.accepts_gzip
    }

    /// The `grpc-encoding` token the peer used on this call, if any.
    /// See [`Request::encoding`].
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Whether this inbound RPC has been cancelled.
    /// See [`Request::is_cancelled`].
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|rx| *rx.borrow())
    }

    /// Resolves when this inbound RPC is cancelled.
    /// See [`Request::cancelled`].
    #[must_use = "cancelled does nothing unless awaited"]
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static {
        when_cancelled(self.cancel.clone())
    }

    /// HTTP/2 `:authority` the peer sent. See [`Request::authority`].
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// HTTP/2 `:scheme` for this RPC. See [`Request::scheme`].
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// Full gRPC path. See [`Request::path`].
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Service half of the path. See [`Request::service`].
    #[must_use]
    pub fn service(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).0)
    }

    /// Method half of the path. See [`Request::method`].
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).1)
    }

    /// Message caps the kernel is enforcing. See [`Request::limits`].
    #[must_use]
    pub fn limits(&self) -> Option<MessageLimits> {
        self.limits
    }
}

impl fmt::Debug for Parts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parts")
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline)
            .field("compress", &self.compress)
            .field("compressed", &self.compressed)
            .field("remote_addr", &self.remote_addr)
            .field("local_addr", &self.local_addr)
            .field("peer_identity", &self.peer_identity)
            .field("peer_cred", &self.peer_cred)
            .field("authority", &self.authority)
            .field("scheme", &self.scheme)
            .field("path", &self.path())
            .field("service", &self.service())
            .field("method", &self.method())
            .field("wait_for_ready", &self.wait_for_ready)
            .field("limits", &self.limits)
            .field("peer_timeout", &self.peer_timeout)
            .field("accepts_gzip", &self.accepts_gzip)
            .field("encoding", &self.encoding)
            .field("cancelled", &self.is_cancelled())
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
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
/// resp.set_compress(true);
/// let (n, mut parts) = resp.into_message_and_parts();
/// assert_eq!(parts.metadata().get("x-cache"), Some("miss"));
/// assert!(parts.compressed());
/// parts.set_compress(false);
/// let resp = Response::from_message_and_parts(n, parts);
/// assert!(!resp.compressed());
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
#[derive(Clone)]
pub struct Response<T> {
    message: T,
    metadata: Metadata,
    trailers: Metadata,
    compress: Option<bool>,
}

impl<T> Response<T> {
    /// Wrap a message with no headers and no trailers.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self {
            message,
            metadata: Metadata::new(),
            trailers: Metadata::new(),
            compress: None,
        }
    }

    /// Take the message.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }

    /// Split into message and envelope, keeping headers, trailers, and
    /// compression.
    ///
    /// Same idea as [`Request::into_message_and_parts`]. Rebuild with
    /// [`Self::from_message_and_parts`].
    #[must_use]
    pub fn into_message_and_parts(self) -> (T, ResponseParts) {
        (
            self.message,
            ResponseParts {
                metadata: self.metadata,
                trailers: self.trailers,
                compress: self.compress,
            },
        )
    }

    /// Rebuild a [`Response`] from [`Self::into_message_and_parts`].
    #[must_use]
    pub fn from_message_and_parts(message: T, parts: ResponseParts) -> Self {
        Self {
            message,
            metadata: parts.metadata,
            trailers: parts.trailers,
            compress: parts.compress,
        }
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
        let (message, parts) = self.into_message_and_parts();
        Response::from_message_and_parts(f(message), parts)
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
    ///
    /// Passing `false` opts out of a later [`crate::Server::send_compressed`]
    /// overlay. [`Self::clear_compress`] drops the choice so that overlay
    /// can fill it in.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later server overlay can fill it in.
    pub fn clear_compress(&mut self) {
        self.compress = None;
    }

    /// Whether this payload will be gzipped. Outbound intent.
    ///
    /// `false` when unset; [`crate::Server::send_compressed`] fills that in
    /// when the peer advertised gzip. Same effective bit as
    /// [`Self::compressed`] after that overlay.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called.
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset.
    /// [`crate::Server::send_compressed`] fills only when this is `false`.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// Whether this payload is gzipped.
    ///
    /// On a response you build, this is [`Self::compress`]. On a received
    /// unary response, it is the Compressed-Flag from the wire. Streaming
    /// payloads report the flag on each [`crate::Framed`] instead.
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    pub(crate) fn from_parts(message: T, metadata: Metadata, trailers: Metadata) -> Self {
        Self {
            message,
            metadata,
            trailers,
            compress: Some(false),
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
            compress: Some(compress),
        }
    }

    pub(crate) fn split(self) -> (T, Metadata, Metadata, Option<bool>) {
        let (message, parts) = self.into_message_and_parts();
        (message, parts.metadata, parts.trailers, parts.compress)
    }
}

/// A [`Response`] envelope without its message. See
/// [`Response::into_message_and_parts`].
#[derive(Clone, Debug)]
pub struct ResponseParts {
    metadata: Metadata,
    trailers: Metadata,
    compress: Option<bool>,
}

impl ResponseParts {
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
    /// See [`Response::set_compress`].
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later server overlay can fill it in.
    /// See [`Response::clear_compress`].
    pub fn clear_compress(&mut self) {
        self.compress = None;
    }

    /// Outbound gzip intent. See [`Response::compress`].
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called.
    /// See [`Response::compress_is_set`].
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// Whether this payload is gzipped. See [`Response::compressed`].
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compress.unwrap_or(false)
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

    /// Whether [`Self::cancel`] has already fired.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
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

    /// Whether [`Self::cancel`] has already fired. See [`Call::is_cancelled`].
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
}

async fn when_cancelled(rx: Option<watch::Receiver<bool>>) {
    let Some(mut rx) = rx else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
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
        req.set_compressed(true);
        req.extensions_mut().insert(7u8);
        req.metadata_mut().insert("k", "v").expect("insert");
        req = req.with_http(
            Some("127.0.0.1:9".into()),
            Some("http".into()),
            Some("/helloworld.Greeter/SayHello".into()),
        );
        let at = tokio::time::Instant::now() + Duration::from_millis(50);
        req.set_deadline(at);
        req.set_peer_timeout(Some(Duration::from_secs(5)));
        req.set_accepts_gzip(true);
        req.set_encoding(Some("gzip".into()));
        let (message, mut parts) = req.into_message_and_parts();
        assert_eq!(message, 1);
        assert!(parts.wait_for_ready());
        assert!(parts.wait_for_ready_is_set());
        assert!(parts.compress());
        assert!(parts.compressed());
        assert_eq!(parts.peer_timeout(), Some(Duration::from_secs(5)));
        assert!(parts.accepts_gzip());
        assert_eq!(parts.encoding(), Some("gzip"));
        assert!(parts.peer_cred().is_none());
        assert!(parts.limits().is_none());
        assert_eq!(parts.authority(), Some("127.0.0.1:9"));
        assert_eq!(parts.scheme(), Some("http"));
        assert_eq!(parts.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(parts.service(), Some("helloworld.Greeter"));
        assert_eq!(parts.method(), Some("SayHello"));
        assert_eq!(parts.deadline(), Some(at));
        assert_eq!(parts.extensions().get::<u8>().copied(), Some(7));
        parts.set_timeout(Duration::from_millis(3));
        parts.set_compress(false);
        parts.clear_wait_for_ready();
        assert!(!parts.wait_for_ready_is_set());
        let shown_parts = format!("{parts:?}");
        assert!(
            shown_parts.contains("/helloworld.Greeter/SayHello"),
            "{shown_parts}"
        );
        assert!(shown_parts.contains("helloworld.Greeter"), "{shown_parts}");
        assert!(shown_parts.contains("SayHello"), "{shown_parts}");
        assert!(shown_parts.contains("peer_timeout: Some("), "{shown_parts}");
        assert!(shown_parts.contains("accepts_gzip: true"), "{shown_parts}");
        assert!(shown_parts.contains("encoding: Some("), "{shown_parts}");
        let rebuilt = Request::<u32>::from_message_and_parts("swapped", parts);
        assert_eq!(rebuilt.timeout(), Some(Duration::from_millis(3)));
        assert_eq!(rebuilt.metadata().get("k"), Some("v"));
        assert!(!rebuilt.wait_for_ready());
        assert!(!rebuilt.compress());
        assert!(rebuilt.compress_is_set());
        assert!(rebuilt.compressed());
        assert_eq!(rebuilt.peer_timeout(), Some(Duration::from_secs(5)));
        assert!(rebuilt.accepts_gzip());
        assert_eq!(rebuilt.encoding(), Some("gzip"));
        assert!(rebuilt.peer_cred().is_none());
        assert!(rebuilt.limits().is_none());
        assert_eq!(rebuilt.authority(), Some("127.0.0.1:9"));
        assert_eq!(rebuilt.scheme(), Some("http"));
        assert_eq!(rebuilt.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(rebuilt.service(), Some("helloworld.Greeter"));
        assert_eq!(rebuilt.method(), Some("SayHello"));
        assert_eq!(rebuilt.deadline(), Some(at));
        assert_eq!(rebuilt.extensions().get::<u8>().copied(), Some(7));
        let shown = format!("{rebuilt:?}");
        assert!(shown.contains("compress: Some(false)"), "{shown}");
        assert!(shown.contains("compressed: true"), "{shown}");
        assert!(shown.contains("deadline: Some("), "{shown}");
        assert!(shown.contains("peer_timeout: Some("), "{shown}");
        assert!(shown.contains("accepts_gzip: true"), "{shown}");
        assert!(shown.contains("/helloworld.Greeter/SayHello"), "{shown}");
        assert!(shown.contains("helloworld.Greeter"), "{shown}");
        assert!(shown.contains("SayHello"), "{shown}");
        let cloned = rebuilt.clone();
        assert_eq!(cloned.peer_timeout(), Some(Duration::from_secs(5)));
        assert_eq!(cloned.encoding(), Some("gzip"));
        assert_eq!(rebuilt.into_inner(), "swapped");
        assert!(Request::new(0u32).path().is_none());
        assert!(Request::new(0u32).service().is_none());
        assert!(Request::new(0u32).method().is_none());
        assert!(Request::new(0u32).peer_timeout().is_none());
        assert!(!Request::new(0u32).accepts_gzip());
        assert!(Request::new(0u32).encoding().is_none());
        let garbage = Request::new(0u32).with_http(None, None, Some("/nomethod".into()));
        assert_eq!(garbage.path(), Some("/nomethod"));
        assert_eq!(garbage.service(), Some(""));
        assert_eq!(garbage.method(), Some(""));
        let mut cleared = Request::new(0u32);
        cleared.set_timeout(Duration::from_secs(1));
        cleared.clear_timeout();
        assert_eq!(cleared.timeout(), None);
        let mut inherit = Request::new(0u32);
        inherit.set_wait_for_ready(false);
        inherit.clear_wait_for_ready();
        assert!(!inherit.wait_for_ready());
        assert!(!inherit.wait_for_ready_is_set());
        inherit.set_compress(false);
        inherit.clear_compress();
        assert!(!inherit.compress());
        assert!(!inherit.compress_is_set());
        assert!(!Request::new(0u32).is_cancelled());
        let (tx, rx) = tokio::sync::watch::channel(false);
        let mut flagged = Request::new(0u32);
        flagged.set_cancel(rx);
        assert!(!flagged.is_cancelled());
        tx.send(true).expect("signal");
        assert!(flagged.is_cancelled());
        let shown = format!("{flagged:?}");
        assert!(shown.contains("cancelled: true"), "{shown}");
        let (_, parts) = flagged.into_message_and_parts();
        assert!(parts.is_cancelled());
        let rebuilt = Request::<u32>::from_message_and_parts(1u32, parts);
        assert!(rebuilt.is_cancelled());
    }

    #[test]
    fn request_map_keeps_metadata() {
        let mut req = Request::new(1u32);
        req.set_timeout(Duration::from_millis(7));
        req.set_compress(true);
        req.metadata_mut().insert("k", "v").expect("insert");
        let mapped = req
            .with_http(
                Some("svc".into()),
                Some("https".into()),
                Some("/svc/Ping".into()),
            )
            .map(|n| n + 1);
        assert_eq!(mapped.timeout(), Some(Duration::from_millis(7)));
        assert_eq!(mapped.metadata().get("k"), Some("v"));
        assert!(mapped.compress());
        assert_eq!(mapped.authority(), Some("svc"));
        assert_eq!(mapped.scheme(), Some("https"));
        assert_eq!(mapped.path(), Some("/svc/Ping"));
        assert_eq!(mapped.service(), Some("svc"));
        assert_eq!(mapped.method(), Some("Ping"));
        assert_eq!(mapped.into_inner(), 2);
    }

    #[test]
    fn response_map_keeps_metadata() {
        let mut resp = Response::new(2u32);
        resp.metadata_mut().insert("h", "v").expect("insert");
        resp.trailers_mut().insert("t", "1").expect("insert");
        resp.set_compress(true);
        let mapped = resp.map(|n| n * 21);
        assert_eq!(mapped.metadata().get("h"), Some("v"));
        assert_eq!(mapped.trailers().get("t"), Some("1"));
        assert!(mapped.compressed());
        assert!(mapped.compress());
        let (n, mut parts) = mapped.into_message_and_parts();
        assert_eq!(n, 42);
        assert!(parts.compress());
        parts.set_compress(false);
        assert!(!parts.compress());
        assert!(parts.compress_is_set());
        let rebuilt = Response::from_message_and_parts(n, parts);
        assert!(!rebuilt.compressed());
        assert!(!rebuilt.compress());
        assert!(rebuilt.compress_is_set());
        assert_eq!(rebuilt.metadata().get("h"), Some("v"));
        assert_eq!(rebuilt.into_inner(), 42);
    }

    #[test]
    fn outgoing_debug_names_path_authority_and_user_metadata() {
        let mut req = Request::new(());
        req.metadata_mut().insert("x-trace", "abc").expect("insert");
        req.set_timeout(Duration::from_secs(1));
        let shown = {
            let call = req.outgoing(
                "/svc/Method",
                "127.0.0.1:1",
                false,
                "pbrs-grpc/test",
                crate::MessageLimits::default(),
            );
            assert_eq!(call.service(), "svc");
            assert_eq!(call.method(), "Method");
            assert_eq!(call.limits(), crate::MessageLimits::default());
            assert_eq!(call.timeout(), Some(Duration::from_secs(1)));
            assert!(call.deadline().is_some());
            assert!(!call.wait_for_ready_is_set());
            format!("{call:?}")
        };
        assert!(shown.contains("/svc/Method"), "{shown}");
        assert!(shown.contains("svc"), "{shown}");
        assert!(shown.contains("Method"), "{shown}");
        assert!(shown.contains("127.0.0.1:1"), "{shown}");
        assert!(shown.contains("http"), "{shown}");
        assert!(shown.contains("pbrs-grpc/test"), "{shown}");
        assert!(shown.contains("x-trace"), "{shown}");
        assert!(shown.contains("abc"), "{shown}");
        assert!(shown.contains("max_decoding"), "{shown}");
        assert!(shown.contains("deadline"), "{shown}");
        let https = req.outgoing(
            "/svc/Method",
            "127.0.0.1:1",
            true,
            "pbrs-grpc/test",
            crate::MessageLimits::default(),
        );
        assert!(format!("{https:?}").contains("https"));
    }

    #[test]
    fn outgoing_deadline_and_wait_for_ready_is_set() {
        let mut req = Request::new(());
        {
            let call = req.outgoing(
                "/svc/Method",
                "127.0.0.1:1",
                false,
                "pbrs-grpc/test",
                crate::MessageLimits::default(),
            );
            assert!(call.timeout().is_none());
            assert!(call.deadline().is_none());
            assert!(!call.wait_for_ready_is_set());
            assert!(!call.wait_for_ready());
            assert!(!call.compress_is_set());
            assert!(!call.compress());
        }
        req.set_timeout(Duration::from_secs(5));
        req.set_wait_for_ready(false);
        {
            let mut call = req.outgoing(
                "/svc/Method",
                "127.0.0.1:1",
                false,
                "pbrs-grpc/test",
                crate::MessageLimits::default(),
            );
            assert_eq!(call.timeout(), Some(Duration::from_secs(5)));
            let at = call.deadline().expect("instant");
            let left = at.saturating_duration_since(tokio::time::Instant::now());
            assert!(left <= Duration::from_secs(5));
            assert!(call.wait_for_ready_is_set());
            assert!(!call.wait_for_ready());
            call.clear_wait_for_ready();
            assert!(!call.wait_for_ready_is_set());
            call.set_compress(false);
            assert!(call.compress_is_set());
            assert!(!call.compress());
            call.clear_compress();
            assert!(!call.compress_is_set());
            call.set_timeout(Duration::from_millis(40));
            let tightened = call.deadline().expect("instant");
            let left = tightened.saturating_duration_since(tokio::time::Instant::now());
            assert!(left <= Duration::from_millis(40));
        }
    }

    #[test]
    fn call_handle_observes_cancel() {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        let call = super::Call::<u32>::new(tx, Box::pin(std::future::pending()));
        let handle = call.handle();
        assert!(!handle.is_cancelled());
        assert!(!call.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
        assert!(call.is_cancelled());
    }
}
