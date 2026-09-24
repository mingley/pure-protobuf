//! Integration tests for WorkerService server lifecycle, marks, and core count.

#[path = "../src/benchmark_service.rs"]
pub mod benchmark_service;
#[path = "../src/load.rs"]
pub mod load;
#[path = "../src/report.rs"]
pub mod report;
#[path = "../src/resources.rs"]
pub mod resources;
#[path = "../src/worker_client.rs"]
pub mod worker_client;
#[path = "../src/worker_server.rs"]
pub mod worker_server;

use pbrs_grpc::Request;
use std::time::Duration;
use tokio::net::TcpListener;

use worker_client::{
    ByteBufferParams, ChannelArg, ClientArgs, ClientConfig, ClientType, ClosedLoopParams,
    CoreRequest, Histogram, HistogramParams, LoadParams, Mark, PayloadConfig, PoissonParams,
    Protocol, RpcType, SecurityParams, ServerArgs, ServerConfig, ServerType, SimpleProtoParams,
    Void, WorkerServiceClient, WorkerServiceImpl, WorkerServiceServer,
};

fn lazy_targets(targets: &[&str]) -> Vec<pbrs::rt::LazyStr> {
    targets
        .iter()
        .map(|s| pbrs::rt::LazyStr::from_bytes(s.as_bytes()))
        .collect()
}

fn async_server_config() -> ServerConfig {
    let mut config = ServerConfig::new();
    config.set_server_type(ServerType::AsyncServer);
    config
}

async fn spawn_worker_service() -> (std::net::SocketAddr, tokio::sync::watch::Sender<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (quit_tx, mut quit_rx) = tokio::sync::watch::channel(false);
    let worker_impl = WorkerServiceImpl::with_shutdown(quit_tx.clone());

    let shutdown = async move {
        while !*quit_rx.borrow() {
            if quit_rx.changed().await.is_err() {
                break;
            }
        }
    };

    tokio::spawn(async move {
        WorkerServiceServer::new(worker_impl)
            .serve_with_shutdown(listener, shutdown)
            .await
            .ok();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, quit_tx)
}

#[tokio::test]
async fn test_core_count() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    let resp = client
        .core_count(Request::new(CoreRequest::new()))
        .await
        .unwrap();
    let core_count = resp.into_inner().cores();
    assert!(core_count > 0, "core count must be strictly positive");

    let expected = worker_server::checked_system_cores(std::thread::available_parallelism())
        .expect("system CPU count must be available and representable");
    assert_eq!(
        core_count, expected,
        "core count should match host parallelism"
    );
}

#[test]
fn test_worker_snapshot_failure_is_a_status_not_a_zero_sample() {
    for stage in [
        "RunServer initial",
        "RunServer Mark",
        "RunClient initial",
        "RunClient Mark",
    ] {
        let error = std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "synthetic resource collector unavailable",
        );
        let status = worker_server::require_snapshot(Err(error), stage)
            .expect_err("missing snapshot cannot become a valid sample");
        assert_eq!(status.code(), pbrs_grpc::Code::Unavailable);
        assert!(status.message().contains(stage));
        assert!(
            status
                .message()
                .contains("synthetic resource collector unavailable")
        );
    }
    let snapshot = resources::ResourceSnapshot {
        user_cpu_nanos: 123,
        system_cpu_nanos: 45,
        current_rss_bytes: 4096,
        peak_rss_bytes: 8192,
        thread_count: 3,
    };
    assert_eq!(
        worker_server::require_snapshot(Ok(snapshot), "RunClient Mark")
            .expect("real snapshot must pass through"),
        snapshot
    );
}

#[test]
fn test_worker_core_count_rejects_missing_or_unrepresentable_values() {
    let missing = worker_server::checked_system_cores(Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "synthetic CPU probe failure",
    )))
    .expect_err("missing CPU count cannot become one core");
    assert_eq!(missing.code(), pbrs_grpc::Code::Unavailable);
    assert!(missing.message().contains("synthetic CPU probe failure"));
    assert_eq!(
        worker_server::checked_system_cores(Ok(
            std::num::NonZeroUsize::new(4).expect("positive count")
        ))
        .expect("four cores fit"),
        4
    );
    assert_eq!(
        worker_server::checked_core_count(0)
            .expect_err("zero cores are invalid")
            .code(),
        pbrs_grpc::Code::InvalidArgument
    );
    let oversized = worker_server::checked_core_count((i32::MAX as usize) + 1)
        .expect_err("wire i32 cannot hold system core count");
    assert_eq!(oversized.code(), pbrs_grpc::Code::ResourceExhausted);
}

