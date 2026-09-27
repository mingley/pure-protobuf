//! pick_first: one sticky connection with ordered failover (A62).
//!
//! Each address list runs the A61 pipeline — optional weighted shuffle
//! (A113, Efraimidis–Spirakis keys), then v4/v6 interleave — and the
//! policy sticks to the current address while it dials. A failed
//! handshake advances to the next address; exhausting the list enters
//! `TransientFailure` with exponential backoff, then restarts from the
//! front. Any address-list change reorders and resets failure state,
//! keeping the current address when it survives.
//!
//! Backoff follows the reference constants: 1s base, ×1.6, 20%
//! jitter, 120s cap.

use super::{LbPolicyFactory, register_lb_policy_factory};
use crate::resolver::ResolvedAddress;
use crate::service_config::ServiceConfig;
use crate::status::Status;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, watch};

/// Base delay after the whole list fails once.
const BACKOFF_BASE: Duration = Duration::from_secs(1);
/// Backoff multiplier per exhausted round.
const BACKOFF_MULTIPLIER: f64 = 1.6;
/// Backoff cap.
const BACKOFF_MAX: Duration = Duration::from_secs(120);

/// pick_first policy state shared by a channel's picks.
#[derive(Debug)]
pub struct PickFirst {
    state: Mutex<State>,
    changed: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    addresses: Vec<WeightedAddress>,
    current: usize,
    /// Failures since the last success or list change.
    consecutive_failures: u32,
    /// When `TransientFailure` lifts; `None` while an address is live.
    backoff_until: Option<tokio::time::Instant>,
    backoff_rounds: u32,
    last_error: Option<Status>,
    shuffle: bool,
}

/// One dialable address with its A113 shuffle weight. Resolvers that
/// carry no weights (DNS, static) use 1; xDS endpoints will carry
/// normalized locality × endpoint products.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeightedAddress {
    /// The address to dial.
    pub address: ResolvedAddress,
    /// Shuffle weight; 0 is treated as 1.
    pub weight: u32,
}

/// What a pick resolves to.
#[derive(Debug)]
pub enum Pick {
    /// Dial (or reuse the connection to) this address.
    Use(ResolvedAddress),
    /// No address is ready; await [`PickFirst::watch`] for movement.
    Wait,
    /// Fail the RPC now (fail-fast, or backoff still running).
    Fail(Status),
}

impl PickFirst {
    /// Build from a service-config document: the `pick_first` entry's
    /// config when selected, else defaults (no shuffle).
    #[must_use]
    pub fn from_config(config: Option<&ServiceConfig>) -> Arc<Self> {
        let shuffle = config
            .and_then(|c| {
                c.lb_policies().iter().find_map(|entry| match entry {
                    crate::service_config::LbPolicyConfig::PickFirst {
                        shuffle_address_list,
                    } => Some(*shuffle_address_list),
                    _ => None,
                })
            })
            .unwrap_or(false);
        Arc::new(Self {
            state: Mutex::new(State {
                addresses: Vec::new(),
                current: 0,
                consecutive_failures: 0,
                backoff_until: None,
                backoff_rounds: 0,
                last_error: None,
                shuffle,
            }),
            changed: watch::channel(0).0,
        })
    }

