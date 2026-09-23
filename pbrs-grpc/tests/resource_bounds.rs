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
use common::{Echo, name_of, req};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{
    CallLabels, Channel, ChannelConfig, Code, LifecycleObserver, RejectionEvent, RejectionReason,
    Request, Server, Status,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::Instant as TokioInstant;

// RT-07 CI smoke bounds from docs/resource-budgets.md, section 7.3.
const BULK_SMALL_P99: Duration = Duration::from_millis(500);
const BULK_MAX_SCHEDULING_LAG: Duration = Duration::from_millis(100);
const SLOW_PEER_P99: Duration = Duration::from_millis(200);
const IDLE_PEER_P99: Duration = Duration::from_millis(200);
const RESET_STORM_P99: Duration = Duration::from_millis(300);
const OVERLOAD_P99: Duration = Duration::from_millis(500);
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
            {
                return rss_pages.checked_mul(4096);
            }
        }
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[derive(Debug, Default)]
struct FairnessRecord {
    successful: usize,
    rejected: usize,
    timed_out: usize,
    other_errors: Vec<String>,
    queue_delays: Vec<Duration>,
    latencies: Vec<Duration>,
    admitted_latencies: Vec<Duration>,
    rejection_latencies: Vec<Duration>,
    max_sampled_allocated_bytes: usize,
    start_rss_bytes: Option<u64>,
    peak_rss_bytes: Option<u64>,
}

impl FairnessRecord {
    fn new() -> Self {
        let rss = current_rss_bytes();
        Self {
            start_rss_bytes: rss,
            peak_rss_bytes: rss,
            ..Self::default()
        }
    }

    fn observe(&mut self, queue_delay: Duration, latency: Duration, allocated_bytes: usize) {
        self.queue_delays.push(queue_delay);
        self.latencies.push(latency);
        self.max_sampled_allocated_bytes = self.max_sampled_allocated_bytes.max(allocated_bytes);
    }

    fn record_success(&mut self, queue_delay: Duration, latency: Duration, allocated_bytes: usize) {
        self.successful += 1;
        self.admitted_latencies.push(latency);
        self.observe(queue_delay, latency, allocated_bytes);
    }

    fn record_error(
        &mut self,
        status: &Status,
        queue_delay: Duration,
        latency: Duration,
        allocated_bytes: usize,
    ) {
        if status.code() == Code::ResourceExhausted {
            self.rejected += 1;
            self.rejection_latencies.push(latency);
        } else {
            self.other_errors.push(status.to_string());
        }
        self.observe(queue_delay, latency, allocated_bytes);
    }

    fn record_timeout(&mut self, queue_delay: Duration, latency: Duration, allocated_bytes: usize) {
        self.timed_out += 1;
        self.observe(queue_delay, latency, allocated_bytes);
    }

    fn sample_resources(&mut self, allocated_bytes: usize) {
        self.max_sampled_allocated_bytes = self.max_sampled_allocated_bytes.max(allocated_bytes);
        if let Some(rss) = current_rss_bytes() {
            self.peak_rss_bytes = Some(self.peak_rss_bytes.map_or(rss, |peak| peak.max(rss)));
        }
    }

    fn p99(samples: &[Duration]) -> Duration {
        assert!(!samples.is_empty(), "p99 needs observed calls");
        let mut sorted = samples.to_vec();
        sorted.sort();
        sorted[(99 * sorted.len()).div_ceil(100) - 1]
    }

    fn p99_latency(&self) -> Duration {
        Self::p99(&self.latencies)
    }

    fn p99_admitted(&self) -> Duration {
        Self::p99(&self.admitted_latencies)
    }

    fn max_queue_delay(&self) -> Duration {
        self.queue_delays
            .iter()
            .copied()
            .max()
            .expect("queue lag needs observed calls")
    }

    fn assert_complete(&self, offered: usize) {
        assert_eq!(self.latencies.len(), offered, "missing latency samples");
        assert_eq!(self.queue_delays.len(), offered, "missing scheduling lag");
        assert_eq!(
            self.successful + self.rejected + self.timed_out + self.other_errors.len(),
            offered,
            "every offered call needs an explicit outcome: {self:?}"
        );
        assert!(
            self.other_errors.is_empty(),
            "unexpected statuses: {self:?}"
        );
        assert_eq!(self.timed_out, 0, "unanswered calls: {self:?}");
    }

    fn report(&self, label: &str, queue: &QueueProbe, budget: usize, permit: &PermitProbe) {
        let (waits, max_wait) = queue.summary();
        assert_eq!(waits, self.latencies.len(), "missing client queue waits");
        let observed_bytes = self
            .max_sampled_allocated_bytes
            .max(permit.peak_allocated_bytes.load(Ordering::SeqCst));
        assert!(
            observed_bytes <= budget && (self.successful == 0 || observed_bytes > 0),
            "observed allocation {observed_bytes} is outside the expected 0..={budget} byte budget"
        );
        let admitted_streams_peak = permit.peak_streams.load(Ordering::SeqCst);
        let admitted_streams_active = permit.active();
        eprintln!(
            "{label}: offered={} success={} rejected={} p99_all={:?} max_scheduling_lag={:?} max_client_queue_wait={max_wait:?} max_observed_server_allocated_bytes={observed_bytes} byte_budget_bytes={budget} admitted_streams_peak={admitted_streams_peak} admitted_streams_active={admitted_streams_active} start_rss_bytes={:?} peak_rss_bytes={:?}",
            self.latencies.len(),
            self.successful,
            self.rejected,
            self.p99_latency(),
            self.max_queue_delay(),
            self.start_rss_bytes,
            self.peak_rss_bytes
        );
    }
}

