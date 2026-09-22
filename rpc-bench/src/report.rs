//! Versioned benchmark result record format for `rpc-bench`.
//!
//! Captures comprehensive execution profiles in compliance with `docs/benchmark-contract.md`:
//! - Scenario definition and workload category
//! - Source, compiler, and peer implementation version pins
//! - Host hardware and operating system environment
//! - Connection and concurrency parameters
//! - Attempted, successful, failed, and timed-out RPC counts
//! - Fine-grained error breakdown by gRPC status code
//! - High-resolution latency percentiles and distribution histogram in nanoseconds
//! - Client and server CPU accounting (with unsupported metrics strictly distinguished from numeric zero)
//! - Start, end, and duration execution windows
#![allow(dead_code, reason = "public report API")]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

#[path = "resources.rs"]
pub mod resources;
pub use resources::{
    CombinedResourceMetrics, CpuConstraints, EndpointResourceAttribution, EndpointResources,
    EndpointRole, ProcessResources, ResourceAttributionError, ResourceSnapshot,
};

/// Current schema version for benchmark result records.
pub const REPORT_SCHEMA_VERSION: &str = "1.0.0";

/// Canonical latency unit across all benchmark reports.
pub const LATENCY_UNIT_NANOS: &str = "nanoseconds";

/// Transport implementation under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    /// Pure-Rust `pbrs-grpc` native kernel transport.
    Native,
    /// `tonic` 0.14+ reference transport.
    Tonic,
}

impl std::fmt::Display for TransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Tonic => write!(f, "tonic"),
        }
    }
}

/// Host hardware, OS, and runtime environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// Target operating system (e.g. "macos", "linux").
    pub os: String,
    /// CPU architecture (e.g. "aarch64", "x86_64").
    pub arch: String,
    /// Number of logical CPU cores available to the process.
    pub cpu_count: u32,
    /// Optional CPU model or processor string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_model: Option<String>,
    /// Optional hostname of the machine executing the benchmark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Optional total system memory in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_memory_bytes: Option<u64>,
    /// Effective CPU constraints (core quota/affinity) observed at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_constraints: Option<CpuConstraints>,
}

impl HostInfo {
    /// Detect environment information from the current system.
    pub fn detect() -> Self {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1);
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .ok();

        Self {
            os,
            arch,
            cpu_count,
            cpu_model: None,
            hostname,
            total_memory_bytes: None,
            cpu_constraints: Some(CpuConstraints::detect()),
        }
    }
}

/// Toolchain and dependency version pins for reproducibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPins {
    /// Git commit SHA of the repository under test.
    pub git_commit: String,
    /// Whether uncommitted changes were present during the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_dirty: Option<bool>,
    /// Rust compiler version (`rustc --version`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rustc_version: Option<String>,
    /// Peer implementation identifier (e.g. "pbrs-grpc", "tonic").
    pub peer_implementation: String,
    /// Peer implementation version string (e.g. "0.1.0-alpha.1", "0.14.6").
    pub peer_version: String,
    /// In-tree `pbrs` crate version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbrs_version: Option<String>,
    /// `tonic` dependency version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tonic_version: Option<String>,
    /// Build profile (e.g. "release", "debug").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

impl ToolPins {
    /// Detect available toolchain pins and peer versions.
    pub fn detect(transport: TransportMode, git_commit: impl Into<String>) -> Self {
        let (peer_implementation, peer_version) = match transport {
            TransportMode::Native => ("pbrs-grpc".to_string(), "0.1.0-alpha.1".to_string()),
            TransportMode::Tonic => ("tonic".to_string(), "0.14.6".to_string()),
        };

        Self {
            git_commit: git_commit.into(),
            git_dirty: detect_git_dirty(),
            rustc_version: detect_rustc_version(),
            peer_implementation,
            peer_version,
            pbrs_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            tonic_version: Some("0.14".to_string()),
            profile: Some(
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
                .to_string(),
            ),
        }
    }
}

/// Description of the scenario being evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioInfo {
    /// Machine-readable scenario identifier matching `leadership.json` (e.g. "unary_empty_plaintext").
    pub id: String,
    /// Human-readable scenario name.
    pub name: String,
    /// Workload category: "primary", "holdout", or "smoke".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// RPC communication pattern: "unary", "server_streaming", "client_streaming", or "bidi_streaming".
    pub rpc_type: String,
    /// Request payload size in bytes, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_size_bytes: Option<usize>,
    /// Response payload size in bytes, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_size_bytes: Option<usize>,
    /// Narrative description of the scenario.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Benchmark load and concurrency configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Target transport implementation.
    pub transport: TransportMode,
    /// Number of concurrent TCP connections.
    pub connections: u32,
    /// Number of concurrent worker tasks generating load.
    pub concurrency: u32,
    /// Warmup iterations or duration in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warmup_duration_nanos: Option<u64>,
    /// Target measurement window duration in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_duration_nanos: Option<u64>,
    /// Optional repetition index for multi-round benchmark executions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_index: Option<u32>,
}

/// Start and end execution timestamps and completion status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartEndConditions {
    /// UTC timestamp when the measurement phase began (RFC 3339).
    pub start_time_rfc3339: String,
    /// UTC timestamp when the measurement phase concluded (RFC 3339).
    pub end_time_rfc3339: String,
    /// Measured duration of the measurement phase in nanoseconds.
    pub duration_nanos: u64,
    /// Whether warmup phase completed prior to measurement.
    pub warmup_completed: bool,
    /// Whether the execution was terminated early (e.g. error threshold or cancellation).
    pub early_termination: bool,
    /// Reason for early termination, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<String>,
}

