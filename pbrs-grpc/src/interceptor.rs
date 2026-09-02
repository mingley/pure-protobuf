//! Interceptors: inspect an inbound [`Rpc`] or outbound [`crate::Outgoing`]
//! before the handler or the wire, or a [`crate::ResponseParts`] after the
//! handler returns `Ok` or after a successful receive.

use crate::server::{Rpc, Service};
use crate::status::Status;
use std::fmt;
use std::sync::Arc;

/// Inspect an inbound RPC before the handler runs.
///
/// Return `Err` to reject without reading the body on every call shape;
/// `Ok` to proceed.
/// Closures with this signature implement the trait, so most interceptors
/// are one function. Mutate inbound metadata with [`Rpc::metadata_mut`]
/// (strip with [`crate::Metadata::remove`] or [`crate::Metadata::retain`],
/// overwrite a hop with [`crate::Metadata::set`] / [`crate::Metadata::set_bin`];
/// those mutations reach the
/// handler on h2c, TLS including mTLS, Unix, and [`crate::Channel::from_io`]),
/// cap the deadline with
/// [`Rpc::set_timeout`], read the client's `grpc-timeout` with [`Rpc::peer_timeout`],
/// the server overlay with [`Rpc::rpc_timeout`],
/// or the effective remaining budget with [`Rpc::effective_timeout`] /
/// [`Rpc::deadline`], read the path with
/// [`Rpc::path`] / [`Rpc::service`] / [`Rpc::method`], read `:authority` with
/// [`Rpc::authority`] and `:scheme` with [`Rpc::scheme`], read the mTLS
/// client certificate with [`Rpc::peer_identity`], Unix credentials with
/// [`Rpc::peer_cred`] (including values [`crate::Incoming::peer`] stamped),
/// message caps with [`Rpc::limits`], `accepts_gzip` / encoding with
/// [`Rpc::accepts_gzip`] / [`Rpc::encoding`] / [`Rpc::compresses_outbound`]
/// / [`Rpc::gzip_level`] / [`Rpc::accepts_compressed`] / [`Rpc::concurrent_rpc_limit`] / [`Rpc::send_buffer_size`] (`encoding` is `None` for identity).
/// Distinct from [`Rpc::compresses_outbound`]: that is on or off; [`Rpc::gzip_level`] is deflate effort.
/// Distinct from [`Rpc::accepts_gzip`]: that is the peer's `grpc-accept-encoding`; [`Rpc::accepts_compressed`] is this overlay.
/// Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits; [`Rpc::concurrent_rpc_limit`] is this overlay.
/// Distinct from HTTP/2 `SETTINGS_MAX_FRAME_SIZE` and stream/connection windows, which are handshake; [`Rpc::send_buffer_size`] is this write-time overlay.
/// Read the TCP interface with
/// [`Rpc::local_addr`] / [`Rpc::remote_addr`], or insert typed values with
/// [`Rpc::extensions_mut`] for the handler to read from
/// [`crate::Request::extensions`] / [`crate::Parts::extensions`] (including
/// over TLS, mTLS, Unix, and [`crate::Channel::from_io`]). Generated handlers see the same path,
/// service, method, client timeout, server timeout overlay, gzip facts, response-gzip overlay, deflate effort, inbound-gzip overlay, process RPC cap, write-time send buffer, peer, and caps on
/// [`crate::Request`]. `Err` may
/// carry [`crate::Status::with_error_details`]; those trailers reach the client.
/// [`crate::Status::from_error_details`] is the typed bag on this Interceptor Err; those trailers reach the client without reading the body.
/// Distinct from a handler Err: that is after the handler ran; this Interceptor Err is trailers without reading the body.
///
/// ```
/// use pbrs_grpc::{Rpc, Service, ServiceExt, Status};
///
/// fn require_token(rpc: &mut Rpc) -> Result<(), Status> {
///     if rpc.metadata().get("authorization") != Some("Bearer secret") {
///         return Err(Status::unauthenticated("bad or missing token"));
///     }
///     rpc.metadata_mut().remove("authorization");
///     rpc.metadata_mut().set("x-actor", "gateway")?;
///     let _ = (
///         rpc.path(),
///         rpc.service(),
///         rpc.method(),
///         rpc.metadata(),
///         rpc.timeout(),
///         rpc.peer_timeout(),
///         rpc.rpc_timeout(),
///         rpc.effective_timeout(),
///         rpc.deadline(),
///         rpc.accepts_gzip(),
///         rpc.encoding(),
///         rpc.compresses_outbound(),
///         rpc.gzip_level(),
///         rpc.accepts_compressed(),
///         rpc.concurrent_rpc_limit(),
///         rpc.send_buffer_size(),
///         rpc.limits(),
///         rpc.local_addr(),
///         rpc.remote_addr(),
///         rpc.peer_identity(),
///         rpc.peer_cred(),
///         rpc.authority(),
///         rpc.scheme(),
///         rpc.extensions(),
///     );
///     Ok(())
/// }
///
/// fn _mount<S: Service>(inner: S) -> pbrs_grpc::Intercepted<S, fn(&mut Rpc) -> Result<(), Status>> {
///     inner.intercept(require_token)
/// }
/// ```
///
/// Generated servers expose the same method, so
/// `GreeterServer::new(svc).intercept(require_token).serve(addr)` is the
/// one-service form; calling `.intercept` twice stacks (first interceptor
/// first). Wrapping a hand-written [`Service`] with [`ServiceExt::intercept`]
/// stacks the same way: [`Intercepted::intercept`] is inherent, so
/// `svc.intercept(a).intercept(b)` runs `a` then `b`. A single interceptor
/// still rejects before the handler on every call shape, including over TLS,
/// mTLS, Unix, and [`crate::Channel::from_io`]. On a [`crate::Router`],
/// call [`crate::Router::intercept`] or wrap one service with [`Intercepted`].
/// Applies to every call shape.
/// Distinct from [`ClientInterceptor`]: that runs on the outbound call before the stream opens; this runs on the inbound RPC before the handler.
/// Distinct from [`ClientInterceptor`]: that runs on the outbound call before the stream opens; this Interceptor runs on the inbound RPC before the handler.
/// Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this runs on the inbound RPC before the handler.
/// Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this Interceptor runs on the inbound RPC before the handler.
pub trait Interceptor: Send + Sync + 'static {
    /// Inspect `rpc`. The body has not been read yet.
    /// [`crate::Status::from_error_details`] is the typed bag on this method-level Interceptor Err; those trailers reach the client without reading the body.
    /// Distinct from a handler Err: that is after the handler ran; this method-level Interceptor Err is trailers without reading the body.
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status>;
}

