//! Implementation of the official gRPC `WorkerService` client workload driver and statistics accounting.
//!
//! Provides client lifecycle management, load generation driving with `LoadGenerator`,
//! mark reset semantics, official gRPC histogram calculation matching `grpc/support/histogram.c`,
//! CPU and resource accounting, and clean shutdown adhering to the official gRPC benchmark control protocol.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    missing_docs,
    reason = "bench worker client service implementation"
)]

pub mod proto {
    pub use crate::worker_server::proto::*;
}

pub use proto::*;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pbrs_grpc::{Request, Response, Status, Streaming};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::benchmark_service::BenchmarkServiceClient;
use crate::load::{LoadConfig, LoadGenerator, RpcCallError};
use crate::resources::ResourceSnapshot;
use crate::worker_server::require_snapshot;

pub(crate) const MAX_WORKER_CLIENT_CHANNELS: usize = 64;
pub(crate) const MAX_WORKER_IN_FLIGHT_RPCS: usize = 256;
pub(crate) const MAX_WORKER_HISTOGRAM_BUCKETS: usize = 65_536;
const WORKER_RPC_TIMEOUT: Duration = Duration::from_secs(5);

/// High-resolution latency histogram based on `grpc/support/histogram.c`.
///
/// Buckets are exponentially spaced:
/// - Bucket 0 covers `[0, 1 + resolution)`.
/// - Bucket n (`n >= 1`) covers `[(1 + resolution)^n, (1 + resolution)^(n + 1))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    resolution: f64,
    max_possible: f64,
    multiplier: f64,
    one_on_log_multiplier: f64,
    num_buckets: usize,
    buckets: Vec<u32>,
    min_seen: f64,
    max_seen: f64,
    sum: f64,
    sum_of_squares: f64,
    count: f64,
}

impl Histogram {
    /// Create a new histogram with exponential buckets matching `grpc/support/histogram.c`.
    pub fn new(resolution: f64, max_possible: f64) -> Result<Self, Status> {
        if !resolution.is_finite()
            || !max_possible.is_finite()
            || resolution <= 0.0
            || max_possible <= resolution
        {
            return Err(Status::invalid_argument(format!(
                "invalid histogram params: resolution={resolution}, max_possible={max_possible}"
            )));
        }
        let multiplier = 1.0 + resolution;
        let one_on_log_multiplier = 1.0 / multiplier.ln();
        let num_buckets = Self::bucket_for_unchecked(max_possible, one_on_log_multiplier)
            .checked_add(1)
            .ok_or_else(|| Status::resource_exhausted("histogram bucket count overflow"))?;
        if num_buckets <= 1 || num_buckets > MAX_WORKER_HISTOGRAM_BUCKETS {
            return Err(Status::invalid_argument(format!(
                "histogram bucket count {num_buckets} exceeds supported range 2..={MAX_WORKER_HISTOGRAM_BUCKETS}"
            )));
        }
        Ok(Self {
            resolution,
            max_possible,
            multiplier,
            one_on_log_multiplier,
            num_buckets,
            buckets: vec![0; num_buckets],
            min_seen: max_possible,
            max_seen: 0.0,
            sum: 0.0,
            sum_of_squares: 0.0,
            count: 0.0,
        })
    }

    /// Number of buckets in the histogram.
    #[must_use]
    pub fn num_buckets(&self) -> usize {
        self.num_buckets
    }

    /// Resolution of the histogram.
    #[must_use]
    pub fn resolution(&self) -> f64 {
        self.resolution
    }

    /// Maximum possible value allowed by the histogram.
    #[must_use]
    pub fn max_possible(&self) -> f64 {
        self.max_possible
    }

    fn bucket_for_unchecked(x: f64, one_on_log_multiplier: f64) -> usize {
        (x.ln() * one_on_log_multiplier) as usize
    }

    /// Determine the bucket index for a given value x.
    pub fn bucket_for(&self, x: f64) -> usize {
        let clamped = x.clamp(1.0, self.max_possible);
        let b = Self::bucket_for_unchecked(clamped, self.one_on_log_multiplier);
        b.min(self.num_buckets - 1)
    }

