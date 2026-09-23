//! Open-loop and closed-loop load generation for gRPC transport benchmarks.
//!
//! Provides offered load injection without coordinated omission by decoupling
//! scheduled arrival times from server response times, per `docs/benchmark-contract.md`
//! Section 4.
//!
//! Supports:
//! - Closed-loop concurrency (fixed in-flight calls)
//! - Open-loop constant rate (paced at fixed 1/lambda intervals)
//! - Open-loop Poisson arrival schedules (exponential inter-arrival with seedable PRNG)
//! - Tracking scheduled start time vs actual issue time (scheduling lag)
//! - Tracking timeouts, unstarted calls, rejected calls (queue overflow), and unfinished requests
//! - Bounded in-flight queue to prevent unbounded memory growth on server stalls

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    missing_docs,
    reason = "bench load generator"
)]

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::report::{LatencyDistribution, RpcMetrics, SchedulingLagNanos};

/// Seedable 64-bit SplitMix64 pseudo-random number generator for deterministic Poisson arrivals.
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    /// Create a new PRNG with a 64-bit seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generate the next pseudo-random 64-bit integer.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Generate the next pseudo-random `f64` in the half-open interval `(0.0, 1.0]`.
    pub fn next_f64(&mut self) -> f64 {
        // Use upper 53 bits + 1 to guarantee non-zero (so ln(u) is finite)
        let bits = (self.next_u64() >> 11) + 1;
        bits as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Sample an exponential distribution with parameter `rate_qps` (mean interval = 1.0 / rate_qps).
    /// Returns the inter-arrival interval in nanoseconds.
    pub fn next_exponential_nanos(&mut self, rate_qps: f64) -> u64 {
        assert!(rate_qps > 0.0, "rate_qps must be strictly positive");
        let u = self.next_f64();
        let secs = -u.ln() / rate_qps;
        let nanos = (secs * 1_000_000_000.0).round();
        if nanos <= 0.0 {
            1
        } else if nanos > u64::MAX as f64 {
            u64::MAX
        } else {
            nanos as u64
        }
    }
}

/// Mode of arrival schedule distribution for the load generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LoadDistribution {
    /// Closed-loop: client workers wait for response before issuing next request.
    Closed,
    /// Open-loop with constant inter-arrival pacing (1/lambda).
    Constant,
    /// Open-loop with Poisson arrival process (exponential inter-arrival times).
    Poisson,
}

impl FromStr for LoadDistribution {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "closed" | "closed-loop" | "closed_loop" => Ok(Self::Closed),
            "constant" | "open-constant" | "open_constant" | "paced" => Ok(Self::Constant),
            "poisson" | "open-poisson" | "open_poisson" => Ok(Self::Poisson),
            other => Err(format!(
                "unknown load distribution '{other}': expected closed, constant, or poisson"
            )),
        }
    }
}

impl fmt::Display for LoadDistribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Constant => write!(f, "constant"),
            Self::Poisson => write!(f, "poisson"),
        }
    }
}

/// Error classification for benchmark RPC calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcCallError {
    /// gRPC status code error (e.g., "UNAVAILABLE", "RESOURCE_EXHAUSTED").
    Status(String),
    /// Call deadline/timeout exceeded.
    Timeout,
    /// Call rejected due to bounded in-flight queue overflow.
    QueueOverflow,
    /// Call remained unfinished at end of measurement and drain period.
    Unfinished,
    /// Other transport or execution error.
    Other(String),
}

impl RpcCallError {
    /// Canonical status code string representation.
    pub fn code_name(&self) -> String {
        match self {
            Self::Status(s) => s.clone(),
            Self::Timeout => "DEADLINE_EXCEEDED".to_string(),
            Self::QueueOverflow => "QUEUE_OVERFLOW".to_string(),
            Self::Unfinished => "UNFINISHED".to_string(),
            Self::Other(s) => s.clone(),
        }
    }
}

impl From<String> for RpcCallError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

impl From<&str> for RpcCallError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