impl<F> Interceptor for F
where
    F: Fn(&mut Rpc) -> Result<(), Status> + Send + Sync + 'static,
{
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status> {
        self(rpc)
    }
}

/// Inspect an outbound or received [`crate::ResponseParts`].
///
/// Closures with this signature implement the trait, so most hooks are one
/// function. Typed values on [`crate::Response::extensions`] are visible
/// here — they are not headers and they are not on the wire. Stamp
/// [`crate::ResponseParts::metadata_mut`] to send a header, or
/// [`crate::ResponseParts::trailers_mut`] for trailing metadata that ships
/// with `grpc-status`. Distinct from [`Interceptor`] / [`ClientInterceptor`],
/// which run before the handler or before the stream opens.
/// Distinct from [`Interceptor`]: that runs on the inbound RPC before the handler; this ResponseInterceptor runs after the handler returns Ok or after a successful receive.
/// Distinct from [`ClientInterceptor`]: that runs on the outbound call before the stream opens; this ResponseInterceptor runs after the handler returns Ok or after a successful receive.
/// [`crate::ResponseParts::path`] is kernel-stamped.
/// Distinct from [`crate::Request::path`]: that is the inbound request.
/// Distinct from [`crate::Outgoing::path`]: that is a client interceptor before send.
/// [`crate::ResponseParts::gzip_level`] is the server encode overlay.
/// Distinct from [`crate::ResponseParts::compress`]: that is on or off.
/// [`crate::ResponseParts::compresses_outbound`] is the server encode overlay.
/// Distinct from [`crate::ResponseParts::compress`]: that is the per-RPC Compressed-Flag.
/// [`crate::ResponseParts::accepts_gzip`] is the peer `grpc-accept-encoding` advertisement.
/// Distinct from [`crate::ResponseParts::encoding`]: that is received `grpc-encoding`.
/// [`crate::ResponseParts::deadline`] is kernel-stamped when writing.
/// Distinct from [`crate::Request::deadline`]: that is the inbound request.
/// Distinct from [`crate::Rpc::deadline`]: that is computed when that getter runs.
/// [`crate::ResponseParts::timeout`] is the duration stamped at dispatch.
/// Distinct from [`crate::ResponseParts::deadline`]: that is the Instant.
/// [`crate::ResponseParts::limits`] is the encode cap when writing.
/// Distinct from [`crate::Request::limits`]: that is the inbound request.
/// Distinct from [`crate::Rpc::limits`]: that is a server interceptor before the handler.
/// [`crate::ResponseParts::peer_timeout`] is the client's `grpc-timeout`.
/// Distinct from [`crate::ResponseParts::timeout`]: that is the effective cap.
/// [`crate::ResponseParts::rpc_timeout`] is the server overlay.
/// Distinct from [`crate::ResponseParts::timeout`]: that is soonest-of-three, not the overlay.
/// Distinct from [`crate::ResponseParts::peer_timeout`]: that is the client's `grpc-timeout`.
/// [`crate::ResponseParts::accepts_compressed`] is the inbound gzip overlay.
/// Distinct from [`crate::ResponseParts::accepts_gzip`]: that is the peer advertisement.
/// [`crate::ResponseParts::send_buffer_size`] is the write-time HTTP/2 send buffer overlay.
/// Distinct from [`crate::ResponseParts::limits`]: that is the encode cap, not this send buffer.
/// [`crate::ResponseParts::compress_is_set`] is occupancy on this ResponseInterceptor path, so a later interceptor can fill compress only when unset.
/// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay on this ResponseInterceptor path.
///
/// On the server, [`crate::Server::on_response`] /
/// [`crate::Router::on_response`] / generated `FooServer::on_response`
/// run this after the handler returns `Ok`, before headers go out.
/// `Err` after the handler already ran; that status is sent trailers-only
/// instead of the response, including [`crate::Status::with_error_details`].
/// A handler `Err` skips this hook. On a stream, headers have not gone
/// out yet, so a rejected envelope never ships DATA. Applies to every
/// call shape, including over TLS, mTLS, Unix, and
/// [`crate::Server::serve_connection`].
///
/// On the client, [`crate::Channel::on_response`] / generated
/// `FooClient::on_response` run this after a successful receive, before
/// the [`crate::Call`] is Ready. `Err` fails that Call (the peer already sent OK),
/// including [`crate::Status::with_error_details`].
/// A non-OK peer status skips this hook. On server-streaming and bidi, this
/// envelope holds initial headers; [`crate::Streaming::trailers`] still come
/// from the wire after end-of-stream. Applies to every call shape, including
/// over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
/// [`crate::Status::from_error_details`] is the typed bag on this ResponseInterceptor Err; a local reject is trailers-only after handler Ok, or fails the Call after a successful receive.
/// Distinct from a handler Err: that is after the handler ran; this ResponseInterceptor Err is trailers-only after handler Ok, or fails the Call after a successful receive.
///
/// Calling either attach point twice stacks (first interceptor first).
///
/// ```
/// use pbrs_grpc::{ResponseParts, Status};
///
/// fn stamp_trace(parts: &mut ResponseParts) -> Result<(), Status> {
///     if let Some(n) = parts.extensions().get::<u8>().copied() {
///         parts.metadata_mut().insert("x-trace", n.to_string())?;
///     }
///     let _ = (
///         parts.path(),
///         parts.service(),
///         parts.method(),
///         parts.metadata(),
///         parts.trailers(),
///         parts.compress(),
///         parts.compress_is_set(),
///         parts.encoding(),
///         parts.gzip_level(),
///         parts.compresses_outbound(),
///         parts.accepts_gzip(),
///         parts.deadline(),
///         parts.timeout(),
///         parts.limits(),
///         parts.peer_timeout(),
///         parts.rpc_timeout(),
///         parts.accepts_compressed(),
///         parts.send_buffer_size(),
///         parts.extensions(),
///     );
///     Ok(())
/// }
/// # let _ = stamp_trace;
/// ```
pub trait ResponseInterceptor: Send + Sync + 'static {
    /// Inspect and mutate the envelope.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy on this method-level on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay on this method-level on_response.
    /// [`crate::Status::from_error_details`] is the typed bag on this method-level on_response Err; a local reject is trailers-only after handler Ok, or fails the Call after a successful receive.
    /// Distinct from a handler Err: that is after the handler ran; this method-level on_response Err is trailers-only after handler Ok, or fails the Call after a successful receive.
    fn intercept(&self, parts: &mut crate::ResponseParts) -> Result<(), Status>;
}

