//! RPC envelopes: [`Request`], [`Response`], and the cancellable [`Call`].

use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::server::{split_path, PeerCred};
use crate::status::Status;
use crate::tls::PeerIdentity;
use futures_core::future::FusedFuture;
use std::borrow::Cow;
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
///
/// ```
/// fn dump_request(request: &pbrs_grpc::Request<()>) {
///     let _ = (
///         request.path(),
///         request.service(),
///         request.method(),
///         request.metadata(),
///         request.timeout(),
///         request.rpc_timeout(),
///         request.peer_timeout(),
///         request.deadline(),
///         request.compress(),
///         request.compressed(),
///         request.encoding(),
///         request.accepts_gzip(),
///         request.compresses_outbound(),
///         request.gzip_level(),
///         request.accepts_compressed(),
///         request.concurrent_rpc_limit(),
///         request.send_buffer_size(),
///         request.remote_addr(),
///         request.local_addr(),
///         request.peer_identity(),
///         request.peer_cred(),
///         request.authority(),
///         request.scheme(),
///         request.wait_for_ready(),
///         request.limits(),
///         request.extensions(),
///         request.user_agent(),
///     );
///     let _ = request.cancelled();
/// }
/// # let _ = dump_request;
/// ```
/// [`Self::user_agent_is_set`] is occupancy on this request envelope, so a later interceptor can prefix only when unset.
/// [`Self::wait_for_ready_is_set`] is occupancy on this request envelope, so a later interceptor can fill wait-for-ready only when unset.
/// [`Self::compress_is_set`] is occupancy on this request envelope, so a later interceptor can fill compress only when unset.
/// [`Self::clear_timeout`] opts out of the channel timeout on this request envelope.
/// [`Self::clear_wait_for_ready`] restores the channel wait-for-ready overlay on this request envelope.
/// [`Self::clear_compress`] restores the channel gzip overlay on this request envelope.
/// [`Self::clear_user_agent`] restores the channel user-agent on this request envelope.
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
    rpc_timeout: Option<Duration>,
    accepts_gzip: bool,
    compresses_outbound: bool,
    gzip_level: u32,
    accepts_compressed: bool,
    concurrent_rpc_limit: Option<usize>,
    send_buffer_size: usize,
    encoding: Option<String>,
    cancel: Option<watch::Receiver<bool>>,
    extensions: http::Extensions,
    /// Call-site [`Request::set_user_agent`] / interceptor
    /// [`Outgoing::set_user_agent`] override. `None` uses the channel value.
    user_agent: Option<http::HeaderValue>,
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
            rpc_timeout: None,
            accepts_gzip: false,
            compresses_outbound: false,
            gzip_level: crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL,
            accepts_compressed: true,
            concurrent_rpc_limit: None,
            send_buffer_size: crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE,
            encoding: None,
            cancel: None,
            extensions: http::Extensions::new(),
            user_agent: None,
        }
    }

    /// Take the message, discarding the envelope.
    ///
    /// Metadata, the deadline, [`Self::cancelled`], peer facts, and extensions
    /// go with it. Use [`Self::into_message_and_parts`] when spawned work
    /// still needs [`Parts::cancelled`].
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

    /// Split into message and envelope, keeping metadata, deadline, cancel,
    /// compression choice, method path, and a [`Self::set_user_agent`] override.
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
                rpc_timeout: self.rpc_timeout,
                accepts_gzip: self.accepts_gzip,
                compresses_outbound: self.compresses_outbound,
                gzip_level: self.gzip_level,
                accepts_compressed: self.accepts_compressed,
                concurrent_rpc_limit: self.concurrent_rpc_limit,
                send_buffer_size: self.send_buffer_size,
                encoding: self.encoding,
                cancel: self.cancel,
                extensions: self.extensions,
                user_agent: self.user_agent,
            },
        )
    }

    /// Rebuild a request around a different message, keeping the envelope.
    ///
    /// This is how a proxy or interceptor rewrites a payload without losing
    /// the caller's metadata, deadline, gzip choice, method path, or
    /// [`Self::set_user_agent`] override.
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
            rpc_timeout: parts.rpc_timeout,
            accepts_gzip: parts.accepts_gzip,
            compresses_outbound: parts.compresses_outbound,
            gzip_level: parts.gzip_level,
            accepts_compressed: parts.accepts_compressed,
            concurrent_rpc_limit: parts.concurrent_rpc_limit,
            send_buffer_size: parts.send_buffer_size,
            encoding: parts.encoding,
            cancel: parts.cancel,
            extensions: parts.extensions,
            user_agent: parts.user_agent,
        }
    }

    /// Replace the message, keeping metadata, deadline, compression,
    /// extensions, and a [`Self::set_user_agent`] override.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Request<U> {
        let (message, parts) = self.into_message_and_parts();
        Request::<U>::from_message_and_parts(f(message), parts)
    }

    /// Request headers, as gRPC metadata.
    ///
    /// Distinct from [`Self::metadata_mut`]: that mutates this envelope; this borrows it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable request headers.
    ///
    /// Distinct from [`Self::metadata`]: that borrows this envelope; this mutates it.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Set the relative timeout. Outbound this becomes `grpc-timeout`.
    ///
    /// Distinct from [`Self::timeout`]: that reads the relative timeout this envelope carries; this writes it.
    ///
    /// Inbound, the kernel stamps the effective remaining duration at
    /// dispatch (client, server cap, interceptor). That value does not
    /// shrink as the handler runs; see [`Self::deadline`] for the absolute
    /// Instant a downstream RPC should inherit.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on this request.
    ///
    /// Distinct from [`Self::set_timeout`]: that writes the relative timeout this envelope carries; this opts out.
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
    /// onto a downstream call preserves the remaining budget. Stamped on
    /// every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`]. The Instant elapses while the handler
    /// runs; [`Self::timeout`] stays the duration stamped at dispatch.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Queue this RPC until the channel is connected instead of failing
    /// immediately with [`crate::Code::Unavailable`].
    ///
    /// Distinct from [`Self::wait_for_ready`]: that reads the wait-for-ready choice this envelope carries; this writes it.
    ///
    /// Pair this with a deadline. Without one, a lazy channel whose
    /// peer never comes up waits until cancellation. The usual source
    /// of a not-yet-connected channel is [`crate::Channel::connect_lazy`].
    /// [`crate::Channel::wait_for_ready`] fills this in when the request
    /// omits it; passing `false` here opts out of that default. Applies to
    /// every call shape.
    pub fn set_wait_for_ready(&mut self, wait: bool) {
        self.wait_for_ready = Some(wait);
    }

    /// Drop a wait-for-ready choice so a later [`crate::Channel::wait_for_ready`]
    /// or interceptor can fill it in. See [`Self::clear_timeout`].
    ///
    /// Distinct from [`Self::set_wait_for_ready`]: that writes the wait-for-ready choice this envelope carries; this opts out.
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
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values this envelope carries; this borrows them.
    /// Empty on a request you built yourself until something inserts into
    /// [`Self::extensions_mut`]. On the server, this is the map an
    /// [`crate::Interceptor`] filled on the [`crate::Rpc`] before the
    /// handler ran. Same map on [`Parts::extensions`] after
    /// [`Self::into_message_and_parts`].
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values for later handlers or interceptors.
    ///
    /// Distinct from [`Self::extensions`]: that borrows them; this inserts typed values this envelope carries.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// gzip this request's payload and set the Compressed-Flag.
    ///
    /// Distinct from [`Self::compress`]: that reads outbound payload gzip on this envelope; this writes it.
    ///
    /// Passing `false` opts out of a later [`crate::Channel::send_compressed`]
    /// overlay on every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`]. [`Self::clear_compress`] drops the choice so that overlay
    /// can fill it in. Applies to every call shape.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later [`crate::Channel::send_compressed`]
    /// or interceptor can fill it in. See [`Self::clear_wait_for_ready`].
    ///
    /// Distinct from [`Self::set_compress`]: that writes outbound payload gzip on this envelope; this opts out.
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

    /// The `user-agent` prefix override on this request, if any.
    ///
    /// `None` uses the channel [`crate::Channel::grpc_user_agent`]. Distinct
    /// from [`crate::Outgoing::user_agent`], which is the effective header
    /// this RPC will send (channel or override). The override only, like
    /// [`Self::timeout`]. Bind it before [`Self::metadata_mut`]:
    /// `let ua = request.user_agent();`.
    #[must_use]
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent
            .as_ref()
            .and_then(|value| value.to_str().ok())
    }

    /// Prefix the kernel `user-agent` on this RPC.
    ///
    /// Distinct from [`Self::user_agent`]: that is the override this envelope carries; this prefixes it.
    ///
    /// Same construction as [`crate::Channel::user_agent`] /
    /// [`crate::Outgoing::set_user_agent`]: `prefix pbrs-grpc/<version>`.
    /// Empty prefix is the kernel identity alone. Invalid HTTP is
    /// [`crate::Code::InvalidArgument`]. Distinct from inserting `user-agent`
    /// into metadata, which the kernel overwrites. Distinct from
    /// [`crate::Outgoing::user_agent`], which is the effective value; this
    /// getter is the override only, like [`Self::timeout`]. An interceptor
    /// [`crate::Outgoing::set_user_agent`] that runs after the call site wins.
    /// [`Self::clear_user_agent`] restores the channel value. Applies to
    /// every call shape.
    pub fn set_user_agent(&mut self, prefix: impl AsRef<str>) -> Result<(), Status> {
        self.user_agent = Some(crate::wire::user_agent_value(prefix.as_ref())?);
        Ok(())
    }

    /// Whether [`Self::set_user_agent`] has already overridden the channel
    /// value. Distinct from [`crate::Outgoing::user_agent_is_set`], which is
    /// the same flag after interceptors run. Applies to every call shape.
    #[must_use]
    pub fn user_agent_is_set(&self) -> bool {
        self.user_agent.is_some()
    }

    /// Drop a [`Self::set_user_agent`] override so this RPC uses the channel
    /// [`crate::Channel::grpc_user_agent`] again. Applies to every call shape.
    ///
    /// Distinct from [`Self::set_user_agent`]: that prefixes this envelope; this restores the channel value.
    pub fn clear_user_agent(&mut self) {
        self.user_agent = None;
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
    /// Applies to every call shape.
    #[must_use]
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    /// Local address of this connection, when the transport exposed one.
    ///
    /// TCP fills this from the accepted socket. Unix, in-process, and the
    /// default [`crate::Incoming`] yield `None`. [`crate::Incoming::peer`]
    /// can fill it. See [`crate::Rpc::local_addr`]. Applies to every call
    /// shape.
    #[must_use]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Client certificate chain from mTLS, when the peer presented one.
    ///
    /// Same value as [`crate::Rpc::peer_identity`]. TLS without a client
    /// certificate, h2c, Unix, in-process connections, and the default
    /// [`crate::Incoming`] yield `None`. [`crate::Incoming::peer`] can supply
    /// a chain via [`crate::PeerIdentity::from_der_certs`]. Applies to every
    /// call shape.
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
    /// probed. Applies to every call shape.
    #[must_use]
    pub fn peer_cred(&self) -> Option<PeerCred> {
        self.peer_cred
    }

    /// The client's own `grpc-timeout`, when the kernel dispatched this call.
    ///
    /// Distinct from [`timeout`](Self::timeout). After interceptors run, that
    /// method is the *effective* cap — the soonest of the client's header, the
    /// server overlay, and any interceptor cap. This method is the client's
    /// original duration so a handler or proxy can log "the client asked 30s,
    /// we run under 5s" or forward the original header. The server overlay is
    /// [`rpc_timeout`](Self::rpc_timeout). `None` on a request you built to
    /// send, and `None` when the client omitted `grpc-timeout`.
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    /// Server [`crate::Server::timeout`] overlay, when the kernel dispatched
    /// this call.
    ///
    /// Distinct from [`timeout`](Self::timeout) (the effective cap at
    /// dispatch) and [`peer_timeout`](Self::peer_timeout) (the client's
    /// `grpc-timeout`). This is the server policy even after an interceptor
    /// tightened [`crate::Rpc::set_timeout`]. Same value as
    /// [`crate::Rpc::rpc_timeout`] / [`crate::Server::rpc_timeout`]. `None`
    /// on a request you built to send, and `None` when the server omitted a
    /// cap.
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.rpc_timeout
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    ///
    /// Distinct from [`Self::accepts_compressed`]: that is the inbound overlay, not the peer advertisement.
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

    /// Whether this server gzips responses when the peer advertised gzip.
    ///
    /// Same overlay as [`crate::Rpc::compresses_outbound`] /
    /// [`crate::Server::compresses_outbound`]. `true` after inbound dispatch
    /// when the server called [`crate::Server::send_compressed`]. `false` on
    /// a request you built to send. [`crate::Response::set_compress`]`(false)`
    /// opts out; unset follows this default. Distinct from [`Self::compress`],
    /// which is the outbound request-payload flag.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.compresses_outbound
    }

    /// Server [`crate::Server::gzip_compression_level`] overlay, when the kernel dispatched this call.
    ///
    /// Same overlay as [`crate::Rpc::gzip_level`] / [`crate::Server::gzip_level`].
    /// Distinct from [`Self::compresses_outbound`]: that is on or off; this is deflate effort.
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay.
    /// [`crate::DEFAULT_GZIP_COMPRESSION_LEVEL`] on a request you built to send.
    /// An interceptor cannot change this; the kernel applies it when encoding.
    ///
    /// ```
    /// let req = pbrs_grpc::Request::new(());
    /// assert_eq!(req.gzip_level(), pbrs_grpc::DEFAULT_GZIP_COMPRESSION_LEVEL);
    /// ```
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.gzip_level
    }

    /// Server [`crate::Server::accept_compressed`] overlay, when the kernel dispatched this call.
    ///
    /// Same overlay as [`crate::Rpc::accepts_compressed`] / [`crate::Server::accepts_compressed`].
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this overlay.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay.
    /// Default `true` on a request you built to send.
    /// An interceptor cannot change this; the kernel applies it when decoding.
    ///
    /// ```
    /// let req = pbrs_grpc::Request::new(());
    /// assert!(req.accepts_compressed());
    /// assert!(!req.accepts_gzip());
    /// ```
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.accepts_compressed
    }

    /// Server [`crate::Server::max_concurrent_rpcs`] overlay, when the kernel dispatched this call.
    ///
    /// Same overlay as [`crate::Rpc::concurrent_rpc_limit`] / [`crate::Server::concurrent_rpc_limit`].
    /// Distinct from [`crate::Outgoing::concurrent_rpc_limit`]: that is a client interceptor overlay.
    /// Distinct from [`Self::limits`]: that is message size, not how many RPCs.
    /// `None` on a request you built to send, and `None` when the server omitted a cap.
    /// An interceptor cannot change this; extras are [`crate::Code::ResourceExhausted`] before the handler runs.
    ///
    /// ```
    /// let req = pbrs_grpc::Request::new(());
    /// assert_eq!(req.concurrent_rpc_limit(), None);
    /// ```
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.concurrent_rpc_limit
    }

    /// Server [`crate::Server::max_send_buffer_size`] overlay, when the kernel dispatched this call.
    ///
    /// Same overlay as [`crate::Rpc::send_buffer_size`] / [`crate::Server::send_buffer_size`].
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay.
    /// Distinct from [`Self::limits`]: that is message size, not this HTTP/2 send buffer.
    /// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows: those are handshake SETTINGS, not this write-time threshold.
    /// Default [`crate::DEFAULT_MAX_SEND_BUFFER_SIZE`] on a request you built to send.
    /// An interceptor cannot change this; the kernel still applies this buffer when writing DATA.
    ///
    /// ```
    /// let req = pbrs_grpc::Request::new(());
    /// assert_eq!(req.send_buffer_size(), pbrs_grpc::DEFAULT_MAX_SEND_BUFFER_SIZE);
    /// ```
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.send_buffer_size
    }

    /// The `grpc-encoding` token the peer used on this call, if any.
    ///
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this inbound `grpc-encoding`.
    /// `Some("gzip")` when the request body (unary) or stream (client/bidi)
    /// is gzip-compressed. `None` means identity — header absent, empty, or
    /// an explicit `identity` token — or a request you built to send. Distinct
    /// from [`compressed`](Self::compressed): that is the per-message
    /// Compressed-Flag on a unary first frame; this is the HTTP header that
    /// applies to the whole call. Bind it before [`Self::metadata_mut`]:
    /// `let enc = request.encoding();`.
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
    /// On client RST and on deadline the kernel signals this, then drops a
    /// handler that is still `Pending`. A handler awaiting this can finish
    /// that await and return. A server timeout fires this when the deadline
    /// wins, not after trailers are written. Work the handler `tokio::spawn`ed
    /// keeps running unless it awaits this.
    ///
    /// Resolves when the RPC ends: after the response is written (unary) or
    /// the stream drains (streaming), not when the handler function returns.
    /// A server-streaming producer spawned before `Ok(Response::new(stream))`
    /// stays live until that drain, including over TLS, mTLS, Unix, and
    /// [`crate::Server::serve_connection`]. A client RST while drain is waiting for
    /// the next message aborts the drain, so this (and
    /// [`crate::StreamSender::closed`]) resolve without another send. On a
    /// request you built to send this never resolves.
    #[must_use = "cancelled does nothing unless awaited"]
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static {
        when_cancelled(self.cancel.clone())
    }

    pub(crate) fn set_cancel(&mut self, rx: watch::Receiver<bool>) {
        self.cancel = Some(rx);
    }

    /// HTTP/2 `:authority` the peer sent, e.g. `127.0.0.1:50051`.
    ///
    /// Same value as [`crate::Rpc::authority`]. TLS uses the client's
    /// [`crate::Target`], not SNI. Outbound requests you build
    /// yourself have `None` until the channel stamps its authority on the
    /// wire; this is a server-side field. Applies to every call shape.
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
    /// [`crate::Rpc::scheme`]. Applies to every call shape.
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// Full gRPC path, e.g. `/helloworld.Greeter/SayHello`.
    ///
    /// Same value as [`crate::Rpc::path`] on an inbound server request.
    /// `None` on a request you built to send: the channel stamps the path
    /// on the wire from the generated method, not from this envelope. Bind it
    /// before [`Self::metadata_mut`]: `let path = request.path();`. Stamped on
    /// every call shape.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Same split as [`crate::Rpc::service`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`. Stamped on every
    /// call shape.
    #[must_use]
    pub fn service(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).0)
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Same split as [`crate::Rpc::method`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`. Stamped on every
    /// call shape.
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).1)
    }

    /// Message caps the kernel is enforcing on this RPC.
    ///
    /// Same value as [`crate::Rpc::limits`] on an inbound server request.
    /// `None` on a request you built to send: the channel's
    /// [`crate::Channel::message_limits`] applies at send time and is not
    /// stored here. Stamped on every call shape.
    #[must_use]
    pub fn limits(&self) -> Option<MessageLimits> {
        self.limits
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        T,
        Metadata,
        Option<Duration>,
        bool,
        Option<http::HeaderValue>,
    ) {
        let compress = self.compress.unwrap_or(false);
        (
            self.message,
            self.metadata,
            self.timeout,
            compress,
            self.user_agent,
        )
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
            rpc_timeout: None,
            accepts_gzip: false,
            compresses_outbound: false,
            gzip_level: crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL,
            accepts_compressed: true,
            concurrent_rpc_limit: None,
            send_buffer_size: crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE,
            encoding: None,
            cancel: None,
            extensions: http::Extensions::new(),
            user_agent: None,
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

    pub(crate) fn set_rpc_timeout(&mut self, timeout: Option<Duration>) {
        self.rpc_timeout = timeout;
    }

    pub(crate) fn set_accepts_gzip(&mut self, accepts: bool) {
        self.accepts_gzip = accepts;
    }

    pub(crate) fn set_compresses_outbound(&mut self, gzip: bool) {
        self.compresses_outbound = gzip;
    }

    pub(crate) fn set_gzip_level(&mut self, level: u32) {
        self.gzip_level = level;
    }

    pub(crate) fn set_accepts_compressed(&mut self, accept: bool) {
        self.accepts_compressed = accept;
    }

    pub(crate) fn set_concurrent_rpc_limit(&mut self, n: Option<usize>) {
        self.concurrent_rpc_limit = n;
    }

    pub(crate) fn set_send_buffer_size(&mut self, bytes: usize) {
        self.send_buffer_size = bytes;
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
        config: crate::ChannelConfig,
    ) -> Outgoing<'a> {
        Outgoing {
            path,
            authority,
            scheme: if https { "https" } else { "http" },
            channel_user_agent: user_agent,
            user_agent: &mut self.user_agent,
            limits: config.limits(),
            rpc_timeout: config.rpc_timeout(),
            waits_for_ready: config.waits_for_ready(),
            compresses_outbound: config.compresses_outbound(),
            accepts_compressed: config.accepts_compressed(),
            gzip_level: config.gzip_level(),
            concurrent_rpc_limit: config.concurrent_rpc_limit(),
            stream_buffer_size: config.stream_buffer_size(),
            send_buffer_size: config.send_buffer_size(),
            metadata: &mut self.metadata,
            timeout: &mut self.timeout,
            wait_for_ready: &mut self.wait_for_ready,
            compress: &mut self.compress,
            extensions: &mut self.extensions,
            connected: false,
        }
    }
}

/// The outbound half of an RPC, as a [`crate::ClientInterceptor`] sees it.
///
/// The request message is not here: interceptors run after the caller has
/// already built it, and object-safe interceptors cannot be generic over it.
/// Everything else an interceptor typically stamps — metadata, deadline,
/// wait-for-ready, compression, typed extensions — is. So is the channel's
/// `:authority`, `:scheme`, `user-agent`, message caps, timeout / wait-for-ready
/// / gzip overlays ([`Self::rpc_timeout`] / [`Self::waits_for_ready`] /
/// [`Self::compresses_outbound`] / [`Self::accepts_compressed`] / [`Self::gzip_level`] / [`Self::concurrent_rpc_limit`] / [`Self::stream_buffer_size`] / [`Self::send_buffer_size`] / [`Self::limits`]), and the service/method halves of the path,
/// which the interceptor cannot otherwise see. Those overlays fill in before
/// interceptors run; [`Self::clear_timeout`] / [`Self::clear_wait_for_ready`] /
/// [`Self::clear_compress`] / [`Self::clear_user_agent`] opt out of an already-applied default.
/// [`Self::set_user_agent`] prefixes this RPC's `user-agent` (kernel suffix
/// stays). Distinct from inserting `user-agent` into metadata, which the
/// kernel overwrites. [`crate::Request::set_user_agent`] is the same prefix
/// at the call site; this method wins if an interceptor runs after.
/// [`Self::user_agent_is_set`] is occupancy on this outbound envelope, so a later interceptor can prefix only when unset.
/// [`Self::wait_for_ready_is_set`] is occupancy on this outbound envelope, so a later interceptor can fill wait-for-ready only when unset.
/// [`Self::compress_is_set`] is occupancy on this outbound envelope, so a later interceptor can fill compress only when unset.
/// [`Self::clear_timeout`] opts out of the channel timeout on this outbound envelope.
/// [`Self::clear_wait_for_ready`] restores the channel wait-for-ready overlay on this outbound envelope.
/// [`Self::clear_compress`] then [`Self::set_compress`] from [`Self::compresses_outbound`] reapplies channel gzip on this outbound envelope.
/// [`Self::clear_user_agent`] restores the channel user-agent on this outbound envelope.
/// Typed values the caller inserted on [`crate::Request::extensions_mut`] are on this map.
/// [`Self::connected`] is whether a pool slot holds a live socket (same
/// snapshot as [`crate::Channel::connected`]), taken when this interceptor
/// runs. Applies to every call shape.
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
///     let _ = (
///         call.path(),
///         call.service(),
///         call.method(),
///         call.authority(),
///         call.scheme(),
///         call.user_agent(),
///         call.user_agent_is_set(),
///         call.metadata(),
///         call.timeout(),
///         call.deadline(),
///         call.rpc_timeout(),
///         call.wait_for_ready(),
///         call.wait_for_ready_is_set(),
///         call.waits_for_ready(),
///         call.compress(),
///         call.compress_is_set(),
///         call.compresses_outbound(),
///         call.accepts_compressed(),
///         call.gzip_level(),
///         call.concurrent_rpc_limit(),
///         call.stream_buffer_size(),
///         call.send_buffer_size(),
///         call.limits(),
///         call.connected(),
///         call.extensions(),
///     );
///     Ok(())
/// }
/// # let _ = stamp;
/// ```
pub struct Outgoing<'a> {
    path: &'static str,
    authority: &'a str,
    scheme: &'static str,
    channel_user_agent: &'a str,
    user_agent: &'a mut Option<http::HeaderValue>,
    limits: MessageLimits,
    rpc_timeout: Option<Duration>,
    waits_for_ready: bool,
    compresses_outbound: bool,
    accepts_compressed: bool,
    gzip_level: u32,
    concurrent_rpc_limit: Option<usize>,
    stream_buffer_size: usize,
    send_buffer_size: usize,
    metadata: &'a mut Metadata,
    timeout: &'a mut Option<Duration>,
    wait_for_ready: &'a mut Option<bool>,
    compress: &'a mut Option<bool>,
    extensions: &'a mut http::Extensions,
    connected: bool,
}

