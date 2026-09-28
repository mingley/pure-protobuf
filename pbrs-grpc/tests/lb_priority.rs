//! priority failover and outlier_detection end to end (CH-08).
//!
//! Priority serving through the highest priority, per-address
//! failover inside it, hash delegation through a ring child, and
//! outlier ejection/unejection of a failing backend driven by real
//! call outcomes. Timer-driven priority behavior (failover hold,
//! deactivation destroy) and detector edge cases are pinned by the
//! fake-clock unit tests in `lb::priority` and `lb::outlier`, which
//! can advance the 10s/15m A56 timelines exactly.

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
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::resolver::{DnsConfig, DnsLookup, ResolverConfig, TxtLookup};
use pbrs_grpc::{Channel, Request, Response, Router, Status};
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

/// A backend that counts RPCs and fails every one (outlier food).
struct Fail {
    tag: &'static str,
    unaries: Arc<AtomicUsize>,
}

impl Greeter for Fail {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.unaries.fetch_add(1, Ordering::SeqCst);
        Err(Status::internal(format!("{} always fails", self.tag)))
    }
}

async fn serve(tag: &'static str) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
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

async fn serve_fail(tag: &'static str) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Fail {
                tag,
                unaries: worker,
            }))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

/// A loopback port nothing listens on (bound, read, dropped).
async fn closed_port() -> SocketAddr {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    drop(listener);
    addr
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

async fn lb_channel(addrs: Vec<SocketAddr>, doc: &'static str, host: &str) -> Channel {
    let dns = Arc::new(FixedAddrs(addrs));
    let txt = Arc::new(FixedTxt(doc));
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns).with_txt_provider(txt);
    Channel::connect_uri(&format!("dns:///{host}:443"), config)
        .await
        .expect("channel")
}

const PRIORITY_RR_DOC: &str = r#"{"loadBalancingConfig": [{"priority": {
    "children": {
        "p0": {"config": [{"round_robin": {}}]},
        "p1": {"config": [{"round_robin": {}}]}},
    "priorities": ["p0", "p1"]}}]}"#;
const PRIORITY_RING_DOC: &str = r#"{"loadBalancingConfig": [{"priority": {
    "children": {
        "p0": {"config": [{"ring_hash": {"requestHashHeader": "x-session"}}]},
        "p1": {"config": [{"round_robin": {}}]}},
    "priorities": ["p0", "p1"]}}]}"#;
/// 1s sweeps, 30s base, 50% cap: ejects a 100%-failing backend
/// after one interval and holds it out for the whole test.
const OUTLIER_DOC: &str = r#"{"loadBalancingConfig": [{"outlier_detection": {
    "interval": "1s",
    "baseEjectionTime": "30s",
    "maxEjectionPercent": 50,
    "failurePercentageEjection": {
        "threshold": 50,
        "enforcementPercentage": 100,
        "minimumHosts": 2,
        "requestVolume": 5},
    "childPolicy": [{"round_robin": {}}]}}]}"#;
/// Same detectors with a 2s base: the backend rejoins quickly.
const OUTLIER_SHORT_DOC: &str = r#"{"loadBalancingConfig": [{"outlier_detection": {
    "interval": "1s",
    "baseEjectionTime": "2s",
    "maxEjectionPercent": 50,
    "failurePercentageEjection": {
        "threshold": 50,
        "enforcementPercentage": 100,
        "minimumHosts": 2,
        "requestVolume": 5},
    "childPolicy": [{"round_robin": {}}]}}]}"#;

fn session_req(session: &str) -> Request<HelloRequest> {
    let mut request = Request::new(req("ada"));
    request
        .metadata_mut()
        .set("x-session", session)
        .expect("header");
    request
}

async fn unary_tag(client: &GreeterClient, request: Request<HelloRequest>) -> String {
    client
        .say_hello(request)
        .await
        .expect("unary")
        .into_inner()
        .message()
        .to_string()
}

