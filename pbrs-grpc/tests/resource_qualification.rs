//! Reproducible current-h2 diagnostics; the ignored scenario is not a production soak.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "resource qualification integration tests"
)]

mod common;

use common::{Echo, name_of, req};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{
    ByteBudgetTracker, CallLabels, Channel, ChannelConfig, Code, LifecycleObserver, Request,
    Server, ServerConfig, Status,
};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const SEND_BUFFER: usize = 16 * 1024;
const BYTE_BUDGET: usize = 256 * 1024;
const RSS_RECOVERY_TOLERANCE: u64 = 32 * 1024 * 1024;

#[test]
fn server_default_and_explicit_default_have_distinct_budget_policy() {
    let default = Server::new(GreeterServer::new(Echo));
    let configured = Server::new(GreeterServer::new(Echo)).config(ServerConfig::default());
    let explicit = Server::new(GreeterServer::new(Echo))
        .max_send_buffer_size(ServerConfig::default().send_buffer_size());
    assert_eq!(default.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(configured.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(default.byte_budget_limit(), None);
    assert_eq!(configured.byte_budget_limit(), None);
    assert_eq!(explicit.byte_budget_limit(), Some(1024 * 1024));
}

#[test]
fn server_builder_order_controls_shared_tracker_identity() {
    let tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let permit = tracker.try_acquire(7).expect("tracker permit");
    let config = ServerConfig::default().max_send_buffer_size(SEND_BUFFER);
    let replaced = Server::new(GreeterServer::new(Echo))
        .with_byte_budget_tracker(tracker.clone())
        .config(config);
    let preserved = Server::new(GreeterServer::new(Echo))
        .config(config)
        .with_byte_budget_tracker(tracker.clone());
    assert_eq!(replaced.byte_budget_limit(), Some(SEND_BUFFER));
    assert_eq!(replaced.byte_budget_allocated(), 0);
    assert_eq!(preserved.byte_budget_limit(), Some(BYTE_BUDGET));
    assert_eq!(preserved.byte_budget_allocated(), 7);
    let default_config_preserves = Server::new(GreeterServer::new(Echo))
        .with_byte_budget_tracker(tracker)
        .config(ServerConfig::default());
    assert_eq!(default_config_preserves.byte_budget_allocated(), 7);
    drop(permit);
    assert_eq!(preserved.byte_budget_allocated(), 0);
}

#[test]
fn router_has_the_same_coupled_budget_and_ordering_policy() {
    let default = Server::new(GreeterServer::new(Echo)).into_router();
    let explicit = Server::new(GreeterServer::new(Echo))
        .into_router()
        .max_send_buffer_size(ServerConfig::default().send_buffer_size());
    assert_eq!(default.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(default.byte_budget_limit(), None);
    assert_eq!(explicit.byte_budget_limit(), Some(1024 * 1024));
    let tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let permit = tracker.try_acquire(11).expect("tracker permit");
    let config = ServerConfig::default().max_send_buffer_size(SEND_BUFFER);
    let replaced = Server::new(GreeterServer::new(Echo))
        .into_router()
        .with_byte_budget_tracker(tracker.clone())
        .config(config);
    let preserved = Server::new(GreeterServer::new(Echo))
        .into_router()
        .config(config)
        .with_byte_budget_tracker(tracker);
    assert_eq!(replaced.byte_budget_allocated(), 0);
    assert_eq!(replaced.byte_budget_limit(), Some(SEND_BUFFER));
    assert_eq!(preserved.byte_budget_allocated(), 11);
    assert_eq!(preserved.byte_budget_limit(), Some(BYTE_BUDGET));
    drop(permit);
}

#[derive(Clone, Default)]
struct Calls {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    started: Arc<AtomicUsize>,
    ended: Arc<AtomicUsize>,
}

impl LifecycleObserver for Calls {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        if !matches!(call.method(), "ClientHello" | "ServerHello") {
            return;
        }
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        self.started.fetch_add(1, Ordering::SeqCst);
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, _status: &Status, _latency: Duration) {
        if !matches!(call.method(), "ClientHello" | "ServerHello") {
            return;
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.ended.fetch_add(1, Ordering::SeqCst);
    }
}

fn linux_memory(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .expect("Linux process status")
        .lines()
        .find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .expect("required memory counter")
        * 1024
}

fn entry_count(path: &str) -> usize {
    std::fs::read_dir(path)
        .expect("Linux process accounting")
        .count()
}

fn snapshot(
    phase: &str,
    cycle: u64,
    calls: &Calls,
    server: &ByteBudgetTracker,
    client: &ByteBudgetTracker,
) -> serde_json::Value {
    json!({
        "phase": phase, "cycle": cycle,
        "rss_bytes": linux_memory("VmRSS:"), "rss_hwm_bytes": linux_memory("VmHWM:"),
        "file_descriptors": entry_count("/proc/self/fd"),
        "os_threads": entry_count("/proc/self/task"),
        "tokio_alive_tasks": tokio::runtime::Handle::current().metrics().num_alive_tasks(),
        "observed_streaming_calls_active": calls.active.load(Ordering::SeqCst),
        "observed_streaming_calls_peak": calls.peak.load(Ordering::SeqCst),
        "observed_streaming_calls_started": calls.started.load(Ordering::SeqCst),
        "observed_streaming_calls_ended": calls.ended.load(Ordering::SeqCst),
        "server_allocated_bytes": server.allocated(), "client_allocated_bytes": client.allocated(),
        "server_byte_peak": server.peak_allocated(), "client_byte_peak": client.peak_allocated(),
        "server_byte_tokens": server.active_byte_permit_tokens(),
        "client_byte_tokens": client.active_byte_permit_tokens(),
        "server_token_peak": server.peak_active_byte_permit_tokens(),
        "client_token_peak": client.peak_active_byte_permit_tokens(),
    })
}

fn record(value: &serde_json::Value) {
    let path = std::env::var("PBRS_CURRENT_H2_EVENTS").expect("runner supplies event path");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("events");
    writeln!(file, "{value}").expect("write resource event");
}

async fn wait_for_idle(calls: &Calls, server: &ByteBudgetTracker, client: &ByteBudgetTracker) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if calls.active.load(Ordering::SeqCst) == 0
                && server.allocated() == 0
                && client.allocated() == 0
                && server.active_byte_permit_tokens() == 0
                && client.active_byte_permit_tokens() == 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("calls and byte permits must return to zero");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "use scripts/current-h2-soak.py to freeze source and finite process limits"]
async fn current_h2_resource_smoke() {
    if !cfg!(target_os = "linux") {
        panic!("qualification accounting requires Linux");
    }
    let duration = std::env::var("PBRS_CURRENT_H2_SECONDS")
        .expect("runner duration")
        .parse::<u64>()
        .expect("integer duration");
    let seed = std::env::var("PBRS_CURRENT_H2_SEED")
        .expect("runner seed")
        .parse::<u64>()
        .expect("integer seed");
    let started = Instant::now();
    let calls = Calls::default();
    let empty = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let baseline = snapshot("baseline", 0, &calls, &empty, &empty);
    record(&baseline);
    let mut cycle = 0;
    while cycle == 0 || started.elapsed() < Duration::from_secs(duration) {
        cycle += 1;
        let server_tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
        let client_tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
        let server = Server::new(GreeterServer::new(Echo))
            .max_concurrent_connections(4)
            .max_concurrent_streams(8)
            .max_concurrent_rpcs(2)
            .max_decoding_message_size(64 * 1024)
            .max_encoding_message_size(64 * 1024)
            .max_send_buffer_size(SEND_BUFFER)
            .timeout(Duration::from_millis(300))
            .max_connection_age_grace(Duration::from_millis(150))
            .with_byte_budget_tracker(server_tracker.clone())
            .observer(calls.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(async move {
            server
                .serve_with_shutdown(listener, async {
                    stop_rx.await.expect("shutdown signal");
                })
                .await
        });
        let channel = Channel::connect_with(
            addr,
            ChannelConfig::default()
                .stream_buffer(1)
                .initial_stream_window_size(1024)
                .initial_connection_window_size(4096),
        )
        .await
        .expect("connect")
        .max_decoding_message_size(64 * 1024)
        .max_encoding_message_size(64 * 1024)
        .max_send_buffer_size(SEND_BUFFER)
        .with_byte_budget_tracker(client_tracker.clone());
        let client = GreeterClient::new(channel.clone());
        let label = format!("seed-{seed}-cycle-{cycle}");
        let reply = client
            .say_hello(Request::new(req(&label)))
            .await
            .expect("warmup");
        assert_eq!(name_of(reply.get_ref()), label);
        record(&snapshot(
            "warmup",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));

        // Deliberately hold the consumer without reading; then validate every response.
        let part = "s".repeat(2048);
        let payload = std::iter::repeat_n(part.as_str(), 16)
            .collect::<Vec<_>>()
            .join(",");
        let mut slow = client
            .server_hello(Request::new(req(&payload)))
            .await
            .expect("slow reader")
            .into_inner();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            calls.active.load(Ordering::SeqCst),
            1,
            "slow consumer must hold the response stream open"
        );
        record(&snapshot(
            "slow_reader",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));
        let mut received = 0;
        while let Some(reply) = slow.message().await.expect("stream response") {
            assert_eq!(name_of(&reply), part);
            received += 1;
        }
        assert_eq!(received, 16);
        drop(slow);
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;

        let (tx1, future1) = client.client_hello(Request::new(()));
        let (tx2, future2) = client.client_hello(Request::new(()));
        let first = tokio::spawn(future1);
        let second = tokio::spawn(future2);
        tx1.send(req("held-1")).await.expect("first upload");
        tx2.send(req("held-2")).await.expect("second upload");
        tokio::time::timeout(Duration::from_secs(1), async {
            while calls.active.load(Ordering::SeqCst) != 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("both calls admitted");
        let rejected = client
            .say_hello(Request::new(req("overload")))
            .await
            .expect_err("server cap");
        assert_eq!(rejected.code(), Code::ResourceExhausted);
        record(&snapshot(
            "overload",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));
        first.abort();
        second.abort();
        drop(tx1);
        drop(tx2);
        assert!(first.await.expect_err("first cancelled").is_cancelled());
        assert!(second.await.expect_err("second cancelled").is_cancelled());
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;
        record(&snapshot(
            "cancelled",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));

        let (deadline_tx, deadline) = client.client_hello(Request::new(()));
        deadline_tx
            .send(req("unending"))
            .await
            .expect("deadline upload");
        let expired = tokio::time::timeout(Duration::from_secs(2), deadline)
            .await
            .expect("deadline bounded")
            .expect_err("server deadline");
        assert_eq!(expired.code(), Code::DeadlineExceeded);
        drop(deadline_tx);
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;
        record(&snapshot(
            "deadline",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));
        let reply = client
            .say_hello(Request::new(req("recovered")))
            .await
            .expect("recovery probe");
        assert_eq!(name_of(reply.get_ref()), "recovered");
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;
        record(&snapshot(
            "recovered",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));
        drop(client);
        drop(channel);
        stop_tx.send(()).expect("shutdown signal");
        tokio::time::timeout(Duration::from_secs(2), serving)
            .await
            .expect("bounded drain")
            .expect("server join")
            .expect("server drain");
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        let drain = snapshot("drain", cycle, &calls, &server_tracker, &client_tracker);
        record(&drain);
        assert_eq!(
            calls.started.load(Ordering::SeqCst),
            calls.ended.load(Ordering::SeqCst)
        );
        assert!(server_tracker.peak_allocated() <= BYTE_BUDGET);
        assert!(client_tracker.peak_allocated() <= BYTE_BUDGET);
        assert!(
            drain["rss_bytes"].as_u64().expect("rss")
                <= baseline["rss_bytes"].as_u64().expect("rss") + RSS_RECOVERY_TOLERANCE
        );
        assert!(
            drain["file_descriptors"].as_u64().expect("fds")
                <= baseline["file_descriptors"].as_u64().expect("fds") + 1
        );
        assert!(
            drain["tokio_alive_tasks"].as_u64().expect("tasks")
                <= baseline["tokio_alive_tasks"].as_u64().expect("tasks") + 2
        );
    }
}
