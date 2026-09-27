//! weighted_round_robin: EDF scheduling over ORCA-weighted endpoints (A58/A114).
//!
//! A superset of `round_robin`: each resolver address carries readiness
//! (tried on pick, backed off after a failed dial) and an ORCA weight
//! (`UpdateWeight` per load report, `GetWeight` with blackout and
//! expiration). Picks run earliest-deadline-first over the ready set,
//! rebuilt lazily at `weight_update_period` (floored at 100ms), when the
//! ready set changes, or when a report moves a weight. With fewer than
//! two weighted endpoints every member schedules at weight 1 and the
//! policy behaves as `round_robin`; zero-weight members schedule at the
//! average. Health gating matches `round_robin` (A17): unhealthy and
//! health-pending addresses leave the ready set. Reports are advisory:
//! per-call ingestion pauses while OOB is enabled, and OOB failures never
//! mark addresses unhealthy.

use super::{HealthSignal, LbPolicyFactory, Pick, register_lb_policy_factory};
use crate::orca::OrcaLoadReport;
use crate::resolver::ResolvedAddress;
use crate::service_config::WeightedRoundRobinConfig;
use crate::status::Status;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::time::Instant;

/// Floor for `weight_update_period` (A58: values below 100ms cap at 100ms).
const MIN_WEIGHT_UPDATE_PERIOD: Duration = Duration::from_millis(100);

/// weighted_round_robin policy state shared by a channel's picks.
#[derive(Debug)]
pub struct WeightedRoundRobin {
    config: WeightedRoundRobinConfig,
    state: Mutex<State>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    /// Resolver order; readiness and weights consulted per pick.
    addresses: Vec<ResolvedAddress>,
    /// Per-address failure state; absent means ready.
    down: HashMap<ResolvedAddress, Down>,
    /// A17 health reports; absent means unknown (treated ready
    /// unless the address is still awaiting its first Watch).
    unhealthy: HashSet<ResolvedAddress>,
    /// Addresses with a Watch in flight but no report yet.
    health_pending: HashSet<ResolvedAddress>,
    /// Per-address ORCA weight lifecycle; absent means no report yet.
    weights: HashMap<ResolvedAddress, EndpointWeight>,
    /// Live scheduler, rebuilt when stale (see `pick`).
    scheduler: Option<Scheduler>,
    /// Bumped on every accepted weight update.
    weights_version: u64,
    /// Addresses with an OOB pump running (pool tracks one per address).
    oob_running: HashSet<ResolvedAddress>,
    /// Accepted/ignored report and rebuild counters (A78 hooks).
    stats: WrrStats,
    last_error: Option<Status>,
}

#[derive(Clone, Debug)]
struct Down {
    until: Instant,
    rounds: u32,
}

/// A58 weight lifecycle for one endpoint: `weight` from the last accepted
/// report, `last_updated` gating expiration, `non_empty_since` gating
/// blackout (reset on re-READY and on expiry).
#[derive(Clone, Debug, Default)]
struct EndpointWeight {
    weight: f64,
    last_updated: Option<Instant>,
    non_empty_since: Option<Instant>,
}

impl EndpointWeight {
    /// A58 `UpdateWeight`: fold one report into the weight. Returns
    /// whether the weight moved (drives scheduler rebuilds).
    fn update(
        &mut self,
        report: &OrcaLoadReport,
        metric_names: &[String],
        error_penalty: f64,
        now: Instant,
    ) -> bool {
        let Some(utilization) = crate::orca::utilization(report, metric_names) else {
            return false;
        };
        let qps = crate::orca::qps(report);
        if qps <= 0.0 {
            return false;
        }
        let utilization = utilization + crate::orca::eps(report) / qps * error_penalty;
        if utilization <= 0.0 || !utilization.is_finite() {
            return false;
        }
        let weight = qps / utilization;
        if weight <= 0.0 || !weight.is_finite() {
            return false;
        }
        if self.non_empty_since.is_none() {
            self.non_empty_since = Some(now);
        }
        self.last_updated = Some(now);
        let moved = (self.weight - weight).abs() > f64::EPSILON || self.weight == 0.0;
        self.weight = weight;
        moved
    }