impl<F> ResponseInterceptor for F
where
    F: Fn(&mut crate::ResponseParts) -> Result<(), Status> + Send + Sync + 'static,
{
    fn intercept(&self, parts: &mut crate::ResponseParts) -> Result<(), Status> {
        self(parts)
    }
}

pub(crate) type ResponseHook = Arc<dyn ResponseInterceptor>;

/// Run `hook` on `response` after the handler returned `Ok` or after a
/// successful receive.
pub(crate) fn intercept_response<T>(
    response: crate::Response<T>,
    hook: Option<&dyn ResponseInterceptor>,
) -> Result<crate::Response<T>, Status> {
    match hook {
        None => Ok(response),
        Some(hook) => {
            let (msg, mut parts) = response.into_message_and_parts();
            hook.intercept(&mut parts)?;
            Ok(crate::Response::from_message_and_parts(msg, parts))
        }
    }
}

/// Run every hook in order after a successful receive or handler `Ok`.
pub(crate) fn intercept_response_all<T>(
    mut response: crate::Response<T>,
    hooks: &[ResponseHook],
) -> Result<crate::Response<T>, Status> {
    for hook in hooks {
        response = intercept_response(response, Some(hook.as_ref()))?;
    }
    Ok(response)
}

/// A [`Service`] with an [`Interceptor`] in front of it, and optionally a
/// [`ResponseInterceptor`] after the handler returns `Ok`.
///
/// `NAME` is inherited, so the wrapper mounts wherever the inner service
/// would. Build one with [`ServiceExt::intercept`] / [`ServiceExt::on_response`]
/// or [`crate::Server::intercept`]. Calling [`Intercepted::intercept`] stacks
/// another interceptor after this one (first registered runs first).
/// [`Intercepted::on_response`] is the same stack for the response hook.
/// A per-service response hook does not cover other mounts; Distinct from
/// [`crate::Server::on_response`] / [`crate::Router::on_response`].
/// Cloning is cheap when `I: Clone`: the inner service is shared.
/// Distinct from [`Interceptor`]: that is the inbound hook; this wrapper runs it before the handler.
/// Distinct from [`ClientInterceptor`]: that is the outbound hook; this wrapper runs an inbound [`Interceptor`] before the handler.
/// Distinct from [`ResponseInterceptor`]: that is the after-Ok hook; this wrapper runs before the handler and may hold that hook for after Ok.
pub struct Intercepted<S, I> {
    inner: Arc<S>,
    interceptor: I,
    response_interceptor: Option<ResponseHook>,
}

impl<S, I> Intercepted<S, I> {
    /// Wrap `inner` with `interceptor`.
    #[must_use]
    pub fn new(inner: S, interceptor: I) -> Self {
        Self {
            inner: Arc::new(inner),
            interceptor,
            response_interceptor: None,
        }
    }