#[test]
fn explicit_worker_options_are_not_silently_ignored() {
    type ServerOption = (&'static str, fn(&mut ServerConfig));
    let server_options: [ServerOption; 9] = [
        ("security_params", |cfg| {
            cfg.set_security_params(SecurityParams::new())
        }),
        ("async_server_threads", |cfg| {
            cfg.set_async_server_threads(2)
        }),
        ("core_limit", |cfg| cfg.set_core_limit(2)),
        ("core_list", |cfg| cfg.core_list_mut().push(1)),
        ("threads_per_cq", |cfg| cfg.set_threads_per_cq(2)),
        ("resource_quota_size", |cfg| {
            cfg.set_resource_quota_size(1024)
        }),
        ("channel_args", |cfg| {
            cfg.channel_args_mut().push(ChannelArg::new())
        }),
        ("server_processes", |cfg| cfg.set_server_processes(2)),
        ("other_server_api", |cfg| {
            cfg.set_other_server_api("unsupported")
        }),
    ];
    assert_eq!(
        worker_server::unsupported_server_option(&async_server_config()),
        None
    );
    for (name, set) in server_options {
        let mut config = async_server_config();
        set(&mut config);
        assert_eq!(
            worker_server::unsupported_server_option(&config),
            Some(name)
        );
    }

    type ClientOption = (&'static str, fn(&mut ClientConfig));
    let client_options: [ClientOption; 13] = [
        ("security_params", |cfg| {
            cfg.set_security_params(SecurityParams::new())
        }),
        ("async_client_threads", |cfg| {
            cfg.set_async_client_threads(2)
        }),
        ("core_limit", |cfg| cfg.set_core_limit(2)),
        ("core_list", |cfg| cfg.core_list_mut().push(1)),
        ("distribute_load_across_threads", |cfg| {
            cfg.set_distribute_load_across_threads(true)
        }),
        ("threads_per_cq", |cfg| cfg.set_threads_per_cq(2)),
        ("messages_per_stream", |cfg| cfg.set_messages_per_stream(2)),
        ("use_coalesce_api", |cfg| cfg.set_use_coalesce_api(true)),
        ("median_latency_collection_interval_millis", |cfg| {
            cfg.set_median_latency_collection_interval_millis(10)
        }),
        ("client_processes", |cfg| cfg.set_client_processes(2)),
        ("channel_args", |cfg| {
            cfg.channel_args_mut().push(ChannelArg::new())
        }),
        ("use_session", |cfg| cfg.set_use_session(true)),
        ("other_client_api", |cfg| {
            cfg.set_other_client_api("unsupported")
        }),
    ];
    assert_eq!(
        worker_client::unsupported_client_option(&ClientConfig::new()),
        None
    );
    for (name, set) in client_options {
        let mut config = ClientConfig::new();
        set(&mut config);
        assert_eq!(
            worker_client::unsupported_client_option(&config),
            Some(name)
        );
    }
}

#[tokio::test]
async fn test_worker_cleanup_helpers_join_owned_tasks() {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server = tokio::spawn(async move {
        let _ = shutdown_rx.await;
    });
    worker_server::stop_owned_server(shutdown_tx, &mut server)
        .await
        .expect("owned server shuts down and joins");
    assert!(server.is_finished());

    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let mut generator = tokio::spawn(async move {
        let _ = cancel_rx.changed().await;
        std::future::pending::<()>().await;
    });
    worker_client::stop_owned_generator(&cancel_tx, &mut generator)
        .await
        .expect("owned generator cancels and joins");
    assert!(generator.is_finished());
    assert!(*cancel_tx.borrow());
}

#[tokio::test]
async fn test_quit_worker() {
    let (addr, quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    let resp = client.quit_worker(Request::new(Void::new())).await;
    assert!(resp.is_ok(), "QuitWorker should return OK");
    assert!(*quit_tx.borrow(), "QuitWorker should set shutdown signal");
}

#[tokio::test]
async fn test_run_server_lifecycle_marks_and_shutdown() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    // 1. Open bidirectional RunServer stream
    let (tx, call) = client.run_server(Request::new(()));
    let mut out_stream = call.await.unwrap().into_inner();

    // 2. Send initial setup specifying ephemeral port 0
    let mut setup_args = ServerArgs::new();
    let mut config = async_server_config();
    config.set_port(0);
    setup_args.set_setup(config);
    tx.send(setup_args).await.unwrap();

    // 3. Receive initial ServerStatus with bound port and cores
    let init_status = out_stream
        .message()
        .await
        .unwrap()
        .expect("must receive initial ServerStatus");
    let server_port = init_status.port();
    assert!(server_port > 0, "bound port must be > 0");
    assert!(init_status.cores() > 0, "cores must be > 0");

    // 4. Connect to the spawned benchmark server and perform work
    let bench_addr: std::net::SocketAddr = format!("127.0.0.1:{server_port}").parse().unwrap();
    let bench_channel = pbrs_grpc::Channel::connect(bench_addr).await.unwrap();
    let bench_client = benchmark_service::BenchmarkServiceClient::new(bench_channel);

    // Perform multiple unary calls to consume CPU and record elapsed time
    for _ in 0..50 {
        let mut req = benchmark_service::SimpleRequest::new();
        req.set_response_size(4096);
        let resp = bench_client
            .unary_call(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.payload().body().len(), 4096);
    }

    // Wait briefly so elapsed time accumulates
    tokio::time::sleep(Duration::from_millis(60)).await;

    // 5. Send Mark with reset = false
    let mut mark_arg1 = ServerArgs::new();
    let mut mark1 = Mark::new();
    mark1.set_reset(false);
    mark_arg1.set_mark(mark1);
    tx.send(mark_arg1).await.unwrap();

    let status1 = out_stream
        .message()
        .await
        .unwrap()
        .expect("must receive mark 1 ServerStatus");
    assert!(status1.has_stats(), "status1 must have stats");
    let stats1 = status1.stats();
    let elapsed1 = stats1.time_elapsed();
    let user1 = stats1.time_user();
    let sys1 = stats1.time_system();
    assert!(elapsed1 > 0.0, "time_elapsed must be positive: {elapsed1}");
    assert!(user1 >= 0.0, "time_user must be non-negative: {user1}");
    assert!(sys1 >= 0.0, "time_system must be non-negative: {sys1}");

    // Do more work and sleep
    for _ in 0..50 {
        let mut req = benchmark_service::SimpleRequest::new();
        req.set_response_size(4096);
        let _ = bench_client.unary_call(Request::new(req)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(60)).await;

    // 6. Send second Mark with reset = false (accumulating)
    let mut mark_arg2 = ServerArgs::new();
    let mut mark2 = Mark::new();
    mark2.set_reset(false);
    mark_arg2.set_mark(mark2);
    tx.send(mark_arg2).await.unwrap();

    let status2 = out_stream
        .message()
        .await
        .unwrap()
        .expect("must receive mark 2 ServerStatus");
    let stats2 = status2.stats();
    let elapsed2 = stats2.time_elapsed();
    let user2 = stats2.time_user();
    let sys2 = stats2.time_system();
    assert!(
        elapsed2 >= elapsed1,
        "elapsed2 ({elapsed2}) should be >= elapsed1 ({elapsed1}) when reset=false"
    );
    assert!(
        user2 >= user1,
        "user2 ({user2}) should be >= user1 ({user1}) when reset=false"
    );
    assert!(
        sys2 >= sys1,
        "sys2 ({sys2}) should be >= sys1 ({sys1}) when reset=false"
    );

    // 7. Send third Mark with reset = true
    tokio::time::sleep(Duration::from_millis(40)).await;
    let mut mark_arg3 = ServerArgs::new();
    let mut mark3 = Mark::new();
    mark3.set_reset(true);
    mark_arg3.set_mark(mark3);
    tx.send(mark_arg3).await.unwrap();

    let status3 = out_stream
        .message()
        .await
        .unwrap()
        .expect("must receive mark 3 ServerStatus");
    let stats3 = status3.stats();
    let elapsed3 = stats3.time_elapsed();
    assert!(
        elapsed3 >= elapsed2,
        "elapsed3 ({elapsed3}) must still capture up to current before reset"
    );

    // 8. Send fourth Mark with reset = false shortly after reset
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut mark_arg4 = ServerArgs::new();
    let mut mark4 = Mark::new();
    mark4.set_reset(false);
    mark_arg4.set_mark(mark4);
    tx.send(mark_arg4).await.unwrap();

    let status4 = out_stream
        .message()
        .await
        .unwrap()
        .expect("must receive mark 4 ServerStatus");
    let stats4 = status4.stats();
    let elapsed4 = stats4.time_elapsed();
    assert!(
        elapsed4 < elapsed3,
        "elapsed4 ({elapsed4}) after reset must be strictly less than elapsed3 ({elapsed3})"
    );

    // 9. Close inbound stream -> triggers graceful shutdown of benchmark server
    drop(tx);

    // out_stream should see EOF with OK status
    let end = out_stream.message().await.unwrap();
    assert!(end.is_none(), "stream must terminate with None / OK status");

    // 10. Verify benchmark server on server_port is shut down
    tokio::time::sleep(Duration::from_millis(100)).await;
    let probe_conn = pbrs_grpc::Channel::connect(bench_addr).await;
    if let Ok(ch) = probe_conn {
        let probe_client = benchmark_service::BenchmarkServiceClient::new(ch);
        let mut req = benchmark_service::SimpleRequest::new();
        req.set_response_size(10);
        let call_res = probe_client.unary_call(Request::new(req)).await;
        assert!(
            call_res.is_err(),
            "calls to benchmark server should fail after shutdown"
        );
    }
}

#[tokio::test]
async fn test_duplicate_setup_rejected() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    let (tx, call) = client.run_server(Request::new(()));
    let mut out_stream = call.await.unwrap().into_inner();

    // First setup
    let mut setup1 = ServerArgs::new();
    let mut config1 = async_server_config();
    config1.set_port(0);
    setup1.set_setup(config1);
    tx.send(setup1).await.unwrap();

    let init_status = out_stream.message().await.unwrap().unwrap();
    assert!(init_status.port() > 0);

    // Duplicate setup
    let mut setup2 = ServerArgs::new();
    let mut config2 = async_server_config();
    config2.set_port(0);
    setup2.set_setup(config2);
    tx.send(setup2).await.unwrap();

    // Stream should return gRPC error status (InvalidArgument)
    let next_msg = out_stream.message().await;
    assert!(
        next_msg.is_err(),
        "duplicate setup must return gRPC error status"
    );
    let status = next_msg.unwrap_err();
    assert_eq!(status.code(), pbrs_grpc::Code::InvalidArgument);
}

#[tokio::test]
async fn test_invalid_config_rejected() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    // Invalid port (-1)
    let (tx, call) = client.run_server(Request::new(()));
    let mut out_stream = call.await.unwrap().into_inner();
    let mut setup_invalid = ServerArgs::new();
    let mut config_invalid = async_server_config();
    config_invalid.set_port(-1);
    setup_invalid.set_setup(config_invalid);
    tx.send(setup_invalid).await.unwrap();

    let res = out_stream.message().await;
    assert!(res.is_err(), "invalid port must be rejected");
    assert_eq!(res.unwrap_err().code(), pbrs_grpc::Code::InvalidArgument);

    // Invalid core limit (-5)
    let (tx2, call2) = client.run_server(Request::new(()));
    let mut out_stream2 = call2.await.unwrap().into_inner();
    let mut setup_invalid2 = ServerArgs::new();
    let mut config_invalid2 = async_server_config();
    config_invalid2.set_port(0);
    config_invalid2.set_core_limit(-5);
    setup_invalid2.set_setup(config_invalid2);
    tx2.send(setup_invalid2).await.unwrap();

    let res2 = out_stream2.message().await;
    assert!(res2.is_err(), "negative core_limit must be rejected");
    assert_eq!(res2.unwrap_err().code(), pbrs_grpc::Code::InvalidArgument);

    // First message is Mark instead of setup
    let (tx3, call3) = client.run_server(Request::new(()));
    let mut out_stream3 = call3.await.unwrap().into_inner();
    let mut mark_as_first = ServerArgs::new();
    let mut mark = Mark::new();
    mark.set_reset(false);
    mark_as_first.set_mark(mark);
    tx3.send(mark_as_first).await.unwrap();

    let res3 = out_stream3.message().await;
    assert!(res3.is_err(), "first message as Mark must be rejected");
    assert_eq!(res3.unwrap_err().code(), pbrs_grpc::Code::InvalidArgument);
}

#[test]
fn test_histogram_known_synthetic_latencies() {
    let mut h = Histogram::new(0.01, 60_000_000_000.0).unwrap();
    assert_eq!(h.num_buckets(), 2495);

    // Verify boundaries
    assert_eq!(h.bucket_for(0.0), 0);
    assert_eq!(h.bucket_for(0.5), 0);
    assert_eq!(h.bucket_for(1.0), 0);
    assert_eq!(h.bucket_for(1.009), 0);
    assert_eq!(h.bucket_for(1.01), 1);

    // 100 us = 100,000 ns
    let b_100k = (100_000.0_f64.ln() / 1.01_f64.ln()) as usize;
    assert_eq!(h.bucket_for(100_000.0), b_100k);
    h.add(100_000.0);

    let d1 = h.to_data();
    assert_eq!(d1.count(), 1.0);
    assert_eq!(d1.sum(), 100_000.0);
    assert_eq!(d1.min_seen(), 100_000.0);
    assert_eq!(d1.max_seen(), 100_000.0);
    assert_eq!(d1.sum_of_squares(), 10_000_000_000.0);
    assert_eq!(d1.bucket().get(b_100k), Some(1));

    // 500 us = 500,000 ns
    let b_500k = (500_000.0_f64.ln() / 1.01_f64.ln()) as usize;
    h.add(500_000.0);
    let d2 = h.to_data();
    assert_eq!(d2.count(), 2.0);
    assert_eq!(d2.sum(), 600_000.0);
    assert_eq!(d2.min_seen(), 100_000.0);
    assert_eq!(d2.max_seen(), 500_000.0);
    assert_eq!(
        d2.sum_of_squares(),
        100_000.0 * 100_000.0 + 500_000.0 * 500_000.0
    );
    assert_eq!(d2.bucket().get(b_500k), Some(1));

    // Value exceeding max_possible clamped to last bucket
    let max_p = 60_000_000_000.0;
    let last_bucket = h.num_buckets() - 1;
    assert_eq!(h.bucket_for(max_p * 2.0), last_bucket);
    h.add(max_p * 2.0);
    let d3 = h.to_data();
    assert_eq!(d3.count(), 3.0);
    assert_eq!(d3.max_seen(), max_p * 2.0);
    assert_eq!(d3.bucket().get(last_bucket), Some(1));

    // Reset
    h.reset();
    let d_reset = h.to_data();
    assert_eq!(d_reset.count(), 0.0);
    assert_eq!(d_reset.sum(), 0.0);
    assert_eq!(d_reset.min_seen(), 0.0);
    assert_eq!(d_reset.max_seen(), 0.0);
    assert_eq!(d_reset.bucket().get(b_100k), Some(0));
}

#[test]
fn worker_client_capacity_and_histogram_are_bounded_before_allocation() {
    let mut config = ClientConfig::new();
    config.set_client_channels(worker_client::MAX_WORKER_CLIENT_CHANNELS as i32);
    config.set_outstanding_rpcs_per_channel(
        (worker_client::MAX_WORKER_IN_FLIGHT_RPCS / worker_client::MAX_WORKER_CLIENT_CHANNELS)
            as i32,
    );
    assert_eq!(
        worker_client::checked_client_capacity(&config).unwrap(),
        (
            worker_client::MAX_WORKER_CLIENT_CHANNELS,
            worker_client::MAX_WORKER_IN_FLIGHT_RPCS,
        )
    );

    config.set_client_channels(worker_client::MAX_WORKER_CLIENT_CHANNELS as i32 + 1);
    let error = worker_client::checked_client_capacity(&config).unwrap_err();
    assert_eq!(error.code(), pbrs_grpc::Code::ResourceExhausted);
    assert!(error.message().contains("client_channels"));

    config.set_client_channels(worker_client::MAX_WORKER_CLIENT_CHANNELS as i32);
    config.set_outstanding_rpcs_per_channel(
        (worker_client::MAX_WORKER_IN_FLIGHT_RPCS / worker_client::MAX_WORKER_CLIENT_CHANNELS)
            as i32
            + 1,
    );
    let error = worker_client::checked_client_capacity(&config).unwrap_err();
    assert_eq!(error.code(), pbrs_grpc::Code::ResourceExhausted);
    assert!(error.message().contains("outstanding_rpcs_per_channel"));

    config.set_client_channels(1);
    config.set_outstanding_rpcs_per_channel(i32::MAX);
    assert_eq!(
        worker_client::checked_client_capacity(&config)
            .unwrap_err()
            .code(),
        pbrs_grpc::Code::ResourceExhausted
    );
    config.set_outstanding_rpcs_per_channel(0);
    assert_eq!(
        worker_client::checked_client_capacity(&config)
            .unwrap_err()
            .code(),
        pbrs_grpc::Code::InvalidArgument
    );

    assert_eq!(
        Histogram::new(0.01, 60_000_000_000.0)
            .unwrap()
            .num_buckets(),
        2495
    );
    let excessive = Histogram::new(0.01, f64::MAX).unwrap_err();
    assert_eq!(excessive.code(), pbrs_grpc::Code::InvalidArgument);
    assert!(excessive.message().contains("histogram bucket count"));
    let overflow = Histogram::new(f64::MIN_POSITIVE, f64::MAX).unwrap_err();
    assert_eq!(overflow.code(), pbrs_grpc::Code::ResourceExhausted);
    assert!(
        overflow
            .message()
            .contains("histogram bucket count overflow")
    );
}

#[test]
fn worker_channel_slots_never_exceed_per_channel_concurrency() {
    let slots = [
        std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    ];
    let (first, first_permit) = worker_client::acquire_channel_slot(&slots, 0)
        .unwrap()
        .unwrap();
    let (second, second_permit) = worker_client::acquire_channel_slot(&slots, 0)
        .unwrap()
        .unwrap();
    assert_eq!((first, second), (0, 1));
    assert!(
        worker_client::acquire_channel_slot(&slots, 0)
            .unwrap()
            .is_none()
    );
    drop(first_permit);
    let (available, _permit) = worker_client::acquire_channel_slot(&slots, 1)
        .unwrap()
        .unwrap();
    assert_eq!(available, 0);
    drop(second_permit);
}

#[test]
fn worker_rejections_are_counts_not_latency_samples() {
    let tracker =
        worker_client::ClientStatsTracker::new(Histogram::new(0.01, 60_000_000_000.0).unwrap());
    tracker.record_success(100_000.0);
    tracker.record_rejection(pbrs_grpc::Code::ResourceExhausted as i32);
    let (histogram, results) = tracker.snapshot(true);
    assert_eq!(histogram.count(), 1.0);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status_code(),
        pbrs_grpc::Code::ResourceExhausted as i32
    );
    assert_eq!(results[0].count(), 1);
    let (histogram, results) = tracker.snapshot(false);
    assert_eq!(histogram.count(), 0.0);
    assert!(results.is_empty());
}

