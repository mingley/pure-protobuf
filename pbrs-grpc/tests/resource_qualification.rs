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

use common::{Echo, name_of, reply, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    ByteBudgetTracker, CallLabels, Channel, ChannelConfig, Code, LifecycleObserver, Request,
    Response, Server, ServerConfig, Status, Streaming,
};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const SEND_BUFFER: usize = 16 * 1024;
const BYTE_BUDGET: usize = 256 * 1024;
const RSS_RECOVERY_TOLERANCE: u64 = 32 * 1024 * 1024;

struct FlowControlledEcho {
    sent: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
    payload: String,
}

impl Greeter for FlowControlledEcho {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Echo.say_hello(request).await
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Echo.client_hello(request).await
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let (sender, stream) = Streaming::channel(4);
        let sent = self.sent.clone();
        let done = self.done.clone();
        let payload = self.payload.clone();
        drop(tokio::spawn(async move {
            let response = reply(payload);
            for _ in 0..128 {
                if sender.send(response.clone()).await.is_err() {
                    break;
                }
                sent.fetch_add(1, Ordering::SeqCst);
            }
            done.store(true, Ordering::SeqCst);
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Echo.stream_hello(request).await
    }
}

#[test]
fn server_default_and_explicit_default_have_the_same_budget_policy() {
    let default = Server::new(GreeterServer::new(Echo));
    let configured = Server::new(GreeterServer::new(Echo)).config(ServerConfig::default());
    let explicit = Server::new(GreeterServer::new(Echo))
        .max_send_buffer_size(ServerConfig::default().send_buffer_size());
    assert_eq!(default.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(configured.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(default.byte_budget_limit(), None);
    assert_eq!(configured.byte_budget_limit(), None);
    assert_eq!(explicit.byte_budget_limit(), None);
}

#[test]
fn server_transport_configuration_preserves_shared_tracker_identity() {
    let tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let permit = tracker.try_acquire(7).expect("tracker permit");
    let config = ServerConfig::default().max_send_buffer_size(SEND_BUFFER);
    let configured_after = Server::new(GreeterServer::new(Echo))
        .with_byte_budget_tracker(tracker.clone())
        .config(config);
    let preserved = Server::new(GreeterServer::new(Echo))
        .config(config)
        .with_byte_budget_tracker(tracker.clone());
    assert_eq!(configured_after.byte_budget_limit(), Some(BYTE_BUDGET));
    assert_eq!(configured_after.byte_budget_allocated(), 7);
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
fn router_transport_configuration_preserves_shared_tracker_identity() {
    let default = Server::new(GreeterServer::new(Echo)).into_router();
    let explicit = Server::new(GreeterServer::new(Echo))
        .into_router()
        .max_send_buffer_size(ServerConfig::default().send_buffer_size());
    assert_eq!(default.send_buffer_size(), explicit.send_buffer_size());
    assert_eq!(default.byte_budget_limit(), None);
    assert_eq!(explicit.byte_budget_limit(), None);
    let tracker = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let permit = tracker.try_acquire(11).expect("tracker permit");
    let config = ServerConfig::default().max_send_buffer_size(SEND_BUFFER);
    let configured_after = Server::new(GreeterServer::new(Echo))
        .into_router()
        .with_byte_budget_tracker(tracker.clone())
        .config(config);
    let preserved = Server::new(GreeterServer::new(Echo))
        .into_router()
        .config(config)
        .with_byte_budget_tracker(tracker);
    assert_eq!(configured_after.byte_budget_allocated(), 11);
    assert_eq!(configured_after.byte_budget_limit(), Some(BYTE_BUDGET));
    assert_eq!(preserved.byte_budget_allocated(), 11);
    assert_eq!(preserved.byte_budget_limit(), Some(BYTE_BUDGET));
    drop(permit);
}

#[tokio::test]
async fn small_send_buffers_admit_larger_messages_without_an_implicit_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let server = Server::new(GreeterServer::new(Echo))
        .max_send_buffer_size(1024)
        .into_router();
    assert_eq!(server.byte_budget_limit(), None);
    let serving = tokio::spawn(async move { server.serve_listener(listener).await });
    let channel = Channel::connect_with(
        addr,
        ChannelConfig::default()
            .max_send_buffer_size(1024)
            .initial_stream_window_size(1024),
    )
    .await
    .expect("connect");
    assert_eq!(channel.byte_budget_limit(), None);
    let tracker = ByteBudgetTracker::with_limit(2 * 1024 * 1024);
    let held = tracker.try_acquire(11).expect("held shared permit");
    let channel = channel
        .with_byte_budget_tracker(tracker.clone())
        .max_send_buffer_size(2048);
    assert_eq!(channel.byte_budget_limit(), Some(2 * 1024 * 1024));
    assert_eq!(channel.byte_budget_allocated(), 11);
    let client = GreeterClient::new(channel);
    let payload = "x".repeat(64 * 1024);
    tokio::time::timeout(Duration::from_secs(5), async {
        let response = client
            .say_hello(Request::new(req(&payload)))
            .await
            .expect("unary");
        assert_eq!(name_of(response.get_ref()), payload);
        let mut stream = client
            .server_hello(Request::new(req(&payload)))
            .await
            .expect("server stream")
            .into_inner();
        assert_eq!(
            name_of(&stream.message().await.expect("status").expect("reply")),
            payload
        );
        assert!(stream.message().await.expect("final status").is_none());
        let (tx, call) = client.client_hello(Request::new(()));
        let (sent, response) = tokio::join!(
            async {
                tx.send(req(&payload)).await.expect("upload");
                tx.close();
            },
            call
        );
        let () = sent;
        assert_eq!(name_of(response.expect("client stream").get_ref()), payload);
        let (tx, call) = client.stream_hello(Request::new(()));
        let mut inbound = call.await.expect("bidi").into_inner();
        tx.send(req(&payload)).await.expect("bidi send");
        assert_eq!(
            name_of(&inbound.message().await.expect("status").expect("reply")),
            payload
        );
        tx.close();
        assert!(inbound.message().await.expect("final status").is_none());
    })
    .await
    .expect("large frames must make progress with small send buffers");
    drop(client);
    drop(held);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !tracker.is_quiescent() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shared tracker recovered");
    serving.abort();
    assert!(serving.await.expect_err("cancel server").is_cancelled());
}

#[tokio::test]
async fn gzip_small_batches_stop_a_producer_until_the_reader_resumes() {
    let sent = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let mut state = 20261008_u64;
    let payload: String = (0..2048)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(b'0' + u8::try_from(state % 64).expect("ASCII range"))
        })
        .collect();
    // With four producer slots, each gzip batch is below the 16 KiB send threshold.
    assert!(
        pbrs_grpc::gzip::encode(payload.as_bytes())
            .expect("gzip")
            .len()
            * 8
            < SEND_BUFFER
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let server = Server::new(GreeterServer::new(FlowControlledEcho {
        sent: sent.clone(),
        done: done.clone(),
        payload: payload.clone(),
    }))
    .max_send_buffer_size(SEND_BUFFER)
    .send_compressed();
    let serving = tokio::spawn(async move { server.serve_listener(listener).await });
    let channel = Channel::connect_with(
        addr,
        ChannelConfig::default()
            .initial_stream_window_size(1024)
            .initial_connection_window_size(4096),
    )
    .await
    .expect("connect")
    .send_compressed();
    let client = GreeterClient::new(channel);
    let response = client
        .server_hello(Request::new(req("paused")))
        .await
        .expect("headers");
    assert_eq!(response.encoding(), Some("gzip"));
    let mut inbound = response.into_inner();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = sent.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        before > 0 && before < 128,
        "the paused reader must stop the producer"
    );
    assert_eq!(sent.load(Ordering::SeqCst), before);
    assert!(!done.load(Ordering::SeqCst));
    tokio::time::timeout(Duration::from_secs(3), async {
        for _ in 0..128 {
            assert_eq!(
                name_of(&inbound.message().await.expect("status").expect("reply")),
                payload
            );
        }
        assert!(inbound.message().await.expect("final status").is_none());
    })
    .await
    .expect("resumed reader drains every reply");
    assert_eq!(sent.load(Ordering::SeqCst), 128);
    assert!(done.load(Ordering::SeqCst));
    drop(inbound);
    drop(client);
    serving.abort();
    assert!(serving.await.expect_err("cancel server").is_cancelled());
}

#[derive(Clone, Default)]
struct Calls {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    started: Arc<AtomicUsize>,
    ended: Arc<AtomicUsize>,
    producer_sent: Arc<AtomicUsize>,
    producer_done: Arc<AtomicBool>,
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
        "producer_sent_messages": calls.producer_sent.load(Ordering::SeqCst),
        "producer_done": calls.producer_done.load(Ordering::SeqCst),
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
    run_resource_cycles(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "use scripts/grpc-resource-campaign.py for frozen source and process limits"]
async fn current_h2_resource_campaign() {
    run_resource_cycles(true).await;
}

async fn run_resource_cycles(campaign: bool) {
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
    let budget = if campaign {
        8 * 1024 * 1024
    } else {
        BYTE_BUDGET
    };
    let max_message = if campaign { 2 * 1024 * 1024 } else { 64 * 1024 };
    let server_deadline = if campaign { 3000 } else { 300 };
    let mut noise_state = seed | 1;
    let slow_payload: String = if campaign {
        (0..2048)
            .map(|_| {
                noise_state ^= noise_state << 13;
                noise_state ^= noise_state >> 7;
                noise_state ^= noise_state << 17;
                char::from(b'0' + u8::try_from(noise_state % 64).expect("ASCII range"))
            })
            .collect()
    } else {
        "s".repeat(2048)
    };
    let calls = Calls::default();
    let empty = ByteBudgetTracker::with_limit(BYTE_BUDGET);
    let baseline = snapshot("baseline", 0, &calls, &empty, &empty);
    record(&baseline);
    let mut cycle = 0;
    while cycle == 0 || started.elapsed() < Duration::from_secs(duration) {
        cycle += 1;
        calls.producer_sent.store(0, Ordering::SeqCst);
        calls.producer_done.store(false, Ordering::SeqCst);
        let tls_enabled = campaign && (cycle - 1) % 4 >= 2;
        let gzip = campaign && cycle % 2 == 0;
        let server_tracker = ByteBudgetTracker::with_limit(budget);
        let client_tracker = ByteBudgetTracker::with_limit(budget);
        let server = Server::new(GreeterServer::new(FlowControlledEcho {
            sent: calls.producer_sent.clone(),
            done: calls.producer_done.clone(),
            payload: slow_payload.clone(),
        }))
        .max_concurrent_connections(4)
        .max_concurrent_streams(8)
        .max_concurrent_rpcs(2)
        .max_decoding_message_size(max_message)
        .max_encoding_message_size(max_message)
        .max_send_buffer_size(SEND_BUFFER)
        .timeout(Duration::from_millis(server_deadline))
        .max_connection_age_grace(Duration::from_millis(150))
        .with_byte_budget_tracker(server_tracker.clone())
        .observer(calls.clone());
        let server = if gzip {
            server.send_compressed()
        } else {
            server
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(async move {
            let shutdown = async {
                stop_rx.await.expect("shutdown signal");
            };
            if tls_enabled {
                let identity = pbrs_grpc::Identity::from_pem(
                    include_str!("tls_data/server.crt"),
                    include_str!("tls_data/server.key"),
                )
                .expect("server identity");
                let tls = pbrs_grpc::ServerTls::new(identity).expect("server TLS");
                server
                    .serve_tls_with_shutdown(listener, shutdown, tls)
                    .await
            } else {
                server.serve_with_shutdown(listener, shutdown).await
            }
        });
        let config = ChannelConfig::default()
            .stream_buffer(1)
            .initial_stream_window_size(1024)
            .initial_connection_window_size(4096);
        let channel = if tls_enabled {
            Channel::connect_tls_with(
                addr,
                config,
                pbrs_grpc::ClientTls::ca("localhost", include_str!("tls_data/ca.crt"))
                    .expect("client TLS"),
            )
            .await
            .expect("TLS connect")
        } else {
            Channel::connect_with(addr, config).await.expect("connect")
        }
        .max_decoding_message_size(max_message)
        .max_encoding_message_size(max_message)
        .max_send_buffer_size(SEND_BUFFER)
        .with_byte_budget_tracker(client_tracker.clone());
        let channel = if gzip {
            channel.send_compressed()
        } else {
            channel
        };
        let client = GreeterClient::new(channel.clone());
        let label = format!("seed-{seed}-cycle-{cycle}");
        let reply = client
            .say_hello(Request::new(req(&label)))
            .await
            .expect("warmup");
        assert_eq!(name_of(reply.get_ref()), label);
        if campaign {
            // Both directions carry all bytes, including gzip inflation to 1 MiB.
            for size in [0, 1024, 65536, 1048576] {
                let payload = "x".repeat(size);
                let response = client
                    .say_hello(Request::new(req(&payload)))
                    .await
                    .expect("mixed unary");
                assert_eq!(name_of(response.get_ref()), payload);
                let (tx, call) = client.stream_hello(Request::new(()));
                let mut inbound = call.await.expect("mixed bidi").into_inner();
                tx.send(req(&payload)).await.expect("mixed bidi send");
                assert_eq!(
                    name_of(&inbound.message().await.expect("status").expect("reply")),
                    payload
                );
                tx.close();
                assert!(inbound.message().await.expect("final status").is_none());
            }
            record(
                &json!({"phase": "profile", "cycle": cycle, "tls": tls_enabled, "gzip": gzip,
                "mixed_payload_bytes": [0, 1024, 65536, 1048576], "byte_budget": budget,
                "message_limit": max_message, "server_deadline_ms": server_deadline}),
            );
        }
        record(&snapshot(
            "warmup",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        ));

        // Deliberately hold the consumer without reading; then validate every response.
        let part = slow_payload.clone();
        let mut slow = client
            .server_hello(Request::new(req("slow-reader")))
            .await
            .expect("slow reader")
            .into_inner();
        let stall_started = Instant::now();
        let (first_progress, second_progress) = if campaign {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let before = calls.producer_sent.load(Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let after = calls.producer_sent.load(Ordering::SeqCst);
                    if before > 0
                        && before == after
                        && after < 128
                        && !calls.producer_done.load(Ordering::SeqCst)
                    {
                        break (before, after);
                    }
                }
            })
            .await
            .expect("bounded slow-reader stall")
        } else {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let first = calls.producer_sent.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            (first, calls.producer_sent.load(Ordering::SeqCst))
        };
        let mut slow_event = snapshot(
            "slow_reader",
            cycle,
            &calls,
            &server_tracker,
            &client_tracker,
        );
        if campaign {
            slow_event.as_object_mut().expect("event object").insert(
                "stall_wait_ms".into(),
                json!(stall_started.elapsed().as_millis()),
            );
        }
        slow_event.as_object_mut().expect("event object").insert(
            if campaign {
                "producer_progress_before_hold"
            } else {
                "producer_progress_after_30_ms"
            }
            .into(),
            json!(first_progress),
        );
        slow_event.as_object_mut().expect("event object").insert(
            if campaign {
                "producer_progress_after_hold"
            } else {
                "producer_progress_after_60_ms"
            }
            .into(),
            json!(second_progress),
        );
        record(&slow_event);
        assert!(
            first_progress > 0 && second_progress < 128,
            "producer must be underway and unfinished"
        );
        assert_eq!(
            first_progress, second_progress,
            "paused reader must stop producer progress"
        );
        assert!(
            !calls.producer_done.load(Ordering::SeqCst),
            "producer must be blocked"
        );
        let mut received = 0;
        while let Some(reply) = slow.message().await.expect("stream response") {
            assert_eq!(name_of(&reply), part);
            received += 1;
        }
        assert_eq!(received, 128);
        drop(slow);
        wait_for_idle(&calls, &server_tracker, &client_tracker).await;
        assert_eq!(calls.producer_sent.load(Ordering::SeqCst), 128);
        assert!(calls.producer_done.load(Ordering::SeqCst));

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
        let expired = tokio::time::timeout(Duration::from_millis(server_deadline + 2000), deadline)
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
        if campaign {
            // Plaintext injection includes real RST_STREAM and GOAWAY frames.
            // TLS faults are connection resets; encrypted frame injection is not claimed.
            use common::lifecycle::{
                CallShape, FaultKind, LifecycleBoundary, LifecycleRunner, LifecycleScenario,
                RstReason, TransportKind,
            };
            let faults = [
                FaultKind::RstStream(RstReason::Cancel),
                FaultKind::Goaway,
                FaultKind::TcpReset,
            ];
            let fault = *faults
                .get(usize::try_from((cycle - 1) % 3).expect("fault index"))
                .expect("scheduled fault");
            let scenario = LifecycleScenario {
                shape: CallShape::Bidi,
                transport: TransportKind::Tcp,
                boundary: LifecycleBoundary::BodyStarted,
                fault,
                seed: seed.wrapping_add(cycle),
            };
            LifecycleRunner::run_scenario(scenario).await;
            record(
                &json!({"phase": "fault", "cycle": cycle, "fault": format!("{fault:?}"),
                "transport": "plaintext_tcp"}),
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let drain = snapshot("drain", cycle, &calls, &server_tracker, &client_tracker);
        record(&drain);
        assert_eq!(
            calls.started.load(Ordering::SeqCst),
            calls.ended.load(Ordering::SeqCst)
        );
        assert!(server_tracker.peak_allocated() <= budget);
        assert!(client_tracker.peak_allocated() <= budget);
        assert!(
            drain
                .get("rss_bytes")
                .expect("recorded resource gauge")
                .as_u64()
                .expect("rss")
                <= baseline
                    .get("rss_bytes")
                    .expect("recorded resource gauge")
                    .as_u64()
                    .expect("rss")
                    + RSS_RECOVERY_TOLERANCE
        );
        assert!(
            drain
                .get("file_descriptors")
                .expect("recorded resource gauge")
                .as_u64()
                .expect("fds")
                <= baseline
                    .get("file_descriptors")
                    .expect("recorded resource gauge")
                    .as_u64()
                    .expect("fds")
                    + 1
        );
        assert!(
            drain
                .get("tokio_alive_tasks")
                .expect("recorded resource gauge")
                .as_u64()
                .expect("tasks")
                <= baseline
                    .get("tokio_alive_tasks")
                    .expect("recorded resource gauge")
                    .as_u64()
                    .expect("tasks")
                    + 2
        );
    }
}
