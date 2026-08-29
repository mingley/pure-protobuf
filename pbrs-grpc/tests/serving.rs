//! Serving surface exercised the way a third party would: a hand-written
//! [`Service`], a [`Router`] hosting two services, and graceful drain.
//!
//! This file deliberately avoids the generated `GreeterServer` wrapper for the
//! hand-written cases, because the point is that the public API is enough.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{greeter_client, name_of, req, serve_at, spawn_greeter, Echo};
use pbrs_grpc::hello::{GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    Channel, ChannelConfig, Code, Empty, Incoming, InteropTestService, Outgoing, Request, Response,
    Router, Rpc, Server, ServerConfig, Service, ServiceExt, Status, TestServiceClient,
    TestServiceServer,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

/// A service written without any generated code, mounted on the public API.
struct Reverser {
    seen: Arc<AtomicUsize>,
}

impl Service for Reverser {
    const NAME: &'static str = "demo.Reverser";

    async fn call(&self, rpc: Rpc) {
        let seen = Arc::clone(&self.seen);
        match rpc.method() {
            "Reverse" => {
                let peer = rpc.remote_addr();
                let local = rpc.local_addr();
                let tls_id = rpc.peer_identity().cloned();
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    seen.fetch_add(1, Ordering::Relaxed);
                    if peer.is_none() {
                        return Err(Status::internal("expected a peer address"));
                    }
                    if local.is_none() || request.local_addr() != local {
                        return Err(Status::internal("expected a local address"));
                    }
                    if tls_id.is_some() || request.peer_identity().is_some() {
                        return Err(Status::internal("h2c has no TLS client certificate"));
                    }
                    let name: String = request
                        .get_ref()
                        .name()
                        .to_str()
                        .unwrap_or_default()
                        .chars()
                        .rev()
                        .collect();
                    let mut reply = HelloReply::new();
                    reply.set_message(name);
                    Ok(Response::new(reply))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

async fn channel(addr: SocketAddr) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

#[tokio::test]
async fn a_hand_written_service_serves_without_generated_code() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser {
        seen: Arc::clone(&seen),
    };
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    let channel = channel(addr).await;
    let reply: HelloReply = channel
        .unary("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(name_of(&reply), "desserts");
    assert_eq!(seen.load(Ordering::Relaxed), 1);

    let missing = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Nope", Request::new(req("x")))
        .await
        .expect_err("unknown method");
    assert_eq!(missing.code(), Code::Unimplemented);

    task.abort();
}

#[tokio::test]
async fn a_router_dispatches_between_two_services() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = channel(addr).await;

    let greeting = GreeterClient::new(channel.clone())
        .say_hello(Request::new(req("ada")))
        .await
        .expect("greeter");
    assert_eq!(name_of(greeting.get_ref()), "ada");

    TestServiceClient::new(channel.clone())
        .empty_call(Request::new(Empty::new()))
        .await
        .expect("test service");

    let missing = channel
        .unary::<HelloRequest, HelloReply>("/nope.Absent/Method", Request::new(req("x")))
        .await
        .expect_err("unmounted service");
    assert_eq!(missing.code(), Code::Unimplemented);

    task.abort();
}

#[tokio::test]
async fn router_reports_its_mounted_services() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(TestServiceServer::new(InteropTestService));
    let mut names: Vec<&str> = router.service_names().collect();
    names.sort_unstable();
    assert_eq!(names, ["grpc.testing.TestService", "helloworld.Greeter"]);
}

#[tokio::test]
async fn mounting_the_same_service_twice_keeps_the_last() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(GreeterServer::new(Echo));
    assert_eq!(router.service_names().count(), 1);
}

/// A handler slow enough that the drain has to wait for it.
struct Slow;

impl pbrs_grpc::Greeter for Slow {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("slow"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("slow"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("slow"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_finishes_in_flight_rpcs() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.say_hello(Request::new(req("ada")));

    // Drive the call far enough that Slow's 200 ms handler is running, then
    // signal drain. Creating a Call does not start the RPC; first poll does.
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before shutdown: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    shutdown_tx.send(()).expect("signal");

    let reply = call.await.expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");

    // Drain finishes on its own once the connection closes.
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_stops_accepting_new_connections() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    // Prove the server is up, then shut it down and let the drain complete.
    let client = GreeterClient::new(channel(addr).await);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("before shutdown");
    drop(client);
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");

    // A fresh connection now finds nothing listening, or finds a socket that
    // refuses to carry an RPC.
    let refused = match Channel::connect(addr).await {
        Err(_) => true,
        Ok(channel) => GreeterClient::new(channel)
            .say_hello(Request::new(req("late")))
            .await
            .is_err(),
    };
    assert!(refused, "the listener must be closed after drain");
}

/// A server-streaming handler whose producer stops as soon as reading the
/// request stream fails, which is what almost every real handler does. When the
/// read fails because the deadline expired, the RPC must not look like a clean
/// end of stream.
struct QuietUntilDeadline;

impl pbrs_grpc::Greeter for QuietUntilDeadline {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::Streaming::channel(4);
        drop(tokio::spawn(async move {
            // Swallows the error and stops, exactly like the reference
            // interop service and most hand-written handlers.
            while let Ok(Some(_)) = inbound.message().await {}
            drop(tx);
        }));
        Ok(Response::new(stream))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_deadline_is_never_a_clean_end_of_stream() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);

    // Repeat, because the original bug was a coin flip between the deadline
    // firing and the producer stopping.
    for _ in 0..12 {
        let mut request = Request::new(());
        request.set_timeout(Duration::from_millis(5));
        let (tx, call) = client.stream_hello(request);
        tx.send(req("ada")).await.ok();
        let outcome = match call.await {
            Err(status) => Err(status),
            Ok(response) => response.into_inner().message().await,
        };
        match outcome {
            Err(status) => assert_eq!(status.code(), Code::DeadlineExceeded),
            Ok(None) => panic!("an expired deadline must not read as a clean end"),
            Ok(Some(_)) => panic!("the handler sends nothing"),
        }
    }

    task.abort();
}

/// The wrapping-service pattern from `docs/grpc.md`: authenticate, then
/// delegate. `NAME` is inherited, so the wrapper mounts where the wrapped
/// service would.
struct RequireAuth<S> {
    inner: Arc<S>,
    token: String,
}

impl<S: Service> Service for RequireAuth<S> {
    const NAME: &'static str = S::NAME;

    async fn call(&self, rpc: Rpc) {
        if rpc.metadata().get("authorization") != Some(self.token.as_str()) {
            return rpc.reject(Status::unauthenticated("bad or missing token"));
        }
        self.inner.call(rpc).await;
    }
}

#[tokio::test]
async fn a_wrapping_service_can_reject_before_the_body_is_read() {
    let (addr, listener) = bind().await;
    let guard = RequireAuth {
        inner: Arc::new(GreeterServer::new(Echo)),
        token: "Bearer letmein".to_owned(),
    };
    let task = tokio::spawn(async move {
        Server::new(guard).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await);

    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("no token");
    assert_eq!(denied.code(), Code::Unauthenticated);

    let mut authorized = Request::new(req("ada"));
    authorized
        .metadata_mut()
        .insert("authorization", "Bearer letmein")
        .expect("metadata");
    let allowed = client.say_hello(authorized).await.expect("with token");
    assert_eq!(name_of(allowed.get_ref()), "ada");

    task.abort();
}

fn require_bearer(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("authorization") != Some("Bearer letmein") {
        return Err(Status::unauthenticated("bad or missing token"));
    }
    Ok(())
}

fn inject_bearer(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut()
        .insert("authorization", "Bearer letmein")?;
    Ok(())
}

#[tokio::test]
async fn h2c_requests_use_the_http_scheme() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&seen);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                let n = match rpc.scheme() {
                    Some("http") => 1,
                    Some("https") => 2,
                    _ => 3,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("rpc");
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn tcp_rpcs_expose_local_and_remote_addr() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&seen);
    let listen = addr;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                let n = match (rpc.local_addr(), rpc.remote_addr(), rpc.peer_identity()) {
                    (Some(local), Some(remote), None)
                        if local == listen && remote.ip().is_loopback() =>
                    {
                        1
                    }
                    _ => 2,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let reply = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "ada");
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn a_generated_server_interceptor_rejects_before_the_handler() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("no token");
    assert_eq!(denied.code(), Code::Unauthenticated);

    let allowed = GreeterClient::new(channel(addr).await)
        .intercept(inject_bearer)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("with token");
    assert_eq!(name_of(allowed.get_ref()), "ada");

    task.abort();
}

#[tokio::test]
async fn generated_server_interceptors_stack_in_declaration_order() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-trace").is_none() {
                    return Err(Status::invalid_argument("missing x-trace"));
                }
                Ok(())
            })
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let missing_trace = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("neither header");
    assert_eq!(missing_trace.code(), Code::InvalidArgument);

    let mut only_auth = Request::new(req("ada"));
    only_auth
        .metadata_mut()
        .insert("authorization", "Bearer letmein")
        .expect("metadata");
    let still_trace = client
        .say_hello(only_auth)
        .await
        .expect_err("auth without trace");
    assert_eq!(still_trace.code(), Code::InvalidArgument);

    let authed = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.metadata_mut().insert("x-trace", "1")?;
        call.metadata_mut()
            .insert("authorization", "Bearer letmein")?;
        Ok(())
    });
    let allowed = authed
        .say_hello(Request::new(req("ada")))
        .await
        .expect("both headers");
    assert_eq!(name_of(allowed.get_ref()), "ada");

    task.abort();
}