/// Quantitative RPC execution metrics and error accounting.
///
/// `client_cpu_seconds`, `server_cpu_seconds`, and `timeouts` are modeled as
/// `Option` types where `None` signifies that the metric is unsupported or uncollected
/// in the given environment (e.g. shared loopback process where per-role CPU accounting
/// cannot be isolated), strictly distinguishing unsupported counters from a numeric zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcMetrics {
    /// Total number of RPC requests initiated during the measurement window.
    pub attempted_rpcs: u64,
    /// Total number of RPC requests that completed successfully (status OK).
    pub successful_rpcs: u64,
    /// Total number of RPC requests that failed or terminated with non-OK status.
    pub failed_rpcs: u64,
    /// Error breakdown by gRPC status code (e.g. "UNAVAILABLE", "DEADLINE_EXCEEDED").
    #[serde(default)]
    pub status_errors: BTreeMap<String, u64>,
    /// Count of RPC calls that timed out before receiving a response.
    /// `None` indicates timeout tracking is unsupported by this runner; `Some(0)` indicates 0 timeouts occurred.
    #[serde(default)]
    pub timeouts: Option<u64>,
    /// Client CPU time in seconds (`user + system`).
    /// `None` indicates CPU instrumentation was unsupported/uncollected; `Some(0.0)` indicates measured zero.
    #[serde(default)]
    pub client_cpu_seconds: Option<f64>,
    /// Server CPU time in seconds (`user + system`).
    /// `None` indicates CPU instrumentation was unsupported/uncollected; `Some(0.0)` indicates measured zero.
    #[serde(default)]
    pub server_cpu_seconds: Option<f64>,
    /// Elapsed wall-clock time for the measurement phase in nanoseconds.
    pub duration_nanos: u64,
    /// Throughput in RPC requests or operations per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_qps: Option<f64>,
    /// Count of messages sent (e.g. for streaming workloads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_sent: Option<u64>,
    /// Count of messages received (e.g. for streaming workloads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_received: Option<u64>,
    /// Total payload bytes sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_sent: Option<u64>,
    /// Total payload bytes received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_received: Option<u64>,
    /// Statistical summary of scheduling lag in nanoseconds (p50, p99, max).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduling_lag_nanos: Option<SchedulingLagNanos>,
    /// Total number of RPC calls scheduled / offered by the load generator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offered_rpcs: Option<u64>,
    /// Total number of RPC calls actually dispatched into the transport pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_rpcs: Option<u64>,
    /// Count of offered calls dropped/rejected due to bounded in-flight queue overflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_overflows: Option<u64>,
    /// Detailed client process resource utilization (User/System CPU, Peak RSS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_resources: Option<ProcessResources>,
    /// Detailed server process resource utilization (User/System CPU, Peak RSS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_resources: Option<ProcessResources>,
}

impl RpcMetrics {
    /// Create new metrics with mandatory counts.
    pub fn new(
        attempted_rpcs: u64,
        successful_rpcs: u64,
        failed_rpcs: u64,
        duration_nanos: u64,
    ) -> Self {
        let throughput_qps = if duration_nanos > 0 {
            Some((successful_rpcs as f64) / (duration_nanos as f64 / 1_000_000_000.0))
        } else {
            None
        };

        Self {
            attempted_rpcs,
            successful_rpcs,
            failed_rpcs,
            status_errors: BTreeMap::new(),
            timeouts: None,
            client_cpu_seconds: None,
            server_cpu_seconds: None,
            duration_nanos,
            throughput_qps,
            messages_sent: None,
            messages_received: None,
            bytes_sent: None,
            bytes_received: None,
            scheduling_lag_nanos: None,
            offered_rpcs: None,
            dispatched_rpcs: None,
            queue_overflows: None,
            client_resources: None,
            server_resources: None,
        }
    }

    /// Record a failure with a specific gRPC status code name.
    pub fn record_status_error(&mut self, code_name: impl Into<String>, count: u64) {
        *self.status_errors.entry(code_name.into()).or_insert(0) += count;
    }

    /// Attach client process resources.
    #[must_use]
    pub fn with_client_resources(mut self, client: ProcessResources) -> Self {
        self.client_cpu_seconds = Some(client.total_cpu_seconds());
        self.client_resources = Some(client);
        self
    }

    /// Attach server process resources.
    #[must_use]
    pub fn with_server_resources(mut self, server: ProcessResources) -> Self {
        self.server_cpu_seconds = Some(server.total_cpu_seconds());
        self.server_resources = Some(server);
        self
    }

    /// Attach both client and server process resources.
    #[must_use]
    pub fn with_resources(
        mut self,
        client: Option<ProcessResources>,
        server: Option<ProcessResources>,
    ) -> Self {
        if let Some(ref c) = client {
            self.client_cpu_seconds = Some(c.total_cpu_seconds());
        }
        if let Some(ref s) = server {
            self.server_cpu_seconds = Some(s.total_cpu_seconds());
        }
        self.client_resources = client;
        self.server_resources = server;
        self
    }
}

/// Statistical summary of scheduling lag in nanoseconds (p50, p99, max).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingLagNanos {
    /// 50th percentile (median) scheduling lag in nanoseconds.
    pub p50: u64,
    /// 99th percentile scheduling lag in nanoseconds.
    pub p99: u64,
    /// Maximum observed scheduling lag in nanoseconds.
    pub max: u64,
}

pub type SchedulingLagSummary = SchedulingLagNanos;

impl SchedulingLagNanos {
    /// Create a new scheduling lag summary.
    pub fn new(p50: u64, p99: u64, max: u64) -> Self {
        Self { p50, p99, max }
    }