impl<'a> Outgoing<'a> {
    pub(crate) fn with_connected(mut self, connected: bool) -> Self {
        self.connected = connected;
        self
    }

    /// The HTTP/2 `:authority` this channel sends, e.g. `127.0.0.1:50051`
    /// or `localhost` on a Unix socket.
    ///
    /// Distinct from [`crate::Rpc::authority`]: that is the inbound `:authority`; this is the `:authority` this channel sends.
    /// TLS uses the channel [`crate::Target`], not SNI. Applies to every call
    /// shape.
    #[must_use]
    pub fn authority(&self) -> &'a str {
        self.authority
    }

    /// HTTP/2 `:scheme` this channel sends.
    ///
    /// Distinct from [`crate::Rpc::scheme`]: that is the inbound `:scheme`; this is the `:scheme` this channel sends.
    /// Same string as [`crate::Channel::scheme`]: `https` when the channel was
    /// built with [`crate::ClientTls`], or when a [`crate::Channel::from_io`]
    /// clone called [`crate::Channel::https_scheme`]. Otherwise `http`
    /// (cleartext TCP, Unix, and `from_io` without that overlay). Matches
    /// what the kernel writes on the request. Applies to every call shape.
    #[must_use]
    pub fn scheme(&self) -> &'static str {
        self.scheme
    }

    /// The `user-agent` this RPC will send, including the kernel suffix.
    ///
    /// Distinct from [`crate::Request::user_agent`]: that is the override only; this is the effective header this RPC will send.
    /// Same value as [`crate::Channel::grpc_user_agent`] until
    /// [`Self::set_user_agent`]. A prefix set with [`crate::Channel::user_agent`]
    /// is visible here. Inserting `user-agent` into metadata succeeds — that
    /// name is not reserved — but the kernel overwrites it after user
    /// metadata, so a smuggled value cannot win. [`Self::set_user_agent`]
    /// prefixes this RPC. The channel value is borrowed; an override is
    /// owned so an interceptor can stamp it into metadata without holding
    /// `&self` across [`Self::metadata_mut`]. Applies to every call shape.
    #[must_use]
    pub fn user_agent(&self) -> Cow<'a, str> {
        match self
            .user_agent
            .as_ref()
            .and_then(|value| value.to_str().ok())
        {
            Some(text) => Cow::Owned(text.to_owned()),
            None => Cow::Borrowed(self.channel_user_agent),
        }
    }

    /// Prefix the kernel `user-agent` on this RPC.
    ///
    /// Distinct from [`Self::user_agent`]: that is the effective header this RPC will send; this prefixes it.
    /// Same construction as [`crate::Channel::user_agent`]:
    /// `prefix pbrs-grpc/<version>`. Empty prefix is the kernel identity
    /// alone. Invalid HTTP is [`crate::Code::InvalidArgument`]. Distinct from
    /// inserting `user-agent` into metadata, which the kernel overwrites.
    /// [`crate::Request::set_user_agent`] is the same prefix at the call site;
    /// this method wins if an interceptor runs after. [`Self::clear_user_agent`]
    /// restores the channel value. Applies to
    /// every call shape.
    pub fn set_user_agent(&mut self, prefix: impl AsRef<str>) -> Result<(), Status> {
        *self.user_agent = Some(crate::wire::user_agent_value(prefix.as_ref())?);
        Ok(())
    }

    /// Whether [`Self::set_user_agent`] has already overridden the channel
    /// value. Applies to every call shape.
    ///
    /// Distinct from [`Self::user_agent`]: that is the effective header this RPC will send; this is occupancy.
    /// Distinct from [`crate::Request::user_agent_is_set`]: that is the call-site occupancy; this is the same flag after interceptors run.
    #[must_use]
    pub fn user_agent_is_set(&self) -> bool {
        self.user_agent.is_some()
    }

    /// Drop a [`Self::set_user_agent`] override so this RPC uses the channel
    /// [`crate::Channel::grpc_user_agent`] again. Applies to every call shape.
    ///
    /// Distinct from [`Self::set_user_agent`]: that prefixes this RPC; this restores the channel value.
    pub fn clear_user_agent(&mut self) {
        *self.user_agent = None;
    }

    /// Message caps this channel will enforce on this RPC.
    ///
    /// Same value as [`crate::ChannelConfig::limits`] after overlays
    /// ([`crate::Channel::message_limits`],
    /// [`crate::Channel::max_decoding_message_size`],
    /// [`crate::Channel::max_encoding_message_size`]). Same overlay as [`crate::Channel::limits`].
    /// An interceptor cannot
    /// raise them; the kernel applies them when encoding and decoding.
    /// Distinct from [`crate::Request::limits`], which is `None` on a request
    /// you built to send. Applies to every call shape.
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.limits
    }

    /// Channel [`crate::Channel::timeout`] overlay.
    ///
    /// Distinct from [`Self::timeout`]: that is the per-RPC `grpc-timeout`
    /// after the overlay and any interceptor mutation. This is the channel
    /// default even after [`Self::clear_timeout`]. Same value as
    /// [`crate::Channel::rpc_timeout`]. Applies to every call shape.
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.rpc_timeout
    }

    /// Channel [`crate::Channel::wait_for_ready`] overlay.
    ///
    /// Distinct from [`Self::wait_for_ready`]: that is the per-RPC choice
    /// after the overlay and any interceptor mutation. This is the channel
    /// policy even after [`Self::clear_wait_for_ready`]. Same value as
    /// [`crate::Channel::waits_for_ready`]. Applies to every call shape.
    #[must_use]
    pub fn waits_for_ready(&self) -> bool {
        self.waits_for_ready
    }

    /// Channel [`crate::Channel::send_compressed`] overlay.
    ///
    /// Distinct from [`Self::compress`]: that is the per-RPC choice after
    /// the overlay and any interceptor mutation. This is the channel policy
    /// even after [`Self::clear_compress`]. Same value as
    /// [`crate::Channel::compresses_outbound`] / [`crate::Rpc::compresses_outbound`].
    /// Applies to every call shape.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.compresses_outbound
    }

    /// Channel [`crate::Channel::accept_compressed`] overlay.
    ///
    /// Default `true`. Distinct from [`crate::Rpc::accepts_gzip`], which is
    /// the peer's `grpc-accept-encoding`. Same value as
    /// [`crate::Channel::accepts_compressed`]. An interceptor cannot change
    /// it; the kernel already stamped `grpc-accept-encoding` from this.
    /// Applies to every call shape.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.accepts_compressed
    }

    /// Channel [`crate::Channel::gzip_compression_level`] overlay.
    ///
    /// Distinct from [`Self::compresses_outbound`]: that is on or off; this is deflate effort.
    /// Distinct from [`Self::compress`]: that is the per-RPC Compressed-Flag after overlay and interceptor mutation.
    /// An interceptor cannot change this; the kernel applies it when encoding.
    /// Same value as [`crate::Channel::gzip_level`]. Applies to every call shape.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.gzip_level
    }

    /// Channel [`crate::Channel::max_concurrent_rpcs`] overlay.
    ///
    /// Distinct from [`Self::waits_for_ready`]: that waits for a connection; this refuses extras.
    /// Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits.
    /// `None` when unset. An interceptor cannot change this; the kernel refuses extras with [`crate::Code::ResourceExhausted`] before the stream opens.
    /// Same value as [`crate::Channel::concurrent_rpc_limit`]. Applies to every call shape.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.concurrent_rpc_limit
    }

    /// Channel [`crate::Channel::stream_buffer`] overlay.
    ///
    /// Distinct from [`Self::limits`]: that is message size, not how many messages sit in the outbound queue.
    /// Distinct from received streams: those are decoded inline and have no queue.
    /// Applies to client-streaming and bidi request streams. Unary and server-streaming have no request stream to queue.
    /// An interceptor cannot change this; the kernel applies it when opening the request stream.
    /// Same value as [`crate::Channel::stream_buffer_size`]. Default [`crate::DEFAULT_STREAM_BUFFER`].
    #[must_use]
    pub fn stream_buffer_size(&self) -> usize {
        self.stream_buffer_size
    }

    /// Channel [`crate::Channel::max_send_buffer_size`] overlay.
    ///
    /// Distinct from [`Self::limits`]: that is uncompressed protobuf bytes, not this HTTP/2 send buffer.
    /// Distinct from [`Self::stream_buffer_size`]: that is decoded-message queue depth, not this send buffer.
    /// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows: those are handshake flow control.
    /// An interceptor cannot change this; the kernel applies it when sending DATA.
    /// Same value as [`crate::Channel::send_buffer_size`]. Default [`crate::DEFAULT_MAX_SEND_BUFFER_SIZE`].
    /// Applies to every call shape.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.send_buffer_size
    }

    /// The full gRPC path, `/<package>.<Service>/<Method>`. Visible on every
    /// call shape.
    ///
    /// Distinct from [`crate::Rpc::path`]: that is a server interceptor; this is a client interceptor before send.
    #[must_use]
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Distinct from [`crate::Rpc::service`]: that is a server interceptor; this is a client interceptor before send.
    /// Same split as [`crate::Rpc::service`]. Unparseable paths yield `""`.
    /// Bind it before [`Self::metadata_mut`]: `let svc = call.service();`.
    /// Applies to every call shape.
    #[must_use]
    pub fn service(&self) -> &'static str {
        split_path(self.path).0
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Distinct from [`crate::Rpc::method`]: that is a server interceptor; this is a client interceptor before send.
    /// Same split as [`crate::Rpc::method`]. Unparseable paths yield `""`.
    /// Bind it before [`Self::metadata_mut`]: `let method = call.method();`.
    /// Applies to every call shape.
    #[must_use]
    pub fn method(&self) -> &'static str {
        split_path(self.path).1
    }

    /// Request headers, as gRPC metadata. Applies to every call shape.
    /// Distinct from [`Self::metadata_mut`]: that mutates the outbound map; this borrows it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        self.metadata
    }

    /// Mutable request headers. Applies to every call shape.
    ///
    /// Distinct from [`Self::metadata`]: that borrows the outbound map; this mutates it.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        self.metadata
    }

    /// Relative timeout that becomes `grpc-timeout` on the wire.
    ///
    /// Distinct from [`Self::rpc_timeout`]: that is the channel overlay; this is the Call `grpc-timeout`.
    /// `None` when neither the request nor a channel overlay set one. Fill
    /// that case with [`Self::set_timeout`]. The matching Instant is
    /// [`Self::deadline`]. Applies to every call shape.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        *self.timeout
    }

    /// Absolute Instant matching [`Self::timeout`].
    ///
    /// Distinct from [`Self::timeout`]: that is the duration; this Instant is computed when the getter runs.
    /// Distinct from [`Self::rpc_timeout`]: that is the duration overlay; this Instant is computed when the getter runs.
    /// Computed when you call this, so an interceptor that just set
    /// [`Self::set_timeout`] sees the new Instant. Same contract as
    /// [`crate::Rpc::deadline`]. Visible on every call shape.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.timeout.map(|d| tokio::time::Instant::now() + d)
    }

    /// Set the relative timeout. Becomes `grpc-timeout` on the wire.
    ///
    /// Distinct from [`Self::timeout`]: that reads the Call `grpc-timeout`; this writes it.
    /// This is the [`crate::Call`]'s deadline on every call shape.
    pub fn set_timeout(&mut self, timeout: Duration) {
        *self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on the request, by the channel overlay,
    /// or by an earlier interceptor.
    ///
    /// Distinct from [`Self::set_timeout`]: that writes the Call `grpc-timeout`; this opts out.
    /// The channel overlay has already run; clearing here opts out of that
    /// default too. [`Self::rpc_timeout`] still reports the channel policy so
    /// a later interceptor can re-apply it. Applies to every call shape.
    pub fn clear_timeout(&mut self) {
        *self.timeout = None;
    }

    /// Whether this RPC waits for a connection instead of failing fast.
    ///
    /// Distinct from [`Self::waits_for_ready`]: that is the channel overlay; this is the per-RPC choice.
    /// `false` when unset. Use [`Self::wait_for_ready_is_set`] to tell
    /// `None` from an explicit `false`. Applies to every call shape.
    #[must_use]
    pub fn wait_for_ready(&self) -> bool {
        self.wait_for_ready.unwrap_or(false)
    }

    /// Whether [`Self::set_wait_for_ready`] has been called, including a
    /// channel overlay.
    ///
    /// Distinct from [`Self::wait_for_ready`], which is `false` when unset.
    /// Fill a default only when this is `false`, the same pattern as
    /// [`Self::timeout`] being `None`. Applies to every call shape.
    #[must_use]
    pub fn wait_for_ready_is_set(&self) -> bool {
        self.wait_for_ready.is_some()
    }

    /// Queue this RPC until the channel is connected. Applies to every call
    /// shape.
    /// Distinct from [`Self::connected`]: that is a live snapshot; this still queues when a slot is empty.
    /// Distinct from [`Self::wait_for_ready`]: that reads the per-RPC choice; this writes it.
    pub fn set_wait_for_ready(&mut self, wait: bool) {
        *self.wait_for_ready = Some(wait);
    }

    /// Drop a wait-for-ready choice so a later interceptor can fill it in.
    ///
    /// Distinct from [`Self::set_wait_for_ready`]: that queues this RPC; this opts out.
    /// The channel overlay has already run; clearing here opts out of that
    /// default too, the same as [`Self::clear_timeout`]. [`Self::waits_for_ready`]
    /// still reports the channel policy so a later interceptor can re-apply it.
    /// Applies to every call shape.
    pub fn clear_wait_for_ready(&mut self) {
        *self.wait_for_ready = None;
    }

    /// Whether any pool slot currently holds a live HTTP/2 connection.
    ///
    /// Same snapshot as [`crate::Channel::connected`], taken when this
    /// interceptor runs. Distinct from [`Self::wait_for_ready`]: that queues
    /// until a dial; this is whether a socket is already live. A lazy first
    /// RPC sees `false` even when wait-for-ready is on — interceptors run
    /// before the stream opens. An eager [`crate::Channel::connect`] or a
    /// live [`crate::Channel::from_io`] duplex sees `true`. After
    /// [`crate::ChannelConfig::max_connection_idle`] or
    /// [`crate::ChannelConfig::max_connection_age`] this is `false` until
    /// the next RPC redials. Applies to every call shape.
    #[must_use]
    pub fn connected(&self) -> bool {
        self.connected
    }

    /// Whether the request payload will be gzipped.
    ///
    /// Distinct from [`Self::compresses_outbound`]: that is the channel overlay; this is the per-RPC choice.
    /// `false` when unset. Use [`Self::compress_is_set`] to tell `None`
    /// from an explicit `false`. Applies to every call shape.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called, including a
    /// channel overlay.
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset.
    /// Fill a default only when this is `false`, the same pattern as
    /// [`Self::timeout`] being `None`. Applies to every call shape.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// gzip this request's payload and set the Compressed-Flag.
    ///
    /// Distinct from [`Self::compress`]: that reads the per-RPC Compressed-Flag; this writes it.
    /// Passing `false` opts out of a channel [`crate::Channel::send_compressed`]
    /// overlay. Applies to every call shape.
    pub fn set_compress(&mut self, compress: bool) {
        *self.compress = Some(compress);
    }

    /// Drop a compression choice so a later interceptor can fill it in.
    ///
    /// Distinct from [`Self::set_compress`]: that writes the per-RPC Compressed-Flag; this opts out.
    /// The channel overlay has already run; clearing here opts out of that
    /// default too, the same as [`Self::clear_timeout`].
    /// [`Self::compresses_outbound`] still reports the channel policy so a
    /// later interceptor can re-apply it. Applies to every call shape.
    pub fn clear_compress(&mut self) {
        *self.compress = None;
    }

    /// Typed values earlier interceptors or the caller attached to this RPC.
    ///
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values; this borrows the map.
    /// The caller inserts on [`crate::Request::extensions_mut`] before the
    /// call; stacked interceptors share the same map. These values are not
    /// sent on the wire. Visible on every call shape.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        self.extensions
    }

    /// Insert typed values for later interceptors.
    ///
    /// Distinct from [`Self::extensions`]: that borrows the map; this inserts typed values.
    /// Use this to pass a parsed identity or span into the next interceptor
    /// without a metadata round-trip. Applies to every call shape.
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
            .field("user_agent", &self.user_agent())
            .field("limits", &self.limits)
            .field("rpc_timeout", &self.rpc_timeout)
            .field("waits_for_ready", &self.waits_for_ready)
            .field("compresses_outbound", &self.compresses_outbound)
            .field("accepts_compressed", &self.accepts_compressed)
            .field("gzip_level", &self.gzip_level)
            .field("concurrent_rpc_limit", &self.concurrent_rpc_limit)
            .field("stream_buffer_size", &self.stream_buffer_size)
            .field("send_buffer_size", &self.send_buffer_size)
            .field("metadata", &self.metadata)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline())
            .field("wait_for_ready", &self.wait_for_ready)
            .field("connected", &self.connected)
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
            .field("rpc_timeout", &self.rpc_timeout)
            .field("accepts_gzip", &self.accepts_gzip)
            .field("compresses_outbound", &self.compresses_outbound)
            .field("gzip_level", &self.gzip_level)
            .field("accepts_compressed", &self.accepts_compressed)
            .field("concurrent_rpc_limit", &self.concurrent_rpc_limit)
            .field("send_buffer_size", &self.send_buffer_size)
            .field("encoding", &self.encoding)
            .field("cancelled", &self.is_cancelled())
            .field("extensions", &self.extensions.len())
            .field("user_agent", &self.user_agent())
            .finish_non_exhaustive()
    }
}

