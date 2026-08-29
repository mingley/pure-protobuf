//! gRPC client: [`Channel`] and the four call shapes.

use crate::config::{ChannelConfig, Wire};
use crate::interceptor::{ClientHook, ClientInterceptor};
use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::stream::{StreamSender, Streaming};
use crate::tls::ClientTls;
use crate::wire::{
    encode_msg, finish_stream, finish_unary, grpc_request, pump_outbound, send_bytes,
};
use bytes::Bytes;
use h2::Reason;
use http::uri::Authority;
use http::HeaderValue;
use pbrs::{Parse, Serialize};
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::sync::{watch, Mutex};

/// Where a [`Channel`] should dial.
///
/// Built with [`From`], so [`Channel::connect`] (and generated
/// `FooClient::connect`) takes a `SocketAddr`, a `&str` of the form
/// `host:port`, or a `String`.
///
/// ```
/// use pbrs_grpc::Target;
///
/// let from_addr: Target = "127.0.0.1:50051".parse::<std::net::SocketAddr>()?.into();
/// let from_name: Target = "greeter.internal:50051".into();
/// assert_eq!(from_addr.authority(), "127.0.0.1:50051");
/// assert_eq!(from_name.authority(), "greeter.internal:50051");
/// # Ok::<(), std::net::AddrParseError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Target {
    authority: String,
}

impl Target {
    /// The `host:port` string used both for DNS and for `:authority`.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    fn parse(&self) -> Result<Authority, Status> {
        self.authority.parse().map_err(|e| {
            Status::unavailable(format!("invalid authority {:?}: {e}", self.authority))
        })
    }
}

impl From<SocketAddr> for Target {
    fn from(addr: SocketAddr) -> Self {
        Self {
            authority: addr.to_string(),
        }
    }
}

impl From<&str> for Target {
    fn from(authority: &str) -> Self {
        Self {
            authority: authority.to_owned(),
        }
    }
}

impl From<String> for Target {
    fn from(authority: String) -> Self {
        Self { authority }
    }
}

impl From<&String> for Target {
    fn from(authority: &String) -> Self {
        Self {
            authority: authority.clone(),
        }
    }
}

/// One pooled HTTP/2 client. `gen` changes whenever the slot is redialed, so a
/// grabber that observed the previous generation die does not overwrite a
/// reconnect that already landed. `send` is `None` until the first successful
/// handshake on a lazy channel, and after a dead handle is discarded or the
/// slot idle-closes.
struct ConnSlot {
    gen: u64,
    send: Option<h2::client::SendRequest<Bytes>>,
    /// Stops the connection driver (idle close, lost-race handshake, drop).
    stop: Option<watch::Sender<bool>>,
    /// Outstanding RPCs; `None` when idle-close is not configured.
    busy: Option<Arc<crate::keepalive::Busy>>,
}

/// A finished handshake: the sender plus the handles that stop its driver.
struct Dialed {
    send: h2::client::SendRequest<Bytes>,
    stop: watch::Sender<bool>,
    busy: Option<Arc<crate::keepalive::Busy>>,
}

/// A sender taken from a pool slot, plus the generation so a raced `GOAWAY`
/// can discard this slot instead of writing into a reconnect that already
/// landed.
struct LiveConn {
    send: h2::client::SendRequest<Bytes>,
    lease: Option<crate::keepalive::Lease>,
    slot: usize,
    gen: u64,
}

/// Backoff between wait-for-ready handshake attempts, in milliseconds.
/// Caps at the last entry; see [`ChannelInner::acquire`].
const WAIT_FOR_READY_BACKOFF_MS: &[u64] = &[20, 40, 80, 160, 320, 640, 1000];

struct ChannelInner {
    slots: Vec<Mutex<ConnSlot>>,
    next: AtomicUsize,
    authority: Authority,
    endpoint: Endpoint,
    tls: Option<ClientTls>,
    /// Settings used to dial. Per-clone overlays on [`Channel`] (timeout,
    /// wait-for-ready, send_compressed, message sizes, stream_buffer) do not
    /// change how a dead slot is redialed.
    dial: ChannelConfig,
}

/// Where a handshake should connect. TCP is `host:port`; Unix is a filesystem
/// path. HTTP/2 `:authority` for a Unix socket is `localhost`. [`Self::Once`]
/// is an already-connected stream that cannot be redialed.
#[derive(Clone)]
enum Endpoint {
    Tcp(String),
    #[cfg(unix)]
    Unix(PathBuf),
    Once,
}

impl Endpoint {
    fn describe(&self) -> String {
        match self {
            Self::Tcp(host) => host.clone(),
            #[cfg(unix)]
            Self::Unix(path) => path.display().to_string(),
            Self::Once => "once".to_owned(),
        }
    }

    fn can_redial(&self) -> bool {
        !matches!(self, Self::Once)
    }
}

