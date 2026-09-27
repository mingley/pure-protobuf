//! pick_first: one sticky connection with ordered failover (A62).
//!
//! The policy keeps the resolver's address order (shuffled once when
//! the config sets `shuffleAddressList`) and sticks to the current
//! address while it dials. A failed handshake advances to the next
//! address; exhausting the list enters `TransientFailure` with
//! exponential backoff, then restarts from the front. Any address-list
//! change resets failure state. Weighted shuffling (A113) arrives
//! with CH-04; the weighted fields are ignored here.
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
    addresses: Vec<ResolvedAddress>,
    current: usize,
    /// Failures since the last success or list change.
    consecutive_failures: u32,
    /// When `TransientFailure` lifts; `None` while an address is live.
    backoff_until: Option<tokio::time::Instant>,
    backoff_rounds: u32,
    last_error: Option<Status>,
    shuffle: bool,
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

    /// Reconcile a new address list. The current address sticks when
    /// still present; otherwise (or when the list changes at all)
    /// failure state resets. Wakes [`Self::watch`] on any change.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses == state.addresses {
            return;
        }
        let current_addr = state.addresses.get(state.current).cloned();
        state.addresses = addresses;
        state.current = current_addr
            .as_ref()
            .and_then(|addr| state.addresses.iter().position(|a| a == addr))
            .unwrap_or(0);
        state.consecutive_failures = 0;
        state.backoff_until = None;
        state.backoff_rounds = 0;
        state.last_error = None;
        if state.shuffle {
            let current = state.current;
            shuffle_from(&mut state.addresses, current);
            state.current = 0;
        }
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
        match state.addresses.get(state.current).cloned() {
            Some(addr) => Pick::Use(addr),
            None => Pick::Fail(Status::unavailable("pick_first: no addresses")),
        }
    }

    /// Record a successful dial: stick here and clear failure state.
    pub async fn note_success(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = 0;
        state.backoff_until = None;
        state.backoff_rounds = 0;
        state.last_error = None;
    }

    /// Record a failed dial: advance to the next address, or enter
    /// backoff when the list is exhausted. Wakes waiters either way.
    pub async fn note_failure(&self, status: Status) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
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
            state.current = (state.current + 1) % n;
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
    let scaled = BACKOFF_BASE.as_secs_f64() * BACKOFF_MULTIPLIER.powi(rounds.min(16) as i32);
    let capped = scaled.min(BACKOFF_MAX.as_secs_f64());
    // Deterministic ±20% jitter from a counter-free hash of the rounds.
    let wobble = 0.8 + 0.4 * f64::from((rounds.wrapping_mul(2_654_435_761) >> 9) % 1000) / 1000.0;
    Duration::from_secs_f64(capped * wobble).max(Duration::from_millis(1))
}

/// Fisher–Yates from `start`, time-seeded xorshift (no rng dependency).
fn shuffle_from(addresses: &mut [ResolvedAddress], start: usize) {
    let mut rng = xorshift_seed();
    let mut i = addresses.len();
    while i > start + 1 {
        rng = xorshift_next(rng);
        let j = start + (rng as usize % (i - start));
        i -= 1;
        addresses.swap(i, j);
    }
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
        pf.note_success().await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(1)));
        // Failure advances; success sticks at the new address.
        pf.note_failure(crate::status::Status::unavailable("x"))
            .await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(2)));
        pf.note_success().await;
        assert!(matches!(pf.pick().await, Pick::Use(a) if a == tcp(2)));
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

    #[test]
    fn backoff_grows_and_caps() {
        let first = backoff_for(0);
        assert!(first >= Duration::from_millis(800) && first <= Duration::from_millis(1200));
        let later = backoff_for(100);
        assert!(later <= Duration::from_secs(120));
        assert!(later >= Duration::from_secs(90));
    }
}