/// A [`Request`] envelope without its message, including [`Self::cancelled`].
/// See [`Request::into_message_and_parts`].
///
/// ```
/// fn dump_parts(parts: &pbrs_grpc::Parts) {
///     let _ = (
///         parts.path(),
///         parts.service(),
///         parts.method(),
///         parts.metadata(),
///         parts.timeout(),
///         parts.rpc_timeout(),
///         parts.peer_timeout(),
///         parts.deadline(),
///         parts.compress(),
///         parts.compressed(),
///         parts.encoding(),
///         parts.accepts_gzip(),
///         parts.compresses_outbound(),
///         parts.gzip_level(),
///         parts.accepts_compressed(),
///         parts.concurrent_rpc_limit(),
///         parts.send_buffer_size(),
///         parts.remote_addr(),
///         parts.local_addr(),
///         parts.peer_identity(),
///         parts.peer_cred(),
///         parts.authority(),
///         parts.scheme(),
///         parts.wait_for_ready(),
///         parts.limits(),
///         parts.extensions(),
///         parts.user_agent(),
///     );
///     let _ = parts.cancelled();
/// }
/// # let _ = dump_parts;
/// ```
/// [`Self::user_agent_is_set`] is occupancy on this split envelope, so a later interceptor can prefix only when unset.
/// [`Self::wait_for_ready_is_set`] is occupancy on this split envelope, so a later interceptor can fill wait-for-ready only when unset.
/// [`Self::compress_is_set`] is occupancy on this split envelope, so a later interceptor can fill compress only when unset.
/// [`Self::clear_timeout`] opts out of the channel timeout on this split envelope.
/// [`Self::clear_wait_for_ready`] restores the channel wait-for-ready overlay on this split envelope.
/// [`Self::clear_compress`] restores the channel gzip overlay on this split envelope.
/// [`Self::clear_user_agent`] restores the channel user-agent on this split envelope.
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
    rpc_timeout: Option<Duration>,
    accepts_gzip: bool,
    compresses_outbound: bool,
    gzip_level: u32,
    accepts_compressed: bool,
    concurrent_rpc_limit: Option<usize>,
    send_buffer_size: usize,
    encoding: Option<String>,
    cancel: Option<watch::Receiver<bool>>,
    extensions: http::Extensions,
    user_agent: Option<http::HeaderValue>,
}

