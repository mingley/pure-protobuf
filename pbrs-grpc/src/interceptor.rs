//! Interceptors: inspect an inbound [`Rpc`] or outbound [`crate::Outgoing`]
//! before the handler or the wire.

use crate::server::{Rpc, Service};
use crate::status::Status;
use std::fmt;
use std::sync::Arc;

/// Inspect an inbound RPC before the handler runs.
///
/// Return `Err` to reject without reading the body; `Ok` to proceed.
/// Closures with this signature implement the trait, so most interceptors
/// are one function. Mutate inbound metadata with [`Rpc::metadata_mut`]
/// (strip with [`crate::Metadata::remove`] or [`crate::Metadata::retain`],
/// overwrite a hop with [`crate::Metadata::set`]), cap the deadline with
/// [`Rpc::set_timeout`], read the client's deadline with [`Rpc::peer_timeout`]
/// or the handler's with [`Rpc::effective_timeout`], read `:authority` with
/// [`Rpc::authority`] and `:scheme` with [`Rpc::scheme`], read the mTLS
/// client certificate with [`Rpc::peer_identity`], or insert typed
/// values with [`Rpc::extensions_mut`] for the handler to read from
/// [`crate::Request::extensions`]. `Err` may
/// carry [`crate::Status::with_error_details`]; those trailers reach the client.
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
/// `svc.intercept(a).intercept(b)` runs `a` then `b`. On a [`crate::Router`],
/// call [`crate::Router::intercept`] or wrap one service with [`Intercepted`].
pub trait Interceptor: Send + Sync + 'static {
    /// Inspect `rpc`. The body has not been read yet.
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

/// A [`Service`] with an [`Interceptor`] in front of it.
///
/// `NAME` is inherited, so the wrapper mounts wherever the inner service
/// would. Build one with [`ServiceExt::intercept`] or
/// [`crate::Server::intercept`]. Calling [`Intercepted::intercept`] stacks
/// another interceptor after this one (first registered runs first).
pub struct Intercepted<S, I> {
    inner: Arc<S>,
    interceptor: I,
}

impl<S, I> Intercepted<S, I> {
    /// Wrap `inner` with `interceptor`.
    #[must_use]
    pub fn new(inner: S, interceptor: I) -> Self {
        Self {
            inner: Arc::new(inner),
            interceptor,
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
    /// onion-style (which would run `b` first).
    #[must_use]
    pub fn intercept<J: Interceptor>(self, next: J) -> Intercepted<S, impl Interceptor> {
        Intercepted {
            inner: self.inner,
            interceptor: Then::new(Arc::new(self.interceptor), next),
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
        self.inner.call(rpc).await;
    }
}

/// Extra methods on every [`Service`].
pub trait ServiceExt: Service + Sized {
    /// Run `interceptor` before this service sees the RPC.
    ///
    /// Calling this on an [`Intercepted`] uses [`Intercepted::intercept`]
    /// instead, which stacks first-interceptor-first.
    #[must_use]
    fn intercept<I: Interceptor>(self, interceptor: I) -> Intercepted<Self, I> {
        Intercepted::new(self, interceptor)
    }
}

impl<S: Service> ServiceExt for S {}

/// Outbound call hook. Closures with this signature implement it.
///
/// Attach one with [`crate::Channel::intercept`] or the generated
/// `FooClient::intercept`. Calling either twice stacks; the first interceptor
/// runs first. The interceptor sees the method path, `:authority`,
/// `:scheme`, and `user-agent`, and can set a deadline, wait-for-ready,
/// compression, or typed extensions — not only metadata.
///
/// Typed context the caller put on [`crate::Request::extensions_mut`] is
/// visible here, so an interceptor can stamp metadata from a trace id or
/// tenant without the call site knowing the header names. An earlier
/// interceptor can insert values for a later one the same way.
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
///     Ok(())
/// }
/// # let _ = stamp;
/// ```
pub trait ClientInterceptor: Send + Sync + 'static {
    /// Inspect and mutate the outbound call. Called once per RPC, before the
    /// stream opens.
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