/// A prior-knowledge HTTP/2 connection (or small pool) to a gRPC server.
///
/// Cloning is cheap and shares the underlying connections, so a `Channel` is
/// meant to be cloned into every task that needs it.
///
/// If a connection dies — peer `GOAWAY`, TCP reset, keepalive timeout — the
/// next RPC on that slot dials again. Unary and server-streaming calls that
/// observe the death after the slot still looked live (a raced `GOAWAY`)
/// retry that redial once on the same RPC, matching gRPC transparent retry.
/// Client-streaming and bidi do not: the caller already holds the send half.
/// A healthy connection that is only waiting for a free stream
/// (`SETTINGS_MAX_CONCURRENT_STREAMS`) is not replaced. Redial is part of
/// the RPC: it is cancelled if the [`Call`] is cancelled, and it fails with
/// [`Code::DeadlineExceeded`] if the request deadline elapses while connecting.
///
/// A connection with no outstanding RPCs is closed after
/// [`ChannelConfig::max_connection_idle`] when that is set. Keepalive PINGs
/// do not keep it. The next RPC redials, except [`Self::from_io`], which
/// cannot redial and fails with [`Code::Unavailable`].
///
/// [`Self::connect_lazy`] skips the initial dial so a client can exist
/// before its server. The first RPC fails fast with [`Code::Unavailable`]
/// unless that request set [`Request::set_wait_for_ready`] or the channel
/// was built with [`Self::wait_for_ready`] / [`ChannelConfig::wait_for_ready`],
/// in which case it retries until connected, cancelled, or the deadline fires.
///
/// A dial is bounded by [`ChannelConfig::connect_timeout`] (default 20 s),
/// covering TCP or Unix connect, optional TLS, and the peer's HTTP/2
/// SETTINGS. A peer that accepts the socket and never speaks fails with
/// [`Code::Unavailable`] instead of hanging forever. Connection refused
/// still fails immediately.
///
/// On Unix, [`Self::connect_unix`] / [`Self::connect_unix_lazy`] speak the
/// same protocol over a domain socket. TLS is TCP-only.
///
/// [`Self::from_io`] speaks over an already-connected byte stream and cannot
/// redial. Pair it with [`crate::Server::serve_connection`] for in-process
/// tests.
///
/// Generated `FooClient` types wrap these constructors as `FooClient::connect`,
/// `connect_tls`, `connect_unix`, and `from_io`, so a service crate rarely
/// constructs a `Channel` by hand.
///
/// [`Self::intercept`] runs on every outbound RPC before the stream opens,
/// which is how a client injects auth metadata, a default deadline, or
/// wait-for-ready without touching each call.
///
/// After connect, [`Self::timeout`], [`Self::wait_for_ready`],
/// [`Self::send_compressed`], the two message-size caps /
/// [`Self::message_limits`], and [`Self::stream_buffer`] overlay this clone.
/// Keepalive, idle, TCP keepalive, connection count, HTTP/2 windows, and
/// the rapid-reset cap are set at handshake ([`ChannelConfig`] /
/// [`Self::connect_with`]).
///
/// [`Debug`] prints the authority, pool size, and config. It does not dump
/// live HTTP/2 state.
///
/// ```no_run
/// use pbrs_grpc::{Channel, ChannelConfig};
///
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// // One connection, 4 MiB inbound cap.
/// let channel = Channel::connect("127.0.0.1:50051").await?;
///
/// // Four connections, so four cores can drive HTTP/2 framing.
/// let pooled = Channel::connect_with(
///     "127.0.0.1:50051",
///     ChannelConfig::new().connections(4),
/// )
/// .await?;
///
/// // No dial until the first RPC. Pair with `Channel::wait_for_ready`
/// // or `Request::set_wait_for_ready`.
/// let late = Channel::connect_lazy("127.0.0.1:50051")?;
/// # let _ = (channel, pooled, late);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Channel {
    inner: Arc<ChannelInner>,
    config: ChannelConfig,
    interceptors: Arc<[ClientHook]>,
    user_agent: HeaderValue,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field("authority", &self.inner.authority.as_str())
            .field("endpoint", &self.inner.endpoint.describe())
            .field("connections", &self.inner.slots.len())
            .field("tls", &self.inner.tls.is_some())
            .field("interceptors", &self.interceptors.len())
            .field("config", &self.config)
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

impl Channel {
    /// Dial `target` with default configuration: one connection, 4 MiB
    /// inbound cap.
    pub async fn connect(target: impl Into<Target>) -> Result<Self, Status> {
        Self::connect_with(target, ChannelConfig::default()).await
    }

    /// Dial `target` with `config`.
    ///
    /// Opens [`ChannelConfig::connections`] connections up front; RPCs are
    /// spread over them round-robin. All of them must succeed. A slot that
    /// later dies is redialed on the next RPC that lands on it. Each dial is
    /// bounded by [`ChannelConfig::connect_timeout`].
    pub async fn connect_with(
        target: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        connect_inner(target.into(), config, None).await
    }

    /// Shorthand for [`Self::connect_with`] with `connections` connections.
    ///
    /// One connection means one `h2` driver task, so concurrent small RPCs
    /// serialize behind a single core's framing work. Pooling is the fix.
    pub async fn connect_pool(
        target: impl Into<Target>,
        connections: usize,
    ) -> Result<Self, Status> {
        Self::connect_with(target, ChannelConfig::default().connections(connections)).await
    }

    /// Dial `target` over TLS with default configuration.
    ///
    /// `target` is the TCP address; [`ClientTls`] carries the name verified
    /// against the certificate, which can be different (dial `127.0.0.1`,
    /// verify `localhost`).
    pub async fn connect_tls(target: impl Into<Target>, tls: ClientTls) -> Result<Self, Status> {
        Self::connect_tls_with(target, ChannelConfig::default(), tls).await
    }