#[tokio::test]
async fn intercept_on_a_generated_server_survives_add_service() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("no token");
    assert_eq!(denied.code(), Code::Unauthenticated);

    let allowed = GreeterClient::new(channel(addr).await)
        .intercept(inject_bearer)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("with token");
    assert_eq!(name_of(allowed.get_ref()), "ada");

    let denied_empty = TestServiceClient::new(channel(addr).await)
        .empty_call(Request::new(Empty::new()))
        .await
        .expect_err("router interceptor covers every mount");
    assert_eq!(denied_empty.code(), Code::Unauthenticated);

    task.abort();
}

#[tokio::test]
async fn service_ext_intercept_wraps_a_hand_written_service() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser {
        seen: Arc::clone(&seen),
    }
    .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    let ch = channel(addr).await;
    let denied = ch
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect_err("no token");
    assert_eq!(denied.code(), Code::Unauthenticated);
    assert_eq!(seen.load(Ordering::Relaxed), 0);

    let allowed = ch
        .intercept(inject_bearer)
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("with token")
        .into_inner();
    assert_eq!(name_of(&allowed), "desserts");
    assert_eq!(seen.load(Ordering::Relaxed), 1);

    task.abort();
}

#[tokio::test]
async fn service_ext_interceptors_stack_in_declaration_order() {
    // Same contract as generated_server_interceptors_stack_in_declaration_order,
    // but wrapping the Service itself. Intercepted::intercept is inherent, so
    // svc.intercept(trace).intercept(require_bearer) runs trace first. Onion
    // wrapping would run require_bearer first and return Unauthenticated.
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser {
        seen: Arc::clone(&seen),
    }
    .intercept(|rpc: &mut Rpc| {
        if rpc.metadata().get("x-trace").is_none() {
            return Err(Status::invalid_argument("missing x-trace"));
        }
        Ok(())
    })
    .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    let ch = channel(addr).await;
    let missing_trace = ch
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect_err("neither header");
    assert_eq!(missing_trace.code(), Code::InvalidArgument);

    let mut only_auth = Request::new(req("stressed"));
    only_auth
        .metadata_mut()
        .insert("authorization", "Bearer letmein")
        .expect("metadata");
    let still_trace = ch
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", only_auth)
        .await
        .expect_err("auth without trace");
    assert_eq!(still_trace.code(), Code::InvalidArgument);

    let allowed = ch
        .intercept(|call: &mut Outgoing<'_>| {
            call.metadata_mut().insert("x-trace", "1")?;
            call.metadata_mut()
                .insert("authorization", "Bearer letmein")?;
            Ok(())
        })
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("both headers")
        .into_inner();
    assert_eq!(name_of(&allowed), "desserts");
    assert_eq!(seen.load(Ordering::Relaxed), 1);

    task.abort();
}