    /// Run `interceptor` after the inner handler returns `Ok`.
    ///
    /// Closures implement [`ResponseInterceptor`]. Calling this twice stacks:
    /// the first interceptor runs first. A [`crate::Server::on_response`] /
    /// [`crate::Router::on_response`] hook still runs first, then this one.
    /// This hook does not cover other mounts.
    /// Same kernel-stamped [`crate::ResponseParts`] overlays as [`crate::Server::on_response`]:
    /// `path` / `gzip_level` / `compresses_outbound` / `accepts_gzip` / `deadline` / `timeout` /
    /// `limits` / `peer_timeout` / `rpc_timeout` / `accepts_compressed` / `send_buffer_size`.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy after this Intercepted on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay after this Intercepted on_response.
    /// [`crate::Status::from_error_details`] is the typed bag after this Intercepted on_response Err; a local reject is trailers-only after handler Ok.
    /// Distinct from a handler Err: that is after the handler ran; this Intercepted on_response Err is trailers-only after handler Ok.
    /// Distinct from [`Self::intercept`]: that runs on the inbound RPC before the handler; this Intercepted on_response runs after the handler returns Ok.
    /// Distinct from [`crate::Server::on_response`]: that runs after the handler returns Ok on the Server's Service; this Intercepted on_response runs after the handler returns Ok on one wrapped service.
    /// Distinct from [`crate::Router::on_response`]: that runs after the handler returns Ok on every mounted service on that Router; this Intercepted on_response runs after the handler returns Ok on one wrapped service.
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// Distinct from [`crate::Request::path`]: that is the inbound request.
    /// `Err` after the handler already ran; that status is sent trailers-only instead of the response,
    /// including [`crate::Status::with_error_details`]. A handler `Err` skips
    /// this hook. Applies to every call shape, including over TLS, mTLS, Unix,
    /// and [`crate::Server::serve_connection`].
    ///
    /// ```
    /// # fn demo<S, I>(wrapped: pbrs_grpc::Intercepted<S, I>) -> pbrs_grpc::Intercepted<S, I> {
    /// wrapped.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = (
    ///         parts.path(),
    ///         parts.service(),
    ///         parts.method(),
    ///         parts.metadata(),
    ///         parts.trailers(),
    ///         parts.compress(),
    ///         parts.compress_is_set(),
    ///         parts.encoding(),
    ///         parts.gzip_level(),
    ///         parts.compresses_outbound(),
    ///         parts.accepts_gzip(),
    ///         parts.deadline(),
    ///         parts.timeout(),
    ///         parts.limits(),
    ///         parts.peer_timeout(),
    ///         parts.rpc_timeout(),
    ///         parts.accepts_compressed(),
    ///         parts.send_buffer_size(),
    ///         parts.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn on_response<R: ResponseInterceptor>(mut self, interceptor: R) -> Self {
        self.response_interceptor = Some(match self.response_interceptor {
            None => Arc::new(interceptor),
            Some(prev) => Arc::new(ResponseThen::new(prev, interceptor)),
        });
        self
    }
}

impl<S, I: Clone> Clone for Intercepted<S, I> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            interceptor: self.interceptor.clone(),
            response_interceptor: self.response_interceptor.clone(),
        }
    }
}

impl<S: Send + Sync + 'static, I: Interceptor> Intercepted<S, I> {
    /// Run `next` after this interceptor. The first interceptor runs first,
    /// matching [`crate::Server::intercept`], [`crate::Router::intercept`],
    /// and [`crate::Channel::intercept`].
    ///
    /// This inherent method is what `.intercept()` resolves to on an
    /// [`Intercepted`], so `svc.intercept(a).intercept(b)` does not wrap
    /// onion-style (which would run `b` first). A response hook already
    /// attached with [`Self::on_response`] stays.
    /// Distinct from [`crate::Channel::intercept`]: that runs on the outbound call before the stream opens; this Intercepted intercept stacks another inbound hook before the handler.
    /// Distinct from [`Self::on_response`]: that runs after the handler returns Ok; this Intercepted intercept stacks another inbound hook before the handler.
    /// Distinct from [`crate::Server::intercept`]: that runs on the inbound RPC before the Server's Service; this Intercepted intercept stacks another inbound hook before one wrapped service.
    /// Distinct from [`crate::Router::intercept`]: that runs on the inbound RPC before every mounted service on that Router; this Intercepted intercept stacks another inbound hook before one wrapped service.
    ///
    /// ```
    /// # fn demo<S, I>(wrapped: pbrs_grpc::Intercepted<S, I>) -> pbrs_grpc::Intercepted<S, impl pbrs_grpc::Interceptor>
    /// # where
    /// #     S: Send + Sync + 'static,
    /// #     I: pbrs_grpc::Interceptor,
    /// # {
    /// wrapped.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = (
    ///         rpc.path(),
    ///         rpc.service(),
    ///         rpc.method(),
    ///         rpc.metadata(),
    ///         rpc.timeout(),
    ///         rpc.peer_timeout(),
    ///         rpc.rpc_timeout(),
    ///         rpc.effective_timeout(),
    ///         rpc.deadline(),
    ///         rpc.accepts_gzip(),
    ///         rpc.encoding(),
    ///         rpc.compresses_outbound(),
    ///         rpc.gzip_level(),
    ///         rpc.accepts_compressed(),
    ///         rpc.concurrent_rpc_limit(),
    ///         rpc.send_buffer_size(),
    ///         rpc.limits(),
    ///         rpc.local_addr(),
    ///         rpc.remote_addr(),
    ///         rpc.peer_identity(),
    ///         rpc.peer_cred(),
    ///         rpc.authority(),
    ///         rpc.scheme(),
    ///         rpc.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn intercept<J: Interceptor>(self, next: J) -> Intercepted<S, impl Interceptor> {
        Intercepted {
            inner: self.inner,
            interceptor: Then::new(Arc::new(self.interceptor), next),
            response_interceptor: self.response_interceptor,
        }
    }
}

impl<S: Service, I> fmt::Debug for Intercepted<S, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Intercepted")
            .field("service", &S::NAME)
            .finish_non_exhaustive()
    }
}

impl<S: Service, I: Interceptor> Service for Intercepted<S, I> {
    const NAME: &'static str = S::NAME;

    async fn call(&self, mut rpc: Rpc) {
        if let Err(status) = self.interceptor.intercept(&mut rpc) {
            return rpc.reject(status);
        }
        if let Some(hook) = &self.response_interceptor {
            rpc.push_response_hook(Arc::clone(hook));
        }
        self.inner.call(rpc).await;
    }
}

