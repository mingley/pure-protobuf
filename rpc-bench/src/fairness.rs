//! Bounded, single-cell RT-07 diagnostic. A separate server process keeps
//! endpoint resources attributable; missing transport gauges remain missing.

#![allow(
    clippy::disallowed_types,
    reason = "observer callbacks hold small synchronous locks only; no lock crosses an await"
)]

use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pbrs_grpc::{
    ByteBudgetTracker, CallLabels, Channel, ChannelConfig, InteropTestService, LifecycleObserver,
    Payload, RejectionEvent, Request, SimpleRequest, Status, StreamingOutputCallRequest,
    TestServiceClient, TestServiceServer,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::Command;

use crate::load::{LoadConfig, LoadGenerator, LoadRecord, RpcCallError};
use crate::process::stream_kernel_req;
use crate::report::{
    detect_git_commit, HostInfo, LatencyDistribution, SchedulingLagNanos, ToolPins, TransportMode,
};
use crate::resources::ResourceSnapshot;

const SCENARIO_ID: &str = "bulk_vs_small_streams";
const SMALL_TIMEOUT: Duration = Duration::from_secs(3);
const BULK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Bulk {
    rpc_type: String,
    request_size: usize,
    response_message_size: usize,
    messages_per_stream: usize,
    concurrency: usize,
    connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Small {
    rpc_type: String,
    request_size: usize,
    response_size: usize,
    offered_qps: f64,
    concurrency: usize,
    connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Limits {
    max_concurrent_connections: usize,
    max_concurrent_rpcs: usize,
    max_send_buffer_size: usize,
    initial_connection_window_size: u32,
    initial_stream_window_size: u32,
    byte_budget_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Matching {
    tcp_nodelay: bool,
    tls_cipher: String,
    connections: usize,
    concurrency: usize,
}

#[derive(Deserialize)]
struct Defaults {
    duration_seconds: f64,
    warmup_seconds: f64,
    min_paired_runs: usize,
    open_loop_distribution: String,
    seed: u64,
}

#[derive(Deserialize)]
struct ScenarioFile {
    defaults: Defaults,
    scenarios: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct WorkloadMix {
    bulk_streams: Bulk,
    competing_small_rpcs: Small,
}

#[derive(Deserialize)]
struct SelectedScenario {
    id: String,
    name: String,
    workload_mix: WorkloadMix,
    resource_limits: Limits,
    matching_configuration: Matching,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Case {
    id: String,
    name: String,
    duration_seconds: f64,
    warmup_seconds: f64,
    min_paired_runs: usize,
    open_loop_distribution: String,
    seed: u64,
    bulk: Bulk,
    small: Small,
    limits: Limits,
    matching: Matching,
}

impl Case {
    fn load(path: &PathBuf, smoke: bool) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read fairness scenario {}: {e}", path.display()))?;
        Self::parse(&contents, smoke)
    }

    fn parse(contents: &str, smoke: bool) -> Result<Self, String> {
        let file: ScenarioFile = serde_json::from_str(contents)
            .map_err(|e| format!("invalid fairness scenario: {e}"))?;
        let entries: Vec<_> = file
            .scenarios
            .into_iter()
            .filter(|entry| {
                entry.get("id").and_then(serde_json::Value::as_str) == Some(SCENARIO_ID)
            })
            .collect();
        if entries.len() != 1 {
            return Err(format!(
                "expected exactly one {SCENARIO_ID} scenario, found {}",
                entries.len()
            ));
        }
        let selected: SelectedScenario = serde_json::from_value(
            entries
                .into_iter()
                .next()
                .ok_or("missing selected scenario")?,
        )
        .map_err(|e| format!("invalid {SCENARIO_ID} workload: {e}"))?;
        let mut case = Self {
            id: selected.id,
            name: selected.name,
            duration_seconds: file.defaults.duration_seconds,
            warmup_seconds: file.defaults.warmup_seconds,
            min_paired_runs: file.defaults.min_paired_runs,
            open_loop_distribution: file.defaults.open_loop_distribution,
            seed: file.defaults.seed,
            bulk: selected.workload_mix.bulk_streams,
            small: selected.workload_mix.competing_small_rpcs,
            limits: selected.resource_limits,
            matching: selected.matching_configuration,
        };
        case.validate()?;
        if smoke {
            case.duration_seconds = 0.6;
            case.warmup_seconds = 0.1;
            case.small.offered_qps = 120.0;
        }
        Ok(case)
    }

    fn validate(&self) -> Result<(), String> {
        if self.id != SCENARIO_ID
            || self.bulk.rpc_type != "server_streaming"
            || self.small.rpc_type != "unary"
            || self.open_loop_distribution != "poisson"
            || !self.matching.tcp_nodelay
            || self.matching.tls_cipher != "none"
        {
            return Err(
                "fairness runner supports only native h2c bulk streaming + Poisson unary".into(),
            );
        }
        if !self.duration_seconds.is_finite()
            || !(60.0..=120.0).contains(&self.duration_seconds)
            || !self.warmup_seconds.is_finite()
            || !(15.0..=30.0).contains(&self.warmup_seconds)
            || self.min_paired_runs < 5
            || !self.small.offered_qps.is_finite()
            || !(1.0..=10_000.0).contains(&self.small.offered_qps)
            || self.small.offered_qps * self.duration_seconds > 1_000_000.0
        {
            return Err(
                "unbounded or below-contract fairness timing, paired runs or offered QPS".into(),
            );
        }
        if self.bulk.concurrency == 0
            || self.bulk.concurrency > 32
            || self.small.concurrency == 0
            || self.small.concurrency > 256
            || self.bulk.connections == 0
            || self.small.connections == 0
            || self.matching.connections != self.bulk.connections + self.small.connections
            || self.matching.concurrency != self.bulk.concurrency + self.small.concurrency
            || self.limits.max_concurrent_connections < self.matching.connections
            || self.limits.max_concurrent_rpcs < self.matching.concurrency
            || self.limits.max_send_buffer_size == 0
            || self.limits.max_send_buffer_size > self.limits.byte_budget_bytes
            || self.limits.byte_budget_bytes > 64 * 1024 * 1024
            || self.limits.initial_stream_window_size < 65_535
            || self.limits.initial_connection_window_size < 65_535
        {
            return Err("invalid fairness concurrency, connection count or resource limits".into());
        }
        if self.bulk.messages_per_stream == 0
            || self.bulk.messages_per_stream > 256
            || self.bulk.response_message_size == 0
            || self.bulk.response_message_size > 256 * 1024
            || self.bulk.request_size == 0
            || self.bulk.request_size > 4096
            || self.small.request_size == 0
            || self.small.request_size > 4096
            || self.small.response_size == 0
            || self.small.response_size > 4096
        {
            return Err("invalid fairness payload size or stream length".into());
        }
        Ok(())
    }

    fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.duration_seconds)
    }

    fn warmup(&self) -> Duration {
        Duration::from_secs_f64(self.warmup_seconds)
    }
}

#[derive(Default)]
struct Queues {
    small: Mutex<Vec<u64>>,
    bulk: Mutex<Vec<u64>>,
}

struct Probe {
    tracker: ByteBudgetTracker,
    recording: AtomicBool,
    sampled_peak: AtomicUsize,
    queues: Queues,
    rejections: Mutex<BTreeMap<String, u64>>,
}

impl Probe {
    fn new(tracker: ByteBudgetTracker) -> Arc<Self> {
        Arc::new(Self {
            tracker,
            recording: AtomicBool::new(false),
            sampled_peak: AtomicUsize::new(0),
            queues: Queues::default(),
            rejections: Mutex::new(BTreeMap::new()),
        })
    }

    fn begin(&self) {
        self.queues
            .small
            .lock()
            .expect("queue probe mutex poisoned")
            .clear();
        self.queues
            .bulk
            .lock()
            .expect("queue probe mutex poisoned")
            .clear();
        self.rejections
            .lock()
            .expect("rejection probe mutex poisoned")
            .clear();
        self.sampled_peak
            .store(self.tracker.allocated(), Ordering::SeqCst);
        self.recording.store(true, Ordering::SeqCst);
    }

    fn end(&self) {
        self.recording.store(false, Ordering::SeqCst);
    }

    fn sample(&self) {
        if self.recording.load(Ordering::SeqCst) {
            self.sampled_peak
                .fetch_max(self.tracker.allocated(), Ordering::SeqCst);
        }
    }

    fn wait_samples(&self, method: &str) -> Vec<u64> {
        match method {
            "UnaryCall" => self
                .queues
                .small
                .lock()
                .expect("queue probe mutex poisoned")
                .clone(),
            "StreamingOutputCall" => self
                .queues
                .bulk
                .lock()
                .expect("queue probe mutex poisoned")
                .clone(),
            _ => Vec::new(),
        }
    }

    fn rejected_by_reason(&self) -> BTreeMap<String, u64> {
        self.rejections
            .lock()
            .expect("rejection probe mutex poisoned")
            .clone()
    }
}

impl LifecycleObserver for Probe {
    fn on_queue_wait(&self, call: &CallLabels<'_>, wait: Duration) {
        if !self.recording.load(Ordering::SeqCst) {
            return;
        }
        let queue = match call.method() {
            "UnaryCall" => &self.queues.small,
            "StreamingOutputCall" => &self.queues.bulk,
            _ => return,
        };
        queue
            .lock()
            .expect("queue probe mutex poisoned")
            .push(wait.as_nanos().min(u64::MAX as u128) as u64);
    }

    fn on_bytes_sent(&self, _call: &CallLabels<'_>, _bytes: usize) {
        self.sample();
    }

    fn on_bytes_received(&self, _call: &CallLabels<'_>, _bytes: usize) {
        self.sample();
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        if self.recording.load(Ordering::SeqCst) {
            let key = format!("{}:{:?}", event.call.method(), event.reason);
            *self
                .rejections
                .lock()
                .expect("rejection probe mutex poisoned")
                .entry(key)
                .or_insert(0) += 1;
        }
    }
}

fn sample_periodically(
    probe: Arc<Probe>,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(5));
        loop {
            tokio::select! {
                _ = &mut stopped => break,
                _ = interval.tick() => probe.sample(),
            }
        }
    });
    (stop, task)
}