#[tokio::test]
async fn worker_rpc_timeout_is_a_counted_error_not_a_missing_sample() {
    let tracker =
        worker_client::ClientStatsTracker::new(Histogram::new(0.01, 60_000_000_000.0).unwrap());
    let result = worker_client::track_worker_rpc(&tracker, Duration::from_millis(5), async {
        std::future::pending::<Result<(), pbrs_grpc::Status>>().await
    })
    .await;
    assert!(matches!(result, Err(load::RpcCallError::Timeout)));
    let (histogram, results) = tracker.snapshot(false);
    assert_eq!(histogram.count(), 1.0);
    assert_eq!(histogram.min_seen(), 5_000_000.0);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].status_code(),
        pbrs_grpc::Code::DeadlineExceeded as i32
    );
    assert_eq!(results[0].count(), 1);
}

#[tokio::test]
async fn test_run_client_closed_loop_lifecycle_marks_and_shutdown() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    // 1. Start benchmark server via RunServer
    let (server_tx, server_call) = client.run_server(Request::new(()));
    let mut server_out = server_call.await.unwrap().into_inner();

    let mut setup_args = ServerArgs::new();
    let mut server_config = async_server_config();
    server_config.set_port(0);
    setup_args.set_setup(server_config);
    server_tx.send(setup_args).await.unwrap();

    let server_status = server_out
        .message()
        .await
        .unwrap()
        .expect("server init status");
    let server_port = server_status.port();
    assert!(server_port > 0);

    // 2. Start client via RunClient
    let (client_tx, client_call) = client.run_client(Request::new(()));
    let mut client_out = client_call.await.unwrap().into_inner();

    // 3. Send ClientConfig setup
    let mut client_args = ClientArgs::new();
    let mut client_config = ClientConfig::new();
    let server_target = format!("127.0.0.1:{server_port}");
    client_config.set_server_targets(lazy_targets(&[&server_target]));
    client_config.set_client_channels(2);
    client_config.set_outstanding_rpcs_per_channel(2);
    client_config.set_client_type(ClientType::AsyncClient);
    client_config.set_rpc_type(RpcType::Unary);

    let mut load_params = LoadParams::new();
    load_params.set_closed_loop(ClosedLoopParams::new());
    client_config.set_load_params(load_params);

    let mut payload_config = PayloadConfig::new();
    let mut simple_params = SimpleProtoParams::new();
    simple_params.set_req_size(64);
    simple_params.set_resp_size(64);
    payload_config.set_simple_params(simple_params);
    client_config.set_payload_config(payload_config);

    let mut hist_params = HistogramParams::new();
    hist_params.set_resolution(0.01);
    hist_params.set_max_possible(60_000_000_000.0);
    client_config.set_histogram_params(hist_params);

    client_args.set_setup(client_config);
    client_tx.send(client_args).await.unwrap();

    // 4. Initial ClientStatus
    let init_status = client_out
        .message()
        .await
        .unwrap()
        .expect("client init status");
    assert!(init_status.has_stats());
    let init_stats = init_status.stats();
    assert_eq!(init_stats.time_elapsed(), 0.0);
    assert_eq!(init_stats.latencies().count(), 0.0);

    // Allow client workers to run and complete calls
    tokio::time::sleep(Duration::from_millis(80)).await;

    // 5. Send first Mark (reset = false)
    let mut mark_arg1 = ClientArgs::new();
    let mut mark1 = Mark::new();
    mark1.set_reset(false);
    mark_arg1.set_mark(mark1);
    client_tx.send(mark_arg1).await.unwrap();

    let status1 = client_out.message().await.unwrap().expect("mark 1 status");
    assert!(status1.has_stats());
    let stats1 = status1.stats();
    let count1 = stats1.latencies().count();
    let elapsed1 = stats1.time_elapsed();
    assert!(count1 > 0.0, "latency count must be > 0: got {count1}");
    assert!(stats1.latencies().sum() > 0.0, "latency sum must be > 0");
    assert!(
        stats1.latencies().min_seen() > 0.0,
        "min_seen must be positive"
    );
    assert!(stats1.latencies().max_seen() >= stats1.latencies().min_seen());
    assert!(
        elapsed1 > 0.0,
        "elapsed time must be positive: got {elapsed1}"
    );
    assert!(stats1.time_user() >= 0.0);
    assert!(stats1.time_system() >= 0.0);
    assert!(
        stats1.latencies().bucket().iter().any(|b| b > 0),
        "at least one histogram bucket must be non-empty"
    );

    // Do more work
    tokio::time::sleep(Duration::from_millis(80)).await;

    // 6. Send second Mark (reset = false, accumulating)
    let mut mark_arg2 = ClientArgs::new();
    let mut mark2 = Mark::new();
    mark2.set_reset(false);
    mark_arg2.set_mark(mark2);
    client_tx.send(mark_arg2).await.unwrap();

    let status2 = client_out.message().await.unwrap().expect("mark 2 status");
    let stats2 = status2.stats();
    let count2 = stats2.latencies().count();
    let elapsed2 = stats2.time_elapsed();
    assert!(
        count2 >= count1,
        "count2 ({count2}) should be >= count1 ({count1})"
    );
    assert!(
        elapsed2 >= elapsed1,
        "elapsed2 ({elapsed2}) should be >= elapsed1 ({elapsed1})"
    );

    // 7. Send third Mark (reset = true)
    let mut mark_arg3 = ClientArgs::new();
    let mut mark3 = Mark::new();
    mark3.set_reset(true);
    mark_arg3.set_mark(mark3);
    client_tx.send(mark_arg3).await.unwrap();

    let status3 = client_out.message().await.unwrap().expect("mark 3 status");
    let stats3 = status3.stats();
    let count3 = stats3.latencies().count();
    assert!(count3 >= count2);

    // 8. Sleep and send fourth Mark (reset = false, fresh interval)
    tokio::time::sleep(Duration::from_millis(40)).await;
    let mut mark_arg4 = ClientArgs::new();
    let mut mark4 = Mark::new();
    mark4.set_reset(false);
    mark_arg4.set_mark(mark4);
    client_tx.send(mark_arg4).await.unwrap();

    let status4 = client_out.message().await.unwrap().expect("mark 4 status");
    let stats4 = status4.stats();
    let count4 = stats4.latencies().count();
    assert!(
        count4 < count3,
        "count4 ({count4}) after reset must be strictly less than accumulated count3 ({count3})"
    );

    // 9. Close client stream: clean cancellation and shutdown
    drop(client_tx);
    let end_client = client_out.message().await.unwrap();
    assert!(
        end_client.is_none(),
        "client out_stream should close with OK status"
    );

    // Clean up server
    drop(server_tx);
    let _ = server_out.message().await;
}