#[derive(Clone, Default)]
struct QueueProbe {
    samples: Arc<AtomicUsize>,
    max_wait_nanos: Arc<AtomicU64>,
}

impl LifecycleObserver for QueueProbe {
    fn on_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        if call.method() == "SayHello" {
            self.samples.fetch_add(1, Ordering::SeqCst);
            self.max_wait_nanos.fetch_max(
                u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX),
                Ordering::SeqCst,
            );
        }
    }
}

impl QueueProbe {
    fn summary(&self) -> (usize, Duration) {
        let count = self.samples.load(Ordering::SeqCst);
        assert!(count > 0, "client queue wait was not observed");
        (
            count,
            Duration::from_nanos(self.max_wait_nanos.load(Ordering::SeqCst)),
        )
    }
}

#[derive(Clone, Default)]
struct PermitProbe {
    active_streams: Arc<AtomicUsize>,
    peak_streams: Arc<AtomicUsize>,
    budget_reader: Arc<OnceLock<Arc<dyn Fn() -> usize + Send + Sync>>>,
    peak_allocated_bytes: Arc<AtomicUsize>,
    concurrency_rejections: Arc<AtomicUsize>,
    unary_payload_bytes: Arc<AtomicUsize>,
    completed_streams: Arc<AtomicUsize>,
    failed_streams: Arc<AtomicUsize>,
    last_stream_failure_code: Arc<AtomicI32>,
    started: Arc<Notify>,
}