/// Configuration for the benchmark load generator.
#[derive(Debug, Clone)]
pub struct LoadConfig {
    /// Arrival schedule distribution.
    pub distribution: LoadDistribution,
    /// Target rate in queries per second (QPS). Required for Constant and Poisson.
    pub rate_qps: Option<f64>,
    /// Concurrency level (number of in-flight worker loops) for closed-loop testing.
    pub concurrency: usize,
    /// Maximum in-flight RPCs allowed before rejecting new requests (queue overflow).
    pub max_in_flight: usize,
    /// Random seed for Poisson arrival generation.
    pub seed: u64,
    /// Duration of the measurement window.
    pub duration: Duration,
    /// Optional per-RPC timeout.
    pub timeout: Option<Duration>,
    /// Drain period to wait for in-flight requests to complete after duration expires.
    pub drain_timeout: Duration,
    /// Optional cap on maximum calls to schedule (useful for bounded count tests).
    pub max_calls: Option<u64>,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            distribution: LoadDistribution::Closed,
            rate_qps: None,
            concurrency: 1,
            max_in_flight: 1000,
            seed: 0x5eed_2026_0918,
            duration: Duration::from_secs(5),
            timeout: Some(Duration::from_secs(5)),
            drain_timeout: Duration::from_secs(2),
            max_calls: None,
        }
    }
}

impl LoadConfig {
    /// Configure closed-loop load with fixed concurrency.
    pub fn closed(concurrency: usize, duration: Duration) -> Self {
        Self {
            distribution: LoadDistribution::Closed,
            concurrency: concurrency.max(1),
            duration,
            ..Default::default()
        }
    }

    /// Configure open-loop constant rate load.
    pub fn open_constant(rate_qps: f64, duration: Duration) -> Self {
        Self {
            distribution: LoadDistribution::Constant,
            rate_qps: Some(rate_qps),
            duration,
            ..Default::default()
        }
    }

    /// Configure open-loop Poisson arrival load with explicit seed.
    pub fn open_poisson(rate_qps: f64, seed: u64, duration: Duration) -> Self {
        Self {
            distribution: LoadDistribution::Poisson,
            rate_qps: Some(rate_qps),
            seed,
            duration,
            ..Default::default()
        }
    }

    /// Set maximum in-flight cap (bounded queue).
    pub fn with_max_in_flight(mut self, cap: usize) -> Self {
        self.max_in_flight = cap.max(1);
        self
    }

    /// Set per-call timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set drain timeout.
    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    /// Set maximum number of calls to schedule.
    pub fn with_max_calls(mut self, max: u64) -> Self {
        self.max_calls = Some(max);
        self
    }
}

/// Execution outcome and measurement record from a [`LoadGenerator`] run.
#[derive(Debug, Clone)]
pub struct LoadRecord {
    pub distribution: LoadDistribution,
    /// Total number of RPC calls offered by the arrival schedule.
    pub offered_calls: u64,
    /// Total number of RPC calls dispatched into the transport pipeline.
    pub dispatched_calls: u64,
    /// Total number of RPC calls that completed (either success or failure).
    pub completed_calls: u64,
    /// Number of RPC calls completed successfully.
    pub successful_calls: u64,
    /// Number of RPC calls that failed (including timeouts and errors).
    pub failed_calls: u64,
    /// Number of RPC calls that timed out before receiving a response.
    pub timed_out_calls: u64,
    /// Number of offered calls rejected due to bounded in-flight queue overflow.
    pub rejected_calls: u64,
    /// Number of offered calls that were never started (rejected or unstarted at stop).
    pub unstarted_calls: u64,
    /// Number of dispatched calls that remained unfinished when the measurement and drain window ended.
    pub unfinished_calls: u64,
    /// Scheduling lag measurements in nanoseconds (T_actual - T_sched).
    pub scheduling_lags_nanos: Vec<u64>,
    /// Service latency measurements in nanoseconds (T_complete - T_actual).
    pub service_latencies_nanos: Vec<u64>,
    /// End-to-end latency measurements in nanoseconds (T_complete - T_sched) including scheduling lag.
    pub e2e_latencies_nanos: Vec<u64>,
    /// Wall-clock duration of the measurement phase.
    pub duration: Duration,
    /// Breakdown of error status codes.
    pub status_errors: BTreeMap<String, u64>,
}

impl LoadRecord {
    /// Convert to canonical [`RpcMetrics`] for benchmark reports.
    pub fn to_rpc_metrics(&self) -> RpcMetrics {
        let mut m = RpcMetrics::new(
            self.dispatched_calls,
            self.successful_calls,
            self.failed_calls,
            self.duration.as_nanos().min(u64::MAX as u128) as u64,
        );
        m.offered_rpcs = Some(self.offered_calls);
        m.dispatched_rpcs = Some(self.dispatched_calls);
        m.queue_overflows = Some(self.rejected_calls);
        m.timeouts = Some(self.timed_out_calls);
        m.status_errors = self.status_errors.clone();
        m.scheduling_lag_nanos = SchedulingLagNanos::from_samples(&self.scheduling_lags_nanos);
        m
    }

