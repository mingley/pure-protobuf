//! outlier_detection: heuristic ejection over a child policy (A50).
//!
//! The pool reports every completed call via
//! [`OutlierDetection::note_call_status`]; each sweep interval the
//! success-rate and failure-percentage detectors nominate outliers,
//! the ejection cap trims them, and survivors publish to the child
//! (ejected addresses simply disappear from its list). Ejection
//! duration is `base * total_ejections`, capped at
//! `maxEjectionTime`; the count accumulates per address across
//! ejections and clears only when the address leaves the list.
//! Unejection is lazy (evaluated on picks, observations, and
//! sweeps), so
//! fake-clock tests observe exact timelines with no background
//! tasks.
//!
//! The call path never blocks: reporting uses `try_lock` and drops
//! the sample under contention. Passive stats must never slow a
//! call, and one lost sample never changes a sweep outcome.
//!
//! Lock order is always outlier-state then child; children never
//! call back into the parent, so awaiting the child while holding
//! the state lock cannot deadlock.

use super::{
    HealthSignal, LbPolicy, LbPolicyFactory, LrTrack, Pick, Readiness, register_lb_policy_factory,
};
use crate::resolver::ResolvedAddress;
use crate::service_config::OutlierDetectionConfig;
use crate::status::{Code, Status};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::time::Instant;

/// A50 default ejection cap when `maxEjectionTime` is unset: 300s or
/// the base time, whichever is larger.
const DEFAULT_MAX_EJECTION: Duration = Duration::from_secs(300);

/// Per-address call outcomes in the current window.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    success: u64,
    failure: u64,
}

impl Counts {
    fn total(self) -> u64 {
        self.success.saturating_add(self.failure)
    }
}

/// outlier_detection policy state shared by a channel's picks.
#[derive(Debug)]
pub struct OutlierDetection {
    interval: Duration,
    base: Duration,
    cap: Duration,
    max_ejection_percent: u32,
    success_rate: Option<crate::service_config::SuccessRateEjectionConfig>,
    failure_percentage: Option<crate::service_config::FailurePercentageEjectionConfig>,
    child: LbPolicy,
    state: Mutex<State>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    /// Full resolver list, resolver order.
    addresses: Vec<ResolvedAddress>,
    /// Last list published to the child.
    published: Vec<ResolvedAddress>,
    /// Current-window call outcomes.
    counts: HashMap<ResolvedAddress, Counts>,
    /// Ejected addresses and their unejection deadlines.
    ejected: HashMap<ResolvedAddress, Instant>,
    /// Total ejections per address (backoff state). Monotonic while
    /// the address stays listed; ejection duration is `base * count`.
    ejections: HashMap<ResolvedAddress, u32>,
    /// A91 cumulative counters. Never pruned: ejection decisions
    /// stay reportable after their addresses leave.
    metrics: OutlierMetrics,
    /// Next sweep deadline.
    next_sweep: Instant,
}

/// A50 ejection snapshot for tests and observability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlierStats {
    /// Currently ejected addresses, in resolver-list order.
    /// Expirations evaluate before the snapshot is taken.
    pub ejected: Vec<ResolvedAddress>,
    /// Total ejection counts, in resolver-list order.
    pub ejection_counts: Vec<(ResolvedAddress, u32)>,
}

/// A91 counters for one detection method. Cumulative; export to OTel
/// instruments (names, units, target labels) lands with the OTel
/// metrics lane — these hooks already carry the per-method and
/// per-reason splits A91 requires.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutlierMethodMetrics {
    /// Enforced ejections (`ejections_enforced`).
    pub enforced: u64,
    /// Detected but skipped by the enforcement roll
    /// (`ejections_unenforced{reason=enforcement_percentage}`).
    pub unenforced_enforcement: u64,
    /// Detected but trimmed by the ejection cap
    /// (`ejections_unenforced{reason=max_ejection_overflow}`).
    pub unenforced_overflow: u64,
}

/// A91 outlier-detection metric counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutlierMetrics {
    /// Success-rate detector (`detection_method=success_rate`).
    pub success_rate: OutlierMethodMetrics,
    /// Failure-percentage detector
    /// (`detection_method=failure_percentage`).
    pub failure_percentage: OutlierMethodMetrics,
}

