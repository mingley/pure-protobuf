//! round_robin: rotation over ready endpoints (FL-04).
//!
//! Each resolver address carries lazy readiness: tried on rotation,
//! marked down with backoff after a failed dial, and retried once
//! the backoff lapses. Picks rotate strictly over the ready set and
//! advance past the returned address, so consecutive RPCs spread
//! across backends. List updates keep surviving addresses' state
//! (no flap on reorder) and drop removed ones; removed connections
//! drain in the pool, never migrate streams.

use super::{HealthSignal, LbPolicyFactory, Pick, register_lb_policy_factory};
use crate::resolver::ResolvedAddress;
use crate::status::Status;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, watch};

/// round_robin policy state shared by a channel's picks.
#[derive(Debug)]
pub struct RoundRobin {
    state: Mutex<State>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    /// Resolver order; readiness consulted per pick.
    addresses: Vec<ResolvedAddress>,
    /// Rotation cursor: index to start the next scan.
    cursor: usize,
    /// Per-address failure state; absent means ready.
    down: HashMap<ResolvedAddress, Down>,
    /// A17 health reports; absent means unknown (treated ready
    /// unless the address is still awaiting its first Watch).
    unhealthy: HashSet<ResolvedAddress>,
    /// Addresses with a Watch in flight but no report yet. Skipped
    /// like down addresses; subchannels stay CONNECTING until the
    /// first response.
    health_pending: HashSet<ResolvedAddress>,
    last_error: Option<Status>,
}

#[derive(Clone, Debug)]
struct Down {
    until: tokio::time::Instant,
    rounds: u32,
}