    /// Add a sample observation in nanoseconds.
    pub fn add(&mut self, x: f64) {
        let x = if x.is_nan() || x < 0.0 { 0.0 } else { x };
        self.sum += x;
        self.sum_of_squares += x * x;
        self.count += 1.0;
        if x < self.min_seen {
            self.min_seen = x;
        }
        if x > self.max_seen {
            self.max_seen = x;
        }
        let idx = self.bucket_for(x);
        if idx < self.buckets.len() {
            self.buckets[idx] = self.buckets[idx].saturating_add(1);
        }
    }

    /// Reset all buckets and statistical counters.
    pub fn reset(&mut self) {
        self.sum = 0.0;
        self.sum_of_squares = 0.0;
        self.count = 0.0;
        self.min_seen = self.max_possible;
        self.max_seen = 0.0;
        self.buckets.fill(0);
    }

    /// Merge observations from another histogram matching bucket structure.
    pub fn merge(&mut self, other: &Self) {
        if self.num_buckets != other.num_buckets {
            return;
        }
        self.sum += other.sum;
        self.sum_of_squares += other.sum_of_squares;
        self.count += other.count;
        if other.count > 0.0 && other.min_seen < self.min_seen {
            self.min_seen = other.min_seen;
        }
        if other.max_seen > self.max_seen {
            self.max_seen = other.max_seen;
        }
        for (dst, src) in self.buckets.iter_mut().zip(&other.buckets) {
            *dst = dst.saturating_add(*src);
        }
    }

    /// Convert to official protobuf `HistogramData`.
    pub fn to_data(&self) -> HistogramData {
        let mut data = HistogramData::new();
        data.set_bucket(self.buckets.clone());
        data.set_min_seen(if self.count > 0.0 { self.min_seen } else { 0.0 });
        data.set_max_seen(self.max_seen);
        data.set_sum(self.sum);
        data.set_sum_of_squares(self.sum_of_squares);
        data.set_count(self.count);
        data
    }
}

/// Thread-safe accumulator for client latency histogram and error code counts.
pub struct ClientStatsTracker {
    histogram: Mutex<Histogram>,
    request_results: Mutex<BTreeMap<i32, i64>>,
}

impl ClientStatsTracker {
    /// Create a new tracker wrapping the configured histogram.
    pub fn new(histogram: Histogram) -> Self {
        Self {
            histogram: Mutex::new(histogram),
            request_results: Mutex::new(BTreeMap::new()),
        }
    }

    /// Record a successful RPC with its measured latency in nanoseconds.
    pub fn record_success(&self, latency_nanos: f64) {
        let mut h = self.histogram.lock().unwrap();
        h.add(latency_nanos);
    }

    /// Record a failed RPC with its measured latency in nanoseconds and gRPC status code.
    pub fn record_error(&self, latency_nanos: f64, status_code: i32) {
        {
            let mut h = self.histogram.lock().unwrap();
            h.add(latency_nanos);
        }
        if status_code != 0 {
            let mut r = self.request_results.lock().unwrap();
            *r.entry(status_code).or_insert(0) += 1;
        }
    }

    /// Record an offered call rejected before dispatch, without fabricating a latency sample.
    pub fn record_rejection(&self, status_code: i32) {
        let mut results = self.request_results.lock().unwrap();
        *results.entry(status_code).or_insert(0) += 1;
    }

    /// Generate initial empty `HistogramData` for the setup status message.
    pub fn initial_histogram_data(&self) -> HistogramData {
        let h = self.histogram.lock().unwrap();
        h.to_data()
    }

    /// Capture a snapshot of histogram data and request results, optionally resetting counters.
    pub fn snapshot(&self, reset: bool) -> (HistogramData, Vec<RequestResultCount>) {
        let hist_data = {
            let mut h = self.histogram.lock().unwrap();
            let d = h.to_data();
            if reset {
                h.reset();
            }
            d
        };

        let result_counts = {
            let mut r = self.request_results.lock().unwrap();
            let mut list = Vec::with_capacity(r.len());
            for (&code, &count) in r.iter() {
                let mut rrc = RequestResultCount::new();
                rrc.set_status_code(code);
                rrc.set_count(count);
                list.push(rrc);
            }
            if reset {
                r.clear();
            }
            list
        };

        (hist_data, result_counts)
    }
}

