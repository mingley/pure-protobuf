//! LB policy registry and `loadBalancingConfig` selection (A24, CH-03).
//!
//! Policies register factories by name at initialization
//! ([`register_lb_policy_factory`]); [`select_lb_policy`] walks a
//! parsed document's `loadBalancingConfig` in preference order and
//! returns the first entry whose policy is registered. Unknown or
//! unregistered names are skipped, never fatal: an empty selection
//! means no listed policy is available. Concrete policies ship per
//! card: `pick_first` (FL-03/CH-04), `round_robin` (FL-04),
//! `weighted_round_robin` (CH-06); ring hash and the rest register
//! as they land.

#![allow(
    clippy::disallowed_types,
    reason = "init-time factory registry; reads happen at selection, never across await"
)]

use crate::service_config::{LbPolicyConfig, ServiceConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

mod health;
mod pick_first;
mod ring_hash;
mod round_robin;
mod wrr;

pub use health::{HealthSignal, disables_health_check, signal_for};
pub(crate) use pick_first::SplitMix64;
pub(crate) use pick_first::ensure_registered as ensure_pick_first_registered;
pub(crate) use pick_first::transient_backoff;
pub use pick_first::{Pick, PickFirst, PickFirstFactory, WeightedAddress};
pub(crate) use ring_hash::ensure_registered as ensure_ring_hash_registered;
pub use ring_hash::{RingHash, RingHashFactory};
pub(crate) use round_robin::ensure_registered as ensure_round_robin_registered;
pub use round_robin::{RoundRobin, RoundRobinFactory};
pub(crate) use wrr::ensure_registered as ensure_weighted_round_robin_registered;
pub use wrr::{WeightedRoundRobin, WeightedRoundRobinFactory, WrrStats};

/// Builds one LB policy's runtime from its parsed config. Only the
/// name is needed for selection; runtimes live behind [`LbPolicy`].
pub trait LbPolicyFactory: Send + Sync + 'static {
    /// Policy name as it appears in `loadBalancingConfig`
    /// (`pick_first`, `round_robin`, …).
    fn name(&self) -> &str;
}

/// A selected policy: the winning entry plus its parsed config.
#[derive(Clone, Debug)]
pub struct SelectedPolicy {
    /// Registered policy name.
    pub name: String,
    /// The winning `loadBalancingConfig` entry.
    pub policy: LbPolicyConfig,
}

/// A running LB policy on a resolver-managed channel. Variants grow
/// as policies land (ring hash, …); the pool dispatches acquire
/// per variant because connection shapes differ (one sticky slot
/// versus one subchannel per address).
#[derive(Clone, Debug)]
pub enum LbPolicy {
    /// One sticky connection with ordered failover.
    PickFirst(std::sync::Arc<PickFirst>),
    /// Rotation over ready endpoints.
    RoundRobin(std::sync::Arc<RoundRobin>),
    /// EDF scheduling over ORCA-weighted endpoints.
    WeightedRoundRobin(std::sync::Arc<WeightedRoundRobin>),
    /// Consistent hashing over endpoints.
    RingHash(std::sync::Arc<RingHash>),
}

impl LbPolicy {
    /// Reconcile a new address list.
    pub async fn update(&self, addresses: Vec<crate::resolver::ResolvedAddress>) {
        match self {
            Self::PickFirst(policy) => policy.update(addresses).await,
            Self::RoundRobin(policy) => policy.update(addresses).await,
            Self::WeightedRoundRobin(policy) => policy.update(addresses).await,
            Self::RingHash(policy) => policy.update(addresses).await,
        }
    }

    /// Movement notifications: list updates and failures.
    #[must_use]
    pub fn watch(&self) -> tokio::sync::watch::Receiver<u64> {
        match self {
            Self::PickFirst(policy) => policy.watch(),
            Self::RoundRobin(policy) => policy.watch(),
            Self::WeightedRoundRobin(policy) => policy.watch(),
            Self::RingHash(policy) => policy.watch(),
        }
    }

    /// Record a Watch starting for a freshly dialed address.
    /// Returns whether this call newly marked the address.
    pub async fn note_health_pending(&self, addr: &crate::resolver::ResolvedAddress) -> bool {
        match self {
            Self::PickFirst(policy) => policy.note_health_pending(addr).await,
            Self::RoundRobin(policy) => policy.note_health_pending(addr).await,
            Self::WeightedRoundRobin(policy) => policy.note_health_pending(addr).await,
            Self::RingHash(policy) => policy.note_health_pending(addr).await,
        }
    }

