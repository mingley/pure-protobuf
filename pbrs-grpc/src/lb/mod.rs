//! LB policy registry and `loadBalancingConfig` selection (A24, CH-03).
//!
//! Policies register factories by name at initialization
//! ([`register_lb_policy_factory`]); [`select_lb_policy`] walks a
//! parsed document's `loadBalancingConfig` in preference order and
//! returns the first entry whose policy is registered. Unknown or
//! unregistered names are skipped, never fatal: an empty selection
//! means no listed policy is available. Concrete policies ship per
//! card: `pick_first` (FL-03/CH-04), `round_robin` (FL-04),
//! `weighted_round_robin` (CH-06), `ring_hash`, `least_request`,
//! `random_subsetting_experimental` (CH-07); the rest register as
//! they land.

#![allow(
    clippy::disallowed_types,
    reason = "init-time factory registry; reads happen at selection, never across await"
)]

use crate::service_config::{LbPolicyConfig, ServiceConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

mod health;
mod least_request;
mod pick_first;
mod ring_hash;
mod round_robin;
mod subset;
mod wrr;

pub use health::{HealthSignal, disables_health_check, signal_for};
pub(crate) use least_request::LrTrack;
pub(crate) use least_request::ensure_registered as ensure_least_request_registered;
pub use least_request::{LeastRequest, LeastRequestFactory};
pub(crate) use pick_first::SplitMix64;
pub(crate) use pick_first::ensure_registered as ensure_pick_first_registered;
pub(crate) use pick_first::transient_backoff;
pub use pick_first::{Pick, PickFirst, PickFirstFactory, WeightedAddress};
pub(crate) use ring_hash::ensure_registered as ensure_ring_hash_registered;
pub(crate) use ring_hash::xxh64;
pub use ring_hash::{RingHash, RingHashFactory};
pub(crate) use round_robin::ensure_registered as ensure_round_robin_registered;
pub use round_robin::{RoundRobin, RoundRobinFactory};
pub(crate) use subset::ensure_registered as ensure_random_subsetting_registered;
pub use subset::{RandomSubsetting, RandomSubsettingFactory};
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
/// as policies land; the pool dispatches acquire per variant
/// because connection shapes differ (one sticky slot versus one
/// subchannel per address).
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
    /// Least-loaded of sampled candidates.
    LeastRequest(std::sync::Arc<LeastRequest>),
    /// Rendezvous subsetting over a child policy.
    RandomSubsetting(std::sync::Arc<RandomSubsetting>),
}

/// Build a running policy from one config entry. Used for the
/// top-level selection and for subset children alike, so both paths
/// construct identical runtimes. Entries without a runtime in this
/// build (`Priority`, `Unknown`) fail; callers surface that as an
/// invalid selection.
pub(crate) fn instantiate(
    entry: &crate::service_config::LbPolicyConfig,
) -> Result<LbPolicy, crate::status::Status> {
    use crate::service_config::LbPolicyConfig;
    match entry {
        LbPolicyConfig::PickFirst {
            shuffle_address_list,
        } => Ok(LbPolicy::PickFirst(PickFirst::with_shuffle(
            *shuffle_address_list,
        ))),
        LbPolicyConfig::RoundRobin => Ok(LbPolicy::RoundRobin(RoundRobin::new())),
        LbPolicyConfig::WeightedRoundRobin(config) => Ok(LbPolicy::WeightedRoundRobin(
            WeightedRoundRobin::with_config(config.clone()),
        )),
        LbPolicyConfig::RingHash(config) => {
            Ok(LbPolicy::RingHash(RingHash::with_config(config.clone())))
        }
        LbPolicyConfig::LeastRequest(config) => Ok(LbPolicy::LeastRequest(
            LeastRequest::with_config(config.clone()),
        )),
        LbPolicyConfig::RandomSubsetting(config) => Ok(LbPolicy::RandomSubsetting(
            RandomSubsetting::with_config(config)?,
        )),
        LbPolicyConfig::Priority => Err(crate::status::Status::unimplemented(
            "priority LB has no runtime in this build",
        )),
        LbPolicyConfig::Unknown(name) => Err(crate::status::Status::invalid_argument(format!(
            "LB policy {name:?} has no runtime in this build"
        ))),
    }
}

impl LbPolicy {
    /// Reconcile a new address list.
    pub async fn update(&self, addresses: Vec<crate::resolver::ResolvedAddress>) {
        match self {
            Self::PickFirst(policy) => policy.update(addresses).await,
            Self::RoundRobin(policy) => policy.update(addresses).await,
            Self::WeightedRoundRobin(policy) => policy.update(addresses).await,
            Self::RingHash(policy) => policy.update(addresses).await,
            Self::LeastRequest(policy) => policy.update(addresses).await,
            Self::RandomSubsetting(policy) => policy.update(addresses).await,
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
            Self::LeastRequest(policy) => policy.watch(),
            Self::RandomSubsetting(policy) => policy.watch(),
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
            Self::LeastRequest(policy) => policy.note_health_pending(addr).await,
            Self::RandomSubsetting(policy) => policy.note_health_pending(addr).await,
        }
    }

    /// Clear a Watch record without reporting.
    pub async fn note_health_gone(&self, addr: &crate::resolver::ResolvedAddress) {
        match self {
            Self::PickFirst(policy) => policy.note_health_gone(addr).await,
            Self::RoundRobin(policy) => policy.note_health_gone(addr).await,
            Self::WeightedRoundRobin(policy) => policy.note_health_gone(addr).await,
            Self::RingHash(policy) => policy.note_health_gone(addr).await,
            Self::LeastRequest(policy) => policy.note_health_gone(addr).await,
            Self::RandomSubsetting(policy) => policy.note_health_gone(addr).await,
        }
    }