impl Parts {
    /// Request headers.
    ///
    /// Distinct from [`Self::metadata_mut`]: that mutates this split envelope; this borrows it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable request headers.
    ///
    /// Distinct from [`Self::metadata`]: that borrows this split envelope; this mutates it.
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
    /// Distinct from [`Self::timeout`]: that reads the relative timeout this split envelope carries; this writes it.
    ///
    /// Same as [`Request::set_timeout`]. A proxy that split the envelope
    /// with [`Request::into_message_and_parts`] can tighten the deadline
    /// here without rebuilding a [`Request`] first.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = Some(timeout);
    }

    /// Clear a timeout previously set on this envelope.
    /// See [`Request::clear_timeout`].
    ///
    /// Distinct from [`Self::set_timeout`]: that writes the relative timeout this split envelope carries; this opts out.
    pub fn clear_timeout(&mut self) {
        self.timeout = None;
    }

    /// Absolute deadline the server is enforcing, if any. See [`Request::deadline`].
    ///
    /// Distinct from [`timeout`](Self::timeout): that is the duration stamped at dispatch on this split envelope; this Instant does not shrink.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Whether the payload will be gzipped. Outbound only.
    /// See [`Request::compress`].
    ///
    /// Distinct from [`Self::compressed`]: that is the inbound unary Compressed-Flag on this split envelope; this is outbound gzip intent.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// Whether [`Self::set_compress`] has been called.
    /// See [`Request::compress_is_set`].
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset on this split envelope.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// gzip this request's payload and set the Compressed-Flag.
    /// See [`Request::set_compress`].
    ///
    /// Distinct from [`Self::compress`]: that reads outbound payload gzip on this split envelope; this writes it.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later channel default can fill it in.
    /// See [`Request::clear_compress`].
    ///
    /// Distinct from [`Self::set_compress`]: that writes outbound payload gzip on this split envelope; this opts out.
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
    ///
    /// Distinct from [`Self::wait_for_ready`], which is `false` when unset on this split envelope.
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

    /// The `user-agent` prefix override on this envelope, if any.
    /// See [`Request::user_agent`]. The override only, like [`Self::timeout`].
    ///
    /// Distinct from [`crate::Outgoing::user_agent`]: that is the effective header; this split envelope is the override only.
    #[must_use]
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent
            .as_ref()
            .and_then(|value| value.to_str().ok())
    }

    /// Prefix the kernel `user-agent` on this envelope.
    /// See [`Request::set_user_agent`].
    pub fn set_user_agent(&mut self, prefix: impl AsRef<str>) -> Result<(), Status> {
        self.user_agent = Some(crate::wire::user_agent_value(prefix.as_ref())?);
        Ok(())
    }

    /// Whether [`Self::set_user_agent`] has already overridden the channel
    /// value. See [`Request::user_agent_is_set`].
    ///
    /// Distinct from [`crate::Outgoing::user_agent_is_set`]: that is the same flag after interceptors run, not this split envelope.
    #[must_use]
    pub fn user_agent_is_set(&self) -> bool {
        self.user_agent.is_some()
    }

    /// Drop a [`Self::set_user_agent`] override so this RPC uses the channel
    /// value again. See [`Request::clear_user_agent`].
    pub fn clear_user_agent(&mut self) {
        self.user_agent = None;
    }

    /// Typed values an interceptor attached to this RPC. See [`Request::extensions`].
    ///
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values this split envelope carries; this borrows them.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values for later handlers or interceptors.
    ///
    /// Distinct from [`Self::extensions`]: that borrows them; this inserts typed values this split envelope carries.
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
    ///
    /// Distinct from [`timeout`](Self::timeout): that is the effective cap on this split envelope; this is the client's original header.
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    /// Server [`crate::Server::timeout`] overlay, when the kernel dispatched
    /// this call. See [`Request::rpc_timeout`].
    ///
    /// Distinct from [`timeout`](Self::timeout): that is the effective cap on this split envelope; this is the server overlay.
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.rpc_timeout
    }

    /// Whether the peer advertised gzip in `grpc-accept-encoding`.
    /// See [`Request::accepts_gzip`].
    ///
    /// Distinct from [`Self::accepts_compressed`]: that is the inbound overlay on this split envelope, not the peer advertisement.
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        self.accepts_gzip
    }

    /// Whether this server gzips responses when the peer advertised gzip.
    /// See [`Request::compresses_outbound`].
    ///
    /// Distinct from [`Self::compress`]: that is outbound request-payload gzip on this split envelope; this is the server encode overlay.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.compresses_outbound
    }

    /// Server gzip deflate overlay. See [`Request::gzip_level`].
    ///
    /// Distinct from [`Self::compresses_outbound`]: that is on or off on this split envelope; this is deflate effort.
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay, not this split envelope's server overlay.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.gzip_level
    }

    /// Server inbound gzip overlay. See [`Request::accepts_compressed`].
    ///
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this split envelope's inbound overlay.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay, not this split envelope's server overlay.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.accepts_compressed
    }

    /// Server process RPC cap overlay. See [`Request::concurrent_rpc_limit`].
    ///
    /// Distinct from [`crate::Outgoing::concurrent_rpc_limit`]: that is a client interceptor overlay, not this split envelope's server overlay.
    /// Distinct from [`Self::limits`]: that is message size on this split envelope, not how many RPCs.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.concurrent_rpc_limit
    }

    /// Server write-time send buffer overlay. See [`Request::send_buffer_size`].
    ///
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay, not this split envelope's server overlay.
    /// Distinct from [`Self::limits`]: that is message size on this split envelope, not this HTTP/2 send buffer.
    /// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows: those are handshake SETTINGS, not this split envelope's write-time threshold.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.send_buffer_size
    }

    /// The `grpc-encoding` token the peer used on this call, if any.
    /// See [`Request::encoding`].
    ///
    /// Distinct from [`Self::accepts_gzip`]: that is the peer's `grpc-accept-encoding`, not this split envelope's inbound `grpc-encoding`.
    /// Distinct from [`compressed`](Self::compressed): that is the per-message Compressed-Flag on this split envelope; this is the HTTP header.
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
            .field("rpc_timeout", &self.rpc_timeout)
            .field("accepts_gzip", &self.accepts_gzip)
            .field("compresses_outbound", &self.compresses_outbound)
            .field("gzip_level", &self.gzip_level)
            .field("accepts_compressed", &self.accepts_compressed)
            .field("concurrent_rpc_limit", &self.concurrent_rpc_limit)
            .field("send_buffer_size", &self.send_buffer_size)
            .field("encoding", &self.encoding)
            .field("cancelled", &self.is_cancelled())
            .field("extensions", &self.extensions.len())
            .field("user_agent", &self.user_agent())
            .finish_non_exhaustive()
    }
}