    /// Reconcile a new address list (all weight 1). Runs the A61
    /// pipeline — shuffle when configured, then family interleave —
    /// and keeps the current address when it survives. Wakes
    /// [`Self::watch`] on any change.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let weighted = addresses
            .into_iter()
            .map(|address| WeightedAddress { address, weight: 1 })
            .collect();
        self.update_ordered(weighted).await;
    }

    /// Reconcile a new address list with A113 shuffle weights.
    /// Entries compare by address and weight, so a weight-only change
    /// reshuffles. Zero weights are treated as 1.
    pub async fn update_weighted(&self, entries: Vec<(ResolvedAddress, u32)>) {
        let weighted = entries
            .into_iter()
            .map(|(address, weight)| WeightedAddress {
                address,
                weight: weight.max(1),
            })
            .collect();
        self.update_ordered(weighted).await;
    }

    async fn update_ordered(&self, mut ordered: Vec<WeightedAddress>) {
        let mut state = self.state.lock().await;
        if ordered == state.addresses {
            return;
        }
        let current_addr = state
            .addresses
            .get(state.current)
            .map(|weighted| weighted.address.clone());
        if state.shuffle {
            weighted_shuffle(&mut ordered);
        }
        let ordered = interleave_families(ordered);
        state.current = current_addr
            .as_ref()
            .and_then(|addr| ordered.iter().position(|w| &w.address == addr))
            .unwrap_or(0);
        state.addresses = ordered;
        state.consecutive_failures = 0;
        state.backoff_until = None;
        state.backoff_rounds = 0;
        state.last_error = None;
        self.changed
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Pick the current address, or wait/fail when none is usable.
    pub async fn pick(&self) -> Pick {
        let state = self.state.lock().await;
        if state.addresses.is_empty() {
            return Pick::Fail(Status::unavailable("pick_first: no addresses"));
        }
        if let Some(until) = state.backoff_until {
            if tokio::time::Instant::now() < until {
                return match &state.last_error {
                    Some(status) => Pick::Fail(status.clone()),
                    None => Pick::Wait,
                };
            }
        }
        match state.addresses.get(state.current) {
            Some(weighted) => Pick::Use(weighted.address.clone()),
            None => Pick::Fail(Status::unavailable("pick_first: no addresses")),
        }
    }

    /// Ordered dial plan for a Happy-Eyeballs race: the full list
    /// starting at the current address. Empty while backing off or
    /// when no addresses are known.
    pub async fn race_plan(&self) -> Vec<ResolvedAddress> {
        let state = self.state.lock().await;
        let n = state.addresses.len();
        if n == 0 {
            return Vec::new();
        }
        if let Some(until) = state.backoff_until {
            if tokio::time::Instant::now() < until {
                return Vec::new();
            }
        }
        let start = state.current.min(n.saturating_sub(1));
        state
            .addresses
            .iter()
            .cycle()
            .skip(start)
            .take(n)
            .map(|weighted| weighted.address.clone())
            .collect()
    }

    /// Record a successful dial to `winner`: the winning address
    /// becomes current (a Happy-Eyeballs race may have skipped past
    /// the old head) and failure state clears.
    pub async fn note_success(&self, winner: &ResolvedAddress) {
        let mut state = self.state.lock().await;
        if let Some(position) = state
            .addresses
            .iter()
            .position(|weighted| &weighted.address == winner)
        {
            state.current = position;
        }
        state.consecutive_failures = 0;
        state.backoff_until = None;
        state.backoff_rounds = 0;
        state.last_error = None;
    }

    /// Record a failed dial: advance to the next address, or enter
    /// backoff when the list is exhausted. Wakes waiters either way.
    pub async fn note_failure(&self, status: Status) {
        self.note_round_failed(1, status).await;
    }

    /// Record a Happy-Eyeballs round in which `tried` addresses all
    /// failed: advance past them, or enter backoff when the failures
    /// cover the list. Wakes waiters either way.
    pub async fn note_round_failed(&self, tried: usize, status: Status) {
        let mut state = self.state.lock().await;
        let add = u32::try_from(tried).unwrap_or(u32::MAX);
        state.consecutive_failures = state.consecutive_failures.saturating_add(add);
        state.last_error = Some(status);
        let n = state.addresses.len();
        if n == 0 {
            return;
        }
        if usize::try_from(state.consecutive_failures).unwrap_or(usize::MAX) >= n {
            // Exhausted: back off, then restart from the front.
            let delay = backoff_for(state.backoff_rounds);
            state.backoff_rounds = state.backoff_rounds.saturating_add(1);
            state.backoff_until = Some(tokio::time::Instant::now() + delay);
            state.consecutive_failures = 0;
            state.current = 0;
        } else {
            state.current = (state.current + tried) % n;
        }
        self.changed
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Movement notifications: failovers, backoff entries, and list
    /// updates. Waiters re-pick on every bump.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
}

/// Backoff for an exhausted round: 1s × 1.6^rounds ± 20%, capped.
fn backoff_for(rounds: u32) -> Duration {
    let scaled = BACKOFF_BASE.as_secs_f64()
        * BACKOFF_MULTIPLIER.powi(i32::try_from(rounds.min(16)).unwrap_or(16));
    let capped = scaled.min(BACKOFF_MAX.as_secs_f64());
    // Deterministic ±20% jitter from a counter-free hash of the rounds.
    let wobble = 0.8 + 0.4 * f64::from((rounds.wrapping_mul(2_654_435_761) >> 9) % 1000) / 1000.0;
    Duration::from_secs_f64(capped * wobble).max(Duration::from_millis(1))
}

/// Weighted random shuffle (A113): Efraimidis–Spirakis keys —
/// `u^(1/weight)` per entry from a uniform `u` in `[0, 1)`, sorted
/// descending. All-equal weights reduce to a uniform permutation.
/// Time-seeded xorshift; no rng dependency.
fn weighted_shuffle(entries: &mut [WeightedAddress]) {
    if entries.len() < 2 {
        return;
    }
    const UNIT: f64 = 4_294_967_296.0;
    let mut rng = xorshift_seed();
    let mut keyed: Vec<(f64, usize)> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        rng = xorshift_next(rng);
        let hi = u32::try_from(rng >> 32).unwrap_or(u32::MAX);
        let u = f64::from(hi) / UNIT;
        let weight = f64::from(entry.weight.max(1));
        keyed.push((u.powf(1.0 / weight), index));
    }
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    let snapshot = entries.to_vec();
    for (slot, (_, index)) in entries.iter_mut().zip(keyed.iter()) {
        if let Some(entry) = snapshot.get(*index) {
            *slot = entry.clone();
        }
    }
}