impl OutlierMetrics {
    fn enforced(&mut self, method: Detector) {
        match method {
            Detector::SuccessRate => {
                self.success_rate.enforced = self.success_rate.enforced.saturating_add(1)
            }
            Detector::FailurePercentage => {
                self.failure_percentage.enforced =
                    self.failure_percentage.enforced.saturating_add(1)
            }
        }
    }

    fn unenforced_enforcement(&mut self, method: Detector) {
        match method {
            Detector::SuccessRate => {
                self.success_rate.unenforced_enforcement =
                    self.success_rate.unenforced_enforcement.saturating_add(1)
            }
            Detector::FailurePercentage => {
                self.failure_percentage.unenforced_enforcement = self
                    .failure_percentage
                    .unenforced_enforcement
                    .saturating_add(1)
            }
        }
    }

    fn unenforced_overflow(&mut self, method: Detector) {
        match method {
            Detector::SuccessRate => {
                self.success_rate.unenforced_overflow =
                    self.success_rate.unenforced_overflow.saturating_add(1)
            }
            Detector::FailurePercentage => {
                self.failure_percentage.unenforced_overflow = self
                    .failure_percentage
                    .unenforced_overflow
                    .saturating_add(1)
            }
        }
    }
}

/// Which detector nominated a candidate (A91 `detection_method`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Detector {
    SuccessRate,
    FailurePercentage,
}

impl OutlierDetection {
    /// Build from a service-config document: the `outlier_detection`
    /// entry's config when selected. Fails when the child list
    /// selects no registered policy.
    pub fn from_config(
        config: Option<&crate::service_config::ServiceConfig>,
    ) -> Result<Arc<Self>, Status> {
        let entry = config.and_then(|doc| {
            doc.lb_policies().iter().find_map(|entry| match entry {
                crate::service_config::LbPolicyConfig::OutlierDetection(config) => {
                    Some(config.clone())
                }
                _ => None,
            })
        });
        let Some(entry) = entry else {
            return Err(Status::invalid_argument(
                "outlier_detection selected but not configured",
            ));
        };
        Self::with_config(&entry)
    }

    /// Build from an explicit config (tests and direct construction).
    /// Fails when the child list selects no registered policy.
    pub fn with_config(config: &OutlierDetectionConfig) -> Result<Arc<Self>, Status> {
        let child_entry = config
            .child_policy
            .iter()
            .find(|entry| super::is_policy_registered(entry.name()))
            .ok_or_else(|| {
                Status::invalid_argument("outlier_detection.childPolicy lists no registered policy")
            })?;
        let child = super::instantiate(child_entry)?;
        let max_ejection = config
            .max_ejection_time
            .unwrap_or_else(|| DEFAULT_MAX_EJECTION.max(config.base_ejection_time));
        let (bump, changed) = watch::channel(0);
        let next_sweep = Instant::now() + config.interval;
        Ok(Arc::new(Self {
            interval: config.interval,
            base: config.base_ejection_time,
            cap: max_ejection,
            max_ejection_percent: config.max_ejection_percent,
            success_rate: config.success_rate_ejection.clone(),
            failure_percentage: config.failure_percentage_ejection.clone(),
            child,
            state: Mutex::new(State {
                addresses: Vec::new(),
                published: Vec::new(),
                counts: HashMap::new(),
                ejected: HashMap::new(),
                ejections: HashMap::new(),
                metrics: OutlierMetrics::default(),
                next_sweep,
            }),
            changed,
            bump,
        }))
    }

    /// Record one completed call's outcome for the detectors. `Ok`
    /// counts as success; `Cancelled` is dropped (a client cancel
    /// says nothing about the backend); anything else counts as
    /// failure. Uses `try_lock`: a contended sample is dropped
    /// rather than slowing the call path.
    pub fn note_call_status(&self, addr: &ResolvedAddress, status: &Status) {
        if status.code() == Code::Cancelled {
            return;
        }
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        let counts = state.counts.entry(addr.clone()).or_default();
        if status.code() == Code::Ok {
            counts.success = counts.success.saturating_add(1);
        } else {
            counts.failure = counts.failure.saturating_add(1);
        }
    }