    /// Dial `target` over TLS with `config`.
    pub async fn connect_tls_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        tls: ClientTls,
    ) -> Result<Self, Status> {
        connect_inner(target.into(), config, Some(tls)).await
    }

    /// Build a channel that dials on the first RPC instead of now.
    ///
    /// Invalid `target` still fails immediately. A closed port, a name that
    /// does not resolve, or a TLS handshake the peer refuses surfaces on the
    /// RPC as [`Code::Unavailable`], or waits until the deadline if that RPC
    /// set [`Request::set_wait_for_ready`] or this channel used
    /// [`Self::wait_for_ready`].
    pub fn connect_lazy(target: impl Into<Target>) -> Result<Self, Status> {
        Self::connect_lazy_with(target, ChannelConfig::default())
    }

    /// [`Self::connect_lazy`] with `config`. Each slot dials when an RPC first
    /// lands on it, not all at once.
    pub fn connect_lazy_with(
        target: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        connect_lazy_inner(target.into(), config, None)
    }

    /// [`Self::connect_lazy`] over TLS.
    pub fn connect_tls_lazy(target: impl Into<Target>, tls: ClientTls) -> Result<Self, Status> {
        Self::connect_tls_lazy_with(target, ChannelConfig::default(), tls)
    }

    /// [`Self::connect_lazy_with`] over TLS.
    pub fn connect_tls_lazy_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        tls: ClientTls,
    ) -> Result<Self, Status> {
        connect_lazy_inner(target.into(), config, Some(tls))
    }

    /// Dial a Unix domain socket with default configuration.
    ///
    /// h2c only; TLS over a Unix socket is not supported. `:authority` is
    /// `localhost`. `path` is a filesystem path, not a `unix://` URI.
    #[cfg(unix)]
    pub async fn connect_unix(path: impl AsRef<Path>) -> Result<Self, Status> {
        Self::connect_unix_with(path, ChannelConfig::default()).await
    }

    /// [`Self::connect_unix`] with `config`.
    #[cfg(unix)]
    pub async fn connect_unix_with(
        path: impl AsRef<Path>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        connect_unix_inner(path.as_ref(), config).await
    }

    /// [`Self::connect_unix`] that dials on the first RPC instead of now.
    #[cfg(unix)]
    pub fn connect_unix_lazy(path: impl AsRef<Path>) -> Result<Self, Status> {
        Self::connect_unix_lazy_with(path, ChannelConfig::default())
    }

    /// [`Self::connect_unix_lazy`] with `config`.
    #[cfg(unix)]
    pub fn connect_unix_lazy_with(
        path: impl AsRef<Path>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        Ok(finish_channel(
            Endpoint::Unix(path.as_ref().to_owned()),
            unix_authority(),
            config,
            None,
            empty_slots(config.connection_count()),
        ))
    }

    /// Speak gRPC over an already-connected byte stream.
    ///
    /// The channel has one slot and cannot redial: if the stream dies, the
    /// next RPC fails with [`Code::Unavailable`]. There is no TCP connect,
    /// no TLS, and no Unix path. Pair with [`crate::Server::serve_connection`]
    /// over `tokio::io::duplex` or `tokio::net::UnixStream::pair`.
    ///
    /// `authority` is the HTTP/2 `:authority` sent on every RPC.
    /// [`ChannelConfig::connections`] is ignored (always one slot).
    ///
    /// ```no_run
    /// # async fn run(
    /// #     io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    /// # ) -> Result<(), pbrs_grpc::Status> {
    /// let channel = pbrs_grpc::Channel::from_io(io, "localhost").await?;
    /// # let _ = channel;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_io<IO>(io: IO, authority: impl Into<Target>) -> Result<Self, Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_io_with(io, authority, ChannelConfig::default()).await
    }

    /// [`Self::from_io`] with `config`.
    pub async fn from_io_with<IO>(
        io: IO,
        authority: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let target = authority.into();
        let parsed = target.parse()?;
        let config = config.connections(1);
        let timeout = config.handshake_timeout();
        let send = match tokio::time::timeout(timeout, finish_h2(config, io)).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(Status::unavailable(format!(
                    "connect {parsed}: timed out after {timeout:?}"
                )));
            }
        };
        Ok(finish_channel(
            Endpoint::Once,
            parsed,
            config,
            None,
            live_slots(vec![send]),
        ))
    }

    /// The configuration in effect.
    #[must_use]
    pub fn config(&self) -> ChannelConfig {
        self.config
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

    /// Replace both message caps at once. See [`ChannelConfig::message_limits`].
    ///
    /// Overlay: applies to RPCs from this clone. Does not change how a dead
    /// slot is redialed.
    #[must_use]
    pub fn message_limits(mut self, limits: crate::MessageLimits) -> Self {
        self.config = self.config.message_limits(limits);
        self
    }

    /// gzip every unary request payload, and every
    /// [`crate::StreamSender::send`] on a client- or bidi-stream opened from
    /// this channel.
    ///
    /// Off by default. Equivalent to [`ChannelConfig::send_compressed`].
    /// A later interceptor can still call [`crate::Outgoing::set_compress`].
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// Default per-RPC deadline when the request omits one. See
    /// [`ChannelConfig::timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// Wait for a connection instead of failing fast. See
    /// [`ChannelConfig::wait_for_ready`].
    ///
    /// A request that already called [`crate::Request::set_wait_for_ready`]
    /// is left alone. Interceptors run after this fill and can still set
    /// or clear it.
    #[must_use]
    pub fn wait_for_ready(mut self) -> Self {
        self.config = self.config.wait_for_ready(true);
        self
    }

    /// How many messages sit between a client-streaming caller and the wire.
    /// See [`ChannelConfig::stream_buffer`].
    ///
    /// Overlay: applies to streams opened from this clone. Does not change
    /// already-open streams or how a dead slot is redialed.
    #[must_use]
    pub fn stream_buffer(mut self, messages: usize) -> Self {
        self.config = self.config.stream_buffer(messages);
        self
    }

    /// Prefix the kernel `user-agent`, matching grpc-go `WithUserAgent`.
    ///
    /// `user_agent("my-app/1.0")` sends `my-app/1.0 pbrs-grpc/<version>`.
    /// The kernel suffix is always present so a peer can identify the stack.
    /// Empty or whitespace-only prefix restores the kernel identity alone.
    ///
    /// ```
    /// # fn demo(channel: pbrs_grpc::Channel) -> Result<(), pbrs_grpc::Status> {
    /// let channel = channel.user_agent("inventory/2.1")?;
    /// assert!(channel.grpc_user_agent().starts_with("inventory/2.1 "));
    /// # let _ = channel;
    /// # Ok(())
    /// # }
    /// ```
    pub fn user_agent(mut self, prefix: impl AsRef<str>) -> Result<Self, Status> {
        self.user_agent = crate::wire::user_agent_value(prefix.as_ref())?;
        Ok(self)
    }

    /// The `user-agent` sent on every RPC.
    #[must_use]
    pub fn grpc_user_agent(&self) -> &str {
        self.user_agent.to_str().unwrap_or(crate::wire::DEFAULT_UA)
    }

    /// Run `interceptor` on every outbound RPC before the stream opens.
    /// Calling this twice stacks: the first interceptor runs first. The
    /// interceptor sees the method path, `:authority`, `:scheme`, and
    /// `user-agent`, and can set metadata, a deadline, wait-for-ready,
    /// compression, or typed extensions.
    /// Values the caller put on [`crate::Request::extensions_mut`] are
    /// visible; stacked interceptors share that map.
    #[must_use]
    pub fn intercept(self, interceptor: impl ClientInterceptor) -> Self {
        let mut hooks: Vec<ClientHook> = self.interceptors.iter().cloned().collect();
        hooks.push(Arc::new(interceptor));
        Self {
            interceptors: hooks.into(),
            ..self
        }
    }

    fn prepare_outbound<T>(&self, path: &'static str, req: &mut Request<T>) -> Result<(), Status> {
        if req.timeout().is_none() {
            if let Some(timeout) = self.config.rpc_timeout() {
                req.set_timeout(timeout);
            }
        }
        if !req.wait_for_ready_is_set() && self.config.waits_for_ready() {
            req.set_wait_for_ready(true);
        }
        if self.config.compresses_outbound() {
            req.set_compress(true);
        }
        self.apply_interceptors(path, req)
    }

    fn apply_interceptors<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
    ) -> Result<(), Status> {
        for hook in self.interceptors.iter() {
            hook.intercept(&mut req.outgoing(
                path,
                self.authority(),
                self.inner.tls.is_some(),
                self.grpc_user_agent(),
            ))?;
        }
        Ok(())
    }

    /// The `:authority` sent with every request.
    #[must_use]
    pub fn authority(&self) -> &str {
        self.inner.authority.as_str()
    }

    /// Wait for a live HTTP/2 sender, redialing this slot if the current one
    /// is dead. Raced against the RPC's deadline and cancel signal so a
    /// hanging reconnect cannot outlive the call. `wait_for_ready` retries
    /// a failed handshake until that race fires.
    async fn grab(
        &self,
        cancel_rx: watch::Receiver<bool>,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
    ) -> Result<LiveConn, Status> {
        let inner = Arc::clone(&self.inner);
        let grabbed = prefer_deadline(
            first_of(inner.acquire(wait_for_ready), cancel_rx, deadline).await,
            deadline,
        )?;
        if deadline.is_some_and(|at| tokio::time::Instant::now() >= at) {
            return Err(Status::deadline_exceeded());
        }
        Ok(grabbed)
    }

    /// Issue a unary RPC: one request message, one response message.
    ///
    /// `path` is the full gRPC path, `/<package>.<Service>/<Method>`.
    /// Generated clients call this for you.
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let mut req = HelloRequest::new();
    /// req.set_name("world");
    /// let reply: HelloReply = channel
    ///     .unary("/helloworld.Greeter/SayHello", Request::new(req))
    ///     .await?
    ///     .into_inner();
    /// # let _ = reply;
    /// # Ok(())
    /// # }
    /// ```
    pub fn unary<Req, Resp>(&self, path: &'static str, req: Request<Req>) -> Call<Response<Resp>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let wire = self.config.wire();
        Call::new(
            cancel,
            Box::pin(async move {
                let mut req = req;
                channel.prepare_outbound(path, &mut req)?;
                let wait = req.wait_for_ready();
                let deadline = deadline_from(req.timeout());
                let (msg, md, timeout, compress) = req.into_parts();
                // Encode before opening so an oversize message never occupies a
                // stream slot, and a transparent retry does not re-serialize.
                let frame = encode_msg(&msg, compress, wire.limits)?;
                let https = channel.inner.tls.is_some();
                let ua = channel.user_agent.clone();
                let mut retried = false;
                loop {
                    let live = channel.grab(cancel_rx.clone(), deadline, wait).await?;
                    let (slot, gen) = (live.slot, live.gen);
                    match run_unary(
                        live.send,
                        &channel.inner.authority,
                        path,
                        &md,
                        timeout,
                        compress,
                        frame.clone(),
                        cancel_rx.clone(),
                        wire,
                        ua.clone(),
                        https,
                    )
                    .await
                    {
                        Err(status)
                            if !retried
                                && status.is_transport()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            retried = true;
                            channel.inner.discard(slot, gen).await;
                        }
                        result => return result,
                    }
                }
            }),
        )
    }

    /// Issue a server-streaming RPC: one request message, many responses.
    pub fn server_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<Req>,
    ) -> Call<Response<Streaming<Resp>>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let wire = self.config.wire();
        Call::new(
            cancel,
            Box::pin(async move {
                let mut req = req;
                channel.prepare_outbound(path, &mut req)?;
                let wait = req.wait_for_ready();
                let deadline = deadline_from(req.timeout());
                let (msg, md, timeout, compress) = req.into_parts();
                // Encode before opening so an oversize message never occupies a
                // stream slot, and a transparent retry does not re-serialize.
                let frame = encode_msg(&msg, compress, wire.limits)?;
                let https = channel.inner.tls.is_some();
                let ua = channel.user_agent.clone();
                let mut retried = false;
                loop {
                    let live = channel.grab(cancel_rx.clone(), deadline, wait).await?;
                    let (slot, gen, lease) = (live.slot, live.gen, live.lease);
                    match run_server_stream(
                        live.send,
                        &channel.inner.authority,
                        path,
                        &md,
                        timeout,
                        compress,
                        frame.clone(),
                        cancel_rx.clone(),
                        wire,
                        ua.clone(),
                        https,
                    )
                    .await
                    {
                        Ok(response) => return Ok(attach_lease(response, lease)),
                        Err(status)
                            if !retried
                                && status.is_transport()
                                && channel.inner.endpoint.can_redial() =>
                        {
                            retried = true;
                            channel.inner.discard(slot, gen).await;
                        }
                        Err(status) => return Err(status),
                    }
                }
            }),
        )
    }

    /// Issue a client-streaming RPC: many request messages, one response.
    ///
    /// Send on the returned [`StreamSender`], drop it to half-close, then
    /// await the [`Call`].
    ///
    /// ```no_run
    /// # use pbrs_grpc::{Channel, HelloReply, HelloRequest, Request};
    /// # async fn run(channel: Channel) -> Result<(), pbrs_grpc::Status> {
    /// let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(
    ///     "/helloworld.Greeter/ClientHello",
    ///     Request::new(()),
    /// );
    /// for name in ["ada", "grace"] {
    ///     let mut req = HelloRequest::new();
    ///     req.set_name(name);
    ///     tx.send(req).await?;
    /// }
    /// tx.close();
    /// let reply = call.await?.into_inner();
    /// # let _ = reply;
    /// # Ok(())
    /// # }
    /// ```
    pub fn client_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamSender<Req>, Call<Response<Resp>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let wire = self.config.wire();
        let (tx, rx) = Streaming::channel(self.config.stream_buffer_size());
        let tx = tx
            .with_limits(wire.limits)
            .with_compress(self.config.compresses_outbound());
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let call = Call::new(
            cancel,
            Box::pin(async move {
                let mut req = req;
                channel.prepare_outbound(path, &mut req)?;
                let wait = req.wait_for_ready();
                let deadline = deadline_from(req.timeout());
                let live = channel.grab(cancel_rx.clone(), deadline, wait).await?;
                run_client_stream(
                    live.send,
                    &channel.inner.authority,
                    path,
                    req,
                    rx,
                    cancel_rx,
                    wire,
                    channel.user_agent.clone(),
                    channel.inner.tls.is_some(),
                )
                .await
            }),
        );
        (tx, call)
    }

    /// Issue a bidirectional-streaming RPC.
    pub fn bidi<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamSender<Req>, Call<Response<Streaming<Resp>>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let wire = self.config.wire();
        let buffer = self.config.stream_buffer_size();
        let (tx, rx) = Streaming::channel(buffer);
        let tx = tx
            .with_limits(wire.limits)
            .with_compress(self.config.compresses_outbound());
        let (cancel, cancel_rx) = watch::channel(false);
        let channel = self.clone();
        let call = Call::new(
            cancel,
            Box::pin(async move {
                let mut req = req;
                channel.prepare_outbound(path, &mut req)?;
                let wait = req.wait_for_ready();
                let deadline = deadline_from(req.timeout());
                let live = channel.grab(cancel_rx.clone(), deadline, wait).await?;
                Ok(attach_lease(
                    run_bidi(
                        live.send,
                        &channel.inner.authority,
                        path,
                        req,
                        rx,
                        cancel_rx,
                        wire,
                        channel.user_agent.clone(),
                        channel.inner.tls.is_some(),
                    )
                    .await?,
                    live.lease,
                ))
            }),
        );
        (tx, call)
    }
}