    /// Compute statistical summary of scheduling lag.
    pub fn scheduling_lag_summary(&self) -> Option<SchedulingLagNanos> {
        SchedulingLagNanos::from_samples(&self.scheduling_lags_nanos)
    }

    /// Compute end-to-end latency distribution (accounting for coordinated omission).
    pub fn e2e_latency_distribution(&self) -> Option<LatencyDistribution> {
        LatencyDistribution::from_samples(&self.e2e_latencies_nanos)
    }

    /// Compute service latency distribution (excluding scheduling lag).
    pub fn service_latency_distribution(&self) -> Option<LatencyDistribution> {
        LatencyDistribution::from_samples(&self.service_latencies_nanos)
    }
}

struct GeneratorState {
    offered_calls: AtomicU64,
    dispatched_calls: AtomicU64,
    completed_calls: AtomicU64,
    successful_calls: AtomicU64,
    failed_calls: AtomicU64,
    timed_out_calls: AtomicU64,
    rejected_calls: AtomicU64,
    unstarted_calls: AtomicU64,
    unfinished_calls: AtomicU64,
    active_in_flight: AtomicUsize,
    scheduling_lags: Mutex<Vec<u64>>,
    service_latencies: Mutex<Vec<u64>>,
    e2e_latencies: Mutex<Vec<u64>>,
    status_errors: Mutex<BTreeMap<String, u64>>,
}

impl GeneratorState {
    fn new() -> Self {
        Self {
            offered_calls: AtomicU64::new(0),
            dispatched_calls: AtomicU64::new(0),
            completed_calls: AtomicU64::new(0),
            successful_calls: AtomicU64::new(0),
            failed_calls: AtomicU64::new(0),
            timed_out_calls: AtomicU64::new(0),
            rejected_calls: AtomicU64::new(0),
            unstarted_calls: AtomicU64::new(0),
            unfinished_calls: AtomicU64::new(0),
            active_in_flight: AtomicUsize::new(0),
            scheduling_lags: Mutex::new(Vec::new()),
            service_latencies: Mutex::new(Vec::new()),
            e2e_latencies: Mutex::new(Vec::new()),
            status_errors: Mutex::new(BTreeMap::new()),
        }
    }

    fn record_status_error(&self, code: &str, count: u64) {
        let mut errs = self.status_errors.lock().unwrap();
        *errs.entry(code.to_string()).or_insert(0) += count;
    }

    fn record_scheduling_lag(&self, scheduled: Instant) -> Instant {
        let actual = Instant::now();
        let nanos = actual
            .saturating_duration_since(scheduled)
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        self.scheduling_lags.lock().unwrap().push(nanos);
        actual
    }

