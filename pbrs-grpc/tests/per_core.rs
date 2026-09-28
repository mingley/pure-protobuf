//! Thread-per-core mode: sharded serving, per-core client pools, and parity
//! with the default mode for shutdown, drain, and limits.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{name_of, name_of_request, reply, req};
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{
    Channel, ClientTls, Code, Identity, Request, Response, Router, Rpc, Server, ServerTls, Service,
};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

/// `Ping` echoes; `Block` parks every call on the gate, so tests can hold
/// connections and RPC permits open deterministically.
#[derive(Clone)]
struct PerCoreSvc {
    seen: Arc<AtomicUsize>,
    gate: Arc<tokio::sync::Notify>,
}

impl Service for PerCoreSvc {
    const NAME: &'static str = "test.PerCore";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Ping" => {
                let seen = Arc::clone(&self.seen);
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    seen.fetch_add(1, Ordering::Relaxed);
                    Ok(Response::new(reply(format!(
                        "pong:{}",
                        name_of_request(request.get_ref())
                    ))))
                })
                .await;
            }
            "Block" => {
                let gate = Arc::clone(&self.gate);
                let seen = Arc::clone(&self.seen);
                rpc.unary(move |_request: Request<HelloRequest>| async move {
                    gate.notified().await;
                    seen.fetch_add(1, Ordering::Relaxed);
                    Ok(Response::new(reply("unblocked")))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

fn svc() -> (PerCoreSvc, Arc<AtomicUsize>, Arc<tokio::sync::Notify>) {
    let seen = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(tokio::sync::Notify::new());
    (
        PerCoreSvc {
            seen: Arc::clone(&seen),
            gate: Arc::clone(&gate),
        },
        seen,
        gate,
    )
}

fn loopback0() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
}

async fn channel(addr: SocketAddr) -> Channel {
    Channel::connect(addr).await.expect("connect")
}

async fn ping(channel: &Channel, name: &str) -> HelloReply {
    channel
        .unary("/test.PerCore/Ping", Request::new(req(name)))
        .await
        .expect("ping")
        .into_inner()
}

/// Bind `cores` shards on an ephemeral port and serve them on a task.
/// Returns the bound address and the serve handle.
async fn serve_shards(
    server: Server<PerCoreSvc>,
    cores: usize,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<Result<(), pbrs_grpc::Status>>,
) {
    let (bound, listeners) = server
        .bind_per_core(loopback0(), cores)
        .expect("bind shards");
    assert_ne!(bound.port(), 0, "port 0 must resolve");
    let serve = tokio::spawn(async move { server.serve_per_core_on(listeners, shutdown).await });
    (bound, serve)
}

#[tokio::test]
async fn per_core_serves_unary_on_two_shards() {
    let (service, seen, _gate) = svc();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (bound, serve) = serve_shards(Server::new(service), 2, async move {
        shutdown_rx.await.ok();
    })
    .await;
    let channel = channel(bound).await;
    for i in 0..50 {
        let reply = ping(&channel, &format!("n{i}")).await;
        assert_eq!(name_of(&reply), format!("pong:n{i}"));
    }
    assert_eq!(seen.load(Ordering::Relaxed), 50);
    shutdown_tx.send(()).expect("shutdown");
    serve.await.expect("serve joins").expect("serve ok");
}

#[tokio::test]
async fn per_core_router_serves_unary_on_two_shards() {
    let (service, seen, _gate) = svc();
    let router = Router::new().add_service(service);
    let (bound, listeners) = router.bind_per_core(loopback0(), 2).expect("bind shards");
    assert_ne!(bound.port(), 0, "port 0 must resolve");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let serve = tokio::spawn(async move {
        router
            .serve_per_core_on(listeners, async move {
                shutdown_rx.await.ok();
            })
            .await
    });
    let channel = channel(bound).await;
    for i in 0..50 {
        let reply = ping(&channel, &format!("n{i}")).await;
        assert_eq!(name_of(&reply), format!("pong:n{i}"));
    }
    assert_eq!(seen.load(Ordering::Relaxed), 50);
    shutdown_tx.send(()).expect("shutdown");
    serve.await.expect("serve joins").expect("serve ok");
}

#[tokio::test]
async fn per_core_zero_cores_rejected() {
    let (service, _, _) = svc();
    let err = Server::new(service.clone())
        .serve_per_core(loopback0(), 0, std::future::pending())
        .await
        .expect_err("zero cores");
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = Server::new(service.clone())
        .serve_per_core_on(Vec::new(), std::future::pending())
        .await
        .expect_err("zero shards");
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = Server::new(service)
        .bind_per_core(loopback0(), 0)
        .expect_err("zero bind");
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = Channel::connect_per_core("127.0.0.1:1", 0).expect_err("zero channels");
    assert_eq!(err.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn per_core_drain_waits_for_inflight() {
    let (service, seen, gate) = svc();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (bound, serve) = serve_shards(Server::new(service), 2, async move {
        shutdown_rx.await.ok();
    })
    .await;
    let channel = channel(bound).await;
    // Park two RPCs, then shut down: the drain must wait, not abandon.
    let mut parked = Vec::new();
    for _ in 0..2 {
        let channel = channel.clone();
        parked.push(tokio::spawn(async move {
            channel
                .unary::<HelloRequest, HelloReply>("/test.PerCore/Block", Request::new(req("hold")))
                .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    shutdown_tx.send(()).expect("shutdown");
    // Give the signal a moment to reach the shards, then the serve must
    // still be running: two RPCs are in flight.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!serve.is_finished(), "drain must wait for in-flight RPCs");
    gate.notify_waiters();
    for task in parked {
        let reply = task
            .await
            .expect("rpc joins")
            .expect("parked rpc completes through drain")
            .into_inner();
        assert_eq!(name_of(&reply), "unblocked");
    }
    serve.await.expect("serve joins").expect("serve ok");
    assert_eq!(seen.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn per_core_rpc_limit_exact_on_one_shard() {
    let (service, seen, gate) = svc();
    let (bound, serve) = serve_shards(
        Server::new(service).max_concurrent_rpcs(2),
        1,
        std::future::pending(),
    )
    .await;
    let channel = channel(bound).await;
    let mut parked = Vec::new();
    for _ in 0..2 {
        let channel = channel.clone();
        parked.push(tokio::spawn(async move {
            channel
                .unary::<HelloRequest, HelloReply>("/test.PerCore/Block", Request::new(req("hold")))
                .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Both permits are held: the third RPC is rejected, fast.
    let rejected = tokio::time::timeout(
        Duration::from_secs(5),
        channel
            .unary::<HelloRequest, HelloReply>("/test.PerCore/Block", Request::new(req("extra"))),
    )
    .await
    .expect("rejection must not stall")
    .expect_err("third RPC over the limit");
    assert_eq!(rejected.code(), Code::ResourceExhausted);
    gate.notify_waiters();
    for task in parked {
        let _reply = task
            .await
            .expect("rpc joins")
            .expect("parked rpc completes")
            .into_inner();
    }
    assert_eq!(seen.load(Ordering::Relaxed), 2);
    serve.abort();
}

#[tokio::test]
async fn per_core_rpc_limit_divides_across_shards() {
    // 4 RPCs over 2 shards: each shard admits at most 2, whatever the
    // kernel's spray. 8 RPCs across 4 connections must settle with 2..=4
    // parked and the rest rejected.
    let (service, _, gate) = svc();
    let (bound, serve) = serve_shards(
        Server::new(service).max_concurrent_rpcs(4),
        2,
        std::future::pending(),
    )
    .await;
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let channel = channel(bound).await;
        for _ in 0..2 {
            let channel = channel.clone();
            tasks.push(tokio::spawn(async move {
                channel
                    .unary::<HelloRequest, HelloReply>(
                        "/test.PerCore/Block",
                        Request::new(req("hold")),
                    )
                    .await
            }));
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut parked = 0;
    let mut rejected = 0;
    for task in &tasks {
        if task.is_finished() {
            rejected += 1;
        } else {
            parked += 1;
        }
    }
    assert!(
        (2..=4).contains(&parked),
        "expected 2..=4 parked, got {parked} (spray-dependent)"
    );
    assert_eq!(parked + rejected, 8);
    gate.notify_waiters();
    let mut ok = 0;
    for task in tasks {
        match task.await.expect("rpc joins") {
            Ok(_) => ok += 1,
            Err(status) => assert_eq!(status.code(), Code::ResourceExhausted),
        }
    }
    assert_eq!(ok, parked, "every parked RPC completes after release");
    serve.abort();
}

#[tokio::test]
async fn per_core_connection_limit_is_shared_exact() {
    // The connection budget is one semaphore across shards: exactly 2
    // connections, regardless of spray.
    let (service, _, gate) = svc();
    let (bound, serve) = serve_shards(
        Server::new(service).max_concurrent_connections(2),
        2,
        std::future::pending(),
    )
    .await;
    let first = channel(bound).await;
    let second = channel(bound).await;
    // Hold both connections with parked RPCs.
    let mut parked = Vec::new();
    for channel in [first.clone(), second.clone()] {
        parked.push(tokio::spawn(async move {
            channel
                .unary::<HelloRequest, HelloReply>("/test.PerCore/Block", Request::new(req("hold")))
                .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    // A third connection is refused while the budget is exhausted: the
    // dial must fail fast, not hang and not succeed.
    tokio::time::timeout(Duration::from_secs(5), Channel::connect(bound))
        .await
        .expect("third dial must settle, not hang")
        .expect_err("third connection over the limit");
    gate.notify_waiters();
    for task in parked {
        task.await
            .expect("rpc joins")
            .expect("parked rpc completes");
    }
    // Budget freed: a new connection works, proving the refusal was the
    // limit and nothing stuck.
    drop(first);
    drop(second);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let fresh = channel(bound).await;
    assert_eq!(name_of(&ping(&fresh, "again").await), "pong:again");
    serve.abort();
}

#[tokio::test]
async fn per_core_client_channels_drive_from_own_threads() {
    let (service, seen, _gate) = svc();
    let (bound, serve) = serve_shards(Server::new(service), 2, std::future::pending()).await;
    // Wait for the server before the client threads dial.
    channel(bound).await;
    let channels = Channel::connect_per_core(bound, 2).expect("channels");
    assert_eq!(channels.len(), 2);
    let mut joins = Vec::new();
    for (i, channel) in channels.into_iter().enumerate() {
        joins.push(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("client runtime");
            runtime.block_on(async move {
                for n in 0..20 {
                    let reply: HelloReply = channel
                        .unary(
                            "/test.PerCore/Ping",
                            Request::new(req(&format!("c{i}-{n}"))),
                        )
                        .await
                        .expect("ping")
                        .into_inner();
                    assert_eq!(name_of(&reply), format!("pong:c{i}-{n}"));
                }
            });
        }));
    }
    // Join off the test executor: this test runs current-thread, and a
    // blocking join would freeze the runtime the serve task lives on.
    tokio::task::spawn_blocking(move || {
        for join in joins {
            join.join().expect("client thread joins");
        }
    })
    .await
    .expect("joiner joins");
    assert_eq!(seen.load(Ordering::Relaxed), 40);
    serve.abort();
}

#[tokio::test]
async fn per_core_shards_survive_connect_churn() {
    // Rapid connect/drop churn must not kill shards or wedge the loops.
    let (service, seen, _gate) = svc();
    let (bound, serve) = serve_shards(Server::new(service), 2, std::future::pending()).await;
    for _ in 0..50 {
        let sock = tokio::net::TcpStream::connect(bound).await.expect("dial");
        drop(sock);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!serve.is_finished(), "shards must survive churn");
    let channel = channel(bound).await;
    assert_eq!(name_of(&ping(&channel, "after").await), "pong:after");
    assert_eq!(seen.load(Ordering::Relaxed), 1);
    serve.abort();
}

#[tokio::test]
async fn per_core_shards_do_not_depend_on_the_creator_reactor() {
    // Shard listeners convert on their own threads: freezing the runtime
    // that bound them must not stall accepts. (Converting on the caller
    // pinned every shard's readiness to the creator's reactor.)
    let (service, seen, _gate) = svc();
    let (bound, serve) = serve_shards(Server::new(service), 1, std::future::pending()).await;
    channel(bound).await;
    let channels = Channel::connect_per_core(bound, 1).expect("channels");
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("client runtime");
        runtime.block_on(async move {
            for n in 0..5 {
                let reply: HelloReply = channels[0]
                    .unary("/test.PerCore/Ping", Request::new(req(&format!("f{n}"))))
                    .await
                    .expect("ping during freeze")
                    .into_inner();
                assert_eq!(name_of(&reply), format!("pong:f{n}"));
            }
        });
    });
    // Freeze the test (current-thread) runtime while the client thread
    // dials and calls.
    std::thread::sleep(Duration::from_secs(2));
    assert!(!serve.is_finished(), "serve keeps running");
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || handle.join().expect("client thread joins")),
    )
    .await
    .expect("client thread must finish despite the freeze")
    .expect("joiner joins");
    assert_eq!(seen.load(Ordering::Relaxed), 5);
    serve.abort();
}

#[tokio::test]
async fn per_core_tls_roundtrip() {
    let (service, seen, _gate) = svc();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let server = Server::new(service);
    let (bound, listeners) = server.bind_per_core(loopback0(), 2).expect("bind shards");
    let serve = tokio::spawn(async move {
        server
            .serve_tls_per_core_on(listeners, std::future::pending(), tls)
            .await
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls(bound, client_tls)
        .await
        .expect("tls connect");
    for i in 0..10 {
        let reply = ping(&channel, &format!("t{i}")).await;
        assert_eq!(name_of(&reply), format!("pong:t{i}"));
    }
    assert_eq!(seen.load(Ordering::Relaxed), 10);
    serve.abort();
}
