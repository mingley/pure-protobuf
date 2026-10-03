//! Run tonic-generated services on the native server transport.
//!
//! With the opt-in `tonic` feature, wrap an unchanged generated server in
//! [`TonicServer`] or call [`TonicServerExt::into_pbrs_service`], then pass it
//! to [`crate::Server::new`] or [`crate::Router::add_service`]. Apply ordinary
//! Tower layers with [`TonicServer::layer`]; the original generated routing
//! name is retained. Native and tonic services can share one router.
//!
//! The native kernel retains TLS, connection information, request validation,
//! inbound interceptors, HTTP/2 settings, concurrency, keepalive, shutdown and
//! deadlines. Post-interceptor metadata and kernel-stamped extensions reach
//! the tonic service. Readiness, handlers and response streaming all count
//! against the same deadline; cancellation drops pending service/body work.
//! The built-in TCP/TLS/Unix accept loops stamp tonic's exact connect-info
//! types alongside native information. Custom [`crate::Incoming`] acceptors
//! can retain the real stream's information with
//! [`crate::ConnectionInfo::with_tonic_tls`] / `with_tonic_uds` (Unix). Native
//! TLS/Unix facts alone cannot reconstruct tonic's private TLS/credential
//! values. [`crate::Server::serve_connection`] deliberately supplies no peer
//! facts; use `serve_with_incoming` for a custom acceptor that has them.
//! Default `Bytes` DATA and terminal trailers are forwarded without payload
//! copies, protobuf decoding, a second HTTP/2 connection or an unbounded queue.
//!
//! Tonic owns opaque message framing, codecs, its default 4 MiB decoding cap,
//! outbound message limits and opt-in compression. Configure those on the
//! generated tonic server. Explicit native message-cap overrides additionally
//! restrict identity-coded messages without protobuf decoding or DATA copies.
//! Each direction retains the default forwarding path unless its cap changes;
//! a native cap cannot enlarge tonic's own configured cap. Configured inbound
//! finite decoding caps also validate gzip requests with the native encoded and
//! inflated bounds. Gzip messages are withheld until validated, then their
//! original DATA views are forwarded; tonic inflates again when decoding.
//! The guard bounds logical retained bytes and descriptor count, while Bytes
//! backing allocations and tonic's private decoded buffers remain opaque.
//! Other encodings and unlimited changed decoding caps reject compressed input
//! before readiness; configured outbound caps
//! reject nonidentity response encoding after the handler, before headers.
//! Disabling native input compression admits identity requests and advertises
//! identity acceptance without changing tonic's negotiated response codec.
//! Nonidentity request headers are rejected before readiness; a compressed flag
//! without such a header is rejected only when its prefix is read, which can
//! happen after readiness and handler entry for streaming methods. Native
//! identity aliases are canonicalized only for this input opt-out. An unchanged
//! native decoding cap does not constrain tonic's separately configured decoder.
//! Changed native outbound compression settings with a finite outbound cap use
//! a cold transform: tonic sees no response-encoding advertisement and emits
//! identity frames, then native gzip/deflate applies the original peer's coding
//! negotiation and configured level. The transform checks uncompressed lengths
//! before forwarding, completes one message at a time, and accounts its own
//! vector capacities even on an unlimited tracker. Output permits follow Bytes
//! clones through queued writes. Coalesced DATA backing and private codec state
//! remain outside that accounting. Tonic's private encode cap can be stricter
//! after identity serialization; it is never enlarged. An unexpected compressed
//! response header or a tonic compression-override extension rejects before
//! headers; a compressed flag rejects when its prefix is read. Compression of
//! one capped message is synchronous and cannot be preempted mid-call.
//! Unlimited changed outbound compression policies, a selected unsupported
//! codec, finite byte budgets, response hooks, binary logging, lifecycle observers
//! and grpc-web cannot currently apply to these bodies: they fail with
//! `FAILED_PRECONDITION` before Tower readiness or business dispatch. This
//! includes a native send-buffer setting that implicitly enables a finite
//! byte budget. Inbound native authentication interceptors remain supported.
//! Default native message/compression settings are not an additional codec
//! policy over the tonic service; full native opaque-body policy enforcement
//! and performance qualification remain separate work.

use crate::metadata::Metadata;
use crate::server::drain::{CancelOnDrop, hold_cancel, run_handler};
use crate::transport::{FlowControl, RecvStream, SendResponse, SendStream, h2 as backend};
use crate::{Code, Rpc, ServerConfig, Status};
use bytes::Bytes;
use http_body::{Body, Frame};
use std::future::{Future, poll_fn};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::watch;
use tokio::time::Instant;
use tonic::body::Body as TonicBody;
use tonic::server::NamedService;
use tower::Service as TowerService;