    /// Clear a Watch record without reporting.
    pub async fn note_health_gone(&self, addr: &crate::resolver::ResolvedAddress) {
        match self {
            Self::PickFirst(policy) => policy.note_health_gone(addr).await,
            Self::RoundRobin(policy) => policy.note_health_gone(addr).await,
            Self::WeightedRoundRobin(policy) => policy.note_health_gone(addr).await,
            Self::RingHash(policy) => policy.note_health_gone(addr).await,
        }
    }

    /// Record a Watch report for an address.
    pub async fn note_health(&self, addr: &crate::resolver::ResolvedAddress, signal: HealthSignal) {
        match self {
            Self::PickFirst(policy) => policy.note_health(addr, signal).await,
            Self::RoundRobin(policy) => policy.note_health(addr, signal).await,
            Self::WeightedRoundRobin(policy) => policy.note_health(addr, signal).await,
            Self::RingHash(policy) => policy.note_health(addr, signal).await,
        }
    }

    /// Last reported health: `None` while no Watch has reported.
    pub async fn health_of(&self, addr: &crate::resolver::ResolvedAddress) -> Option<HealthSignal> {
        match self {
            Self::PickFirst(policy) => policy.health_of(addr).await,
            Self::RoundRobin(policy) => policy.health_of(addr).await,
            Self::WeightedRoundRobin(policy) => policy.health_of(addr).await,
            Self::RingHash(policy) => policy.health_of(addr).await,
        }
    }

    /// Fold one ORCA report into an address's weight. Only
    /// `weighted_round_robin` consumes reports; other policies ignore
    /// them. `oob` marks OOB pump reports (per-call reports pause
    /// while OOB is enabled).
    pub async fn note_orca_report(
        &self,
        addr: &crate::resolver::ResolvedAddress,
        report: &crate::orca::OrcaLoadReport,
        oob: bool,
    ) {
        match self {
            Self::WeightedRoundRobin(policy) => policy.note_orca_report(addr, report, oob).await,
            Self::PickFirst(_) | Self::RoundRobin(_) | Self::RingHash(_) => {}
        }
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<dyn LbPolicyFactory>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<dyn LbPolicyFactory>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register an LB policy factory. Call during initialization, before
/// channels select; re-registering a name replaces its factory.
pub fn register_lb_policy_factory(factory: Arc<dyn LbPolicyFactory>) {
    if let Ok(mut guard) = registry().lock() {
        guard.insert(factory.name().to_owned(), factory);
    }
}

/// Whether a factory is registered for `name`.
#[must_use]
pub fn is_policy_registered(name: &str) -> bool {
    registry()
        .lock()
        .is_ok_and(|guard| guard.contains_key(name))
}

/// Select the first `loadBalancingConfig` entry with a registered
/// factory (A24). Returns `None` when the document lists no LB config
/// or none of its policies is registered.
#[must_use]
pub fn select_lb_policy(config: &ServiceConfig) -> Option<SelectedPolicy> {
    let guard = registry().lock().ok()?;
    for entry in config.lb_policies() {
        let name = entry.name();
        if guard.contains_key(name) {
            return Some(SelectedPolicy {
                name: name.to_owned(),
                policy: entry.clone(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{is_policy_registered, register_lb_policy_factory, select_lb_policy};
    use crate::service_config::ServiceConfig;
    use std::sync::Arc;

    struct Probe(&'static str);

    impl super::LbPolicyFactory for Probe {
        fn name(&self) -> &str {
            self.0
        }
    }

    #[test]
    fn selects_first_registered_in_preference_order() {
        register_lb_policy_factory(Arc::new(Probe("round_robin")));
        let config = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"no_such_policy": {}}, {"round_robin": {}}]}"#,
        )
        .expect("parses");
        let selected = select_lb_policy(&config).expect("selected");
        assert_eq!(selected.name, "round_robin");
        assert!(is_policy_registered("round_robin"));
        assert!(!is_policy_registered("no_such_policy"));
    }

    #[test]
    fn no_selection_without_config_or_factory() {
        let bare = ServiceConfig::parse(r#"{"methodConfig": []}"#).expect("parses");
        assert!(select_lb_policy(&bare).is_none());
        let config =
            ServiceConfig::parse(r#"{"loadBalancingConfig": [{"lb_unit_unregistered": {}}]}"#)
                .expect("parses");
        assert!(select_lb_policy(&config).is_none());
    }
}
