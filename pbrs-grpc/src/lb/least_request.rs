//! least_request: power-of-N-choices least-loaded pick (A48).
//!
//! Each pick samples `choice_count` ready addresses uniformly (with
//! replacement, like grpc-go) and returns the one with the fewest
//! in-flight RPCs; ties keep the first sampled. In-flight counts
//! cover unary attempts via [`LrTrack`] guards: the pool starts one
//! per attempt after grab and drops it when the attempt ends (RAII,
//! so hedged-task aborts still decrement). Server-streaming and
//! bidi RPCs do not move counts — they need stream-completion
//! plumbing owned by a later card — so least-request weighs unary
//! load. Failed dials back off per address like `round_robin`, and
//! health gating matches `round_robin` (A17).

use super::{HealthSignal, LbPolicyFactory, Pick, Readiness, register_lb_policy_factory};
use crate::resolver::ResolvedAddress;
use crate::service_config::LeastRequestConfig;
use crate::status::Status;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{Mutex, watch};
use tokio::time::Instant;

/// least_request policy state shared by a channel's picks.
#[derive(Debug)]
pub struct LeastRequest {
    config: LeastRequestConfig,
    state: Mutex<State>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    /// Resolver order; sampled uniformly per pick.
    addresses: Vec<ResolvedAddress>,
    /// In-flight unary attempts per address. Entries are shared
    /// atomics so [`LrTrack`] guards decrement without locking.
    in_flight: HashMap<ResolvedAddress, Arc<AtomicU64>>,
    /// Per-address failure state; absent means ready.
    down: HashMap<ResolvedAddress, Down>,
    /// A17 health reports; absent means unknown (treated ready
    /// unless the address is still awaiting its first Watch).
    unhealthy: HashSet<ResolvedAddress>,
    /// Addresses with a Watch in flight but no report yet.
    health_pending: HashSet<ResolvedAddress>,
    last_error: Option<Status>,
}

#[derive(Clone, Debug)]
struct Down {
    until: Instant,
    rounds: u32,
}

/// One in-flight unary attempt: decrements its address on drop, so
/// every completion path — including hedged-task aborts — releases
/// the count. Created by [`LeastRequest::track_start`].
#[derive(Debug)]
pub struct LrTrack {
    counter: Option<Arc<AtomicU64>>,
}

impl LrTrack {
    /// A guard that counts nothing, for non-least-request policies.
    pub(crate) fn empty() -> Self {
        Self { counter: None }
    }
}