#[derive(Clone, Copy)]
struct AllowAll;

impl Interceptor for AllowAll {
    fn intercept(&self, _: &mut Rpc) -> Result<(), Status> {
        Ok(())
    }
}

impl ResponseInterceptor for ResponseHook {
    fn intercept(&self, parts: &mut crate::ResponseParts) -> Result<(), Status> {
        (**self).intercept(parts)
    }
}

/// Extra methods on every [`Service`].
pub trait ServiceExt: Service + Sized {
    /// Run `interceptor` before this service sees the RPC.
    ///
    /// Calling this on an [`Intercepted`] uses [`Intercepted::intercept`]
    /// instead, which stacks first-interceptor-first. A single interceptor
    /// still rejects before the handler on every call shape, including over
    /// TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    /// [`crate::Status::from_error_details`] is the typed bag after this ServiceExt intercept Err; those trailers reach the client without reading the body.
    /// Distinct from a handler Err: that is after the handler ran; this ServiceExt intercept Err is trailers without reading the body.
    /// Distinct from [`crate::Channel::intercept`]: that runs on the outbound call before the stream opens; this ServiceExt intercept runs on the inbound RPC before the handler.
    /// Distinct from [`Self::on_response`]: that runs after the handler returns Ok; this ServiceExt intercept runs on the inbound RPC before the handler.
    /// Distinct from [`crate::Server::intercept`]: that runs on the inbound RPC before the Server's Service; this ServiceExt intercept wraps one service with an inbound hook.
    /// Distinct from [`crate::Router::intercept`]: that runs on the inbound RPC before every mounted service on that Router; this ServiceExt intercept wraps one service with an inbound hook.
    ///
    /// ```
    /// use pbrs_grpc::ServiceExt;
    /// # fn demo<S: pbrs_grpc::Service>(svc: S) -> pbrs_grpc::Intercepted<S, impl pbrs_grpc::Interceptor> {
    /// svc.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = (
    ///         rpc.path(),
    ///         rpc.service(),
    ///         rpc.method(),
    ///         rpc.metadata(),
    ///         rpc.timeout(),
    ///         rpc.peer_timeout(),
    ///         rpc.rpc_timeout(),
    ///         rpc.effective_timeout(),
    ///         rpc.deadline(),
    ///         rpc.accepts_gzip(),
    ///         rpc.encoding(),
    ///         rpc.compresses_outbound(),
    ///         rpc.gzip_level(),
    ///         rpc.accepts_compressed(),
    ///         rpc.concurrent_rpc_limit(),
    ///         rpc.send_buffer_size(),
    ///         rpc.limits(),
    ///         rpc.local_addr(),
    ///         rpc.remote_addr(),
    ///         rpc.peer_identity(),
    ///         rpc.peer_cred(),
    ///         rpc.authority(),
    ///         rpc.scheme(),
    ///         rpc.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    fn intercept<I: Interceptor>(self, interceptor: I) -> Intercepted<Self, I> {
        Intercepted::new(self, interceptor)
    }

    /// Run `interceptor` after this service's handler returns `Ok`.
    ///
    /// Calling this on an [`Intercepted`] uses [`Intercepted::on_response`]
    /// instead, which stacks first-interceptor-first. A
    /// [`crate::Server::on_response`] / [`crate::Router::on_response`] hook
    /// still runs first, then this one.
    /// This hook does not cover other mounts.
    /// Same kernel-stamped [`crate::ResponseParts`] overlays as [`crate::Server::on_response`]:
    /// `path` / `gzip_level` / `compresses_outbound` / `accepts_gzip` / `deadline` / `timeout` /
    /// `limits` / `peer_timeout` / `rpc_timeout` / `accepts_compressed` / `send_buffer_size`.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy after this ServiceExt on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay after this ServiceExt on_response.
    /// [`crate::Status::from_error_details`] is the typed bag after this ServiceExt on_response Err; a local reject is trailers-only after handler Ok.
    /// Distinct from a handler Err: that is after the handler ran; this ServiceExt on_response Err is trailers-only after handler Ok.
    /// Distinct from [`Self::intercept`]: that runs on the inbound RPC before the handler; this ServiceExt on_response runs after the handler returns Ok.
    /// Distinct from [`crate::Server::on_response`]: that runs after the handler returns Ok on the Server's Service; this ServiceExt on_response wraps one service with an after-Ok hook.
    /// Distinct from [`crate::Router::on_response`]: that runs after the handler returns Ok on every mounted service on that Router; this ServiceExt on_response wraps one service with an after-Ok hook.
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// Distinct from [`crate::Request::path`]: that is the inbound request.
    /// `Err` after the handler already ran; that status is sent
    /// trailers-only instead of the response, including
    /// [`crate::Status::with_error_details`]. A handler `Err` skips this
    /// hook. Applies to every call shape, including over TLS, mTLS, Unix,
    /// and [`crate::Server::serve_connection`].
    ///
    /// ```
    /// use pbrs_grpc::ServiceExt;
    /// # fn demo<S: pbrs_grpc::Service>(svc: S) -> pbrs_grpc::Intercepted<S, impl pbrs_grpc::Interceptor> {
    /// svc.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = (
    ///         parts.path(),
    ///         parts.service(),
    ///         parts.method(),
    ///         parts.metadata(),
    ///         parts.trailers(),
    ///         parts.compress(),
    ///         parts.compress_is_set(),
    ///         parts.encoding(),
    ///         parts.gzip_level(),
    ///         parts.compresses_outbound(),
    ///         parts.accepts_gzip(),
    ///         parts.deadline(),
    ///         parts.timeout(),
    ///         parts.limits(),
    ///         parts.peer_timeout(),
    ///         parts.rpc_timeout(),
    ///         parts.accepts_compressed(),
    ///         parts.send_buffer_size(),
    ///         parts.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    fn on_response<R: ResponseInterceptor>(
        self,
        interceptor: R,
    ) -> Intercepted<Self, impl Interceptor> {
        Intercepted::new(self, AllowAll).on_response(interceptor)
    }
}