#[tokio::test]
async fn an_interceptor_can_attach_typed_state_the_handler_reads() {
    struct TenantEcho;

    impl Service for TenantEcho {
        const NAME: &'static str = "demo.TenantEcho";

        async fn call(&self, rpc: Rpc) {
            rpc.unary(|request: Request<HelloRequest>| async move {
                let tenant = request
                    .extensions()
                    .get::<String>()
                    .cloned()
                    .unwrap_or_default();
                let mut reply = HelloReply::new();
                reply.set_message(tenant);
                Ok(Response::new(reply))
            })
            .await;
        }
    }

    fn with_tenant(rpc: &mut Rpc) -> Result<(), Status> {
        let Some(tenant) = rpc.metadata().get("x-tenant").map(str::to_owned) else {
            return Err(Status::unauthenticated("missing x-tenant"));
        };
        rpc.extensions_mut().insert(tenant);
        Ok(())
    }

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(with_tenant))
            .serve_listener(listener)
            .await
            .ok();
    });

    let ch = channel(addr).await;
    let denied = ch
        .unary::<HelloRequest, HelloReply>("/demo.TenantEcho/Ping", Request::new(req("ignored")))
        .await
        .expect_err("no tenant");
    assert_eq!(denied.code(), Code::Unauthenticated);

    let mut tagged = Request::new(req("ignored"));
    tagged
        .metadata_mut()
        .insert("x-tenant", "acme")
        .expect("metadata");
    let reply = ch
        .unary::<HelloRequest, HelloReply>("/demo.TenantEcho/Ping", tagged)
        .await
        .expect("with tenant")
        .into_inner();
    assert_eq!(name_of(&reply), "acme");

    task.abort();
}

#[tokio::test]
async fn router_interceptors_stack_in_declaration_order() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-trace").is_none() {
                    return Err(Status::invalid_argument("missing x-trace"));
                }
                Ok(())
            })
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let missing_trace = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("neither header");
    assert_eq!(missing_trace.code(), Code::InvalidArgument);

    let mut only_auth = Request::new(req("ada"));
    only_auth
        .metadata_mut()
        .insert("authorization", "Bearer letmein")
        .expect("metadata");
    let still_trace = client
        .say_hello(only_auth)
        .await
        .expect_err("auth without trace");
    assert_eq!(still_trace.code(), Code::InvalidArgument);

    let authed = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.metadata_mut().insert("x-trace", "1")?;
        call.metadata_mut()
            .insert("authorization", "Bearer letmein")?;
        Ok(())
    });
    let allowed = authed
        .say_hello(Request::new(req("ada")))
        .await
        .expect("both headers");
    assert_eq!(name_of(allowed.get_ref()), "ada");

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_authority() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let channel = channel(addr).await;
    let want = channel.authority().to_owned();
    let client = GreeterClient::new(channel).intercept(move |call: &mut Outgoing<'_>| {
        if call.authority() != want.as_str() {
            return Err(Status::internal(format!(
                "authority {} want {want}",
                call.authority()
            )));
        }
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("authority");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_the_authority() {
    let (addr, listener) = bind().await;
    let want = addr.to_string();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                if rpc.authority() != Some(want.as_str()) {
                    return Err(Status::internal(format!(
                        "authority {:?} want {want}",
                        rpc.authority()
                    )));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let reply = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("authority");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_cannot_insert_reserved_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.metadata_mut()
            .insert("grpc-previous-rpc-attempts", "1")?;
        Ok(())
    });
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("reserved");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    assert!(
        err.message().contains("reserved"),
        "expected reserved-key status, got {err}"
    );
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.metadata_mut().insert("connection", "close")?;
        Ok(())
    });
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("hop-by-hop");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await)
        .intercept(|_: &mut Outgoing<'_>| Err(Status::failed_precondition("blocked locally")));
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("interceptor");
    assert_eq!(err.code(), Code::FailedPrecondition);

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_method_path() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-path") != Some("/helloworld.Greeter/SayHello") {
                    return Err(Status::invalid_argument("missing stamped path"));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        let path = call.path();
        call.metadata_mut().insert("x-path", path)?;
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("stamped");
    assert_eq!(name_of(reply.get_ref()), "ada");

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_set_a_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.set_timeout(Duration::from_millis(40));
        Ok(())
    });
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded);

    task.abort();
}