mod gzip;
mod opaque;
mod outbound;

/// A tonic-generated named service mounted in the native gRPC kernel.
#[derive(Debug)]
pub struct TonicServer<N, S = N> {
    inner: S,
    name: PhantomData<fn() -> N>,
}

impl<N, S: Clone> Clone for TonicServer<N, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            name: PhantomData,
        }
    }
}

impl<S> TonicServer<S> {
    /// Wrap a tonic service, including a named interceptor or Tower wrapper.
    #[must_use]
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            name: PhantomData,
        }
    }
}

impl<N, S> TonicServer<N, S> {
    /// Apply an ordinary Tower layer, retaining the original routing name.
    ///
    /// The layer's service need not implement [`NamedService`]. Readiness,
    /// handler errors and response bodies still run under native cancellation
    /// and deadline enforcement.
    #[must_use]
    pub fn layer<L: tower::Layer<S>>(self, layer: L) -> TonicServer<N, L::Service> {
        TonicServer {
            inner: layer.layer(self.inner),
            name: PhantomData,
        }
    }

    /// Recover the wrapped service.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }
}

/// Mount a tonic-generated server in [`crate::Server`] or [`crate::Router`].
pub trait TonicServerExt: NamedService + Sized {
    /// Forward this service's encoded HTTP bodies through the native kernel.
    fn into_pbrs_service(self) -> TonicServer<Self> {
        TonicServer::new(self)
    }
}

impl<S: NamedService> TonicServerExt for S {}

impl<N, S, B> crate::Service for TonicServer<N, S>
where
    N: NamedService + 'static,
    S: TowerService<http::Request<TonicBody>, Response = http::Response<B>>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send,
    S::Error: Into<tonic::codegen::StdError>,
    B: Body<Data = Bytes> + Send,
    B::Error: Into<tonic::codegen::StdError>,
{
    const NAME: &'static str = N::NAME;

    async fn call(&self, rpc: Rpc) {
        if rpc.limits() != ServerConfig::default().limits()
            || !rpc.config.accepts_compressed()
            || outbound::requested(rpc.config)
        {
            // Keep the framing state and cap-body boxes out of the default
            // path. Explicit caps or compression policies use the cold future.
            Box::pin(call_configured(self, rpc)).await;
            return;
        }
        if let Some(policy) = unsupported_policy(&rpc) {
            rpc.reject(Status::failed_precondition(format!(
                "tonic server transport cannot apply native {policy}; configure opaque-body policies on the tonic service"
            )));
            return;
        }
        let deadline = rpc.deadline();
        let mut accounting = Accounting::new(rpc.channelz_server, rpc.channelz_socket);
        let Rpc {
            request,
            mut respond,
            config,
            metadata,
            extensions,
            ..
        } = rpc;
        let (mut parts, recv) = request.into_parts();
        // Remove original user headers first. Otherwise an interceptor's
        // removal or replacement (especially authorization) could be undone.
        let original = Metadata::from_headers(&parts.headers);
        for name in original.keys() {
            parts.headers.remove(name);
        }
        if let Err(status) = metadata.write_to(&mut parts.headers) {
            crate::wire::send_trailers_only(&mut respond, status, &Metadata::new());
            return;
        }
        parts.extensions.extend(extensions);
        let (cancel, cancelled) = watch::channel(false);
        let request =
            http::Request::from_parts(parts, TonicBody::new(IncomingBody::new(recv, cancelled)));
        let mut service = self.inner.clone();
        hold_cancel(CancelOnDrop(cancel.clone()), async {
            let handler = async {
                poll_fn(|cx| service.poll_ready(cx))
                    .await
                    .map_err(service_error)?;
                service.call(request).await.map_err(service_error)
            };
            let outcome = tokio::select! {
                biased;
                () = wait_deadline(deadline) => {
                    cancel.send(true).ok();
                    Err(Status::deadline_exceeded())
                }
                result = run_handler(&mut respond, cancel.clone(), handler, None) => result,
            };
            let response = match outcome {
                Ok(response) => response,
                Err(status) => {
                    crate::wire::send_trailers_only(&mut respond, status, &Metadata::new());
                    return;
                }
            };
            let (parts, body) = response.into_parts();
            let end = body.is_end_stream();
            let header_ok = end && successful_status(&parts.headers);
            let Ok(mut send) = respond.send_response(http::Response::from_parts(parts, ()), end)
            else {
                return;
            };
            if end {
                accounting.ok = header_ok;
                return;
            }
            let result = tokio::select! {
                biased;
                () = wait_deadline(deadline) => {
                    cancel.send(true).ok();
                    Err(Status::deadline_exceeded())
                }
                result = drain_response(&mut send, body, config.send_buffer_size()) => result,
            };
            match result {
                Ok(ok) => accounting.ok = ok,
                Err(status) => {
                    cancel.send(true).ok();
                    if let Ok(trailers) = crate::wire::grpc_trailers(&status) {
                        send.send_trailers(trailers).ok();
                    }
                }
            }
        })
        .await;
    }
}

