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

use common::{Echo, ServerGuard, greeter_client, reply, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::lb::PickFirst;
use pbrs_grpc::resolver::{
    BuiltResolver, DnsConfig, DnsLookup, Resolution, ResolvedAddress, ResolverConfig, TxtLookup,
    parse_target_uri, resolver_for,
};
use pbrs_grpc::{
    Channel, ChannelConfig, ClientTls, Code, Endpoint, Request, Response, ServerTls, Status,
    Streaming,
};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

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
    use std::os::unix::ffi::OsStrExt as _;

    // tokio has no abstract-namespace constructor; socket2 builds one
    // from a leading-NUL path instead.
    let name = format!("pbrs-resolver-{}", std::process::id());
    let mut raw = Vec::with_capacity(name.len() + 1);
    raw.push(0u8);
    raw.extend_from_slice(name.as_bytes());
    let sock = socket2::SockAddr::unix(std::path::Path::new(std::ffi::OsStr::from_bytes(&raw)))
        .expect("abstract sockaddr");
    let socket =
        socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).expect("socket");
    socket.bind(&sock).expect("bind");
    socket.listen(128).expect("listen");
    let std_listener: std::os::unix::net::UnixListener = socket.into();
    std_listener.set_nonblocking(true).expect("nonblocking");
    let listener = tokio::net::UnixListener::from_std(std_listener).expect("from_std");
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

/// Named Greeter: replies carry the server tag, unaries counted.
#[derive(Clone)]
struct Named {
    tag: &'static str,
    unaries: Arc<AtomicUsize>,
}

impl Greeter for Named {
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

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        // Slow second item: still on the wire when a handoff lands.
        let (tx, stream) = Streaming::channel(4);
        let name = request.get_ref().name().to_string();
        let tag = self.tag;
        drop(tokio::spawn(async move {
            let _ = tx.send(reply(format!("{tag}:1:{name}"))).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _ = tx.send(reply(format!("{tag}:2:{name}"))).await;
        }));
        Ok(Response::new(stream))
    }
}

async fn serve_named(tag: &'static str) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Named {
            tag,
            unaries: worker,
        })
        .serve_listener(listener)
        .await
        .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

async fn unary_tag_is(client: &GreeterClient, want: &str) {
    assert_eq!(
        client
            .say_hello(Request::new(req("ada")))
            .await
            .expect("unary")
            .into_inner()
            .message(),
        want
    );
}