async fn connect_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Tcp(target.authority().to_owned());
    let authority = target.parse()?;
    let n = config.connection_count();
    let mut sends = Vec::with_capacity(n);
    for _ in 0..n {
        sends.push(handshake(&endpoint, config, tls.as_ref()).await?);
    }
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        live_slots(sends),
    ))
}

fn connect_lazy_inner(
    target: Target,
    config: ChannelConfig,
    tls: Option<ClientTls>,
) -> Result<Channel, Status> {
    let endpoint = Endpoint::Tcp(target.authority().to_owned());
    let authority = target.parse()?;
    Ok(finish_channel(
        endpoint,
        authority,
        config,
        tls,
        empty_slots(config.connection_count()),
    ))
}

#[cfg(unix)]
async fn connect_unix_inner(path: &Path, config: ChannelConfig) -> Result<Channel, Status> {
    let endpoint = Endpoint::Unix(path.to_owned());
    let n = config.connection_count();
    let mut sends = Vec::with_capacity(n);
    for _ in 0..n {
        sends.push(handshake(&endpoint, config, None).await?);
    }
    Ok(finish_channel(
        endpoint,
        unix_authority(),
        config,
        None,
        live_slots(sends),
    ))
}

fn finish_channel(
    endpoint: Endpoint,
    authority: Authority,
    config: ChannelConfig,
    tls: Option<ClientTls>,
    slots: Vec<Mutex<ConnSlot>>,
) -> Channel {
    let inner = Arc::new(ChannelInner {
        slots,
        next: AtomicUsize::new(0),
        authority,
        endpoint,
        tls,
        dial: config,
    });
    for i in 0..inner.slots.len() {
        spawn_idle_watch(Arc::clone(&inner), i);
    }
    Channel {
        inner,
        config,
        interceptors: Arc::from([]),
        user_agent: crate::wire::PBRS_GRPC_UA,
    }
}