#[tokio::test]
async fn a_channel_timeout_expires_when_the_request_omits_one() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).timeout(Duration::from_millis(40));
    let started = Instant::now();
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "channel default should fire before Slow returns: {:?}",
        started.elapsed()
    );
    task.abort();
}

#[tokio::test]
async fn a_channel_config_timeout_is_the_default_rpc_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let mut last = None;
    let channel = {
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_with(
                addr,
                ChannelConfig::new().timeout(Duration::from_millis(40)),
            )
            .await
            {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("could not connect: {last:?}"))
    };
    let started = Instant::now();
    let err = GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "ChannelConfig::timeout should fire: {:?}",
        started.elapsed()
    );
    task.abort();
}

#[tokio::test]
async fn a_request_timeout_wins_over_the_channel_default() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).timeout(Duration::from_millis(40));
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let reply = client.say_hello(request).await.expect("request deadline");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_clear_the_channel_timeout() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await)
        .timeout(Duration::from_millis(40))
        .intercept(|call: &mut Outgoing<'_>| {
            call.clear_timeout();
            Ok(())
        });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("cleared");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_reads_caller_extensions() {
    #[derive(Clone)]
    struct Tenant(String);

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-tenant") != Some("acme") {
                    return Err(Status::unauthenticated("missing tenant"));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        let Some(tenant) = call.extensions().get::<Tenant>().cloned() else {
            return Err(Status::internal("missing Tenant"));
        };
        call.metadata_mut().insert("x-tenant", tenant.0)?;
        Ok(())
    });
    let mut request = Request::new(req("ada"));
    request.extensions_mut().insert(Tenant("acme".into()));
    let reply = client.say_hello(request).await.expect("stamped");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let missing = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("no tenant");
    assert_eq!(missing.code(), Code::Internal);

    task.abort();
}

#[tokio::test]
async fn client_interceptors_stack_and_share_extensions() {
    #[derive(Clone, Copy)]
    struct Trace(&'static str);

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-trace") != Some("abc") {
                    return Err(Status::invalid_argument("missing trace"));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await)
        .intercept(|call: &mut Outgoing<'_>| {
            call.extensions_mut().insert(Trace("abc"));
            Ok(())
        })
        .intercept(|call: &mut Outgoing<'_>| {
            let Some(trace) = call.extensions().get::<Trace>().copied() else {
                return Err(Status::internal("first interceptor did not run"));
            };
            call.metadata_mut().insert("x-trace", trace.0)?;
            Ok(())
        });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("stacked");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(true);
        Ok(())
    });
    let mut request = Request::new(req("late"));
    request.set_timeout(Duration::from_secs(5));
    let mut call = client.say_hello(request);

    tokio::select! {
        biased;
        result = &mut call => panic!("RPC finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = serve_at(addr, Echo, ServerConfig::default())
        .await
        .expect("serve");

    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("wait-for-ready hung after listen")
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "late");
}

#[tokio::test]
async fn a_client_interceptor_can_reject_with_typed_status_details() {
    let (addr, listener) = bind().await;
    drop(listener);
    let client = GreeterClient::new(Channel::connect_lazy(addr).expect("lazy")).intercept(
        |_: &mut Outgoing<'_>| {
            let mut info = pbrs_grpc::pb::ErrorInfo::new();
            info.set_reason("BLOCKED");
            info.set_domain("example.com");
            Err(Status::with_error_details(
                Code::FailedPrecondition,
                "blocked locally",
                [pbrs_grpc::pb::Any::pack(&info)?],
            )?)
        },
    );
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("interceptor");
    assert_eq!(err.code(), Code::FailedPrecondition);
    let info = err
        .error_details()
        .expect("details")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "BLOCKED");
}