#[tokio::test]
async fn test_run_client_poisson_load_lifecycle() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    // Start benchmark server
    let (server_tx, server_call) = client.run_server(Request::new(()));
    let mut server_out = server_call.await.unwrap().into_inner();
    let mut setup_args = ServerArgs::new();
    let mut server_config = async_server_config();
    server_config.set_port(0);
    setup_args.set_setup(server_config);
    server_tx.send(setup_args).await.unwrap();
    let server_status = server_out.message().await.unwrap().expect("server status");
    let server_port = server_status.port();

    // Start client with Poisson arrival schedule
    let (client_tx, client_call) = client.run_client(Request::new(()));
    let mut client_out = client_call.await.unwrap().into_inner();

    let mut client_args = ClientArgs::new();
    let mut client_config = ClientConfig::new();
    let server_target = format!("127.0.0.1:{server_port}");
    client_config.set_server_targets(lazy_targets(&[&server_target]));
    client_config.set_client_channels(1);
    client_config.set_outstanding_rpcs_per_channel(4);
    client_config.set_client_type(ClientType::AsyncClient);
    client_config.set_rpc_type(RpcType::Unary);

    let mut load_params = LoadParams::new();
    let mut poisson = PoissonParams::new();
    poisson.set_offered_load(200.0);
    load_params.set_poisson(poisson);
    client_config.set_load_params(load_params);

    client_args.set_setup(client_config);
    client_tx.send(client_args).await.unwrap();

    let init = client_out.message().await.unwrap().expect("init");
    assert_eq!(init.stats().latencies().count(), 0.0);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut mark_arg = ClientArgs::new();
    let mut mark = Mark::new();
    mark.set_reset(false);
    mark_arg.set_mark(mark);
    client_tx.send(mark_arg).await.unwrap();

    let status = client_out.message().await.unwrap().expect("mark status");
    assert!(status.stats().latencies().count() > 0.0);
    assert!(status.stats().latencies().bucket().iter().any(|b| b > 0));

    drop(client_tx);
    let end = client_out.message().await.unwrap();
    assert!(end.is_none());

    drop(server_tx);
    let _ = server_out.message().await;
}