impl LifecycleObserver for PermitProbe {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        if matches!(call.method(), "ClientHello" | "ServerHello") {
            let active = self.active_streams.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_streams.fetch_max(active, Ordering::SeqCst);
            self.started.notify_one();
        }
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _latency: Duration) {
        if matches!(call.method(), "ClientHello" | "ServerHello") {
            if status.code() != Code::Ok {
                self.last_stream_failure_code
                    .store(status.code().to_i32(), Ordering::SeqCst);
                self.failed_streams.fetch_add(1, Ordering::SeqCst);
            }
            self.completed_streams.fetch_add(1, Ordering::SeqCst);
            self.active_streams.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        if event.reason == RejectionReason::ConcurrencyLimit {
            self.concurrency_rejections.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn on_bytes_received(&self, call: &CallLabels<'_>, bytes: usize) {
        if call.method() == "SayHello" {
            self.unary_payload_bytes.fetch_add(bytes, Ordering::SeqCst);
        }
    }

    fn on_bytes_sent(&self, _call: &CallLabels<'_>, _bytes: usize) {
        self.sample_budget();
    }
}

impl PermitProbe {
    fn watch_budget(&self, server: &Server<GreeterServer<Echo>>) {
        let tracker = server.byte_budget_tracker().clone();
        assert!(
            self.budget_reader
                .set(Arc::new(move || tracker.allocated()))
                .is_ok(),
            "test budget probe already attached"
        );
    }

    fn sample_budget(&self) {
        let allocated = self
            .budget_reader
            .get()
            .expect("test budget probe must be attached before RPCs")();
        self.peak_allocated_bytes
            .fetch_max(allocated, Ordering::SeqCst);
    }

    fn active(&self) -> usize {
        self.active_streams.load(Ordering::SeqCst)
    }

    async fn wait_for_active(&self, expected: usize) {
        tokio::time::timeout(PROBE_TIMEOUT, async {
            while self.active() < expected {
                self.started.notified().await;
            }
        })
        .await
        .expect("stream never acquired an RPC permit");
    }

    fn assert_streams_ok(&self, count: usize) {
        assert_eq!(
            self.completed_streams.load(Ordering::SeqCst),
            count,
            "missing stream outcomes"
        );
        assert_eq!(
            self.failed_streams.load(Ordering::SeqCst),
            0,
            "accepted streams failed with gRPC code {}",
            self.last_stream_failure_code.load(Ordering::SeqCst)
        );
    }
}

async fn assert_server_quiescent(server: &Server<GreeterServer<Echo>>, permit: &PermitProbe) {
    tokio::time::timeout(PROBE_TIMEOUT, async {
        while !server.is_byte_budget_quiescent() || permit.active() != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("server bytes or admitted stream permits did not drain");
    assert_eq!(server.byte_budget_allocated(), 0);
    assert_eq!(permit.active(), 0);
}

async fn run_small_calls(
    client: &GreeterClient,
    server: &Server<GreeterServer<Echo>>,
    label: &'static str,
    count: usize,
) -> FairnessRecord {
    let mut record = FairnessRecord::new();
    record.sample_resources(server.byte_budget_allocated());
    let first = TokioInstant::now() + Duration::from_millis(20);
    let mut calls = Vec::with_capacity(count);
    for i in 0..count {
        let client = client.clone();
        let server = server.clone();
        let scheduled = first + Duration::from_millis(i as u64 * 2);
        let name = format!("{label}-{i}");
        calls.push(tokio::spawn(async move {
            tokio::time::sleep_until(scheduled).await;
            let lag = TokioInstant::now().saturating_duration_since(scheduled);
            let result =
                tokio::time::timeout(PROBE_TIMEOUT, client.say_hello(Request::new(req(&name))))
                    .await;
            let latency = scheduled.elapsed();
            (name, result, lag, latency, server.byte_budget_allocated())
        }));
    }
    for call in calls {
        let (name, result, lag, latency, allocated) = call.await.expect("competing RPC task");
        match result {
            Ok(Ok(reply)) => {
                assert_eq!(name_of(reply.get_ref()), name, "wrong competing response");
                record.record_success(lag, latency, allocated);
            }
            Ok(Err(status)) => record.record_error(&status, lag, latency, allocated),
            Err(_) => record.record_timeout(lag, latency, allocated),
        }
    }
    record.sample_resources(server.byte_budget_allocated());
    record.assert_complete(count);
    record
}

#[test]
fn fairness_p99_uses_nearest_rank_and_retains_rejections() {
    let mut record = FairnessRecord::new();
    for ms in 1..=98 {
        record.record_success(Duration::ZERO, Duration::from_millis(ms), 0);
    }
    for ms in [100, 101] {
        record.record_error(
            &Status::resource_exhausted("too many concurrent RPCs"),
            Duration::ZERO,
            Duration::from_millis(ms),
            0,
        );
    }
    assert_eq!(record.p99_latency(), Duration::from_millis(100));
    assert_eq!(record.p99_admitted(), Duration::from_millis(98));
    record.assert_complete(100);
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

async fn connect_with_config(addr: SocketAddr, config: ChannelConfig) -> Channel {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect_with(addr, config).await {
            Ok(ch) => return ch,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

async fn connect_client(addr: SocketAddr) -> Channel {
    connect_with_config(addr, ChannelConfig::default()).await
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

/// Deterministic incompressible payload: xorshift bytes over ASCII
/// alphanumerics, so gzip cannot shrink it and protobuf strings stay valid.
fn incompressible_payload(len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut state: u64 = 0x243f_6a88_85a3_08d3;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ALPHABET[(state % ALPHABET.len() as u64) as usize] as char
        })
        .collect()
}

#[tokio::test]
async fn test_mixed_large_small_compressed_byte_budget() {
    // Client budget admits small and gzip-shrunk messages but rejects large
    // wire payloads with explicit errors; a held allocation deterministically
    // blocks new admissions until released (permits guard the microsecond
    // send window, so steady-state sampling cannot observe them reliably).
    const LIMIT: usize = 2048;
    let (addr, server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr)
        .await
        .byte_budget(LIMIT)
        .send_compressed();
    let client = GreeterClient::new(channel.clone());

    let big_compressible = "x".repeat(5000);
    let big_incompressible = incompressible_payload(5000);

    // Sanity: the compressible payload gzips well under budget while the
    // incompressible one does not (flate2 fast, matching channel default).
    {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(big_compressible.as_bytes()).expect("gzip");
        let small = enc.finish().expect("finish");
        assert!(
            small.len() < 128,
            "compressible must shrink: {}",
            small.len()
        );
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(big_incompressible.as_bytes()).expect("gzip");
        let big = enc.finish().expect("finish");
        assert!(
            big.len() > LIMIT,
            "incompressible must stay over budget: {}",
            big.len()
        );
    }

    let mut handles = Vec::new();
    for round in 0..3 {
        // Small plain messages (opt out of channel gzip per request).
        for i in 0..3 {
            let cl = client.clone();
            let name = format!("plain-{round}-{i}");
            handles.push(tokio::spawn(async move {
                let mut call = Request::new(req(&name));
                call.set_compress(false);
                let reply = cl.say_hello(call).await.expect("small plain succeeds");
                assert_eq!(name_of(reply.get_ref()), name);
            }));
        }
        // Small gzip messages (channel default).
        for i in 0..3 {
            let cl = client.clone();
            let name = format!("gzip-{round}-{i}");
            handles.push(tokio::spawn(async move {
                let reply = cl
                    .say_hello(Request::new(req(&name)))
                    .await
                    .expect("small gzip succeeds");
                assert_eq!(name_of(reply.get_ref()), name);
            }));
        }
        // Large but highly compressible: wire bytes fit, so it succeeds even
        // though the uncompressed form (5000+ bytes) exceeds the budget.
        for i in 0..3 {
            let cl = client.clone();
            let body = big_compressible.clone();
            handles.push(tokio::spawn(async move {
                let name = format!("{body}-{i}");
                let reply = cl
                    .say_hello(Request::new(req(&name)))
                    .await
                    .expect("compressible large succeeds");
                assert_eq!(name_of(reply.get_ref()), name);
            }));
        }
        // Large incompressible over gzip: wire bytes exceed the budget.
        for _ in 0..3 {
            let cl = client.clone();
            let body = big_incompressible.clone();
            handles.push(tokio::spawn(async move {
                let err = cl
                    .say_hello(Request::new(req(&body)))
                    .await
                    .expect_err("incompressible large must be rejected");
                assert_eq!(err.code(), Code::ResourceExhausted);
                assert!(
                    err.message().contains("transport byte budget exceeded"),
                    "rejection must cite the byte budget: {err}"
                );
            }));
        }
        // Large plain (gzip opted out): uncompressed wire bytes rejected.
        for _ in 0..3 {
            let cl = client.clone();
            let body = big_compressible.clone();
            handles.push(tokio::spawn(async move {
                let mut call = Request::new(req(&body));
                call.set_compress(false);
                let err = cl
                    .say_hello(call)
                    .await
                    .expect_err("plain large must be rejected");
                assert_eq!(err.code(), Code::ResourceExhausted);
                assert!(
                    err.message().contains("transport byte budget exceeded"),
                    "rejection must cite the byte budget: {err}"
                );
            }));
        }
    }

    for h in handles {
        h.await.expect("load task join");
    }

    // Held in-flight bytes deterministically block new admissions (no silent
    // queueing past the cap), and releasing them re-admits immediately (no
    // deadlock between the budget and the RPC path).
    let hold = channel
        .byte_budget_tracker()
        .acquire(LIMIT)
        .expect("hold full budget");
    assert_eq!(channel.byte_budget_allocated(), LIMIT);
    let err = client
        .say_hello(Request::new(req("blocked")))
        .await
        .expect_err("held budget must block admission");
    assert_eq!(err.code(), Code::ResourceExhausted);
    assert!(
        err.message().contains("transport byte budget exceeded"),
        "rejection must cite the byte budget: {err}"
    );
    drop(hold);
    let reply = client
        .say_hello(Request::new(req("after-release")))
        .await
        .expect("release must re-admit immediately");
    assert_eq!(name_of(reply.get_ref()), "after-release");

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        channel.is_byte_budget_quiescent(),
        "client must quiesce, allocated = {}",
        channel.byte_budget_allocated()
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
    assert!(
        server.is_byte_budget_quiescent(),
        "server must quiesce, allocated = {}",
        server.byte_budget_allocated()
    );
    assert_eq!(server.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn test_encode_error_releases_budget_to_baseline() {
    // Client-side unary encode error: oversize vs max_encoding, rejected
    // before any byte permit is acquired.
    let (addr, _server, _guard) = spawn_budgeted_server(1_000_000).await;
    let channel = connect_client(addr)
        .await
        .byte_budget(64 * 1024)
        .max_encoding_message_size(16);
    let client = GreeterClient::new(channel.clone());

    let err = client
        .say_hello(Request::new(req(&"y".repeat(500))))
        .await
        .expect_err("oversize unary must fail encode");
    assert_eq!(err.code(), Code::ResourceExhausted);
    assert!(
        err.message().contains("encoded message length"),
        "encode error must cite the encoding limit: {err}"
    );
    assert_eq!(channel.byte_budget_allocated(), 0);
    assert!(channel.is_byte_budget_quiescent());

    // Client-side mid-stream encode error: one small item flows, then an
    // oversize item fails fast at enqueue and the error is forwarded through
    // the pump so the call fails; held permits are released on failure.
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("fine")).await.expect("send small");
    let send_err = tx
        .send(req(&"y".repeat(500)))
        .await
        .expect_err("oversize send must fail at enqueue");
    assert_eq!(send_err.code(), Code::ResourceExhausted);
    assert!(
        send_err.message().contains("encoded message length"),
        "enqueue error must cite the encoding limit: {send_err}"
    );
    tx.close();
    let err = call
        .await
        .expect_err("oversize stream item must fail encode");
    assert_eq!(err.code(), Code::ResourceExhausted);
    assert!(
        err.message().contains("encoded message length"),
        "encode error must cite the encoding limit: {err}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(channel.byte_budget_allocated(), 0);
    assert!(channel.is_byte_budget_quiescent());

    // Server-side unary encode error: the echo reply exceeds the server
    // max_encoding cap, answered trailers-only without holding budget.
    let (addr2, server2, _guard2) =
        spawn_custom_server(|s| s.byte_budget(64 * 1024).max_encoding_message_size(16)).await;
    let channel2 = connect_client(addr2).await;
    let client2 = GreeterClient::new(channel2);

    let err = client2
        .say_hello(Request::new(req(&"z".repeat(500))))
        .await
        .expect_err("oversize server reply must fail encode");
    assert_eq!(err.code(), Code::ResourceExhausted);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(server2.byte_budget_allocated(), 0);
    assert!(server2.is_byte_budget_quiescent());

    // Server-side mid-stream encode error: the stream ends truncated with
    // the explicit encode status (a producer failure truncates; it never
    // ships OK trailers over a dropped item). The pre-error item may or may
    // not have been flushed before the failure; either way the terminal
    // status is explicit and the budget drains.
    let mut stream = client2
        .server_hello(Request::new(req(&format!("ok,{}", "z".repeat(500)))))
        .await
        .expect("server stream opens")
        .into_inner();
    let mut saw_ok = false;
    let mut err = None;
    loop {
        match stream.message().await {
            Ok(Some(msg)) => {
                assert!(!saw_ok, "at most one pre-error item");
                assert_eq!(name_of(&msg), "ok");
                saw_ok = true;
            }
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
    assert_eq!(server2.byte_budget_allocated(), 0);
    assert!(server2.is_byte_budget_quiescent());
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
    const BUDGET: usize = 4 * 1024 * 1024;
    const BULK_MESSAGES: usize = 16;
    const BULK_ITEM_BYTES: usize = 64 * 1024;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(8)
            .max_concurrent_rpcs(64)
            .max_send_buffer_size(256 * 1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let bulk_channel = connect_with_config(
        addr,
        ChannelConfig::default()
            .initial_stream_window_size(256 * 1024)
            .initial_connection_window_size(1024 * 1024)
            .max_send_buffer_size(2 * 1024 * 1024),
    )
    .await;
    let bulk_client = GreeterClient::new(bulk_channel);
    let queue = QueueProbe::default();
    let client = GreeterClient::new(connect_client(addr).await.observer(queue.clone()));
    let parts: Vec<String> = (0..BULK_MESSAGES)
        .map(|i| format!("{i:02}-{}", "b".repeat(BULK_ITEM_BYTES - 3)))
        .collect();
    let payload = parts.join(",");
    let bulk_progress = Arc::new(AtomicUsize::new(0));
    let mut bulk_handles = Vec::new();
    for _ in 0..3 {
        let client = bulk_client.clone();
        let parts = parts.clone();
        let payload = payload.clone();
        let progress = bulk_progress.clone();
        bulk_handles.push(tokio::spawn(async move {
            let mut stream = tokio::time::timeout(
                PROBE_TIMEOUT,
                client.server_hello(Request::new(req(&payload))),
            )
            .await
            .expect("bulk stream headers stalled")
            .expect("bulk stream opens")
            .into_inner();
            for (i, expected) in parts.iter().enumerate() {
                let message = tokio::time::timeout(PROBE_TIMEOUT, stream.message())
                    .await
                    .expect("bulk stream stalled")
                    .expect("bulk stream status")
                    .expect("bulk stream truncated");
                assert_eq!(name_of(&message), *expected, "bulk stream item {i}");
                progress.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(8)).await;
            }
            assert!(
                tokio::time::timeout(PROBE_TIMEOUT, stream.message())
                    .await
                    .expect("bulk stream trailers stalled")
                    .expect("bulk trailers")
                    .is_none()
            );
        }));
    }
    permit.wait_for_active(3).await;
    assert_eq!(permit.active(), 3, "bulk streams must overlap");

    let before = bulk_progress.load(Ordering::SeqCst);
    let record = run_small_calls(&client, &server, "small", 32).await;
    let during = bulk_progress.load(Ordering::SeqCst) - before;
    assert_eq!(record.successful, 32, "small RPC starvation: {record:?}");
    assert_eq!(record.rejected, 0, "no unexpected rejections");
    assert!(
        during >= BULK_MESSAGES,
        "only {during} bulk messages arrived during the competing probes"
    );
    record.report("bulk-vs-small", &queue, BUDGET, &permit);
    eprintln!("bulk-vs-small: bulk_messages_during={during}");
    assert!(
        record.p99_latency() < BULK_SMALL_P99,
        "F-2: p99 under bulk load was {:?}",
        record.p99_latency()
    );
    assert!(
        record.max_queue_delay() < BULK_MAX_SCHEDULING_LAG
            && queue.summary().1 < BULK_MAX_SCHEDULING_LAG,
        "F-3: scheduling lag {:?}, client pool wait {:?}",
        record.max_queue_delay(),
        queue.summary().1
    );

    for handle in bulk_handles {
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("bulk stream did not finish")
            .expect("bulk task failed");
    }
    assert_eq!(permit.peak_streams.load(Ordering::SeqCst), 3);
    assert_server_quiescent(&server, &permit).await;
    permit.assert_streams_ok(3);
    client
        .say_hello(Request::new(req("after-bulk")))
        .await
        .expect("post-bulk permit probe");
}

#[tokio::test]
async fn test_slow_reader_peer_isolation() {
    const BUDGET: usize = 2 * 1024 * 1024;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(4)
            .max_concurrent_rpcs(16)
            .max_send_buffer_size(64 * 1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let slow_channel = connect_with_config(
        addr,
        ChannelConfig::default()
            .initial_stream_window_size(64 * 1024)
            .initial_connection_window_size(128 * 1024),
    )
    .await;
    let slow_client = GreeterClient::new(slow_channel);
    let parts: Vec<String> = (0..40)
        .map(|i| format!("{i:02}-{}", "r".repeat(8192 - 3)))
        .collect();
    let mut stream = tokio::time::timeout(
        PROBE_TIMEOUT,
        slow_client.server_hello(Request::new(req(&parts.join(",")))),
    )
    .await
    .expect("slow reader headers stalled")
    .expect("slow reader stream opens")
    .into_inner();
    let first = tokio::time::timeout(PROBE_TIMEOUT, stream.message())
        .await
        .expect("slow reader first message stalled")
        .expect("slow reader first status")
        .expect("slow reader first message");
    assert_eq!(name_of(&first), parts[0]);
    permit.wait_for_active(1).await;

    let queue = QueueProbe::default();
    let fast_client = GreeterClient::new(connect_client(addr).await.observer(queue.clone()));
    let record = run_small_calls(&fast_client, &server, "fast-reader", 32).await;
    assert_eq!(
        record.successful, 32,
        "slow reader starved fast RPCs: {record:?}"
    );
    assert_eq!(permit.peak_streams.load(Ordering::SeqCst), 1);
    assert!(
        server.byte_budget_allocated() <= BUDGET,
        "paused reader exceeded the transport byte budget"
    );
    record.report("slow-reader", &queue, BUDGET, &permit);
    assert!(
        record.p99_latency() < SLOW_PEER_P99,
        "F-4: fast RPC p99 with slow reader was {:?}",
        record.p99_latency()
    );

    tokio::time::timeout(Duration::from_secs(2), async {
        for expected in parts.iter().skip(1) {
            let message = stream
                .message()
                .await
                .expect("slow reader stream status")
                .expect("slow reader lost an accepted message");
            assert_eq!(name_of(&message), *expected);
        }
        assert!(
            stream
                .message()
                .await
                .expect("slow reader trailers")
                .is_none()
        );
    })
    .await
    .expect("slow reader never drained");
    drop(stream);
    assert_server_quiescent(&server, &permit).await;
    permit.assert_streams_ok(1);
    fast_client
        .say_hello(Request::new(req("after-slow-reader")))
        .await
        .expect("post-reader permit probe");
}

#[tokio::test]
async fn test_large_stream_frame_exceeding_send_buffer_does_not_stall() {
    let (addr, _server, _guard) =
        spawn_custom_server(|s| s.max_send_buffer_size(4 * 1024).byte_budget(128 * 1024)).await;
    let channel = connect_with_config(
        addr,
        ChannelConfig::default()
            .initial_stream_window_size(128 * 1024)
            .initial_connection_window_size(256 * 1024),
    )
    .await;
    let client = GreeterClient::new(channel);
    let payload = "x".repeat(64 * 1024);
    let mut stream = client
        .server_hello(Request::new(req(&payload)))
        .await
        .expect("stream headers")
        .into_inner();
    let message = tokio::time::timeout(PROBE_TIMEOUT, stream.message())
        .await
        .expect("one 64 KiB frame stalled against a 4 KiB send buffer")
        .expect("stream status")
        .expect("stream response");
    assert_eq!(name_of(&message), payload);
    assert!(
        tokio::time::timeout(PROBE_TIMEOUT, stream.message())
            .await
            .expect("trailers stalled after the frame")
            .expect("stream trailers")
            .is_none()
    );
}

#[tokio::test]
async fn test_large_stream_frame_with_peer_window_below_send_buffer_completes() {
    const SEND_BUFFER: usize = 4 * 1024;
    const PEER_WINDOW: u32 = 1024;
    const MESSAGE_SIZE: usize = 64 * 1024;
    let (addr, server, _guard) =
        spawn_custom_server(|s| s.max_send_buffer_size(SEND_BUFFER).byte_budget(128 * 1024)).await;
    let payload = incompressible_payload(MESSAGE_SIZE);
    assert!((PEER_WINDOW as usize) < SEND_BUFFER && SEND_BUFFER < payload.len());

    tokio::time::timeout(Duration::from_secs(3), async {
        let channel = connect_with_config(
            addr,
            ChannelConfig::default()
                .initial_stream_window_size(PEER_WINDOW)
                .initial_connection_window_size(64 * 1024),
        )
        .await;
        let mut stream = GreeterClient::new(channel)
            .server_hello(Request::new(req(&payload)))
            .await
            .expect("stream headers")
            .into_inner();
        let reply = stream
            .message()
            .await
            .expect("response status")
            .expect("one response message");
        assert_eq!(name_of(&reply), payload);
        assert!(
            stream.message().await.expect("final gRPC status").is_none(),
            "one response must be followed by OK trailers"
        );
        stream.trailers().await.expect("OK trailers");
    })
    .await
    .expect("64 KiB response stalled with a 4 KiB send buffer and 1 KiB peer window");
    assert!(server.is_byte_budget_quiescent());
}

#[tokio::test]
async fn test_slow_writer_peer_isolation() {
    const BUDGET: usize = 128 * 1024;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(4)
            .max_concurrent_rpcs(16)
            .max_send_buffer_size(32 * 1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let slow_client = GreeterClient::new(connect_client(addr).await);
    let (slow_tx, slow_call) = slow_client.client_hello(Request::new(()));
    let slow_call = tokio::spawn(slow_call);
    slow_tx
        .send(req("slow_start"))
        .await
        .expect("slow writer initial message");
    permit.wait_for_active(1).await;

    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let sent = Arc::new(AtomicUsize::new(0));
    let sent_by_writer = sent.clone();
    let writer = tokio::spawn(async move {
        for i in 0..3 {
            tokio::time::sleep(Duration::from_millis(15)).await;
            slow_tx
                .send(req(&format!("drip-{i}")))
                .await
                .expect("dripped message");
            sent_by_writer.fetch_add(1, Ordering::SeqCst);
        }
        release_rx.await.expect("writer release");
        slow_tx.close();
    });

    let queue = QueueProbe::default();
    let fast_client = GreeterClient::new(connect_client(addr).await.observer(queue.clone()));
    let record = run_small_calls(&fast_client, &server, "fast-writer", 32).await;
    assert_eq!(
        record.successful, 32,
        "slow writer starved fast RPCs: {record:?}"
    );
    assert_eq!(
        sent.load(Ordering::SeqCst),
        3,
        "writer did not drip during probes"
    );
    assert_eq!(permit.active(), 1, "writer must still hold a server permit");
    record.report("slow-writer", &queue, BUDGET, &permit);
    assert!(
        record.p99_latency() < SLOW_PEER_P99,
        "F-4: fast RPC p99 with slow writer was {:?}",
        record.p99_latency()
    );

    release_tx.send(()).expect("release slow writer");
    writer.await.expect("writer task");
    let slow_reply = tokio::time::timeout(PROBE_TIMEOUT, slow_call)
        .await
        .expect("slow writer did not complete")
        .expect("slow writer task")
        .expect("slow writer status");
    assert_eq!(
        name_of(slow_reply.get_ref()),
        "slow_start,drip-0,drip-1,drip-2"
    );
    assert_server_quiescent(&server, &permit).await;
    permit.assert_streams_ok(1);
    fast_client
        .say_hello(Request::new(req("after-slow-writer")))
        .await
        .expect("post-writer permit probe");
}

#[tokio::test]
async fn test_idle_peers_contention_and_fairness() {
    const BUDGET: usize = 128 * 1024;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(12)
            .max_concurrent_rpcs(20)
            .max_send_buffer_size(32 * 1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let mut idle_channels = Vec::new();
    for _ in 0..10 {
        idle_channels.push(connect_client(addr).await);
    }
    assert!(idle_channels.iter().all(Channel::connected));

    let queue = QueueProbe::default();
    let active_channel = connect_client(addr).await.observer(queue.clone());
    let active_client = GreeterClient::new(active_channel);
    let record = run_small_calls(&active_client, &server, "active", 32).await;
    assert_eq!(
        record.successful, 32,
        "idle peers starved active RPCs: {record:?}"
    );
    assert!(idle_channels.iter().all(Channel::connected));
    record.report("idle-peers", &queue, BUDGET, &permit);
    assert!(
        record.p99_latency() < IDLE_PEER_P99,
        "F-6: active RPC p99 with idle peers was {:?}",
        record.p99_latency()
    );

    drop(idle_channels);
    assert_server_quiescent(&server, &permit).await;
    active_client
        .say_hello(Request::new(req("after-idle")))
        .await
        .expect("post-idle permit probe");
}

#[tokio::test]
async fn test_reset_storm_isolation_and_competing_progress() {
    const BUDGET: usize = 128 * 1024;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(8)
            .max_pending_accept_reset_streams(32)
            .max_concurrent_reset_streams(64)
            .max_concurrent_rpcs(20)
            .max_send_buffer_size(32 * 1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let storm_stop = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let storm_handle = {
        let stop = storm_stop.clone();
        let sent = sent.clone();
        tokio::spawn(async move {
            let mut raw = RawPeer::connect(addr).await.expect("storm peer connects");
            let mut started = Some(started_tx);
            let mut reconnects = 0;
            while !stop.load(Ordering::SeqCst) {
                let ready = tokio::time::timeout(PROBE_TIMEOUT, raw.send.clone().ready()).await;
                let mut sender = match ready {
                    Ok(Ok(sender)) => sender,
                    Ok(Err(_)) | Err(_) => {
                        reconnects += 1;
                        assert!(reconnects <= 8, "storm cannot reconnect repeatedly");
                        raw = RawPeer::connect(addr).await.expect("storm peer reconnects");
                        continue;
                    }
                };
                match sender.send_request(raw.request("/helloworld.Greeter/SayHello"), false) {
                    Ok((_response, mut stream)) => {
                        stream.send_reset(h2::Reason::CANCEL);
                        sent.fetch_add(1, Ordering::SeqCst);
                        if let Some(tx) = started.take() {
                            tx.send(()).expect("storm start receiver");
                        }
                    }
                    Err(_) => {
                        reconnects += 1;
                        assert!(reconnects <= 8, "storm connection repeatedly closed");
                        raw = RawPeer::connect(addr).await.expect("storm peer reconnects");
                    }
                }
                tokio::task::yield_now().await;
            }
            reconnects
        })
    };

    tokio::time::timeout(PROBE_TIMEOUT, started_rx)
        .await
        .expect("storm never sent a reset")
        .expect("storm task exited without sending");
    let queue = QueueProbe::default();
    let fast_client = GreeterClient::new(connect_client(addr).await.observer(queue.clone()));
    let before = sent.load(Ordering::SeqCst);
    let record = run_small_calls(&fast_client, &server, "legit", 32).await;
    let during = sent.load(Ordering::SeqCst) - before;
    storm_stop.store(true, Ordering::SeqCst);
    let reconnects = tokio::time::timeout(Duration::from_secs(2), storm_handle)
        .await
        .expect("storm task did not stop")
        .expect("storm task failed");
    assert!(
        during >= 16,
        "storm sent only {during} resets during probes"
    );
    assert_eq!(
        record.successful, 32,
        "RST storm starved valid RPCs: {record:?}"
    );
    record.report("reset-storm", &queue, BUDGET, &permit);
    eprintln!("reset-storm: resets_during={during} reconnects={reconnects}");
    assert!(
        record.p99_latency() < RESET_STORM_P99,
        "F-5: legitimate RPC p99 during RST storm was {:?}",
        record.p99_latency()
    );

    assert_server_quiescent(&server, &permit).await;
    fast_client
        .say_hello(Request::new(req("after-reset-storm")))
        .await
        .expect("post-reset permit probe");
}

#[tokio::test]
async fn test_overload_explicit_status_rejection_no_silent_buffering() {
    const BUDGET: usize = 2048;
    const REJECTED: usize = 24;
    let permit = PermitProbe::default();
    let (addr, server, _guard) = spawn_custom_server(|s| {
        s.max_concurrent_connections(4)
            .max_concurrent_rpcs(1)
            .max_send_buffer_size(1024)
            .byte_budget(BUDGET)
            .observer(permit.clone())
    })
    .await;
    assert_eq!(server.byte_budget_limit(), Some(BUDGET));
    permit.watch_budget(&server);

    let holder = GreeterClient::new(connect_client(addr).await);
    let (tx, call) = holder.client_hello(Request::new(()));
    let held_call = tokio::spawn(call);
    tx.send(req("holding-slot")).await.expect("start held call");
    permit.wait_for_active(1).await;
    assert_eq!(permit.peak_streams.load(Ordering::SeqCst), 1);

    let queue = QueueProbe::default();
    let client = GreeterClient::new(connect_client(addr).await.observer(queue.clone()));
    let mut record = FairnessRecord::new();
    let scheduled = TokioInstant::now() + Duration::from_millis(20);
    let mut calls = Vec::new();
    for i in 0..REJECTED {
        let client = client.clone();
        let server = server.clone();
        calls.push(tokio::spawn(async move {
            tokio::time::sleep_until(scheduled).await;
            let lag = TokioInstant::now().saturating_duration_since(scheduled);
            let result = tokio::time::timeout(
                PROBE_TIMEOUT,
                client.say_hello(Request::new(req(&format!("overload-{i}")))),
            )
            .await;
            (
                result,
                lag,
                scheduled.elapsed(),
                server.byte_budget_allocated(),
            )
        }));
    }
    for call in calls {
        let (result, lag, latency, allocated) = call.await.expect("overload task");
        match result {
            Ok(Ok(_)) => record.record_success(lag, latency, allocated),
            Ok(Err(status)) => {
                assert_eq!(status.code(), Code::ResourceExhausted, "{status}");
                assert!(
                    status.message().contains("too many concurrent RPCs"),
                    "server rejection must name the exhausted permit: {status}"
                );
                record.record_error(&status, lag, latency, allocated);
            }
            Err(_) => record.record_timeout(lag, latency, allocated),
        }
    }
    record.sample_resources(server.byte_budget_allocated());
    record.assert_complete(REJECTED);
    record.report("server-overload", &queue, BUDGET, &permit);
    assert_eq!(record.successful, 0, "no slot was free: {record:?}");
    assert_eq!(
        record.rejected, REJECTED,
        "no offered call may silently wait"
    );
    assert_eq!(
        permit.concurrency_rejections.load(Ordering::SeqCst),
        REJECTED,
        "all rejections must come from server admission"
    );
    assert_eq!(
        permit.unary_payload_bytes.load(Ordering::SeqCst),
        0,
        "rejected request payloads reached the unary handler"
    );
    assert_eq!(permit.active(), 1, "held call must survive rejected burst");
    assert!(
        record.p99_latency() < OVERLOAD_P99,
        "O-1: rejected calls were silently queued: p99 {:?}",
        record.p99_latency()
    );

    tx.close();
    let reply = tokio::time::timeout(PROBE_TIMEOUT, held_call)
        .await
        .expect("held call did not finish")
        .expect("held task")
        .expect("held call status");
    assert_eq!(name_of(reply.get_ref()), "holding-slot");
    assert_server_quiescent(&server, &permit).await;

    let recovered_queue = QueueProbe::default();
    let recovered =
        GreeterClient::new(connect_client(addr).await.observer(recovered_queue.clone()));
    let mut admitted = FairnessRecord::new();
    for i in 0..8 {
        let scheduled = TokioInstant::now();
        let lag = TokioInstant::now().saturating_duration_since(scheduled);
        let result = tokio::time::timeout(
            PROBE_TIMEOUT,
            recovered.say_hello(Request::new(req(&format!("recovered-{i}")))),
        )
        .await;
        let latency = scheduled.elapsed();
        match result {
            Ok(Ok(reply)) => {
                assert_eq!(name_of(reply.get_ref()), format!("recovered-{i}"));
                admitted.record_success(lag, latency, server.byte_budget_allocated());
            }
            Ok(Err(status)) => {
                admitted.record_error(&status, lag, latency, server.byte_budget_allocated())
            }
            Err(_) => admitted.record_timeout(lag, latency, server.byte_budget_allocated()),
        }
    }
    admitted.sample_resources(server.byte_budget_allocated());
    admitted.assert_complete(8);
    admitted.report("post-overload-admitted", &recovered_queue, BUDGET, &permit);
    assert_eq!(admitted.successful, 8, "admitted calls did not recover");
    assert!(
        admitted.p99_admitted() < OVERLOAD_P99,
        "O-2: admitted p99 after overload was {:?}",
        admitted.p99_admitted()
    );

    let hold = server
        .byte_budget_tracker()
        .acquire(BUDGET)
        .expect("hold complete server byte budget");
    permit.sample_budget();
    assert_eq!(
        permit.peak_allocated_bytes.load(Ordering::SeqCst),
        BUDGET,
        "full budget was not recorded"
    );
    let start = Instant::now();
    let err = recovered
        .say_hello(Request::new(req("byte-overload")))
        .await
        .expect_err("held byte budget must reject, not buffer");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    assert!(err.message().contains("transport byte budget exceeded"));
    assert!(start.elapsed() < OVERLOAD_P99, "byte rejection was delayed");
    assert_eq!(server.byte_budget_allocated(), BUDGET);
    drop(hold);
    recovered
        .say_hello(Request::new(req("after-byte-release")))
        .await
        .expect("byte budget re-admits");

    let limited_channel = connect_client(addr)
        .await
        .max_concurrent_rpcs(1)
        .byte_budget(BUDGET);
    let limited = GreeterClient::new(limited_channel.clone());
    let (tx, call) = limited.client_hello(Request::new(()));
    let held_client_call = tokio::spawn(call);
    tx.send(req("client-held")).await.expect("hold client slot");
    permit.wait_for_active(1).await;
    let server_rejections = permit.concurrency_rejections.load(Ordering::SeqCst);
    let err = limited
        .say_hello(Request::new(req("client-overload")))
        .await
        .expect_err("client must reject before opening a stream");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    assert!(err.message().contains("too many concurrent RPCs"));
    assert_eq!(
        permit.concurrency_rejections.load(Ordering::SeqCst),
        server_rejections,
        "client rejection was sent to the server"
    );
    tx.close();
    tokio::time::timeout(PROBE_TIMEOUT, held_client_call)
        .await
        .expect("client-held call did not finish")
        .expect("client-held task")
        .expect("client-held call status");
    limited
        .say_hello(Request::new(req("after-client-release")))
        .await
        .expect("client permit re-admits");
    assert_server_quiescent(&server, &permit).await;
    assert_eq!(limited_channel.byte_budget_allocated(), 0);
    permit.assert_streams_ok(2);
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