/// Interleave the two IP families (RFC 8305 §4, A61 step 3):
/// alternate addresses starting with the family of the first IP
/// address, keeping each family's relative order. Non-IP addresses
/// (unix targets are single-address in practice) keep the front in
/// their original order. Single-family lists are unchanged.
fn interleave_families(entries: Vec<WeightedAddress>) -> Vec<WeightedAddress> {
    let first_is_v4 = entries.iter().find_map(|entry| match &entry.address {
        ResolvedAddress::Tcp(sock) => Some(sock.is_ipv4()),
        _ => None,
    });
    let Some(first_is_v4) = first_is_v4 else {
        return entries;
    };
    let mut other = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    for entry in entries {
        match &entry.address {
            ResolvedAddress::Tcp(sock) if sock.is_ipv4() == first_is_v4 => first.push(entry),
            ResolvedAddress::Tcp(_) => second.push(entry),
            _ => other.push(entry),
        }
    }
    let mut ordered = other;
    let mut first = first.into_iter();
    let mut second = second.into_iter();
    loop {
        let a = first.next();
        let b = second.next();
        if a.is_none() && b.is_none() {
            break;
        }
        if let Some(entry) = a {
            ordered.push(entry);
        }
        if let Some(entry) = b {
            ordered.push(entry);
        }
    }
    ordered
}

fn xorshift_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()))
        .unwrap_or(0x9E37_79B9);
    let pid = u64::from(std::process::id()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (nanos ^ pid).max(1)
}

fn xorshift_next(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x.max(1)
}

/// Factory registering `pick_first` for A24 selection.
#[derive(Debug, Default)]
pub struct PickFirstFactory;

impl LbPolicyFactory for PickFirstFactory {
    fn name(&self) -> &str {
        "pick_first"
    }
}

/// Register `pick_first`, once. Called on the resolver-channel path so
/// the default policy needs no user setup.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(PickFirstFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::{Pick, PickFirst, backoff_for};
    use crate::resolver::ResolvedAddress;
    use std::time::Duration;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    #[tokio::test]
    async fn sticks_and_fails_over_in_order() {
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1), tcp(2)]).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(1)));
        pf.note_success(&tcp(1)).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(1)));
        // Failure advances; success sticks at the new address.
        pf.note_failure(crate::status::Status::unavailable("x"))
            .await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(2)));
        pf.note_success(&tcp(2)).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(2)));
        // A race winner past the head becomes current.
        pf.note_success(&tcp(1)).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(1)));
    }

    #[tokio::test]
    async fn exhausting_enters_backoff_then_restarts() {
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1)]).await;
        pf.note_failure(crate::status::Status::unavailable("down"))
            .await;
        // Single address exhausted: fail-fast sees the error, not Wait.
        assert!(matches!(pf.pick().await, Pick::Fail(_)));
    }

    #[tokio::test]
    async fn list_change_resets_and_keeps_current() {
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1), tcp(2)]).await;
        pf.note_failure(crate::status::Status::unavailable("x"))
            .await;
        // Current (tcp(2)) survives: still picked, failures cleared.
        pf.update(vec![tcp(2), tcp(3)]).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(2)));
        // Current removed: restart from the front.
        pf.update(vec![tcp(3)]).await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(3)));
    }

    #[tokio::test]
    async fn interleave_alternates_families_from_first() {
        fn tcp6(n: u16) -> ResolvedAddress {
            ResolvedAddress::Tcp(format!("[::1]:{n}").parse().expect("addr"))
        }
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1), tcp(2), tcp6(1), tcp6(2), tcp(3)])
            .await;
        // v4 first: v4, v6, v4, v6, v4, each family in order.
        assert_eq!(
            pf.race_plan().await,
            vec![tcp(1), tcp6(1), tcp(2), tcp6(2), tcp(3)]
        );
        // v6 first: families swap roles (fresh policy: no sticky
        // current to rotate the plan).
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp6(1), tcp(1), tcp6(2), tcp(2)]).await;
        assert_eq!(pf.race_plan().await, vec![tcp6(1), tcp(1), tcp6(2), tcp(2)]);
    }

    #[tokio::test]
    async fn race_plan_rotates_from_current() {
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1), tcp(2), tcp(3)]).await;
        pf.note_failure(crate::status::Status::unavailable("x"))
            .await;
        assert_eq!(pf.race_plan().await, vec![tcp(2), tcp(3), tcp(1)]);
    }

    #[tokio::test]
    async fn round_failure_covers_list_enters_backoff() {
        let pf = PickFirst::from_config(None);
        pf.update(vec![tcp(1), tcp(2)]).await;
        pf.note_round_failed(2, crate::status::Status::unavailable("down"))
            .await;
        assert!(matches!(pf.pick().await, Pick::Fail(_)));
        assert!(pf.race_plan().await.is_empty());
    }

    #[test]
    fn backoff_grows_and_caps() {
        let first = backoff_for(0);
        assert!(first >= Duration::from_millis(800) && first <= Duration::from_millis(1200));
        let later = backoff_for(100);
        assert!(later <= Duration::from_secs(120));
        assert!(later >= Duration::from_secs(90));
    }
}