#[tokio::test]
async fn test_run_client_unsupported_options_fail_clearly() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    // 1. Unsupported client_type (OtherClient)
    let (tx1, call1) = client.run_client(Request::new(()));
    let mut out1 = call1.await.unwrap().into_inner();
    let mut args1 = ClientArgs::new();
    let mut cfg1 = ClientConfig::new();
    cfg1.set_server_targets(lazy_targets(&["127.0.0.1:50051"]));
    cfg1.set_client_channels(1);
    cfg1.set_outstanding_rpcs_per_channel(1);
    cfg1.set_client_type(ClientType::OtherClient);
    let mut lp1 = LoadParams::new();
    lp1.set_closed_loop(ClosedLoopParams::new());
    cfg1.set_load_params(lp1);
    args1.set_setup(cfg1);
    tx1.send(args1).await.unwrap();
    let err1 = out1.message().await.unwrap_err();
    assert_eq!(err1.code(), pbrs_grpc::Code::InvalidArgument);

    // 2. Empty server_targets
    let (tx2, call2) = client.run_client(Request::new(()));
    let mut out2 = call2.await.unwrap().into_inner();
    let mut args2 = ClientArgs::new();
    let mut cfg2 = ClientConfig::new();
    cfg2.set_client_channels(1);
    cfg2.set_outstanding_rpcs_per_channel(1);
    cfg2.set_client_type(ClientType::AsyncClient);
    let mut lp2 = LoadParams::new();
    lp2.set_closed_loop(ClosedLoopParams::new());
    cfg2.set_load_params(lp2);
    args2.set_setup(cfg2);
    tx2.send(args2).await.unwrap();
    let err2 = out2.message().await.unwrap_err();
    assert_eq!(err2.code(), pbrs_grpc::Code::InvalidArgument);

    // 3. First message is Mark instead of setup
    let (tx3, call3) = client.run_client(Request::new(()));
    let mut out3 = call3.await.unwrap().into_inner();
    let mut args3 = ClientArgs::new();
    let mut mark3 = Mark::new();
    mark3.set_reset(false);
    args3.set_mark(mark3);
    tx3.send(args3).await.unwrap();
    let err3 = out3.message().await.unwrap_err();
    assert_eq!(err3.code(), pbrs_grpc::Code::InvalidArgument);

    // 4. Invalid histogram params (negative resolution)
    let (tx4, call4) = client.run_client(Request::new(()));
    let mut out4 = call4.await.unwrap().into_inner();
    let mut args4 = ClientArgs::new();
    let mut cfg4 = ClientConfig::new();
    cfg4.set_server_targets(lazy_targets(&["127.0.0.1:50051"]));
    cfg4.set_client_channels(1);
    cfg4.set_outstanding_rpcs_per_channel(1);
    cfg4.set_client_type(ClientType::AsyncClient);
    let mut lp4 = LoadParams::new();
    lp4.set_closed_loop(ClosedLoopParams::new());
    cfg4.set_load_params(lp4);
    let mut hp = HistogramParams::new();
    hp.set_resolution(-0.5);
    cfg4.set_histogram_params(hp);
    args4.set_setup(cfg4);
    tx4.send(args4).await.unwrap();
    let err4 = out4.message().await.unwrap_err();
    assert_eq!(err4.code(), pbrs_grpc::Code::InvalidArgument);

    // 5. Unsupported protocol (ChaoticGood)
    let (tx5, call5) = client.run_client(Request::new(()));
    let mut out5 = call5.await.unwrap().into_inner();
    let mut args5 = ClientArgs::new();
    let mut cfg5 = ClientConfig::new();
    cfg5.set_server_targets(lazy_targets(&["127.0.0.1:50051"]));
    cfg5.set_client_channels(1);
    cfg5.set_outstanding_rpcs_per_channel(1);
    cfg5.set_client_type(ClientType::AsyncClient);
    let mut lp5 = LoadParams::new();
    lp5.set_closed_loop(ClosedLoopParams::new());
    cfg5.set_load_params(lp5);
    cfg5.set_protocol(Protocol::ChaoticGood);
    args5.set_setup(cfg5);
    tx5.send(args5).await.unwrap();
    let err5 = out5.message().await.unwrap_err();
    assert_eq!(err5.code(), pbrs_grpc::Code::InvalidArgument);

    // 6. Duplicate setup
    let (server_tx, server_call) = client.run_server(Request::new(()));
    let mut server_out = server_call.await.unwrap().into_inner();
    let mut setup_args = ServerArgs::new();
    let mut server_config = async_server_config();
    server_config.set_port(0);
    setup_args.set_setup(server_config);
    server_tx.send(setup_args).await.unwrap();
    let server_port = server_out.message().await.unwrap().expect("server").port();

    let (tx6, call6) = client.run_client(Request::new(()));
    let mut out6 = call6.await.unwrap().into_inner();
    let mut args6 = ClientArgs::new();
    let mut cfg6 = ClientConfig::new();
    let server_target6 = format!("127.0.0.1:{server_port}");
    cfg6.set_server_targets(lazy_targets(&[&server_target6]));
    cfg6.set_client_channels(1);
    cfg6.set_outstanding_rpcs_per_channel(1);
    cfg6.set_client_type(ClientType::AsyncClient);
    let mut lp6 = LoadParams::new();
    lp6.set_closed_loop(ClosedLoopParams::new());
    cfg6.set_load_params(lp6);
    args6.set_setup(cfg6.clone());
    tx6.send(args6).await.unwrap();
    let init6 = out6.message().await.unwrap().expect("init");
    assert_eq!(init6.stats().time_elapsed(), 0.0);

    // Send duplicate setup
    let mut dup_args = ClientArgs::new();
    dup_args.set_setup(cfg6);
    tx6.send(dup_args).await.unwrap();
    let err6 = out6.message().await.unwrap_err();
    assert_eq!(err6.code(), pbrs_grpc::Code::InvalidArgument);

    drop(server_tx);
    let _ = server_out.message().await;
}