pub(crate) async fn track_worker_rpc<F>(
    tracker: &ClientStatsTracker,
    timeout: Duration,
    call: F,
) -> Result<(), RpcCallError>
where
    F: Future<Output = Result<(), Status>>,
{
    let start = Instant::now();
    match tokio::time::timeout(timeout, call).await {
        Ok(Ok(())) => {
            tracker.record_success(start.elapsed().as_nanos() as f64);
            Ok(())
        }
        Ok(Err(status)) => {
            tracker.record_error(start.elapsed().as_nanos() as f64, status.code() as i32);
            Err(RpcCallError::Status(format!("{:?}", status.code())))
        }
        Err(_) => {
            tracker.record_error(
                timeout.as_nanos() as f64,
                pbrs_grpc::Code::DeadlineExceeded as i32,
            );
            Err(RpcCallError::Timeout)
        }
    }
}

pub(crate) async fn stop_owned_generator(
    cancel: &tokio::sync::watch::Sender<bool>,
    handle: &mut tokio::task::JoinHandle<()>,
) -> Result<(), Status> {
    let _ = cancel.send(true);
    handle.abort();
    match handle.await {
        Ok(()) => Ok(()),
        Err(error) if error.is_cancelled() => Ok(()),
        Err(error) => Err(Status::internal(format!(
            "benchmark client generator failed during shutdown: {error}"
        ))),
    }
}

async fn fail_after_client_cleanup(
    tx: pbrs_grpc::StreamSender<ClientStatus>,
    status: Status,
    cancel: &tokio::sync::watch::Sender<bool>,
    handle: &mut tokio::task::JoinHandle<()>,
) {
    let status = match stop_owned_generator(cancel, handle).await {
        Ok(()) => status,
        Err(error) => Status::internal(format!("{status}; cleanup failed: {error}")),
    };
    tx.fail(status).await;
}

pub(crate) fn unsupported_client_option(config: &ClientConfig) -> Option<&'static str> {
    if config.has_security_params() {
        Some("security_params")
    } else if config.async_client_threads() > 0 {
        Some("async_client_threads")
    } else if config.core_limit() > 0 {
        Some("core_limit")
    } else if !config.core_list().is_empty() {
        Some("core_list")
    } else if config.distribute_load_across_threads() {
        Some("distribute_load_across_threads")
    } else if config.threads_per_cq() != 0 {
        Some("threads_per_cq")
    } else if config.messages_per_stream() != 0 {
        Some("messages_per_stream")
    } else if config.use_coalesce_api() {
        Some("use_coalesce_api")
    } else if config.median_latency_collection_interval_millis() != 0 {
        Some("median_latency_collection_interval_millis")
    } else if config.client_processes() != 0 {
        Some("client_processes")
    } else if !config.channel_args().is_empty() {
        Some("channel_args")
    } else if config.use_session() {
        Some("use_session")
    } else if !config.other_client_api().as_bytes().is_empty() {
        Some("other_client_api")
    } else {
        None
    }
}

pub(crate) fn checked_client_capacity(config: &ClientConfig) -> Result<(usize, usize), Status> {
    let channels = usize::try_from(config.client_channels())
        .ok()
        .filter(|&count| count > 0)
        .ok_or_else(|| Status::invalid_argument("client_channels must be positive"))?;
    let per_channel = usize::try_from(config.outstanding_rpcs_per_channel())
        .ok()
        .filter(|&count| count > 0)
        .ok_or_else(|| Status::invalid_argument("outstanding_rpcs_per_channel must be positive"))?;
    if channels > MAX_WORKER_CLIENT_CHANNELS {
        return Err(Status::resource_exhausted(format!(
            "client_channels {channels} exceeds the worker limit of {MAX_WORKER_CLIENT_CHANNELS}"
        )));
    }
    let total = channels.checked_mul(per_channel).ok_or_else(|| {
        Status::resource_exhausted("client channel/outstanding RPC product overflow")
    })?;
    if total > MAX_WORKER_IN_FLIGHT_RPCS {
        return Err(Status::resource_exhausted(format!(
            "client_channels * outstanding_rpcs_per_channel ({total}) exceeds the worker limit of {MAX_WORKER_IN_FLIGHT_RPCS}"
        )));
    }
    Ok((channels, total))
}