#[tokio::test]
async fn priority_serves_through_highest_priority() {
    let (a, ua, _ga) = serve("A").await;
    let (b, ub, _gb) = serve("B").await;
    // Flat resolver update: both addresses feed p0, whose
    // round_robin child splits them evenly.
    let channel = lb_channel(vec![a, b], PRIORITY_RR_DOC, "prio.invalid").await;
    let client = GreeterClient::new(channel);
    for _ in 0..20 {
        unary_tag(&client, Request::new(req("ada"))).await;
    }
    assert_eq!(ua.load(Ordering::SeqCst), 10);
    assert_eq!(ub.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn priority_skips_dead_address_in_p0() {
    let dead = closed_port().await;
    let (a, ua, _ga) = serve("A").await;
    let channel = lb_channel(vec![dead, a], PRIORITY_RR_DOC, "priodead.invalid").await;
    let client = GreeterClient::new(channel);
    let mut failures = 0;
    for _ in 0..10 {
        match client.say_hello(Request::new(req("ada"))).await {
            Ok(_) => {}
            Err(status) => {
                assert_eq!(status.code(), pbrs_grpc::Code::Unavailable);
                failures += 1;
            }
        }
    }
    // The refused dial fails at most the first RPCs; the survivor
    // backs off the dead address and serves the rest.
    assert!(failures <= 2, "too many failures: {failures}/10");
    assert_eq!(ua.load(Ordering::SeqCst), 10 - failures);
}

#[tokio::test]
async fn priority_over_ring_keeps_affinity() {
    let (a, _ua, _ga) = serve("A").await;
    let (b, _ub, _gb) = serve("B").await;
    let (c, _uc, _gc) = serve("C").await;
    let channel = lb_channel(vec![a, b, c], PRIORITY_RING_DOC, "priorring.invalid").await;
    let client = GreeterClient::new(channel);
    // Hash derivation and the hash itself both delegate through
    // priority to its ring child: the session sticks.
    let first = unary_tag(&client, session_req("affinity-9")).await;
    for _ in 0..10 {
        assert_eq!(unary_tag(&client, session_req("affinity-9")).await, first);
    }
}

#[tokio::test]
async fn outlier_ejects_failing_backend() {
    let (a, ua, _ga) = serve_fail("A").await;
    let (b, ub, _gb) = serve("B").await;
    let (c, uc, _gc) = serve("C").await;
    let channel = lb_channel(vec![a, b, c], OUTLIER_DOC, "outlier.invalid").await;
    let client = GreeterClient::new(channel);
    // Warmup: strict rotation deals 5 RPCs to each backend; the 5
    // failures on A meet the request volume.
    let mut failed = 0;
    for _ in 0..15 {
        if client.say_hello(Request::new(req("ada"))).await.is_err() {
            failed += 1;
        }
    }
    assert_eq!(failed, 5, "warmup must fail exactly on A");
    assert_eq!(ua.load(Ordering::SeqCst), 5);
    // Past the 1s sweep interval the next pick ejects A for 30s.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for _ in 0..30 {
        unary_tag(&client, Request::new(req("ada"))).await;
    }
    assert_eq!(
        ua.load(Ordering::SeqCst),
        5,
        "ejected backend must serve nothing after the sweep"
    );
    assert_eq!(ub.load(Ordering::SeqCst), 20);
    assert_eq!(uc.load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn outlier_lets_unejected_backend_serve_again() {
    let (a, ua, _ga) = serve_fail("A").await;
    let (b, _ub, _gb) = serve("B").await;
    let (c, _uc, _gc) = serve("C").await;
    let channel = lb_channel(vec![a, b, c], OUTLIER_SHORT_DOC, "outlierrej.invalid").await;
    let client = GreeterClient::new(channel);
    for _ in 0..15 {
        let _ = client.say_hello(Request::new(req("ada"))).await;
    }
    assert_eq!(ua.load(Ordering::SeqCst), 5);
    // Eject (1s interval), then outlive the 2s base ejection.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for _ in 0..6 {
        unary_tag(&client, Request::new(req("ada"))).await;
    }
    assert_eq!(ua.load(Ordering::SeqCst), 5, "A ejects first");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    for _ in 0..6 {
        let _ = client.say_hello(Request::new(req("ada"))).await;
    }
    assert_eq!(
        ua.load(Ordering::SeqCst),
        7,
        "unejected backend rejoins rotation"
    );
}
