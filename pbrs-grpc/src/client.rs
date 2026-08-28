//! gRPC client: [`Channel`] and the four call shapes.

use crate::config::{ChannelConfig, Wire};
use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::stream::{StreamSender, Streaming};
use crate::wire::{
    encode_msg, finish_stream, finish_unary, grpc_request, pump_outbound, send_bytes,
};
use bytes::Bytes;
use h2::Reason;
use http::uri::Authority;
use pbrs::{Parse, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::watch;

/// Where a [`Channel`] should dial.
///
/// Built with [`From`], so `Channel::connect` takes a `SocketAddr`, a
/// `&str` of the form `host:port`, or a `String`.
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

struct ChannelInner {
    sends: Vec<h2::client::SendRequest<Bytes>>,
    next: AtomicUsize,
    authority: Authority,
}

/// A prior-knowledge HTTP/2 connection (or small pool) to a gRPC server.
///
/// Cloning is cheap and shares the underlying connections, so a `Channel` is
/// meant to be cloned into every task that needs it.
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
/// # let _ = (channel, pooled);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Channel {
    inner: Arc<ChannelInner>,
    config: ChannelConfig,
}

impl Channel {
    /// Dial `target` with default configuration: one connection, 4 MiB
    /// inbound cap.
    pub async fn connect(target: impl Into<Target>) -> Result<Self, Status> {
        Self::connect_with(target, ChannelConfig::default()).await
    }

    /// Dial `target` with `config`.
    ///
    /// Opens [`ChannelConfig::connections`] TCP connections up front; RPCs are
    /// spread over them round-robin. All of them must succeed.
    pub async fn connect_with(
        target: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        let target = target.into();
        let authority = target.parse()?;
        let n = config.connection_count();
        let mut sends = Vec::with_capacity(n);
        for _ in 0..n {
            sends.push(handshake(target.authority(), config).await?);
        }
        Ok(Self {
            inner: Arc::new(ChannelInner {
                sends,
                next: AtomicUsize::new(0),
                authority,
            }),
            config,
        })
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

    /// The `:authority` sent with every request.
    #[must_use]
    pub fn authority(&self) -> &str {
        self.inner.authority.as_str()
    }

    fn grab(&self) -> Result<h2::client::SendRequest<Bytes>, Status> {
        let sends = &self.inner.sends;
        let n = sends.len();
        let i = if n <= 1 {
            0
        } else {
            self.inner.next.fetch_add(1, Ordering::Relaxed) % n
        };
        sends
            .get(i)
            .cloned()
            .ok_or_else(|| Status::unavailable("empty connection pool"))
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
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => return Call::new(cancel, Box::pin(async move { Err(e) })),
        };
        let authority = self.inner.authority.clone();
        let wire = self.config.wire();
        Call::new(
            cancel,
            Box::pin(async move { run_unary(send, &authority, path, req, cancel_rx, wire).await }),
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
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => return Call::new(cancel, Box::pin(async move { Err(e) })),
        };
        let authority = self.inner.authority.clone();
        let wire = self.config.wire();
        Call::new(
            cancel,
            Box::pin(async move {
                run_server_stream(send, &authority, path, req, cancel_rx, wire).await
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
        let (tx, rx) = Streaming::channel(self.config.buffer());
        let tx = tx.with_limits(wire.limits);
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => return (tx, Call::new(cancel, Box::pin(async move { Err(e) }))),
        };
        let authority = self.inner.authority.clone();
        let call = Call::new(
            cancel,
            Box::pin(async move {
                run_client_stream(send, &authority, path, req, rx, cancel_rx, wire).await
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
        let buffer = self.config.buffer();
        let (tx, rx) = Streaming::channel(buffer);
        let tx = tx.with_limits(wire.limits);
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => return (tx, Call::new(cancel, Box::pin(async move { Err(e) }))),
        };
        let authority = self.inner.authority.clone();
        let call = Call::new(
            cancel,
            Box::pin(
                async move { run_bidi(send, &authority, path, req, rx, cancel_rx, wire).await },
            ),
        );
        (tx, call)
    }
}

async fn handshake(
    authority: &str,
    config: ChannelConfig,
) -> Result<h2::client::SendRequest<Bytes>, Status> {
    let tcp = TcpStream::connect(authority)
        .await
        .map_err(|e| Status::unavailable(format!("connect {authority}: {e}")))?;
    tcp.set_nodelay(true)
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let (send, conn) = config
        .h2_builder()
        .handshake(tcp)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        conn.await.ok();
    }));
    Ok(send)
}

async fn run_unary<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
) -> Result<Response<Resp>, Status>
where
    Req: Serialize,
    Resp: Parse + Default,
{
    let (msg, md, timeout, compress) = req.into_parts();
    // Encode before opening the stream so an oversize message never reaches
    // the wire and never occupies a stream slot.
    let frame = encode_msg(&msg, compress, wire.limits)?;
    let deadline = deadline_from(timeout);
    let (resp_fut, mut send_stream) =
        open(send_req, authority, path, &md, timeout, compress).await?;
    send_bytes(&mut send_stream, frame, true, wire.send_buffer).await?;
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
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
async fn run_server_stream<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<Req>,
    cancel_rx: watch::Receiver<bool>,
    wire: Wire,
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: Serialize,
    Resp: Parse + Default + Send + 'static,
{
    let (msg, md, timeout, compress) = req.into_parts();
    let frame = encode_msg(&msg, compress, wire.limits)?;
    // One deadline for the whole RPC: setup, and every read of the response
    // stream that outlives it.
    let deadline = deadline_from(timeout);
    let (resp_fut, mut send_stream) =
        open(send_req, authority, path, &md, timeout, compress).await?;
    send_bytes(&mut send_stream, frame, true, wire.send_buffer).await?;
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
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
) -> Result<Response<Resp>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default,
{
    let (_, md, timeout, compress) = req.into_parts();
    let deadline = deadline_from(timeout);
    let (resp_fut, send_stream) = open(send_req, authority, path, &md, timeout, compress).await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
        wire,
    )));
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
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
) -> Result<Response<Streaming<Resp>>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default + Send + 'static,
{
    let (_, md, timeout, compress) = req.into_parts();
    let deadline = deadline_from(timeout);
    let (resp_fut, send_stream) = open(send_req, authority, path, &md, timeout, compress).await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
        wire,
    )));
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            finish_stream::<Resp>(response, wire.limits, deadline).await
        },
        cancel_rx,
        deadline,
        None,
    )
    .await
}

async fn open(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
) -> Result<(h2::client::ResponseFuture, h2::SendStream<Bytes>), Status> {
    let mut send_req = send_req
        .ready()
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let http_req = grpc_request(authority, path, md, timeout, send_gzip)?;
    send_req
        .send_request(http_req, false)
        .map_err(|e| Status::unavailable(e.to_string()))
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

async fn race<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    mut cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
    send: Option<&mut h2::SendStream<Bytes>>,
) -> Result<T, Status> {
    let result = if let Some(at) = deadline {
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
    };
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
}