pub(crate) fn acquire_channel_slot(
    slots: &[Arc<Semaphore>],
    start: usize,
) -> Result<Option<(usize, OwnedSemaphorePermit)>, Status> {
    if slots.is_empty() {
        return Err(Status::internal("benchmark client has no channel slots"));
    }
    for offset in 0..slots.len() {
        let index = start.wrapping_add(offset) % slots.len();
        match slots[index].clone().try_acquire_owned() {
            Ok(permit) => return Ok(Some((index, permit))),
            Err(TryAcquireError::NoPermits) => {}
            Err(TryAcquireError::Closed) => {
                return Err(Status::internal("benchmark client channel slot closed"));
            }
        }
    }
    Ok(None)
}

/// Start and manage the benchmark client workload, marks, and statistics accounting.
pub async fn run_client(
    request: Request<Streaming<ClientArgs>>,
) -> Result<Response<Streaming<ClientStatus>>, Status> {
    let mut in_stream = request.into_inner();
    let (tx, out_stream) = Streaming::channel(32);

    tokio::spawn(async move {
        // 1. Read first message: must be ClientConfig setup
        let first_arg = match in_stream.message().await {
            Ok(Some(arg)) => arg,
            Ok(None) => {
                tx.fail(Status::invalid_argument(
                    "empty ClientArgs stream: expected ClientConfig setup",
                ))
                .await;
                return;
            }
            Err(e) => {
                tx.fail(e).await;
                return;
            }
        };

        if !first_arg.has_setup() {
            tx.fail(Status::invalid_argument(
                "first request in stream must specify ClientConfig setup",
            ))
            .await;
            return;
        }

        let cfg = first_arg.setup();

        // 2. Validate configuration
        if cfg.server_targets().is_empty() {
            tx.fail(Status::invalid_argument("server_targets must not be empty"))
                .await;
            return;
        }
        let (num_channels, total_concurrency) = match checked_client_capacity(cfg) {
            Ok(capacity) => capacity,
            Err(status) => {
                tx.fail(status).await;
                return;
            }
        };
        if cfg.core_limit() < 0 {
            tx.fail(Status::invalid_argument("core_limit cannot be negative"))
                .await;
            return;
        }
        if cfg.async_client_threads() < 0 {
            tx.fail(Status::invalid_argument(
                "async_client_threads cannot be negative",
            ))
            .await;
            return;
        }
        if cfg.client_type() != ClientType::AsyncClient {
            tx.fail(Status::invalid_argument(format!(
                "unsupported client_type {:?}: only ASYNC_CLIENT is implemented",
                cfg.client_type()
            )))
            .await;
            return;
        }
        if let Some(option) = unsupported_client_option(cfg) {
            tx.fail(Status::invalid_argument(format!(
                "unsupported client config option {option}"
            )))
            .await;
            return;
        }
        // Protocol: only HTTP2 (0) supported
        if cfg.protocol() != Protocol::Http2 {
            tx.fail(Status::invalid_argument(format!(
                "unsupported protocol: {:?}",
                cfg.protocol()
            )))
            .await;
            return;
        }
        // RpcType: UNARY (0) or STREAMING (1)
        if cfg.rpc_type() != RpcType::Unary && cfg.rpc_type() != RpcType::Streaming {
            tx.fail(Status::invalid_argument(format!(
                "unsupported rpc_type: only UNARY and STREAMING supported, got {:?}",
                cfg.rpc_type()
            )))
            .await;
            return;
        }
        // LoadParams: closed_loop or poisson
        if !cfg.has_load_params() {
            tx.fail(Status::invalid_argument(
                "missing load_params: expected closed_loop or poisson",
            ))
            .await;
            return;
        }
        if !cfg.load_params().has_closed_loop() && !cfg.load_params().has_poisson() {
            tx.fail(Status::invalid_argument(
                "unsupported load_params: expected closed_loop or poisson",
            ))
            .await;
            return;
        }
        if cfg.load_params().has_poisson() {
            let offered_load = cfg.load_params().poisson().offered_load();
            if !offered_load.is_finite() || offered_load <= 0.0 {
                tx.fail(Status::invalid_argument(
                    "poisson offered_load must be finite and strictly positive",
                ))
                .await;
                return;
            }
        }
        if cfg.has_payload_config() {
            let payload = cfg.payload_config();
            if payload.has_bytebuf_params() || payload.has_complex_params() {
                tx.fail(Status::invalid_argument(
                    "unsupported payload_config: only simple_params proto messages are implemented",
                ))
                .await;
                return;
            }
            if !payload.has_simple_params() {
                tx.fail(Status::invalid_argument(
                    "payload_config must specify simple_params",
                ))
                .await;
                return;
            }
            let simple = payload.simple_params();
            if simple.req_size() < 0 || simple.resp_size() < 0 {
                tx.fail(Status::invalid_argument(
                    "negative simple_params payload size",
                ))
                .await;
                return;
            }
            let cap = i32::try_from(crate::benchmark_service::MAX_BENCHMARK_PAYLOAD_SIZE)
                .expect("benchmark worker body cap fits i32");
            if simple.req_size() > cap || simple.resp_size() > cap {
                tx.fail(Status::resource_exhausted(
                    "simple_params payload exceeds the 4 MiB benchmark worker limit",
                ))
                .await;
                return;
            }
        }
        // HistogramParams validation
        let (resolution, max_possible) = if cfg.has_histogram_params() {
            let hp = cfg.histogram_params();
            let res = hp.resolution();
            let max_p = hp.max_possible();
            if res == 0.0 && max_p == 0.0 {
                (0.01, 60_000_000_000.0)
            } else if !res.is_finite() || !max_p.is_finite() || res <= 0.0 || max_p <= res {
                tx.fail(Status::invalid_argument(format!(
                    "invalid histogram_params: resolution={res}, max_possible={max_p}"
                )))
                .await;
                return;
            } else {
                (res, max_p)
            }
        } else {
            (0.01, 60_000_000_000.0)
        };

        let histogram = match Histogram::new(resolution, max_possible) {
            Ok(h) => h,
            Err(st) => {
                tx.fail(st).await;
                return;
            }
        };

        // 3. Connect channels to target servers
        let targets: Vec<String> = match cfg
            .server_targets()
            .iter()
            .map(|target| target.to_str().map(str::to_string))
            .collect()
        {
            Ok(targets) => targets,
            Err(error) => {
                tx.fail(Status::invalid_argument(format!(
                    "server_targets contains invalid UTF-8: {error}"
                )))
                .await;
                return;
            }
        };
        let mut channels = Vec::with_capacity(num_channels);
        for i in 0..num_channels {
            let target_str = targets
                .get(i % targets.len())
                .map(|s| s.as_str())
                .expect("validated server_targets are nonempty");
            let clean_target = target_str
                .strip_prefix("dns:///")
                .or_else(|| target_str.strip_prefix("ipv4:"))
                .or_else(|| target_str.strip_prefix("http://"))
                .unwrap_or(target_str);

            let channel = match pbrs_grpc::Channel::connect(clean_target).await {
                Ok(ch) => ch,
                Err(e) => {
                    tx.fail(Status::internal(format!(
                        "failed to connect to channel {i} at {target_str}: {e}"
                    )))
                    .await;
                    return;
                }
            };
            channels.push(BenchmarkServiceClient::new(channel));
        }

        // 4. Configure LoadGenerator and stats tracker
        let load_cfg = if cfg.load_params().has_closed_loop() {
            LoadConfig::closed(total_concurrency, Duration::from_secs(86400 * 365))
        } else {
            let offered_load = cfg.load_params().poisson().offered_load();
            LoadConfig::open_poisson(
                offered_load,
                0x5eed_2026_0918,
                Duration::from_secs(86400 * 365),
            )
        }
        .with_max_in_flight(total_concurrency)
        .without_raw_samples()
        .without_timeout();

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let stats_tracker = Arc::new(ClientStatsTracker::new(histogram));

        let (req_size, resp_size) = if cfg.has_payload_config() {
            let simple = cfg.payload_config().simple_params();
            (simple.req_size(), simple.resp_size())
        } else {
            (0, 0)
        };

        let mut template_req = crate::benchmark_service::SimpleRequest::new();
        if req_size > 0 {
            let mut payload = crate::benchmark_service::Payload::new();
            payload.set_body(vec![0u8; req_size as usize]);
            template_req.set_payload(payload);
        }
        if resp_size > 0 {
            template_req.set_response_size(resp_size);
        }
        let template_req = Arc::new(template_req);

        let channels = Arc::new(channels);
        let slots = Arc::new(
            (0..num_channels)
                .map(|_| Arc::new(Semaphore::new(total_concurrency / num_channels)))
                .collect::<Vec<_>>(),
        );
        let rr_counter = Arc::new(AtomicUsize::new(0));
        let rpc_type = cfg.rpc_type();

        let invoke_channels = channels.clone();
        let invoke_slots = slots.clone();
        let invoke_rr = rr_counter.clone();
        let invoke_tracker = stats_tracker.clone();
        let invoke_template = template_req.clone();
        let invoke_cancel = cancel_rx.clone();

        let invoke = move || {
            let channels = invoke_channels.clone();
            let slots = invoke_slots.clone();
            let rr = invoke_rr.clone();
            let tracker = invoke_tracker.clone();
            let template = invoke_template.clone();
            let mut cancel_watch = invoke_cancel.clone();

            async move {
                if *cancel_watch.borrow() {
                    std::future::pending::<()>().await;
                    return Err(RpcCallError::Other("cancelled".to_string()));
                }

                let (idx, _slot) =
                    match acquire_channel_slot(&slots, rr.fetch_add(1, Ordering::Relaxed)) {
                        Ok(Some(slot)) => slot,
                        Ok(None) => {
                            tracker.record_rejection(pbrs_grpc::Code::ResourceExhausted as i32);
                            return Err(RpcCallError::Status("RESOURCE_EXHAUSTED".into()));
                        }
                        Err(status) => {
                            tracker.record_rejection(status.code() as i32);
                            return Err(RpcCallError::Status(format!("{:?}", status.code())));
                        }
                    };
                let client = &channels[idx];

                tokio::select! {
                    _ = cancel_watch.changed() => {
                        std::future::pending::<()>().await;
                        Err(RpcCallError::Other("cancelled".to_string()))
                    }
                    res = track_worker_rpc(&tracker, WORKER_RPC_TIMEOUT, async {
                        if rpc_type == RpcType::Unary {
                            client.unary_call(Request::new((*template).clone())).await.map(|_| ())
                        } else {
                            let (sender, call) = client.streaming_call(Request::new(()));
                            sender.send((*template).clone()).await.map_err(|e| Status::internal(e.to_string()))?;
                            let mut stream = call.await?.into_inner();
                            let _ = stream.message().await?;
                            drop(sender);
                            Ok(())
                        }
                    }) => res,
                }
            }
        };

        let generator = LoadGenerator::new(load_cfg);
        let rejection_tracker = stats_tracker.clone();
        let mut gen_handle = tokio::spawn(async move {
            generator
                .run_with_rejections(invoke, || {
                    rejection_tracker.record_rejection(pbrs_grpc::Code::ResourceExhausted as i32);
                })
                .await;
        });

        // 5. Build and send initial ClientStatus
        let initial_snapshot =
            match require_snapshot(ResourceSnapshot::capture(), "RunClient initial") {
                Ok(snapshot) => snapshot,
                Err(status) => {
                    fail_after_client_cleanup(tx, status, &cancel_tx, &mut gen_handle).await;
                    return;
                }
            };

        let mut initial_status = ClientStatus::new();
        let mut initial_stats = ClientStats::new();
        initial_stats.set_time_elapsed(0.0);
        initial_stats.set_time_user(0.0);
        initial_stats.set_time_system(0.0);
        initial_stats.set_latencies(stats_tracker.initial_histogram_data());
        initial_status.set_stats(initial_stats);

        if tx.send(initial_status).await.is_err() {
            if let Err(error) = stop_owned_generator(&cancel_tx, &mut gen_handle).await {
                eprintln!("{error}");
            }
            return;
        }

        // 6. Loop for subsequent Mark requests and stream termination
        let mut baseline_snapshot = initial_snapshot;
        let mut baseline_time = Instant::now();

        loop {
            let arg = match in_stream.message().await {
                Ok(Some(a)) => a,
                Ok(None) => break, // Closing inbound stream triggers clean termination
                Err(e) => {
                    fail_after_client_cleanup(tx, e, &cancel_tx, &mut gen_handle).await;
                    return;
                }
            };

            // Reject duplicate setup
            if arg.has_setup() {
                fail_after_client_cleanup(
                    tx,
                    Status::invalid_argument("duplicate ClientConfig setup received"),
                    &cancel_tx,
                    &mut gen_handle,
                )
                .await;
                return;
            }

            if !arg.has_mark() {
                fail_after_client_cleanup(
                    tx,
                    Status::invalid_argument("expected Mark in subsequent ClientArgs"),
                    &cancel_tx,
                    &mut gen_handle,
                )
                .await;
                return;
            }

            let mark = arg.mark();
            let now = Instant::now();
            let time_elapsed = (now - baseline_time).as_secs_f64();

            let current_snapshot =
                match require_snapshot(ResourceSnapshot::capture(), "RunClient Mark") {
                    Ok(snapshot) => snapshot,
                    Err(status) => {
                        fail_after_client_cleanup(tx, status, &cancel_tx, &mut gen_handle).await;
                        return;
                    }
                };
            let delta = baseline_snapshot.delta_to(&current_snapshot);

            let (latencies, request_results) = stats_tracker.snapshot(mark.reset());

            let mut stats = ClientStats::new();
            stats.set_time_elapsed(time_elapsed);
            stats.set_time_user(delta.user_cpu_seconds);
            stats.set_time_system(delta.system_cpu_seconds);
            stats.set_latencies(latencies);
            stats.set_request_results(request_results);

            if mark.reset() {
                baseline_snapshot = current_snapshot;
                baseline_time = now;
            }

            let mut status = ClientStatus::new();
            status.set_stats(stats);

            if tx.send(status).await.is_err() {
                break;
            }
        }

        // 7. Clean shutdown: notify cancellation and abort generator
        if let Err(status) = stop_owned_generator(&cancel_tx, &mut gen_handle).await {
            tx.fail(status).await;
        }
        // Dropping tx ends with OK only after the owned generator stops.
    });

    Ok(Response::new(out_stream))
}

