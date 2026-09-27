//! ring_hash, least_request, and random_subsetting end to end (CH-07).
//!
//! Ring affinity/spread/failover over header-hashed picks, least-request
//! preference for idle backends under concurrent load, and subset
//! membership stability with child delegation (round_robin and ring_hash
//! children).

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
    /// Extra latency for requests named "slow" (least-request load).
    slow_ms: u64,
}

impl Greeter for Tagged {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.unaries.fetch_add(1, Ordering::SeqCst);
        if request.get_ref().name() == "slow" && self.slow_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.slow_ms)).await;
        }
        Ok(Response::new(reply(format!(
            "{}:{}",
            self.tag,
            request.get_ref().name()
        ))))
    }
}

async fn serve(tag: &'static str, slow_ms: u64) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
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
                slow_ms,
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

async fn lb_channel(addrs: Vec<SocketAddr>, doc: &'static str, host: &str) -> Channel {
    let dns = Arc::new(FixedAddrs(addrs));
    let txt = Arc::new(FixedTxt(doc));
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns).with_txt_provider(txt);
    Channel::connect_uri(&format!("dns:///{host}:443"), config)
        .await
        .expect("channel")
}

const RING_DOC: &str = r#"{"loadBalancingConfig": [{"ring_hash": {
    "minRingSize": 100, "maxRingSize": 1000,
    "requestHashHeader": "x-session"}}]}"#;
const RING_NO_HASH_DOC: &str = r#"{"loadBalancingConfig": [{"ring_hash": {"minRingSize": 100}}]}"#;
const LR_DOC: &str = r#"{"loadBalancingConfig": [{"least_request": {"choiceCount": 2}}]}"#;
const SUBSET_RR_DOC: &str = r#"{"loadBalancingConfig": [{"random_subsetting_experimental": {
    "subsetSize": 2, "childPolicy": [{"round_robin": {}}]}}]}"#;
const SUBSET_RING_DOC: &str = r#"{"loadBalancingConfig": [{"random_subsetting_experimental": {
    "subsetSize": 2, "childPolicy": [{"ring_hash": {"requestHashHeader": "x-session"}}]}}]}"#;

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
async fn same_session_sticks_same_backend() {
    let (a, _ua, _ga) = serve("A", 0).await;
    let (b, _ub, _gb) = serve("B", 0).await;
    let (c, _uc, _gc) = serve("C", 0).await;
    let channel = lb_channel(vec![a, b, c], RING_DOC, "ring.invalid").await;
    let client = GreeterClient::new(channel);
    let first = unary_tag(&client, session_req("session-42")).await;
    for _ in 0..20 {
        assert_eq!(unary_tag(&client, session_req("session-42")).await, first);
    }
}

#[tokio::test]
async fn sessions_spread_across_backends() {
    let (a, _ua, _ga) = serve("A", 0).await;
    let (b, _ub, _gb) = serve("B", 0).await;
    let (c, _uc, _gc) = serve("C", 0).await;
    let channel = lb_channel(vec![a, b, c], RING_DOC, "spread.invalid").await;
    let client = GreeterClient::new(channel);
    let mut hits = [0u32; 3];
    for i in 0..90 {
        let tag = unary_tag(&client, session_req(&format!("s-{i}"))).await;
        match tag.chars().next() {
            Some('A') => hits[0] += 1,
            Some('B') => hits[1] += 1,
            Some('C') => hits[2] += 1,
            other => panic!("unexpected tag {tag} ({other:?})"),
        }
    }
    // Smoke band, not an evenness proof: ring shares vary run to run
    // (ephemeral ports place the entries), so a tight band flakes ~7%.
    // Exact distribution is pinned deterministically by the
    // `hashes_spread_across_backends` unit test over fixed addresses.
    for (i, h) in hits.iter().enumerate() {
        assert!((5..=80).contains(h), "backend {i}: {h}/90");
    }
}

#[tokio::test]
async fn missing_header_scatters_without_failing() {
    let (a, _ua, _ga) = serve("A", 0).await;
    let (b, _ub, _gb) = serve("B", 0).await;
    let (c, _uc, _gc) = serve("C", 0).await;
    let channel = lb_channel(vec![a, b, c], RING_DOC, "nohdr.invalid").await;
    let client = GreeterClient::new(channel);
    // No x-session header: random hashes, every RPC still succeeds.
    let mut seen = [false; 3];
    for _ in 0..60 {
        let tag = unary_tag(&client, Request::new(req("ada"))).await;
        match tag.chars().next() {
            Some('A') => seen[0] = true,
            Some('B') => seen[1] = true,
            Some('C') => seen[2] = true,
            other => panic!("unexpected tag {tag} ({other:?})"),
        }
    }
    assert!(
        seen.iter().filter(|s| **s).count() >= 2,
        "random hashes should reach 2+ backends over 60 RPCs"
    );
}

