//! Service dispatch: [`Service`], [`DynService`], [`Dispatch`], [`Single`].

#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::accept::Server;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use super::router::Router;
use super::rpc::Rpc;
use crate::limits::ByteBudgetTracker;
use crate::telemetry::LifecycleObserver;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A gRPC service that can be served.
///
/// `protoc-gen-pbrs` emits one implementation per `service` in your `.proto`,
/// so the usual path is to implement the generated trait (`Greeter`) and let
/// the generated type (`GreeterServer`) implement this.
///
/// Implementing it by hand takes a name and a `match` on
/// [`Rpc::method`]:
///
/// ```
/// use pbrs_grpc::{HelloReply, HelloRequest, Request, Response, Rpc, Service, Status};
///
/// struct Echo;
///
/// impl Service for Echo {
///     const NAME: &'static str = "demo.Echo";
///
///     async fn call(&self, rpc: Rpc) {
///         match rpc.method() {
///             "Ping" => {
///                 rpc.unary(|req: Request<HelloRequest>| async move {
///                     let mut reply = HelloReply::new();
///                     reply.set_message(req.get_ref().name());
///                     Ok::<_, Status>(Response::new(reply))
///                 })
///                 .await;
///             }
///             _ => rpc.unimplemented(),
///         }
///     }
/// }
/// ```
pub trait Service: Send + Sync + 'static {
    /// Fully qualified proto service name, e.g. `helloworld.Greeter`.
    ///
    /// [`Router`] keys on this, and it is the `<service>` half of the
    /// `/<service>/<method>` request path.
    const NAME: &'static str;

    /// Extra `/<service>/` prefixes [`Router`] also mounts this service at.
    ///
    /// Default is empty. Generated `grpc.reflection.v1.ServerReflection`
    /// aliases `grpc.reflection.v1alpha.ServerReflection` so older grpcurl
    /// that falls back to v1alpha hits the same handler. That is a path
    /// alias, not a second proto and not a second `ServerReflectionServer`.
    /// An interceptor on the v1alpha path sees
    /// [`Rpc::service`] `grpc.reflection.v1alpha.ServerReflection` — Distinct
    /// from the v1 name, which is the path the peer sent.
    /// [`Server`] does not look up [`Self::NAME`] or these aliases: a lone
    /// reflection server already answers a v1alpha path. Distinct from
    /// mounting the same handler twice. Distinct from grpc-web, which is a
    /// second protocol, not a path alias.
    /// A wrapping [`Service`] should forward these like [`Self::NAME`].
    const ALIASES: &'static [&'static str] = &[];

    /// Dispatch one RPC.
    ///
    /// Match on [`Rpc::method`] and consume the [`Rpc`] with the call shape
    /// the method declares. Returning without consuming it resets the stream.
    fn call(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
}

/// Object-safe [`Service`], so [`Router`] can hold a heterogeneous map.
pub(crate) trait DynService: Send + Sync + 'static {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

impl<S: Service> DynService for S {
    fn dispatch<'a>(&'a self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(self.call(rpc))
    }
}

/// What the accept loop hands each stream. Monomorphic for [`Server`], boxed
/// for [`Router`].
pub(crate) trait Dispatch: Send + Sync + 'static {
    fn dispatch(&self, rpc: Rpc) -> impl Future<Output = ()> + Send;
    fn observer(&self) -> Option<&Arc<dyn LifecycleObserver>> {
        None
    }
}

/// Newtype so the monomorphic path gets its own [`Dispatch`] impl.
pub(crate) struct Single<S> {
    pub(crate) service: Arc<S>,
    pub(crate) interceptor: Option<Arc<dyn crate::Interceptor>>,
    pub(crate) response_interceptor: Option<crate::interceptor::ResponseHook>,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) byte_budget: ByteBudgetTracker,
    pub(crate) binlog: Option<Arc<crate::binlog::BinaryLogger>>,
}

impl<S: Service> Dispatch for Single<S> {
    async fn dispatch(&self, mut rpc: Rpc) {
        rpc.response_interceptor = self.response_interceptor.clone();
        rpc.byte_budget = self.byte_budget.clone();
        rpc.observer = self.observer.clone();
        if let Some(tap) = self
            .binlog
            .as_ref()
            .and_then(|binlog| binlog.start_call(rpc.path(), crate::binlog::Logger::Server))
        {
            if let Some(peer) = rpc.remote_addr() {
                tap.set_peer(peer);
            }
            tap.log_client_header(
                rpc.metadata(),
                rpc.path(),
                rpc.authority().unwrap_or(""),
                rpc.effective_timeout(),
            );
            rpc.binlog = Some(tap);
        }
        if let Some(interceptor) = &self.interceptor {
            if let Err(status) = interceptor.intercept(&mut rpc) {
                return rpc.reject(status);
            }
        }
        self.service.call(rpc).await;
    }

    fn observer(&self) -> Option<&Arc<dyn LifecycleObserver>> {
        self.observer.as_ref()
    }
}
