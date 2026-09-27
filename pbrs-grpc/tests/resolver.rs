//! Resolver targets, refresh timelines, and URI channels (CH-02).
//!
//! Every scheme parses and dials; DNS refresh follows the contracted
//! t0–t7 timelines under a fake provider and paused clock; resolver
//! updates publish without blocking picks.

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

use common::{Echo, ServerGuard, greeter_client, req};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::resolver::{
    BuiltResolver, DnsConfig, DnsLookup, Resolution, ResolvedAddress, ResolverConfig,
    parse_target_uri, resolver_for,
};
use pbrs_grpc::{Channel, ClientTls, Request, ServerTls};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");

fn socket(ip: &str, port: u16) -> SocketAddr {
    format!("{ip}:{port}").parse().expect("addr")
}

/// One scripted lookup outcome.
#[derive(Clone)]
enum Step {
    Addrs(Vec<SocketAddr>),
    Fail,
}

/// Fake DNS: replays `steps`, repeating the last one forever.
struct Script {
    steps: tokio::sync::Mutex<Vec<Step>>,
    calls: AtomicUsize,
}

impl Script {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: tokio::sync::Mutex::new(steps),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl DnsLookup for Script {
    fn lookup(
        &self,
        _host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut steps = self.steps.lock().await;
            let step = if steps.len() > 1 {
                steps.remove(0)
            } else {
                steps.first().cloned().unwrap_or(Step::Fail)
            };
            match step {
                Step::Addrs(addrs) => Ok(addrs),
                Step::Fail => Err(io::Error::other("scripted failure")),
            }
        })
    }
}

fn dns_config() -> DnsConfig {
    DnsConfig::new(
        Duration::from_secs(1),
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_millis(100),
        Duration::from_secs(5),
        Duration::from_secs(60),
    )
    .expect("bounds")
}

fn dns_target() -> pbrs_grpc::resolver::ParsedTarget {
    parse_target_uri("dns:///t0.invalid:443").expect("target")
}

