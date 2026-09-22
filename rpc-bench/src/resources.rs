//! Process resource attribution and measurement for `rpc-bench`.
//!
//! Complies with `docs/benchmark-contract.md` Section 5:
//! - Measures per-process User and System CPU time (nanoseconds and seconds).
//! - Measures Current Resident Set Size (RSS) and Peak RSS (bytes and MiB).
//! - Measures active thread counts.
//! - Uses standard platform APIs (`libc::getrusage` / `/proc/self/stat` / `/proc/self/status`
//!   on Linux; `getrusage` / Mach `task_info` / `task_threads` on macOS) without pulling in
//!   heavy unreviewed external crates.
//! - Computes CPU cost per successful RPC: `endpoint_cpu_seconds / successful_rpcs`.
//! - Distinguishes client CPU, server CPU, and combined total, strictly preventing ambiguous
//!   conflation of separate process resource boundaries.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    missing_docs,
    reason = "benchmarking resource metrics"
)]

use serde::{Deserialize, Serialize};

/// Role of an endpoint process in a benchmark scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRole {
    /// Load generator client process.
    Client,
    /// Target server process.
    Server,
}

impl std::fmt::Display for EndpointRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client => write!(f, "client"),
            Self::Server => write!(f, "server"),
        }
    }
}

/// Quantitative process resource utilization metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessResources {
    /// User-mode CPU time in seconds.
    pub user_cpu_seconds: f64,
    /// System/kernel-mode CPU time in seconds.
    pub system_cpu_seconds: f64,
    /// Peak resident set size in bytes.
    pub peak_rss_bytes: u64,
    /// Peak resident set size in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_mib: Option<f64>,
    /// Current resident set size in bytes, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_rss_bytes: Option<u64>,
    /// Current resident set size in MiB, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_rss_mib: Option<f64>,
    /// User CPU time in nanoseconds, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_cpu_nanos: Option<u64>,
    /// System CPU time in nanoseconds, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_cpu_nanos: Option<u64>,
    /// Active thread count, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_count: Option<u32>,
    /// CPU seconds consumed per successful RPC (`total_cpu_seconds / successful_rpcs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_seconds_per_rpc: Option<f64>,
}

impl ProcessResources {
    /// Create new process resource metrics with primary fields.
    pub fn new(user_cpu_seconds: f64, system_cpu_seconds: f64, peak_rss_bytes: u64) -> Self {
        let peak_rss_mib = peak_rss_bytes as f64 / (1024.0 * 1024.0);
        let user_cpu_nanos = (user_cpu_seconds * 1_000_000_000.0).round() as u64;
        let system_cpu_nanos = (system_cpu_seconds * 1_000_000_000.0).round() as u64;

        Self {
            user_cpu_seconds,
            system_cpu_seconds,
            peak_rss_bytes,
            peak_rss_mib: Some(peak_rss_mib),
            current_rss_bytes: None,
            current_rss_mib: None,
            user_cpu_nanos: Some(user_cpu_nanos),
            system_cpu_nanos: Some(system_cpu_nanos),
            thread_count: None,
            cpu_seconds_per_rpc: None,
        }
    }

    /// Create process resources from high-resolution nanosecond counters and bytes.
    pub fn from_nanos(user_cpu_nanos: u64, system_cpu_nanos: u64, peak_rss_bytes: u64) -> Self {
        let user_cpu_seconds = user_cpu_nanos as f64 / 1_000_000_000.0;
        let system_cpu_seconds = system_cpu_nanos as f64 / 1_000_000_000.0;
        let peak_rss_mib = peak_rss_bytes as f64 / (1024.0 * 1024.0);

        Self {
            user_cpu_seconds,
            system_cpu_seconds,
            peak_rss_bytes,
            peak_rss_mib: Some(peak_rss_mib),
            current_rss_bytes: None,
            current_rss_mib: None,
            user_cpu_nanos: Some(user_cpu_nanos),
            system_cpu_nanos: Some(system_cpu_nanos),
            thread_count: None,
            cpu_seconds_per_rpc: None,
        }
    }

    /// Total CPU time (`user + system`) in seconds.
    pub fn total_cpu_seconds(&self) -> f64 {
        self.user_cpu_seconds + self.system_cpu_seconds
    }

    /// Total CPU time in nanoseconds, if both user and system nanoseconds are available.
    pub fn total_cpu_nanos(&self) -> Option<u64> {
        match (self.user_cpu_nanos, self.system_cpu_nanos) {
            (Some(u), Some(s)) => Some(u.saturating_add(s)),
            _ => None,
        }
    }

    /// Peak RSS in MiB (from field or computed).
    pub fn peak_rss_mib(&self) -> f64 {
        self.peak_rss_mib
            .unwrap_or_else(|| self.peak_rss_bytes as f64 / (1024.0 * 1024.0))
    }