    /// Calculate p50, p99, max from nanosecond samples.
    pub fn from_samples(samples: &[u64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let pick = |q: f64| -> u64 {
            let i = ((n as f64 - 1.0) * q).round() as usize;
            sorted.get(i).copied().unwrap_or(0)
        };
        Some(Self {
            p50: pick(0.50),
            p99: pick(0.99),
            max: *sorted.last().unwrap_or(&0),
        })
    }
}

/// A single bucket in a latency histogram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyBucket {
    /// Lower bound in nanoseconds (inclusive).
    pub lower_bound_nanos: u64,
    /// Upper bound in nanoseconds (exclusive).
    pub upper_bound_nanos: u64,
    /// Number of observations falling within `[lower_bound_nanos, upper_bound_nanos)`.
    pub count: u64,
}

/// Latency histogram containing observation counts across buckets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyHistogram {
    /// Unit of time for all bucket bounds (always "nanoseconds").
    pub unit: String,
    /// Total number of observations represented in this histogram.
    pub total_count: u64,
    /// Ordered, non-overlapping latency buckets.
    pub buckets: Vec<LatencyBucket>,
}

impl LatencyHistogram {
    /// Build an exponential latency histogram covering nanosecond latencies from 0 to `u64::MAX`.
    pub fn build_exponential_buckets(sorted_samples: &[u64]) -> Self {
        // Standard exponential boundaries spanning sub-microsecond to multi-second delays
        const BOUNDARIES: &[u64] = &[
            0,
            500,           // 500 ns
            1_000,         // 1 µs
            2_000,         // 2 µs
            5_000,         // 5 µs
            10_000,        // 10 µs
            20_000,        // 20 µs
            50_000,        // 50 µs
            100_000,       // 100 µs
            200_000,       // 200 µs
            500_000,       // 500 µs
            1_000_000,     // 1 ms
            2_000_000,     // 2 ms
            5_000_000,     // 5 ms
            10_000_000,    // 10 ms
            20_000_000,    // 20 ms
            50_000_000,    // 50 ms
            100_000_000,   // 100 ms
            250_000_000,   // 250 ms
            500_000_000,   // 500 ms
            1_000_000_000, // 1 s
            2_000_000_000, // 2 s
            5_000_000_000, // 5 s
            u64::MAX,
        ];

        let mut buckets = Vec::new();
        let mut sample_idx = 0;
        let total_samples = sorted_samples.len();

        for window in BOUNDARIES.windows(2) {
            let lower = window[0];
            let upper = window[1];
            let start = sample_idx;
            while sample_idx < total_samples && sorted_samples[sample_idx] < upper {
                sample_idx += 1;
            }
            let count = (sample_idx - start) as u64;
            buckets.push(LatencyBucket {
                lower_bound_nanos: lower,
                upper_bound_nanos: upper,
                count,
            });
        }

        // Trim leading and trailing empty buckets to keep output compact,
        // while preserving all buckets in the active range.
        let first_non_empty = buckets.iter().position(|b| b.count > 0);
        let last_non_empty = buckets.iter().rposition(|b| b.count > 0);

        let filtered_buckets = match (first_non_empty, last_non_empty) {
            (Some(first), Some(last)) => buckets[first..=last].to_vec(),
            _ => Vec::new(),
        };

        Self {
            unit: LATENCY_UNIT_NANOS.to_string(),
            total_count: total_samples as u64,
            buckets: filtered_buckets,
        }
    }
}

/// Complete statistical summary and histogram distribution of latency measurements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatencyDistribution {
    /// Unit of time for all latency measurements (always "nanoseconds").
    pub unit: String,
    /// 50th percentile (median) latency in nanoseconds.
    pub p50_nanos: u64,
    /// 90th percentile latency in nanoseconds.
    pub p90_nanos: u64,
    /// 95th percentile latency in nanoseconds.
    pub p95_nanos: u64,
    /// 99th percentile latency in nanoseconds.
    pub p99_nanos: u64,
    /// 99.9th percentile latency in nanoseconds.
    /// `None` when sample count is insufficient to satisfy the contract's statistical threshold.
    #[serde(default)]
    pub p999_nanos: Option<u64>,
    /// Minimum observed latency in nanoseconds.
    pub min_nanos: u64,
    /// Maximum observed latency in nanoseconds.
    pub max_nanos: u64,
    /// Arithmetic mean latency in nanoseconds.
    pub mean_nanos: f64,
    /// Latency histogram containing bucket counts.
    pub histogram: LatencyHistogram,
    /// Optional raw sample vector in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_samples_nanos: Option<Vec<u64>>,
}

impl LatencyDistribution {
    /// Construct a latency distribution from raw nanosecond samples (as `u128`).
    pub fn from_nanos_samples(samples: &[u128]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let u64_samples: Vec<u64> = samples
            .iter()
            .map(|&s| s.min(u64::MAX as u128) as u64)
            .collect();
        Self::from_samples(&u64_samples)
    }