fn unsupported_policy(rpc: &Rpc) -> Option<&'static str> {
    if outbound::requested(rpc.config) && !outbound::supported(rpc) {
        return Some("compression settings");
    }
    if rpc.byte_budget.limit().is_some() {
        return Some("byte budget");
    }
    if rpc.response_interceptor.is_some() {
        return Some("response hooks");
    }
    if rpc.binlog.is_some() {
        return Some("binary logging");
    }
    if rpc.observer.is_some() {
        return Some("lifecycle observer");
    }
    #[cfg(feature = "grpc-web")]
    if rpc.web.is_some() {
        return Some("grpc-web");
    }
    None
}

async fn call_configured<N, S, B>(server: &TonicServer<N, S>, rpc: Rpc)
where
    N: NamedService + 'static,
    S: TowerService<http::Request<TonicBody>, Response = http::Response<B>>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send,
    S::Error: Into<tonic::codegen::StdError>,
    B: Body<Data = Bytes> + Send,
    B::Error: Into<tonic::codegen::StdError>,
{
    if let Some(policy) = unsupported_policy(&rpc) {
        rpc.reject(Status::failed_precondition(format!(
            "tonic server transport cannot apply native {policy}; configure opaque-body policies on the tonic service"
        )));
        return;
    }
    let compression_disabled = !rpc.config.accepts_compressed();
    let limits = rpc.limits();
    let defaults = ServerConfig::default().limits();
    let cap_input = limits.max_decoding() != defaults.max_decoding();
    let cap_output = limits.max_encoding() != defaults.max_encoding();
    // Capture real peer negotiation before forcing tonic identity encoding.
    let outbound = outbound::requested(rpc.config).then(|| outbound::Settings::new(&rpc));
    let input_gzip = if cap_input {
        match (
            crate::wire::grpc_encoding(rpc.request.headers()),
            limits.max_decoding(),
        ) {
            (Some("gzip"), Some(max)) => Some(max),
            (Some(_), _) => {
                rpc.reject(opaque::compressed_policy());
                return;
            }
            (None, _) => None,
        }
    } else {
        None
    };
    let deadline = rpc.deadline();
    let mut accounting = Accounting::new(rpc.channelz_server, rpc.channelz_socket);
    let Rpc {
        request,
        mut respond,
        config,
        metadata,
        extensions,
        ..
    } = rpc;
    let (mut parts, recv) = request.into_parts();
    let original = Metadata::from_headers(&parts.headers);
    for name in original.keys() {
        parts.headers.remove(name);
    }
    if let Err(status) = metadata.write_to(&mut parts.headers) {
        send_configured_error(&mut respond, status, compression_disabled);
        return;
    }
    if compression_disabled && parts.headers.contains_key("grpc-encoding") {
        // Native validation already admitted exactly its identity aliases.
        // Tonic's decoder requires the canonical token; defaults stay opaque.
        parts
            .headers
            .insert("grpc-encoding", http::HeaderValue::from_static("identity"));
    }
    if outbound.is_some() {
        parts.headers.remove("grpc-accept-encoding");
    }
    parts.extensions.extend(extensions);
    let (cancel, cancelled) = watch::channel(false);
    let incoming = IncomingBody::new(recv, cancelled);
    let body = if let Some(max) = input_gzip {
        TonicBody::new(gzip::GzipBody::new(incoming, limits, max))
    } else if compression_disabled {
        // Input opt-out is not an implicit native default-size override on
        // tonic's independently configured private decoder.
        let guard_limits = if cap_input {
            limits
        } else {
            limits.with_unlimited_decoding()
        };
        TonicBody::new(opaque::CappedBody::new(
            incoming,
            guard_limits,
            opaque::Direction::DecodeCompressionDisabled,
        ))
    } else if cap_input {
        TonicBody::new(opaque::CappedBody::new(
            incoming,
            limits,
            opaque::Direction::Decode,
        ))
    } else {
        TonicBody::new(incoming)
    };
    let request = http::Request::from_parts(parts, body);
    let mut service = server.inner.clone();
    hold_cancel(CancelOnDrop(cancel.clone()), async {
        let handler = async {
            poll_fn(|cx| service.poll_ready(cx))
                .await
                .map_err(service_error)?;
            service.call(request).await.map_err(service_error)
        };
        let outcome = tokio::select! {
            biased;
            () = wait_deadline(deadline) => {
                cancel.send(true).ok();
                Err(Status::deadline_exceeded())
            }
            result = run_handler(&mut respond, cancel.clone(), handler, None) => result,
        };
        let response = match outcome {
            Ok(response) => response,
            Err(status) => {
                send_configured_error(&mut respond, status, compression_disabled);
                return;
            }
        };
        // Unlike request encoding, the response encoding is only observable
        // after business dispatch. Reject before emitting response headers.
        if cap_output && crate::wire::grpc_encoding(response.headers()).is_some() {
            send_configured_error(
                &mut respond,
                opaque::compressed_policy(),
                compression_disabled,
            );
            return;
        }
        // The public extension survives tonic's HTTP conversion, but this
        // generic service does not expose the RPC shape on which it applies.
        if outbound.is_some()
            && response.extensions().get::<tonic::codec::SingleMessageCompressionOverride>()
                == Some(&tonic::codec::SingleMessageCompressionOverride::Disable)
        {
            send_configured_error(
                &mut respond,
                Status::failed_precondition("tonic server transport cannot apply native outbound compression to an opaque response compression override"),
                compression_disabled,
            );
            return;
        }
        let (mut parts, body) = response.into_parts();
        if compression_disabled {
            opaque::identity_acceptance(&mut parts.headers);
        }
        let end = body.is_end_stream();
        if !end {
            if let Some(settings) = &outbound {
                if let Some(codec) = settings.codec {
                    parts.headers.insert("grpc-encoding", http::HeaderValue::from_static(codec.name()));
                } else {
                    parts.headers.remove("grpc-encoding");
                }
            }
        }
        let header_ok = end && successful_status(&parts.headers);
        let Ok(mut send) = respond.send_response(http::Response::from_parts(parts, ()), end) else {
            return;
        };
        if end {
            accounting.ok = header_ok;
            return;
        }
        let writer = async {
            if let Some(settings) = outbound {
                if settings.codec.is_some() {
                    let body = outbound::TransformBody::new(body, settings);
                    if compression_disabled {
                        drain_response(
                            &mut send,
                            opaque::IdentityAcceptance::new(body),
                            config.send_buffer_size(),
                        ).await
                    } else {
                        drain_response(&mut send, body, config.send_buffer_size()).await
                    }
                } else {
                    let body = opaque::CappedBody::new(body, limits, opaque::Direction::Encode);
                    if compression_disabled {
                        drain_response(
                            &mut send,
                            opaque::IdentityAcceptance::new(body),
                            config.send_buffer_size(),
                        ).await
                    } else {
                        drain_response(&mut send, body, config.send_buffer_size()).await
                    }
                }
            } else if compression_disabled {
                let body = opaque::IdentityAcceptance::new(body);
                if cap_output {
                    drain_response(
                        &mut send,
                        opaque::CappedBody::new(body, limits, opaque::Direction::Encode),
                        config.send_buffer_size(),
                    )
                    .await
                } else {
                    drain_response(&mut send, body, config.send_buffer_size()).await
                }
            } else if cap_output {
                drain_response(
                    &mut send,
                    opaque::CappedBody::new(body, limits, opaque::Direction::Encode),
                    config.send_buffer_size(),
                )
                .await
            } else {
                drain_response(&mut send, body, config.send_buffer_size()).await
            }
        };
        let result = tokio::select! {
            biased;
            () = wait_deadline(deadline) => {
                cancel.send(true).ok();
                Err(Status::deadline_exceeded())
            }
            result = writer => result,
        };
        match result {
            Ok(ok) => accounting.ok = ok,
            Err(status) => {
                cancel.send(true).ok();
                if let Ok(mut trailers) = crate::wire::grpc_trailers(&status) {
                    if compression_disabled {
                        opaque::identity_acceptance(&mut trailers);
                    }
                    send.send_trailers(trailers).ok();
                }
            }
        }
    })
    .await;
}