    /// Current RSS in MiB, if available.
    pub fn current_rss_mib(&self) -> Option<f64> {
        self.current_rss_mib
            .or_else(|| self.current_rss_bytes.map(|b| b as f64 / (1024.0 * 1024.0)))
    }

    /// Compute and record CPU seconds per successful RPC.
    pub fn compute_cpu_per_successful_rpc(&mut self, successful_rpcs: u64) -> Option<f64> {
        let val = if successful_rpcs > 0 {
            Some(self.total_cpu_seconds() / successful_rpcs as f64)
        } else {
            None
        };
        self.cpu_seconds_per_rpc = val;
        val
    }

    /// Attach current RSS measurement.
    #[must_use]
    pub fn with_current_rss(mut self, current_rss_bytes: u64) -> Self {
        self.current_rss_bytes = Some(current_rss_bytes);
        self.current_rss_mib = Some(current_rss_bytes as f64 / (1024.0 * 1024.0));
        self
    }

    /// Attach thread count.
    #[must_use]
    pub fn with_thread_count(mut self, thread_count: u32) -> Self {
        self.thread_count = Some(thread_count);
        self
    }

    /// Attach nanosecond precision CPU measurements.
    #[must_use]
    pub fn with_nanos(mut self, user_cpu_nanos: u64, system_cpu_nanos: u64) -> Self {
        self.user_cpu_nanos = Some(user_cpu_nanos);
        self.system_cpu_nanos = Some(system_cpu_nanos);
        self
    }

    /// Attach CPU seconds per successful RPC.
    #[must_use]
    pub fn with_cpu_per_rpc(mut self, successful_rpcs: u64) -> Self {
        self.compute_cpu_per_successful_rpc(successful_rpcs);
        self
    }

    /// Validate internal semantic consistency.
    pub fn validate(&self) -> Result<(), ResourceAttributionError> {
        if self.user_cpu_seconds < 0.0 || self.user_cpu_seconds.is_nan() {
            return Err(ResourceAttributionError::NegativeCpuSeconds(format!(
                "invalid user_cpu_seconds: {}",
                self.user_cpu_seconds
            )));
        }
        if self.system_cpu_seconds < 0.0 || self.system_cpu_seconds.is_nan() {
            return Err(ResourceAttributionError::NegativeCpuSeconds(format!(
                "invalid system_cpu_seconds: {}",
                self.system_cpu_seconds
            )));
        }
        if let Some(per_rpc) = self.cpu_seconds_per_rpc {
            if per_rpc < 0.0 || per_rpc.is_nan() {
                return Err(ResourceAttributionError::NegativeCpuSeconds(format!(
                    "invalid cpu_seconds_per_rpc: {per_rpc}"
                )));
            }
        }
        Ok(())
    }
}

/// Effective CPU constraints observed for the current process.
///
/// Records the core budget a benchmark actually runs under: the process-visible
/// CPU count, the scheduler affinity set, and any cgroup CPU quota. Fields the
/// platform cannot provide are `None`, explicitly distinguishing "unknown /
/// unsupported" from a measured value. Quota is stored as integer millicpus so
/// records using this type keep exact equality semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuConstraints {
    /// Process-visible logical CPU count (`available_parallelism`), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_cpu_count: Option<usize>,
    /// CPU affinity set in kernel list form (e.g. `"0-3,8"`), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_cpus: Option<String>,
    /// Number of CPUs in the affinity set, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_count: Option<usize>,
    /// cgroup CPU quota in millicpus (`quota_us * 1000 / period_us`), if capped.
    /// `None` means uncapped or unknown, never a measured zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgroup_quota_millicpus: Option<u64>,
    /// How this record was obtained (e.g. `"linux-procfs"`,
    /// `"available-parallelism"`, or `"unsupported"`).
    pub source: String,
}

impl CpuConstraints {
    /// Detect the effective CPU constraints of the current process.
    pub fn detect() -> Self {
        let effective_cpu_count = std::thread::available_parallelism().map(|n| n.get()).ok();
        let (affinity_cpus, affinity_count, cgroup_quota_millicpus, source) =
            detect_platform_constraints();
        Self {
            effective_cpu_count,
            affinity_cpus,
            affinity_count,
            cgroup_quota_millicpus,
            source,
        }
    }

    /// cgroup CPU quota expressed in whole CPUs, if capped.
    pub fn cgroup_quota_cpus(&self) -> Option<f64> {
        self.cgroup_quota_millicpus.map(|m| m as f64 / 1000.0)
    }
}