    fn record_call_outcome(
        &self,
        t_sched: Instant,
        t_actual: Instant,
        res: Result<(), RpcCallError>,
        timed_out: bool,
        call_timeout: Option<Duration>,
    ) {
        let t_finish = Instant::now();
        self.completed_calls.fetch_add(1, Ordering::Relaxed);

        let service_nanos = if timed_out {
            call_timeout
                .map(|t| t.as_nanos().min(u64::MAX as u128) as u64)
                .unwrap_or_else(|| {
                    t_finish
                        .saturating_duration_since(t_actual)
                        .as_nanos()
                        .min(u64::MAX as u128) as u64
                })
        } else {
            t_finish
                .saturating_duration_since(t_actual)
                .as_nanos()
                .min(u64::MAX as u128) as u64
        };

        let e2e_nanos = if timed_out {
            let base = call_timeout
                .map(|t| t.as_nanos().min(u64::MAX as u128) as u64)
                .unwrap_or_else(|| {
                    t_finish
                        .saturating_duration_since(t_actual)
                        .as_nanos()
                        .min(u64::MAX as u128) as u64
                });
            let lag = t_actual
                .saturating_duration_since(t_sched)
                .as_nanos()
                .min(u64::MAX as u128) as u64;
            base.saturating_add(lag)
        } else {
            t_finish
                .saturating_duration_since(t_sched)
                .as_nanos()
                .min(u64::MAX as u128) as u64
        };

        // Latencies are recorded for ALL completed/timed-out calls (never dropped!)
        {
            self.service_latencies.lock().unwrap().push(service_nanos);
            self.e2e_latencies.lock().unwrap().push(e2e_nanos);
        }

        if timed_out {
            self.timed_out_calls.fetch_add(1, Ordering::Relaxed);
            self.failed_calls.fetch_add(1, Ordering::Relaxed);
            self.record_status_error("DEADLINE_EXCEEDED", 1);
        } else {
            match res {
                Ok(()) => {
                    self.successful_calls.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    self.failed_calls.fetch_add(1, Ordering::Relaxed);
                    self.record_status_error(&err.code_name(), 1);
                }
            }
        }
        // Drain must not observe a free slot before its outcome is recorded.
        self.active_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Load generator driving closed-loop and open-loop benchmark workloads.
#[derive(Debug, Clone)]
pub struct LoadGenerator {
    cfg: LoadConfig,
}

impl LoadGenerator {
    /// Create a load generator with the given configuration.
    pub fn new(cfg: LoadConfig) -> Self {
        Self { cfg }
    }

    /// Execute the load benchmark against an async RPC invocation closure.
    pub async fn run<F, Fut>(&self, invoke: F) -> LoadRecord
    where
        F: Fn() -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = Result<(), RpcCallError>> + Send + 'static,
    {
        match self.cfg.distribution {
            LoadDistribution::Closed => self.run_closed_loop(invoke).await,
            LoadDistribution::Constant => self.run_open_loop_constant(invoke).await,
            LoadDistribution::Poisson => self.run_open_loop_poisson(invoke).await,
        }
    }

    async fn run_closed_loop<F, Fut>(&self, invoke: F) -> LoadRecord
    where
        F: Fn() -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = Result<(), RpcCallError>> + Send + 'static,
    {
        let state = Arc::new(GeneratorState::new());
        let running = Arc::new(AtomicBool::new(true));
        let start_time = Instant::now();
        let end_time = start_time + self.cfg.duration;

        let mut handles = Vec::with_capacity(self.cfg.concurrency);
        for _ in 0..self.cfg.concurrency {
            let invoke = invoke.clone();
            let state = state.clone();
            let running = running.clone();
            let timeout = self.cfg.timeout;
            let max_calls = self.cfg.max_calls;

            handles.push(tokio::spawn(async move {
                while running.load(Ordering::Relaxed) {
                    if let Some(max) = max_calls {
                        if state.offered_calls.load(Ordering::Relaxed) >= max {
                            running.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                    if Instant::now() >= end_time {
                        running.store(false, Ordering::Relaxed);
                        break;
                    }

                    state.offered_calls.fetch_add(1, Ordering::Relaxed);
                    let t_sched = Instant::now();
                    let t_actual = t_sched;
                    state.scheduling_lags.lock().unwrap().push(0);

                    state.dispatched_calls.fetch_add(1, Ordering::Relaxed);
                    state.active_in_flight.fetch_add(1, Ordering::SeqCst);

                    let fut = invoke();
                    let (res, timed_out) = if let Some(t) = timeout {
                        match tokio::time::timeout(t, fut).await {
                            Ok(r) => (r, false),
                            Err(_) => (Err(RpcCallError::Timeout), true),
                        }
                    } else {
                        (fut.await, false)
                    };

                    state.record_call_outcome(t_sched, t_actual, res, timed_out, timeout);
                }
            }));
        }

        // Wait for workers to finish
        for h in handles {
            let _ = h.await;
        }

        self.drain_and_finalize(state, start_time.elapsed()).await
    }

    async fn run_open_loop_constant<F, Fut>(&self, invoke: F) -> LoadRecord
    where
        F: Fn() -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = Result<(), RpcCallError>> + Send + 'static,
    {
        let rate_qps = self
            .cfg
            .rate_qps
            .expect("rate_qps is required for open-loop constant load");
        assert!(rate_qps > 0.0, "rate_qps must be strictly positive");

        let state = Arc::new(GeneratorState::new());
        let semaphore = Arc::new(Semaphore::new(self.cfg.max_in_flight));
        let start_time = Instant::now();
        let end_time = start_time + self.cfg.duration;
        let interval_nanos = (1_000_000_000.0 / rate_qps).round() as u64;

        let mut scheduled_offset_nanos = 0u64;
        let mut count = 0u64;

        while Instant::now() < end_time {
            if let Some(max) = self.cfg.max_calls {
                if count >= max {
                    break;
                }
            }

            let t_sched = start_time + Duration::from_nanos(scheduled_offset_nanos);
            if t_sched >= end_time {
                break;
            }

            state.offered_calls.fetch_add(1, Ordering::Relaxed);
            count += 1;

            // Sleep until scheduled arrival time
            let now = Instant::now();
            if t_sched > now {
                tokio::time::sleep(t_sched - now).await;
            }

            // Bounded in-flight queue: verify permit availability
            match semaphore.clone().try_acquire_owned() {
                Ok(permit) => {
                    state.dispatched_calls.fetch_add(1, Ordering::Relaxed);
                    state.active_in_flight.fetch_add(1, Ordering::SeqCst);

                    let invoke = invoke.clone();
                    let state = state.clone();
                    let timeout = self.cfg.timeout;

                    tokio::spawn(async move {
                        let _permit = permit;
                        let t_actual = state.record_scheduling_lag(t_sched);
                        let fut = invoke();
                        let (res, timed_out) = if let Some(t) = timeout {
                            match tokio::time::timeout(t, fut).await {
                                Ok(r) => (r, false),
                                Err(_) => (Err(RpcCallError::Timeout), true),
                            }
                        } else {
                            (fut.await, false)
                        };

                        state.record_call_outcome(t_sched, t_actual, res, timed_out, timeout);
                    });
                }
                Err(_) => {
                    // Queue overflow! Do not spawn unbounded tasks
                    state.record_scheduling_lag(t_sched);
                    state.rejected_calls.fetch_add(1, Ordering::Relaxed);
                    state.unstarted_calls.fetch_add(1, Ordering::Relaxed);
                    state.record_status_error("QUEUE_OVERFLOW", 1);
                }
            }

            scheduled_offset_nanos = scheduled_offset_nanos.saturating_add(interval_nanos);
        }

        self.drain_and_finalize(state, start_time.elapsed()).await
    }

    async fn run_open_loop_poisson<F, Fut>(&self, invoke: F) -> LoadRecord
    where
        F: Fn() -> Fut + Send + Sync + 'static + Clone,
        Fut: Future<Output = Result<(), RpcCallError>> + Send + 'static,
    {
        let rate_qps = self
            .cfg
            .rate_qps
            .expect("rate_qps is required for open-loop Poisson load");
        assert!(rate_qps > 0.0, "rate_qps must be strictly positive");

        let mut rng = SeededRng::new(self.cfg.seed);
        let state = Arc::new(GeneratorState::new());
        let semaphore = Arc::new(Semaphore::new(self.cfg.max_in_flight));
        let start_time = Instant::now();
        let end_time = start_time + self.cfg.duration;

        let mut scheduled_offset_nanos = 0u64;
        let mut count = 0u64;

        while Instant::now() < end_time {
            if let Some(max) = self.cfg.max_calls {
                if count >= max {
                    break;
                }
            }

            let interval_nanos = rng.next_exponential_nanos(rate_qps);
            scheduled_offset_nanos = scheduled_offset_nanos.saturating_add(interval_nanos);

            let t_sched = start_time + Duration::from_nanos(scheduled_offset_nanos);
            if t_sched >= end_time {
                break;
            }

            state.offered_calls.fetch_add(1, Ordering::Relaxed);
            count += 1;

            // Sleep until scheduled arrival time
            let now = Instant::now();
            if t_sched > now {
                tokio::time::sleep(t_sched - now).await;
            }

            // Bounded in-flight queue: verify permit availability
            match semaphore.clone().try_acquire_owned() {
                Ok(permit) => {
                    state.dispatched_calls.fetch_add(1, Ordering::Relaxed);
                    state.active_in_flight.fetch_add(1, Ordering::SeqCst);

                    let invoke = invoke.clone();
                    let state = state.clone();
                    let timeout = self.cfg.timeout;

                    tokio::spawn(async move {
                        let _permit = permit;
                        let t_actual = state.record_scheduling_lag(t_sched);
                        let fut = invoke();
                        let (res, timed_out) = if let Some(t) = timeout {
                            match tokio::time::timeout(t, fut).await {
                                Ok(r) => (r, false),
                                Err(_) => (Err(RpcCallError::Timeout), true),
                            }
                        } else {
                            (fut.await, false)
                        };

                        state.record_call_outcome(t_sched, t_actual, res, timed_out, timeout);
                    });
                }
                Err(_) => {
                    // Queue overflow! Do not spawn unbounded tasks
                    state.record_scheduling_lag(t_sched);
                    state.rejected_calls.fetch_add(1, Ordering::Relaxed);
                    state.unstarted_calls.fetch_add(1, Ordering::Relaxed);
                    state.record_status_error("QUEUE_OVERFLOW", 1);
                }
            }
        }

        self.drain_and_finalize(state, start_time.elapsed()).await
    }

    async fn drain_and_finalize(
        &self,
        state: Arc<GeneratorState>,
        measurement_dur: Duration,
    ) -> LoadRecord {
        let drain_deadline = Instant::now() + self.cfg.drain_timeout;
        while state.active_in_flight.load(Ordering::SeqCst) > 0 && Instant::now() < drain_deadline {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }

        // Account for any remaining in-flight requests that did not finish within the drain window
        let unfinished = state.active_in_flight.load(Ordering::SeqCst) as u64;
        if unfinished > 0 {
            state.unfinished_calls.store(unfinished, Ordering::Relaxed);
            state.failed_calls.fetch_add(unfinished, Ordering::Relaxed);
            state.record_status_error("UNFINISHED", unfinished);

            let drain_nanos = self.cfg.drain_timeout.as_nanos().min(u64::MAX as u128) as u64;
            let mut e2e = state.e2e_latencies.lock().unwrap();
            let mut serv = state.service_latencies.lock().unwrap();
            for _ in 0..unfinished {
                e2e.push(drain_nanos);
                serv.push(drain_nanos);
            }
        }

        LoadRecord {
            distribution: self.cfg.distribution,
            offered_calls: state.offered_calls.load(Ordering::Relaxed),
            dispatched_calls: state.dispatched_calls.load(Ordering::Relaxed),
            completed_calls: state.completed_calls.load(Ordering::Relaxed),
            successful_calls: state.successful_calls.load(Ordering::Relaxed),
            failed_calls: state.failed_calls.load(Ordering::Relaxed),
            timed_out_calls: state.timed_out_calls.load(Ordering::Relaxed),
            rejected_calls: state.rejected_calls.load(Ordering::Relaxed),
            unstarted_calls: state.unstarted_calls.load(Ordering::Relaxed),
            unfinished_calls: state.unfinished_calls.load(Ordering::Relaxed),
            scheduling_lags_nanos: state.scheduling_lags.lock().unwrap().clone(),
            service_latencies_nanos: state.service_latencies.lock().unwrap().clone(),
            e2e_latencies_nanos: state.e2e_latencies.lock().unwrap().clone(),
            duration: measurement_dur,
            status_errors: state.status_errors.lock().unwrap().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seeded_rng_determinism() {
        let mut rng1 = SeededRng::new(42);
        let mut rng2 = SeededRng::new(42);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
            assert_eq!(rng1.next_f64(), rng2.next_f64());
            assert_eq!(
                rng1.next_exponential_nanos(1000.0),
                rng2.next_exponential_nanos(1000.0)
            );
        }

        // Different seeds diverge
        let mut rng3 = SeededRng::new(99);
        assert_ne!(rng1.next_u64(), rng3.next_u64());
    }

    #[test]
    fn test_seeded_rng_mean_interval() {
        let mut rng = SeededRng::new(12345);
        let rate_qps = 1000.0;
        let samples = 20_000;
        let sum: u128 = (0..samples)
            .map(|_| rng.next_exponential_nanos(rate_qps) as u128)
            .sum();
        let mean = sum as f64 / samples as f64;
        let expected = 1_000_000_000.0 / rate_qps; // 1 ms = 1,000,000 ns
        let relative_error = (mean - expected).abs() / expected;

        // Mean should be within 3% of theoretical mean with 20,000 samples
        assert!(
            relative_error < 0.03,
            "mean {mean} differs from expected {expected} by {relative_error:.3}"
        );
    }

    #[test]
    fn test_dispatch_lag_is_measured_at_task_start() {
        let state = GeneratorState::new();
        let scheduled = Instant::now() - Duration::from_millis(10);
        let actual = state.record_scheduling_lag(scheduled);
        let samples = state.scheduling_lags.lock().unwrap().clone();
        assert_eq!(samples.len(), 1);
        assert!(samples[0] >= 10_000_000);
        assert!(actual >= scheduled + Duration::from_millis(10));
    }

    #[test]
    fn test_drain_waits_until_outcome_samples_are_committed() {
        let state = Arc::new(GeneratorState::new());
        state.active_in_flight.store(1, Ordering::SeqCst);
        let hold_samples = state.service_latencies.lock().unwrap();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            let scheduled = Instant::now();
            worker_state.record_call_outcome(
                scheduled,
                scheduled,
                Err(RpcCallError::Status("RESOURCE_EXHAUSTED".into())),
                false,
                Some(Duration::from_millis(50)),
            );
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.completed_calls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(state.completed_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.active_in_flight.load(Ordering::SeqCst),
            1,
            "drain must not return before latency and status samples are committed"
        );
        drop(hold_samples);
        worker.join().expect("call outcome worker");
        assert_eq!(state.active_in_flight.load(Ordering::SeqCst), 0);
        assert_eq!(state.e2e_latencies.lock().unwrap().len(), 1);
        assert_eq!(
            state
                .status_errors
                .lock()
                .unwrap()
                .get("RESOURCE_EXHAUSTED"),
            Some(&1)
        );
    }

    #[tokio::test]
    async fn test_closed_loop_concurrency() {
        let cfg = LoadConfig::closed(4, Duration::from_millis(100))
            .with_timeout(Duration::from_millis(50));
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Ok(())
            })
            .await;

        assert_eq!(record.distribution, LoadDistribution::Closed);
        assert!(record.offered_calls > 0);
        assert_eq!(record.offered_calls, record.dispatched_calls);
        assert_eq!(record.rejected_calls, 0);
        assert_eq!(record.unstarted_calls, 0);
        assert_eq!(record.successful_calls, record.completed_calls);
        assert_eq!(record.failed_calls, 0);

        // In closed-loop, scheduling lag is 0
        let lag_summary = record.scheduling_lag_summary().unwrap();
        assert_eq!(lag_summary.p50, 0);
        assert_eq!(lag_summary.max, 0);

        let metrics = record.to_rpc_metrics();
        assert_eq!(metrics.successful_rpcs, record.successful_calls);
        assert_eq!(metrics.offered_rpcs, Some(record.offered_calls));
        assert_eq!(metrics.queue_overflows, Some(0));
    }

    #[tokio::test]
    async fn test_open_loop_constant_arrival_schedule() {
        let cfg = LoadConfig::open_constant(200.0, Duration::from_millis(150))
            .with_timeout(Duration::from_millis(50))
            .with_max_calls(20);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(2)).await;
                Ok(())
            })
            .await;

        assert_eq!(record.distribution, LoadDistribution::Constant);
        assert!(record.offered_calls >= 15);
        assert_eq!(record.offered_calls, record.dispatched_calls);
        assert_eq!(record.rejected_calls, 0);
        assert_eq!(record.successful_calls, record.completed_calls);

        // Scheduling lag is recorded and non-negative
        let lag = record.scheduling_lag_summary().unwrap();
        assert!(lag.p50 <= lag.p99);
        assert!(lag.p99 <= lag.max);
    }