    /// A58 `GetWeight`: the effective weight now, or 0 while blacked out
    /// or expired (expiry also re-arms the blackout).
    fn get(&mut self, now: Instant, blackout: Duration, expiration: Duration) -> f64 {
        let Some(updated) = self.last_updated else {
            return 0.0;
        };
        if now.duration_since(updated) >= expiration {
            self.non_empty_since = None;
            return 0.0;
        }
        if !blackout.is_zero()
            && let Some(since) = self.non_empty_since
            && now.duration_since(since) < blackout
        {
            return 0.0;
        }
        self.weight
    }
}

/// Live EDF scheduler over one ready-set snapshot.
#[derive(Debug)]
struct Scheduler {
    members: Vec<ResolvedAddress>,
    edf: Edf,
    built_at: Instant,
    weights_version: u64,
}

/// Earliest-deadline-first scheduler (A58): each member is a job with
/// period inversely proportional to its weight; the earliest deadline
/// wins each pick and advances by its period. Deadlines renormalize by
/// the minimum every pick so long runs never lose precision. Initial
/// deadlines draw uniform in `[0, period)` to desynchronize members.
#[derive(Debug)]
struct Edf {
    deadlines: Vec<f64>,
    periods: Vec<f64>,
}

impl Edf {
    fn new(weights: &[f64]) -> Self {
        let mut rng = super::SplitMix64::seed();
        const UNIT: f64 = 4_294_967_296.0;
        let mut deadlines = Vec::with_capacity(weights.len());
        let mut periods = Vec::with_capacity(weights.len());
        for weight in weights {
            let period = 1.0 / weight.max(f64::MIN_POSITIVE);
            let hi = u32::try_from(rng.next() >> 32).unwrap_or(u32::MAX);
            deadlines.push(f64::from(hi) / UNIT * period);
            periods.push(period);
        }
        Self { deadlines, periods }
    }

    /// Index of the winning member; advances its deadline.
    fn pick(&mut self) -> usize {
        let mut best = 0;
        let mut min = f64::INFINITY;
        for (index, deadline) in self.deadlines.iter().enumerate() {
            if *deadline < min {
                min = *deadline;
                best = index;
            }
        }
        // Renormalize so deadlines stay near zero across long runs.
        for deadline in self.deadlines.iter_mut() {
            *deadline -= min;
        }
        if let (Some(deadline), Some(period)) =
            (self.deadlines.get_mut(best), self.periods.get(best))
        {
            *deadline += *period;
        }
        best
    }
}

/// Advisory-report counters polled by observability (A78 hooks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WrrStats {
    /// Reports that moved an endpoint weight.
    pub reports_accepted: u64,
    /// Reports ignored (no usable utilization/QPS, unknown address, or
    /// per-call reports while OOB is enabled).
    pub reports_ignored: u64,
    /// Scheduler rebuilds (stale period, ready-set change, weight move).
    pub scheduler_rebuilds: u64,
}

impl WeightedRoundRobin {
    /// Build from a service-config document: the `weighted_round_robin`
    /// entry's config when selected, else defaults (per-call reports).
    /// Config snapshots at channel build, like `pick_first`.
    #[must_use]
    pub fn from_config(config: Option<&crate::service_config::ServiceConfig>) -> Arc<Self> {
        let config = config
            .and_then(|doc| {
                doc.lb_policies().iter().find_map(|entry| match entry {
                    crate::service_config::LbPolicyConfig::WeightedRoundRobin(config) => {
                        Some(config.clone())
                    }
                    _ => None,
                })
            })
            .unwrap_or_default();
        Self::with_config(config)
    }

    /// Build with an explicit config (tests and direct construction).
    #[must_use]
    pub fn with_config(config: WeightedRoundRobinConfig) -> Arc<Self> {
        let (bump, changed) = watch::channel(0);
        Arc::new(Self {
            config,
            state: Mutex::new(State {
                addresses: Vec::new(),
                down: HashMap::new(),
                unhealthy: HashSet::new(),
                health_pending: HashSet::new(),
                weights: HashMap::new(),
                scheduler: None,
                weights_version: 0,
                oob_running: HashSet::new(),
                stats: WrrStats::default(),
                last_error: None,
            }),
            changed,
            bump,
        })
    }

    /// Effective config (snapshot at build).
    #[must_use]
    pub fn config(&self) -> &WeightedRoundRobinConfig {
        &self.config
    }