impl Drop for LrTrack {
    fn drop(&mut self) {
        if let Some(counter) = &self.counter {
            counter.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl LeastRequest {
    /// Build from a service-config document: the `least_request`
    /// entry's config when selected, else defaults. Config snapshots
    /// at channel build, like `pick_first`.
    #[must_use]
    pub fn from_config(config: Option<&crate::service_config::ServiceConfig>) -> Arc<Self> {
        let config = config
            .and_then(|doc| {
                doc.lb_policies().iter().find_map(|entry| match entry {
                    crate::service_config::LbPolicyConfig::LeastRequest(config) => {
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
    pub fn with_config(config: LeastRequestConfig) -> Arc<Self> {
        let (bump, changed) = watch::channel(0);
        Arc::new(Self {
            config,
            state: Mutex::new(State {
                addresses: Vec::new(),
                in_flight: HashMap::new(),
                down: HashMap::new(),
                unhealthy: HashSet::new(),
                health_pending: HashSet::new(),
                last_error: None,
            }),
            changed,
            bump,
        })
    }

    /// Effective config (snapshot at build).
    #[must_use]
    pub fn config(&self) -> &LeastRequestConfig {
        &self.config
    }

    /// Reconcile a new address list. Surviving addresses keep
    /// readiness and counts (no flap on reorder); removed ones are
    /// forgotten. Wakes [`Self::watch`] on any change.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses == state.addresses {
            return;
        }
        state.in_flight.retain(|addr, _| addresses.contains(addr));
        state.down.retain(|addr, _| addresses.contains(addr));
        state.unhealthy.retain(|addr| addresses.contains(addr));
        state.health_pending.retain(|addr| addresses.contains(addr));
        state.addresses = addresses;
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Pick the least-loaded of `choice_count` uniform samples over
    /// the ready set (first sampled wins ties), or wait/fail when
    /// none is usable. Expired backoffs rejoin lazily.
    pub async fn pick(&self) -> Pick {
        let mut state = self.state.lock().await;
        if state.addresses.is_empty() {
            return Pick::Fail(Status::unavailable("least_request: no addresses"));
        }
        let now = Instant::now();
        state.down.retain(|_, down| down.until > now);
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
        let mut rng = super::SplitMix64::seed();
        let mut best: Option<(ResolvedAddress, u64)> = None;
        for _ in 0..self.config.choice_count.max(1) {
            let span = u64::try_from(ready.len()).unwrap_or(u64::MAX);
            let idx = usize::try_from(rng.next() % span).unwrap_or(0);
            let Some(addr) = ready.get(idx).cloned() else {
                continue;
            };
            let load = state
                .in_flight
                .get(&addr)
                .map(|counter| counter.load(Ordering::SeqCst))
                .unwrap_or(0);
            match &best {
                Some((_, best_load)) if load >= *best_load => {}
                _ => best = Some((addr, load)),
            }
        }
        match best {
            Some((addr, _)) => Pick::Use(addr),
            None => Pick::Wait,
        }
    }

    /// Start tracking one unary attempt on `addr`: increments the
    /// count and returns a guard that decrements on drop. Unknown
    /// addresses yield an empty guard (no counting).
    pub async fn track_start(&self, addr: &ResolvedAddress) -> LrTrack {
        let mut state = self.state.lock().await;
        if !state.addresses.contains(addr) {
            return LrTrack { counter: None };
        }
        let counter = state
            .in_flight
            .entry(addr.clone())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone();
        counter.fetch_add(1, Ordering::SeqCst);
        LrTrack {
            counter: Some(counter),
        }
    }

    /// In-flight count snapshot for tests and observability.
    pub async fn loads_snapshot(&self) -> Vec<(ResolvedAddress, u64)> {
        let state = self.state.lock().await;
        state
            .addresses
            .iter()
            .map(|addr| {
                let load = state
                    .in_flight
                    .get(addr)
                    .map(|c| c.load(Ordering::SeqCst))
                    .unwrap_or(0);
                (addr.clone(), load)
            })
            .collect()
    }

    /// Record a successful dial: the address is ready and its failure
    /// state clears.
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        state.down.remove(addr);
    }

    /// Record a Watch starting for a freshly dialed address: it stays
    /// out of picks until the first report (A17 CONNECTING).
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

    /// Record a Watch report: unhealthy addresses leave picks
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

    /// Movement notifications: list updates and failures. Backoff
    /// expiry bumps nothing; waiters re-poll as well as watch.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.clone()
    }

    /// Child connectivity snapshot for priority failover (A56).
    /// Ready when some address is pickable; Connecting when a Watch
    /// is still in flight (a dial is trying); else
    /// TransientFailure. Pure observation: expired backoffs read as
    /// usable but are not pruned here; picks do that. In-flight
    /// counts never affect usability.
    pub(crate) async fn readiness(&self) -> Readiness {
        let state = self.state.lock().await;
        let now = tokio::time::Instant::now();
        let usable = |addr: &ResolvedAddress| {
            state.down.get(addr).is_none_or(|down| down.until <= now)
                && !state.unhealthy.contains(addr)
                && !state.health_pending.contains(addr)
        };
        if state.addresses.iter().any(usable) {
            return Readiness::Ready;
        }
        if state
            .addresses
            .iter()
            .any(|addr| state.health_pending.contains(addr))
        {
            return Readiness::Connecting;
        }
        Readiness::TransientFailure
    }
}

/// Factory registering `least_request` for A24 selection.
#[derive(Debug, Default)]
pub struct LeastRequestFactory;

impl LbPolicyFactory for LeastRequestFactory {
    fn name(&self) -> &str {
        "least_request"
    }
}

/// Register `least_request`, once. Called on the resolver-channel
/// path so selected configs need no user setup.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(LeastRequestFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::{LeastRequest, Pick};
    use crate::resolver::ResolvedAddress;
    use crate::service_config::LeastRequestConfig;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    #[tokio::test]
    async fn avoids_loaded_backend() {
        let lr = LeastRequest::with_config(LeastRequestConfig { choice_count: 2 });
        lr.update(vec![tcp(1), tcp(2)]).await;
        // Saturate endpoint 1: picks land on 2 unless both
        // samples draw 1 (p=1/4 per pick with choice_count 2).
        let _held = lr.track_start(&tcp(1)).await;
        let mut idle_wins = 0u32;
        for _ in 0..40 {
            match lr.pick().await {
                Pick::Use(a) if a == tcp(2) => idle_wins += 1,
                Pick::Use(a) if a == tcp(1) => {}
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert!(
            (20..=38).contains(&idle_wins),
            "idle wins out of band: {idle_wins}/40"
        );
        // Released: both backends win samples again.
        drop(_held);
        let mut seen = [false; 2];
        for _ in 0..40 {
            match lr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] = true,
                Pick::Use(a) if a == tcp(2) => seen[1] = true,
                other => panic!("unexpected pick: {other:?}"),
            }
        }
        assert!(seen[0] && seen[1], "both backends picked when idle");
    }

    #[tokio::test]
    async fn guard_drop_releases_count() {
        let lr = LeastRequest::with_config(LeastRequestConfig::default());
        lr.update(vec![tcp(1)]).await;
        {
            let _a = lr.track_start(&tcp(1)).await;
            let _b = lr.track_start(&tcp(1)).await;
            assert_eq!(lr.loads_snapshot().await[0].1, 2);
        }
        assert_eq!(lr.loads_snapshot().await[0].1, 0);
        // Unknown addresses are not counted.
        let _u = lr.track_start(&tcp(9)).await;
        assert_eq!(lr.loads_snapshot().await[0].1, 0);
    }

    #[tokio::test]
    async fn failure_backs_off_then_rejoins() {
        let lr = LeastRequest::with_config(LeastRequestConfig::default());
        lr.update(vec![tcp(1), tcp(2)]).await;
        lr.note_failure(&tcp(1), crate::status::Status::unavailable("down"))
            .await;
        for _ in 0..6 {
            assert!(matches!(lr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
    }
}