/// A reply: message, initial headers, and trailing metadata.
///
/// The kernel stamps [`Self::path`] after a handler `Ok` and after a successful receive.
/// Distinct from [`crate::Request::path`]: that is the inbound request.
/// Distinct from [`crate::Outgoing::path`]: that is a client interceptor before send.
///
/// Trailing metadata set here survives on the OK path; to attach metadata to
/// an error, put it on the [`Status`] instead.
///
/// [`Self::extensions`] is local typed context. It is not on the wire.
/// Distinct from [`Self::metadata`]. A received reply starts empty.
///
/// ```
/// use pbrs_grpc::Response;
///
/// let mut resp = Response::new(42);
/// resp.metadata_mut().insert("x-cache", "miss")?;
/// resp.trailers_mut().insert("x-rows-scanned", "17")?;
/// resp.set_compress(true);
/// resp.extensions_mut().insert(7u8);
/// let (n, mut parts) = resp.into_message_and_parts();
/// assert_eq!(parts.metadata().get("x-cache"), Some("miss"));
/// assert_eq!(parts.extensions().get::<u8>().copied(), Some(7));
/// assert!(parts.compressed());
/// parts.set_compress(false);
/// let resp = Response::from_message_and_parts(n, parts);
/// assert!(!resp.compressed());
/// assert_eq!(resp.extensions().get::<u8>().copied(), Some(7));
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
/// [`Self::compress_is_set`] is occupancy on this reply envelope, so a later interceptor can fill compress only when unset.
/// [`Self::clear_compress`] restores the server gzip overlay on this reply envelope.
#[derive(Clone)]
pub struct Response<T> {
    message: T,
    metadata: Metadata,
    trailers: Metadata,
    compress: Option<bool>,
    encoding: Option<String>,
    path: Option<String>,
    gzip_level: u32,
    compresses_outbound: bool,
    accepts_gzip: bool,
    accepts_compressed: bool,
    deadline: Option<tokio::time::Instant>,
    timeout: Option<Duration>,
    peer_timeout: Option<Duration>,
    rpc_timeout: Option<Duration>,
    limits: Option<MessageLimits>,
    send_buffer_size: Option<usize>,
    extensions: http::Extensions,
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
            encoding: None,
            path: None,
            gzip_level: crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL,
            compresses_outbound: false,
            accepts_gzip: false,
            accepts_compressed: false,
            deadline: None,
            timeout: None,
            peer_timeout: None,
            rpc_timeout: None,
            limits: None,
            send_buffer_size: None,
            extensions: http::Extensions::new(),
        }
    }

    /// Take the message, discarding the envelope.
    ///
    /// Headers, trailers, compress intent, received [`Self::encoding`], and
    /// extensions go with it. Use [`Self::into_message_and_parts`] to keep them.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }

    /// Split into message and envelope, keeping headers, trailers, compression,
    /// received [`Self::encoding`], and extensions.
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
                encoding: self.encoding,
                path: self.path,
                gzip_level: self.gzip_level,
                compresses_outbound: self.compresses_outbound,
                accepts_gzip: self.accepts_gzip,
                accepts_compressed: self.accepts_compressed,
                deadline: self.deadline,
                timeout: self.timeout,
                peer_timeout: self.peer_timeout,
                rpc_timeout: self.rpc_timeout,
                limits: self.limits,
                send_buffer_size: self.send_buffer_size,
                extensions: self.extensions,
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
            encoding: parts.encoding,
            path: parts.path,
            gzip_level: parts.gzip_level,
            compresses_outbound: parts.compresses_outbound,
            accepts_gzip: parts.accepts_gzip,
            accepts_compressed: parts.accepts_compressed,
            deadline: parts.deadline,
            timeout: parts.timeout,
            peer_timeout: parts.peer_timeout,
            rpc_timeout: parts.rpc_timeout,
            limits: parts.limits,
            send_buffer_size: parts.send_buffer_size,
            extensions: parts.extensions,
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

    /// Replace the message, keeping headers, trailers, and extensions.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Response<U> {
        let (message, parts) = self.into_message_and_parts();
        Response::from_message_and_parts(f(message), parts)
    }

    /// Initial headers, sent before the first message. Applies to every call
    /// shape: a streaming [`crate::Call`] exposes these on the [`Response`]
    /// before [`crate::Streaming`] messages.
    ///
    /// Distinct from [`Self::metadata_mut`]: that mutates this reply envelope; this borrows it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable initial headers.
    ///
    /// Distinct from [`Self::metadata`]: that borrows this reply envelope; this mutates it.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Trailing metadata, sent alongside `grpc-status`.
    ///
    /// Distinct from [`Self::trailers_mut`]: that mutates this reply envelope; this borrows it.
    ///
    /// On unary and client-streaming [`crate::Call`] results this is the
    /// OK-path custom trailer map. Server-streaming and bidi clients read
    /// the same map with [`crate::Streaming::trailers`] after end-of-stream.
    /// A `-bin` trailer must not appear as a header, including over TLS,
    /// mTLS, Unix, and [`crate::Channel::from_io`].
    #[must_use]
    pub fn trailers(&self) -> &Metadata {
        &self.trailers
    }

    /// Mutable trailing metadata.
    ///
    /// Distinct from [`Self::trailers`]: that borrows this reply envelope; this mutates it.
    pub fn trailers_mut(&mut self) -> &mut Metadata {
        &mut self.trailers
    }

    /// Typed values on this envelope. They are not headers and they are not
    /// on the wire. Distinct from [`Self::metadata`].
    ///
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values this reply envelope carries; this borrows them.
    ///
    /// Empty on a reply you built until something inserts into
    /// [`Self::extensions_mut`]. A received reply starts empty: the peer
    /// cannot insert here. Same map on [`ResponseParts::extensions`] after
    /// [`Self::into_message_and_parts`]. A [`crate::ResponseInterceptor`]
    /// can read this map and stamp [`Self::metadata`] that does go on the
    /// wire. A [`crate::Channel::on_response`] hook can insert after a
    /// successful receive; the peer still cannot.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values that stay on this envelope and on
    /// [`ResponseParts`] after a message swap. Not sent to the peer.
    ///
    /// Distinct from [`Self::extensions`]: that borrows them; this inserts typed values this reply envelope carries.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// gzip this payload and set the Compressed-Flag.
    ///
    /// Distinct from [`Self::compress`]: that reads outbound payload gzip on this reply envelope; this writes it.
    ///
    /// Passing `false` opts out of a later [`crate::Server::send_compressed`]
    /// overlay on every call shape, including over TLS, mTLS, Unix, and
    /// [`crate::Channel::from_io`]. [`Self::clear_compress`] drops the choice so that overlay
    /// can fill it in. On a stream, `true` advertises `grpc-encoding: gzip`
    /// so mixed per-message flags are legal; identity [`crate::StreamSender::send`]
    /// frames stay identity. Choose gzip per message with
    /// [`crate::StreamSender::send_compressed`].
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later server overlay can fill it in.
    ///
    /// Distinct from [`Self::set_compress`]: that writes outbound payload gzip on this reply envelope; this opts out.
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

    /// The `grpc-encoding` token on a received reply, if any.
    ///
    /// `Some("gzip")` when the peer advertised gzip on this response.
    /// `None` means identity — header absent, empty, or an explicit
    /// `identity` token — or a response you built to send. Outbound intent
    /// is [`Self::set_compress`], not this header. Distinct from
    /// [`Self::compressed`]: that is the unary Compressed-Flag (and
    /// outbound intent); this is the HTTP header that applies to the whole
    /// call. Streaming payloads still report the per-message flag on
    /// [`crate::Framed`]. A default server (no [`crate::Server::send_compressed`])
    /// leaves this `None` on every call shape, including over TLS, mTLS, Unix,
    /// and [`crate::Channel::from_io`]. Bind it before [`Self::metadata_mut`]:
    /// `let enc = response.encoding();`.
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Full gRPC path, e.g. `/helloworld.Greeter/SayHello`.
    ///
    /// Kernel-stamped after a handler `Ok` and after a successful receive.
    /// `None` on a response you built: the kernel stamps this, not this envelope.
    /// Distinct from [`crate::Request::path`]: that is the inbound request.
    /// Distinct from [`crate::Outgoing::path`]: that is a client interceptor before send.
    /// Bind it before [`Self::metadata_mut`]: `let path = response.path();`. Stamped on every call shape.
    /// An interceptor cannot change this.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Service half of the path, e.g. `helloworld.Greeter`.
    ///
    /// Same split as [`crate::Rpc::service`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`. Stamped on every
    /// call shape.
    #[must_use]
    pub fn service(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).0)
    }

    /// Method half of the path, e.g. `SayHello`.
    ///
    /// Same split as [`crate::Rpc::method`]. Unparseable paths yield
    /// `Some("")`. `None` when [`Self::path`] is `None`. Stamped on every
    /// call shape.
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).1)
    }

    pub(crate) fn with_path(mut self, path: Option<String>) -> Self {
        self.path = path;
        self
    }

    pub(crate) fn with_gzip_level(mut self, gzip_level: u32) -> Self {
        self.gzip_level = gzip_level;
        self
    }

    /// Server [`crate::Server::gzip_compression_level`] overlay, when the kernel is encoding this reply.
    ///
    /// Same overlay as [`crate::Rpc::gzip_level`] / [`crate::Request::gzip_level`].
    /// Distinct from [`Self::compress`]: that is on or off; this is deflate effort.
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay.
    /// Distinct from [`crate::Rpc::gzip_level`]: that is a server interceptor before the handler.
    /// [`crate::DEFAULT_GZIP_COMPRESSION_LEVEL`] on a response you built or a received reply (deflate effort is not on the wire).
    /// Distinct from [`Self::encoding`]: that is the received `grpc-encoding` token.
    /// An interceptor cannot change this; the kernel applies it when encoding.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert_eq!(resp.gzip_level(), pbrs_grpc::DEFAULT_GZIP_COMPRESSION_LEVEL);
    /// ```
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.gzip_level
    }

    pub(crate) fn with_compresses_outbound(mut self, compresses_outbound: bool) -> Self {
        self.compresses_outbound = compresses_outbound;
        self
    }

    /// Server [`crate::Server::send_compressed`] overlay, when the kernel is encoding this reply.
    ///
    /// Same overlay as [`crate::Rpc::compresses_outbound`] / [`crate::Request::compresses_outbound`].
    /// Distinct from [`Self::compress`]: that is the per-RPC choice after overlay and interceptor mutation.
    /// Distinct from [`crate::Outgoing::compresses_outbound`]: that is a client interceptor overlay.
    /// Distinct from [`crate::Rpc::compresses_outbound`]: that is a server interceptor before the handler.
    /// Distinct from [`Self::gzip_level`]: that is deflate effort, not on or off.
    /// `false` on a response you built or a received reply (the overlay is not on the wire).
    /// An interceptor cannot change this; unset [`Self::compress`] follows this default when the peer advertised gzip.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(!resp.compresses_outbound());
    /// ```
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.compresses_outbound
    }

    pub(crate) fn with_accepts_gzip(mut self, accepts_gzip: bool) -> Self {
        self.accepts_gzip = accepts_gzip;
        self
    }

    /// Peer `grpc-accept-encoding` gzip advertisement, when the kernel is encoding this reply.
    ///
    /// Same value as [`crate::Rpc::accepts_gzip`] / [`crate::Request::accepts_gzip`].
    /// Distinct from [`Self::encoding`]: that is received `grpc-encoding`, not `grpc-accept-encoding`.
    /// Distinct from [`crate::Rpc::accepts_gzip`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Request::accepts_gzip`]: that is the inbound request.
    /// Distinct from [`Self::compresses_outbound`]: that is the server encode overlay, not the peer advertisement.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay.
    /// `false` on a response you built or a received reply (the advertisement is not on the reply wire).
    /// An interceptor cannot change this; gzip only goes out when this is true.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(!resp.accepts_gzip());
    /// ```
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        self.accepts_gzip
    }

    pub(crate) fn with_accepts_compressed(mut self, accepts_compressed: bool) -> Self {
        self.accepts_compressed = accepts_compressed;
        self
    }

    /// Server [`crate::Server::accept_compressed`] overlay, when the kernel is writing this reply.
    ///
    /// Same overlay as [`crate::Rpc::accepts_compressed`] / [`crate::Request::accepts_compressed`].
    /// Distinct from [`Self::accepts_gzip`]: that is the peer advertisement, not this overlay.
    /// Distinct from [`crate::Rpc::accepts_compressed`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay, not this server stamp.
    /// Distinct from [`Self::compresses_outbound`]: that is whether this reply is gzipped.
    /// Distinct from [`Self::encoding`]: that is received `grpc-encoding`, not this advertisement.
    /// `false` on a response you built or a received reply (this overlay is not a received-reply field).
    /// An interceptor cannot change this; the kernel still advertises `grpc-accept-encoding` from this.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(!resp.accepts_compressed());
    /// ```
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.accepts_compressed
    }

    pub(crate) fn with_deadline(mut self, deadline: Option<tokio::time::Instant>) -> Self {
        self.deadline = deadline;
        self
    }

    /// Kernel-stamped remaining Instant after a handler `Ok`, when the kernel is writing this reply.
    ///
    /// Same Instant as [`crate::Request::deadline`] after dispatch.
    /// Distinct from [`crate::Request::deadline`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::deadline`]: that is computed when that getter runs.
    /// Distinct from [`crate::Request::timeout`]: that is the duration stamped at dispatch.
    /// Distinct from [`crate::Outgoing::deadline`]: that is a client interceptor Instant.
    /// `None` on a response you built or a received reply (the peer deadline is not on the wire).
    /// An interceptor cannot change this; the kernel still enforces it when writing.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.deadline().is_none());
    /// ```
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    pub(crate) fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Effective timeout duration stamped at dispatch, when the kernel is writing this reply.
    ///
    /// Same duration as [`crate::Request::timeout`] after dispatch. This duration does not shrink.
    /// Distinct from [`crate::Request::timeout`]: that is the inbound request.
    /// Distinct from [`Self::deadline`]: that is the Instant; this duration does not shrink.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the effective duration.
    /// Distinct from [`crate::Rpc::effective_timeout`]: that is computed when that getter runs.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is the server overlay, not the effective cap.
    /// Distinct from [`crate::Request::peer_timeout`]: that is the client's `grpc-timeout`.
    /// Distinct from [`crate::Outgoing::timeout`]: that is a client interceptor duration.
    /// `None` on a response you built or a received reply (the peer timeout is not on the reply wire).
    /// An interceptor cannot change this; the kernel still enforces [`Self::deadline`] when writing.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.timeout().is_none());
    /// ```
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub(crate) fn with_peer_timeout(mut self, peer_timeout: Option<Duration>) -> Self {
        self.peer_timeout = peer_timeout;
        self
    }

    /// The client's original `grpc-timeout`, when the kernel is writing this reply.
    ///
    /// Same duration as [`crate::Request::peer_timeout`] after dispatch.
    /// Distinct from [`crate::Request::peer_timeout`]: that is the inbound request.
    /// Distinct from [`Self::timeout`]: that is the effective cap; this is the client's original header.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the client header.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is the server overlay, not the client header.
    /// Distinct from [`crate::Rpc::peer_timeout`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Rpc::effective_timeout`]: that is the soonest of the three caps.
    /// Distinct from [`Self::deadline`]: that is the Instant, not the client header.
    /// `None` on a response you built or a received reply (the client's `grpc-timeout` is not on the reply wire).
    /// An interceptor cannot change this; the kernel already combined it into [`Self::timeout`].
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.peer_timeout().is_none());
    /// ```
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    pub(crate) fn with_rpc_timeout(mut self, rpc_timeout: Option<Duration>) -> Self {
        self.rpc_timeout = rpc_timeout;
        self
    }

    /// Server [`crate::Server::timeout`] overlay, when the kernel is writing this reply.
    ///
    /// Same duration as [`crate::Request::rpc_timeout`] after dispatch.
    /// Distinct from [`crate::Request::rpc_timeout`]: that is the inbound request.
    /// Distinct from [`Self::timeout`]: that is the effective cap; this is the server overlay.
    /// Distinct from [`Self::peer_timeout`]: that is the client's `grpc-timeout`, not the server overlay.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the server overlay.
    /// Distinct from [`crate::Outgoing::rpc_timeout`]: that is a client interceptor overlay.
    /// Distinct from [`Self::deadline`]: that is the Instant, not the server overlay.
    /// `None` on a response you built or a received reply (the server overlay is not on the reply wire).
    /// An interceptor cannot change this; an interceptor cap only tightens [`Self::timeout`].
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.rpc_timeout().is_none());
    /// ```
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.rpc_timeout
    }

    pub(crate) fn with_limits(mut self, limits: Option<MessageLimits>) -> Self {
        self.limits = limits;
        self
    }

    /// Message caps the kernel is enforcing when encoding this reply.
    ///
    /// Same caps as [`crate::Rpc::limits`] / [`crate::Request::limits`] after dispatch.
    /// Distinct from [`crate::Request::limits`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::limits`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Outgoing::limits`]: that is a client interceptor overlay.
    /// Distinct from [`Self::timeout`]: that is a duration, not a size cap.
    /// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is queue depth, not message size.
    /// `None` on a response you built or a received reply (the peer encode cap is not on the wire).
    /// An interceptor cannot raise them; the kernel still checks these caps when encoding.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.limits().is_none());
    /// ```
    #[must_use]
    pub fn limits(&self) -> Option<MessageLimits> {
        self.limits
    }

    pub(crate) fn with_send_buffer_size(mut self, send_buffer_size: Option<usize>) -> Self {
        self.send_buffer_size = send_buffer_size;
        self
    }

    /// Server [`crate::Server::max_send_buffer_size`] overlay, when the kernel is writing this reply.
    ///
    /// Same overlay as [`crate::Rpc::send_buffer_size`] / [`crate::Request::send_buffer_size`].
    /// Distinct from [`crate::Request::send_buffer_size`]: that is the inbound request.
    /// Distinct from [`crate::Rpc::send_buffer_size`]: that is a server interceptor before the handler.
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay, not this server stamp.
    /// Distinct from [`Self::limits`]: that is the encode cap, not this HTTP/2 send buffer.
    /// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is decoded-message queue depth, not this send buffer.
    /// `None` on a response you built or a received reply (the peer send buffer is not on the reply wire).
    /// An interceptor cannot change this; the kernel still applies this buffer when writing DATA.
    ///
    /// ```
    /// let resp = pbrs_grpc::Response::new(());
    /// assert!(resp.send_buffer_size().is_none());
    /// ```
    #[must_use]
    pub fn send_buffer_size(&self) -> Option<usize> {
        self.send_buffer_size
    }

    pub(crate) fn from_parts(message: T, metadata: Metadata, trailers: Metadata) -> Self {
        Self {
            message,
            metadata,
            trailers,
            compress: Some(false),
            encoding: None,
            path: None,
            gzip_level: crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL,
            compresses_outbound: false,
            accepts_gzip: false,
            accepts_compressed: false,
            deadline: None,
            timeout: None,
            peer_timeout: None,
            rpc_timeout: None,
            limits: None,
            send_buffer_size: None,
            extensions: http::Extensions::new(),
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
            encoding: None,
            path: None,
            gzip_level: crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL,
            compresses_outbound: false,
            accepts_gzip: false,
            accepts_compressed: false,
            deadline: None,
            timeout: None,
            peer_timeout: None,
            rpc_timeout: None,
            limits: None,
            send_buffer_size: None,
            extensions: http::Extensions::new(),
        }
    }

    pub(crate) fn with_encoding(mut self, encoding: Option<String>) -> Self {
        self.encoding = encoding;
        self
    }

    pub(crate) fn split(self) -> (T, Metadata, Metadata, Option<bool>) {
        let (message, parts) = self.into_message_and_parts();
        (message, parts.metadata, parts.trailers, parts.compress)
    }
}

