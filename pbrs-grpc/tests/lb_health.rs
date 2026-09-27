//! Client-side health checking gates LB readiness (A17, CH-05).
//!
//! Backends serve Greeter plus a controllable health service; a TXT
//! service config opts the channel into `healthCheckConfig` with
//! `round_robin` (or the default `pick_first`). Unhealthy backends
//! leave rotation within one Watch update and rejoin on recovery;
//! backends without a health service stay healthy (UNIMPLEMENTED);
//! the channel switch disables gating when set false.

#![allow(
    clippy::disallowed_methods,
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

use common::{ServerGuard, reply, req};
use pbrs_grpc::health::{HealthReporter, service};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::resolver::{DnsConfig, DnsLookup, ResolverConfig, TxtLookup};
use pbrs_grpc::{Channel, ChannelConfig, Request, Response, Router, Status};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

struct Tagged {
    tag: &'static str,
    unaries: Arc<AtomicUsize>,
}

impl Greeter for Tagged {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.unaries.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(reply(format!(
            "{}:{}",
            self.tag,
            request.get_ref().name()
        ))))
    }
}

/// A backend serving Greeter plus a controllable health service.
async fn serve_healthy(
    tag: &'static str,
) -> (SocketAddr, Arc<AtomicUsize>, HealthReporter, ServerGuard) {
    let (svc, reporter) = service();
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Tagged {
                tag,
                unaries: worker,
            }))
            .add_service(svc)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, reporter, ServerGuard(handle))
}

/// A backend serving Greeter with no health service at all.
async fn serve_no_health(tag: &'static str) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Tagged {
                tag,
                unaries: worker,
            }))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

/// One fixed DNS answer, repeated on every refresh.
struct FixedAddrs(Vec<SocketAddr>);

impl DnsLookup for FixedAddrs {
    fn lookup(
        &self,
        _host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        let addrs = self.0.clone();
        Box::pin(async move { Ok(addrs) })
    }
}

/// One TXT document, served on every fetch.
struct FixedTxt(&'static str);

impl TxtLookup for FixedTxt {
    fn fetch_txt(
        &self,
        _name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, io::Error>> + Send + '_>> {
        let doc = self.0.to_owned();
        Box::pin(async move { Ok(vec![doc]) })
    }
}

fn fast_bounds() -> DnsConfig {
    DnsConfig::new(
        Duration::from_millis(50),
        Duration::from_secs(1),
        Duration::from_millis(100),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Duration::from_secs(1),
    )
    .expect("bounds")
}

const RR_HEALTH_DOC: &str =
    r#"{"loadBalancingConfig": [{"round_robin": {}}], "healthCheckConfig": {}}"#;
const PF_HEALTH_DOC: &str = r#"{"healthCheckConfig": {}}"#;

async fn health_channel(addrs: Vec<SocketAddr>, doc: &'static str, host: &str) -> Channel {
    let dns = Arc::new(FixedAddrs(addrs));
    let txt = Arc::new(FixedTxt(doc));
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns).with_txt_provider(txt);
    Channel::connect_uri(&format!("dns:///{host}:443"), config)
        .await
        .expect("channel")
}

async fn unary_tag(client: &GreeterClient) -> String {
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary")
        .into_inner()
        .message()
        .to_string()
}

/// Loop RPCs until `want` consecutive tags equal `tag`, or time out.
async fn until_consecutive(client: &GreeterClient, tag: &str, want: usize, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut consecutive = 0;
    while consecutive < want {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what}: still flipping after 10s"
        );
        if unary_tag(client).await == tag {
            consecutive += 1;
        } else {
            consecutive = 0;
        }
    }
}