    /// Record a Watch report for an address.
    pub async fn note_health(&self, addr: &crate::resolver::ResolvedAddress, signal: HealthSignal) {
        match self {
            Self::PickFirst(policy) => policy.note_health(addr, signal).await,
            Self::RoundRobin(policy) => policy.note_health(addr, signal).await,
            Self::WeightedRoundRobin(policy) => policy.note_health(addr, signal).await,
            Self::RingHash(policy) => policy.note_health(addr, signal).await,
            Self::LeastRequest(policy) => policy.note_health(addr, signal).await,
            Self::RandomSubsetting(policy) => policy.note_health(addr, signal).await,
        }
    }

    /// Last reported health: `None` while no Watch has reported.
    pub async fn health_of(&self, addr: &crate::resolver::ResolvedAddress) -> Option<HealthSignal> {
        match self {
            Self::PickFirst(policy) => policy.health_of(addr).await,
            Self::RoundRobin(policy) => policy.health_of(addr).await,
            Self::WeightedRoundRobin(policy) => policy.health_of(addr).await,
            Self::RingHash(policy) => policy.health_of(addr).await,
            Self::LeastRequest(policy) => policy.health_of(addr).await,
            Self::RandomSubsetting(policy) => policy.health_of(addr).await,
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
            Self::RandomSubsetting(policy) => policy.note_orca_report(addr, report, oob).await,
            Self::PickFirst(_)
            | Self::RoundRobin(_)
            | Self::RingHash(_)
            | Self::LeastRequest(_) => {}
        }
    }

    /// Pick an address. Ring children need [`Self::pick_hash`].
    pub async fn pick(&self) -> Pick {
        match self {
            Self::PickFirst(policy) => policy.pick().await,
            Self::RoundRobin(policy) => policy.pick().await,
            Self::WeightedRoundRobin(policy) => policy.pick().await,
            Self::LeastRequest(policy) => policy.pick().await,
            Self::RandomSubsetting(policy) => policy.pick().await,
            Self::RingHash(policy) => policy.pick_hash(None).await,
        }
    }

    /// Pick with a request hash. Only ring policies (and subsets
    /// over them) use the hash; the rest delegate to [`Self::pick`].
    pub async fn pick_hash(&self, hash: Option<u64>) -> Pick {
        match self {
            Self::RingHash(policy) => policy.pick_hash(hash).await,
            Self::RandomSubsetting(policy) => policy.pick_hash(hash).await,
            Self::PickFirst(policy) => policy.pick().await,
            Self::RoundRobin(policy) => policy.pick().await,
            Self::WeightedRoundRobin(policy) => policy.pick().await,
            Self::LeastRequest(policy) => policy.pick().await,
        }
    }

    /// Record a successful dial on an address.
    pub async fn note_success(&self, addr: &crate::resolver::ResolvedAddress) {
        match self {
            Self::PickFirst(policy) => policy.note_success(addr).await,
            Self::RoundRobin(policy) => policy.note_success(addr).await,
            Self::WeightedRoundRobin(policy) => policy.note_success(addr).await,
            Self::RingHash(policy) => policy.note_success(addr).await,
            Self::LeastRequest(policy) => policy.note_success(addr).await,
            Self::RandomSubsetting(policy) => policy.note_success(addr).await,
        }
    }

    /// Record a failed dial on an address. `pick_first` tracks one
    /// sticky target, so it ignores which address failed.
    pub async fn note_failure(
        &self,
        addr: &crate::resolver::ResolvedAddress,
        status: crate::status::Status,
    ) {
        match self {
            Self::PickFirst(policy) => policy.note_failure(status).await,
            Self::RoundRobin(policy) => policy.note_failure(addr, status).await,
            Self::WeightedRoundRobin(policy) => policy.note_failure(addr, status).await,
            Self::RingHash(policy) => policy.note_failure(addr, status).await,
            Self::LeastRequest(policy) => policy.note_failure(addr, status).await,
            Self::RandomSubsetting(policy) => policy.note_failure(addr, status).await,
        }
    }

    /// OOB reporting interval when the policy (or subset child)
    /// enables OOB; `None` means per-call reports.
    pub async fn wants_oob(&self) -> Option<std::time::Duration> {
        match self {
            Self::WeightedRoundRobin(policy) => policy.wants_oob().await,
            Self::RandomSubsetting(policy) => policy.wants_oob().await,
            _ => None,
        }
    }

    /// Claim the OOB pump slot for an address.
    pub async fn note_oob_started(&self, addr: &crate::resolver::ResolvedAddress) -> bool {
        match self {
            Self::WeightedRoundRobin(policy) => policy.note_oob_started(addr).await,
            Self::RandomSubsetting(policy) => policy.note_oob_started(addr).await,
            _ => false,
        }
    }

    /// Release the OOB pump slot for an address.
    pub async fn note_oob_gone(&self, addr: &crate::resolver::ResolvedAddress) {
        match self {
            Self::WeightedRoundRobin(policy) => policy.note_oob_gone(addr).await,
            Self::RandomSubsetting(policy) => policy.note_oob_gone(addr).await,
            _ => {}
        }
    }

    /// Start tracking one unary attempt for least-request counts
    /// (through subset children). Other policies yield an empty
    /// guard that counts nothing.
    pub async fn track_start(&self, addr: &crate::resolver::ResolvedAddress) -> LrTrack {
        match self {
            Self::LeastRequest(policy) => policy.track_start(addr).await,
            Self::RandomSubsetting(policy) => policy.track_start(addr).await,
            _ => LrTrack::empty(),
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