/// Advance paused time until `rx` publishes `want`, or panic.
async fn wait_generation(rx: &mut tokio::sync::watch::Receiver<Arc<Resolution>>, want: u64) {
    for _ in 0..200 {
        if rx.borrow().generation() == want {
            rx.mark_changed();
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!(
        "generation {want} never published (stuck at {})",
        rx.borrow().generation()
    );
}

#[test]
fn every_scheme_parses_and_builds() {
    for uri in [
        "passthrough:///127.0.0.1:1",
        "ipv4:127.0.0.1:1,10.0.0.1:2",
        "ipv6:[::1]:1",
        "unix:/tmp/pbrs-x.sock",
        "unix-abstract:pbrs-x",
    ] {
        parse_target_uri(uri).expect(uri);
    }
}

#[tokio::test]
async fn static_schemes_resolve_without_dns() {
    let config = ResolverConfig::static_only();
    for (uri, want) in [
        (
            "ipv4:127.0.0.1:1,10.0.0.1:2",
            vec!["127.0.0.1:1", "10.0.0.1:2"],
        ),
        ("ipv6:[::1]:1", vec!["[::1]:1"]),
    ] {
        let target = parse_target_uri(uri).expect(uri);
        let built = resolver_for(&target, &config).await.expect(uri);
        let got: Vec<String> = built
            .initial
            .addresses()
            .iter()
            .map(|a| match a {
                ResolvedAddress::Tcp(sock) => sock.to_string(),
                _ => panic!("tcp expected"),
            })
            .collect();
        assert_eq!(got, want);
    }
    let target = parse_target_uri("unix:/tmp/pbrs-x.sock").expect("unix");
    let built = resolver_for(&target, &config).await.expect("unix");
    assert!(matches!(
        built.initial.addresses(),
        [ResolvedAddress::Unix(_)]
    ));
}

#[tokio::test]
async fn dns_literal_needs_no_lookup_and_no_bounds() {
    let script = Arc::new(Script::new(vec![Step::Fail]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script.clone());
    // Even with failing DNS, a literal resolves statically.
    let target = parse_target_uri("dns:///127.0.0.1:443").expect("target");
    let built = resolver_for(&target, &config).await.expect("literal");
    assert_eq!(
        built.initial.addresses(),
        &[ResolvedAddress::Tcp(socket("127.0.0.1", 443))]
    );
    assert_eq!(script.calls(), 0);
    // ... and without any DNS bounds at all.
    let target = parse_target_uri("dns:///127.0.0.1:443").expect("target");
    let built = resolver_for(&target, &ResolverConfig::static_only())
        .await
        .expect("literal without bounds");
    assert!(!built.initial.is_empty());
}

#[tokio::test]
async fn dns_hostname_needs_bounds() {
    let err = resolver_for(&dns_target(), &ResolverConfig::static_only())
        .await
        .expect_err("dns without bounds is rejected");
    assert!(err.message().contains("DnsConfig"));
}

#[test]
fn invalid_targets_fail_fast() {
    for uri in [
        "example.com:443",
        "grpc://example.com:443",
        "xds:///cluster",
        "dns:///example.com",
        "ipv4:not-an-ip",
        "ipv6:127.0.0.1:80",
        "unix:",
    ] {
        assert!(parse_target_uri(uri).is_err(), "{uri}");
    }
}

/// t0: A (IPv4) + B (IPv6) arrive in resolver order.
#[tokio::test(start_paused = true)]
async fn timeline_initial_snapshot_keeps_order() {
    let script = Arc::new(Script::new(vec![Step::Addrs(vec![
        socket("10.0.0.1", 443),
        socket("[2001:db8::1]", 443),
    ])]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script);
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    assert_eq!(
        built.initial.addresses(),
        &[
            ResolvedAddress::Tcp(socket("10.0.0.1", 443)),
            ResolvedAddress::Tcp(socket("[2001:db8::1]", 443)),
        ]
    );
    assert_eq!(built.initial.generation(), 0);
}

/// t1: a repeated identical answer (even with duplicates) republishes nothing.
#[tokio::test(start_paused = true)]
async fn timeline_identical_answer_keeps_generation() {
    let addrs = vec![socket("10.0.0.1", 443), socket("10.0.0.1", 443)];
    let script = Arc::new(Script::new(vec![
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
        Step::Addrs(addrs),
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
    ]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script);
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    assert_eq!(built.initial.addresses().len(), 1);
    let rx = built.watch.clone();
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert_eq!(rx.borrow().generation(), 0);
    assert!(!rx.has_changed().expect("watch"));
}

/// t2+t3: failures serve stale inside the budget, then publish empty.
#[tokio::test(start_paused = true)]
async fn timeline_failure_stale_then_empty() {
    let script = Arc::new(Script::new(vec![
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
        Step::Fail,
    ]));
    let config = ResolverConfig::with_dns_provider(
        DnsConfig::new(
            Duration::from_secs(1),
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_millis(100),
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .expect("bounds"),
        script,
    );
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    let mut rx = built.watch.clone();
    // t2: 8s in, still inside valid_until(5s) + max_stale(10s): stale served.
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(rx.borrow().generation(), 0);
    assert!(!rx.borrow().is_empty());
    // t3: past 15s: authoritative empty at a new generation.
    wait_generation(&mut rx, 1).await;
    assert!(rx.borrow().is_empty());
}

/// t4: a successful empty answer stops new picks immediately.
#[tokio::test(start_paused = true)]
async fn timeline_empty_answer_is_authoritative() {
    let script = Arc::new(Script::new(vec![
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
        Step::Addrs(vec![]),
    ]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script);
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    let mut rx = built.watch.clone();
    wait_generation(&mut rx, 1).await;
    assert!(rx.borrow().is_empty());
}

/// t5: B returns while A is gone; a changed port is a new endpoint.
#[tokio::test(start_paused = true)]
async fn timeline_updates_replace_and_reorder() {
    let script = Arc::new(Script::new(vec![
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
        Step::Addrs(vec![socket("10.0.0.2", 443), socket("10.0.0.1", 444)]),
    ]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script);
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    let mut rx = built.watch.clone();
    wait_generation(&mut rx, 1).await;
    assert_eq!(
        rx.borrow().addresses(),
        &[
            ResolvedAddress::Tcp(socket("10.0.0.2", 443)),
            ResolvedAddress::Tcp(socket("10.0.0.1", 444)),
        ]
    );
}

/// t6+t7: dropping the resolver stops lookups; nothing lingers.
#[tokio::test(start_paused = true)]
async fn timeline_drop_cancels_refresh() {
    let script = Arc::new(Script::new(vec![Step::Addrs(vec![socket(
        "10.0.0.1", 443,
    )])]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script.clone());
    let built: BuiltResolver = resolver_for(&dns_target(), &config).await.expect("dns");
    assert_eq!(script.calls(), 1);
    drop(built);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(script.calls(), 1);
}

/// Picks borrow the current snapshot without awaiting while updates flow.
#[tokio::test(start_paused = true)]
async fn updates_do_not_block_picks() {
    let script = Arc::new(Script::new(vec![
        Step::Addrs(vec![socket("10.0.0.1", 443)]),
        Step::Addrs(vec![socket("10.0.0.2", 443)]),
        Step::Addrs(vec![socket("10.0.0.3", 443)]),
        Step::Addrs(vec![socket("10.0.0.4", 443)]),
    ]));
    let config = ResolverConfig::with_dns_provider(dns_config(), script);
    let built = resolver_for(&dns_target(), &config).await.expect("dns");
    let rx = built.watch.clone();
    let picks = tokio::spawn(async move {
        let mut n = 0u64;
        for _ in 0..50_000 {
            let snapshot = rx.borrow().clone();
            assert!(!snapshot.addresses().is_empty());
            n += 1;
            if n % 10_000 == 0 {
                tokio::task::yield_now().await;
            }
        }
        n
    });
    let mut rx = built.watch.clone();
    wait_generation(&mut rx, 3).await;
    assert_eq!(picks.await.expect("picks"), 50_000);
    assert_eq!(rx.borrow().generation(), 3);
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

async fn say_hello(channel: Channel) -> String {
    GreeterClient::new(channel)
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary")
        .into_inner()
        .message()
        .to_string()
}

#[tokio::test]
async fn uri_channels_dial_each_scheme() {
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let _guard = ServerGuard(handle);
    let port = addr.port();

    for uri in [
        format!("dns:///127.0.0.1:{port}"),
        format!("passthrough:///127.0.0.1:{port}"),
        format!("ipv4:127.0.0.1:{port}"),
    ] {
        let channel = Channel::connect_uri(&uri, ResolverConfig::static_only())
            .await
            .expect(&uri);
        assert_eq!(say_hello(channel).await, "ada");
    }
    // dns: with explicit bounds also dials (and would refresh behind it).
    let channel = Channel::connect_uri(
        &format!("dns:///127.0.0.1:{port}"),
        ResolverConfig::with_dns(dns_config()),
    )
    .await
    .expect("dns with bounds");
    assert_eq!(say_hello(channel).await, "ada");

    // Plain host:port is not a URI.
    let err = Channel::connect_uri("127.0.0.1:1", ResolverConfig::static_only())
        .await
        .expect_err("host:port rejected");
    assert!(err.message().contains("host:port"));
    // ... and still works on the classic path, exactly as before.
    let classic = greeter_client(addr).await;
    assert_eq!(
        classic
            .say_hello(Request::new(req("ada")))
            .await
            .expect("classic")
            .into_inner()
            .message(),
        "ada"
    );
}

#[tokio::test]
async fn tls_uri_verifies_the_configured_name() {
    let tls =
        ServerTls::new(pbrs_grpc::Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity"))
            .expect("server tls");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    // The dial address is an IP; verification still uses "localhost".
    let channel = Channel::connect_tls_uri(
        &format!("dns:///127.0.0.1:{}", addr.port()),
        ClientTls::ca("localhost", CA).expect("client tls"),
        ResolverConfig::static_only(),
    )
    .await
    .expect("tls uri");
    assert_eq!(say_hello(channel).await, "ada");
}

#[cfg(unix)]
#[tokio::test]
async fn unix_uri_dials_a_socket_path() {
    let path = std::env::temp_dir().join(format!("pbrs-resolver-{}.sock", std::process::id()));
    std::fs::remove_file(&path).ok();
    let socket = std::os::unix::net::UnixListener::bind(&path).expect("bind");
    socket.set_nonblocking(true).expect("nonblocking");
    let listener = tokio::net::UnixListener::from_std(socket).expect("tokio");
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let channel = Channel::connect_uri(
        &format!("unix:{}", path.display()),
        ResolverConfig::static_only(),
    )
    .await
    .expect("unix uri");
    assert_eq!(say_hello(channel).await, "ada");
    // TLS over Unix is rejected up front, not on first RPC.
    let err = Channel::connect_tls_uri(
        &format!("unix:{}", path.display()),
        ClientTls::ca("localhost", CA).expect("client tls"),
        ResolverConfig::static_only(),
    )
    .await
    .expect_err("tls over unix rejected");
    assert!(err.message().contains("Unix socket"));
    std::fs::remove_file(&path).ok();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn unix_abstract_uri_dials_on_linux() {
    let name = format!("pbrs-resolver-{}", std::process::id());
    let addr = tokio::net::unix::SocketAddr::from_abstract_name(&name).expect("abstract");
    let listener = tokio::net::UnixListener::bind_addr(&addr).expect("bind");
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let channel = Channel::connect_uri(
        &format!("unix-abstract:{name}"),
        ResolverConfig::static_only(),
    )
    .await
    .expect("abstract uri");
    assert_eq!(say_hello(channel).await, "ada");
}