    /// Reconcile a new address list: prune counters, ejections, and
    /// backoff state for removed addresses, sweep when due, and
    /// republish the live set to the child.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses != state.addresses {
            state.counts.retain(|addr, _| addresses.contains(addr));
            state.ejected.retain(|addr, _| addresses.contains(addr));
            state.ejections.retain(|addr, _| addresses.contains(addr));
            state.addresses = addresses;
        }
        self.maybe_sweep(&mut state, Instant::now());
        self.republish(&mut state).await;
    }

    /// Pick through the child over the live (non-ejected) set,
    /// sweeping and unejecting first.
    pub async fn pick(&self) -> Pick {
        self.pick_hash(None).await
    }

    /// Hashed pick through the child over the live set.
    pub async fn pick_hash(&self, hash: Option<u64>) -> Pick {
        let mut state = self.state.lock().await;
        self.maybe_sweep(&mut state, Instant::now());
        self.republish(&mut state).await;
        Box::pin(self.child.pick_hash(hash)).await
    }

    /// Sweep when the interval has elapsed. Idempotent: sets the next
    /// deadline first, so a sweep runs at most once per interval no
    /// matter how many picks land inside it.
    fn maybe_sweep(&self, state: &mut State, now: Instant) {
        self.uneject_expired(state, now);
        if now < state.next_sweep {
            return;
        }
        state.next_sweep = now + self.interval;
        // Candidates in resolver order with their nominating
        // detector's enforcement percentage and method; an address
        // nominated by both keeps the success-rate one (first wins).
        let mut candidates: Vec<(ResolvedAddress, u32, Detector)> = Vec::new();
        if let Some(detector) = &self.success_rate {
            for addr in Self::success_rate_outliers(state, detector) {
                if !candidates.iter().any(|(known, _, _)| known == &addr) {
                    candidates.push((addr, detector.enforcement_percentage, Detector::SuccessRate));
                }
            }
        }
        if let Some(detector) = &self.failure_percentage {
            for addr in Self::failure_percentage_outliers(state, detector) {
                if !candidates.iter().any(|(known, _, _)| known == &addr) {
                    candidates.push((
                        addr,
                        detector.enforcement_percentage,
                        Detector::FailurePercentage,
                    ));
                }
            }
        }
        // A50 ejection cap: at most this many ejected overall, so a
        // widespread outage cannot eject the whole fleet. Resolver
        // order keeps trimming deterministic.
        let hosts = u32::try_from(state.addresses.len()).unwrap_or(u32::MAX);
        let cap = usize::try_from(
            (u128::from(self.max_ejection_percent) * u128::from(hosts) / 100)
                .min(usize::MAX as u128),
        )
        .unwrap_or(usize::MAX);
        let room = cap.saturating_sub(state.ejected.len());
        let mut enforced = 0usize;
        for (addr, enforcement, method) in &candidates {
            if enforced >= room {
                // A91: detected but trimmed by the cap.
                state.metrics.unenforced_overflow(*method);
                continue;
            }
            if state.ejected.contains_key(addr) {
                // Invariant: detectors pre-filter ejected addresses.
                continue;
            }
            if !roll_enforcement(*enforcement) {
                // A91: detected but skipped by the enforcement roll.
                state.metrics.unenforced_enforcement(*method);
                continue;
            }
            let count = state.ejections.get(addr).copied().unwrap_or(0);
            let count = count.saturating_add(1);
            state.ejections.insert(addr.clone(), count);
            let duration = self
                .base
                .checked_mul(count)
                .unwrap_or(Duration::MAX)
                .min(self.cap);
            state.ejected.insert(addr.clone(), now + duration);
            state.metrics.enforced(*method);
            enforced += 1;
        }
        state.counts.clear();
    }

    /// Success-rate detector (A50): among addresses with at least
    /// `request_volume` calls, eject those strictly below
    /// `mean - stdev * stdev_factor / 1000`. Skipped with fewer than
    /// `minimum_hosts` qualifying addresses. Resolver order.
    fn success_rate_outliers(
        state: &State,
        detector: &crate::service_config::SuccessRateEjectionConfig,
    ) -> Vec<ResolvedAddress> {
        let qualifying: Vec<(&ResolvedAddress, f64)> = state
            .addresses
            .iter()
            .filter(|addr| !state.ejected.contains_key(*addr))
            .filter_map(|addr| {
                let counts = state.counts.get(addr).copied().unwrap_or_default();
                if counts.total() < u64::from(detector.request_volume) {
                    return None;
                }
                #[allow(clippy::cast_precision_loss, reason = "rates need f64; u64 sums fit")]
                let rate = (counts.success as f64) / (counts.total() as f64);
                Some((addr, rate))
            })
            .collect();
        if qualifying.len() < detector.minimum_hosts as usize || qualifying.is_empty() {
            return Vec::new();
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "rates need f64; host counts fit"
        )]
        let hosts = qualifying.len() as f64;
        let mean = qualifying.iter().map(|(_, rate)| rate).sum::<f64>() / hosts;
        let variance = qualifying
            .iter()
            .map(|(_, rate)| (rate - mean) * (rate - mean))
            .sum::<f64>()
            / hosts;
        let threshold = mean - variance.sqrt() * f64::from(detector.stdev_factor) / 1000.0;
        qualifying
            .iter()
            .filter(|(_, rate)| *rate < threshold)
            .map(|(addr, _)| (*addr).clone())
            .collect()
    }

    /// Failure-percentage detector (A50): among addresses with at
    /// least `request_volume` calls, eject those whose failures are
    /// strictly more than `threshold` percent. Exact integer math.
    /// Skipped with fewer than `minimum_hosts` qualifying addresses.
    /// Resolver order.
    fn failure_percentage_outliers(
        state: &State,
        detector: &crate::service_config::FailurePercentageEjectionConfig,
    ) -> Vec<ResolvedAddress> {
        let qualifying: Vec<&ResolvedAddress> = state
            .addresses
            .iter()
            .filter(|addr| !state.ejected.contains_key(*addr))
            .filter(|addr| {
                state.counts.get(*addr).copied().unwrap_or_default().total()
                    >= u64::from(detector.request_volume)
            })
            .collect();
        if qualifying.len() < detector.minimum_hosts as usize {
            return Vec::new();
        }
        qualifying
            .into_iter()
            .filter(|addr| {
                let counts = state.counts.get(*addr).copied().unwrap_or_default();
                u128::from(counts.failure) * 100
                    > u128::from(detector.threshold) * u128::from(counts.total())
            })
            .cloned()
            .collect()
    }

    /// Drop expired ejections. Runs on every observation (picks,
    /// sweeps, stats, readiness) so timers need no background task.
    fn uneject_expired(&self, state: &mut State, now: Instant) {
        state.ejected.retain(|_, until| *until > now);
    }

    /// Publish the live (non-ejected) set to the child, waking
    /// waiters on change.
    async fn republish(&self, state: &mut State) {
        let live: Vec<ResolvedAddress> = state
            .addresses
            .iter()
            .filter(|addr| !state.ejected.contains_key(*addr))
            .cloned()
            .collect();
        if live != state.published {
            Box::pin(self.child.update(live.clone())).await;
            state.published = live;
            self.bump
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }

    /// Record a successful dial through the child.
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_success(addr)).await;
    }

    /// Record a failed dial through the child. Dial failures are not
    /// call outcomes, so they feed only the child's backoff, never
    /// the detectors.
    pub async fn note_failure(&self, addr: &ResolvedAddress, status: Status) {
        Box::pin(self.child.note_failure(addr, status)).await;
    }

    /// Health Watch started: forward to the child.
    pub async fn note_health_pending(&self, addr: &ResolvedAddress) -> bool {
        Box::pin(self.child.note_health_pending(addr)).await
    }

    /// Health Watch ended: forward to the child.
    pub async fn note_health_gone(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_health_gone(addr)).await;
    }

    /// Health report: forward to the child.
    pub async fn note_health(&self, addr: &ResolvedAddress, signal: HealthSignal) {
        Box::pin(self.child.note_health(addr, signal)).await;
    }

    /// Latest health signal through the child.
    pub async fn health_of(&self, addr: &ResolvedAddress) -> Option<HealthSignal> {
        Box::pin(self.child.health_of(addr)).await
    }

    /// ORCA report through the child.
    pub async fn note_orca_report(
        &self,
        addr: &ResolvedAddress,
        report: &crate::orca::OrcaLoadReport,
        oob: bool,
    ) {
        Box::pin(self.child.note_orca_report(addr, report, oob)).await;
    }

    /// Movement notifications: ejections, unejections, and updates.
    /// The child bumps its own watchers too; waiters hold both.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.clone()
    }

    /// OOB interval through the child.
    pub async fn wants_oob(&self) -> Option<Duration> {
        // Boxed: the enum dispatches back here (E0733).
        Box::pin(self.child.wants_oob()).await
    }

    /// Claim the OOB pump slot through the child.
    pub async fn note_oob_started(&self, addr: &ResolvedAddress) -> bool {
        Box::pin(self.child.note_oob_started(addr)).await
    }

    /// Release the OOB pump slot through the child.
    pub async fn note_oob_gone(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_oob_gone(addr)).await;
    }

    /// Request hash through the child, if it hashes.
    pub fn request_hash(&self, md: &crate::Metadata) -> Option<u64> {
        self.child.request_hash(md)
    }

    /// Least-request tracking through the child.
    pub async fn track_start(&self, addr: &ResolvedAddress) -> LrTrack {
        Box::pin(self.child.track_start(addr)).await
    }

    /// Child readiness (for nesting). Expirations evaluate first so
    /// a parent never observes a stale ejection.
    pub(crate) async fn readiness(&self) -> Readiness {
        let mut state = self.state.lock().await;
        self.uneject_expired(&mut state, Instant::now());
        self.republish(&mut state).await;
        Box::pin(self.child.readiness()).await
    }

    /// A50 ejection snapshot, expirations evaluated first.
    pub async fn outlier_stats(&self) -> OutlierStats {
        let mut state = self.state.lock().await;
        self.uneject_expired(&mut state, Instant::now());
        self.republish(&mut state).await;
        let ejected: Vec<ResolvedAddress> = state
            .addresses
            .iter()
            .filter(|addr| state.ejected.contains_key(*addr))
            .cloned()
            .collect();
        let ejection_counts: Vec<(ResolvedAddress, u32)> = state
            .addresses
            .iter()
            .filter_map(|addr| {
                state
                    .ejections
                    .get(addr)
                    .map(|count| (addr.clone(), *count))
            })
            .collect();
        OutlierStats {
            ejected,
            ejection_counts,
        }
    }

    /// A91 cumulative counters (hooks for the OTel metrics lane).
    pub async fn outlier_metrics(&self) -> OutlierMetrics {
        self.state.lock().await.metrics.clone()
    }
}