    /// Construct a latency distribution from raw nanosecond samples (as `u64`).
    pub fn from_samples(samples: &[u64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();

        let n = sorted.len();
        let min_nanos = *sorted.first()?;
        let max_nanos = *sorted.last()?;
        let sum: u128 = sorted.iter().map(|&x| x as u128).sum();
        let mean_nanos = sum as f64 / n as f64;

        let pick = |q: f64| -> u64 {
            let i = ((n as f64 - 1.0) * q).round() as usize;
            sorted.get(i).copied().unwrap_or(0)
        };

        let p50_nanos = pick(0.50);
        let p90_nanos = pick(0.90);
        let p95_nanos = pick(0.95);
        let p99_nanos = pick(0.99);

        // Per benchmark contract Section 6.4: p99.9 requires at least 1,000 observations
        // for meaningful reporting (and 1,000,000 for authoritative claims).
        let p999_nanos = if n >= 1000 { Some(pick(0.999)) } else { None };

        let histogram = LatencyHistogram::build_exponential_buckets(&sorted);

        Some(Self {
            unit: LATENCY_UNIT_NANOS.to_string(),
            p50_nanos,
            p90_nanos,
            p95_nanos,
            p99_nanos,
            p999_nanos,
            min_nanos,
            max_nanos,
            mean_nanos,
            histogram,
            raw_samples_nanos: None,
        })
    }

    /// Attach raw samples to the distribution.
    #[must_use]
    pub fn with_raw_samples(mut self, samples: Vec<u64>) -> Self {
        self.raw_samples_nanos = Some(samples);
        self
    }
}

/// Complete benchmark execution result record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkRun {
    /// Result record schema version (e.g. "1.0.0").
    pub schema_version: String,
    /// Unique identifier for this run.
    pub run_id: String,
    /// Timestamp when this record was generated (RFC 3339).
    pub timestamp: String,
    /// Git commit SHA of the codebase under test.
    pub git_commit: String,
    /// Host hardware, OS, and concurrency information.
    pub host_info: HostInfo,
    /// Toolchain and peer implementation versions.
    pub tool_pins: ToolPins,
    /// Scenario definition and parameters.
    pub scenario: ScenarioInfo,
    /// Transport mode (native vs tonic).
    pub transport: TransportMode,
    /// Number of TCP connections used.
    pub connections: u32,
    /// Concurrency level (number of concurrent request workers).
    pub concurrency: u32,
    /// Detailed benchmark configuration parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<BenchmarkConfig>,
    /// Measurement start, end, and duration conditions.
    pub start_end: StartEndConditions,
    /// Aggregated RPC counts and error breakdown.
    pub metrics: RpcMetrics,
    /// Latency percentiles and distribution histogram (if latency was measured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyDistribution>,
}

impl BenchmarkRun {
    /// Serialize this benchmark run to a compact JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize this benchmark run to a formatted, pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a benchmark run from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Save pretty-printed JSON record to a file on disk.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let json = self
            .to_json_pretty()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Validate internal semantic consistency of the benchmark run record.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version.trim().is_empty() {
            return Err(ValidationError::MissingField("schema_version"));
        }
        if self.run_id.trim().is_empty() {
            return Err(ValidationError::MissingField("run_id"));
        }
        if self.timestamp.trim().is_empty() {
            return Err(ValidationError::MissingField("timestamp"));
        }
        if self.git_commit.trim().is_empty() {
            return Err(ValidationError::MissingField("git_commit"));
        }
        if self.tool_pins.peer_implementation.trim().is_empty() {
            return Err(ValidationError::MissingField(
                "tool_pins.peer_implementation",
            ));
        }
        if self.tool_pins.peer_version.trim().is_empty() {
            return Err(ValidationError::MissingPeerVersion);
        }
        if self.scenario.id.trim().is_empty() {
            return Err(ValidationError::MissingField("scenario.id"));
        }
        if self.scenario.name.trim().is_empty() {
            return Err(ValidationError::MissingField("scenario.name"));
        }
        if self.connections == 0 || self.concurrency == 0 {
            return Err(ValidationError::ZeroConcurrency);
        }
        if self.start_end.duration_nanos == 0 || self.metrics.duration_nanos == 0 {
            return Err(ValidationError::ZeroDuration);
        }

        // Validate count consistency
        if self.metrics.attempted_rpcs < self.metrics.successful_rpcs + self.metrics.failed_rpcs {
            return Err(ValidationError::InconsistentCounts(format!(
                "attempted_rpcs ({}) cannot be less than successful_rpcs ({}) + failed_rpcs ({})",
                self.metrics.attempted_rpcs, self.metrics.successful_rpcs, self.metrics.failed_rpcs
            )));
        }

        let total_status_errors: u64 = self.metrics.status_errors.values().sum();
        if total_status_errors > self.metrics.failed_rpcs {
            return Err(ValidationError::InvalidStatusErrors(format!(
                "sum of status_errors ({total_status_errors}) exceeds failed_rpcs ({})",
                self.metrics.failed_rpcs
            )));
        }
        if self.metrics.failed_rpcs == 0 && !self.metrics.status_errors.is_empty() {
            return Err(ValidationError::InvalidStatusErrors(
                "status_errors present but failed_rpcs is 0".to_string(),
            ));
        }

        // Validate open-loop / scheduling lag metrics if present
        if let (Some(offered), Some(dispatched)) =
            (self.metrics.offered_rpcs, self.metrics.dispatched_rpcs)
        {
            if dispatched > offered {
                return Err(ValidationError::InconsistentCounts(format!(
                    "dispatched_rpcs ({dispatched}) exceeds offered_rpcs ({offered})"
                )));
            }
        }
        if let Some(ref lag) = self.metrics.scheduling_lag_nanos {
            if lag.p50 > lag.p99 || lag.p99 > lag.max {
                return Err(ValidationError::InvalidLatencyDistribution(format!(
                    "scheduling_lag_nanos percentiles not non-decreasing: p50={}, p99={}, max={}",
                    lag.p50, lag.p99, lag.max
                )));
            }
        }