fn send_configured_error(
    respond: &mut backend::SendResponse,
    status: Status,
    compression_disabled: bool,
) {
    if compression_disabled {
        crate::wire::reject(respond, status, false);
    } else {
        crate::wire::send_trailers_only(respond, status, &Metadata::new());
    }
}

fn service_error<E: Into<tonic::codegen::StdError>>(error: E) -> Status {
    native_status(tonic::Status::from_error(error.into()))
}

fn native_status(status: tonic::Status) -> Status {
    let mut native = Status::with_details(
        Code::from_i32(status.code() as i32),
        status.message(),
        Bytes::copy_from_slice(status.details()),
    );
    *native.metadata_mut() = Metadata::from_owned_headers(status.metadata().clone().into_headers());
    native
}

async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

struct IncomingBody {
    recv: Option<backend::RecvStream>,
    data_done: bool,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl IncomingBody {
    fn new(recv: backend::RecvStream, mut cancelled: watch::Receiver<bool>) -> Self {
        Self {
            recv: Some(recv),
            data_done: false,
            cancelled: Box::pin(async move {
                while !*cancelled.borrow_and_update() {
                    if cancelled.changed().await.is_err() {
                        return;
                    }
                }
            }),
        }
    }
}

impl Body for IncomingBody {
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.recv.is_none() {
            return Poll::Ready(None);
        }
        if self.cancelled.as_mut().poll(cx).is_ready() {
            self.recv.take();
            return Poll::Ready(Some(Err(tonic::Status::cancelled("RPC cancelled"))));
        }
        if !self.data_done {
            let Some(recv) = self.recv.as_mut() else {
                return Poll::Ready(None);
            };
            match recv.poll_data(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(data))) => {
                    if let Err(error) = recv.flow_control().release_capacity(data.len()) {
                        self.recv.take();
                        return Poll::Ready(Some(Err(tonic::Status::from_error(Box::new(error)))));
                    }
                    return Poll::Ready(Some(Ok(Frame::data(data))));
                }
                Poll::Ready(Some(Err(error))) => {
                    self.recv.take();
                    return Poll::Ready(Some(Err(tonic::Status::from_error(Box::new(error)))));
                }
                Poll::Ready(None) => self.data_done = true,
            }
        }
        let Some(recv) = self.recv.as_mut() else {
            return Poll::Ready(None);
        };
        let trailers = recv.poll_trailers(cx);
        match trailers {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.recv.take();
                Poll::Ready(match result {
                    Ok(Some(trailers)) => Some(Ok(Frame::trailers(trailers))),
                    Ok(None) => None,
                    Err(error) => Some(Err(tonic::Status::from_error(Box::new(error)))),
                })
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.recv.as_ref().is_none_or(RecvStream::is_end_stream)
    }
}