/// Enforcement roll: `percent` chance in 100. Deterministic at the
/// 0/100 extremes tests pin.
fn roll_enforcement(percent: u32) -> bool {
    if percent >= 100 {
        return true;
    }
    if percent == 0 {
        return false;
    }
    super::SplitMix64::seed().next() % 100 < u64::from(percent)
}

/// Factory for the `outlier_detection` policy name.
#[derive(Debug, Default)]
pub struct OutlierDetectionFactory;

impl LbPolicyFactory for OutlierDetectionFactory {
    fn name(&self) -> &str {
        "outlier_detection"
    }
}

/// Register the `outlier_detection` policy name once.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(OutlierDetectionFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::OutlierDetection;
    use crate::resolver::ResolvedAddress;
    use crate::service_config::ServiceConfig;
    use crate::status::Status;
    use std::time::Duration;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    fn register_children() {
        super::super::ensure_round_robin_registered();
    }

    /// Failure-percentage detector: 10s interval, 30s base, 50% cap,
    /// volume 5, 2 hosts minimum, full enforcement.
    fn failure_percentage() -> ServiceConfig {
        ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "interval": "10s",
                "baseEjectionTime": "30s",
                "maxEjectionPercent": 50,
                "failurePercentageEjection": {
                    "threshold": 50,
                    "enforcementPercentage": 100,
                    "minimumHosts": 2,
                    "requestVolume": 5
                },
                "childPolicy": [{"round_robin": {}}]}}]}"#,
        )
        .expect("parses")
    }

    fn report(policy: &OutlierDetection, addr: &ResolvedAddress, ok: u32, fail: u32) {
        for _ in 0..ok {
            policy.note_call_status(addr, &Status::ok());
        }
        let failure = Status::internal("backend error");
        for _ in 0..fail {
            policy.note_call_status(addr, &failure);
        }
    }

    async fn assert_never_serves(policy: &OutlierDetection, banned: &ResolvedAddress) {
        // Round_robin rotates over the live set; a full rotation that
        // never touches the ejected address proves exclusion.
        for _ in 0..4 {
            let pick = policy.pick().await;
            assert!(
                !matches!(pick, super::Pick::Use(ref addr) if addr == banned),
                "ejected address must not serve, got {pick:?}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn failure_percentage_ejects_and_unejects() {
        register_children();
        let policy = OutlierDetection::from_config(Some(&failure_percentage())).expect("builds");
        policy.update(vec![tcp(1), tcp(2), tcp(3), tcp(4)]).await;
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 5, 0);
        report(&policy, &tcp(3), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        let stats = policy.outlier_stats().await;
        assert_eq!(stats.ejected, vec![tcp(1)]);
        assert_eq!(stats.ejection_counts, vec![(tcp(1), 1)]);
        assert_never_serves(&policy, &tcp(1)).await;
        // Past the 30s base ejection the address serves again.
        tokio::time::advance(Duration::from_secs(31)).await;
        let _ = policy.pick().await;
        assert!(policy.outlier_stats().await.ejected.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn ejection_backoff_grows_with_total_ejections() {
        register_children();
        let policy = OutlierDetection::from_config(Some(&failure_percentage())).expect("builds");
        policy.update(vec![tcp(1), tcp(2), tcp(3), tcp(4)]).await;
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        assert_eq!(policy.outlier_stats().await.ejected, vec![tcp(1)]);
        // Uneject, fail again, sweep again: duration is now 2x base.
        tokio::time::advance(Duration::from_secs(31)).await;
        let _ = policy.pick().await;
        assert!(policy.outlier_stats().await.ejected.is_empty());
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        let stats = policy.outlier_stats().await;
        assert_eq!(stats.ejected, vec![tcp(1)]);
        assert_eq!(stats.ejection_counts, vec![(tcp(1), 2)]);
        tokio::time::advance(Duration::from_secs(31)).await;
        let _ = policy.pick().await;
        assert_eq!(
            policy.outlier_stats().await.ejected,
            vec![tcp(1)],
            "second ejection lasts 60s"
        );
        tokio::time::advance(Duration::from_secs(30)).await;
        let _ = policy.pick().await;
        assert!(policy.outlier_stats().await.ejected.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn cap_trims_candidates_in_resolver_order() {
        register_children();
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "interval": "10s",
                "baseEjectionTime": "30s",
                "maxEjectionPercent": 25,
                "failurePercentageEjection": {
                    "threshold": 50,
                    "enforcementPercentage": 100,
                    "minimumHosts": 2,
                    "requestVolume": 5
                },
                "childPolicy": [{"round_robin": {}}]}}]}"#,
        )
        .expect("parses");
        let policy = OutlierDetection::from_config(Some(&doc)).expect("builds");
        policy.update(vec![tcp(1), tcp(2), tcp(3), tcp(4)]).await;
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 0, 5);
        report(&policy, &tcp(3), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        // 25% of 4 rounds down to 1: only the first candidate ejects.
        assert_eq!(policy.outlier_stats().await.ejected, vec![tcp(1)]);
        let metrics = policy.outlier_metrics().await;
        assert_eq!(metrics.failure_percentage.enforced, 1);
        assert_eq!(metrics.failure_percentage.unenforced_overflow, 1);
        assert_eq!(
            metrics,
            super::OutlierMetrics {
                success_rate: super::OutlierMethodMetrics::default(),
                failure_percentage: super::OutlierMethodMetrics {
                    enforced: 1,
                    unenforced_enforcement: 0,
                    unenforced_overflow: 1,
                },
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn success_rate_detector_ejects_low_outlier() {
        register_children();
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "interval": "10s",
                "baseEjectionTime": "30s",
                "maxEjectionPercent": 50,
                "successRateEjection": {
                    "stdevFactor": 1900,
                    "enforcementPercentage": 100,
                    "minimumHosts": 5,
                    "requestVolume": 10
                },
                "childPolicy": [{"round_robin": {}}]}}]}"#,
        )
        .expect("parses");
        let policy = OutlierDetection::from_config(Some(&doc)).expect("builds");
        let addrs = vec![tcp(1), tcp(2), tcp(3), tcp(4), tcp(5)];
        policy.update(addrs).await;
        // Rates 0.2, 1, 1, 1, 1: mean 0.84, stdev 0.32, threshold
        // 0.84 - 0.32*1.9 = 0.232. Only tcp(1) falls below.
        report(&policy, &tcp(1), 2, 8);
        for n in 2..=5u8 {
            report(&policy, &tcp(n), 10, 0);
        }
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        assert_eq!(policy.outlier_stats().await.ejected, vec![tcp(1)]);
        let metrics = policy.outlier_metrics().await;
        assert_eq!(metrics.success_rate.enforced, 1);
        assert_eq!(
            metrics.failure_percentage,
            super::OutlierMethodMetrics::default()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn enforcement_zero_disables_ejection() {
        register_children();
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "interval": "10s",
                "baseEjectionTime": "30s",
                "maxEjectionPercent": 50,
                "failurePercentageEjection": {
                    "threshold": 50,
                    "enforcementPercentage": 0,
                    "minimumHosts": 2,
                    "requestVolume": 5
                },
                "childPolicy": [{"round_robin": {}}]}}]}"#,
        )
        .expect("parses");
        let policy = OutlierDetection::from_config(Some(&doc)).expect("builds");
        policy.update(vec![tcp(1), tcp(2)]).await;
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        assert!(policy.outlier_stats().await.ejected.is_empty());
        let metrics = policy.outlier_metrics().await;
        assert_eq!(metrics.failure_percentage.unenforced_enforcement, 1);
        assert_eq!(metrics.failure_percentage.enforced, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_outcomes_do_not_count() {
        register_children();
        let policy = OutlierDetection::from_config(Some(&failure_percentage())).expect("builds");
        policy.update(vec![tcp(1), tcp(2)]).await;
        for _ in 0..5 {
            policy.note_call_status(&tcp(1), &Status::cancelled());
        }
        report(&policy, &tcp(2), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        // tcp(1) never reaches the request volume: no quorum, no ejection.
        assert!(policy.outlier_stats().await.ejected.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn minimum_hosts_gates_detectors() {
        register_children();
        let policy = OutlierDetection::from_config(Some(&failure_percentage())).expect("builds");
        policy.update(vec![tcp(1), tcp(2)]).await;
        // Only one address reaches volume; minimumHosts is 2.
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 4, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        assert!(policy.outlier_stats().await.ejected.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn removed_addresses_prune_state() {
        register_children();
        let policy = OutlierDetection::from_config(Some(&failure_percentage())).expect("builds");
        policy.update(vec![tcp(1), tcp(2), tcp(3), tcp(4)]).await;
        report(&policy, &tcp(1), 0, 5);
        report(&policy, &tcp(2), 5, 0);
        tokio::time::advance(Duration::from_secs(11)).await;
        let _ = policy.pick().await;
        assert_eq!(policy.outlier_stats().await.ejected, vec![tcp(1)]);
        policy.update(vec![tcp(2), tcp(3), tcp(4)]).await;
        let stats = policy.outlier_stats().await;
        assert!(stats.ejected.is_empty());
        assert!(stats.ejection_counts.is_empty());
    }

    #[tokio::test]
    async fn no_detectors_never_ejects() {
        register_children();
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "childPolicy": [{"round_robin": {}}]}}]}"#,
        )
        .expect("parses");
        let policy = OutlierDetection::from_config(Some(&doc)).expect("builds");
        policy.update(vec![tcp(1), tcp(2)]).await;
        report(&policy, &tcp(1), 0, 50);
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(_)),
            "no detectors means no ejections, got {pick:?}"
        );
        assert!(policy.outlier_stats().await.ejected.is_empty());
    }

    #[tokio::test]
    async fn child_without_registered_policy_is_rejected() {
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"outlier_detection": {
                "childPolicy": [{"lb_unit_no_such_policy": {}}]}}]}"#,
        )
        .expect("parses");
        assert!(OutlierDetection::from_config(Some(&doc)).is_err());
    }
}