#[tokio::test]
async fn unsupported_benchmark_worker_modes_fail_before_peer_work() {
    async fn reject_server(client: &WorkerServiceClient, config: ServerConfig, reason: &str) {
        let (sender, call) = client.run_server(Request::new(()));
        let mut output = call.await.expect("server control headers").into_inner();
        let mut setup = ServerArgs::new();
        setup.set_setup(config);
        sender.send(setup).await.expect("server setup");
        let error = tokio::time::timeout(Duration::from_secs(2), output.message())
            .await
            .expect("unsupported server mode did not reject")
            .expect_err("unsupported server mode cannot report success");
        assert_eq!(error.code(), pbrs_grpc::Code::InvalidArgument);
        assert!(error.message().contains(reason), "{error}");
    }

    async fn reject_client(
        client: &WorkerServiceClient,
        config: ClientConfig,
        code: pbrs_grpc::Code,
        reason: &str,
    ) {
        let (sender, call) = client.run_client(Request::new(()));
        let mut output = call.await.expect("client control headers").into_inner();
        let mut setup = ClientArgs::new();
        setup.set_setup(config);
        sender.send(setup).await.expect("client setup");
        let error = tokio::time::timeout(Duration::from_secs(2), output.message())
            .await
            .expect("unsupported client mode did not reject")
            .expect_err("unsupported client mode cannot report success");
        assert_eq!(error.code(), code, "{error}");
        assert!(error.message().contains(reason), "{error}");
    }

    let (addr, _quit_tx) = spawn_worker_service().await;
    let client = WorkerServiceClient::new(pbrs_grpc::Channel::connect(addr).await.unwrap());

    let mut sync = ServerConfig::new();
    sync.set_server_type(ServerType::SyncServer);
    reject_server(&client, sync, "unsupported server_type").await;
    let mut generic = ServerConfig::new();
    generic.set_server_type(ServerType::AsyncGenericServer);
    reject_server(&client, generic, "unsupported server_type").await;
    let mut proto_with_generic_payload = async_server_config();
    proto_with_generic_payload.set_payload_config(PayloadConfig::new());
    reject_server(&client, proto_with_generic_payload, "payload_config").await;
    let mut server_threads = async_server_config();
    server_threads.set_async_server_threads(2);
    reject_server(&client, server_threads, "unsupported server config option").await;

    let mut valid = ClientConfig::new();
    valid.set_server_targets(lazy_targets(&["127.0.0.1:50051"]));
    valid.set_client_channels(1);
    valid.set_outstanding_rpcs_per_channel(1);
    valid.set_client_type(ClientType::AsyncClient);
    let mut load = LoadParams::new();
    load.set_closed_loop(ClosedLoopParams::new());
    valid.set_load_params(load);

    let mut client_threads = valid.clone();
    client_threads.set_async_client_threads(2);
    reject_client(
        &client,
        client_threads,
        pbrs_grpc::Code::InvalidArgument,
        "unsupported client config option async_client_threads",
    )
    .await;

    let mut too_many_channels = valid.clone();
    too_many_channels.set_client_channels(worker_client::MAX_WORKER_CLIENT_CHANNELS as i32 + 1);
    reject_client(
        &client,
        too_many_channels,
        pbrs_grpc::Code::ResourceExhausted,
        "client_channels",
    )
    .await;
    let mut too_many_calls = valid.clone();
    too_many_calls
        .set_outstanding_rpcs_per_channel(worker_client::MAX_WORKER_IN_FLIGHT_RPCS as i32 + 1);
    reject_client(
        &client,
        too_many_calls,
        pbrs_grpc::Code::ResourceExhausted,
        "outstanding_rpcs_per_channel",
    )
    .await;

    let mut too_many_buckets = valid.clone();
    let mut histogram = HistogramParams::new();
    histogram.set_resolution(0.01);
    histogram.set_max_possible(f64::MAX);
    too_many_buckets.set_histogram_params(histogram);
    reject_client(
        &client,
        too_many_buckets,
        pbrs_grpc::Code::InvalidArgument,
        "histogram bucket count",
    )
    .await;

    let mut nonfinite_load = valid.clone();
    let mut load = LoadParams::new();
    let mut poisson = PoissonParams::new();
    poisson.set_offered_load(f64::NAN);
    load.set_poisson(poisson);
    nonfinite_load.set_load_params(load);
    reject_client(
        &client,
        nonfinite_load,
        pbrs_grpc::Code::InvalidArgument,
        "must be finite",
    )
    .await;

    let mut nonfinite_histogram = valid.clone();
    let mut histogram = HistogramParams::new();
    histogram.set_resolution(f64::INFINITY);
    histogram.set_max_possible(f64::INFINITY);
    nonfinite_histogram.set_histogram_params(histogram);
    reject_client(
        &client,
        nonfinite_histogram,
        pbrs_grpc::Code::InvalidArgument,
        "invalid histogram_params",
    )
    .await;

    let mut sync_client = valid.clone();
    sync_client.set_client_type(ClientType::SyncClient);
    reject_client(
        &client,
        sync_client,
        pbrs_grpc::Code::InvalidArgument,
        "unsupported client_type",
    )
    .await;

    let mut bytebuf = valid.clone();
    let mut payload = PayloadConfig::new();
    payload.set_bytebuf_params(ByteBufferParams::new());
    bytebuf.set_payload_config(payload);
    reject_client(
        &client,
        bytebuf,
        pbrs_grpc::Code::InvalidArgument,
        "unsupported payload_config",
    )
    .await;

    let mut unspecified = valid.clone();
    unspecified.set_payload_config(PayloadConfig::new());
    reject_client(
        &client,
        unspecified,
        pbrs_grpc::Code::InvalidArgument,
        "must specify simple_params",
    )
    .await;

    let mut negative = valid.clone();
    let mut payload = PayloadConfig::new();
    let mut simple = SimpleProtoParams::new();
    simple.set_req_size(-1);
    payload.set_simple_params(simple);
    negative.set_payload_config(payload);
    reject_client(
        &client,
        negative,
        pbrs_grpc::Code::InvalidArgument,
        "negative simple_params",
    )
    .await;

    let mut oversize = valid;
    let mut payload = PayloadConfig::new();
    let mut simple = SimpleProtoParams::new();
    simple.set_resp_size(
        i32::try_from(benchmark_service::MAX_BENCHMARK_PAYLOAD_SIZE).expect("cap fits i32") + 1,
    );
    payload.set_simple_params(simple);
    oversize.set_payload_config(payload);
    reject_client(
        &client,
        oversize,
        pbrs_grpc::Code::ResourceExhausted,
        "exceeds the 4 MiB",
    )
    .await;
}