#[tokio::test]
async fn a_server_interceptor_injects_metadata_the_handler_sees() {
    struct ActorEcho;

    impl Service for ActorEcho {
        const NAME: &'static str = "demo.ActorEcho";

        async fn call(&self, rpc: Rpc) {
            rpc.unary(|request: Request<HelloRequest>| async move {
                let actor = request.metadata().get("x-actor").unwrap_or("").to_owned();
                let mut reply = HelloReply::new();
                reply.set_message(actor);
                Ok(Response::new(reply))
            })
            .await;
        }
    }

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(ActorEcho.intercept(|rpc: &mut Rpc| {
            rpc.metadata_mut().insert("x-actor", "kernel")?;
            Ok(())
        }))
        .serve_listener(listener)
        .await
        .ok();
    });

    let reply = channel(addr)
        .await
        .unary::<HelloRequest, HelloReply>("/demo.ActorEcho/Ping", Request::new(req("ignored")))
        .await
        .expect("injected")
        .into_inner();
    assert_eq!(name_of(&reply), "kernel");

    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_strips_metadata_before_the_handler() {
    struct SeesAuth;

    impl Service for SeesAuth {
        const NAME: &'static str = "demo.SeesAuth";

        async fn call(&self, rpc: Rpc) {
            rpc.unary(|request: Request<HelloRequest>| async move {
                if request.metadata().get("authorization").is_some() {
                    return Err(Status::internal("authorization leaked to handler"));
                }
                let mut reply = HelloReply::new();
                reply.set_message(request.get_ref().name());
                Ok(Response::new(reply))
            })
            .await;
        }
    }

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(SeesAuth.intercept(|rpc: &mut Rpc| {
            rpc.metadata_mut().remove("authorization");
            Ok(())
        }))
        .serve_listener(listener)
        .await
        .ok();
    });

    let mut tagged = Request::new(req("ada"));
    tagged
        .metadata_mut()
        .insert("authorization", "Bearer secret")
        .expect("metadata");
    let reply = channel(addr)
        .await
        .unary::<HelloRequest, HelloReply>("/demo.SeesAuth/Ping", tagged)
        .await
        .expect("stripped")
        .into_inner();
    assert_eq!(name_of(&reply), "ada");

    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_interceptor_can_tighten_the_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(|rpc: &mut Rpc| {
                rpc.set_timeout(Duration::from_secs(5));
                Ok(())
            })
            .intercept(|rpc: &mut Rpc| {
                rpc.set_timeout(Duration::from_millis(20));
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let started = Instant::now();
    let err = client.say_hello(request).await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "interceptor cap should win: {:?}",
        started.elapsed()
    );

    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_cannot_extend_the_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(|rpc: &mut Rpc| {
                rpc.set_timeout(Duration::from_secs(5));
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_millis(50));
    let started = Instant::now();
    let err = client.say_hello(request).await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "interceptor must not extend the client deadline: {:?}",
        started.elapsed()
    );
    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_the_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                let peer = rpc.peer_timeout();
                if peer != Some(Duration::from_secs(5)) {
                    return Err(Status::internal(format!("peer timeout {peer:?}")));
                }
                rpc.set_timeout(Duration::from_secs(1));
                let effective = rpc.effective_timeout();
                if effective != Some(Duration::from_secs(1)) {
                    return Err(Status::internal(format!("effective {effective:?}")));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let reply = client.say_hello(request).await.expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_a_missing_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.peer_timeout().is_some() {
                    return Err(Status::internal("unexpected peer timeout"));
                }
                if rpc.effective_timeout().is_some() {
                    return Err(Status::internal("unexpected effective timeout"));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let reply = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_can_reject_with_typed_status_details() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|_rpc: &mut Rpc| {
                let mut info = pbrs_grpc::pb::ErrorInfo::new();
                info.set_reason("API_DISABLED");
                info.set_domain("example.com");
                Err(Status::with_error_details(
                    Code::FailedPrecondition,
                    "api disabled",
                    [pbrs_grpc::Any::pack(&info)?],
                )?)
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let err = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("details");
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(err.message(), "api disabled");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let details = err.error_details().expect("ErrorDetails");
    assert_eq!(
        details
            .error_info
            .expect("ErrorInfo")
            .reason()
            .to_str()
            .unwrap_or(""),
        "API_DISABLED"
    );

    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let (a, b) = tokio::join!(
        client.say_hello(Request::new(req("a"))),
        client.say_hello(Request::new(req("b"))),
    );
    let codes = [
        a.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
        b.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
    ];
    assert!(
        codes.contains(&Code::Ok) && codes.contains(&Code::ResourceExhausted),
        "one Ok and one RESOURCE_EXHAUSTED, got {codes:?}"
    );

    task.abort();
}

#[tokio::test]
async fn outbound_rpcs_send_a_kernel_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                let md = rpc.metadata();
                let ua = md.get("user-agent").unwrap_or("");
                if !ua.starts_with("pbrs-grpc/") {
                    return Err(Status::invalid_argument(format!("user-agent {ua:?}")));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let reply = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("user-agent");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

/// Answers without ever reading the request stream. Inbound messages are
/// decoded on the handler's task, so a handler that ignores them must still
/// terminate the RPC rather than leaving the client blocked on the window.
struct Deaf;

impl pbrs_grpc::Greeter for Deaf {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message("ignored your stream");
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("deaf"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_that_ignores_its_request_stream_still_answers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.client_hello(Request::new(()));

    // Push more than fits in any buffer, from another task, so a hang here
    // would show up as the test timing out rather than as a deadlock.
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });

    let reply = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("must not hang")
        .expect("must answer");
    assert_eq!(name_of(reply.get_ref()), "ignored your stream");
    sender.abort();
    task.abort();
}

#[tokio::test]
async fn config_flows_from_the_generated_server_to_the_router() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_decoding_message_size(16)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = greeter_client(addr).await;
    let err = client
        .say_hello(Request::new(req(&"x".repeat(64))))
        .await
        .expect_err("over the server cap");
    assert_eq!(err.code(), Code::ResourceExhausted);

    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_channel_redials_the_same_address() {
    let (addr, client, guard) = spawn_greeter(Echo).await.expect("spawn");
    let before = client
        .say_hello(Request::new(req("before")))
        .await
        .expect("before");
    assert_eq!(name_of(before.get_ref()), "before");

    drop(guard);
    let _guard = serve_at(addr, Echo, ServerConfig::default())
        .await
        .expect("rebind");

    // The first attempt can still land on the dying connection (`ready`
    // succeeded, then GOAWAY). Subsequent RPCs redial the rebound listener.
    let mut last = None;
    let after = 'done: {
        for _ in 0..40 {
            match tokio::time::timeout(
                Duration::from_secs(2),
                client.say_hello(Request::new(req("after"))),
            )
            .await
            {
                Ok(Ok(reply)) => break 'done reply,
                Ok(Err(status)) => last = Some(status),
                Err(_) => last = Some(Status::unavailable("redial attempt timed out")),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("after: {last:?}");
    };
    assert_eq!(name_of(after.get_ref()), "after");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_channel_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    client
        .say_hello(Request::new(req("before")))
        .await
        .expect("before");

    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");

    let mut request = Request::new(req("gone"));
    request.set_timeout(Duration::from_millis(200));
    let err = tokio::time::timeout(Duration::from_secs(2), client.say_hello(request))
        .await
        .expect("reconnect to a closed port hung")
        .expect_err("rpc succeeded with no server");
    assert!(
        matches!(
            err.code(),
            Code::Unavailable | Code::DeadlineExceeded | Code::Cancelled
        ),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_lazy_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel);
    let started = Instant::now();
    let err = tokio::time::timeout(
        Duration::from_secs(2),
        client.say_hello(Request::new(req("x"))),
    )
    .await
    .expect("fail-fast hung")
    .expect_err("rpc succeeded with no server");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel);
    let mut request = Request::new(req("late"));
    request.set_wait_for_ready(true);
    request.set_timeout(Duration::from_secs(5));
    let mut call = client.say_hello(request);

    // Creating a Call does not start the RPC; first poll does. Drive it
    // long enough to prove it is retrying, then bind the server.
    tokio::select! {
        biased;
        result = &mut call => panic!("RPC finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = serve_at(addr, Echo, ServerConfig::default())
        .await
        .expect("serve");

    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("wait-for-ready hung after listen")
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "late");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    let client = GreeterClient::new(channel);
    let mut request = Request::new(req("late"));
    request.set_timeout(Duration::from_secs(5));
    let mut call = client.say_hello(request);

    tokio::select! {
        biased;
        result = &mut call => panic!("RPC finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = serve_at(addr, Echo, ServerConfig::default())
        .await
        .expect("serve");

    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("channel wait-for-ready hung after listen")
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "late");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    let client = GreeterClient::new(channel);
    let mut request = Request::new(req("nope"));
    request.set_wait_for_ready(false);
    request.set_timeout(Duration::from_secs(5));
    let started = Instant::now();
    let err = tokio::time::timeout(Duration::from_secs(2), client.say_hello(request))
        .await
        .expect("opt-out hung")
        .expect_err("rpc succeeded with no server");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel);
    let mut request = Request::new(req("x"));
    request.set_wait_for_ready(true);
    request.set_timeout(Duration::from_millis(80));
    let started = Instant::now();
    let err = client
        .say_hello(request)
        .await
        .expect_err("should time out");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "deadline returned too fast: {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_times_out_when_the_peer_never_speaks_http2() {
    let (addr, listener) = bind().await;
    let started = Instant::now();
    let err = Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
    )
    .await
    .expect_err("handshake should time out");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        err.message().contains("timed out"),
        "expected timeout status, got {err}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "timed out too fast: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timed out too slow: {:?}",
        started.elapsed()
    );
    drop(listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_to_a_closed_port_fails_fast() {
    let (addr, listener) = bind().await;
    drop(listener);
    let started = Instant::now();
    let err = Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_secs(20)),
    )
    .await
    .expect_err("closed port");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "refused connect took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mute_tcp_peer_does_not_stop_the_server_serving() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .handshake_timeout(Duration::from_millis(80))
            .serve_listener(listener)
            .await
            .ok();
    });
    let mute = tokio::net::TcpStream::connect(addr)
        .await
        .expect("mute connect");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let client = GreeterClient::new(channel(addr).await);
    let reply = client
        .say_hello(Request::new(req("after-mute")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "after-mute");
    drop(mute);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_age_goaway_then_the_channel_redials() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let first = client
        .say_hello(Request::new(req("before")))
        .await
        .expect("before");
    assert_eq!(name_of(first.get_ref()), "before");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(Request::new(req("after"))),
    )
    .await
    .expect("redial hung")
    .expect("after");
    assert_eq!(name_of(after.get_ref()), "after");
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_idle_goaway_then_the_channel_redials() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age_grace(Duration::from_secs(2))
            .max_connection_idle(Duration::from_millis(80))
            .keep_alive_interval(Duration::from_millis(20))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let first = client
        .say_hello(Request::new(req("before")))
        .await
        .expect("before");
    assert_eq!(name_of(first.get_ref()), "before");

    // Keepalive PINGs must not reset idle. Wait well past the idle cap.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let after = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(Request::new(req("after"))),
    )
    .await
    .expect("redial hung")
    .expect("after");
    assert_eq!(name_of(after.get_ref()), "after");
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_age_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before age: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past grace")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_idle_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before idle: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past idle")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