/// A [`Response`] envelope without its message, including received
/// [`Response::encoding`] and local [`Response::extensions`].
/// See [`Response::into_message_and_parts`].
/// [`Self::compress_is_set`] is occupancy on this split reply envelope, so a later interceptor can fill compress only when unset.
/// [`Self::clear_compress`] restores the server gzip overlay on this split reply envelope.
#[derive(Clone, Debug)]
pub struct ResponseParts {
    metadata: Metadata,
    trailers: Metadata,
    compress: Option<bool>,
    encoding: Option<String>,
    path: Option<String>,
    gzip_level: u32,
    compresses_outbound: bool,
    accepts_gzip: bool,
    accepts_compressed: bool,
    deadline: Option<tokio::time::Instant>,
    timeout: Option<Duration>,
    peer_timeout: Option<Duration>,
    rpc_timeout: Option<Duration>,
    limits: Option<MessageLimits>,
    send_buffer_size: Option<usize>,
    extensions: http::Extensions,
}

impl ResponseParts {
    /// Initial headers, sent before the first message.
    ///
    /// Distinct from [`Self::metadata_mut`]: that mutates this split reply envelope; this borrows it.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable initial headers.
    ///
    /// Distinct from [`Self::metadata`]: that borrows this split reply envelope; this mutates it.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// Trailing metadata, sent alongside `grpc-status`.
    ///
    /// Distinct from [`Self::trailers_mut`]: that mutates this split reply envelope; this borrows it.
    #[must_use]
    pub fn trailers(&self) -> &Metadata {
        &self.trailers
    }

    /// Mutable trailing metadata.
    ///
    /// Distinct from [`Self::trailers`]: that borrows this split reply envelope; this mutates it.
    pub fn trailers_mut(&mut self) -> &mut Metadata {
        &mut self.trailers
    }

    /// gzip this payload and set the Compressed-Flag.
    /// See [`Response::set_compress`].
    ///
    /// Distinct from [`Self::compress`]: that reads outbound payload gzip on this split reply envelope; this writes it.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = Some(compress);
    }

    /// Drop a compression choice so a later server overlay can fill it in.
    /// See [`Response::clear_compress`].
    ///
    /// Distinct from [`Self::set_compress`]: that writes outbound payload gzip on this split reply envelope; this opts out.
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
    ///
    /// Distinct from [`Self::compress`], which is `false` when unset on this split reply envelope.
    #[must_use]
    pub fn compress_is_set(&self) -> bool {
        self.compress.is_some()
    }

    /// Whether this payload is gzipped. See [`Response::compressed`].
    #[must_use]
    pub fn compressed(&self) -> bool {
        self.compress.unwrap_or(false)
    }

    /// The `grpc-encoding` token on a received reply, if any.
    /// See [`Response::encoding`].
    ///
    /// Distinct from [`Self::compressed`]: that is the unary Compressed-Flag (and outbound intent) on this split reply envelope; this is the HTTP header.
    #[must_use]
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Full gRPC path. See [`Response::path`].
    ///
    /// Distinct from [`crate::Request::path`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::path`]: that is a client interceptor before send, not this split reply envelope.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Service half of the path. See [`Response::service`].
    #[must_use]
    pub fn service(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).0)
    }

    /// Method half of the path. See [`Response::method`].
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        self.path.as_deref().map(|p| split_path(p).1)
    }

    /// Server encode overlay. See [`Response::gzip_level`].
    ///
    /// Distinct from [`Self::compress`]: that is on or off on this split reply envelope; this is deflate effort.
    /// Distinct from [`crate::Outgoing::gzip_level`]: that is a client interceptor overlay, not this split reply envelope's server overlay.
    /// Distinct from [`crate::Rpc::gzip_level`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`Self::encoding`]: that is the received `grpc-encoding` token on this split reply envelope.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.gzip_level
    }

    /// Server encode overlay. See [`Response::compresses_outbound`].
    ///
    /// Distinct from [`Self::compress`]: that is the per-RPC choice after overlay and interceptor mutation on this split reply envelope.
    /// Distinct from [`crate::Outgoing::compresses_outbound`]: that is a client interceptor overlay, not this split reply envelope's server overlay.
    /// Distinct from [`crate::Rpc::compresses_outbound`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`Self::gzip_level`]: that is deflate effort on this split reply envelope, not on or off.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.compresses_outbound
    }

    /// Peer gzip advertisement. See [`Response::accepts_gzip`].
    ///
    /// Distinct from [`Self::encoding`]: that is received `grpc-encoding`, not `grpc-accept-encoding` on this split reply envelope.
    /// Distinct from [`crate::Rpc::accepts_gzip`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Request::accepts_gzip`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`Self::compresses_outbound`]: that is the server encode overlay on this split reply envelope, not the peer advertisement.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay, not this split reply envelope's server overlay.
    #[must_use]
    pub fn accepts_gzip(&self) -> bool {
        self.accepts_gzip
    }

    /// Inbound gzip overlay. See [`Response::accepts_compressed`].
    ///
    /// Distinct from [`Self::accepts_gzip`]: that is the peer advertisement on this split reply envelope, not this overlay.
    /// Distinct from [`crate::Rpc::accepts_compressed`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::accepts_compressed`]: that is a client interceptor overlay, not this split reply envelope's inbound overlay.
    /// Distinct from [`Self::compresses_outbound`]: that is whether this reply is gzipped on this split reply envelope.
    /// Distinct from [`Self::encoding`]: that is received `grpc-encoding`, not this advertisement on this split reply envelope.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.accepts_compressed
    }

    /// Remaining Instant when writing. See [`Response::deadline`].
    ///
    /// Distinct from [`crate::Request::deadline`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`crate::Rpc::deadline`]: that is computed when that getter runs, not this split reply envelope.
    /// Distinct from [`crate::Request::timeout`]: that is the duration stamped at dispatch, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::deadline`]: that is a client interceptor Instant, not this split reply envelope.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Duration stamped at dispatch. See [`Response::timeout`].
    ///
    /// Distinct from [`crate::Request::timeout`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`Self::deadline`]: that is the Instant on this split reply envelope; this duration does not shrink.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the effective duration on this split reply envelope.
    /// Distinct from [`crate::Rpc::effective_timeout`]: that is computed when that getter runs, not this split reply envelope.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is the server overlay, not the effective cap on this split reply envelope.
    /// Distinct from [`crate::Request::peer_timeout`]: that is the client's `grpc-timeout`, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::timeout`]: that is a client interceptor duration, not this split reply envelope.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Client `grpc-timeout`. See [`Response::peer_timeout`].
    ///
    /// Distinct from [`crate::Request::peer_timeout`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`Self::timeout`]: that is the effective cap on this split reply envelope; this is the client's original header.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the client header on this split reply envelope.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is the server overlay, not the client header on this split reply envelope.
    /// Distinct from [`crate::Rpc::peer_timeout`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Rpc::effective_timeout`]: that is the soonest of the three caps on this split reply envelope.
    /// Distinct from [`Self::deadline`]: that is the Instant on this split reply envelope, not the client header.
    #[must_use]
    pub fn peer_timeout(&self) -> Option<Duration> {
        self.peer_timeout
    }

    /// Server timeout overlay. See [`Response::rpc_timeout`].
    ///
    /// Distinct from [`crate::Request::rpc_timeout`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`Self::timeout`]: that is the effective cap on this split reply envelope; this is the server overlay.
    /// Distinct from [`Self::peer_timeout`]: that is the client's `grpc-timeout` on this split reply envelope, not the server overlay.
    /// Distinct from [`crate::Rpc::rpc_timeout`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Rpc::timeout`]: that is the interceptor cap, not the server overlay on this split reply envelope.
    /// Distinct from [`crate::Outgoing::rpc_timeout`]: that is a client interceptor overlay, not this split reply envelope.
    /// Distinct from [`Self::deadline`]: that is the Instant on this split reply envelope, not the server overlay.
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.rpc_timeout
    }

    /// Encode caps when writing. See [`Response::limits`].
    ///
    /// Distinct from [`crate::Request::limits`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`crate::Rpc::limits`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::limits`]: that is a client interceptor overlay, not this split reply envelope.
    /// Distinct from [`Self::timeout`]: that is a duration on this split reply envelope, not a size cap.
    /// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is queue depth, not message size on this split reply envelope.
    #[must_use]
    pub fn limits(&self) -> Option<MessageLimits> {
        self.limits
    }

    /// Write-time send buffer when writing. See [`Response::send_buffer_size`].
    ///
    /// Distinct from [`crate::Request::send_buffer_size`]: that is the inbound request, not this split reply envelope.
    /// Distinct from [`crate::Rpc::send_buffer_size`]: that is a server interceptor before the handler, not this split reply envelope.
    /// Distinct from [`crate::Outgoing::send_buffer_size`]: that is a client interceptor overlay, not this split reply envelope's server overlay.
    /// Distinct from [`Self::limits`]: that is the encode cap on this split reply envelope, not this HTTP/2 send buffer.
    /// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is decoded-message queue depth, not this send buffer on this split reply envelope.
    #[must_use]
    pub fn send_buffer_size(&self) -> Option<usize> {
        self.send_buffer_size
    }

    /// Typed values on this envelope. See [`Response::extensions`].
    ///
    /// Distinct from [`Self::extensions_mut`]: that inserts typed values this split reply envelope carries; this borrows them.
    /// Distinct from [`Self::metadata`]: that is headers on this split reply envelope; this is typed local state, not on the wire.
    #[must_use]
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Insert typed values that stay on this envelope. See [`Response::extensions_mut`].
    ///
    /// Distinct from [`Self::extensions`]: that borrows them; this inserts typed values this split reply envelope carries.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }
}