    /// OOB reporting interval when enabled; the pool pumps one OOB stream
    /// per subchannel at this period. `None` means per-call reports.
    #[must_use]
    pub async fn wants_oob(&self) -> Option<Duration> {
        let _state = self.state.lock().await;
        self.config
            .enable_oob_load_report
            .then_some(self.config.oob_reporting_period)
    }

    /// Reconcile a new address list. Surviving addresses keep readiness
    /// and weights (no flap on reorder); removed ones are forgotten.
    /// The scheduler rebuilds lazily on the next pick.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses == state.addresses {
            return;
        }
        state.down.retain(|addr, _| addresses.contains(addr));
        state.unhealthy.retain(|addr| addresses.contains(addr));
        state.health_pending.retain(|addr| addresses.contains(addr));
        state.weights.retain(|addr, _| addresses.contains(addr));
        state.oob_running.retain(|addr| addresses.contains(addr));
        state.addresses = addresses;
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Pick by EDF over the ready set, or wait/fail when none is usable.
    /// Expired backoffs rejoin lazily and re-arm their weight blackout
    /// (A58 `OnSubchannelBecomesReady`).
    pub async fn pick(&self) -> Pick {
        let mut state = self.state.lock().await;
        if state.addresses.is_empty() {
            return Pick::Fail(Status::unavailable("weighted_round_robin: no addresses"));
        }
        let now = Instant::now();
        let expired: Vec<ResolvedAddress> = state
            .down
            .iter()
            .filter(|(_, down)| down.until <= now)
            .map(|(addr, _)| addr.clone())
            .collect();
        for addr in &expired {
            state.down.remove(addr);
            // Re-READY: the next report restarts the blackout clock.
            if let Some(weight) = state.weights.get_mut(addr) {
                weight.non_empty_since = None;
            }
        }
        let ready: Vec<ResolvedAddress> = state
            .addresses
            .iter()
            .filter(|addr| {
                !state.down.contains_key(*addr)
                    && !state.unhealthy.contains(*addr)
                    && !state.health_pending.contains(*addr)
            })
            .cloned()
            .collect();
        if ready.is_empty() {
            // Nothing ready: pending Watch calls mean CONNECTING (wait),
            // otherwise fail with the last dial error.
            if state.addresses.iter().any(|addr| {
                !state.down.contains_key(addr)
                    && !state.unhealthy.contains(addr)
                    && state.health_pending.contains(addr)
            }) {
                return Pick::Wait;
            }
            return match state.last_error.clone() {
                Some(status) => Pick::Fail(status),
                None => Pick::Wait,
            };
        }
        let update_period = self
            .config
            .weight_update_period
            .max(MIN_WEIGHT_UPDATE_PERIOD);
        let stale = match state.scheduler.as_ref() {
            None => true,
            Some(scheduler) => {
                scheduler.members != ready
                    || scheduler.weights_version != state.weights_version
                    || now.duration_since(scheduler.built_at) >= update_period
            }
        };
        if stale {
            let weights = self.effective_weights(&mut state, &ready, now);
            state.scheduler = Some(Scheduler {
                members: ready.clone(),
                edf: Edf::new(&weights),
                built_at: now,
                weights_version: state.weights_version,
            });
            state.stats.scheduler_rebuilds += 1;
        }
        let index = state
            .scheduler
            .as_mut()
            .map(|scheduler| scheduler.edf.pick())
            .unwrap_or(0);
        match ready.get(index).cloned() {
            Some(addr) => Pick::Use(addr),
            None => Pick::Wait,
        }
    }

    /// A58 scheduler weights for `ready`: `GetWeight` per member, all 1.0
    /// when fewer than two members carry load info, zero members at the
    /// average otherwise.
    fn effective_weights(
        &self,
        state: &mut State,
        ready: &[ResolvedAddress],
        now: Instant,
    ) -> Vec<f64> {
        let mut raw = Vec::with_capacity(ready.len());
        for addr in ready {
            let weight = state
                .weights
                .get_mut(addr)
                .map(|entry| {
                    entry.get(
                        now,
                        self.config.blackout_period,
                        self.config.weight_expiration_period,
                    )
                })
                .unwrap_or(0.0);
            raw.push(weight);
        }
        let nonzero: Vec<f64> = raw.iter().copied().filter(|w| *w > 0.0).collect();
        if nonzero.len() < 2 {
            return vec![1.0; ready.len()];
        }
        let average = nonzero.iter().sum::<f64>() / nonzero.len() as f64;
        raw.into_iter()
            .map(|weight| if weight > 0.0 { weight } else { average })
            .collect()
    }

    /// Fold one ORCA report (per-call trailer or OOB message) into an
    /// address's weight. Unknown addresses and reports without usable
    /// utilization/QPS are ignored; per-call reports are ignored while
    /// OOB is enabled (pass `oob: true` for OOB pump reports).
    pub async fn note_orca_report(
        &self,
        addr: &ResolvedAddress,
        report: &OrcaLoadReport,
        oob: bool,
    ) {
        let mut state = self.state.lock().await;
        if !state.addresses.contains(addr) {
            state.stats.reports_ignored += 1;
            return;
        }
        if !oob && self.config.enable_oob_load_report {
            state.stats.reports_ignored += 1;
            return;
        }
        let entry = state.weights.entry(addr.clone()).or_default();
        let moved = entry.update(
            report,
            &self.config.metric_names_for_computing_utilization,
            self.config.error_utilization_penalty,
            Instant::now(),
        );
        if moved {
            state.weights_version += 1;
            state.stats.reports_accepted += 1;
        } else {
            state.stats.reports_ignored += 1;
        }
    }

    /// Effective `GetWeight` per address now, for observability (A78
    /// hooks) and tests. Blacked-out and expired entries read 0.
    pub async fn weights_snapshot(&self) -> Vec<(ResolvedAddress, f64)> {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        let blackout = self.config.blackout_period;
        let expiration = self.config.weight_expiration_period;
        let addrs = state.addresses.clone();
        addrs
            .into_iter()
            .map(|addr| {
                let weight = state
                    .weights
                    .get_mut(&addr)
                    .map(|entry| entry.get(now, blackout, expiration))
                    .unwrap_or(0.0);
                (addr, weight)
            })
            .collect()
    }

    /// Advisory-report counters (A78 hooks).
    pub async fn stats(&self) -> WrrStats {
        self.state.lock().await.stats
    }

    /// Record a successful dial: the address is ready and its failure
    /// state clears. Clearing early (before backoff expiry) also re-arms
    /// the weight blackout, as a re-READY does.
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        if state.down.remove(addr).is_some()
            && let Some(weight) = state.weights.get_mut(addr)
        {
            weight.non_empty_since = None;
        }
    }

    /// Record a Watch starting for a freshly dialed address: it stays
    /// out of the scheduler until the first report (A17 CONNECTING).
    /// Returns whether this call newly marked the address.
    pub async fn note_health_pending(&self, addr: &ResolvedAddress) -> bool {
        let mut state = self.state.lock().await;
        if state.addresses.contains(addr) {
            state.health_pending.insert(addr.clone())
        } else {
            false
        }
    }

    /// Clear a Watch record without reporting: the Watch ended (drain,
    /// discard, shutdown) before any terminal signal.
    pub async fn note_health_gone(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        if state.health_pending.remove(addr) {
            self.bump
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }

    /// Record a Watch report: unhealthy addresses leave the scheduler
    /// immediately; healthy ones (re)join. Unknown addresses are ignored.
    pub async fn note_health(&self, addr: &ResolvedAddress, signal: HealthSignal) {
        let mut state = self.state.lock().await;
        if !state.addresses.contains(addr) {
            return;
        }
        state.health_pending.remove(addr);
        match signal {
            HealthSignal::Healthy => state.unhealthy.remove(addr),
            HealthSignal::Unhealthy => state.unhealthy.insert(addr.clone()),
        };
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Last reported health: `None` while no Watch has reported.
    /// Unknown addresses report `None`.
    pub async fn health_of(&self, addr: &ResolvedAddress) -> Option<HealthSignal> {
        let state = self.state.lock().await;
        if !state.addresses.contains(addr) || state.health_pending.contains(addr) {
            return None;
        }
        Some(if state.unhealthy.contains(addr) {
            HealthSignal::Unhealthy
        } else {
            HealthSignal::Healthy
        })
    }

    /// Record a failed dial: the address backs off exponentially
    /// (same 1s × 1.6, 120s cap as pick_first) and rejoins when the
    /// backoff lapses. Wakes waiters.
    pub async fn note_failure(&self, addr: &ResolvedAddress, status: Status) {
        let mut state = self.state.lock().await;
        let rounds = state.down.get(addr).map_or(0, |down| down.rounds);
        let delay = super::pick_first::transient_backoff(rounds);
        state.down.insert(
            addr.clone(),
            Down {
                until: Instant::now() + delay,
                rounds: rounds.saturating_add(1),
            },
        );
        state.last_error = Some(status);
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Claim the OOB pump slot for an address: true when this call newly
    /// claimed it (the caller spawns the pump), false when one runs.
    pub async fn note_oob_started(&self, addr: &ResolvedAddress) -> bool {
        let mut state = self.state.lock().await;
        if state.addresses.contains(addr) {
            state.oob_running.insert(addr.clone())
        } else {
            false
        }
    }

    /// Release the OOB pump slot (drain, discard, shutdown, or
    /// UNIMPLEMENTED from a backend without the OOB service).
    pub async fn note_oob_gone(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        state.oob_running.remove(addr);
    }

    /// Movement notifications: list updates and failures. Backoff
    /// expiry and weight moves bump nothing; waiters re-poll as well
    /// as watch.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.clone()
    }
}

/// Factory registering `weighted_round_robin` for A24 selection.
#[derive(Debug, Default)]
pub struct WeightedRoundRobinFactory;

impl LbPolicyFactory for WeightedRoundRobinFactory {
    fn name(&self) -> &str {
        "weighted_round_robin"
    }
}

/// Register `weighted_round_robin`, once. Called on the
/// resolver-channel path so selected configs need no user setup.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(WeightedRoundRobinFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::{Edf, Pick, WeightedRoundRobin};
    use crate::orca::OrcaLoadReport;
    use crate::resolver::ResolvedAddress;
    use crate::service_config::WeightedRoundRobinConfig;
    use std::time::Duration;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    fn down() -> crate::status::Status {
        crate::status::Status::unavailable("down")
    }

    fn report(utilization: f64, qps: f64) -> OrcaLoadReport {
        let mut r = OrcaLoadReport::new();
        r.set_application_utilization(utilization);
        r.set_rps_fractional(qps);
        r
    }

    fn fast_config() -> WeightedRoundRobinConfig {
        WeightedRoundRobinConfig {
            blackout_period: Duration::ZERO,
            weight_update_period: Duration::from_millis(100),
            ..WeightedRoundRobinConfig::default()
        }
    }

    #[test]
    fn edf_spreads_proportionally() {
        // 3:1 weights over 400 picks stay within a tight band.
        let mut edf = Edf::new(&[3.0, 1.0]);
        let mut hits = [0u32; 2];
        for _ in 0..400 {
            hits[edf.pick()] += 1;
        }
        assert!(hits[0] >= 280 && hits[0] <= 320, "hits: {hits:?}");
        assert_eq!(hits[0] + hits[1], 400);
    }

    #[test]
    fn edf_smooths_bursts() {
        // No member waits more than its period: with 3:1 weights the
        // light member appears at least every 5 picks.
        let mut edf = Edf::new(&[3.0, 1.0]);
        let mut gap = 0;
        for _ in 0..40 {
            if edf.pick() == 1 {
                gap = 0;
            } else {
                gap += 1;
                assert!(gap <= 5, "long gap without the light member");
            }
        }
    }

    #[tokio::test]
    async fn no_reports_behaves_as_round_robin() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        let mut seen = [0u32; 2];
        for _ in 0..20 {
            match wrr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] += 1,
                Pick::Use(a) if a == tcp(2) => seen[1] += 1,
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert_eq!(seen, [10, 10]);
    }

    #[tokio::test]
    async fn weights_converge_to_reported_ratio() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        // Equal QPS, 3x utilization gap: endpoint 2 weighs 3x endpoint 1.
        wrr.note_orca_report(&tcp(1), &report(0.9, 100.0), false)
            .await;
        wrr.note_orca_report(&tcp(2), &report(0.3, 100.0), false)
            .await;
        let mut hits = [0u32; 2];
        for _ in 0..400 {
            match wrr.pick().await {
                Pick::Use(a) if a == tcp(1) => hits[0] += 1,
                Pick::Use(a) if a == tcp(2) => hits[1] += 1,
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert!(hits[1] >= 280 && hits[1] <= 320, "hits: {hits:?}");
        let stats = wrr.stats().await;
        assert_eq!(stats.reports_accepted, 2);
        assert_eq!(stats.reports_ignored, 0);
        assert!(stats.scheduler_rebuilds >= 1);
    }

    #[tokio::test]
    async fn single_weighted_endpoint_stays_round_robin() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        wrr.note_orca_report(&tcp(1), &report(0.1, 100.0), false)
            .await;
        let mut seen = [0u32; 2];
        for _ in 0..20 {
            match wrr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] += 1,
                Pick::Use(a) if a == tcp(2) => seen[1] += 1,
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert_eq!(seen, [10, 10]);
    }

    #[tokio::test]
    async fn blackout_holds_new_weights() {
        let config = WeightedRoundRobinConfig {
            blackout_period: Duration::from_secs(60),
            weight_update_period: Duration::from_millis(100),
            ..WeightedRoundRobinConfig::default()
        };
        let wrr = WeightedRoundRobin::with_config(config);
        wrr.update(vec![tcp(1), tcp(2)]).await;
        wrr.note_orca_report(&tcp(1), &report(0.9, 100.0), false)
            .await;
        wrr.note_orca_report(&tcp(2), &report(0.1, 100.0), false)
            .await;
        // Blacked out: both read 0, so picks split evenly.
        for (_, weight) in wrr.weights_snapshot().await {
            assert_eq!(weight, 0.0);
        }
        let mut seen = [0u32; 2];
        for _ in 0..20 {
            match wrr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] += 1,
                Pick::Use(a) if a == tcp(2) => seen[1] += 1,
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert_eq!(seen, [10, 10]);
    }

    #[tokio::test]
    async fn per_call_ignored_while_oob_enabled() {
        let config = WeightedRoundRobinConfig {
            enable_oob_load_report: true,
            blackout_period: Duration::ZERO,
            ..WeightedRoundRobinConfig::default()
        };
        let wrr = WeightedRoundRobin::with_config(config);
        wrr.update(vec![tcp(1)]).await;
        wrr.note_orca_report(&tcp(1), &report(0.5, 100.0), false)
            .await;
        assert_eq!(wrr.weights_snapshot().await[0].1, 0.0);
        wrr.note_orca_report(&tcp(1), &report(0.5, 100.0), true)
            .await;
        assert!(wrr.weights_snapshot().await[0].1 > 0.0);
        let stats = wrr.stats().await;
        assert_eq!(stats.reports_accepted, 1);
        assert_eq!(stats.reports_ignored, 1);
    }

    #[tokio::test]
    async fn error_penalty_downweights_errors() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        let mut err = report(0.5, 100.0);
        err.set_eps(50.0);
        wrr.note_orca_report(&tcp(1), &err, false).await;
        wrr.note_orca_report(&tcp(2), &report(0.5, 100.0), false)
            .await;
        // Penalty 1.0: endpoint 1 utilization doubles (0.5 + 50/100),
        // so it weighs half of endpoint 2.
        let snap = wrr.weights_snapshot().await;
        assert!((snap[0].1 * 2.0 - snap[1].1).abs() < 1e-6, "snap: {snap:?}");
    }

    #[tokio::test]
    async fn unhealthy_leaves_scheduler_and_rejoins() {
        use super::HealthSignal;
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        wrr.note_health(&tcp(1), HealthSignal::Unhealthy).await;
        for _ in 0..6 {
            assert!(matches!(wrr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
        wrr.note_health(&tcp(1), HealthSignal::Healthy).await;
        let mut seen = false;
        for _ in 0..10 {
            if matches!(wrr.pick().await, Pick::Use(a) if a == tcp(1)) {
                seen = true;
            }
        }
        assert!(seen, "healthy address rejoins the scheduler");
    }

    #[tokio::test]
    async fn failure_backs_off_then_rejoins() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1), tcp(2)]).await;
        wrr.note_failure(&tcp(1), down()).await;
        for _ in 0..6 {
            assert!(matches!(wrr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
    }

    #[tokio::test]
    async fn oob_slot_claims_once() {
        let wrr = WeightedRoundRobin::with_config(fast_config());
        wrr.update(vec![tcp(1)]).await;
        assert!(wrr.note_oob_started(&tcp(1)).await);
        assert!(!wrr.note_oob_started(&tcp(1)).await);
        wrr.note_oob_gone(&tcp(1)).await;
        assert!(wrr.note_oob_started(&tcp(1)).await);
        assert!(!wrr.note_oob_started(&tcp(9)).await);
    }
}