#[tokio::test]
async fn test_run_client_control_disconnect_cancels_work_without_hangs() {
    let (addr, _quit_tx) = spawn_worker_service().await;
    let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
    let client = WorkerServiceClient::new(channel);

    let (server_tx, server_call) = client.run_server(Request::new(()));
    let mut server_out = server_call.await.unwrap().into_inner();
    let mut setup_args = ServerArgs::new();
    let mut server_config = async_server_config();
    server_config.set_port(0);
    setup_args.set_setup(server_config);
    server_tx.send(setup_args).await.unwrap();
    let server_port = server_out.message().await.unwrap().expect("server").port();

    let (client_tx, client_call) = client.run_client(Request::new(()));
    let mut client_out = client_call.await.unwrap().into_inner();

    let mut client_args = ClientArgs::new();
    let mut client_config = ClientConfig::new();
    let server_target = format!("127.0.0.1:{server_port}");
    client_config.set_server_targets(lazy_targets(&[&server_target]));
    client_config.set_client_channels(4);
    client_config.set_outstanding_rpcs_per_channel(4);
    client_config.set_client_type(ClientType::AsyncClient);
    let mut lp = LoadParams::new();
    lp.set_closed_loop(ClosedLoopParams::new());
    client_config.set_load_params(lp);
    client_args.set_setup(client_config);
    client_tx.send(client_args).await.unwrap();

    let _init = client_out.message().await.unwrap().expect("init");

    // Immediately drop tx (disconnect while in-flight)
    drop(client_tx);

    // Must resolve cleanly within 2 seconds without hanging
    let close_fut = async {
        let msg = client_out.message().await.unwrap();
        assert!(msg.is_none());
    };
    tokio::time::timeout(Duration::from_secs(2), close_fut)
        .await
        .expect("stream close must terminate client tasks promptly without hangs");

    drop(server_tx);
    let _ = server_out.message().await;
}