fn live_slots(dialed: Vec<Dialed>) -> Vec<Mutex<ConnSlot>> {
    dialed
        .into_iter()
        .map(|d| {
            Mutex::new(ConnSlot {
                gen: 0,
                send: Some(d.send),
                stop: Some(d.stop),
                busy: d.busy,
            })
        })
        .collect()
}

fn empty_slots(n: usize) -> Vec<Mutex<ConnSlot>> {
    (0..n)
        .map(|_| {
            Mutex::new(ConnSlot {
                gen: 0,
                send: None,
                stop: None,
                busy: None,
            })
        })
        .collect()
}

#[cfg(unix)]
fn unix_authority() -> Authority {
    Authority::from_static("localhost")
}

impl ChannelInner {
    fn pick(&self) -> Result<usize, Status> {
        let n = self.slots.len();
        if n == 0 {
            return Err(Status::unavailable("empty connection pool"));
        }
        if n == 1 {
            Ok(0)
        } else {
            Ok(self.next.fetch_add(1, Ordering::Relaxed) % n)
        }
    }

    fn slot(&self, i: usize) -> Result<&Mutex<ConnSlot>, Status> {
        self.slots
            .get(i)
            .ok_or_else(|| Status::unavailable("empty connection pool"))
    }

    /// Clone a live sender for this slot, redialing only when `ready` reports
    /// the connection is gone or the slot has never been dialed. `ready`
    /// waiting on stream capacity is not treated as death: that wait happens
    /// without holding the slot lock. Handshake and wait-for-ready backoff
    /// also run without the lock, so a down peer cannot stall other RPCs on
    /// the same slot. A `GOAWAY` that races after `ready` is handled by
    /// discarding that generation and retrying once on unary and
    /// server-streaming.
    async fn acquire(self: &Arc<Self>, wait_for_ready: bool) -> Result<LiveConn, Status> {
        let i = self.pick()?;
        let mut attempt = 0usize;
        loop {
            let (handle, lease, gen) = {
                let slot = self.slot(i)?.lock().await;
                let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                (slot.send.clone(), lease, slot.gen)
            };
            if let Some(handle) = handle {
                if let Ok(ready) = handle.ready().await {
                    return Ok(LiveConn {
                        send: ready,
                        lease,
                        slot: i,
                        gen,
                    });
                }
            }
            drop(lease);
            match handshake(&self.endpoint, self.dial, self.tls.as_ref()).await {
                Ok(dialed) => {
                    let mut slot = self.slot(i)?.lock().await;
                    if slot.gen == gen {
                        let send = store_dialed(&mut slot, dialed);
                        let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                        let gen = slot.gen;
                        drop(slot);
                        spawn_idle_watch(Arc::clone(self), i);
                        return Ok(LiveConn {
                            send,
                            lease,
                            slot: i,
                            gen,
                        });
                    }
                    dialed.stop.send(true).ok();
                }
                Err(_) if wait_for_ready && self.endpoint.can_redial() => {
                    let delay_ms = WAIT_FOR_READY_BACKOFF_MS
                        .get(attempt)
                        .copied()
                        .unwrap_or(1000);
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                Err(status) => return Err(status),
            }
        }
    }

    /// Drop a dead generation so the next [`Self::acquire`] redials.
    ///
    /// A raced `GOAWAY` can land after `ready` succeeded. Without this, the
    /// same dying sender would be handed out again. A reconnect that already
    /// stored a newer `gen` is left alone.
    async fn discard(&self, i: usize, gen: u64) {
        let Ok(lock) = self.slot(i) else {
            return;
        };
        let mut slot = lock.lock().await;
        if slot.gen != gen {
            return;
        }
        slot.send = None;
        slot.busy = None;
        if let Some(stop) = slot.stop.take() {
            stop.send(true).ok();
        }
        slot.gen = slot.gen.wrapping_add(1);
    }
}

fn store_dialed(slot: &mut ConnSlot, dialed: Dialed) -> h2::client::SendRequest<Bytes> {
    if let Some(stop) = slot.stop.take() {
        stop.send(true).ok();
    }
    slot.gen = slot.gen.wrapping_add(1);
    slot.send = Some(dialed.send.clone());
    slot.stop = Some(dialed.stop);
    slot.busy = dialed.busy;
    dialed.send
}

fn spawn_idle_watch(inner: Arc<ChannelInner>, i: usize) {
    let Some(idle) = inner.dial.connection_idle() else {
        return;
    };
    drop(tokio::spawn(async move {
        let (gen, busy) = {
            let Ok(slot) = inner.slot(i) else {
                return;
            };
            let slot = slot.lock().await;
            match slot.busy.as_ref() {
                Some(busy) => (slot.gen, Arc::clone(busy)),
                None => return,
            }
        };
        idle_watch(inner, i, gen, busy, idle).await;
    }));
}

async fn idle_watch(
    inner: Arc<ChannelInner>,
    i: usize,
    gen: u64,
    busy: Arc<crate::keepalive::Busy>,
    idle: Duration,
) {
    loop {
        busy.wait_idle().await;
        tokio::select! {
            () = tokio::time::sleep(idle) => {
                let Ok(slot) = inner.slot(i) else {
                    return;
                };
                let mut slot = slot.lock().await;
                if slot.gen != gen {
                    return;
                }
                if busy.count() != 0 {
                    continue;
                }
                slot.send = None;
                slot.busy = None;
                if let Some(stop) = slot.stop.take() {
                    stop.send(true).ok();
                }
                slot.gen = slot.gen.wrapping_add(1);
                return;
            }
            () = busy.wait_busy() => {}
        }
    }
}

async fn handshake(
    endpoint: &Endpoint,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
    let timeout = config.handshake_timeout();
    match tokio::time::timeout(timeout, handshake_io(endpoint, config, tls)).await {
        Ok(result) => result,
        Err(_) => Err(Status::unavailable(format!(
            "connect {}: timed out after {timeout:?}",
            endpoint.describe()
        ))),
    }
}

async fn handshake_io(
    endpoint: &Endpoint,
    config: ChannelConfig,
    tls: Option<&ClientTls>,
) -> Result<Dialed, Status> {
    match endpoint {
        Endpoint::Tcp(host) => {
            let tcp = TcpStream::connect(host)
                .await
                .map_err(|e| Status::unavailable(format!("connect {host}: {e}")))?;
            crate::tcp::tune(&tcp, config.tcp_keepalive_period())
                .map_err(|e| Status::unavailable(e.to_string()))?;
            match tls {
                None => finish_h2(config, tcp).await,
                Some(tls) => finish_h2(config, tls.connect(tcp).await?).await,
            }
        }
        #[cfg(unix)]
        Endpoint::Unix(path) => {
            if tls.is_some() {
                return Err(Status::invalid_argument(
                    "TLS over a Unix socket is not supported",
                ));
            }
            let io = UnixStream::connect(path).await.map_err(|e| {
                Status::unavailable(format!("connect {}: {e}", endpoint.describe()))
            })?;
            finish_h2(config, io).await
        }
        Endpoint::Once => Err(Status::unavailable("channel has no address to redial")),
    }
}

async fn finish_h2<IO>(config: ChannelConfig, io: IO) -> Result<Dialed, Status>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (send, mut conn) = config
        .h2_builder()
        .handshake(io)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let (interval, timeout) = config.keepalive();
    let dead = crate::keepalive::spawn(conn.ping_pong(), interval, timeout);
    // `SendRequest::ready` does not wait for SETTINGS. Drive the connection
    // until send capacity leaves 0, which is when the peer's preface has
    // been applied. Dropping this future on connect_timeout drops `conn`.
    std::future::poll_fn(|cx| {
        if send.current_max_send_streams() > 0 {
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut conn).poll(cx) {
            Poll::Ready(result) => {
                drop(result);
                Poll::Ready(Err(Status::unavailable(
                    "http/2 preface: connection closed",
                )))
            }
            Poll::Pending => {
                if send.current_max_send_streams() > 0 {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }
    })
    .await?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let busy = config
        .connection_idle()
        .map(|_| crate::keepalive::Busy::new());
    drop(tokio::spawn(async move {
        tokio::select! {
            r = conn => {
                drop(r);
            }
            _ = crate::keepalive::wait_opt(dead) => {}
            _ = crate::keepalive::wait(stop_rx) => {}
        }
    }));
    Ok(Dialed {
        send,
        stop: stop_tx,
        busy,
    })
}

fn attach_lease<T>(
    response: crate::request::Response<Streaming<T>>,
    lease: Option<crate::keepalive::Lease>,
) -> crate::request::Response<Streaming<T>> {
    match lease {
        Some(lease) => response.map(|stream| stream.bind_lease(lease)),
        None => response,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, cancel, limits, and scheme"
)]
async fn run_unary<Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    compress: bool,
    frame: Bytes,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
) -> Result<Response<Resp>, Status>
where
    Resp: Parse + Default,
{
    let deadline = deadline_from(timeout);
    let (resp_fut, mut send_stream) = open(
        send_req,
        authority,
        path,
        md,
        timeout,
        compress,
        &user_agent,
        https,
    )
    .await?;
    send_bytes(&mut send_stream, frame, true, wire.send_buffer).await?;
    race(
        async {
            let response = resp_fut.await.map_err(Status::from_h2)?;
            finish_unary::<Resp>(response, wire.limits).await
        },
        cancel_rx,
        deadline,
        Some(&mut send_stream),
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, cancel, limits, and buffer"
)]
async fn run_server_stream<Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    compress: bool,
    frame: Bytes,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Resp: Parse + Default + Send + 'static,
{
    let deadline = deadline_from(timeout);
    let (resp_fut, mut send_stream) = open(
        send_req,
        authority,
        path,
        md,
        timeout,
        compress,
        &user_agent,
        https,
    )
    .await?;
    send_bytes(&mut send_stream, frame, true, wire.send_buffer).await?;
    race(
        async {
            let response = resp_fut.await.map_err(Status::from_h2)?;
            finish_stream::<Resp>(response, wire.limits, deadline).await
        },
        cancel_rx,
        deadline,
        Some(&mut send_stream),
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, stream, cancel, and limits"
)]
async fn run_client_stream<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<()>,
    rx: Streaming<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
) -> Result<Response<Resp>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default,
{
    let (_, md, timeout, compress) = req.into_parts();
    let deadline = deadline_from(timeout);
    let (resp_fut, send_stream) = open(
        send_req,
        authority,
        path,
        &md,
        timeout,
        compress,
        &user_agent,
        https,
    )
    .await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
        wire,
    )));
    race(
        async {
            let response = resp_fut.await.map_err(Status::from_h2)?;
            finish_unary::<Resp>(response, wire.limits).await
        },
        cancel_rx,
        deadline,
        None,
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transport handle plus request, stream, cancel, limits, and buffer"
)]
async fn run_bidi<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<()>,
    rx: Streaming<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
    user_agent: HeaderValue,
    https: bool,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default + Send + 'static,
{
    let (_, md, timeout, compress) = req.into_parts();
    let deadline = deadline_from(timeout);
    let (resp_fut, send_stream) = open(
        send_req,
        authority,
        path,
        &md,
        timeout,
        compress,
        &user_agent,
        https,
    )
    .await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
        wire,
    )));
    race(
        async {
            let response = resp_fut.await.map_err(Status::from_h2)?;
            finish_stream::<Resp>(response, wire.limits, deadline).await
        },
        cancel_rx,
        deadline,
        None,
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "one HTTP/2 stream open plus headers, timeout, encoding, and scheme"
)]
async fn open(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> Result<(h2::client::ResponseFuture, h2::SendStream<Bytes>), Status> {
    let mut send_req = send_req.ready().await.map_err(Status::from_h2)?;
    let http_req = grpc_request(authority, path, md, timeout, send_gzip, user_agent, https)?;
    send_req
        .send_request(http_req, false)
        .map_err(Status::from_h2)
}

/// Race the RPC against its deadline and its cancel signal, resetting the
/// stream if either wins so the server stops working on it.
/// Turn a duration into an absolute instant, so every stage of one RPC races
/// the same deadline rather than restarting the clock.
fn deadline_from(timeout: Option<Duration>) -> Option<tokio::time::Instant> {
    timeout.map(|d| tokio::time::Instant::now() + d)
}

/// Report an expired deadline as `DEADLINE_EXCEEDED`, whatever the transport
/// said.
///
/// A server enforcing the same `grpc-timeout` resets the stream at the
/// deadline, and that reset can reach us before our own timer fires. Reporting
/// it as `UNAVAILABLE` or `CANCELLED` would tell the caller the connection
/// failed when in fact their deadline elapsed, so the deadline wins. Real
/// statuses from the peer are left alone.
fn prefer_deadline<T>(
    result: Result<T, Status>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    let Some(at) = deadline else {
        return result;
    };
    match &result {
        Err(status)
            if matches!(status.code(), Code::Unavailable | Code::Cancelled)
                && tokio::time::Instant::now() >= at =>
        {
            Err(Status::deadline_exceeded())
        }
        _ => result,
    }
}

/// Race a setup or RPC future against its deadline and cancel signal.
async fn first_of<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    mut cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    if let Some(at) = deadline {
        tokio::select! {
            biased;
            r = fut => r,
            _ = tokio::time::sleep_until(at) => Err(Status::deadline_exceeded()),
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
        }
    } else {
        tokio::select! {
            biased;
            r = fut => r,
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
        }
    }
}