impl RoundRobin {
    /// Build a new policy. `round_robin` takes no config knobs.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let (bump, changed) = watch::channel(0);
        Arc::new(Self {
            state: Mutex::new(State {
                addresses: Vec::new(),
                cursor: 0,
                down: HashMap::new(),
                unhealthy: HashSet::new(),
                health_pending: HashSet::new(),
                last_error: None,
            }),
            changed,
            bump,
        })
    }

    /// Reconcile a new address list. Surviving addresses keep their
    /// readiness; removed ones are forgotten (the pool drains their
    /// connections). Wakes [`Self::watch`] on any change.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses == state.addresses {
            return;
        }
        state.down.retain(|addr, _| addresses.contains(addr));
        state.unhealthy.retain(|addr| addresses.contains(addr));
        state.health_pending.retain(|addr| addresses.contains(addr));
        state.addresses = addresses;
        if state.addresses.is_empty() {
            state.cursor = 0;
        } else {
            state.cursor %= state.addresses.len();
        }
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Pick the next ready address in rotation, or wait/fail when
    /// none is usable. Expired backoffs rejoin the ready set lazily.
    pub async fn pick(&self) -> Pick {
        let mut state = self.state.lock().await;
        let n = state.addresses.len();
        if n == 0 {
            return Pick::Fail(Status::unavailable("round_robin: no addresses"));
        }
        let now = tokio::time::Instant::now();
        state.down.retain(|_, down| down.until > now);
        let start = state.cursor % n;
        for offset in 0..n {
            let index = (start + offset) % n;
            let Some(addr) = state.addresses.get(index).cloned() else {
                continue;
            };
            if state.down.contains_key(&addr) {
                continue;
            }
            if state.unhealthy.contains(&addr) || state.health_pending.contains(&addr) {
                continue;
            }
            state.cursor = (index + 1) % n;
            return Pick::Use(addr);
        }
        // Nothing ready: pending Watch calls mean CONNECTING (wait),
        // otherwise fail with the last dial error.
        if state.addresses.iter().any(|addr| {
            !state.down.contains_key(addr)
                && !state.unhealthy.contains(addr)
                && state.health_pending.contains(addr)
        }) {
            return Pick::Wait;
        }
        match state.last_error.clone() {
            Some(status) => Pick::Fail(status),
            None => Pick::Wait,
        }
    }

    /// Record a successful dial: the address is ready and its failure
    /// state clears.
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        state.down.remove(addr);
    }

    /// Record a Watch starting for a freshly dialed address: it stays
    /// out of rotation until the first report (A17 CONNECTING).
    /// Returns whether this call newly marked the address (false when
    /// a Watch is already recorded in flight).
    pub async fn note_health_pending(&self, addr: &ResolvedAddress) -> bool {
        let mut state = self.state.lock().await;
        if state.addresses.contains(addr) {
            state.health_pending.insert(addr.clone())
        } else {
            false
        }
    }

    /// Clear a Watch record without reporting: the Watch ended (drain,
    /// discard, shutdown) before any terminal signal. Wakes waiters so
    /// they re-check; a redial re-pends and spawns a fresh Watch.
    pub async fn note_health_gone(&self, addr: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        if state.health_pending.remove(addr) {
            self.bump
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }

    /// Record a Watch report: unhealthy addresses leave rotation
    /// immediately; healthy ones (re)join. Unknown addresses (removed
    /// mid-Watch) are ignored. Wakes waiters on any change.
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
        // Every report moves waiters: a first report clears pending
        // even when the unhealthy set itself is unchanged.
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
    /// (same 1s × 1.6, 120s cap as pick_first) and rejoins rotation
    /// when the backoff lapses. Wakes waiters.
    pub async fn note_failure(&self, addr: &ResolvedAddress, status: Status) {
        let mut state = self.state.lock().await;
        let rounds = state.down.get(addr).map_or(0, |down| down.rounds);
        let delay = super::pick_first::transient_backoff(rounds);
        state.down.insert(
            addr.clone(),
            Down {
                until: tokio::time::Instant::now() + delay,
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
}

/// Factory registering `round_robin` for A24 selection.
#[derive(Debug, Default)]
pub struct RoundRobinFactory;

impl LbPolicyFactory for RoundRobinFactory {
    fn name(&self) -> &str {
        "round_robin"
    }
}

/// Register `round_robin`, once. Called on the resolver-channel path
/// so selected configs need no user setup.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(RoundRobinFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::{Pick, RoundRobin};
    use crate::resolver::ResolvedAddress;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    fn down() -> crate::status::Status {
        crate::status::Status::unavailable("down")
    }

    #[tokio::test]
    async fn rotates_strictly_over_ready_set() {
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1), tcp(2), tcp(3)]).await;
        for want in [tcp(1), tcp(2), tcp(3), tcp(1), tcp(2)] {
            assert!(matches!(rr.pick().await, Pick::Use(a) if a == want));
        }
    }

    #[tokio::test]
    async fn failed_address_leaves_rotation_then_rejoins() {
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1), tcp(2)]).await;
        rr.note_failure(&tcp(1), down()).await;
        // Only tcp(2) serves while tcp(1) backs off.
        for _ in 0..3 {
            assert!(matches!(rr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
        rr.note_success(&tcp(1)).await;
        let mut seen = [false; 2];
        for _ in 0..4 {
            match rr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] = true,
                Pick::Use(a) if a == tcp(2) => seen[1] = true,
                _ => panic!("expected an address pick"),
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[tokio::test]
    async fn all_down_fails_until_backoff_lapses() {
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1)]).await;
        rr.note_failure(&tcp(1), down()).await;
        assert!(matches!(rr.pick().await, Pick::Fail(_)));
    }

    #[tokio::test]
    async fn health_pending_skips_until_first_report() {
        use super::HealthSignal;
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1), tcp(2)]).await;
        assert!(rr.note_health_pending(&tcp(1)).await);
        assert!(!rr.note_health_pending(&tcp(1)).await);
        // Pending tcp(1) serves nothing until its Watch reports.
        for _ in 0..3 {
            assert!(matches!(rr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
        assert_eq!(rr.health_of(&tcp(1)).await, None);
        rr.note_health(&tcp(1), HealthSignal::Healthy).await;
        assert_eq!(rr.health_of(&tcp(1)).await, Some(HealthSignal::Healthy));
        let mut seen = [false; 2];
        for _ in 0..4 {
            match rr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] = true,
                Pick::Use(a) if a == tcp(2) => seen[1] = true,
                _ => panic!("expected an address pick"),
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[tokio::test]
    async fn unhealthy_leaves_and_rejoins_on_recovery() {
        use super::HealthSignal;
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1), tcp(2)]).await;
        rr.note_health(&tcp(1), HealthSignal::Unhealthy).await;
        for _ in 0..3 {
            assert!(matches!(rr.pick().await, Pick::Use(a) if a == tcp(2)));
        }
        rr.note_health(&tcp(1), HealthSignal::Healthy).await;
        let mut seen = [false; 2];
        for _ in 0..4 {
            match rr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] = true,
                Pick::Use(a) if a == tcp(2) => seen[1] = true,
                _ => panic!("expected an address pick"),
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[tokio::test]
    async fn update_keeps_survivor_state_and_drops_removed() {
        let rr = RoundRobin::new();
        rr.update(vec![tcp(1), tcp(2)]).await;
        rr.note_failure(&tcp(1), down()).await;
        // Reorder keeps tcp(1) down; removal forgets it.
        rr.update(vec![tcp(2), tcp(1)]).await;
        assert!(matches!(rr.pick().await, Pick::Use(a) if a == tcp(2)));
        rr.update(vec![tcp(2)]).await;
        rr.update(vec![tcp(1), tcp(2)]).await;
        let mut seen = [false; 2];
        for _ in 0..4 {
            match rr.pick().await {
                Pick::Use(a) if a == tcp(1) => seen[0] = true,
                Pick::Use(a) if a == tcp(2) => seen[1] = true,
                _ => panic!("expected an address pick"),
            }
        }
        assert!(seen.iter().all(|s| *s));
    }
}