#[tokio::test]
async fn pick_first_sticks_then_fails_over() {
    let (addr_a, unaries_a, guard_a) = serve_named("A").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let channel = Channel::connect_uri(
        &format!(
            "ipv4:{}:{},{}:{}",
            addr_a.ip(),
            addr_a.port(),
            addr_b.ip(),
            addr_b.port()
        ),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel);

    // Stickiness: every call lands on A.
    for _ in 0..5 {
        unary_tag_is(&client, "A:ada").await;
    }
    assert_eq!(unaries_a.load(Ordering::SeqCst), 5);
    assert_eq!(unaries_b.load(Ordering::SeqCst), 0);

    // Kill A: calls fail over to B (the abort lands asynchronously,
    // so early calls may still serve from A or error once).
    drop(guard_a);
    let mut failed_over = false;
    for _ in 0..20 {
        match client.say_hello(Request::new(req("ada"))).await {
            Ok(response) => {
                if response.into_inner().message() == "B:ada" {
                    failed_over = true;
                    break;
                }
            }
            Err(status) => assert_eq!(status.code(), Code::Unavailable),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(failed_over);
    for _ in 0..3 {
        unary_tag_is(&client, "B:ada").await;
    }

    // Rebind A's port: pick_first stays on B (no flap-back).
    let unaries_a2 = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries_a2);
    let listener = TcpListener::bind(addr_a).await.expect("rebind");
    let handle = tokio::spawn(async move {
        GreeterServer::new(Named {
            tag: "A",
            unaries: worker,
        })
        .serve_listener(listener)
        .await
        .ok();
    });
    let _guard_a2 = ServerGuard(handle);
    for _ in 0..3 {
        unary_tag_is(&client, "B:ada").await;
    }
    assert_eq!(unaries_a2.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pick_first_unary_pinned_procedure() {
    // Ports grpc-go@dd51b1c9 DoPickFirstUnary (interop/test_utils.go):
    // rpcCount = 100 unary RPCs, every response must carry a non-empty
    // server id and all 100 must agree (one backend despite several
    // resolved). Greeter echo tags stand in for SimpleResponse.server_id:
    // the vendored testing proto predates fill_server_id, so the
    // wire-exact TestService shape waits on a proto sync.
    const RPC_COUNT: usize = 100;
    let (addr_a, unaries_a, _guard_a) = serve_named("A").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let channel = Channel::connect_uri(
        &format!(
            "ipv4:{}:{},{}:{}",
            addr_a.ip(),
            addr_a.port(),
            addr_b.ip(),
            addr_b.port()
        ),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel);

    let mut server_id = String::new();
    for i in 0..RPC_COUNT {
        let tag = client
            .say_hello(Request::new(req("ada")))
            .await
            .unwrap_or_else(|err| panic!("iteration {i}: unary failed: {err}"))
            .into_inner()
            .message()
            .to_string();
        assert!(!tag.is_empty(), "iteration {i}: empty server id");
        if i == 0 {
            server_id = tag;
        } else {
            assert_eq!(tag, server_id, "iteration {i}: backend changed");
        }
    }
    assert_eq!(server_id, "A:ada");
    assert_eq!(unaries_a.load(Ordering::SeqCst), RPC_COUNT);
    assert_eq!(unaries_b.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pick_first_skips_dead_first_address() {
    let closed = {
        let (addr, listener) = bind().await;
        drop(listener);
        addr
    };
    let (addr_b, _, _guard_b) = serve_named("B").await;
    let channel = Channel::connect_uri(
        &format!(
            "ipv4:{}:{},{}:{}",
            closed.ip(),
            closed.port(),
            addr_b.ip(),
            addr_b.port()
        ),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel);
    // Happy Eyeballs: the refused head fails fast, the race moves to
    // the live address immediately, and the first RPC already serves.
    unary_tag_is(&client, "B:ada").await;
    unary_tag_is(&client, "B:ada").await;
}

/// Scripted A answers switching once, then repeating.
struct SwitchA {
    steps: tokio::sync::Mutex<Vec<Vec<SocketAddr>>>,
}

impl DnsLookup for SwitchA {
    fn lookup(
        &self,
        _host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        Box::pin(async move {
            let mut steps = self.steps.lock().await;
            if steps.len() > 1 {
                Ok(steps.remove(0))
            } else {
                Ok(steps.first().cloned().unwrap_or_default())
            }
        })
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

#[tokio::test]
async fn removed_address_drains_without_migrating() {
    let (addr_a, _, _guard_a) = serve_named("A").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let dns = Arc::new(SwitchA {
        steps: tokio::sync::Mutex::new(vec![vec![addr_a, addr_b], vec![addr_b]]),
    });
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns);
    let channel = Channel::connect_uri("dns:///fl.invalid:443", config)
        .await
        .expect("channel");
    let client = GreeterClient::new(channel);
    unary_tag_is(&client, "A:ada").await;

    // Slow stream starts on A; the refresh then removes A mid-stream.
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("stream")
        .into_inner();
    assert_eq!(
        stream
            .message()
            .await
            .expect("item")
            .expect("some")
            .message(),
        "A:1:ada"
    );
    // Wait for the policy to move off A: new unaries land on B.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if unaries_b.load(Ordering::SeqCst) > 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "policy never moved off A"
        );
        match client.say_hello(Request::new(req("ada"))).await {
            Ok(response) => {
                if response.into_inner().message() == "B:ada" {
                    break;
                }
            }
            Err(status) => assert_eq!(status.code(), Code::Unavailable),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The A stream was never migrated or killed: item 2 arrives from A.
    assert_eq!(
        stream
            .message()
            .await
            .expect("item")
            .expect("some")
            .message(),
        "A:2:ada"
    );
}

#[tokio::test]
async fn all_failing_fast_fails_and_wait_for_ready_recovers() {
    let closed_a = {
        let (addr, listener) = bind().await;
        drop(listener);
        addr
    };
    let closed_b = {
        let (addr, listener) = bind().await;
        drop(listener);
        addr
    };
    let channel = Channel::connect_uri(
        &format!(
            "ipv4:{}:{},{}:{}",
            closed_a.ip(),
            closed_a.port(),
            closed_b.ip(),
            closed_b.port()
        ),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel.clone());

    // Fail-fast errors at once.
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("nothing listening");
    assert_eq!(err.code(), Code::Unavailable);

    // ... and the call deadline bounds a wait-for-ready wait.
    let mut request = Request::new(req("ada"));
    request.set_wait_for_ready(true);
    request.set_timeout(Duration::from_millis(200));
    let err = GreeterClient::new(channel.clone())
        .say_hello(request)
        .await
        .expect_err("deadline bounds the wait");
    assert_eq!(err.code(), Code::DeadlineExceeded);

    // Wait-for-ready outlives the outage: bind B's port mid-wait.
    let mut request = Request::new(req("ada"));
    request.set_wait_for_ready(true);
    let mut slow = client.say_hello(request);
    tokio::select! {
        biased;
        _ = &mut slow => panic!("recovered before the server existed"),
        () = tokio::time::sleep(Duration::from_millis(300)) => {}
    }
    let listener = TcpListener::bind(closed_b).await.expect("rebind");
    let handle = tokio::spawn(async move {
        GreeterServer::new(Named {
            tag: "B",
            unaries: Arc::new(AtomicUsize::new(0)),
        })
        .serve_listener(listener)
        .await
        .ok();
    });
    let _guard = ServerGuard(handle);
    let reply = tokio::time::timeout(Duration::from_secs(10), slow)
        .await
        .expect("recovered in time")
        .expect("unary")
        .into_inner();
    assert_eq!(reply.message(), "B:ada");
}

#[tokio::test]
async fn shuffle_distributes_first_pick() {
    let config = pbrs_grpc::ServiceConfig::parse(
        r#"{"loadBalancingConfig": [{"pick_first": {"shuffleAddressList": true}}]}"#,
    )
    .expect("parses");
    let addrs = vec![
        ResolvedAddress::Tcp("10.0.0.1:80".parse().expect("addr")),
        ResolvedAddress::Tcp("10.0.0.2:80".parse().expect("addr")),
    ];
    let mut firsts = [0u32; 2];
    for _ in 0..50 {
        let policy = PickFirst::from_config(Some(&config));
        policy.update(addrs.clone()).await;
        match policy.pick().await {
            pbrs_grpc::lb::Pick::Use(ResolvedAddress::Tcp(sock)) => {
                let last = sock.ip().to_string();
                if last.starts_with("10.0.0.1") {
                    firsts[0] += 1;
                } else {
                    firsts[1] += 1;
                }
            }
            _ => panic!("expected an address pick"),
        }
    }
    assert!(
        firsts[0] >= 5 && firsts[1] >= 5,
        "shuffle split: {firsts:?}"
    );
}

#[tokio::test]
async fn weighted_shuffle_favors_heavy_endpoint() {
    let config = pbrs_grpc::ServiceConfig::parse(
        r#"{"loadBalancingConfig": [{"pick_first": {"shuffleAddressList": true}}]}"#,
    )
    .expect("parses");
    let heavy = ResolvedAddress::Tcp("10.0.1.1:80".parse().expect("addr"));
    let lights = [
        ResolvedAddress::Tcp("10.0.1.2:80".parse().expect("addr")),
        ResolvedAddress::Tcp("10.0.1.3:80".parse().expect("addr")),
        ResolvedAddress::Tcp("10.0.1.4:80".parse().expect("addr")),
    ];
    // Efraimidis–Spirakis first-pick share is weight/total: 12/15 =
    // 0.8 here. Demand > 0.6 over 200 rounds (14σ of margin).
    const ROUNDS: u32 = 200;
    let mut heavy_first = 0u32;
    for _ in 0..ROUNDS {
        let policy = PickFirst::from_config(Some(&config));
        policy
            .update_weighted(vec![
                (heavy.clone(), 12),
                (lights[0].clone(), 1),
                (lights[1].clone(), 1),
                (lights[2].clone(), 1),
            ])
            .await;
        match policy.pick().await {
            pbrs_grpc::lb::Pick::Use(addr) if addr == heavy => heavy_first += 1,
            pbrs_grpc::lb::Pick::Use(_) => {}
            _ => panic!("expected an address pick"),
        }
    }
    assert!(
        heavy_first > 120,
        "heavy-first {heavy_first}/{ROUNDS}, want > 120"
    );
}

#[tokio::test]
async fn ipv4_only_fixture_connects() {
    let (addr, _unaries, _guard) = serve_named("V4").await;
    let channel = Channel::connect_uri(
        &format!("ipv4:{}:{}", addr.ip(), addr.port()),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    unary_tag_is(&GreeterClient::new(channel), "V4:ada").await;
}

#[tokio::test]
async fn endpoint_from_shared_http_connects() {
    let (addr, _unaries, _guard) = serve_named("EP").await;
    let endpoint = Endpoint::from_shared(format!("http://{addr}")).expect("endpoint");
    let channel = endpoint.connect().await.expect("connect");
    unary_tag_is(&GreeterClient::new(channel), "EP:ada").await;
}

#[tokio::test]
async fn endpoint_connect_with_connector_uses_custom_io() {
    let (addr, _unaries, _guard) = serve_named("CONN").await;
    let endpoint = Endpoint::from_shared(format!("http://{addr}")).expect("endpoint");
    let channel = endpoint
        .connect_with_connector(move |_uri| async move {
            TcpStream::connect(addr)
                .await
                .map_err(|e| Status::unavailable(format!("connect: {e}")))
        })
        .await
        .expect("connector");
    unary_tag_is(&GreeterClient::new(channel), "CONN:ada").await;
}

#[tokio::test]
async fn endpoint_balance_list_uses_round_robin() {
    let (addr_a, unaries_a, _guard_a) = serve_named("EA").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("EB").await;
    let channel = Endpoint::balance_list(
        [
            Endpoint::from_shared(format!("http://{addr_a}")).expect("a"),
            Endpoint::from_shared(format!("http://{addr_b}")).expect("b"),
        ],
        ChannelConfig::new(),
    )
    .await
    .expect("balance_list");
    let client = GreeterClient::new(channel);
    let tags = [
        unary_tag(&client).await,
        unary_tag(&client).await,
        unary_tag(&client).await,
        unary_tag(&client).await,
    ];
    assert_eq!(tags, ["EA:ada", "EB:ada", "EA:ada", "EB:ada"]);
    assert_eq!(unaries_a.load(Ordering::SeqCst), 2);
    assert_eq!(unaries_b.load(Ordering::SeqCst), 2);
}

async fn serve_named_v6(tag: &'static str) -> Option<(SocketAddr, Arc<AtomicUsize>, ServerGuard)> {
    let listener = TcpListener::bind("[::1]:0").await.ok()?;
    let addr = listener.local_addr().ok()?;
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let handle = tokio::spawn(async move {
        GreeterServer::new(Named {
            tag,
            unaries: worker,
        })
        .serve_listener(listener)
        .await
        .ok();
    });
    Some((addr, unaries, ServerGuard(handle)))
}

#[tokio::test]
async fn ipv6_only_fixture_connects() {
    // No loopback v6 on the host: nothing to prove; skip.
    let Some((addr, _unaries, _guard)) = serve_named_v6("V6").await else {
        return;
    };
    let channel = Channel::connect_uri(
        &format!("ipv6:[{}]:{}", addr.ip(), addr.port()),
        ResolverConfig::static_only(),
    )
    .await
    .expect("channel");
    unary_tag_is(&GreeterClient::new(channel), "V6:ada").await;
}

/// One fixed answer, repeated on every refresh.
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

/// A listener that accepts and holds one connection without ever
/// speaking HTTP/2. Signals when the peer's socket closes, proving
/// the losing race attempt left no established connection behind.
async fn blackhole_v6() -> Option<(SocketAddr, tokio::sync::oneshot::Receiver<()>)> {
    use tokio::io::AsyncReadExt as _;
    let listener = TcpListener::bind("[::1]:0").await.ok()?;
    let addr = listener.local_addr().ok()?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        let mut buf = [0u8; 1024];
        loop {
            match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
        tx.send(()).ok();
    });
    Some((addr, rx))
}

#[tokio::test]
async fn broken_ipv6_races_to_ipv4_without_leak() {
    // No loopback v6 on the host: nothing to prove; skip.
    let Some((hole, eof)) = blackhole_v6().await else {
        return;
    };
    let (addr_b, _unaries_b, _guard_b) = serve_named("B").await;
    let dns = Arc::new(FixedAddrs(vec![hole, addr_b]));
    let channel = Channel::connect_uri(
        "dns:///dual.invalid:443",
        ResolverConfig::with_dns_provider(fast_bounds(), dns),
    )
    .await
    .expect("channel");
    let client = GreeterClient::new(channel);
    // The v6 head hangs in the HTTP/2 preface; the 250ms stagger
    // must start the v4 attempt instead of serializing on the 20s
    // handshake timeout.
    let start = tokio::time::Instant::now();
    unary_tag_is(&client, "B:ada").await;
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(200),
        "stagger fired before v4 won: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "raced, not serialized: {elapsed:?}"
    );
    // The hung v6 attempt's socket closed: no duplicate established
    // connection leaked.
    tokio::time::timeout(Duration::from_secs(5), eof)
        .await
        .expect("eof signal arrives")
        .expect("blackhole outlives the race");
}

#[tokio::test]
async fn tls_failover_keeps_identity() {
    let tls =
        ServerTls::new(pbrs_grpc::Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity"))
            .expect("server tls");
    let closed = {
        let (addr, listener) = bind().await;
        drop(listener);
        addr
    };
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let channel = Channel::connect_tls_uri(
        &format!(
            "ipv4:{}:{},{}:{}",
            closed.ip(),
            closed.port(),
            addr.ip(),
            addr.port()
        ),
        ClientTls::ca("localhost", CA).expect("client tls"),
        ResolverConfig::static_only(),
    )
    .await
    .expect("tls uri");
    // The race skips the refused head transparently: the first RPC
    // already serves, and the winning dial verified the configured
    // TLS name rather than the dead IP.
    assert_eq!(say_hello(channel).await, "ada");
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

const RR_DOC: &str = r#"{"loadBalancingConfig": [{"round_robin": {}}]}"#;

/// A round_robin channel over scripted DNS answers plus a TXT service
/// config selecting `round_robin`.
async fn rr_channel_with(dns: Arc<dyn DnsLookup>, host: &str) -> Channel {
    let txt = Arc::new(FixedTxt(RR_DOC));
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

#[tokio::test]
async fn round_robin_distributes_evenly() {
    let (addr_a, unaries_a, _guard_a) = serve_named("A").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let (addr_c, unaries_c, _guard_c) = serve_named("C").await;
    let dns = Arc::new(FixedAddrs(vec![addr_a, addr_b, addr_c]));
    let channel = rr_channel_with(dns, "rr.invalid").await;
    let client = GreeterClient::new(channel);

    // Strict rotation: exact order, then exact thirds over 60 RPCs.
    let mut tags = Vec::new();
    for _ in 0..6 {
        tags.push(unary_tag(&client).await);
    }
    assert_eq!(
        tags,
        vec!["A:ada", "B:ada", "C:ada", "A:ada", "B:ada", "C:ada"]
    );
    for _ in 0..54 {
        unary_tag(&client).await;
    }
    assert_eq!(unaries_a.load(Ordering::SeqCst), 20);
    assert_eq!(unaries_b.load(Ordering::SeqCst), 20);
    assert_eq!(unaries_c.load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn round_robin_skips_dead_backend() {
    let (closed, listener) = bind().await;
    drop(listener);
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let (addr_c, unaries_c, _guard_c) = serve_named("C").await;
    let dns = Arc::new(FixedAddrs(vec![closed, addr_b, addr_c]));
    let channel = rr_channel_with(dns, "rr.invalid").await;
    let client = GreeterClient::new(channel);

    // Fail-fast: the first rotation hits the dead port and errors
    // once; afterwards the survivors split evenly.
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("first rotation hits the dead port");
    assert_eq!(err.code(), Code::Unavailable);
    for _ in 0..40 {
        let tag = unary_tag(&client).await;
        assert!(tag == "B:ada" || tag == "C:ada", "no stale route: {tag}");
    }
    assert_eq!(unaries_b.load(Ordering::SeqCst), 20);
    assert_eq!(unaries_c.load(Ordering::SeqCst), 20);
}

#[tokio::test]
async fn round_robin_churn_removes_and_recovers() {
    let (addr_a, unaries_a, _guard_a) = serve_named("A").await;
    let (addr_b, unaries_b, _guard_b) = serve_named("B").await;
    let dns = Arc::new(SwitchA {
        steps: tokio::sync::Mutex::new(vec![
            vec![addr_a, addr_b],
            vec![addr_b],
            vec![addr_a, addr_b],
        ]),
    });
    let channel = rr_channel_with(dns, "rr.invalid").await;
    let client = GreeterClient::new(channel);

    // Phase 1: both serve in rotation.
    let mut tags = Vec::new();
    for _ in 0..4 {
        tags.push(unary_tag(&client).await);
    }
    assert_eq!(tags, vec!["A:ada", "B:ada", "A:ada", "B:ada"]);

    // Phase 2: A leaves; tight loop until 6 consecutive B-only
    // (phases last one refresh interval, so detect in milliseconds,
    // not sleeps), then prove A routes nothing further (10 more
    // RPCs, counter frozen).
    let mut consecutive_b = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while consecutive_b < 6 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "A removed from rotation"
        );
        if unary_tag(&client).await == "B:ada" {
            consecutive_b += 1;
        } else {
            consecutive_b = 0;
        }
    }
    let frozen_a = unaries_a.load(Ordering::SeqCst);
    for _ in 0..10 {
        assert_eq!(unary_tag(&client).await, "B:ada");
    }
    assert_eq!(unaries_a.load(Ordering::SeqCst), frozen_a);

    // Phase 3: A returns and serves again (fresh dial, no stale conn).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut recovered = false;
    while tokio::time::Instant::now() < deadline {
        if unary_tag(&client).await == "A:ada" {
            recovered = true;
            break;
        }
    }
    assert!(recovered, "A rejoins rotation");
    assert!(unaries_b.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn round_robin_all_down_fails_fast() {
    let (closed_a, listener_a) = bind().await;
    drop(listener_a);
    let (closed_b, listener_b) = bind().await;
    drop(listener_b);
    let dns = Arc::new(FixedAddrs(vec![closed_a, closed_b]));
    let channel = rr_channel_with(dns, "rr.invalid").await;
    let client = GreeterClient::new(channel);

    for _ in 0..2 {
        let err = client
            .say_hello(Request::new(req("ada")))
            .await
            .expect_err("all backends down");
        assert_eq!(err.code(), Code::Unavailable);
    }
}