/// Count the CPUs described by a kernel CPU list such as `"0-3,8"`.
/// Returns `None` when the list is empty or malformed.
pub fn parse_cpu_list_count(list: &str) -> Option<usize> {
    let list = list.trim();
    if list.is_empty() {
        return None;
    }
    let mut count = 0usize;
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let lo: usize = lo.trim().parse().ok()?;
            let hi: usize = hi.trim().parse().ok()?;
            if hi < lo {
                return None;
            }
            count = count.saturating_add(hi.saturating_sub(lo).saturating_add(1));
        } else {
            let _: usize = part.parse().ok()?;
            count = count.saturating_add(1);
        }
    }
    if count == 0 {
        None
    } else {
        Some(count)
    }
}

/// Parse cgroup v2 `cpu.max` contents (`"$MAX $PERIOD"` or `"max $PERIOD"`)
/// into millicpus. Returns `None` when uncapped (`max`) or malformed.
pub fn parse_cgroup_quota_v2_millicpus(contents: &str) -> Option<u64> {
    let mut parts = contents.split_whitespace();
    let max = parts.next()?;
    let period: u64 = parts.next()?.parse().ok()?;
    if max == "max" || period == 0 {
        return None;
    }
    let quota: u64 = max.parse().ok()?;
    Some(quota.saturating_mul(1000) / period)
}

/// Parse cgroup v1 `cpu.cfs_quota_us` / `cpu.cfs_period_us` contents into
/// millicpus. Returns `None` when uncapped (`-1`) or malformed.
pub fn parse_cgroup_quota_v1_millicpus(quota_us: &str, period_us: &str) -> Option<u64> {
    let quota: i64 = quota_us.trim().parse().ok()?;
    let period: u64 = period_us.trim().parse().ok()?;
    if quota < 0 || period == 0 {
        return None;
    }
    Some((quota as u64).saturating_mul(1000) / period)
}

#[cfg(target_os = "linux")]
fn detect_platform_constraints() -> (Option<String>, Option<usize>, Option<u64>, String) {
    let mut affinity_cpus = None;
    let mut affinity_count = None;
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Cpus_allowed_list:") {
                let list = rest.trim().to_string();
                affinity_count = parse_cpu_list_count(&list);
                affinity_cpus = Some(list);
                break;
            }
        }
    }
    (
        affinity_cpus,
        affinity_count,
        detect_cgroup_quota_millicpus(),
        "linux-procfs".to_string(),
    )
}

#[cfg(not(target_os = "linux"))]
fn detect_platform_constraints() -> (Option<String>, Option<usize>, Option<u64>, String) {
    (None, None, None, "available-parallelism".to_string())
}

#[cfg(target_os = "linux")]
fn detect_cgroup_quota_millicpus() -> Option<u64> {
    if let Ok(contents) = std::fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        // cgroup v2 honors an explicit "max" (uncapped) without consulting v1.
        if contents.split_whitespace().next() == Some("max") {
            return None;
        }
        if let Some(millicpus) = parse_cgroup_quota_v2_millicpus(&contents) {
            return Some(millicpus);
        }
    }
    let quota = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us").ok()?;
    let period = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us").ok()?;
    parse_cgroup_quota_v1_millicpus(&quota, &period)
}

/// Point-in-time snapshot of process resource consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSnapshot {
    /// Cumulative user-mode CPU time in nanoseconds.
    pub user_cpu_nanos: u64,
    /// Cumulative system-mode CPU time in nanoseconds.
    pub system_cpu_nanos: u64,
    /// Current resident set size in bytes.
    pub current_rss_bytes: u64,
    /// Peak resident set size observed up to this snapshot in bytes.
    pub peak_rss_bytes: u64,
    /// Active OS threads belonging to this process.
    pub thread_count: u32,
}

impl ResourceSnapshot {
    /// Whether per-process resource capture is supported on this platform.
    pub fn is_supported() -> bool {
        cfg!(any(target_os = "macos", target_os = "linux"))
    }