        // Validate latency distribution consistency
        if let Some(ref lat) = self.latency {
            if lat.unit != LATENCY_UNIT_NANOS {
                return Err(ValidationError::InconsistentUnits(format!(
                    "latency unit must be '{}', got '{}'",
                    LATENCY_UNIT_NANOS, lat.unit
                )));
            }
            if lat.min_nanos > lat.max_nanos {
                return Err(ValidationError::InvalidLatencyDistribution(format!(
                    "min_nanos ({}) exceeds max_nanos ({})",
                    lat.min_nanos, lat.max_nanos
                )));
            }
            if lat.p50_nanos < lat.min_nanos || lat.p50_nanos > lat.max_nanos {
                return Err(ValidationError::InvalidLatencyDistribution(format!(
                    "p50_nanos ({}) outside [min, max] range [{}, {}]",
                    lat.p50_nanos, lat.min_nanos, lat.max_nanos
                )));
            }
            if !(lat.p50_nanos <= lat.p90_nanos
                && lat.p90_nanos <= lat.p95_nanos
                && lat.p95_nanos <= lat.p99_nanos
                && lat.p99_nanos <= lat.max_nanos)
            {
                return Err(ValidationError::InvalidLatencyDistribution(
                    "percentiles not monotonically non-decreasing".to_string(),
                ));
            }
            if let Some(p999) = lat.p999_nanos {
                if !(lat.p99_nanos <= p999 && p999 <= lat.max_nanos) {
                    return Err(ValidationError::InvalidLatencyDistribution(format!(
                        "p999_nanos ({p999}) must be >= p99 ({}) and <= max ({})",
                        lat.p99_nanos, lat.max_nanos
                    )));
                }
            }
            if lat.histogram.unit != LATENCY_UNIT_NANOS {
                return Err(ValidationError::InconsistentUnits(format!(
                    "histogram unit must be '{}', got '{}'",
                    LATENCY_UNIT_NANOS, lat.histogram.unit
                )));
            }
            let bucket_sum: u64 = lat.histogram.buckets.iter().map(|b| b.count).sum();
            if bucket_sum != lat.histogram.total_count {
                return Err(ValidationError::InconsistentCounts(format!(
                    "histogram bucket sum ({bucket_sum}) does not match total_count ({})",
                    lat.histogram.total_count
                )));
            }
        }

        // Validate client and server resource attribution if present
        if let Some(ref client_res) = self.metrics.client_resources {
            if let Err(e) = client_res.validate() {
                return Err(ValidationError::InvalidResourceMetrics(format!(
                    "invalid client_resources: {e}"
                )));
            }
        }
        if let Some(ref server_res) = self.metrics.server_resources {
            if let Err(e) = server_res.validate() {
                return Err(ValidationError::InvalidResourceMetrics(format!(
                    "invalid server_resources: {e}"
                )));
            }
        }

        Ok(())
    }
}

/// A collection of benchmark runs representing a complete suite execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Schema version for the suite report.
    pub schema_version: String,
    /// Unique identifier for this report.
    pub report_id: String,
    /// UTC timestamp when the report was generated.
    pub created_at: String,
    /// Git commit SHA under test.
    pub git_commit: String,
    /// Host execution environment.
    pub host_info: HostInfo,
    /// List of individual benchmark runs.
    pub runs: Vec<BenchmarkRun>,
    /// Detailed client process resource utilization (User/System CPU, Peak RSS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_resources: Option<ProcessResources>,
    /// Detailed server process resource utilization (User/System CPU, Peak RSS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_resources: Option<ProcessResources>,
}

impl BenchmarkReport {
    /// Create a report from a list of runs.
    pub fn new(runs: Vec<BenchmarkRun>) -> Self {
        let host_info = runs
            .first()
            .map(|r| r.host_info.clone())
            .unwrap_or_else(HostInfo::detect);
        let git_commit = runs
            .first()
            .map(|r| r.git_commit.clone())
            .unwrap_or_else(detect_git_commit);

        Self {
            schema_version: REPORT_SCHEMA_VERSION.to_string(),
            report_id: format!(
                "report-{}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
            created_at: format_rfc3339(SystemTime::now()),
            git_commit,
            host_info,
            runs,
            client_resources: None,
            server_resources: None,
        }
    }

    /// Attach overall client and server resource measurements to the report.
    #[must_use]
    pub fn with_resources(
        mut self,
        client: Option<ProcessResources>,
        server: Option<ProcessResources>,
    ) -> Self {
        self.client_resources = client;
        self.server_resources = server;
        self
    }

    /// Serialize report to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize report from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Validate all runs in the report.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.runs.is_empty() {
            return Err(ValidationError::MissingField("runs cannot be empty"));
        }
        for run in &self.runs {
            run.validate()?;
        }
        if let Some(ref client_res) = self.client_resources {
            if let Err(e) = client_res.validate() {
                return Err(ValidationError::InvalidResourceMetrics(format!(
                    "invalid report client_resources: {e}"
                )));
            }
        }
        if let Some(ref server_res) = self.server_resources {
            if let Err(e) = server_res.validate() {
                return Err(ValidationError::InvalidResourceMetrics(format!(
                    "invalid report server_resources: {e}"
                )));
            }
        }
        Ok(())
    }

    /// Save report to a file on disk.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let json = self
            .to_json_pretty()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

