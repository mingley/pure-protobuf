//! Transport byte budget and memory bounds verification tests.

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

use bytes::Bytes;
use common::{name_of, req, Echo};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{Channel, Code, Request, Server, Status};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

fn current_rss_bytes() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
            {
                return rss_pages * 4096;
            }
        }
    }
    // Safe portable fallback using standard `ps` utility (macOS and Unix)
    if let Ok(output) = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
    {
        if let Ok(s) = std::str::from_utf8(&output.stdout) {
            if let Ok(kib) = s.trim().parse::<u64>() {
                return kib * 1024;
            }
        }
    }
    0
}

/// Metrics and observations recorded during fairness, overload, and slow-peer verification.
#[derive(Debug, Default, Clone)]
struct FairnessRecord {
    successful: usize,
    rejected: usize,
    queue_delays: Vec<Duration>,
    latencies: Vec<Duration>,
    peak_permits: usize,
    start_rss_bytes: u64,
    peak_rss_bytes: u64,
}

impl FairnessRecord {
    fn new() -> Self {
        let rss = current_rss_bytes();
        Self {
            successful: 0,
            rejected: 0,
            queue_delays: Vec::new(),
            latencies: Vec::new(),
            peak_permits: 0,
            start_rss_bytes: rss,
            peak_rss_bytes: rss,
        }
    }

    fn record_success(&mut self, queue_delay: Duration, latency: Duration, permits: usize) {
        self.successful += 1;
        self.queue_delays.push(queue_delay);
        self.latencies.push(latency);
        self.peak_permits = self.peak_permits.max(permits);
        let rss = current_rss_bytes();
        self.peak_rss_bytes = self.peak_rss_bytes.max(rss);
    }

    fn record_rejection(&mut self, permits: usize) {
        self.rejected += 1;
        self.peak_permits = self.peak_permits.max(permits);
        let rss = current_rss_bytes();
        self.peak_rss_bytes = self.peak_rss_bytes.max(rss);
    }

    fn p99_latency(&self) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.latencies.clone();
        sorted.sort();
        let idx = ((sorted.len() as f64) * 0.99).ceil() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn max_queue_delay(&self) -> Duration {
        self.queue_delays
            .iter()
            .copied()
            .max()
            .unwrap_or(Duration::ZERO)
    }

    #[allow(dead_code)]
    fn rss_growth_bytes(&self) -> u64 {
        self.peak_rss_bytes.saturating_sub(self.start_rss_bytes)
    }
}

async fn spawn_budgeted_server(
    budget: usize,
) -> (SocketAddr, Server<GreeterServer<Echo>>, common::ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let server = Server::new(GreeterServer::new(Echo)).byte_budget(budget);
    let srv_clone = server.clone();
    let handle = tokio::spawn(async move {
        srv_clone.serve_listener(listener).await.ok();
    });
    (addr, server, common::ServerGuard(handle))
}

async fn spawn_custom_server(
    f: impl FnOnce(Server<GreeterServer<Echo>>) -> Server<GreeterServer<Echo>>,
) -> (SocketAddr, Server<GreeterServer<Echo>>, common::ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let server = f(Server::new(GreeterServer::new(Echo)));
    let srv_clone = server.clone();
    let handle = tokio::spawn(async move {
        srv_clone.serve_listener(listener).await.ok();
    });
    (addr, server, common::ServerGuard(handle))
}