impl<S: Service> ServiceExt for S {}

/// Outbound call hook. Closures with this signature implement it.
///
/// Attach one with [`crate::Channel::intercept`] or the generated
/// `FooClient::intercept`. Calling either twice stacks; the first interceptor
/// runs first. The interceptor sees the method path, service, method,
/// `:authority`, `:scheme`, `user-agent`, and message caps, and can set a
/// timeout / deadline Instant, wait-for-ready, compression, a user-agent
/// prefix ([`crate::Outgoing::set_user_agent`]), or typed
/// extensions — not only metadata. [`crate::Request::set_user_agent`] is the
/// same prefix at the call site; an interceptor
/// [`crate::Outgoing::set_user_agent`] that runs after wins.
/// [`crate::Outgoing::user_agent_is_set`] distinguishes that override from the channel value, so a later interceptor can prefix only when unset.
/// [`crate::Outgoing::wait_for_ready_is_set`] is occupancy on this ClientInterceptor path, so a later interceptor can fill wait-for-ready only when unset.
/// [`crate::Outgoing::compress_is_set`] is occupancy on this ClientInterceptor path, so a later interceptor can fill compress only when unset.
/// Channel overlays (`rpc_timeout`, `waits_for_ready`, `compresses_outbound`, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `stream_buffer_size`, `send_buffer_size`, `limits`) stay visible after `clear_*`
/// opts out of the already-applied default.
/// [`crate::Outgoing::gzip_level`] is deflate effort. Distinct from
/// [`crate::Outgoing::compresses_outbound`] (on or off). An interceptor cannot change it.
/// [`crate::Outgoing::accepts_compressed`] is the inbound gzip overlay
/// (default on).
/// [`crate::Outgoing::limits`] is the channel message-cap overlay.
/// Same overlay as [`crate::Channel::limits`].
/// [`crate::Outgoing::concurrent_rpc_limit`] is the channel RPC cap overlay.
/// Distinct from [`crate::Outgoing::waits_for_ready`]: that waits for a connection; this refuses extras.
/// [`crate::Outgoing::stream_buffer_size`] is the outbound streaming queue overlay.
/// Distinct from [`crate::Outgoing::limits`]: that is message size, not queue depth.
/// [`crate::Outgoing::send_buffer_size`] is the outbound HTTP/2 send buffer overlay.
/// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is queue depth, not this send buffer.
/// [`crate::Outgoing::connected`] is the live-socket snapshot
/// ([`crate::Channel::connected`]), taken when this interceptor runs.
/// Distinct from wait-for-ready: a lazy first RPC sees `false` even when
/// that overlay is on.
///
/// Typed context the caller put on [`crate::Request::extensions_mut`] is
/// visible here, so an interceptor can stamp metadata from a trace id or
/// tenant without the call site knowing the header names. An earlier
/// interceptor can insert values for a later one the same way.
/// `Err` fails the [`crate::Call`] on poll for every call shape, including
/// [`crate::Status::with_error_details`]; nothing is sent. A local
/// [`crate::Status::with_error_details`] is [`crate::Status::rpc`] /
/// [`crate::Status::error_details`] on that Call for every call shape.
/// [`crate::Outgoing::set_timeout`] is that Call's deadline on every call
/// shape. [`crate::Outgoing::clear_timeout`] opts out of the channel timeout
/// on every call shape. [`crate::Outgoing::clear_compress`] then
/// [`crate::Outgoing::set_compress`] from [`crate::Outgoing::compresses_outbound`]
/// reapplies channel gzip on every call shape. [`crate::Outgoing::set_compress`]
/// stamps [`crate::StreamSender::compress`] on client-streaming and bidi
/// request streams. Outgoing getters apply to
/// every call shape.
/// [`crate::Outgoing::clear_user_agent`] drops an interceptor prefix so this RPC uses the channel user-agent again.
/// [`crate::Outgoing::clear_wait_for_ready`] drops an interceptor wait-for-ready choice so this RPC uses the channel overlay again.
/// [`crate::Outgoing::clear_timeout`] opts out of the channel timeout on this ClientInterceptor path.
/// [`crate::Outgoing::clear_compress`] then [`crate::Outgoing::set_compress`] from [`crate::Outgoing::compresses_outbound`] reapplies channel gzip on this ClientInterceptor path.
/// [`crate::Status::from_error_details`] is the typed bag on this ClientInterceptor Err; a local reject never opens a stream.
/// Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this ClientInterceptor already ran, so a local Err never consumes that budget.
/// Distinct from [`Interceptor`]: that runs on the inbound RPC before the handler; this runs on the outbound call before the stream opens.
/// Distinct from [`Interceptor`]: that runs on the inbound RPC before the handler; this ClientInterceptor runs on the outbound call before the stream opens.
/// Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this runs on the outbound call before the stream opens.
/// Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this ClientInterceptor runs on the outbound call before the stream opens.
///
/// ```
/// use pbrs_grpc::{Outgoing, Status};
/// use std::time::Duration;
///
/// #[derive(Clone, Copy)]
/// struct Tenant(&'static str);
///
/// fn stamp(call: &mut Outgoing<'_>) -> Result<(), Status> {
///     let path = call.path();
///     call.metadata_mut().insert("x-rpc", path)?;
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
///     if let Some(tenant) = call.extensions().get::<Tenant>().copied() {
///         call.metadata_mut().insert("x-tenant", tenant.0)?;
///     }
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
pub trait ClientInterceptor: Send + Sync + 'static {
    /// Inspect and mutate the outbound call. Called once per RPC when the
    /// call is created, before the stream opens. `Err` fails the
    /// [`crate::Call`] on poll, including [`crate::Status::with_error_details`];
    /// nothing is sent. A local [`crate::Status::with_error_details`] is
    /// [`crate::Status::rpc`] / [`crate::Status::error_details`] on that Call
    /// for every call shape. [`crate::Outgoing::set_timeout`] is that Call's
    /// deadline on every call shape. [`crate::Outgoing::clear_timeout`] opts
    /// out of the channel timeout on every call shape.
    /// [`crate::Outgoing::clear_compress`] then [`crate::Outgoing::set_compress`] from [`crate::Outgoing::compresses_outbound`] reapplies channel gzip on this method-level intercept.
    /// [`crate::Outgoing::clear_user_agent`] drops a method-level interceptor prefix so this RPC uses the channel user-agent again.
    /// [`crate::Outgoing::clear_wait_for_ready`] drops a method-level interceptor wait-for-ready choice so this RPC uses the channel overlay again.
    /// [`crate::Outgoing::clear_timeout`] opts out of the channel timeout on this method-level intercept.
    /// [`crate::Outgoing::user_agent_is_set`] is occupancy on this method-level intercept, so a later interceptor can prefix only when unset.
    /// [`crate::Outgoing::wait_for_ready_is_set`] is occupancy on this method-level intercept, so a later interceptor can fill wait-for-ready only when unset.
    /// [`crate::Outgoing::compress_is_set`] is occupancy on this method-level intercept, so a later interceptor can fill compress only when unset.
    /// [`crate::Status::from_error_details`] is the typed bag on this method-level intercept Err; a local reject never opens a stream.
    fn intercept(&self, call: &mut crate::Outgoing<'_>) -> Result<(), Status>;
}