/// Validation failure indicating semantic or schema inconsistency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A required field is empty or missing.
    MissingField(&'static str),
    /// The peer implementation version is missing from tool pins.
    MissingPeerVersion,
    /// Counters or counts are logically inconsistent.
    InconsistentCounts(String),
    /// Measurement units are inconsistent with the contract specification.
    InconsistentUnits(String),
    /// Latency percentiles or histogram bounds violate distribution properties.
    InvalidLatencyDistribution(String),
    /// Benchmark duration is zero.
    ZeroDuration,
    /// Concurrency or connections is zero.
    ZeroConcurrency,
    /// Status code error counts exceed total failed RPCs.
    InvalidStatusErrors(String),
    /// Process resource metrics validation failure.
    InvalidResourceMetrics(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::MissingPeerVersion => write!(f, "missing peer version in tool pins"),
            Self::InconsistentCounts(msg) => write!(f, "inconsistent counts: {msg}"),
            Self::InconsistentUnits(msg) => write!(f, "inconsistent units: {msg}"),
            Self::InvalidLatencyDistribution(msg) => {
                write!(f, "invalid latency distribution: {msg}")
            }
            Self::ZeroDuration => write!(f, "benchmark run duration cannot be zero"),
            Self::ZeroConcurrency => write!(f, "concurrency and connections must be at least 1"),
            Self::InvalidStatusErrors(msg) => write!(f, "invalid status error counts: {msg}"),
            Self::InvalidResourceMetrics(msg) => write!(f, "invalid resource metrics: {msg}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Format a [`SystemTime`] as an RFC 3339 UTC string.
pub fn format_rfc3339(time: SystemTime) -> String {
    let dur = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "1970-01-01T00:00:00Z".to_string(),
    };
    let total_secs = dur.as_secs();
    let day_secs = total_secs % 86400;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;

    let mut days = total_secs / 86400;
    let mut year = 1970i64;
    loop {
        let leap = is_leap_year(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap_year(year);
    let days_in_months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1usize;
    for &dim in &days_in_months {
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }
    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Detect git commit SHA from environment or git command.
pub fn detect_git_commit() -> String {
    if let Ok(val) = std::env::var("BENCH_GIT_COMMIT") {
        if !val.trim().is_empty() {
            return val.trim().to_string();
        }
    }
    if let Ok(val) = std::env::var("GIT_COMMIT") {
        if !val.trim().is_empty() {
            return val.trim().to_string();
        }
    }
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
    {
        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sha.is_empty() {
                return sha;
            }
        }
    }
    "unknown".to_string()
}

fn detect_git_dirty() -> Option<bool> {
    std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
}

fn detect_rustc_version() -> Option<String> {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_run() -> BenchmarkRun {
        let samples: Vec<u64> = (10_000..=20_000).step_by(10).collect();
        let latency = LatencyDistribution::from_samples(&samples).unwrap();

        BenchmarkRun {
            schema_version: REPORT_SCHEMA_VERSION.to_string(),
            run_id: "test-run-1".to_string(),
            timestamp: "2026-09-18T14:00:00Z".to_string(),
            git_commit: "139af0c2559ebc36ecae86c647482a758dff4d64".to_string(),
            host_info: HostInfo {
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
                cpu_count: 8,
                cpu_model: Some("Apple M4 Pro".to_string()),
                hostname: None,
                total_memory_bytes: None,
                cpu_constraints: None,
            },
            tool_pins: ToolPins {
                git_commit: "139af0c2559ebc36ecae86c647482a758dff4d64".to_string(),
                git_dirty: Some(false),
                rustc_version: Some("rustc 1.88.0".to_string()),
                peer_implementation: "pbrs-grpc".to_string(),
                peer_version: "0.1.0-alpha.1".to_string(),
                pbrs_version: Some("0.1.0".to_string()),
                tonic_version: Some("0.14.6".to_string()),
                profile: Some("release".to_string()),
            },
            scenario: ScenarioInfo {
                id: "unary_empty_plaintext".to_string(),
                name: "Unary Empty Payload (Plaintext)".to_string(),
                category: Some("primary".to_string()),
                rpc_type: "unary".to_string(),
                request_size_bytes: Some(0),
                response_size_bytes: Some(0),
                description: Some("Baseline HTTP/2 unary test".to_string()),
            },
            transport: TransportMode::Native,
            connections: 1,
            concurrency: 1,
            config: Some(BenchmarkConfig {
                transport: TransportMode::Native,
                connections: 1,
                concurrency: 1,
                warmup_duration_nanos: Some(1_000_000_000),
                target_duration_nanos: Some(60_000_000_000),
                repetition_index: Some(0),
            }),
            start_end: StartEndConditions {
                start_time_rfc3339: "2026-09-18T14:00:00Z".to_string(),
                end_time_rfc3339: "2026-09-18T14:01:00Z".to_string(),
                duration_nanos: 60_000_000_000,
                warmup_completed: true,
                early_termination: false,
                termination_reason: None,
            },
            metrics: {
                let mut m = RpcMetrics::new(1000, 1000, 0, 60_000_000_000);
                m.timeouts = Some(0);
                m.client_cpu_seconds = None; // unsupported in loopback
                m.server_cpu_seconds = None; // unsupported in loopback
                m
            },
            latency: Some(latency),
        }
    }

    #[test]
    fn test_round_trip_json_serialization() {
        let run = sample_run();
        assert!(run.validate().is_ok());

        let json = run.to_json_pretty().expect("serialization should succeed");
        let deserialized = BenchmarkRun::from_json(&json).expect("deserialization should succeed");

        assert_eq!(run, deserialized);
        assert!(deserialized.validate().is_ok());
    }

    #[test]
    fn test_unsupported_counters_distinct_from_zero() {
        // Case 1: Unsupported counters are None -> serialized to null
        let mut run_unsupported = sample_run();
        run_unsupported.metrics.client_cpu_seconds = None;
        run_unsupported.metrics.server_cpu_seconds = None;
        run_unsupported.metrics.timeouts = None;

        let json_unsupported = run_unsupported.to_json().unwrap();
        assert!(json_unsupported.contains("\"client_cpu_seconds\":null"));
        assert!(json_unsupported.contains("\"server_cpu_seconds\":null"));
        assert!(json_unsupported.contains("\"timeouts\":null"));

        let des_unsupported = BenchmarkRun::from_json(&json_unsupported).unwrap();
        assert_eq!(des_unsupported.metrics.client_cpu_seconds, None);
        assert_eq!(des_unsupported.metrics.server_cpu_seconds, None);
        assert_eq!(des_unsupported.metrics.timeouts, None);

        // Case 2: Measured numeric zeroes are Some(0.0) / Some(0)
        let mut run_zero = sample_run();
        run_zero.metrics.client_cpu_seconds = Some(0.0);
        run_zero.metrics.server_cpu_seconds = Some(0.0);
        run_zero.metrics.timeouts = Some(0);

        let json_zero = run_zero.to_json().unwrap();
        assert!(json_zero.contains("\"client_cpu_seconds\":0.0"));
        assert!(json_zero.contains("\"server_cpu_seconds\":0.0"));
        assert!(json_zero.contains("\"timeouts\":0"));

        let des_zero = BenchmarkRun::from_json(&json_zero).unwrap();
        assert_eq!(des_zero.metrics.client_cpu_seconds, Some(0.0));
        assert_eq!(des_zero.metrics.server_cpu_seconds, Some(0.0));
        assert_eq!(des_zero.metrics.timeouts, Some(0));

        // Distinctness check: None must not equal Some(0)
        assert_ne!(
            des_unsupported.metrics.client_cpu_seconds,
            des_zero.metrics.client_cpu_seconds
        );
        assert_ne!(des_unsupported.metrics.timeouts, des_zero.metrics.timeouts);
    }

    #[test]
    fn test_validation_rejects_missing_peer_version() {
        let mut run = sample_run();
        run.tool_pins.peer_version = "".to_string();
        assert_eq!(run.validate(), Err(ValidationError::MissingPeerVersion));
    }

    #[test]
    fn test_validation_rejects_invalid_attempt_counts() {
        let mut run = sample_run();
        run.metrics.attempted_rpcs = 100;
        run.metrics.successful_rpcs = 105;
        run.metrics.failed_rpcs = 0;
        assert!(matches!(
            run.validate(),
            Err(ValidationError::InconsistentCounts(_))
        ));
    }

    #[test]
    fn test_validation_rejects_invalid_status_errors() {
        let mut run = sample_run();
        run.metrics.attempted_rpcs = 100;
        run.metrics.successful_rpcs = 98;
        run.metrics.failed_rpcs = 2;
        run.metrics.record_status_error("UNAVAILABLE", 5); // 5 > 2 failed_rpcs
        assert!(matches!(
            run.validate(),
            Err(ValidationError::InvalidStatusErrors(_))
        ));
    }

    #[test]
    fn test_validation_rejects_inconsistent_units() {
        let mut run = sample_run();
        if let Some(ref mut lat) = run.latency {
            lat.unit = "microseconds".to_string();
        }
        assert!(matches!(
            run.validate(),
            Err(ValidationError::InconsistentUnits(_))
        ));
    }

    #[test]
    fn test_validation_rejects_inverted_latencies() {
        let mut run = sample_run();
        if let Some(ref mut lat) = run.latency {
            lat.p50_nanos = 25_000;
            lat.p90_nanos = 15_000; // inverted
        }
        assert!(matches!(
            run.validate(),
            Err(ValidationError::InvalidLatencyDistribution(_))
        ));
    }

    #[test]
    fn test_validation_rejects_zero_concurrency_or_connections() {
        let mut run = sample_run();
        run.connections = 0;
        assert_eq!(run.validate(), Err(ValidationError::ZeroConcurrency));

        let mut run2 = sample_run();
        run2.concurrency = 0;
        assert_eq!(run2.validate(), Err(ValidationError::ZeroConcurrency));
    }

    #[test]
    fn test_deserialization_rejects_missing_required_fields() {
        let run = sample_run();
        let mut value = serde_json::to_value(&run).unwrap();

        // Removing a required field such as `git_commit`
        value.as_object_mut().unwrap().remove("git_commit");
        let result = serde_json::from_value::<BenchmarkRun>(value);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("missing field `git_commit`"));
    }

    #[test]
    fn test_report_round_trip() {
        let run = sample_run();
        let report = BenchmarkReport::new(vec![run.clone()]);
        assert!(report.validate().is_ok());

        let json = report.to_json_pretty().unwrap();
        let deserialized = BenchmarkReport::from_json(&json).unwrap();
        assert_eq!(report, deserialized);
        assert!(deserialized.validate().is_ok());
    }

    #[test]
    fn test_latency_distribution_from_samples() {
        let samples: Vec<u64> = vec![1000, 2000, 3000, 4000, 5000];
        let lat = LatencyDistribution::from_samples(&samples).unwrap();

        assert_eq!(lat.min_nanos, 1000);
        assert_eq!(lat.max_nanos, 5000);
        assert_eq!(lat.p50_nanos, 3000);
        assert_eq!(lat.mean_nanos, 3000.0);
        assert_eq!(lat.histogram.total_count, 5);

        // Sum of counts in buckets must equal total_count
        let sum: u64 = lat.histogram.buckets.iter().map(|b| b.count).sum();
        assert_eq!(sum, 5);
    }

    #[test]
    fn test_benchmark_run_with_raw_samples_and_save_to_file() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!(
            "test_benchmark_run_{}.json",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let mut run = sample_run();
        let samples = vec![1000, 2000, 3000];
        if let Some(lat) = run.latency.take() {
            run.latency = Some(lat.with_raw_samples(samples.clone()));
        }

        assert_eq!(
            run.latency.as_ref().unwrap().raw_samples_nanos,
            Some(samples)
        );

        run.save_to_file(&path).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded = BenchmarkRun::from_json(&content).unwrap();
        assert_eq!(run, loaded);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_host_info_detect_records_cpu_constraints() {
        let host = HostInfo::detect();
        let constraints = host
            .cpu_constraints
            .as_ref()
            .expect("HostInfo::detect must record effective CPU constraints");
        assert!(!constraints.source.is_empty());
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(constraints.effective_cpu_count.unwrap_or(0) >= 1);
        }

        let json = serde_json::to_string(&host).expect("serialization should succeed");
        assert!(json.contains("\"cpu_constraints\""));
        let deserialized: HostInfo =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(host, deserialized);

        // Records written before BM-06 (no cpu_constraints field) still parse.
        let legacy = serde_json::json!({
            "os": "linux",
            "arch": "x86_64",
            "cpu_count": 4,
        });
        let legacy_host: HostInfo =
            serde_json::from_value(legacy).expect("legacy HostInfo must still parse");
        assert_eq!(legacy_host.cpu_constraints, None);
    }

    #[test]
    fn test_format_rfc3339() {
        let epoch = SystemTime::UNIX_EPOCH;
        assert_eq!(format_rfc3339(epoch), "1970-01-01T00:00:00Z");

        // 2026-09-18T14:28:41Z
        // 56 years after 1970
        let now = SystemTime::now();
        let formatted = format_rfc3339(now);
        assert!(formatted.ends_with('Z'));
        assert_eq!(formatted.len(), 20);
    }

    #[test]
    fn test_scheduling_lag_and_offered_load_metrics() {
        let mut run = sample_run();
        run.metrics.offered_rpcs = Some(1050);
        run.metrics.dispatched_rpcs = Some(1000);
        run.metrics.queue_overflows = Some(50);
        run.metrics.scheduling_lag_nanos = Some(SchedulingLagNanos::new(12_000, 45_000, 89_000));

        assert!(run.validate().is_ok());

        let json = run.to_json_pretty().unwrap();
        assert!(json.contains("\"offered_rpcs\": 1050"));
        assert!(json.contains("\"dispatched_rpcs\": 1000"));
        assert!(json.contains("\"queue_overflows\": 50"));
        assert!(json.contains("\"scheduling_lag_nanos\""));

        let deserialized = BenchmarkRun::from_json(&json).unwrap();
        assert_eq!(run, deserialized);
        assert!(deserialized.validate().is_ok());

        // Validate rejection if dispatched > offered
        let mut invalid_run = run.clone();
        invalid_run.metrics.offered_rpcs = Some(900);
        invalid_run.metrics.dispatched_rpcs = Some(1000);
        assert!(matches!(
            invalid_run.validate(),
            Err(ValidationError::InconsistentCounts(_))
        ));

        // Validate rejection if scheduling lag percentiles inverted
        let mut invalid_lag = run.clone();
        invalid_lag.metrics.scheduling_lag_nanos =
            Some(SchedulingLagNanos::new(50_000, 20_000, 89_000));
        assert!(matches!(
            invalid_lag.validate(),
            Err(ValidationError::InvalidLatencyDistribution(_))
        ));
    }

    #[test]
    fn test_scheduling_lag_from_samples() {
        let samples = vec![1000, 2000, 3000, 4000, 5000];
        let lag = SchedulingLagNanos::from_samples(&samples).unwrap();
        assert_eq!(lag.p50, 3000);
        assert_eq!(lag.max, 5000);
        assert!(lag.p50 <= lag.p99);
        assert!(lag.p99 <= lag.max);

        assert!(SchedulingLagNanos::from_samples(&[]).is_none());
    }

    #[test]
    fn test_client_and_server_resources_in_metrics_and_report() {
        let client_res = ProcessResources::new(0.50, 0.25, 32 * 1024 * 1024)
            .with_current_rss(28 * 1024 * 1024)
            .with_thread_count(4)
            .with_cpu_per_rpc(10_000);

        let server_res = ProcessResources::new(0.25, 0.50, 64 * 1024 * 1024)
            .with_current_rss(55 * 1024 * 1024)
            .with_thread_count(8)
            .with_cpu_per_rpc(10_000);

        let mut run = sample_run();
        run.metrics = run
            .metrics
            .with_resources(Some(client_res.clone()), Some(server_res.clone()));

        assert_eq!(run.metrics.client_cpu_seconds, Some(0.75));
        assert_eq!(run.metrics.server_cpu_seconds, Some(0.75));
        assert_eq!(run.metrics.client_resources, Some(client_res.clone()));
        assert_eq!(run.metrics.server_resources, Some(server_res.clone()));
        assert!(run.validate().is_ok());

        // Report level resources
        let report = BenchmarkReport::new(vec![run.clone()])
            .with_resources(Some(client_res.clone()), Some(server_res.clone()));

        assert_eq!(report.client_resources, Some(client_res));
        assert_eq!(report.server_resources, Some(server_res));
        assert!(report.validate().is_ok());

        // Round-trip serialization
        let json = report
            .to_json_pretty()
            .expect("serialization should succeed");
        assert!(json.contains("\"client_resources\""));
        assert!(json.contains("\"server_resources\""));

        let deserialized =
            BenchmarkReport::from_json(&json).expect("deserialization should succeed");
        assert_eq!(report, deserialized);
        assert!(deserialized.validate().is_ok());

        // Negative CPU seconds in resources must be rejected by validation
        let mut invalid_run = run.clone();
        if let Some(ref mut c) = invalid_run.metrics.client_resources {
            c.user_cpu_seconds = -1.0;
        }
        assert!(matches!(
            invalid_run.validate(),
            Err(ValidationError::InvalidResourceMetrics(_))
        ));
    }
}