/// Implementation of the official gRPC `WorkerService` supporting both server and client benchmarks.
#[derive(Clone, Default)]
pub struct WorkerServiceImpl {
    server_impl: crate::worker_server::WorkerServiceImpl,
}

impl WorkerServiceImpl {
    /// Create a new worker service instance without an external shutdown trigger.
    #[must_use]
    pub fn new() -> Self {
        Self {
            server_impl: crate::worker_server::WorkerServiceImpl::new(),
        }
    }

    /// Create a new worker service instance with a shutdown watch channel sender.
    #[must_use]
    pub fn with_shutdown(quit_tx: tokio::sync::watch::Sender<bool>) -> Self {
        Self {
            server_impl: crate::worker_server::WorkerServiceImpl::with_shutdown(quit_tx),
        }
    }
}

impl WorkerService for WorkerServiceImpl {
    async fn core_count(
        &self,
        request: Request<CoreRequest>,
    ) -> Result<Response<CoreResponse>, Status> {
        self.server_impl.core_count(request).await
    }

    async fn quit_worker(&self, request: Request<Void>) -> Result<Response<Void>, Status> {
        self.server_impl.quit_worker(request).await
    }

    async fn run_server(
        &self,
        request: Request<Streaming<ServerArgs>>,
    ) -> Result<Response<Streaming<ServerStatus>>, Status> {
        self.server_impl.run_server(request).await
    }