    #[tokio::test]
    async fn test_open_loop_poisson_arrival_schedule() {
        let seed = 0xbeef_cafe_1234;
        let cfg = LoadConfig::open_poisson(300.0, seed, Duration::from_millis(150))
            .with_timeout(Duration::from_millis(50))
            .with_max_calls(25);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok(())
            })
            .await;

        assert_eq!(record.distribution, LoadDistribution::Poisson);
        assert!(record.offered_calls >= 10);
        assert_eq!(record.offered_calls, record.dispatched_calls);
        assert_eq!(record.successful_calls, record.completed_calls);

        let lag = record.scheduling_lag_summary().unwrap();
        assert!(lag.p50 <= lag.p99);
        assert!(lag.p99 <= lag.max);

        // Verify repeatability with same seed
        let gen2 = LoadGenerator::new(
            LoadConfig::open_poisson(300.0, seed, Duration::from_millis(150))
                .with_timeout(Duration::from_millis(50))
                .with_max_calls(25),
        );
        let mut rng1 = SeededRng::new(seed);
        let mut rng2 = SeededRng::new(gen2.cfg.seed);
        for _ in 0..10 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[tokio::test]
    async fn test_bounded_in_flight_queue_overflow() {
        // High rate with small in-flight cap and long service time to force queue overflow
        let cap = 3;
        let cfg = LoadConfig::open_constant(1000.0, Duration::from_millis(50))
            .with_max_in_flight(cap)
            .with_timeout(Duration::from_millis(200))
            .with_max_calls(30);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                // Server stalls for 100 ms
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(())
            })
            .await;

        // Queue overflow must have been recorded, bounding memory growth
        assert!(
            record.rejected_calls > 0,
            "expected queue overflow rejections, got 0"
        );
        assert_eq!(record.rejected_calls, record.unstarted_calls);
        assert_eq!(
            record.offered_calls,
            record.dispatched_calls + record.unstarted_calls
        );
        assert!(record.dispatched_calls <= cap as u64 + 10);
        assert!(record.status_errors.contains_key("QUEUE_OVERFLOW"));

        let metrics = record.to_rpc_metrics();
        assert_eq!(metrics.queue_overflows, Some(record.rejected_calls));
    }

    #[tokio::test]
    async fn test_timeout_and_unstarted_calls_tracking() {
        // Service time is 60ms, but timeout is 10ms -> all calls must time out
        let cfg = LoadConfig::open_constant(100.0, Duration::from_millis(60))
            .with_timeout(Duration::from_millis(10))
            .with_max_calls(5);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(60)).await;
                Ok(())
            })
            .await;

        assert!(record.timed_out_calls > 0);
        assert_eq!(record.timed_out_calls, record.failed_calls);
        assert_eq!(record.successful_calls, 0);
        assert!(record.status_errors.contains_key("DEADLINE_EXCEEDED"));

        // Never drop timed-out calls from latency measurements:
        // Latency must be at least timeout (10 ms = 10,000,000 ns)
        assert_eq!(
            record.service_latencies_nanos.len(),
            record.completed_calls as usize
        );
        for &lat in &record.service_latencies_nanos {
            assert!(lat >= 10_000_000, "latency {lat} < timeout 10ms");
        }
    }

    #[tokio::test]
    async fn test_never_drop_failures() {
        // Calls fail with application error
        let cfg = LoadConfig::open_constant(200.0, Duration::from_millis(50))
            .with_timeout(Duration::from_millis(50))
            .with_max_calls(6);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                Err(RpcCallError::Status("UNAVAILABLE".to_string()))
            })
            .await;

        assert_eq!(record.failed_calls, record.completed_calls);
        assert_eq!(record.successful_calls, 0);
        assert_eq!(
            record
                .status_errors
                .get("UNAVAILABLE")
                .copied()
                .unwrap_or(0),
            record.failed_calls
        );

        // Latencies must still be recorded for failed calls
        assert_eq!(
            record.e2e_latencies_nanos.len(),
            record.completed_calls as usize
        );
    }

    #[tokio::test]
    async fn test_coordination_omission_measurement() {
        // In open loop, scheduled arrival times advance independently of delays.
        // We simulate a stall of 20ms during a 500 QPS test (arrival every 2ms).
        // Scheduling lag will reflect the stall, and end-to-end latency will
        // capture the queued delay.
        let cfg = LoadConfig::open_constant(200.0, Duration::from_millis(60))
            .with_timeout(Duration::from_millis(100))
            .with_max_calls(6);
        let gen = LoadGenerator::new(cfg);

        let call_idx = Arc::new(AtomicUsize::new(0));
        let record = gen
            .run(move || {
                let idx = call_idx.fetch_add(1, Ordering::SeqCst);
                async move {
                    if idx == 0 {
                        // First call stalls for 25ms
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    Ok(())
                }
            })
            .await;

        assert!(record.completed_calls >= 4);
        let e2e_dist = record.e2e_latency_distribution().unwrap();
        // The stalled call should have high e2e latency
        assert!(e2e_dist.max_nanos >= 20_000_000);
    }

    #[tokio::test]
    async fn test_to_rpc_metrics_validation() {
        let cfg = LoadConfig::open_constant(100.0, Duration::from_millis(50))
            .with_timeout(Duration::from_millis(50))
            .with_max_calls(4);
        let gen = LoadGenerator::new(cfg);

        let record = gen
            .run(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok(())
            })
            .await;

        let metrics = record.to_rpc_metrics();
        assert_eq!(metrics.attempted_rpcs, record.dispatched_calls);
        assert_eq!(metrics.successful_rpcs, record.successful_calls);
        assert_eq!(metrics.failed_rpcs, record.failed_calls);
        assert_eq!(metrics.offered_rpcs, Some(record.offered_calls));
        assert_eq!(metrics.dispatched_rpcs, Some(record.dispatched_calls));
        assert!(metrics.scheduling_lag_nanos.is_some());
    }
}