#[derive(Debug, Serialize, Deserialize)]
struct Endpoint {
    pid: u32,
    start_rss_bytes: u64,
    peak_rss_bytes: u64,
    cpu_seconds: f64,
    sampled_byte_budget_peak_bytes: u64,
    byte_budget_allocated_bytes_post_drain: u64,
    observed_rejections_by_reason: BTreeMap<String, u64>,
}

impl Endpoint {
    fn measured(
        before: ResourceSnapshot,
        after: ResourceSnapshot,
        probe: &Probe,
    ) -> Result<Self, String> {
        if before.current_rss_bytes == 0
            || after.peak_rss_bytes < before.current_rss_bytes
            || after.total_cpu_nanos() < before.total_cpu_nanos()
        {
            return Err("inconsistent endpoint RSS or CPU resource snapshots".into());
        }
        Ok(Self {
            pid: std::process::id(),
            start_rss_bytes: before.current_rss_bytes,
            peak_rss_bytes: after.peak_rss_bytes,
            cpu_seconds: after
                .total_cpu_nanos()
                .saturating_sub(before.total_cpu_nanos()) as f64
                / 1_000_000_000.0,
            sampled_byte_budget_peak_bytes: probe.sampled_peak.load(Ordering::SeqCst) as u64,
            byte_budget_allocated_bytes_post_drain: probe.tracker.allocated() as u64,
            observed_rejections_by_reason: probe.rejected_by_reason(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Ready {
    addr: SocketAddr,
    case: Case,
    pid: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerResult {
    endpoint: Endpoint,
}

fn send_line<T: Serialize>(prefix: &str, value: &T) -> Result<(), String> {
    let line = serde_json::to_string(value).map_err(|e| format!("serialize {prefix}: {e}"))?;
    println!("{prefix} {line}");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("flush {prefix}: {e}"))
}

async fn receive_line<T: for<'de> Deserialize<'de>>(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    prefix: &str,
) -> Result<T, String> {
    let mut line = String::new();
    let n = tokio::time::timeout(Duration::from_secs(20), reader.read_line(&mut line))
        .await
        .map_err(|_| format!("server did not send {prefix} within 20s"))?
        .map_err(|e| format!("read server {prefix}: {e}"))?;
    if n == 0 {
        return Err(format!("server exited before sending {prefix}"));
    }
    let json = line
        .trim_end()
        .strip_prefix(prefix)
        .ok_or_else(|| format!("unexpected server control response: {}", line.trim_end()))?;
    serde_json::from_str(json).map_err(|e| format!("invalid server {prefix}: {e}"))
}

pub struct Options {
    scenario: PathBuf,
    output: Option<PathBuf>,
    smoke: bool,
    require_qualified: bool,
}

pub fn parse_options(args: &[String], server: bool) -> Result<Options, String> {
    let mut scenario = None;
    let mut output = None;
    let mut smoke = false;
    let mut require_qualified = false;
    let mut i = 2;
    while i < args.len() {
        let option = &args[i];
        if option == "--smoke" {
            smoke = true;
        } else if option == "--require-qualified" && !server {
            require_qualified = true;
        } else if option == "--scenario" || option == "--output" {
            i += 1;
            let value = args
                .get(i)
                .filter(|v| !v.starts_with('-') && !v.is_empty())
                .ok_or_else(|| format!("missing value for {option}"))?;
            if option == "--scenario" {
                scenario = Some(PathBuf::from(value));
            } else {
                output = Some(PathBuf::from(value));
            }
        } else if let Some(value) = option.strip_prefix("--scenario=") {
            scenario = Some(PathBuf::from(value));
        } else if let Some(value) = option.strip_prefix("--output=") {
            output = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown fairness option {option}"));
        }
        i += 1;
    }
    if server && output.is_some() {
        return Err("fairness-server does not accept --output".into());
    }
    let scenario = scenario.ok_or("fairness requires --scenario <path>")?;
    if scenario.as_os_str().is_empty() || output.as_ref().is_some_and(|p| p.as_os_str().is_empty())
    {
        return Err("fairness paths must not be empty".into());
    }
    Ok(Options {
        scenario,
        output,
        smoke,
        require_qualified,
    })
}

pub async fn run_server(options: Options) -> Result<(), String> {
    let case = Case::load(&options.scenario, options.smoke)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("fairness server bind: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("fairness server address: {e}"))?;
    let tracker = ByteBudgetTracker::with_limit(case.limits.byte_budget_bytes);
    let probe = Probe::new(tracker.clone());
    let server = pbrs_grpc::Router::new()
        .add_service(TestServiceServer::new(InteropTestService))
        .max_concurrent_connections(case.limits.max_concurrent_connections)
        .max_concurrent_rpcs(case.limits.max_concurrent_rpcs)
        .initial_connection_window_size(case.limits.initial_connection_window_size)
        .initial_stream_window_size(case.limits.initial_stream_window_size)
        .max_send_buffer_size(case.limits.max_send_buffer_size)
        .with_byte_budget_tracker(tracker)
        .observer(probe.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut serving = tokio::spawn(async move {
        server
            .serve_with_shutdown(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    send_line(
        "READY",
        &Ready {
            addr,
            case,
            pid: std::process::id(),
        },
    )?;

    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut command = String::new();
    stdin
        .read_line(&mut command)
        .await
        .map_err(|e| format!("server START read: {e}"))?;
    if command.trim_end() != "START" {
        return Err(format!("expected START, got {:?}", command.trim_end()));
    }
    let before = ResourceSnapshot::capture().map_err(|e| format!("server RSS baseline: {e}"))?;
    probe.begin();
    let (stop_sampling, sampling) = sample_periodically(probe.clone());
    send_line("STARTED", &true)?;

    command.clear();
    stdin
        .read_line(&mut command)
        .await
        .map_err(|e| format!("server STOP read: {e}"))?;
    if command.trim_end() != "STOP" {
        return Err(format!("expected STOP, got {:?}", command.trim_end()));
    }
    probe.end();
    let _ = stop_sampling.send(());
    sampling
        .await
        .map_err(|e| format!("server sampler failed: {e}"))?;
    let _ = shutdown_tx.send(());
    match tokio::time::timeout(Duration::from_secs(20), &mut serving).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => return Err(format!("fairness server failed: {e}")),
        Ok(Err(e)) => return Err(format!("fairness server task failed: {e}")),
        Err(_) => {
            serving.abort();
            return Err("fairness server did not drain within 20s".into());
        }
    }
    let after = ResourceSnapshot::capture().map_err(|e| format!("server RSS final: {e}"))?;
    let endpoint = Endpoint::measured(before, after, &probe)?;
    send_line("RESULT", &ServerResult { endpoint })
}

#[derive(Debug, Serialize, Deserialize)]
struct ObservedWait {
    samples: u64,
    p99: u64,
    max: u64,
}

fn observed_wait(samples: &[u64]) -> Option<ObservedWait> {
    let summary = SchedulingLagNanos::from_samples(samples)?;
    Some(ObservedWait {
        samples: samples.len() as u64,
        p99: summary.p99,
        max: summary.max,
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct ClassResult {
    arrival: String,
    offered_calls: u64,
    dispatched_calls: u64,
    successful_calls: u64,
    failed_calls: u64,
    rejected_calls_by_reason: BTreeMap<String, u64>,
    timed_out_calls: u64,
    unfinished_calls: u64,
    queue_delay_nanos: Option<ObservedWait>,
    observed_client_pool_wait_nanos: Option<ObservedWait>,
    scheduling_lag_p99_nanos: Option<u64>,
    p99_latency_nanos: Option<u64>,
    latency_samples_nanos: Vec<u64>,
    scheduling_lag_samples_nanos: Option<Vec<u64>>,
    status_errors: BTreeMap<String, u64>,
    successful_stream_messages: Option<u64>,
}

fn small_result(record: LoadRecord, pool_wait: &[u64]) -> Result<ClassResult, String> {
    let error_count: u64 = record.status_errors.values().sum();
    if record.offered_calls != record.dispatched_calls + record.rejected_calls
        || record.unstarted_calls != record.rejected_calls
        || record.dispatched_calls != record.successful_calls + record.failed_calls
        || record.completed_calls != record.dispatched_calls
        || record.timed_out_calls > record.failed_calls
        || error_count != record.failed_calls + record.rejected_calls
        || record.scheduling_lags_nanos.len() != record.offered_calls as usize
        || record.e2e_latencies_nanos.len() != record.dispatched_calls as usize
        || record.unfinished_calls != 0
    {
        return Err(format!(
            "small RPC accounting incomplete (offered={}, dispatched={}, unfinished={})",
            record.offered_calls, record.dispatched_calls, record.unfinished_calls
        ));
    }
    let mut rejected = BTreeMap::new();
    rejected.insert("QUEUE_OVERFLOW".into(), record.rejected_calls);
    rejected.insert(
        "RESOURCE_EXHAUSTED".into(),
        record
            .status_errors
            .get("RESOURCE_EXHAUSTED")
            .copied()
            .unwrap_or(0),
    );
    let p99 = if record.rejected_calls == 0 && record.failed_calls == record.timed_out_calls {
        record.e2e_latency_distribution().map(|d| d.p99_nanos)
    } else {
        None
    };
    Ok(ClassResult {
        arrival: "poisson".into(),
        offered_calls: record.offered_calls,
        dispatched_calls: record.dispatched_calls,
        successful_calls: record.successful_calls,
        failed_calls: record.failed_calls,
        rejected_calls_by_reason: rejected,
        timed_out_calls: record.timed_out_calls,
        unfinished_calls: record.unfinished_calls,
        queue_delay_nanos: None,
        observed_client_pool_wait_nanos: observed_wait(pool_wait),
        scheduling_lag_p99_nanos: record.scheduling_lag_summary().map(|s| s.p99),
        p99_latency_nanos: p99,
        latency_samples_nanos: record.e2e_latencies_nanos,
        scheduling_lag_samples_nanos: Some(record.scheduling_lags_nanos),
        status_errors: record.status_errors,
        successful_stream_messages: None,
    })
}

#[derive(Default)]
struct BulkResults {
    offered: u64,
    successful: u64,
    timed_out: u64,
    successful_messages: u64,
    errors: BTreeMap<String, u64>,
    latencies: Vec<u64>,
}

impl BulkResults {
    fn merge(&mut self, other: Self) {
        self.offered += other.offered;
        self.successful += other.successful;
        self.timed_out += other.timed_out;
        self.successful_messages += other.successful_messages;
        for (key, count) in other.errors {
            *self.errors.entry(key).or_insert(0) += count;
        }
        self.latencies.extend(other.latencies);
    }

    fn report(self, pool_wait: &[u64]) -> Result<ClassResult, String> {
        if self.offered != self.latencies.len() as u64
            || self.offered != self.successful + self.errors.values().sum::<u64>()
        {
            return Err("bulk stream outcomes or latencies are incomplete".into());
        }
        let mut rejected = BTreeMap::new();
        rejected.insert("QUEUE_OVERFLOW".into(), 0);
        rejected.insert(
            "RESOURCE_EXHAUSTED".into(),
            self.errors.get("RESOURCE_EXHAUSTED").copied().unwrap_or(0),
        );
        let p99 = if self.offered == self.successful + self.timed_out {
            LatencyDistribution::from_samples(&self.latencies).map(|d| d.p99_nanos)
        } else {
            None
        };
        Ok(ClassResult {
            arrival: "closed_concurrent".into(),
            offered_calls: self.offered,
            dispatched_calls: self.offered,
            successful_calls: self.successful,
            failed_calls: self.offered - self.successful,
            rejected_calls_by_reason: rejected,
            timed_out_calls: self.timed_out,
            unfinished_calls: 0,
            queue_delay_nanos: None,
            observed_client_pool_wait_nanos: observed_wait(pool_wait),
            scheduling_lag_p99_nanos: None,
            p99_latency_nanos: p99,
            latency_samples_nanos: self.latencies,
            scheduling_lag_samples_nanos: None,
            status_errors: self.errors,
            successful_stream_messages: Some(self.successful_messages),
        })
    }
}

fn status_error(status: Status) -> RpcCallError {
    RpcCallError::Status(status.code().name().to_string())
}

async fn bulk_call(
    client: &TestServiceClient,
    request: StreamingOutputCallRequest,
    case: &Bulk,
) -> Result<(), RpcCallError> {
    let mut stream = client
        .streaming_output_call(Request::new(request))
        .await
        .map_err(status_error)?
        .into_inner();
    for _ in 0..case.messages_per_stream {
        let next = stream.message().await.map_err(status_error)?;
        let Some(response) = next else {
            return Err(RpcCallError::Other("TRUNCATED_STREAM".into()));
        };
        if response.payload().body().len() != case.response_message_size {
            return Err(RpcCallError::Other("PAYLOAD_MISMATCH".into()));
        }
    }
    if stream.message().await.map_err(status_error)?.is_some() {
        return Err(RpcCallError::Other("EXTRA_STREAM_MESSAGE".into()));
    }
    Ok(())
}

struct ActiveBulk(Arc<AtomicUsize>);

impl ActiveBulk {
    fn new(active: Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::SeqCst);
        Self(active)
    }
}

impl Drop for ActiveBulk {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn bulk_phase(
    client: TestServiceClient,
    case: Bulk,
    duration: Duration,
    active: Arc<AtomicUsize>,
) -> Result<BulkResults, String> {
    let deadline = Instant::now() + duration;
    let max_calls_per_worker = (duration.as_secs_f64() * 500.0).ceil() as u64;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..case.concurrency {
        let client = client.clone();
        let case = case.clone();
        let active = active.clone();
        tasks.spawn(async move {
            let mut results = BulkResults::default();
            let mut request = stream_kernel_req(
                case.messages_per_stream as i32,
                case.response_message_size as i32,
            );
            let mut payload = Payload::new();
            payload.set_body(vec![0; case.request_size]);
            request.set_payload(payload);
            while Instant::now() < deadline {
                if results.offered >= max_calls_per_worker {
                    return Err(format!(
                        "bulk worker reached the {max_calls_per_worker}-call safety cap before phase end"
                    ));
                }
                results.offered += 1;
                let started = Instant::now();
                let _guard = ActiveBulk::new(active.clone());
                let result =
                    tokio::time::timeout(BULK_TIMEOUT, bulk_call(&client, request.clone(), &case))
                        .await;
                let nanos = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                match result {
                    Ok(Ok(())) => {
                        results.successful += 1;
                        results.successful_messages += case.messages_per_stream as u64;
                        results.latencies.push(nanos);
                    }
                    Ok(Err(err)) => {
                        *results.errors.entry(err.code_name()).or_insert(0) += 1;
                        results.latencies.push(nanos);
                    }
                    Err(_) => {
                        results.timed_out += 1;
                        *results
                            .errors
                            .entry("DEADLINE_EXCEEDED".into())
                            .or_insert(0) += 1;
                        results
                            .latencies
                            .push(nanos.max(BULK_TIMEOUT.as_nanos() as u64));
                    }
                }
            }
            Ok(results)
        });
    }
    let mut results = BulkResults::default();
    let drain_deadline =
        tokio::time::Instant::now() + duration + BULK_TIMEOUT + Duration::from_secs(2);
    while !tasks.is_empty() {
        let joined = tokio::time::timeout_at(drain_deadline, tasks.join_next())
            .await
            .map_err(|_| "bulk workers did not finish within their call deadlines")?
            .ok_or("bulk worker set became empty unexpectedly")?
            .map_err(|e| format!("bulk worker failed: {e}"))?;
        results.merge(joined?);
    }
    Ok(results)
}

async fn phase(
    case: &Case,
    duration: Duration,
    seed: u64,
    bulk_client: TestServiceClient,
    small_client: TestServiceClient,
) -> Result<(BulkResults, LoadRecord, u64), String> {
    let active = Arc::new(AtomicUsize::new(0));
    let overlap = Arc::new(AtomicU64::new(0));
    let small_config = LoadConfig::open_poisson(case.small.offered_qps, seed, duration)
        .with_max_in_flight(case.small.concurrency)
        .with_timeout(SMALL_TIMEOUT)
        .with_drain_timeout(SMALL_TIMEOUT + Duration::from_secs(1));
    let request = {
        let mut req = SimpleRequest::new();
        let mut payload = Payload::new();
        payload.set_body(vec![0; case.small.request_size]);
        req.set_payload(payload);
        req.set_response_size(case.small.response_size as i32);
        req
    };
    let expected_response_size = case.small.response_size;
    let small_active = active.clone();
    let small_overlap = overlap.clone();
    let generator = LoadGenerator::new(small_config);
    let small = generator.run(move || {
        let client = small_client.clone();
        let request = request.clone();
        let active = small_active.clone();
        let overlap = small_overlap.clone();
        async move {
            if active.load(Ordering::SeqCst) > 0 {
                overlap.fetch_add(1, Ordering::SeqCst);
            }
            let reply = client
                .unary_call(Request::new(request))
                .await
                .map_err(status_error)?;
            if reply.into_inner().payload().body().len() != expected_response_size {
                return Err(RpcCallError::Other("PAYLOAD_MISMATCH".into()));
            }
            Ok(())
        }
    });
    let bulk = bulk_phase(
        bulk_client,
        case.bulk.clone(),
        duration,
        Arc::clone(&active),
    );
    let (bulk, small) = tokio::join!(bulk, small);
    let bulk = bulk?;
    if small.unfinished_calls != 0 {
        return Err(format!(
            "{} small RPCs remained unfinished after bounded drain",
            small.unfinished_calls
        ));
    }
    Ok((bulk, small, overlap.load(Ordering::SeqCst)))
}

#[derive(Debug, Serialize, Deserialize)]
struct EndpointMetrics {
    client_pid: u32,
    server_pid: u32,
    client_cpu_seconds: f64,
    server_cpu_seconds: f64,
    client_start_rss_bytes: u64,
    server_start_rss_bytes: u64,
    client_peak_rss_bytes: u64,
    server_peak_rss_bytes: u64,
    client_active_permits_peak: Option<u64>,
    server_active_permits_peak: Option<u64>,
    client_active_permits_post_drain: Option<u64>,
    server_active_permits_post_drain: Option<u64>,
    client_byte_budget_allocated_bytes_peak: Option<u64>,
    server_byte_budget_allocated_bytes_peak: Option<u64>,
    client_byte_budget_allocated_bytes_post_drain: u64,
    server_byte_budget_allocated_bytes_post_drain: u64,
    client_sampled_byte_budget_peak_bytes: u64,
    server_sampled_byte_budget_peak_bytes: u64,
    post_drain_probe_success: bool,
    client_observed_rejections_by_reason: BTreeMap<String, u64>,
    server_observed_rejections_by_reason: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Qualification {
    qualified: bool,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FairnessReport {
    schema_version: String,
    scenario_id: String,
    mode: String,
    peer_combination: String,
    case: Case,
    host: HostInfo,
    source: ToolPins,
    small_calls_while_bulk_in_flight: u64,
    per_workload_class: BTreeMap<String, ClassResult>,
    per_endpoint: EndpointMetrics,
    unsupported_metrics: BTreeMap<String, String>,
    qualification: Qualification,
}

async fn drain_budget(tracker: &ByteBudgetTracker) -> u64 {
    for _ in 0..40 {
        if tracker.allocated() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracker.allocated() as u64
}

async fn with_server(case: Case, options: &Options) -> Result<FairnessReport, String> {
    let exe = std::env::current_exe().map_err(|e| format!("resolve fairness binary: {e}"))?;
    let mut command = Command::new(exe);
    command
        .arg("fairness-server")
        .arg("--scenario")
        .arg(&options.scenario);
    if options.smoke {
        command.arg("--smoke");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("spawn fairness server: {e}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or("fairness server stdin unavailable")?;
    let mut stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or("fairness server stdout unavailable")?,
    );
    let run: Result<FairnessReport, String> = async {
        let ready: Ready = receive_line(&mut stdout, "READY ").await?;
        if ready.case != case || ready.pid == std::process::id() {
            return Err("fairness server scenario mismatch or shared process".into());
        }
        let channel_config = |connections| {
            ChannelConfig::default()
                .connections(connections)
                .max_send_buffer_size(case.limits.max_send_buffer_size)
                .initial_connection_window_size(case.limits.initial_connection_window_size)
                .initial_stream_window_size(case.limits.initial_stream_window_size)
        };
        let tracker = ByteBudgetTracker::with_limit(case.limits.byte_budget_bytes);
        let probe = Probe::new(tracker.clone());
        let bulk_channel = Channel::connect_with(ready.addr, channel_config(case.bulk.connections))
            .await
            .map_err(|e| format!("bulk channel connect: {e}"))?
            .with_byte_budget_tracker(tracker.clone())
            .observer(probe.clone());
        let small_channel = Channel::connect_with(ready.addr, channel_config(case.small.connections))
            .await
            .map_err(|e| format!("small channel connect: {e}"))?
            .with_byte_budget_tracker(tracker.clone())
            .observer(probe.clone());
        let bulk_client = TestServiceClient::new(bulk_channel);
        let small_client = TestServiceClient::new(small_channel);
        let _ = phase(&case, case.warmup(), case.seed ^ 0xa5a5_a5a5, bulk_client.clone(), small_client.clone()).await?;
        if drain_budget(&tracker).await != 0 {
            return Err("client byte budget did not quiesce after warmup".into());
        }
        stdin.write_all(b"START\n").await.map_err(|e| format!("send START: {e}"))?;
        let started: bool = receive_line(&mut stdout, "STARTED ").await?;
        if !started {
            return Err("server did not start resource sampling".into());
        }
        let before = ResourceSnapshot::capture().map_err(|e| format!("client RSS baseline: {e}"))?;
        probe.begin();
        let (stop_sampling, sampling) = sample_periodically(probe.clone());
        let (bulk, small, overlap) = phase(&case, case.duration(), case.seed, bulk_client.clone(), small_client.clone()).await?;
        probe.end();
        let _ = stop_sampling.send(());
        sampling.await.map_err(|e| format!("client sampler failed: {e}"))?;
        let bulk_result = bulk.report(&probe.wait_samples("StreamingOutputCall"))?;
        let small_result = small_result(small, &probe.wait_samples("UnaryCall"))?;

        let post_drain_probe_success = small_client
            .unary_call(Request::new(SimpleRequest::new()))
            .await
            .is_ok();
        drop(bulk_client);
        drop(small_client);
        let client_post_drain = drain_budget(&tracker).await;
        let after = ResourceSnapshot::capture().map_err(|e| format!("client RSS final: {e}"))?;
        let mut client_endpoint = Endpoint::measured(before, after, &probe)?;
        client_endpoint.byte_budget_allocated_bytes_post_drain = client_post_drain;
        stdin.write_all(b"STOP\n").await.map_err(|e| format!("send STOP: {e}"))?;
        stdin.flush().await.map_err(|e| format!("flush STOP: {e}"))?;
        let server: ServerResult = receive_line(&mut stdout, "RESULT ").await?;
        if server.endpoint.pid != ready.pid {
            return Err("fairness server PID changed mid-run".into());
        }
        let endpoint = EndpointMetrics {
            client_pid: client_endpoint.pid,
            server_pid: server.endpoint.pid,
            client_cpu_seconds: client_endpoint.cpu_seconds,
            server_cpu_seconds: server.endpoint.cpu_seconds,
            client_start_rss_bytes: client_endpoint.start_rss_bytes,
            server_start_rss_bytes: server.endpoint.start_rss_bytes,
            client_peak_rss_bytes: client_endpoint.peak_rss_bytes,
            server_peak_rss_bytes: server.endpoint.peak_rss_bytes,
            client_active_permits_peak: None,
            server_active_permits_peak: None,
            client_active_permits_post_drain: None,
            server_active_permits_post_drain: None,
            client_byte_budget_allocated_bytes_peak: None,
            server_byte_budget_allocated_bytes_peak: None,
            client_byte_budget_allocated_bytes_post_drain: client_endpoint.byte_budget_allocated_bytes_post_drain,
            server_byte_budget_allocated_bytes_post_drain: server.endpoint.byte_budget_allocated_bytes_post_drain,
            client_sampled_byte_budget_peak_bytes: client_endpoint.sampled_byte_budget_peak_bytes,
            server_sampled_byte_budget_peak_bytes: server.endpoint.sampled_byte_budget_peak_bytes,
            post_drain_probe_success,
            client_observed_rejections_by_reason: client_endpoint.observed_rejections_by_reason,
            server_observed_rejections_by_reason: server.endpoint.observed_rejections_by_reason,
        };
        if endpoint.client_sampled_byte_budget_peak_bytes > case.limits.byte_budget_bytes as u64
            || endpoint.server_sampled_byte_budget_peak_bytes > case.limits.byte_budget_bytes as u64
        {
            return Err("observed byte allocations exceeded the configured budget".into());
        }
        let mut unsupported_metrics = BTreeMap::from([
            ("per_workload_class.*.queue_delay_nanos".into(),
             "on_queue_wait measures client pool acquisition only; full outbound-to-TCP and server queue wait are not observable (on_server_queue_wait has no emitter)".into()),
            ("per_workload_class.bulk_streams.scheduling_lag_p99_nanos".into(),
             "bulk streams use fixed concurrent closed-loop workers, not scheduled arrivals".into()),
            ("per_endpoint.*_active_permits_peak".into(),
             "no public gauge exposes client/server transport RPC semaphore occupancy".into()),
            ("per_endpoint.*_active_permits_post_drain".into(),
             "a successful post-drain RPC probes admission, not an exact semaphore occupancy gauge".into()),
            ("per_endpoint.*_byte_budget_allocated_bytes_peak".into(),
             "allocated() gives instantaneous snapshots only; callback + 5ms polling peaks are lower bounds, not an exact high-water mark".into()),
        ]);
        let mut blockers = vec![
            "one native/native loopback run is diagnostic only: no randomized five-run paired reference comparisons on dedicated hosts".into(),
            "full client/server queue wait and transport active-permit/high-water byte-budget gauges are unavailable".into(),
            "bulk arrival is closed-loop; no per-class open-loop bulk schedule".into(),
        ];
        if small_result.rejected_calls_by_reason["QUEUE_OVERFLOW"] > 0 {
            blockers.push("unstarted QUEUE_OVERFLOW calls have no deadline-valued latency; small-class p99 is null".into());
            unsupported_metrics.insert(
                "per_workload_class.competing_small_rpcs.p99_latency_nanos".into(),
                "unstarted QUEUE_OVERFLOW calls lack a deadline-valued latency sample".into(),
            );
        } else if small_result.failed_calls > small_result.timed_out_calls {
            blockers.push(
                "non-timeout small RPC failures lack deadline-valued latency; small-class p99 is null"
                    .into(),
            );
            unsupported_metrics.insert(
                "per_workload_class.competing_small_rpcs.p99_latency_nanos".into(),
                "transport status failures are timestamped at completion, not at the deadline"
                    .into(),
            );
        }
        if bulk_result.failed_calls > bulk_result.timed_out_calls {
            blockers.push(
                "non-timeout bulk stream failures lack deadline-valued latency; bulk-class p99 is null"
                    .into(),
            );
            unsupported_metrics.insert(
                "per_workload_class.bulk_streams.p99_latency_nanos".into(),
                "transport status failures are timestamped at completion, not at the deadline"
                    .into(),
            );
        }
        for (name, result) in [
            ("bulk_streams", &bulk_result),
            ("competing_small_rpcs", &small_result),
        ] {
            if result.offered_calls == 0 {
                blockers.push(format!("{name} received no offered calls"));
            }
            if result.observed_client_pool_wait_nanos.is_none() {
                unsupported_metrics.insert(
                    format!("per_workload_class.{name}.observed_client_pool_wait_nanos"),
                    "no client on_queue_wait callback was observed for this class".into(),
                );
            }
            if result.p99_latency_nanos.is_none() {
                unsupported_metrics
                    .entry(format!("per_workload_class.{name}.p99_latency_nanos"))
                    .or_insert_with(|| "no completed latency samples for this class".into());
            }
        }
        if small_result.scheduling_lag_p99_nanos.is_none() {
            unsupported_metrics.insert(
                "per_workload_class.competing_small_rpcs.scheduling_lag_p99_nanos".into(),
                "no scheduled Poisson arrival was observed".into(),
            );
        }
        if small_result.dispatched_calls != small_result.observed_client_pool_wait_nanos.as_ref().map_or(0, |w| w.samples) {
            blockers.push("client pool-wait callback count does not cover every dispatched small RPC".into());
        }
        if overlap == 0 {
            blockers.push("no scheduled small call overlapped a client-side bulk stream".into());
        }
        if endpoint.client_byte_budget_allocated_bytes_post_drain != 0
            || endpoint.server_byte_budget_allocated_bytes_post_drain != 0
            || !post_drain_probe_success
        {
            blockers.push("post-drain admission or byte-budget recovery did not complete".into());
        }
        let mut per_workload_class = BTreeMap::new();
        per_workload_class.insert("bulk_streams".into(), bulk_result);
        per_workload_class.insert("competing_small_rpcs".into(), small_result);
        Ok(FairnessReport {
            schema_version: "1.0.0".into(),
            scenario_id: SCENARIO_ID.into(),
            mode: if options.smoke { "diagnostic_smoke" } else { "unqualified_single_run" }.into(),
            peer_combination: "native/native".into(),
            case,
            host: HostInfo::detect(),
            source: ToolPins::detect(TransportMode::Native, detect_git_commit()),
            small_calls_while_bulk_in_flight: overlap,
            per_workload_class,
            per_endpoint: endpoint,
            unsupported_metrics,
            qualification: Qualification { qualified: false, blockers },
        })
    }.await;
    if run.is_err() {
        let _ = child.start_kill();
    }
    let status = tokio::time::timeout(Duration::from_secs(20), child.wait())
        .await
        .map_err(|_| "fairness server did not exit after result or failure".to_string())?
        .map_err(|e| format!("wait for fairness server: {e}"))?;
    let report = run?;
    if !status.success() {
        return Err(format!("fairness server exited with {status}"));
    }
    Ok(report)
}

pub async fn run(options: Options) -> Result<(), String> {
    let case = Case::load(&options.scenario, options.smoke)?;
    let report = with_server(case, &options).await?;
    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("serialize fairness report: {e}"))?;
    if let Some(path) = &options.output {
        std::fs::write(path, &json)
            .map_err(|e| format!("write fairness report {}: {e}", path.display()))?;
    }
    println!("{json}");
    if options.require_qualified && !report.qualification.qualified {
        return Err(format!(
            "RT-07 qualification unavailable: {}",
            report.qualification.blockers.join("; ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::LoadDistribution;

    #[test]
    fn scenario_is_bounded_and_only_selected_cell_is_executable() {
        let fixture = include_str!("../scenarios/fairness.json");
        let parsed =
            Case::parse(fixture, false).expect("real fairness scenario must be executable");
        assert_eq!(parsed.id, SCENARIO_ID);
        assert_eq!(parsed.matching.connections, 4);
        assert_eq!(parsed.matching.concurrency, 12);
        assert_eq!(parsed.duration_seconds, 60.0);
        let smoke = Case::parse(fixture, true).expect("smoke must parse");
        assert!(smoke.duration_seconds < parsed.duration_seconds);
        assert!(smoke.small.offered_qps < parsed.small.offered_qps);

        let mut value: serde_json::Value = serde_json::from_str(fixture).expect("fixture JSON");
        value["scenarios"][0]["workload_mix"]["competing_small_rpcs"]["concurrency"] = 300.into();
        let invalid = serde_json::to_string(&value).expect("mutated JSON");
        assert!(
            Case::parse(&invalid, false).is_err(),
            "excessive concurrency cannot be accepted"
        );
        value["scenarios"][0]["workload_mix"]["competing_small_rpcs"]["concurrency"] = 8.into();
        value["scenarios"][0]["resource_limits"]["byte_budget_bytes"] = 0.into();
        let invalid = serde_json::to_string(&value).expect("mutated JSON");
        assert!(
            Case::parse(&invalid, false).is_err(),
            "zero byte budget cannot be accepted"
        );
    }

    #[test]
    fn offered_and_error_counts_preserve_overflow_without_inventing_p99() {
        let record = LoadRecord {
            distribution: LoadDistribution::Poisson,
            offered_calls: 3,
            dispatched_calls: 2,
            completed_calls: 2,
            successful_calls: 1,
            failed_calls: 1,
            timed_out_calls: 0,
            rejected_calls: 1,
            unstarted_calls: 1,
            unfinished_calls: 0,
            scheduling_lags_nanos: vec![1, 2, 3],
            service_latencies_nanos: vec![10, 40],
            e2e_latencies_nanos: vec![11, 42],
            duration: Duration::from_secs(1),
            status_errors: BTreeMap::from([
                ("RESOURCE_EXHAUSTED".into(), 1),
                ("QUEUE_OVERFLOW".into(), 1),
            ]),
        };
        let result = small_result(record.clone(), &[5, 7]).expect("complete record");
        assert_eq!(
            result.offered_calls,
            result.dispatched_calls + result.rejected_calls_by_reason["QUEUE_OVERFLOW"]
        );
        assert_eq!(
            result.successful_calls + result.failed_calls,
            result.dispatched_calls
        );
        assert_eq!(result.rejected_calls_by_reason["RESOURCE_EXHAUSTED"], 1);
        assert_eq!(result.latency_samples_nanos.len(), 2);
        assert_eq!(
            result.scheduling_lag_samples_nanos.as_ref().map(Vec::len),
            Some(3)
        );
        assert_eq!(
            result.p99_latency_nanos, None,
            "unstarted overflow lacks a latency sample"
        );
        assert!(
            result.queue_delay_nanos.is_none(),
            "pool wait is not full queue delay"
        );

        let mut early_status = record.clone();
        early_status.offered_calls = 2;
        early_status.rejected_calls = 0;
        early_status.unstarted_calls = 0;
        early_status.scheduling_lags_nanos.truncate(2);
        early_status.status_errors.remove("QUEUE_OVERFLOW");
        let result = small_result(early_status, &[5, 7]).expect("early transport status retained");
        assert!(
            result.p99_latency_nanos.is_none(),
            "early RESOURCE_EXHAUSTED cannot improve the offered-call p99"
        );

        let mut broken = record;
        broken.status_errors.remove("QUEUE_OVERFLOW");
        assert!(
            small_result(broken, &[5, 7]).is_err(),
            "missing error outcome must fail closed"
        );
        assert_eq!(
            status_error(Status::resource_exhausted("limit")).code_name(),
            "RESOURCE_EXHAUSTED"
        );
    }

    #[test]
    fn bulk_status_failure_does_not_produce_flattering_p99() {
        let result = BulkResults {
            offered: 2,
            successful: 1,
            timed_out: 0,
            successful_messages: 64,
            errors: BTreeMap::from([("RESOURCE_EXHAUSTED".into(), 1)]),
            latencies: vec![20, 2],
        }
        .report(&[3, 4])
        .expect("complete bulk accounting");
        assert_eq!(result.latency_samples_nanos.len(), 2);
        assert_eq!(result.rejected_calls_by_reason["RESOURCE_EXHAUSTED"], 1);
        assert!(
            result.p99_latency_nanos.is_none(),
            "short rejection must not lower the bulk p99"
        );
    }

    #[test]
    fn snapshots_use_real_rss_and_budget_accounting_not_permit_inference() {
        let tracker = ByteBudgetTracker::with_limit(128);
        let probe = Probe::new(tracker.clone());
        let before = ResourceSnapshot {
            user_cpu_nanos: 100,
            system_cpu_nanos: 50,
            current_rss_bytes: 512,
            peak_rss_bytes: 1024,
            thread_count: 2,
        };
        let after = ResourceSnapshot {
            user_cpu_nanos: 2_000_000_100,
            system_cpu_nanos: 1_000_000_050,
            current_rss_bytes: 1024,
            peak_rss_bytes: 2048,
            thread_count: 2,
        };
        probe.begin();
        let permit = tracker.try_acquire(32).expect("within configured budget");
        probe.sample();
        drop(permit);
        probe.end();
        let measured = Endpoint::measured(before, after, &probe).expect("valid resource delta");
        assert_eq!(measured.start_rss_bytes, 512);
        assert_eq!(measured.peak_rss_bytes, 2048);
        assert_eq!(measured.cpu_seconds, 3.0);
        assert_eq!(measured.sampled_byte_budget_peak_bytes, 32);
        assert_eq!(measured.byte_budget_allocated_bytes_post_drain, 0);
        assert!(
            Endpoint::measured(after, before, &probe).is_err(),
            "reversed snapshots fail closed"
        );
    }

    #[test]
    fn cli_rejects_unqualified_silent_mode_or_unknown_flags() {
        let args = [
            "rpc-bench",
            "fairness",
            "--scenario",
            "fairness.json",
            "--require-qualified",
        ]
        .map(str::to_string);
        assert!(
            parse_options(&args, false)
                .expect("valid CLI")
                .require_qualified
        );
        assert!(
            parse_options(&args, true).is_err(),
            "server must reject client-only flag"
        );
        let invalid = [
            "rpc-bench",
            "fairness",
            "--scenario",
            "fairness.json",
            "--unknown",
        ]
        .map(str::to_string);
        assert!(parse_options(&invalid, false).is_err());
    }
}