#[tokio::test]
async fn ring_without_hash_source_fails_fast() {
    let (a, _ua, _ga) = serve("A", 0).await;
    let channel = lb_channel(vec![a], RING_NO_HASH_DOC, "nohash.invalid").await;
    let client = GreeterClient::new(channel);
    let err = client
        .say_hello(session_req("s-1"))
        .await
        .expect_err("pick without a hash source must fail");
    assert_eq!(err.code(), pbrs_grpc::Code::Unavailable);
}

#[tokio::test]
async fn least_request_prefers_idle_backend() {
    let (a, _ua, _ga) = serve("A", 2000).await;
    let (b, _ub, _gb) = serve("B", 2000).await;
    let channel = lb_channel(vec![a, b], LR_DOC, "lr.invalid").await;
    let client = Arc::new(GreeterClient::new(channel));
    // Pin one backend with a slow in-flight RPC, then run fast RPCs:
    // with choice_count 2 they avoid the busy backend unless both
    // samples draw it (p=1/4 each).
    let slow_client = Arc::clone(&client);
    let slow = tokio::spawn(async move {
        slow_client
            .say_hello(Request::new(req("slow")))
            .await
            .expect("slow rpc")
    });
    // Let the slow RPC start and register in-flight.
    tokio::time::sleep(Duration::from_millis(300)).await;
    // n=100 at a 60% bar: a working policy (p=3/4 per pick) clears it
    // with P(fail) ~ 2e-4, while dead tracking (uniform p=1/2) fails
    // ~97% of runs. n=20 flaked ~4% with the same product behavior.
    let mut fast_tags = Vec::new();
    for _ in 0..100 {
        let tag = unary_tag(&client, Request::new(req("ada"))).await;
        fast_tags.push(tag.chars().next().expect("tag"));
    }
    let slow_tag = slow.await.expect("join").into_inner().message().to_string();
    let busy = slow_tag.chars().next().expect("slow tag");
    let avoided = fast_tags.iter().filter(|t| **t != busy).count();
    assert!(
        avoided >= 60,
        "fast RPCs should mostly avoid busy {busy}: avoided {avoided}/100"
    );
}

#[tokio::test]
async fn subset_sticks_to_two_backends() {
    let mut addrs = Vec::new();
    let mut guards = Vec::new();
    let mut counters = Vec::new();
    for tag in ["A", "B", "C", "D", "E", "F"] {
        let (addr, unaries, guard) = serve(tag, 0).await;
        addrs.push(addr);
        counters.push(unaries);
        guards.push(guard);
    }
    let channel = lb_channel(addrs, SUBSET_RR_DOC, "subset.invalid").await;
    let client = GreeterClient::new(channel);
    for _ in 0..40 {
        unary_tag(&client, Request::new(req("ada"))).await;
    }
    let hit: Vec<usize> = counters.iter().map(|c| c.load(Ordering::SeqCst)).collect();
    let used = hit.iter().filter(|h| **h > 0).count();
    assert_eq!(used, 2, "subset of 6 must use exactly 2: {hit:?}");
    // Round-robin child splits the subset evenly.
    for h in hit.iter().filter(|h| **h > 0) {
        assert_eq!(*h, 20, "even split: {hit:?}");
    }
    drop(guards);
}

#[tokio::test]
async fn subset_over_ring_keeps_affinity() {
    let mut addrs = Vec::new();
    let mut guards = Vec::new();
    for tag in ["A", "B", "C", "D"] {
        let (addr, _u, guard) = serve(tag, 0).await;
        addrs.push(addr);
        guards.push(guard);
    }
    let channel = lb_channel(addrs, SUBSET_RING_DOC, "subsetring.invalid").await;
    let client = GreeterClient::new(channel);
    // Hash delegation through the subset reaches a ring child: the
    // same session sticks inside the subset.
    let first = unary_tag(&client, session_req("affinity-7")).await;
    for _ in 0..10 {
        assert_eq!(unary_tag(&client, session_req("affinity-7")).await, first);
    }
    drop(guards);
}