    /// Short name of the platform capture backend, or `"unsupported"`.
    pub fn platform_backend() -> &'static str {
        if cfg!(target_os = "macos") {
            "macos-mach"
        } else if cfg!(target_os = "linux") {
            "linux-procfs"
        } else {
            "unsupported"
        }
    }

    /// Capture an instantaneous snapshot of the current process resources.
    ///
    /// Returns an [`std::io::ErrorKind::Unsupported`] error on platforms without
    /// a capture backend instead of silent zero-fill, so unsupported counters
    /// are never mistaken for measured zeros.
    pub fn capture() -> std::io::Result<Self> {
        capture_platform()
    }

    /// User CPU time in seconds.
    pub fn user_cpu_seconds(&self) -> f64 {
        self.user_cpu_nanos as f64 / 1_000_000_000.0
    }

    /// System CPU time in seconds.
    pub fn system_cpu_seconds(&self) -> f64 {
        self.system_cpu_nanos as f64 / 1_000_000_000.0
    }

    /// Total CPU time (`user + system`) in nanoseconds.
    pub fn total_cpu_nanos(&self) -> u64 {
        self.user_cpu_nanos.saturating_add(self.system_cpu_nanos)
    }

    /// Total CPU time (`user + system`) in seconds.
    pub fn total_cpu_seconds(&self) -> f64 {
        self.total_cpu_nanos() as f64 / 1_000_000_000.0
    }

    /// Current resident set size in MiB.
    pub fn current_rss_mib(&self) -> f64 {
        self.current_rss_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Peak resident set size in MiB.
    pub fn peak_rss_mib(&self) -> f64 {
        self.peak_rss_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Calculate the resource delta between this earlier snapshot and a later snapshot.
    pub fn delta_to(&self, later: &ResourceSnapshot) -> ProcessResources {
        let user_cpu_nanos = later.user_cpu_nanos.saturating_sub(self.user_cpu_nanos);
        let system_cpu_nanos = later.system_cpu_nanos.saturating_sub(self.system_cpu_nanos);
        let user_cpu_seconds = user_cpu_nanos as f64 / 1_000_000_000.0;
        let system_cpu_seconds = system_cpu_nanos as f64 / 1_000_000_000.0;
        let peak_rss_bytes = later.peak_rss_bytes.max(self.peak_rss_bytes);

        ProcessResources {
            user_cpu_seconds,
            system_cpu_seconds,
            peak_rss_bytes,
            peak_rss_mib: Some(peak_rss_bytes as f64 / (1024.0 * 1024.0)),
            current_rss_bytes: Some(later.current_rss_bytes),
            current_rss_mib: Some(later.current_rss_bytes as f64 / (1024.0 * 1024.0)),
            user_cpu_nanos: Some(user_cpu_nanos),
            system_cpu_nanos: Some(system_cpu_nanos),
            thread_count: Some(later.thread_count),
            cpu_seconds_per_rpc: None,
        }
    }
}

/// Resource attribution for a single endpoint process (Client or Server).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointResources {
    /// Role of the endpoint process.
    pub role: EndpointRole,
    /// Detailed process resource consumption.
    pub resources: ProcessResources,
    /// Number of successful RPCs handled or issued by this endpoint.
    pub successful_rpcs: u64,
    /// CPU seconds consumed per successful RPC (`total_cpu_seconds / successful_rpcs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_seconds_per_rpc: Option<f64>,
}

pub type EndpointResourceAttribution = EndpointResources;

impl EndpointResources {
    /// Create new resource attribution for an endpoint role.
    pub fn new(role: EndpointRole, mut resources: ProcessResources, successful_rpcs: u64) -> Self {
        let cpu_seconds_per_rpc = resources.compute_cpu_per_successful_rpc(successful_rpcs);
        Self {
            role,
            resources,
            successful_rpcs,
            cpu_seconds_per_rpc,
        }
    }

    /// Convenience constructor for client endpoint.
    pub fn client(resources: ProcessResources, successful_rpcs: u64) -> Self {
        Self::new(EndpointRole::Client, resources, successful_rpcs)
    }

    /// Convenience constructor for server endpoint.
    pub fn server(resources: ProcessResources, successful_rpcs: u64) -> Self {
        Self::new(EndpointRole::Server, resources, successful_rpcs)
    }

    /// The endpoint role.
    pub fn role(&self) -> EndpointRole {
        self.role
    }

    /// User CPU time in seconds.
    pub fn user_cpu_seconds(&self) -> f64 {
        self.resources.user_cpu_seconds
    }

    /// System CPU time in seconds.
    pub fn system_cpu_seconds(&self) -> f64 {
        self.resources.system_cpu_seconds
    }

    /// Total CPU time (`user + system`) in seconds.
    pub fn total_cpu_seconds(&self) -> f64 {
        self.resources.total_cpu_seconds()
    }

    /// Peak RSS in bytes.
    pub fn peak_rss_bytes(&self) -> u64 {
        self.resources.peak_rss_bytes
    }

    /// Peak RSS in MiB.
    pub fn peak_rss_mib(&self) -> f64 {
        self.resources.peak_rss_mib()
    }

    /// Thread count, if recorded.
    pub fn thread_count(&self) -> Option<u32> {
        self.resources.thread_count
    }

    /// CPU seconds per successful RPC.
    pub fn cpu_seconds_per_rpc(&self) -> Option<f64> {
        self.cpu_seconds_per_rpc
    }

    /// Throughput efficiency: successful RPCs per CPU-second ($\eta = \frac{\text{RPCs}}{\text{CPU Sec}}$).
    pub fn throughput_per_cpu_second(&self) -> Option<f64> {
        let total_cpu = self.total_cpu_seconds();
        if total_cpu > 0.0 {
            Some(self.successful_rpcs as f64 / total_cpu)
        } else {
            None
        }
    }
}