    async fn run_client(
        &self,
        request: Request<Streaming<ClientArgs>>,
    ) -> Result<Response<Streaming<ClientStatus>>, Status> {
        run_client(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_synthetic_latencies() {
        let mut h = Histogram::new(0.01, 60_000_000_000.0).unwrap();
        assert_eq!(h.num_buckets(), 2495);

        // Bucket boundaries
        assert_eq!(h.bucket_for(0.0), 0);
        assert_eq!(h.bucket_for(0.5), 0);
        assert_eq!(h.bucket_for(1.0), 0);
        assert_eq!(h.bucket_for(1.009), 0);
        assert_eq!(h.bucket_for(1.01), 1);

        // 100 us = 100_000 ns
        let expected_bucket_100k = (100_000.0_f64.ln() / 1.01_f64.ln()) as usize;
        assert_eq!(h.bucket_for(100_000.0), expected_bucket_100k);

        h.add(100_000.0);
        let data = h.to_data();
        assert_eq!(data.count(), 1.0);
        assert_eq!(data.sum(), 100_000.0);
        assert_eq!(data.min_seen(), 100_000.0);
        assert_eq!(data.max_seen(), 100_000.0);
        assert_eq!(data.sum_of_squares(), 10_000_000_000.0);
        assert_eq!(data.bucket().get(expected_bucket_100k), Some(1));

        // 500 us = 500_000 ns
        let expected_bucket_500k = (500_000.0_f64.ln() / 1.01_f64.ln()) as usize;
        h.add(500_000.0);
        let data2 = h.to_data();
        assert_eq!(data2.count(), 2.0);
        assert_eq!(data2.sum(), 600_000.0);
        assert_eq!(data2.min_seen(), 100_000.0);
        assert_eq!(data2.max_seen(), 500_000.0);
        assert_eq!(
            data2.sum_of_squares(),
            100_000.0 * 100_000.0 + 500_000.0 * 500_000.0
        );
        assert_eq!(data2.bucket().get(expected_bucket_500k), Some(1));

        // Value exceeding max_possible clamped to last bucket
        let max_p = 60_000_000_000.0;
        let last_bucket = h.num_buckets() - 1;
        assert_eq!(h.bucket_for(max_p * 2.0), last_bucket);
        h.add(max_p * 2.0);
        let data3 = h.to_data();
        assert_eq!(data3.count(), 3.0);
        assert_eq!(data3.max_seen(), max_p * 2.0);
        assert_eq!(data3.bucket().get(last_bucket), Some(1));

        // Reset
        h.reset();
        let data_reset = h.to_data();
        assert_eq!(data_reset.count(), 0.0);
        assert_eq!(data_reset.sum(), 0.0);
        assert_eq!(data_reset.min_seen(), 0.0);
        assert_eq!(data_reset.max_seen(), 0.0);
        assert_eq!(data_reset.bucket().get(expected_bucket_100k), Some(0));
    }

    #[test]
    fn test_stats_tracker_error_accounting() {
        let h = Histogram::new(0.01, 60_000_000_000.0).unwrap();
        let tracker = ClientStatsTracker::new(h);

        tracker.record_success(50_000.0);
        tracker.record_error(100_000.0, 4); // DEADLINE_EXCEEDED
        tracker.record_error(200_000.0, 14); // UNAVAILABLE
        tracker.record_error(300_000.0, 14); // UNAVAILABLE again

        let (hist, req_results) = tracker.snapshot(false);
        assert_eq!(hist.count(), 4.0);
        assert_eq!(hist.sum(), 650_000.0);
        assert_eq!(req_results.len(), 2);

        let mut results_map = BTreeMap::new();
        for r in req_results {
            results_map.insert(r.status_code(), r.count());
        }
        assert_eq!(results_map.get(&4), Some(&1));
        assert_eq!(results_map.get(&14), Some(&2));

        // Snapshot with reset
        let (hist2, req_results2) = tracker.snapshot(true);
        assert_eq!(hist2.count(), 4.0);
        assert_eq!(req_results2.len(), 2);

        // Post-reset snapshot
        let (hist3, req_results3) = tracker.snapshot(false);
        assert_eq!(hist3.count(), 0.0);
        assert_eq!(hist3.sum(), 0.0);
        assert_eq!(req_results3.len(), 0);
    }
}