async fn channel_with(addr: SocketAddr, cfg: ChannelConfig) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_with(addr, cfg).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

struct CountingIncoming {
    listener: TcpListener,
    n: Arc<AtomicUsize>,
}

impl Incoming for CountingIncoming {
    type Io = tokio::net::TcpStream;

    async fn accept(&mut self) -> pbrs_grpc::IncomingAccept<Self::Io> {
        match self.listener.accept().await {
            Ok((io, addr)) => {
                self.n.fetch_add(1, Ordering::Relaxed);
                Some(Ok((io, Some(addr))))
            }
            Err(e) => Some(Err(Status::unavailable(e.to_string()))),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_max_connection_idle_closes_the_socket() {
    let accepts = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let n = Arc::clone(&accepts);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_incoming(CountingIncoming { listener, n })
            .await
            .ok();
    });
    let cfg = ChannelConfig::new()
        .max_connection_idle(Duration::from_millis(80))
        .keep_alive_interval(Duration::from_millis(20));
    let client = GreeterClient::new(channel_with(addr, cfg).await);
    assert_eq!(accepts.load(Ordering::Relaxed), 1, "dial is one accept");
    let first = client
        .say_hello(Request::new(req("before")))
        .await
        .expect("before");
    assert_eq!(name_of(first.get_ref()), "before");
    assert_eq!(accepts.load(Ordering::Relaxed), 1, "unary reuses the dial");

    tokio::time::sleep(Duration::from_millis(250)).await;
    let after = tokio::time::timeout(
        Duration::from_secs(5),
        client.say_hello(Request::new(req("after"))),
    )
    .await
    .expect("redial hung")
    .expect("after");
    assert_eq!(name_of(after.get_ref()), "after");
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        2,
        "idle must tear down the socket so the next RPC dials again"
    );
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    let client = GreeterClient::new(channel_with(addr, cfg).await);
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before client idle: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past client idle")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

struct DelayedStream;

impl pbrs_grpc::Greeter for DelayedStream {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let name = request
            .get_ref()
            .name()
            .to_str()
            .unwrap_or_default()
            .to_owned();
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut reply = HelloReply::new();
            reply.set_message(name);
            tx.send(reply).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_idle_holds_a_server_stream() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_listener(listener)
            .await
            .ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    let client = GreeterClient::new(channel_with(addr, cfg).await);
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reply = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("stream hung")
        .expect("stream status")
        .expect("one message");
    assert_eq!(name_of(&reply), "ada");
    task.abort();
}

#[cfg(unix)]
struct UnixSockGuard(std::path::PathBuf);

#[cfg(unix)]
impl Drop for UnixSockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
fn unix_test_path() -> (std::path::PathBuf, UnixSockGuard) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    (path.clone(), UnixSockGuard(path))
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_socket_unary() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.remote_addr().is_some() || rpc.local_addr().is_some() {
                    return Err(Status::internal("unix has no std::net::SocketAddr"));
                }
                if rpc.peer_identity().is_some() {
                    return Err(Status::internal("unix has no TLS client certificate"));
                }
                Ok(())
            })
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect_unix(&path).await.expect("connect");
    let client = GreeterClient::new(channel);
    let reply = client
        .say_hello(Request::new(req("uds")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "uds");
    task.abort();
}