/// Combined end-to-end resource metrics distinguishing client, server, and total CPU.
///
/// In compliance with `docs/benchmark-contract.md` Section 5, client CPU and server CPU
/// are strictly kept as distinct attribution vectors and never conflated into an ambiguous
/// single value without role identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedResourceMetrics {
    /// Isolated client load-generator process resources.
    pub client: EndpointResources,
    /// Isolated server target process resources.
    pub server: EndpointResources,
    /// Combined total user CPU seconds (`client_user + server_user`).
    pub combined_user_cpu_seconds: f64,
    /// Combined total system CPU seconds (`client_system + server_system`).
    pub combined_system_cpu_seconds: f64,
    /// Combined total CPU seconds (`client_total + server_total`).
    pub combined_total_cpu_seconds: f64,
    /// Combined end-to-end CPU cost per successful RPC:
    /// `(client_total_cpu + server_total_cpu) / successful_rpcs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combined_cpu_seconds_per_rpc: Option<f64>,
    /// Ratio of client CPU to combined total CPU (0.0 ..= 1.0).
    pub client_cpu_fraction: f64,
    /// Ratio of server CPU to combined total CPU (0.0 ..= 1.0).
    pub server_cpu_fraction: f64,
}

pub type CombinedResourceAttribution = CombinedResourceMetrics;

impl CombinedResourceMetrics {
    /// Construct combined resource metrics from strictly verified client and server attributions.
    pub fn new(
        client: EndpointResources,
        server: EndpointResources,
    ) -> Result<Self, ResourceAttributionError> {
        if client.role != EndpointRole::Client {
            return Err(ResourceAttributionError::MismatchedClientRole);
        }
        if server.role != EndpointRole::Server {
            return Err(ResourceAttributionError::MismatchedServerRole);
        }

        let combined_user_cpu_seconds = client.user_cpu_seconds() + server.user_cpu_seconds();
        let combined_system_cpu_seconds = client.system_cpu_seconds() + server.system_cpu_seconds();
        let combined_total_cpu_seconds = client.total_cpu_seconds() + server.total_cpu_seconds();

        let successful_rpcs = client.successful_rpcs.max(server.successful_rpcs);
        let combined_cpu_seconds_per_rpc = if successful_rpcs > 0 {
            Some(combined_total_cpu_seconds / successful_rpcs as f64)
        } else {
            None
        };

        let (client_cpu_fraction, server_cpu_fraction) = if combined_total_cpu_seconds > 0.0 {
            (
                client.total_cpu_seconds() / combined_total_cpu_seconds,
                server.total_cpu_seconds() / combined_total_cpu_seconds,
            )
        } else {
            (0.5, 0.5)
        };

        Ok(Self {
            client,
            server,
            combined_user_cpu_seconds,
            combined_system_cpu_seconds,
            combined_total_cpu_seconds,
            combined_cpu_seconds_per_rpc,
            client_cpu_fraction,
            server_cpu_fraction,
        })
    }

    /// Client total CPU time in seconds.
    pub fn client_cpu_seconds(&self) -> f64 {
        self.client.total_cpu_seconds()
    }

    /// Server total CPU time in seconds.
    pub fn server_cpu_seconds(&self) -> f64 {
        self.server.total_cpu_seconds()
    }

    /// Combined total CPU time in seconds.
    pub fn total_cpu_seconds(&self) -> f64 {
        self.combined_total_cpu_seconds
    }
}

/// Errors occurring during resource attribution and verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceAttributionError {
    /// Expected EndpointRole::Client but found another role.
    MismatchedClientRole,
    /// Expected EndpointRole::Server but found another role.
    MismatchedServerRole,
    /// Negative or NaN CPU seconds encountered.
    NegativeCpuSeconds(String),
    /// Platform process resource capture failed.
    CaptureFailed(String),
}

impl std::fmt::Display for ResourceAttributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MismatchedClientRole => {
                write!(f, "endpoint supplied as client does not have Client role")
            }
            Self::MismatchedServerRole => {
                write!(f, "endpoint supplied as server does not have Server role")
            }
            Self::NegativeCpuSeconds(msg) => write!(f, "negative or invalid CPU seconds: {msg}"),
            Self::CaptureFailed(msg) => write!(f, "process resource capture failed: {msg}"),
        }
    }
}

impl std::error::Error for ResourceAttributionError {}

