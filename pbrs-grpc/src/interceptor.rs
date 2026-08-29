//! Interceptors: inspect an inbound [`Rpc`] or outbound metadata before the
//! handler or the wire.

use crate::metadata::Metadata;
use crate::server::{Rpc, Service};
use crate::status::Status;
use std::sync::Arc;

/// Inspect an inbound RPC before the handler runs.
///
/// Return `Err` to reject without reading the body; `Ok` to proceed.
/// Closures with this signature implement the trait, so most interceptors
/// are one function.
///
/// ```
/// use pbrs_grpc::{Rpc, Service, ServiceExt, Status};
///
/// fn require_token(rpc: &Rpc) -> Result<(), Status> {
///     if rpc.metadata().get("authorization") != Some("Bearer secret") {
///         return Err(Status::unauthenticated("bad or missing token"));
///     }
///     Ok(())
/// }
///
/// fn _mount<S: Service>(inner: S) -> pbrs_grpc::Intercepted<S, fn(&Rpc) -> Result<(), Status>> {
///     inner.intercept(require_token)
/// }
/// ```
///
/// Generated servers expose the same method, so
/// `GreeterServer::new(svc).intercept(require_token).serve(addr)` is the
/// one-service form. On a [`crate::Router`], call
/// [`crate::Router::intercept`] or wrap one service with [`Intercepted`].
pub trait Interceptor: Send + Sync + 'static {
    /// Inspect `rpc`. The body has not been read yet.
    fn intercept(&self, rpc: &Rpc) -> Result<(), Status>;
}

impl<F> Interceptor for F
where
    F: Fn(&Rpc) -> Result<(), Status> + Send + Sync + 'static,
{
    fn intercept(&self, rpc: &Rpc) -> Result<(), Status> {
        self(rpc)
    }
}

/// A [`Service`] with an [`Interceptor`] in front of it.
///
/// `NAME` is inherited, so the wrapper mounts wherever the inner service
/// would. Build one with [`ServiceExt::intercept`] or
/// [`crate::Server::intercept`].
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

    /// Wrap an existing `Arc` without adding another layer of indirection.
    pub(crate) fn from_arc(inner: Arc<S>, interceptor: I) -> Self {
        Self { inner, interceptor }
    }
}

impl<S: Service, I: Interceptor> Service for Intercepted<S, I> {
    const NAME: &'static str = S::NAME;

    async fn call(&self, rpc: Rpc) {
        if let Err(status) = self.interceptor.intercept(&rpc) {
            return rpc.reject(status);
        }
        self.inner.call(rpc).await;
    }
}

/// Extra methods on every [`Service`].
pub trait ServiceExt: Service + Sized {
    /// Run `interceptor` before this service sees the RPC.
    #[must_use]
    fn intercept<I: Interceptor>(self, interceptor: I) -> Intercepted<Self, I> {
        Intercepted::new(self, interceptor)
    }
}

impl<S: Service> ServiceExt for S {}

/// Outbound metadata hook. Closures with this signature implement it.
///
/// Attach one with [`crate::Channel::intercept`] or the generated
/// `FooClient::intercept`. Calling either twice stacks; the first interceptor
/// runs first.
pub trait ClientInterceptor: Send + Sync + 'static {
    /// Mutate request metadata. Called once per RPC, before the stream opens.
    fn intercept(&self, metadata: &mut Metadata) -> Result<(), Status>;
}

impl<F> ClientInterceptor for F
where
    F: Fn(&mut Metadata) -> Result<(), Status> + Send + Sync + 'static,
{
    fn intercept(&self, metadata: &mut Metadata) -> Result<(), Status> {
        self(metadata)
    }
}

pub(crate) type ClientHook = Arc<dyn ClientInterceptor>;

/// Run `prev` then `next`. Used by [`crate::Router::intercept`] so calling it
/// twice stacks instead of replacing.
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
    fn intercept(&self, rpc: &Rpc) -> Result<(), Status> {
        self.prev.intercept(rpc)?;
        self.next.intercept(rpc)
    }
}