impl<T: fmt::Debug> fmt::Debug for Response<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("message", &self.message)
            .field("metadata", &self.metadata)
            .field("trailers", &self.trailers)
            .field("compress", &self.compress)
            .field("encoding", &self.encoding)
            .field("path", &self.path())
            .field("service", &self.service())
            .field("method", &self.method())
            .field("gzip_level", &self.gzip_level)
            .field("compresses_outbound", &self.compresses_outbound)
            .field("accepts_gzip", &self.accepts_gzip)
            .field("accepts_compressed", &self.accepts_compressed)
            .field("deadline", &self.deadline)
            .field("timeout", &self.timeout)
            .field("peer_timeout", &self.peer_timeout)
            .field("rpc_timeout", &self.rpc_timeout)
            .field("limits", &self.limits)
            .field("send_buffer_size", &self.send_buffer_size)
            .field("extensions", &self.extensions.len())
            .finish()
    }
}

/// An RPC in flight.
///
/// Await it for the result. Dropping it without awaiting resets the HTTP/2
/// stream so the server drops the handler, the same as [`Self::cancel`].
/// Cancel while you still hold the future if you need the await to resolve
/// with [`Code::Cancelled`](crate::Code::Cancelled) rather than being dropped.
/// After a server-streaming or bidi call is Ready, a [`CallHandle`] taken
/// beforehand still resets the live stream; dropping the received
/// [`crate::Streaming`] before the end does the same. After a client-streaming
/// sender is closed, that handle still resets while the unary response is
/// pending.
///
/// After this future yields `Ready`, it is terminated
/// (`futures_core::future::FusedFuture`): combinators that skip terminated
/// futures will not poll it again. [`Self::is_cancelled`] is a separate
/// signal — a finished call is terminated even if it was never cancelled.
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
    /// Take it before awaiting. After this [`Call`] is Ready, the handle still
    /// cancels a live server-streaming or bidi response. It also cancels a
    /// server-streaming or bidi call that is still waiting for headers. After a
    /// client-streaming sender is closed, it still cancels while the unary
    /// response is pending. Dropping the [`Call`] or letting its deadline fire
    /// after that half-close resets the same way. A server-streaming or bidi
    /// deadline RSTs the send half whether or not headers have arrived. After
    /// those headers, that deadline still RSTs the parked send half.
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

impl<T> FusedFuture for Call<T> {
    fn is_terminated(&self) -> bool {
        self.done
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
            .field("terminated", &self.done)
            .finish_non_exhaustive()
    }
}