async fn connect_client(addr: SocketAddr) -> Channel {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(ch) => return ch,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

#[tokio::test]
async fn test_client_unary_byte_budget_rejection_and_quiescence() {
    let (addr, server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr).await.byte_budget(128);
    let client = GreeterClient::new(channel.clone());

    // 1. Normal small message under cap succeeds.
    let reply = client
        .say_hello(Request::new(req("small")))
        .await
        .expect("under budget succeeds");
    assert_eq!(name_of(reply.get_ref()), "small");
    assert!(channel.is_byte_budget_quiescent());
    assert_eq!(channel.byte_budget_allocated(), 0);

    // 2. Large message exceeding client byte budget is rejected with ResourceExhausted.
    let big_name = "x".repeat(500);
    let err = client
        .say_hello(Request::new(req(&big_name)))
        .await
        .expect_err("oversize request must be rejected");
    assert_eq!(err.code(), Code::ResourceExhausted);
    assert!(
        err.message().contains("transport byte budget exceeded"),
        "error message should cite byte budget: {err}"
    );

    // 3. After rejection, byte accounting returns to 0 (quiescent).
    assert!(
        channel.is_byte_budget_quiescent(),
        "client allocated bytes should be 0 after rejection, found {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);

    // 4. Subsequent normal message succeeds without interference.
    let reply = client
        .say_hello(Request::new(req("small2")))
        .await
        .expect("subsequent small request succeeds");
    assert_eq!(name_of(reply.get_ref()), "small2");
    assert!(channel.is_byte_budget_quiescent());
    assert_eq!(channel.byte_budget_allocated(), 0);
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_server_unary_byte_budget_rejection_and_quiescence() {
    // Server capped at 128 bytes.
    let (addr, server, _guard) = spawn_budgeted_server(128).await;
    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel.clone());

    // 1. Small request produces small response (~10 bytes) -> succeeds.
    let reply = client
        .say_hello(Request::new(req("ok")))
        .await
        .expect("small reply succeeds");
    assert_eq!(name_of(reply.get_ref()), "ok");
    assert!(server.is_byte_budget_quiescent());
    assert_eq!(server.byte_budget_allocated(), 0);

    // 2. Request whose echo response exceeds 128 bytes.
    let big_payload = "a".repeat(500);
    let err = client
        .say_hello(Request::new(req(&big_payload)))
        .await
        .expect_err("oversize server response must be rejected");
    assert_eq!(err.code(), Code::ResourceExhausted);

    // Give server task a moment to settle after writing trailers-only response.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. After rejection, server byte accounting returns to 0.
    assert!(
        server.is_byte_budget_quiescent(),
        "server allocated bytes should return to 0, found {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);

    // 4. Normal small message succeeds without interference.
    let reply = client
        .say_hello(Request::new(req("ok2")))
        .await
        .expect("subsequent reply succeeds");
    assert_eq!(name_of(reply.get_ref()), "ok2");
    assert!(server.is_byte_budget_quiescent());
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_server_streaming_byte_budget_rejection_and_quiescence() {
    // Server capped at 128 bytes.
    let (addr, server, _guard) = spawn_budgeted_server(128).await;
    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // 1. Small parts succeed.
    let mut stream = client
        .server_hello(Request::new(req("a,b,c")))
        .await
        .expect("server stream open")
        .into_inner();
    let first = stream.message().await.expect("read 1").expect("item 1");
    assert_eq!(name_of(&first), "a");
    let second = stream.message().await.expect("read 2").expect("item 2");
    assert_eq!(name_of(&second), "b");
    let third = stream.message().await.expect("read 3").expect("item 3");
    assert_eq!(name_of(&third), "c");
    assert!(stream.message().await.expect("end").is_none());

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.is_byte_budget_quiescent());
    assert_eq!(server.byte_budget_allocated(), 0);

    // 2. Large part exceeding byte budget causes server stream to terminate with ResourceExhausted.
    let big_item = "z".repeat(500);
    let mut stream = client
        .server_hello(Request::new(req(&format!("first,{big_item}"))))
        .await
        .expect("server stream open")
        .into_inner();

    let mut err = None;
    loop {
        match stream.message().await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let err = match err {
        Some(e) => e,
        None => stream
            .trailers()
            .await
            .expect_err("expected error in trailers"),
    };
    assert_eq!(err.code(), Code::ResourceExhausted);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server byte allocation should return to 0 after stream error, found {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_client_streaming_byte_budget_rejection_and_quiescence() {
    let (addr, _server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr).await.byte_budget(128);
    let client = GreeterClient::new(channel.clone());

    // 1. Normal client-streaming below budget succeeds.
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("alice")).await.expect("send 1");
    tx.send(req("bob")).await.expect("send 2");
    tx.close();
    let reply = call.await.expect("client-stream succeeds");
    assert_eq!(name_of(reply.get_ref()), "alice,bob");
    assert!(channel.is_byte_budget_quiescent());
    assert_eq!(channel.byte_budget_allocated(), 0);

    // 2. Client-streaming with a message exceeding byte budget fails with ResourceExhausted.
    let (tx, call) = client.client_hello(Request::new(()));
    let big_name = "k".repeat(500);
    tx.send(req(&big_name)).await.expect("send into channel");
    tx.close();
    let err = call
        .await
        .expect_err("oversize stream message must fail call");
    assert_eq!(err.code(), Code::ResourceExhausted);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "channel allocated bytes should return to 0, found {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_bidi_streaming_byte_budget_rejection_and_quiescence() {
    let (addr, _server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr).await.byte_budget(128);
    let client = GreeterClient::new(channel.clone());

    // 1. Normal bidi streaming under budget succeeds.
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ping")).await.expect("send ping");
    let mut inbound = call.await.expect("bidi open").into_inner();
    let reply = inbound.message().await.expect("recv").expect("reply");
    assert_eq!(name_of(&reply), "ping");
    tx.close();
    assert!(inbound.message().await.expect("end").is_none());
    drop(inbound);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(channel.is_byte_budget_quiescent());
    assert_eq!(channel.byte_budget_allocated(), 0);

    // 2. Bidi sending oversize message triggers ResourceExhausted.
    let (tx, call) = client.stream_hello(Request::new(()));
    let big_msg = "w".repeat(500);
    tx.send(req(&big_msg)).await.expect("send big into queue");
    tx.close();

    match call.await {
        Ok(mut resp) => {
            let res = resp.get_mut().message().await;
            if let Err(e) = res {
                assert_eq!(e.code(), Code::ResourceExhausted);
            }
        }
        Err(e) => {
            assert_eq!(e.code(), Code::ResourceExhausted);
        }
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "channel budget must return to 0 after bidi rejection, found {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_client_cancellation_releases_budget() {
    let (addr, _server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr).await.byte_budget(1024);
    let client = GreeterClient::new(channel.clone());

    // Start a call and immediately drop the future (cancel)
    let call = client.say_hello(Request::new(req("cancelled_promptly")));
    drop(call);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "cancelled call must release permit, allocated = {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_max_send_buffer_size_sets_budget_limit() {
    let (addr, server, _guard) = spawn_budgeted_server(1_000_000).await;
    assert_eq!(server.byte_budget_limit(), Some(1_000_000));

    let server_custom = Server::new(GreeterServer::new(Echo)).max_send_buffer_size(4096);
    assert_eq!(server_custom.byte_budget_limit(), Some(4096));

    let channel = connect_client(addr).await.max_send_buffer_size(8192);
    assert_eq!(channel.byte_budget_limit(), Some(8192));
}

#[tokio::test]
async fn test_concurrent_requests_below_cap_succeed_and_quiesce() {
    let (addr, server, _guard) = spawn_budgeted_server(50_000).await;
    let channel = connect_client(addr).await.byte_budget(50_000);
    let client = GreeterClient::new(channel.clone());

    let mut handles = Vec::new();
    for i in 0..20 {
        let cl = client.clone();
        handles.push(tokio::spawn(async move {
            let reply = cl
                .say_hello(Request::new(req(&format!("call-{i}"))))
                .await
                .expect("concurrent call succeeds");
            assert_eq!(name_of(reply.get_ref()), format!("call-{i}"));
        }));
    }

    for h in handles {
        h.await.expect("join handle");
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "client should be quiescent, found {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
    assert!(
        server.is_byte_budget_quiescent(),
        "server should be quiescent, found {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

struct RawPeer {
    send: h2::client::SendRequest<Bytes>,
    authority: String,
}

impl RawPeer {
    async fn connect(addr: SocketAddr) -> Result<Self, Status> {
        let tcp = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        let (send, conn) = h2::client::handshake(tcp)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        drop(tokio::spawn(async move {
            conn.await.ok();
        }));
        Ok(Self {
            send,
            authority: addr.to_string(),
        })
    }

    fn request(&self, path: &str) -> http::Request<()> {
        let uri = format!("http://{}{path}", self.authority);
        http::Request::builder()
            .method(http::Method::POST)
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/grpc")
            .header(http::header::TE, "trailers")
            .body(())
            .expect("request")
    }
}

#[tokio::test]
async fn test_competing_small_rpcs_progress_under_bulk_stream_load() {
    // Approved limits: send buffer 64 KiB, byte budget 512 KiB, max 100 concurrent RPCs
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_send_buffer_size(64 * 1024)
            .byte_budget(512 * 1024)
            .max_concurrent_rpcs(100)
    })
    .await;

    let bulk_channel = connect_client(addr).await;
    let bulk_client = GreeterClient::new(bulk_channel);

    // Launch 3 concurrent bulk stream downloads in background
    let bulk_stop = Arc::new(AtomicBool::new(false));
    let mut bulk_handles = Vec::new();
    for _ in 0..3 {
        let b_client = bulk_client.clone();
        let stop = bulk_stop.clone();
        bulk_handles.push(tokio::spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                let req_payload = (0..25)
                    .map(|i| format!("item-{i}"))
                    .collect::<Vec<_>>()
                    .join(",");
                if let Ok(stream) = b_client.server_hello(Request::new(req(&req_payload))).await {
                    let mut inner = stream.into_inner();
                    while let Ok(Some(_)) = inner.message().await {
                        tokio::task::yield_now().await;
                    }
                }
            }
        }));
    }

    tokio::time::sleep(Duration::from_millis(20)).await;

    let client_channel = connect_client(addr).await;
    let client = GreeterClient::new(client_channel);

    let mut record = FairnessRecord::new();
    let num_calls = 40;

    for i in 0..num_calls {
        let scheduled_start = Instant::now();
        tokio::time::sleep(Duration::from_millis(1)).await;
        let actual_start = Instant::now();
        let queue_delay = actual_start
            .duration_since(scheduled_start)
            .saturating_sub(Duration::from_millis(1));

        let reply = client
            .say_hello(Request::new(req(&format!("small-{i}"))))
            .await;
        let latency = actual_start.elapsed();

        let allocated_bytes = server.byte_budget_allocated();
        match reply {
            Ok(resp) => {
                assert_eq!(name_of(resp.get_ref()), format!("small-{i}"));
                record.record_success(queue_delay, latency, allocated_bytes);
            }
            Err(e) => {
                panic!("valid competing small RPC failed under bulk stream load: {e}");
            }
        }
    }

    bulk_stop.store(true, Ordering::Relaxed);
    for h in bulk_handles {
        h.await.ok();
    }

    assert_eq!(
        record.successful, num_calls,
        "all small RPCs must succeed without starvation"
    );
    assert_eq!(record.rejected, 0, "no unexpected rejections");
    assert!(
        record.p99_latency() < Duration::from_millis(500),
        "p99 latency under bulk load must be bounded, found {:?}",
        record.p99_latency()
    );
    assert!(
        record.max_queue_delay() < Duration::from_millis(100),
        "queue delay must remain bounded, found {:?}",
        record.max_queue_delay()
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must quiesce to 0 bytes after bulk streams end, found {}",
        server.byte_budget_allocated()
    );
}

#[tokio::test]
async fn test_slow_reader_peer_isolation() {
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_send_buffer_size(32 * 1024)
            .byte_budget(128 * 1024)
            .max_concurrent_rpcs(20)
    })
    .await;

    let slow_channel = connect_client(addr).await;
    let slow_client = GreeterClient::new(slow_channel);

    let slow_payload = (0..40)
        .map(|i| format!("slow-item-{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let slow_handle = tokio::spawn(async move {
        if let Ok(resp) = slow_client
            .server_hello(Request::new(req(&slow_payload)))
            .await
        {
            let mut stream = resp.into_inner();
            let _ = stream.message().await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            while let Ok(Some(_)) = stream.message().await {}
        }
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    let fast_channel = connect_client(addr).await;
    let fast_client = GreeterClient::new(fast_channel);

    let mut record = FairnessRecord::new();
    let num_fast = 25;

    for i in 0..num_fast {
        let start = Instant::now();
        let reply = fast_client
            .say_hello(Request::new(req(&format!("fast-{i}"))))
            .await;
        let latency = start.elapsed();
        let allocated = server.byte_budget_allocated();

        match reply {
            Ok(resp) => {
                assert_eq!(name_of(resp.get_ref()), format!("fast-{i}"));
                record.record_success(Duration::ZERO, latency, allocated);
            }
            Err(e) => {
                panic!("fast competing RPC should not be blocked by slow reader: {e}");
            }
        }
    }

    assert_eq!(record.successful, num_fast);
    assert!(
        record.p99_latency() < Duration::from_millis(200),
        "fast client p99 latency must remain low despite slow reader, found {:?}",
        record.p99_latency()
    );

    slow_handle.await.ok();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must quiesce after slow reader finishes, allocated = {}",
        server.byte_budget_allocated()
    );
}

#[tokio::test]
async fn test_slow_writer_peer_isolation() {
    let (addr, server, _guard) =
        spawn_custom_server(|s| s.max_concurrent_rpcs(20).byte_budget(128 * 1024)).await;

    let slow_channel = connect_client(addr).await;
    let slow_client = GreeterClient::new(slow_channel);

    let (slow_tx, slow_call) = slow_client.client_hello(Request::new(()));
    slow_tx
        .send(req("slow_start"))
        .await
        .expect("send initial item");

    let fast_channel = connect_client(addr).await;
    let fast_client = GreeterClient::new(fast_channel);

    let mut record = FairnessRecord::new();
    let num_fast = 25;

    for i in 0..num_fast {
        let start = Instant::now();
        let reply = fast_client
            .say_hello(Request::new(req(&format!("writer-probe-{i}"))))
            .await;
        let latency = start.elapsed();
        let allocated = server.byte_budget_allocated();

        match reply {
            Ok(resp) => {
                assert_eq!(name_of(resp.get_ref()), format!("writer-probe-{i}"));
                record.record_success(Duration::ZERO, latency, allocated);
            }
            Err(e) => {
                panic!("fast call must not be blocked by slow writer: {e}");
            }
        }
    }

    assert_eq!(record.successful, num_fast);
    assert!(
        record.p99_latency() < Duration::from_millis(200),
        "fast client p99 latency must remain low despite slow writer, found {:?}",
        record.p99_latency()
    );

    slow_tx
        .send(req("slow_finish"))
        .await
        .expect("send final item");
    slow_tx.close();
    let slow_reply = slow_call.await.expect("slow writer call completes");
    assert_eq!(name_of(slow_reply.get_ref()), "slow_start,slow_finish");

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must quiesce after slow writer finishes, allocated = {}",
        server.byte_budget_allocated()
    );
}

#[tokio::test]
async fn test_idle_peers_contention_and_fairness() {
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(25)
            .max_concurrent_rpcs(20)
            .byte_budget(128 * 1024)
    })
    .await;

    let mut idle_channels = Vec::new();
    for _ in 0..10 {
        idle_channels.push(connect_client(addr).await);
    }

    let active_channel = connect_client(addr).await;
    let active_client = GreeterClient::new(active_channel);

    let mut record = FairnessRecord::new();
    let num_calls = 30;

    for i in 0..num_calls {
        let start = Instant::now();
        let reply = active_client
            .say_hello(Request::new(req(&format!("active-{i}"))))
            .await;
        let latency = start.elapsed();
        let allocated = server.byte_budget_allocated();

        match reply {
            Ok(resp) => {
                assert_eq!(name_of(resp.get_ref()), format!("active-{i}"));
                record.record_success(Duration::ZERO, latency, allocated);
            }
            Err(e) => {
                panic!("active call failed with idle peers connected: {e}");
            }
        }
    }

    assert_eq!(record.successful, num_calls);
    assert!(record.p99_latency() < Duration::from_millis(200));

    drop(idle_channels);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_reset_storm_isolation_and_competing_progress() {
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_pending_accept_reset_streams(32)
            .max_concurrent_reset_streams(64)
            .max_concurrent_rpcs(20)
            .byte_budget(128 * 1024)
    })
    .await;

    let storm_addr = addr;
    let storm_stop = Arc::new(AtomicBool::new(false));
    let storm_stop_clone = storm_stop.clone();

    let storm_handle = tokio::spawn(async move {
        let mut raw = match RawPeer::connect(storm_addr).await {
            Ok(p) => p,
            Err(_) => return,
        };
        while !storm_stop_clone.load(Ordering::Relaxed) {
            match raw.send.clone().ready().await {
                Ok(mut send) => {
                    let req = raw.request("/helloworld.Greeter/SayHello");
                    if let Ok((_resp, mut stream)) = send.send_request(req, false) {
                        stream.send_reset(h2::Reason::CANCEL);
                    }
                }
                Err(_) => {
                    if let Ok(p) = RawPeer::connect(storm_addr).await {
                        raw = p;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    });

    let client_channel = connect_client(addr).await;
    let client = GreeterClient::new(client_channel);

    let mut record = FairnessRecord::new();
    let num_calls = 25;

    for i in 0..num_calls {
        let start = Instant::now();
        let reply = client
            .say_hello(Request::new(req(&format!("legit-{i}"))))
            .await;
        let latency = start.elapsed();
        let allocated = server.byte_budget_allocated();

        match reply {
            Ok(resp) => {
                assert_eq!(name_of(resp.get_ref()), format!("legit-{i}"));
                record.record_success(Duration::ZERO, latency, allocated);
            }
            Err(e) => {
                panic!("legitimate RPC failed during reset storm: {e}");
            }
        }
    }

    storm_stop.store(true, Ordering::Relaxed);
    storm_handle.await.ok();

    assert_eq!(record.successful, num_calls);
    assert!(
        record.p99_latency() < Duration::from_millis(300),
        "legitimate calls p99 latency must remain bounded under reset storm, found {:?}",
        record.p99_latency()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must be quiescent after reset storm ends, allocated = {}",
        server.byte_budget_allocated()
    );
}

#[tokio::test]
async fn test_overload_explicit_status_rejection_no_silent_buffering() {
    // Tight limits: max 2 concurrent RPCs, 2048-byte budget
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_rpcs(2)
            .byte_budget(2048)
            .max_send_buffer_size(1024)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    let mut record = FairnessRecord::new();
    let total_tasks = 24;

    let mut handles = Vec::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(total_tasks));

    for i in 0..total_tasks {
        let cl = client.clone();
        let b = barrier.clone();
        let srv = server.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let start = Instant::now();
            let result = cl
                .say_hello(Request::new(req(&format!("overload-{i}"))))
                .await;
            let latency = start.elapsed();
            let allocated = srv.byte_budget_allocated();
            (result, latency, allocated)
        }));
    }

    for h in handles {
        let (res, latency, allocated) = h.await.expect("task join");
        match res {
            Ok(resp) => {
                assert!(name_of(resp.get_ref()).starts_with("overload-"));
                record.record_success(Duration::ZERO, latency, allocated);
            }
            Err(status) => {
                assert_eq!(
                    status.code(),
                    Code::ResourceExhausted,
                    "rejection must be explicit ResourceExhausted, got {status}"
                );
                assert!(
                    status.message().contains("too many concurrent RPCs")
                        || status.message().contains("transport byte budget exceeded"),
                    "status message must explicitly cite limit violation: {status}"
                );
                record.record_rejection(allocated);
            }
        }
    }

    assert!(
        record.rejected > 0,
        "overload must produce explicit rejections, observed 0 rejections"
    );
    assert!(
        record.successful > 0,
        "admitted calls under cap must succeed, observed 0 successes"
    );
    assert_eq!(
        record.successful + record.rejected,
        total_tasks,
        "every single offered call must have an explicit outcome (success or rejection)"
    );

    assert!(
        record.p99_latency() < Duration::from_millis(500),
        "admitted calls must not suffer unbounded queue delay, p99 = {:?}",
        record.p99_latency()
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must be quiescent after overload drain, found {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

async fn spawn_shutdown_server(
    f: impl FnOnce(Server<GreeterServer<Echo>>) -> Server<GreeterServer<Echo>>,
) -> (
    SocketAddr,
    Server<GreeterServer<Echo>>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), Status>>,
) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let server = f(Server::new(GreeterServer::new(Echo)));
    let srv_clone = server.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        srv_clone
            .serve_with_shutdown(listener, async move {
                rx.await.ok();
            })
            .await
    });
    (addr, server, tx, handle)
}