#[tokio::test]
async fn unhealthy_backend_leaves_rotation_within_one_update() {
    let (addr_a, _unaries_a, _reporter_a, _guard_a) = serve_healthy("A").await;
    let (addr_b, unaries_b, reporter_b, _guard_b) = serve_healthy("B").await;
    let channel = health_channel(vec![addr_a, addr_b], RR_HEALTH_DOC, "hc.invalid").await;
    let client = GreeterClient::new(channel);

    // Both serve in rotation while healthy.
    let mut tags = Vec::new();
    for _ in 0..4 {
        tags.push(unary_tag(&client).await);
    }
    assert_eq!(tags, vec!["A:ada", "B:ada", "A:ada", "B:ada"]);

    // Flip B unhealthy: one streamed Watch update converges rotation
    // to A-only — far under any retry/backoff timescale — then B's
    // counter freezes: no stale routes after the update applies.
    reporter_b.set_not_serving("");
    let start = tokio::time::Instant::now();
    until_consecutive(&client, "A:ada", 6, "B leaves rotation").await;
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "converged in {:?}, want one update",
        start.elapsed()
    );
    let frozen_b = unaries_b.load(Ordering::SeqCst);
    for _ in 0..20 {
        assert_eq!(unary_tag(&client).await, "A:ada");
    }
    assert_eq!(unaries_b.load(Ordering::SeqCst), frozen_b);
}

#[tokio::test]
async fn recovered_backend_rejoins_rotation() {
    let (addr_a, _unaries_a, _reporter_a, _guard_a) = serve_healthy("A").await;
    let (addr_b, _unaries_b, reporter_b, _guard_b) = serve_healthy("B").await;
    let channel = health_channel(vec![addr_a, addr_b], RR_HEALTH_DOC, "hc.invalid").await;
    let client = GreeterClient::new(channel);

    reporter_b.set_not_serving("");
    until_consecutive(&client, "A:ada", 6, "B leaves rotation").await;

    // Recovery rejoins within one update: B serves again promptly.
    reporter_b.set_serving("");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut rejoined = false;
    while tokio::time::Instant::now() < deadline {
        if unary_tag(&client).await == "B:ada" {
            rejoined = true;
            break;
        }
    }
    assert!(rejoined, "B rejoins rotation on recovery");
}

#[tokio::test]
async fn backend_without_health_service_stays_healthy() {
    let (addr_a, unaries_a, _guard_a) = serve_no_health("A").await;
    let (addr_b, unaries_b, _reporter_b, _guard_b) = serve_healthy("B").await;
    let channel = health_channel(vec![addr_a, addr_b], RR_HEALTH_DOC, "hc.invalid").await;
    let client = GreeterClient::new(channel);

    // UNIMPLEMENTED Watch disables checking per backend: A keeps its
    // rotation share instead of gating out.
    for _ in 0..20 {
        unary_tag(&client).await;
    }
    assert_eq!(unaries_a.load(Ordering::SeqCst), 10);
    assert_eq!(unaries_b.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn pick_first_unhealthy_fails_over_and_sticks() {
    let (addr_a, _unaries_a, reporter_a, _guard_a) = serve_healthy("A").await;
    let (addr_b, unaries_b, _reporter_b, _guard_b) = serve_healthy("B").await;
    let channel = health_channel(vec![addr_a, addr_b], PF_HEALTH_DOC, "hc.invalid").await;
    let client = GreeterClient::new(channel);

    // Sticks to A while healthy.
    for _ in 0..3 {
        assert_eq!(unary_tag(&client).await, "A:ada");
    }
    // A unhealthy fails over to B like a failed dial.
    reporter_a.set_not_serving("");
    until_consecutive(&client, "B:ada", 6, "fail over to B").await;
    // Sticks to B: no flap-back even after A recovers.
    reporter_a.set_serving("");
    for _ in 0..6 {
        assert_eq!(unary_tag(&client).await, "B:ada");
    }
    assert!(unaries_b.load(Ordering::SeqCst) >= 12);
}

#[tokio::test]
async fn health_switch_off_disables_gating() {
    let (addr_a, _unaries_a, _reporter_a, _guard_a) = serve_healthy("A").await;
    let (addr_b, unaries_b, reporter_b, _guard_b) = serve_healthy("B").await;
    let dns = Arc::new(FixedAddrs(vec![addr_a, addr_b]));
    let txt = Arc::new(FixedTxt(RR_HEALTH_DOC));
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns).with_txt_provider(txt);
    let channel = Channel::connect_uri_with(
        "dns:///hc.invalid:443",
        ChannelConfig::new().health_checking(false),
        config,
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel);

    // The document opts in but the switch is off: B keeps serving
    // despite reporting NOT_SERVING.
    reporter_b.set_not_serving("");
    for _ in 0..20 {
        unary_tag(&client).await;
    }
    assert_eq!(unaries_b.load(Ordering::SeqCst), 10);
}