async fn race<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
    send: Option<&mut h2::SendStream<Bytes>>,
) -> Result<T, Status> {
    let result = first_of(fut, cancel_rx, deadline).await;
    if let Some(send) = send {
        if matches!(
            &result,
            Err(s) if s.code() == Code::Cancelled || s.code() == Code::DeadlineExceeded
        ) {
            send.send_reset(Reason::CANCEL);
        }
    }
    prefer_deadline(result, deadline)
}

#[cfg(test)]
mod tests {
    use super::Target;
    use std::net::SocketAddr;

    #[test]
    fn targets_accept_addresses_and_names() {
        let addr: SocketAddr = "127.0.0.1:50051".parse().expect("addr");
        assert_eq!(Target::from(addr).authority(), "127.0.0.1:50051");
        assert_eq!(Target::from("host:1").authority(), "host:1");
        assert_eq!(Target::from("host:1".to_owned()).authority(), "host:1");
    }

    #[test]
    fn bad_authority_is_unavailable_not_a_panic() {
        let err = Target::from("not a host").parse().expect_err("invalid");
        assert_eq!(err.code(), crate::status::Code::Unavailable);
    }

    #[test]
    fn channel_debug_names_the_authority() {
        let channel = super::Channel::connect_lazy("127.0.0.1:9").expect("lazy");
        let dbg = format!("{channel:?}");
        assert!(dbg.contains("127.0.0.1:9"), "{dbg}");
        assert!(dbg.contains("connections: 1"), "{dbg}");
        assert!(dbg.contains("tls: false"), "{dbg}");
        assert!(dbg.contains("interceptors: 0"), "{dbg}");
    }

    #[test]
    fn stream_buffer_overlays_a_live_channel() {
        let channel = super::Channel::connect_lazy("127.0.0.1:9")
            .expect("lazy")
            .stream_buffer(64);
        assert_eq!(channel.config().stream_buffer_size(), 64);
    }
}