// ---------------------------------------------------------------------------
// Platform-specific process resource collection implementations
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos_ffi {
    #[repr(C)]
    pub struct Timeval {
        pub tv_sec: isize,
        pub tv_usec: i32,
    }

    #[repr(C)]
    pub struct Rusage {
        pub ru_utime: Timeval,
        pub ru_stime: Timeval,
        pub ru_maxrss: isize,
        pub ru_ixrss: isize,
        pub ru_idrss: isize,
        pub ru_isrss: isize,
        pub ru_minflt: isize,
        pub ru_majflt: isize,
        pub ru_nswap: isize,
        pub ru_inblock: isize,
        pub ru_oublock: isize,
        pub ru_msgsnd: isize,
        pub ru_msgrcv: isize,
        pub ru_nsignals: isize,
        pub ru_nvcsw: isize,
        pub ru_nivcsw: isize,
    }

    #[repr(C)]
    pub struct MachTimeValue {
        pub seconds: i32,
        pub microseconds: i32,
    }

    #[repr(C)]
    pub struct MachTaskBasicInfo {
        pub virtual_size: u64,
        pub resident_size: u64,
        pub resident_size_max: u64,
        pub user_time: MachTimeValue,
        pub system_time: MachTimeValue,
        pub policy: i32,
        pub suspend_count: i32,
    }

    pub const RUSAGE_SELF: i32 = 0;
    pub const MACH_TASK_BASIC_INFO: i32 = 20;
    pub const KERN_SUCCESS: i32 = 0;

    extern "C" {
        pub fn getrusage(who: i32, usage: *mut Rusage) -> i32;
        pub fn mach_task_self() -> u32;
        pub fn task_info(
            target_task: u32,
            flavor: i32,
            task_info_out: *mut MachTaskBasicInfo,
            task_info_outCnt: *mut u32,
        ) -> i32;
        pub fn task_threads(
            target_task: u32,
            act_list: *mut *mut u32,
            act_listCnt: *mut u32,
        ) -> i32;
        pub fn mach_port_deallocate(target_task: u32, name: u32) -> i32;
        pub fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
    }
}