async fn drain_response<B>(
    send: &mut backend::SendStream,
    body: B,
    buffer: usize,
) -> Result<bool, Status>
where
    B: Body<Data = Bytes>,
    B::Error: Into<tonic::codegen::StdError>,
{
    tokio::pin!(body);
    loop {
        let frame = poll_fn(|cx| {
            if send.poll_reset(cx).is_ready() {
                return Poll::Ready(Err(Status::cancelled()));
            }
            body.as_mut()
                .poll_frame(cx)
                .map(|frame| frame.transpose().map_err(service_error))
        })
        .await?;
        let Some(frame) = frame else {
            send.send_data(Bytes::new(), true)
                .map_err(Status::from_h2_send)?;
            // No gRPC terminal status: don't count a malformed body as success.
            return Ok(false);
        };
        match frame.into_data() {
            Ok(data) => crate::wire::send::send_bytes(send, data, false, buffer).await?,
            Err(frame) => {
                if let Ok(trailers) = frame.into_trailers() {
                    let ok = successful_status(&trailers);
                    send.send_trailers(trailers).map_err(Status::from_h2_send)?;
                    return Ok(ok);
                }
            }
        }
    }
}

fn successful_status(headers: &http::HeaderMap) -> bool {
    headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok()?.parse::<i32>().ok())
        == Some(0)
}

struct Accounting {
    server: Option<crate::channelz::ServerId>,
    socket: Option<crate::channelz::SocketId>,
    ok: bool,
}

impl Accounting {
    fn new(
        server: Option<crate::channelz::ServerId>,
        socket: Option<crate::channelz::SocketId>,
    ) -> Self {
        if server.is_some() || socket.is_some() {
            let registry = crate::channelz::Registry::global();
            if let Some(server) = server {
                registry.note_server_call_started(server);
            }
            if let Some(socket) = socket {
                registry.note_stream_started(socket, false);
            }
        }
        Self {
            server,
            socket,
            ok: false,
        }
    }
}

impl Drop for Accounting {
    fn drop(&mut self) {
        if self.server.is_some() || self.socket.is_some() {
            let registry = crate::channelz::Registry::global();
            if let Some(server) = self.server {
                registry.note_server_call_end(server, self.ok);
            }
            if let Some(socket) = self.socket {
                registry.note_stream_end(socket, self.ok);
            }
        }
    }
}