#[cfg(unix)]
async fn unix_channel(path: &std::path::Path) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_unix(path).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("connect unix {}: {last:?}", path.display());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_until_shutdown_serves_then_drains() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    let reply = GreeterClient::new(unix_channel(&path).await)
        .say_hello(Request::new(req("uds")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "uds");
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("unix drain hung")
        .expect("join");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_until_shutdown_replaces_leftover_then_drains() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_unlink_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    let reply = GreeterClient::new(unix_channel(&path).await)
        .say_hello(Request::new(req("uds")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "uds");
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("unix unlink drain hung")
        .expect("join");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_sees_unix_localhost_authority() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect_unix(&path).await.expect("connect");
    assert_eq!(channel.authority(), "localhost");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        if call.authority() != "localhost" {
            return Err(Status::internal(format!(
                "authority {} want localhost",
                call.authority()
            )));
        }
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("uds")))
        .await
        .expect("unix authority");
    assert_eq!(name_of(reply.get_ref()), "uds");
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_lazy_fails_fast_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    let client = GreeterClient::new(channel);
    let err = tokio::time::timeout(
        Duration::from_secs(2),
        client.say_hello(Request::new(req("x"))),
    )
    .await
    .expect("fail-fast hung")
    .expect_err("rpc succeeded with no socket");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    let client = GreeterClient::new(channel);
    let mut request = Request::new(req("late"));
    request.set_wait_for_ready(true);
    request.set_timeout(Duration::from_secs(5));
    let mut call = client.say_hello(request);

    tokio::select! {
        biased;
        result = &mut call => panic!("RPC finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });

    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("wait-for-ready hung after listen")
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "late");
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_fails_on_a_leftover_socket() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let err = GreeterServer::new(Echo)
        .serve_unix(&path)
        .await
        .expect_err("stale path should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_replaces_a_leftover_socket() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix_unlink(sock).await.ok();
    });
    let mut last = None;
    let channel = {
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_unix(&path).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("connect after unlink: {last:?}"))
    };
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("uds")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "uds");
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_does_not_steal_a_live_socket() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let live = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let channel = {
        let mut last = None;
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_unix(&path).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("live listener never came up: {last:?}"))
    };
    let err = GreeterServer::new(Echo)
        .serve_unix_unlink(&path)
        .await
        .expect_err("must not steal a live socket");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("still")))
        .await
        .expect("original listener must keep serving");
    assert_eq!(name_of(reply.get_ref()), "still");
    live.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_does_not_steal_when_the_backlog_is_full() {
    let (path, _guard) = unix_test_path();
    let sock =
        socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).expect("socket");
    sock.bind(&socket2::SockAddr::unix(&path).expect("addr"))
        .expect("bind");
    sock.listen(1).expect("listen");
    let mut held = Vec::new();
    for _ in 0..8 {
        match tokio::time::timeout(
            Duration::from_millis(20),
            tokio::net::UnixStream::connect(&path),
        )
        .await
        {
            Ok(Ok(stream)) => held.push(stream),
            Ok(Err(_)) | Err(_) => break,
        }
    }
    let err = GreeterServer::new(Echo)
        .serve_unix_unlink(&path)
        .await
        .expect_err("full backlog is a live listener");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(path.exists(), "must not unlink a live inode");
    drop(held);
    drop(sock);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_connect_times_out_when_the_peer_never_speaks_http2() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let started = Instant::now();
    let err = Channel::connect_unix_with(
        &path,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
    )
    .await
    .expect_err("handshake should time out");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        err.message().contains("timed out"),
        "expected timeout status, got {err}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "timed out too fast: {:?}",
        started.elapsed()
    );
    drop(listener);
}

#[tokio::test]
async fn a_server_timeout_expires_when_the_client_sends_none() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    task.abort();
}

#[tokio::test]
async fn a_server_timeout_caps_a_longer_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let started = Instant::now();
    let err = client.say_hello(request).await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "server cap should win: {:?}",
        started.elapsed()
    );
    task.abort();
}

