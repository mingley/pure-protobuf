//! Interceptors: inspect an inbound [`Rpc`] or outbound [`crate::Outgoing`]
//! before the handler or the wire, or a [`crate::ResponseParts`] after the
//! handler returns `Ok` or after a successful receive.

use crate::server::{Rpc, Service};
use crate::status::Status;
use std::fmt;
use std::sync::Arc;

/// Inspect an inbound RPC before the handler reads its body.
///
/// Return `Err` to reject the call with that status, or `Ok` to continue.
/// A closure with this signature implements the trait. The hook can change
/// metadata, shorten the deadline, inspect peer identity and message limits,
/// or attach typed values for the handler through [`Rpc::extensions_mut`].
/// These changes reach the handler on every call shape and transport.
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
/// fn mount<S: Service>(inner: S) -> pbrs_grpc::Intercepted<S, fn(&mut Rpc) -> Result<(), Status>> {
///     inner.intercept(require_token)
/// }
/// ```
///
/// Generated servers, [`crate::Server`], and [`crate::Router`] also expose
/// `.intercept()`. Repeated registrations run in order. Use [`Intercepted`]
/// to apply a hook to one service in a router.
pub trait Interceptor: Send + Sync + 'static {
    /// Inspect `rpc`. The body has not been read yet.
    /// [`crate::Status::from_error_details`] is the typed bag on this method-level Interceptor Err; those trailers reach the client without reading the body.
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

/// Inspect or change a successful response envelope.
///
/// On the server, this runs after the handler returns `Ok`, before any
/// response headers or messages are sent. Returning `Err` replaces the
/// response with a Trailers-Only error. A handler error skips the hook.
/// Register it with [`crate::Server::on_response`],
/// [`crate::Router::on_response`], or a generated server's `.on_response()`.
///
/// On the client, [`crate::Channel::on_response`] runs after receiving a
/// successful response envelope, before its [`crate::Call`] completes.
/// Returning `Err` fails that call locally. A non-OK peer status skips the
/// hook. For server-streaming and bidi calls, the envelope contains initial
/// headers; final status and trailers still arrive through [`crate::Streaming`].
///
/// [`crate::ResponseParts`] exposes metadata, compression settings, deadlines,
/// limits, and typed extensions. Server header and trailer mutations go on
/// the wire; typed extensions stay local. Hooks run in registration order
/// for every RPC shape and transport. Error statuses can carry
/// [`crate::Status::with_error_details`].
///
/// ```
/// use pbrs_grpc::{ResponseParts, Status};
///
/// fn stamp_trace(parts: &mut ResponseParts) -> Result<(), Status> {
///     if let Some(n) = parts.extensions().get::<u8>().copied() {
///         parts.metadata_mut().insert("x-trace", n.to_string())?;
///     }
///     Ok(())
/// }
/// # let _ = stamp_trace;
/// ```
pub trait ResponseInterceptor: Send + Sync + 'static {
    /// Inspect and mutate the envelope.
    /// [`crate::ResponseParts::compress_is_set`] is occupancy on this method-level on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] restores the server gzip overlay on this method-level on_response.
    /// [`crate::Status::from_error_details`] is the typed bag on this method-level on_response Err; a local reject is trailers-only after handler Ok, or fails the Call after a successful receive.
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
/// A per-service response hook applies only to this service. Server and
/// Router hooks apply to all their mounted services.
/// Cloning is cheap when `I: Clone`: the inner service is shared.
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
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// `Err` after the handler already ran; that status is sent trailers-only instead of the response,
    /// including [`crate::Status::with_error_details`]. A handler `Err` skips
    /// this hook. Applies to every call shape, including over TLS, mTLS, Unix,
    /// and [`crate::Server::serve_connection`].
    ///
    /// ```
    /// # fn demo<S, I>(wrapped: pbrs_grpc::Intercepted<S, I>) -> pbrs_grpc::Intercepted<S, I> {
    /// wrapped.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = parts.path();
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
    ///
    /// ```
    /// # fn demo<S, I>(wrapped: pbrs_grpc::Intercepted<S, I>) -> pbrs_grpc::Intercepted<S, impl pbrs_grpc::Interceptor>
    /// # where
    /// #     S: Send + Sync + 'static,
    /// #     I: pbrs_grpc::Interceptor,
    /// # {
    /// wrapped.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = rpc.path();
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
    const ALIASES: &'static [&'static str] = S::ALIASES;

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
    ///
    /// ```
    /// use pbrs_grpc::ServiceExt;
    /// # fn demo<S: pbrs_grpc::Service>(svc: S) -> pbrs_grpc::Intercepted<S, impl pbrs_grpc::Interceptor> {
    /// svc.intercept(|rpc: &mut pbrs_grpc::Rpc| {
    ///     let _ = rpc.path();
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
    /// [`crate::ResponseParts::path`] is kernel-stamped.
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
    ///     let _ = parts.path();
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

/// Inspect or change an outbound RPC before its stream opens.
///
/// Attach with [`crate::Channel::intercept`]. The hook runs once when a call is
/// created, for every call shape. Repeated attachments run in order. Closures
/// with the same signature implement this trait.
///
/// Metadata and typed request extensions are available through
/// [`crate::Outgoing`]. A hook can set a timeout, user-agent prefix, compression,
/// or wait-for-ready override. Use `*_is_set` to preserve caller choices.
/// Configuration getters report channel defaults; `connected` is a snapshot
/// at hook execution, not a readiness guarantee.
///
/// Returning an error fails the call when polled, opens no stream, and consumes
/// no RPC slot. Rich error details remain available through [`Status`].
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
///     let _ = rpc.path();
///     Ok(())
/// }
/// # let _ = stamp;
/// ```
pub trait ClientInterceptor: Send + Sync + 'static {
    /// Run once per RPC before opening a stream. An error fails the call without
    /// sending data. Changes to [`crate::Outgoing`] apply to this call.
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
    fn intercepted_forwards_service_aliases() {
        struct Aliased;
        impl Service for Aliased {
            const NAME: &'static str = "dummy.Name";
            const ALIASES: &'static [&'static str] = &["dummy.Alias"];

            async fn call(&self, rpc: Rpc) {
                rpc.unimplemented();
            }
        }
        type Hook = fn(&mut Rpc) -> Result<(), crate::Status>;
        assert_eq!(
            <super::Intercepted<Aliased, Hook> as Service>::ALIASES,
            &["dummy.Alias"]
        );
        assert_eq!(
            <super::Intercepted<Aliased, Hook> as Service>::NAME,
            "dummy.Name"
        );
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
        assert!(shown.contains("path: Some(\"[REDACTED]\")"), "{shown}");
        assert!(shown.contains("service: Some(\"[REDACTED]\")"), "{shown}");
        assert!(shown.contains("method: Some(\"[REDACTED]\")"), "{shown}");
        assert!(!shown.contains("/helloworld.Greeter/SayHello"), "{shown}");
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