#[tokio::test]
async fn test_graceful_shutdown_bounded_drain_unending_client_stream() {
    let grace = Duration::from_millis(150);
    let (addr, server, shutdown_tx, server_handle) = spawn_shutdown_server(|s| {
        s.max_connection_age_grace(grace)
            .byte_budget(64 * 1024)
            .max_concurrent_rpcs(10)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // 1. Client opens client-streaming RPC, sends one item, but NEVER closes stream.
    let (slow_tx, slow_call) = client.client_hello(Request::new(()));
    let _slow_handle = tokio::spawn(slow_call);
    slow_tx
        .send(req("unending_part_1"))
        .await
        .expect("send initial item");

    // Give server task a moment to process the first item.
    tokio::time::sleep(Duration::from_millis(30)).await;

    // 2. Trigger graceful shutdown while the stream is active and unending.
    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("send shutdown signal");

    // 3. Verify new-call admission stops: new connections are either refused or fail immediately.
    let new_conn_res = Channel::connect(addr).await;
    if let Ok(new_ch) = new_conn_res {
        let probe_client = GreeterClient::new(new_ch);
        let probe_res = probe_client.say_hello(Request::new(req("probe"))).await;
        assert!(
            probe_res.is_err(),
            "new call on new connection after shutdown must be refused: {probe_res:?}"
        );
    }

    // 4. Server drain must finish bounded by the grace period, NOT hanging indefinitely.
    let server_res = tokio::time::timeout(Duration::from_millis(1500), server_handle)
        .await
        .expect("server shutdown must complete within bounded grace window")
        .expect("server task join");
    assert!(server_res.is_ok());

    let drain_duration = drain_start.elapsed();
    assert!(
        drain_duration >= Duration::from_millis(100),
        "server must allow in-flight grace period to elapse before force-close, elapsed {drain_duration:?}"
    );
    assert!(
        drain_duration < Duration::from_millis(1200),
        "server drain must not hang indefinitely beyond grace policy, elapsed {drain_duration:?}"
    );

    // 5. Server resources must be fully quiescent after drain completes.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server byte budget must be quiescent after drain, allocated = {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_graceful_shutdown_blocked_upload_stream() {
    let grace = Duration::from_millis(150);
    let (addr, server, shutdown_tx, server_handle) = spawn_shutdown_server(|s| {
        s.max_connection_age_grace(grace)
            .byte_budget(64 * 1024)
            .max_concurrent_rpcs(10)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // Bidi streaming call: client sends 1 item, then stalls / blocks upload.
    let (tx, rx_call) = client.stream_hello(Request::new(()));
    tx.send(req("blocked_upload_1")).await.expect("send item 1");

    let mut rx = rx_call.await.expect("bidi response headers").into_inner();
    let first_reply = rx.message().await.expect("recv 1").expect("first reply");
    assert_eq!(name_of(&first_reply), "blocked_upload_1");

    // Client stops uploading (stalled upload).
    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown signal");

    // Drain terminates within bounded window.
    let server_res = tokio::time::timeout(Duration::from_millis(1500), server_handle)
        .await
        .expect("server shutdown must be bounded when upload blocks")
        .expect("server task join");
    assert!(server_res.is_ok());

    let drain_duration = drain_start.elapsed();
    assert!(
        drain_duration < Duration::from_millis(1200),
        "blocked upload must not extend shutdown beyond grace policy, elapsed {drain_duration:?}"
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        server.is_byte_budget_quiescent(),
        "server must be quiescent, allocated = {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_graceful_shutdown_handshake_stall_terminates_promptly() {
    let (addr, server, shutdown_tx, server_handle) = spawn_shutdown_server(|s| {
        s.handshake_timeout(Duration::from_millis(200))
            .max_connection_age_grace(Duration::from_millis(150))
            .byte_budget(64 * 1024)
    })
    .await;

    // Connect raw TCP socket but send 0 bytes (stall handshake).
    let raw_sock = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");

    // Brief pause to ensure accept loop accepted the socket and is in handshake.
    tokio::time::sleep(Duration::from_millis(25)).await;

    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown signal");

    // Server drain must not be blocked indefinitely by the stalled handshake socket.
    let server_res = tokio::time::timeout(Duration::from_millis(1000), server_handle)
        .await
        .expect("server shutdown must terminate promptly despite stalled handshake")
        .expect("server task join");
    assert!(server_res.is_ok());

    let drain_duration = drain_start.elapsed();
    assert!(
        drain_duration < Duration::from_millis(600),
        "stalled handshake must not delay shutdown beyond bound, took {drain_duration:?}"
    );

    drop(raw_sock);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.is_byte_budget_quiescent());
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_cancellation_cleanup_drops_call_futures_and_quiesces() {
    // Max 2 concurrent RPCs on both server and client.
    let (addr, server, _guard) =
        spawn_custom_server(|s| s.max_concurrent_rpcs(2).byte_budget(64 * 1024)).await;

    let channel = connect_client(addr)
        .await
        .max_concurrent_rpcs(2)
        .byte_budget(64 * 1024);
    let client = GreeterClient::new(channel.clone());

    // 1. Start two client-streaming calls that hold permits.
    let (tx1, call1) = client.client_hello(Request::new(()));
    let (tx2, call2) = client.client_hello(Request::new(()));
    let h1 = tokio::spawn(call1);
    let h2 = tokio::spawn(call2);

    tx1.send(req("holding_slot_1")).await.expect("send 1");
    tx2.send(req("holding_slot_2")).await.expect("send 2");

    tokio::time::sleep(Duration::from_millis(30)).await;

    // 2. A 3rd call must be rejected immediately due to concurrency limit.
    let err3 = client
        .say_hello(Request::new(req("overflow")))
        .await
        .expect_err("3rd call must exceed max_concurrent_rpcs(2)");
    assert_eq!(err3.code(), Code::ResourceExhausted);
    assert!(err3.message().contains("too many concurrent RPCs"));

    // 3. Drop/abort the call futures immediately (cancellation cleanup).
    h1.abort();
    h2.abort();
    drop(tx1);
    drop(tx2);

    tokio::time::sleep(Duration::from_millis(40)).await;

    // 4. Dropping the call futures must immediately release permits:
    // Subsequent calls can now acquire permits and succeed without being blocked.
    let mut success_count = 0;
    for i in 0..4 {
        let reply = client
            .say_hello(Request::new(req(&format!("clean_call_{i}"))))
            .await
            .expect("subsequent call must succeed after cancelled calls release permits");
        assert_eq!(name_of(reply.get_ref()), format!("clean_call_{i}"));
        success_count += 1;
    }
    assert_eq!(success_count, 4);

    // 5. Permitted memory and tracking must return to 0 (quiescent).
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "client byte budget must quiesce after cancellation, allocated = {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);

    assert!(
        server.is_byte_budget_quiescent(),
        "server byte budget must quiesce after cancellation, allocated = {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_unending_stream_deadline_expiration_and_drain() {
    // Server has a 120ms timeout on all calls.
    let (addr, server, shutdown_tx, server_handle) = spawn_shutdown_server(|s| {
        s.timeout(Duration::from_millis(120))
            .max_connection_age_grace(Duration::from_millis(150))
            .byte_budget(64 * 1024)
    })
    .await;

    let channel = connect_client(addr).await;
    let client = GreeterClient::new(channel);

    // Client starts client-streaming, sends 1 message, but never closes the stream.
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("deadline_unending")).await.expect("send 1");

    // Wait for the server-side deadline to expire.
    let err = call
        .await
        .expect_err("call must fail with deadline exceeded");
    assert_eq!(err.code(), Code::DeadlineExceeded);

    // Byte budget on server returns to quiescent after deadline closes the stream.
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(server.is_byte_budget_quiescent());

    // Now trigger server shutdown: should complete quickly since expired stream is already gone.
    let drain_start = Instant::now();
    shutdown_tx.send(()).expect("shutdown signal");
    let server_res = tokio::time::timeout(Duration::from_millis(500), server_handle)
        .await
        .expect("server shutdown after deadline clean should finish quickly")
        .expect("server join");
    assert!(server_res.is_ok());

    let drain_duration = drain_start.elapsed();
    assert!(
        drain_duration < Duration::from_millis(300),
        "shutdown after cleaned stream must be fast, took {drain_duration:?}"
    );
    assert!(server.is_byte_budget_quiescent());
}