impl<F> ClientInterceptor for F
where
    F: Fn(&mut crate::Outgoing<'_>) -> Result<(), Status> + Send + Sync + 'static,
{
    fn intercept(&self, call: &mut crate::Outgoing<'_>) -> Result<(), Status> {
        self(call)
    }
}

pub(crate) type ClientHook = Arc<dyn ClientInterceptor>;

/// Run `prev` then `next`. Used by [`crate::Server::intercept`],
/// [`crate::Router::intercept`], and [`Intercepted::intercept`] so calling
/// them twice stacks instead of replacing.
pub(crate) struct Then<I> {
    prev: Arc<dyn Interceptor>,
    next: I,
}

impl<I> Then<I> {
    /// Stack `next` after `prev`.
    pub(crate) fn new(prev: Arc<dyn Interceptor>, next: I) -> Self {
        Self { prev, next }
    }
}

impl<I: Interceptor> Interceptor for Then<I> {
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status> {
        self.prev.intercept(rpc)?;
        self.next.intercept(rpc)
    }
}

/// Run `prev` then `next`. Used by [`crate::Server::on_response`] and
/// [`crate::Router::on_response`] so calling them twice stacks instead of
/// replacing.
pub(crate) struct ResponseThen<I> {
    prev: Arc<dyn ResponseInterceptor>,
    next: I,
}

impl<I> ResponseThen<I> {
    /// Stack `next` after `prev`.
    pub(crate) fn new(prev: Arc<dyn ResponseInterceptor>, next: I) -> Self {
        Self { prev, next }
    }
}

impl<I: ResponseInterceptor> ResponseInterceptor for ResponseThen<I> {
    fn intercept(&self, parts: &mut crate::ResponseParts) -> Result<(), Status> {
        self.prev.intercept(parts)?;
        self.next.intercept(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceExt;
    use crate::server::{Rpc, Service};

    struct Dummy;

    impl Service for Dummy {
        const NAME: &'static str = "dummy.Dummy";

        async fn call(&self, rpc: Rpc) {
            rpc.unimplemented();
        }
    }

    #[test]
    fn intercepted_clones_when_the_interceptor_does() {
        fn allow(_rpc: &mut Rpc) -> Result<(), crate::Status> {
            Ok(())
        }
        let a = Dummy.intercept(allow);
        let b = a.clone();
        assert!(format!("{a:?}").contains("dummy.Dummy"));
        assert!(format!("{b:?}").contains("dummy.Dummy"));
    }

    #[test]
    fn response_interceptors_stack_first_registered_first() {
        fn first(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            parts.metadata_mut().insert("x-stack", "a")?;
            Ok(())
        }
        fn second(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            let prev = parts.metadata().get("x-stack").unwrap_or("").to_owned();
            parts.metadata_mut().set("x-stack", format!("{prev}b"))?;
            Ok(())
        }
        let stacked = super::ResponseThen::new(std::sync::Arc::new(first), second);
        let resp =
            super::intercept_response(crate::Response::new(1u32), Some(&stacked)).expect("stack");
        assert_eq!(resp.metadata().get("x-stack"), Some("ab"));
    }

    #[test]
    fn response_interceptor_stamps_metadata_from_extensions() {
        fn stamp(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            if let Some(n) = parts.extensions().get::<u8>().copied() {
                parts.metadata_mut().insert("x-from-ext", n.to_string())?;
            }
            Ok(())
        }
        let mut resp = crate::Response::new(1u32);
        resp.extensions_mut().insert(7u8);
        let resp = super::intercept_response(resp, Some(&stamp)).expect("stamp");
        assert_eq!(resp.metadata().get("x-from-ext"), Some("7"));
        assert_eq!(resp.extensions().get::<u8>().copied(), Some(7));
    }

    #[test]
    fn response_interceptor_none_is_identity() {
        let resp = super::intercept_response(crate::Response::new(1u32), None).expect("none");
        assert!(resp.metadata().is_empty());
    }

    #[test]
    fn intercept_response_all_runs_hooks_in_order() {
        fn first(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            parts.metadata_mut().insert("x-stack", "a")?;
            Ok(())
        }
        fn second(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            let prev = parts.metadata().get("x-stack").unwrap_or("").to_owned();
            parts.metadata_mut().set("x-stack", format!("{prev}b"))?;
            Ok(())
        }
        let hooks: [super::ResponseHook; 2] =
            [std::sync::Arc::new(first), std::sync::Arc::new(second)];
        let resp =
            super::intercept_response_all(crate::Response::new(1u32), &hooks).expect("stack");
        assert_eq!(resp.metadata().get("x-stack"), Some("ab"));
    }

    #[test]
    fn intercept_response_all_empty_is_identity() {
        let resp = super::intercept_response_all(crate::Response::new(1u32), &[]).expect("empty");
        assert!(resp.metadata().is_empty());
        assert!(resp.path().is_none());
        assert!(!resp.compresses_outbound());
        assert!(!resp.accepts_gzip());
        assert!(!resp.accepts_compressed());
        assert!(resp.deadline().is_none());
        assert!(resp.timeout().is_none());
        assert!(resp.peer_timeout().is_none());
        assert!(resp.rpc_timeout().is_none());
        assert!(resp.limits().is_none());
        assert!(resp.send_buffer_size().is_none());
    }

    #[test]
    fn response_interceptor_sees_kernel_path() {
        fn require_path(parts: &mut crate::ResponseParts) -> Result<(), crate::Status> {
            assert_eq!(parts.path(), Some("/helloworld.Greeter/SayHello"));
            assert_eq!(parts.service(), Some("helloworld.Greeter"));
            assert_eq!(parts.method(), Some("SayHello"));
            assert_eq!(parts.gzip_level(), 9);
            assert!(parts.compresses_outbound());
            assert!(parts.accepts_gzip());
            assert!(parts.accepts_compressed());
            assert!(parts.deadline().is_some());
            assert_eq!(parts.timeout(), Some(std::time::Duration::from_secs(5)));
            assert_eq!(
                parts.peer_timeout(),
                Some(std::time::Duration::from_secs(30))
            );
            assert_eq!(parts.rpc_timeout(), Some(std::time::Duration::from_secs(9)));
            assert_eq!(parts.limits(), Some(crate::MessageLimits::default()));
            assert_eq!(
                parts.send_buffer_size(),
                Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)
            );
            Ok(())
        }
        let at = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let resp = super::intercept_response(
            crate::Response::new(1u32)
                .with_path(Some("/helloworld.Greeter/SayHello".into()))
                .with_gzip_level(9)
                .with_compresses_outbound(true)
                .with_accepts_gzip(true)
                .with_accepts_compressed(true)
                .with_deadline(Some(at))
                .with_timeout(Some(std::time::Duration::from_secs(5)))
                .with_peer_timeout(Some(std::time::Duration::from_secs(30)))
                .with_rpc_timeout(Some(std::time::Duration::from_secs(9)))
                .with_limits(Some(crate::MessageLimits::default()))
                .with_send_buffer_size(Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)),
            Some(&require_path),
        )
        .expect("path");
        assert_eq!(resp.path(), Some("/helloworld.Greeter/SayHello"));
        assert_eq!(resp.service(), Some("helloworld.Greeter"));
        assert_eq!(resp.method(), Some("SayHello"));
        assert_eq!(resp.gzip_level(), 9);
        assert!(resp.compresses_outbound());
        assert!(resp.accepts_gzip());
        assert!(resp.accepts_compressed());
        assert_eq!(resp.deadline(), Some(at));
        assert_eq!(resp.timeout(), Some(std::time::Duration::from_secs(5)));
        assert_eq!(
            resp.peer_timeout(),
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(resp.rpc_timeout(), Some(std::time::Duration::from_secs(9)));
        assert_eq!(resp.limits(), Some(crate::MessageLimits::default()));
        assert_eq!(
            resp.send_buffer_size(),
            Some(crate::config::DEFAULT_MAX_SEND_BUFFER_SIZE)
        );
        let shown = format!("{resp:?}");
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
        assert!(crate::Response::new(0u32).path().is_none());
        assert!(crate::Response::new(0u32).service().is_none());
        assert!(crate::Response::new(0u32).method().is_none());
        assert_eq!(
            crate::Response::new(0u32).gzip_level(),
            crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL
        );
        assert!(!crate::Response::new(0u32).compresses_outbound());
        assert!(!crate::Response::new(0u32).accepts_gzip());
        assert!(!crate::Response::new(0u32).accepts_compressed());
        assert!(crate::Response::new(0u32).deadline().is_none());
        assert!(crate::Response::new(0u32).timeout().is_none());
        assert!(crate::Response::new(0u32).peer_timeout().is_none());
        assert!(crate::Response::new(0u32).rpc_timeout().is_none());
        assert!(crate::Response::new(0u32).limits().is_none());
        assert!(crate::Response::new(0u32).send_buffer_size().is_none());
    }
}