/// A cancel signal for an in-flight [`Call`], detached from the call itself.
///
/// Take it before awaiting. After a server-streaming or bidi [`Call`] has
/// resolved, [`Self::cancel`] still resets the live response stream — the
/// same as dropping the received [`crate::Streaming`] before the end. It
/// also cancels a server-streaming or bidi call that is still waiting for
/// headers. After a client-streaming sender is closed, it still resets
/// while the unary response is pending. Cancelling before any request
/// message (`cancel_after_begin`) is [`crate::Code::Cancelled`], not OK
/// from a half-close: hold the [`crate::StreamSender`] until the [`Call`]
/// settles, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
/// Dropping that [`Call`] or letting
/// its deadline fire after the half-close resets the same way. A
/// server-streaming or bidi deadline RSTs the send half whether or not
/// headers have arrived. After those headers, that deadline still RSTs the
/// parked send half.
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
        req.set_user_agent("call-site/1.0").expect("user-agent");
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
        req.set_rpc_timeout(Some(Duration::from_secs(9)));
        req.set_accepts_gzip(true);
        req.set_compresses_outbound(true);
        req.set_gzip_level(9);
        req.set_accepts_compressed(false);
        req.set_concurrent_rpc_limit(Some(4));
        req.set_send_buffer_size(123_456);
        req.set_encoding(Some("gzip".into()));
        let (message, mut parts) = req.into_message_and_parts();
        assert_eq!(message, 1);
        assert!(parts.wait_for_ready());
        assert!(parts.wait_for_ready_is_set());
        assert!(parts.compress());
        assert!(parts.compressed());
        assert!(parts.user_agent_is_set());
        assert!(
            parts
                .user_agent()
                .expect("override")
                .starts_with("call-site/1.0 "),
            "{:?}",
            parts.user_agent()
        );
        assert_eq!(parts.peer_timeout(), Some(Duration::from_secs(5)));
        assert_eq!(parts.rpc_timeout(), Some(Duration::from_secs(9)));
        assert!(parts.accepts_gzip());
        assert!(parts.compresses_outbound());
        assert_eq!(parts.gzip_level(), 9);
        assert!(!parts.accepts_compressed());
        assert_eq!(parts.concurrent_rpc_limit(), Some(4));
        assert_eq!(parts.send_buffer_size(), 123_456);
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
        parts.set_user_agent("parts/1.0").expect("parts user-agent");
        assert!(!parts.wait_for_ready_is_set());
        let shown_parts = format!("{parts:?}");
        assert!(
            shown_parts.contains("/helloworld.Greeter/SayHello"),
            "{shown_parts}"
        );
        assert!(shown_parts.contains("helloworld.Greeter"), "{shown_parts}");
        assert!(shown_parts.contains("SayHello"), "{shown_parts}");
        assert!(shown_parts.contains("peer_timeout: Some("), "{shown_parts}");
        assert!(shown_parts.contains("rpc_timeout: Some("), "{shown_parts}");
        assert!(shown_parts.contains("accepts_gzip: true"), "{shown_parts}");
        assert!(
            shown_parts.contains("compresses_outbound: true"),
            "{shown_parts}"
        );
        assert!(shown_parts.contains("gzip_level: 9"), "{shown_parts}");
        assert!(
            shown_parts.contains("accepts_compressed: false"),
            "{shown_parts}"
        );
        assert!(
            shown_parts.contains("concurrent_rpc_limit: Some(4)"),
            "{shown_parts}"
        );
        assert!(
            shown_parts.contains("send_buffer_size: 123456"),
            "{shown_parts}"
        );
        assert!(shown_parts.contains("encoding: Some("), "{shown_parts}");
        assert!(shown_parts.contains("user_agent: Some("), "{shown_parts}");
        let rebuilt = Request::<u32>::from_message_and_parts("swapped", parts);
        assert_eq!(rebuilt.timeout(), Some(Duration::from_millis(3)));
        assert_eq!(rebuilt.metadata().get("k"), Some("v"));
        assert!(!rebuilt.wait_for_ready());
        assert!(!rebuilt.compress());
        assert!(rebuilt.compress_is_set());
        assert!(rebuilt.compressed());
        assert!(rebuilt.user_agent_is_set());
        assert!(
            rebuilt
                .user_agent()
                .expect("override")
                .starts_with("parts/1.0 "),
            "{:?}",
            rebuilt.user_agent()
        );
        assert_eq!(rebuilt.peer_timeout(), Some(Duration::from_secs(5)));
        assert_eq!(rebuilt.rpc_timeout(), Some(Duration::from_secs(9)));
        assert!(rebuilt.accepts_gzip());
        assert!(rebuilt.compresses_outbound());
        assert_eq!(rebuilt.gzip_level(), 9);
        assert!(!rebuilt.accepts_compressed());
        assert_eq!(rebuilt.concurrent_rpc_limit(), Some(4));
        assert_eq!(rebuilt.send_buffer_size(), 123_456);
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
        assert!(shown.contains("rpc_timeout: Some("), "{shown}");
        assert!(shown.contains("accepts_gzip: true"), "{shown}");
        assert!(shown.contains("compresses_outbound: true"), "{shown}");
        assert!(shown.contains("gzip_level: 9"), "{shown}");
        assert!(shown.contains("accepts_compressed: false"), "{shown}");
        assert!(shown.contains("concurrent_rpc_limit: Some(4)"), "{shown}");
        assert!(shown.contains("send_buffer_size: 123456"), "{shown}");
        assert!(shown.contains("encoding: Some("), "{shown}");
        assert!(shown.contains("user_agent: Some("), "{shown}");
        assert!(shown.contains("/helloworld.Greeter/SayHello"), "{shown}");
        assert!(shown.contains("helloworld.Greeter"), "{shown}");
        assert!(shown.contains("SayHello"), "{shown}");
        let cloned = rebuilt.clone();
        assert_eq!(cloned.peer_timeout(), Some(Duration::from_secs(5)));
        assert_eq!(cloned.rpc_timeout(), Some(Duration::from_secs(9)));
        assert_eq!(cloned.encoding(), Some("gzip"));
        assert!(cloned.user_agent_is_set());
        assert_eq!(rebuilt.into_inner(), "swapped");
        assert!(Request::new(0u32).path().is_none());
        assert!(Request::new(0u32).service().is_none());
        assert!(Request::new(0u32).method().is_none());
        assert!(Request::new(0u32).peer_timeout().is_none());
        assert!(Request::new(0u32).rpc_timeout().is_none());
        assert!(!Request::new(0u32).accepts_gzip());
        assert!(!Request::new(0u32).compresses_outbound());
        assert!(Request::new(0u32).concurrent_rpc_limit().is_none());
        assert_eq!(
            Request::new(0u32).send_buffer_size(),
            crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE
        );
        assert!(Request::new(0u32).encoding().is_none());
        assert!(Request::new(0u32).user_agent().is_none());
        assert!(!Request::new(0u32).user_agent_is_set());
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
        inherit.set_user_agent("call-site/1.0").expect("set");
        inherit.clear_user_agent();
        assert!(inherit.user_agent().is_none());
        assert!(!inherit.user_agent_is_set());
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
        req.set_user_agent("call-site/1.0").expect("user-agent");
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
        assert!(mapped.user_agent_is_set());
        assert!(mapped
            .user_agent()
            .expect("override")
            .starts_with("call-site/1.0 "));
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
        resp.extensions_mut().insert(7u8);
        let mapped = resp.map(|n| n * 21);
        assert_eq!(mapped.metadata().get("h"), Some("v"));
        assert_eq!(mapped.trailers().get("t"), Some("1"));
        assert_eq!(mapped.extensions().get::<u8>().copied(), Some(7));
        assert!(mapped.compressed());
        assert!(mapped.compress());
        assert!(mapped.path().is_none());
        assert_eq!(
            mapped.gzip_level(),
            crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL
        );
        assert!(!mapped.compresses_outbound());
        assert!(!mapped.accepts_gzip());
        assert!(!mapped.accepts_compressed());
        assert!(mapped.deadline().is_none());
        assert!(mapped.timeout().is_none());
        assert!(mapped.peer_timeout().is_none());
        assert!(mapped.limits().is_none());
        assert!(mapped.send_buffer_size().is_none());
        let at = tokio::time::Instant::now() + Duration::from_secs(5);
        let stamped = mapped
            .with_path(Some("/helloworld.Greeter/SayHello".into()))
            .with_gzip_level(9)
            .with_compresses_outbound(true)
            .with_accepts_gzip(true)
            .with_accepts_compressed(true)
            .with_deadline(Some(at))
            .with_timeout(Some(Duration::from_secs(5)))
            .with_peer_timeout(Some(Duration::from_secs(30)))
            .with_rpc_timeout(Some(Duration::from_secs(9)))
            .with_limits(Some(crate::MessageLimits::default()))
            .with_send_buffer_size(Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE));
        assert_eq!(stamped.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(stamped.service(), Some("helloworld.Greeter"));
        assert_eq!(stamped.method(), Some("SayHello"));
        assert_eq!(stamped.gzip_level(), 9);
        assert!(stamped.compresses_outbound());
        assert!(stamped.accepts_gzip());
        assert!(stamped.accepts_compressed());
        assert_eq!(stamped.deadline(), Some(at));
        assert_eq!(stamped.timeout(), Some(Duration::from_secs(5)));
        assert_eq!(stamped.peer_timeout(), Some(Duration::from_secs(30)));
        assert_eq!(stamped.rpc_timeout(), Some(Duration::from_secs(9)));
        assert_eq!(stamped.limits(), Some(crate::MessageLimits::default()));
        assert_eq!(
            stamped.send_buffer_size(),
            Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)
        );
        let (n, mut parts) = stamped.into_message_and_parts();
        assert_eq!(n, 42);
        assert!(parts.compress());
        assert!(parts.encoding().is_none());
        assert_eq!(parts.extensions().get::<u8>().copied(), Some(7));
        assert_eq!(parts.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(parts.service(), Some("helloworld.Greeter"));
        assert_eq!(parts.method(), Some("SayHello"));
        assert_eq!(parts.gzip_level(), 9);
        assert!(parts.compresses_outbound());
        assert!(parts.accepts_gzip());
        assert!(parts.accepts_compressed());
        assert_eq!(parts.deadline(), Some(at));
        assert_eq!(parts.timeout(), Some(Duration::from_secs(5)));
        assert_eq!(parts.peer_timeout(), Some(Duration::from_secs(30)));
        assert_eq!(parts.rpc_timeout(), Some(Duration::from_secs(9)));
        assert_eq!(parts.limits(), Some(crate::MessageLimits::default()));
        assert_eq!(
            parts.send_buffer_size(),
            Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)
        );
        parts.set_compress(false);
        parts.extensions_mut().insert(9u8);
        assert!(!parts.compress());
        assert!(parts.compress_is_set());
        let rebuilt = Response::from_message_and_parts(n, parts);
        assert!(!rebuilt.compressed());
        assert!(!rebuilt.compress());
        assert!(rebuilt.compress_is_set());
        assert!(rebuilt.encoding().is_none());
        assert_eq!(rebuilt.metadata().get("h"), Some("v"));
        assert_eq!(rebuilt.extensions().get::<u8>().copied(), Some(9));
        assert_eq!(rebuilt.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(rebuilt.service(), Some("helloworld.Greeter"));
        assert_eq!(rebuilt.method(), Some("SayHello"));
        assert_eq!(rebuilt.gzip_level(), 9);
        assert!(rebuilt.compresses_outbound());
        assert!(rebuilt.accepts_gzip());
        assert!(rebuilt.accepts_compressed());
        assert_eq!(rebuilt.deadline(), Some(at));
        assert_eq!(rebuilt.timeout(), Some(Duration::from_secs(5)));
        assert_eq!(rebuilt.peer_timeout(), Some(Duration::from_secs(30)));
        assert_eq!(rebuilt.rpc_timeout(), Some(Duration::from_secs(9)));
        assert_eq!(rebuilt.limits(), Some(crate::MessageLimits::default()));
        assert_eq!(
            rebuilt.send_buffer_size(),
            Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)
        );
        let shown = format!("{rebuilt:?}");
        assert!(shown.contains("/helloworld.Greeter/SayHello"), "{shown}");
        assert!(shown.contains("helloworld.Greeter"), "{shown}");
        assert!(shown.contains("SayHello"), "{shown}");
        assert!(shown.contains("gzip_level: 9"), "{shown}");
        assert!(shown.contains("compresses_outbound: true"), "{shown}");
        assert!(shown.contains("accepts_gzip: true"), "{shown}");
        assert!(shown.contains("accepts_compressed: true"), "{shown}");
        assert!(shown.contains("deadline: Some("), "{shown}");
        assert!(shown.contains("timeout: Some("), "{shown}");
        assert!(shown.contains("peer_timeout: Some("), "{shown}");
        assert!(shown.contains("rpc_timeout: Some("), "{shown}");
        assert!(shown.contains("limits: Some("), "{shown}");
        assert!(shown.contains("send_buffer_size: Some("), "{shown}");
        assert_eq!(rebuilt.into_inner(), 42);
        let stamped = Response::new(1u32).with_encoding(Some("gzip".into()));
        assert_eq!(stamped.encoding(), Some("gzip"));
        assert!(stamped.extensions().get::<u8>().is_none());
        let shown = format!("{stamped:?}");
        assert!(shown.contains("encoding: Some("), "{shown}");
        assert!(shown.contains("extensions: 0"), "{shown}");
        let (_, parts) = stamped.into_message_and_parts();
        assert_eq!(parts.encoding(), Some("gzip"));
        let rebuilt = Response::from_message_and_parts(1u32, parts);
        assert_eq!(rebuilt.encoding(), Some("gzip"));
        assert!(Response::new(0u32).encoding().is_none());
        assert!(Response::new(0u32).extensions().get::<u8>().is_none());
        assert!(!Response::new(0u32).compresses_outbound());
        assert!(!Response::new(0u32).accepts_gzip());
        assert!(!Response::new(0u32).accepts_compressed());
        assert!(Response::new(0u32).deadline().is_none());
        assert!(Response::new(0u32).timeout().is_none());
        assert!(Response::new(0u32).peer_timeout().is_none());
        assert!(Response::new(0u32).rpc_timeout().is_none());
        assert!(Response::new(0u32).limits().is_none());
        assert!(Response::new(0u32).send_buffer_size().is_none());
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
                crate::ChannelConfig::default(),
            );
            assert_eq!(call.service(), "svc");
            assert_eq!(call.method(), "Method");
            assert_eq!(call.limits(), crate::MessageLimits::default());
            assert_eq!(call.timeout(), Some(Duration::from_secs(1)));
            assert!(call.deadline().is_some());
            assert!(!call.wait_for_ready_is_set());
            assert!(call.rpc_timeout().is_none());
            assert!(!call.waits_for_ready());
            assert!(!call.compresses_outbound());
            assert!(call.accepts_compressed());
            assert!(call.concurrent_rpc_limit().is_none());
            assert_eq!(call.stream_buffer_size(), crate::DEFAULT_STREAM_BUFFER);
            assert_eq!(call.send_buffer_size(), crate::DEFAULT_MAX_SEND_BUFFER_SIZE);
            assert!(!call.connected());
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
        assert!(shown.contains("connected: false"), "{shown}");
        let live = req
            .outgoing(
                "/svc/Method",
                "127.0.0.1:1",
                false,
                "pbrs-grpc/test",
                crate::ChannelConfig::default(),
            )
            .with_connected(true);
        assert!(live.connected());
        assert!(format!("{live:?}").contains("connected: true"), "{live:?}");
        let https = req.outgoing(
            "/svc/Method",
            "127.0.0.1:1",
            true,
            "pbrs-grpc/test",
            crate::ChannelConfig::default(),
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
                crate::ChannelConfig::default(),
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
                crate::ChannelConfig::default(),
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
    fn outgoing_channel_overlays_survive_clear() {
        let mut req = Request::new(());
        let config = crate::ChannelConfig::new()
            .timeout(Duration::from_secs(5))
            .wait_for_ready(true)
            .send_compressed(true)
            .accept_compressed(false)
            .max_concurrent_rpcs(4)
            .stream_buffer(32)
            .max_send_buffer_size(123_456);
        let mut call = req.outgoing(
            "/svc/Method",
            "127.0.0.1:1",
            false,
            "pbrs-grpc/test",
            config,
        );
        assert_eq!(call.rpc_timeout(), Some(Duration::from_secs(5)));
        assert!(call.waits_for_ready());
        assert!(call.compresses_outbound());
        assert!(!call.accepts_compressed());
        assert_eq!(call.concurrent_rpc_limit(), Some(4));
        assert_eq!(call.stream_buffer_size(), 32);
        assert_eq!(call.send_buffer_size(), 123_456);
        // Overlays are not copied onto the per-RPC fields until prepare_outbound.
        assert!(call.timeout().is_none());
        assert!(!call.wait_for_ready_is_set());
        assert!(!call.compress_is_set());
        call.set_timeout(Duration::from_secs(5));
        call.set_wait_for_ready(true);
        call.set_compress(true);
        call.clear_timeout();
        call.clear_wait_for_ready();
        call.clear_compress();
        assert!(call.timeout().is_none());
        assert!(!call.wait_for_ready_is_set());
        assert!(!call.compress_is_set());
        assert_eq!(call.rpc_timeout(), Some(Duration::from_secs(5)));
        assert!(call.waits_for_ready());
        assert!(call.compresses_outbound());
        let shown = format!("{call:?}");
        assert!(shown.contains("rpc_timeout"), "{shown}");
        assert!(shown.contains("waits_for_ready: true"), "{shown}");
        assert!(shown.contains("compresses_outbound: true"), "{shown}");
        assert!(shown.contains("concurrent_rpc_limit: Some(4)"), "{shown}");
        assert!(shown.contains("stream_buffer_size: 32"), "{shown}");
        assert!(shown.contains("send_buffer_size: 123456"), "{shown}");
    }

    #[test]
    fn outgoing_set_user_agent_prefixes_this_rpc() {
        let mut req = Request::new(());
        let mut call = req.outgoing(
            "/svc/Method",
            "127.0.0.1:1",
            false,
            "inventory/2.1 pbrs-grpc/test",
            crate::ChannelConfig::default(),
        );
        assert_eq!(call.user_agent(), "inventory/2.1 pbrs-grpc/test");
        assert!(!call.user_agent_is_set());
        call.set_user_agent("override/1.0").expect("set");
        assert!(call.user_agent().starts_with("override/1.0 "));
        assert!(call.user_agent().contains("pbrs-grpc/"));
        assert!(call.user_agent_is_set());
        let ua = call.user_agent();
        call.metadata_mut().set("x-ua", ua).expect("stamp");
        let stamped = call.metadata().get("x-ua").expect("x-ua");
        assert!(stamped.starts_with("override/1.0 "), "{stamped}");
        let shown = format!("{call:?}");
        assert!(shown.contains("override/1.0 "), "{shown}");
        call.clear_user_agent();
        assert!(!call.user_agent_is_set());
        assert_eq!(call.user_agent(), "inventory/2.1 pbrs-grpc/test");
        let err = call.set_user_agent("bad\nagent").expect_err("http");
        assert_eq!(err.code(), crate::status::Code::InvalidArgument);
    }

    #[test]
    fn request_set_user_agent_prefixes_this_rpc() {
        let mut req = Request::new(());
        assert!(req.user_agent().is_none());
        assert!(!req.user_agent_is_set());
        req.set_user_agent("call-site/1.0").expect("set");
        let override_ua = req.user_agent().expect("override");
        assert!(override_ua.starts_with("call-site/1.0 "), "{override_ua}");
        assert!(override_ua.contains("pbrs-grpc/"), "{override_ua}");
        assert!(req.user_agent_is_set());
        {
            let mut call = req.outgoing(
                "/svc/Method",
                "127.0.0.1:1",
                false,
                "inventory/2.1 pbrs-grpc/test",
                crate::ChannelConfig::default(),
            );
            assert!(call.user_agent_is_set());
            assert!(call.user_agent().starts_with("call-site/1.0 "));
            assert_ne!(call.user_agent(), "inventory/2.1 pbrs-grpc/test");
            call.set_user_agent("override/1.0").expect("interceptor");
            assert!(call.user_agent().starts_with("override/1.0 "));
            call.clear_user_agent();
            assert!(!call.user_agent_is_set());
            assert_eq!(call.user_agent(), "inventory/2.1 pbrs-grpc/test");
        }
        assert!(!req.user_agent_is_set());
        assert!(req.user_agent().is_none());
        req.set_user_agent("").expect("empty");
        assert!(req.user_agent_is_set());
        assert!(req.user_agent().expect("kernel").starts_with("pbrs-grpc/"));
        req.clear_user_agent();
        let err = req.set_user_agent("bad\nagent").expect_err("http");
        assert_eq!(err.code(), crate::status::Code::InvalidArgument);
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

    #[test]
    fn call_is_fused_after_ready() {
        use futures_core::future::FusedFuture;
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        let (tx, _rx) = tokio::sync::watch::channel(false);
        let mut call = super::Call::new(tx, Box::pin(async { Ok::<u32, crate::Status>(1) }));
        assert!(!call.is_terminated());
        let shown = format!("{call:?}");
        assert!(shown.contains("terminated: false"), "{shown}");
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(matches!(
            Pin::new(&mut call).poll(&mut cx),
            Poll::Ready(Ok(1))
        ));
        assert!(call.is_terminated());
        assert!(!call.is_cancelled());
        let shown = format!("{call:?}");
        assert!(shown.contains("terminated: true"), "{shown}");
    }
}