#[tokio::test]
async fn extra_connections_are_refused_when_the_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_listener(listener)
            .await
            .ok();
    });
    let first = channel(addr).await;
    let err = Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(300)),
    )
    .await
    .expect_err("second connection should be refused");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    let _ = channel(addr).await;
    task.abort();
}

#[tokio::test]
async fn tcp_keepalive_still_serves_a_unary() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .tcp_keepalive(Duration::from_secs(15))
            .serve_listener(listener)
            .await
            .ok();
    });
    let cfg = ChannelConfig::new().tcp_keepalive(Duration::from_secs(15));
    let mut last = None;
    let connected = {
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_with(addr, cfg).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("connect with tcp keepalive: {last:?}"))
    };
    let reply = GreeterClient::new(connected)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

fn duplex_pair() -> (tokio::io::DuplexStream, tokio::io::DuplexStream) {
    tokio::io::duplex(1024 * 1024)
}

struct OneIncoming<IO> {
    io: Option<IO>,
}

impl<IO> Incoming for OneIncoming<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    type Io = IO;

    fn accept(
        &mut self,
    ) -> impl std::future::Future<Output = pbrs_grpc::IncomingAccept<IO>> + Send {
        let io = self.io.take();
        async move {
            match io {
                Some(io) => Some(Ok((io, None))),
                None => std::future::pending().await,
            }
        }
    }
}

#[tokio::test]
async fn from_io_round_trips_without_tcp() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert!(format!("{channel:?}").contains("once"), "{channel:?}");
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    server.abort();
}

#[tokio::test]
async fn from_io_authority_is_visible_to_interceptors() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.authority() != Some("my-svc") {
                    return Err(Status::internal(format!(
                        "server authority {:?}",
                        rpc.authority()
                    )));
                }
                if rpc.remote_addr().is_some() || rpc.local_addr().is_some() {
                    return Err(Status::internal("from_io must not invent TCP addrs"));
                }
                if rpc.peer_identity().is_some() {
                    return Err(Status::internal("from_io must not invent a TLS identity"));
                }
                Ok(())
            })
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "my-svc")
        .await
        .expect("from_io");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        if call.authority() != "my-svc" {
            return Err(Status::internal(format!(
                "client authority {}",
                call.authority()
            )));
        }
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("from_io authority");
    assert_eq!(name_of(reply.get_ref()), "ada");
    server.abort();
}

#[tokio::test]
async fn from_io_idle_close_cannot_redial() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        ChannelConfig::new().max_connection_idle(Duration::from_millis(80)),
    )
    .await
    .expect("from_io");
    let client = GreeterClient::new(channel);
    let first = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(first.get_ref()), "ada");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let err = tokio::time::timeout(
        Duration::from_secs(2),
        client.say_hello(Request::new(req("late"))),
    )
    .await
    .expect("idle close hung")
    .expect_err("once channel cannot redial after idle close");
    assert_eq!(err.code(), Code::Unavailable);
    server.abort();
}

#[tokio::test]
async fn from_io_cannot_redial() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(req("a")))
        .await
        .expect("first");
    server.abort();
    let err = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client.say_hello(Request::new(req("b"))).await {
                Ok(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(e) => return e,
            }
        }
    })
    .await
    .expect("should become unavailable");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

#[tokio::test]
async fn serve_with_incoming_accepts_a_duplex() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_incoming(OneIncoming {
                io: Some(server_io),
            })
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    server.abort();
}

/// Sleeps long enough that a cancelled caller would otherwise leave it running.
struct Hang {
    started: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
}

impl pbrs_grpc::Greeter for Hang {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.started.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.finished.fetch_add(1, Ordering::Relaxed);
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("hang"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("hang"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("hang"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_reset_drops_the_handler() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let call = client.say_hello(Request::new(req("ada")));
    let handle = call.handle();
    let mut call = call;
    tokio::select! {
        biased;
        result = &mut call => panic!("Hang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    handle.cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "handler should have been dropped, not run to completion"
    );
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_call_drops_the_handler() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Hang returned before drop: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    drop(call);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "dropping the Call should RST the stream and drop the handler"
    );
    task.abort();
}

/// Refuses a request that was not gzipped.
struct GzipProbe;

impl pbrs_grpc::Greeter for GzipProbe {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        if !request.compressed() {
            return Err(Status::invalid_argument("expected gzip"));
        }
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("gzip-probe"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gzip-probe"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gzip-probe"))
    }
}

#[tokio::test]
async fn a_prefixed_user_agent_is_sent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                let md = rpc.metadata();
                let ua = md.get("user-agent").unwrap_or("");
                if !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
                    return Err(Status::invalid_argument(format!("user-agent {ua:?}")));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = channel(addr)
        .await
        .user_agent("inventory/2.1")
        .expect("user-agent");
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("prefixed user-agent");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn metadata_cannot_override_the_kernel_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                let md = rpc.metadata();
                let ua = md.get("user-agent").unwrap_or("");
                if !ua.starts_with("pbrs-grpc/") {
                    return Err(Status::invalid_argument(format!("user-agent {ua:?}")));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.metadata_mut().insert("user-agent", "evil-agent")?;
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("kernel user-agent wins");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn the_server_gzips_when_configured_and_the_client_accepts() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let reply = GreeterClient::new(channel(addr).await)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("gzip reply");
    assert!(
        reply.compressed(),
        "server send_compressed plus client grpc-accept-encoding: gzip"
    );
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn the_client_gzips_when_configured() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = channel(addr).await.send_compressed();
    let reply = GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("gzip request");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_set_compress() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.set_compress(true);
        Ok(())
    });
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("gzip request");
    assert_eq!(name_of(reply.get_ref()), "ada");
    task.abort();
}