#[cfg(target_os = "macos")]
fn capture_platform() -> std::io::Result<ResourceSnapshot> {
    use macos_ffi::*;
    unsafe {
        let mut ru: Rusage = std::mem::zeroed();
        let ret = getrusage(RUSAGE_SELF, &mut ru);
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }

        let user_cpu_nanos = (ru.ru_utime.tv_sec.max(0) as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add((ru.ru_utime.tv_usec.max(0) as u64).saturating_mul(1_000));
        let system_cpu_nanos = (ru.ru_stime.tv_sec.max(0) as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add((ru.ru_stime.tv_usec.max(0) as u64).saturating_mul(1_000));
        // On macOS, ru_maxrss is in bytes
        let peak_rss_bytes = ru.ru_maxrss.max(0) as u64;

        let task = mach_task_self();
        let mut info: MachTaskBasicInfo = std::mem::zeroed();
        let mut count =
            (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;
        let kr = task_info(task, MACH_TASK_BASIC_INFO, &mut info, &mut count);
        let current_rss_bytes = if kr == KERN_SUCCESS {
            info.resident_size
        } else {
            peak_rss_bytes
        };

        let mut thread_list: *mut u32 = std::ptr::null_mut();
        let mut thread_count: u32 = 0;
        let kr_threads = task_threads(task, &mut thread_list, &mut thread_count);
        let final_thread_count = if kr_threads == KERN_SUCCESS {
            let cnt = thread_count;
            if !thread_list.is_null() {
                for i in 0..cnt {
                    let port = *thread_list.add(i as usize);
                    mach_port_deallocate(task, port);
                }
                vm_deallocate(
                    task,
                    thread_list as usize,
                    (cnt as usize) * std::mem::size_of::<u32>(),
                );
            }
            cnt
        } else {
            1
        };

        Ok(ResourceSnapshot {
            user_cpu_nanos,
            system_cpu_nanos,
            current_rss_bytes,
            peak_rss_bytes,
            thread_count: final_thread_count,
        })
    }
}

#[cfg(target_os = "linux")]
mod linux_ffi {
    #[repr(C)]
    pub struct Timeval {
        pub tv_sec: i64,
        pub tv_usec: i64,
    }

    #[repr(C)]
    pub struct Rusage {
        pub ru_utime: Timeval,
        pub ru_stime: Timeval,
        pub ru_maxrss: i64,
        pub ru_ixrss: i64,
        pub ru_idrss: i64,
        pub ru_isrss: i64,
        pub ru_minflt: i64,
        pub ru_majflt: i64,
        pub ru_nswap: i64,
        pub ru_inblock: i64,
        pub ru_oublock: i64,
        pub ru_msgsnd: i64,
        pub ru_msgrcv: i64,
        pub ru_nsignals: i64,
        pub ru_nvcsw: i64,
        pub ru_nivcsw: i64,
    }

    pub const RUSAGE_SELF: i32 = 0;

    extern "C" {
        pub fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
}

#[cfg(target_os = "linux")]
fn capture_platform() -> std::io::Result<ResourceSnapshot> {
    use linux_ffi::*;
    unsafe {
        let mut ru: Rusage = std::mem::zeroed();
        let ret = getrusage(RUSAGE_SELF, &mut ru);
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }

        let user_cpu_nanos = (ru.ru_utime.tv_sec.max(0) as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add((ru.ru_utime.tv_usec.max(0) as u64).saturating_mul(1_000));
        let system_cpu_nanos = (ru.ru_stime.tv_sec.max(0) as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add((ru.ru_stime.tv_usec.max(0) as u64).saturating_mul(1_000));
        // On Linux, ru_maxrss is in KiB
        let mut peak_rss_bytes = (ru.ru_maxrss.max(0) as u64).saturating_mul(1024);
        let mut current_rss_bytes = peak_rss_bytes;
        let mut thread_count = 1u32;

        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("VmRSS:") {
                    if let Some(val) = rest
                        .trim()
                        .strip_suffix("kB")
                        .and_then(|s| s.trim().parse::<u64>().ok())
                    {
                        current_rss_bytes = val.saturating_mul(1024);
                    }
                } else if let Some(rest) = line.strip_prefix("VmPeak:") {
                    if let Some(val) = rest
                        .trim()
                        .strip_suffix("kB")
                        .and_then(|s| s.trim().parse::<u64>().ok())
                    {
                        peak_rss_bytes = peak_rss_bytes.max(val.saturating_mul(1024));
                    }
                } else if let Some(rest) = line.strip_prefix("Threads:") {
                    if let Ok(val) = rest.trim().parse::<u32>() {
                        thread_count = val;
                    }
                }
            }
        }

        Ok(ResourceSnapshot {
            user_cpu_nanos,
            system_cpu_nanos,
            current_rss_bytes,
            peak_rss_bytes,
            thread_count,
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn capture_platform() -> std::io::Result<ResourceSnapshot> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process resource capture is unsupported on this platform",
    ))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_snapshot_capture_live() {
        let snap = ResourceSnapshot::capture().expect("live capture should succeed");
        assert!(snap.thread_count >= 1);
        assert!(snap.peak_rss_bytes > 0);
        assert!(snap.current_rss_bytes > 0);

        // Do some computation to advance CPU
        let mut val = 0u64;
        for i in 0..1_000_000 {
            val = val.wrapping_add(i);
        }
        std::hint::black_box(val);

        let snap2 = ResourceSnapshot::capture().expect("second live capture should succeed");
        let delta = snap.delta_to(&snap2);

        assert!(delta.user_cpu_seconds >= 0.0);
        assert!(delta.system_cpu_seconds >= 0.0);
        assert!(delta.peak_rss_bytes >= snap.peak_rss_bytes);
        assert!(delta.thread_count.unwrap_or(0) >= 1);
        assert!(delta.validate().is_ok());
    }

    #[test]
    fn test_process_resources_computations() {
        let mut res = ProcessResources::from_nanos(
            250_000_000,       // 0.25s
            50_000_000,        // 0.05s
            100 * 1024 * 1024, // 100 MiB
        );

        assert_eq!(res.user_cpu_seconds, 0.25);
        assert_eq!(res.system_cpu_seconds, 0.05);
        assert!((res.total_cpu_seconds() - 0.30).abs() < 1e-9);
        assert_eq!(res.total_cpu_nanos(), Some(300_000_000));
        assert!((res.peak_rss_mib() - 100.0).abs() < 1e-6);

        // Compute per-RPC CPU
        let per_rpc = res.compute_cpu_per_successful_rpc(10_000);
        assert!((per_rpc.unwrap() - (0.30 / 10_000.0)).abs() < 1e-9);
        assert!((res.cpu_seconds_per_rpc.unwrap() - 0.000_03).abs() < 1e-9);

        // Zero RPCs should return None
        let mut zero_rpc_res = res.clone();
        assert_eq!(zero_rpc_res.compute_cpu_per_successful_rpc(0), None);
    }

    #[test]
    fn test_endpoint_and_combined_attribution() {
        let client_res = ProcessResources::new(0.20, 0.05, 50 * 1024 * 1024);
        let server_res = ProcessResources::new(0.15, 0.10, 80 * 1024 * 1024);

        let client = EndpointResources::client(client_res, 5_000);
        let server = EndpointResources::server(server_res, 5_000);

        assert_eq!(client.role(), EndpointRole::Client);
        assert_eq!(server.role(), EndpointRole::Server);

        assert!((client.total_cpu_seconds() - 0.25).abs() < 1e-9);
        assert!((server.total_cpu_seconds() - 0.25).abs() < 1e-9);

        assert!((client.cpu_seconds_per_rpc().unwrap() - (0.25 / 5000.0)).abs() < 1e-9);
        assert!((server.cpu_seconds_per_rpc().unwrap() - (0.25 / 5000.0)).abs() < 1e-9);

        let combined = CombinedResourceMetrics::new(client.clone(), server.clone())
            .expect("valid endpoints should combine");

        // Client and server metrics must be distinct and non-conflated
        assert!((combined.client.total_cpu_seconds() - 0.25).abs() < 1e-9);
        assert!((combined.server.total_cpu_seconds() - 0.25).abs() < 1e-9);
        assert!((combined.combined_total_cpu_seconds - 0.50).abs() < 1e-9);
        assert!((combined.combined_user_cpu_seconds - 0.35).abs() < 1e-9);
        assert!((combined.combined_system_cpu_seconds - 0.15).abs() < 1e-9);
        assert!((combined.combined_cpu_seconds_per_rpc.unwrap() - (0.50 / 5000.0)).abs() < 1e-9);
        assert!((combined.client_cpu_fraction - 0.50).abs() < 1e-6);
        assert!((combined.server_cpu_fraction - 0.50).abs() < 1e-6);
    }

    #[test]
    fn test_combined_rejects_mismatched_roles() {
        let client_res = ProcessResources::new(0.1, 0.1, 1024);
        let server_res = ProcessResources::new(0.1, 0.1, 1024);

        let fake_client = EndpointResources::server(client_res.clone(), 100);
        let fake_server = EndpointResources::client(server_res.clone(), 100);

        assert_eq!(
            CombinedResourceMetrics::new(fake_client, fake_server),
            Err(ResourceAttributionError::MismatchedClientRole)
        );

        let real_client = EndpointResources::client(client_res, 100);
        let duplicate_client = EndpointResources::client(server_res, 100);

        assert_eq!(
            CombinedResourceMetrics::new(real_client, duplicate_client),
            Err(ResourceAttributionError::MismatchedServerRole)
        );
    }

    #[test]
    fn test_platform_backend_is_explicit() {
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(ResourceSnapshot::is_supported());
            assert_ne!(ResourceSnapshot::platform_backend(), "unsupported");
        } else {
            assert!(!ResourceSnapshot::is_supported());
            assert_eq!(ResourceSnapshot::platform_backend(), "unsupported");
            let err = ResourceSnapshot::capture()
                .expect_err("unsupported platforms must fail capture explicitly");
            assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        }
    }

    #[test]
    fn test_parse_cpu_list_count() {
        assert_eq!(parse_cpu_list_count("0-3"), Some(4));
        assert_eq!(parse_cpu_list_count("0-3,8"), Some(5));
        assert_eq!(parse_cpu_list_count("0,2,4"), Some(3));
        assert_eq!(parse_cpu_list_count("7"), Some(1));
        assert_eq!(parse_cpu_list_count(" 0-1 , 4 "), Some(3));
        assert_eq!(parse_cpu_list_count(""), None);
        assert_eq!(parse_cpu_list_count("3-1"), None);
        assert_eq!(parse_cpu_list_count("0,,2"), None);
        assert_eq!(parse_cpu_list_count("abc"), None);
    }

    #[test]
    fn test_parse_cgroup_quota_millicpus() {
        assert_eq!(
            parse_cgroup_quota_v2_millicpus("200000 100000\n"),
            Some(2000)
        );
        assert_eq!(parse_cgroup_quota_v2_millicpus("50000 100000"), Some(500));
        assert_eq!(parse_cgroup_quota_v2_millicpus("max 100000"), None);
        assert_eq!(parse_cgroup_quota_v2_millicpus("bogus"), None);
        assert_eq!(parse_cgroup_quota_v1_millicpus("-1", "100000"), None);
        assert_eq!(
            parse_cgroup_quota_v1_millicpus("250000", "100000"),
            Some(2500)
        );
        assert_eq!(parse_cgroup_quota_v1_millicpus("250000", "0"), None);
    }

    #[test]
    fn test_cpu_constraints_detect_and_serde() {
        let constraints = CpuConstraints::detect();
        assert!(!constraints.source.is_empty());
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let effective = constraints
                .effective_cpu_count
                .expect("supported platforms must report an effective CPU count");
            assert!(effective >= 1);
        }
        if let (Some(list), Some(count)) = (
            constraints.affinity_cpus.as_deref(),
            constraints.affinity_count,
        ) {
            assert_eq!(parse_cpu_list_count(list), Some(count));
        }

        let json = serde_json::to_string(&constraints).expect("serialization should succeed");
        assert!(json.contains("\"source\""));
        let deserialized: CpuConstraints =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(constraints, deserialized);
    }

    #[test]
    fn test_process_resources_serde() {
        let res = ProcessResources::new(1.25, 0.50, 16 * 1024 * 1024)
            .with_current_rss(12 * 1024 * 1024)
            .with_thread_count(4)
            .with_cpu_per_rpc(1000);

        let json = serde_json::to_string(&res).expect("serialization should succeed");
        let deserialized: ProcessResources =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(res, deserialized);
        assert!(deserialized.validate().is_ok());
    }
}
