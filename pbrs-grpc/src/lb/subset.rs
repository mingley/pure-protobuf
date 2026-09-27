//! random_subsetting: rendezvous subsetting over a child policy (A68).
//!
//! Each resolver update keeps the `subset_size` addresses with the
//! lowest seeded-XXH64 of their address text (grpc-go's rendezvous
//! construction; stable across updates that only reorder or grow
//! the list) and runs a child policy over the subset. The child is
//! the first registered entry of the `childPolicy` list (A24); all
//! picks, health, failure, ORCA, OOB, and load-tracking calls
//! delegate to it, so subsetting composes with every runtime
//! policy. Nested subsetting is rejected at parse. Without xDS the
//! child list is the config's own; xDS supplies it per cluster.

use super::{HealthSignal, LbPolicy, LbPolicyFactory, Pick, Readiness, register_lb_policy_factory};
use crate::resolver::ResolvedAddress;
use crate::service_config::RandomSubsettingConfig;
use crate::status::Status;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, watch};

/// random_subsetting policy state shared by a channel's picks.
#[derive(Debug)]
pub struct RandomSubsetting {
    subset_size: u64,
    seed: u64,
    child: LbPolicy,
    state: Mutex<State>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    /// Full resolver list; the subset derives from it.
    addresses: Vec<ResolvedAddress>,
    /// Last published subset, for tests and observability.
    subset: Vec<ResolvedAddress>,
}

impl RandomSubsetting {
    /// Build from a service-config document: the
    /// `random_subsetting_experimental` entry's config when selected.
    /// Fails when the child list selects no registered policy.
    pub fn from_config(
        config: Option<&crate::service_config::ServiceConfig>,
    ) -> Result<Arc<Self>, Status> {
        let entry = config.and_then(|doc| {
            doc.lb_policies().iter().find_map(|entry| match entry {
                crate::service_config::LbPolicyConfig::RandomSubsetting(config) => {
                    Some(config.clone())
                }
                _ => None,
            })
        });
        let Some(entry) = entry else {
            return Err(Status::invalid_argument(
                "random_subsetting_experimental selected but not configured",
            ));
        };
        Self::with_config(&entry)
    }

    /// Build from an explicit config (tests and direct construction).
    /// Fails when the child list selects no registered policy.
    pub fn with_config(config: &RandomSubsettingConfig) -> Result<Arc<Self>, Status> {
        let child_entry = config
            .child_policy
            .iter()
            .find(|entry| super::is_policy_registered(entry.name()))
            .ok_or_else(|| {
                Status::invalid_argument(
                    "random_subsetting_experimental.childPolicy lists no registered policy",
                )
            })?;
        let child = super::instantiate(child_entry)?;
        let seed = super::SplitMix64::seed().next();
        Self::with_seed(config.subset_size, seed, child)
    }

    /// Build with an explicit seed (tests pin the subset).
    pub(crate) fn with_seed(
        subset_size: u64,
        seed: u64,
        child: LbPolicy,
    ) -> Result<Arc<Self>, Status> {
        if subset_size == 0 {
            return Err(Status::invalid_argument(
                "random_subsetting_experimental.subsetSize must be positive",
            ));
        }
        let (bump, changed) = watch::channel(0);
        Ok(Arc::new(Self {
            subset_size,
            seed,
            child,
            state: Mutex::new(State {
                addresses: Vec::new(),
                subset: Vec::new(),
            }),
            changed,
            bump,
        }))
    }

    /// Last published subset, in rendezvous order.
    pub async fn subset_snapshot(&self) -> Vec<ResolvedAddress> {
        self.state.lock().await.subset.clone()
    }

    /// Reconcile a new address list: recompute the rendezvous subset
    /// and publish it to the child. Wakes [`Self::watch`] when the
    /// full list or the subset changes.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let subset = rendezvous_subset(&addresses, self.subset_size, self.seed);
        let mut state = self.state.lock().await;
        if addresses == state.addresses && subset == state.subset {
            return;
        }
        state.addresses = addresses;
        state.subset = subset.clone();
        // Boxed: child calls recurse through the LbPolicy enum at the
        // type level (nesting is parse-rejected, so never at runtime),
        // and async recursion needs the indirection. Same below.
        Box::pin(self.child.update(subset)).await;
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Pick through the child policy.
    pub async fn pick(&self) -> Pick {
        Box::pin(self.child.pick()).await
    }

    /// Hashed pick through the child policy (ring children hash).
    pub async fn pick_hash(&self, hash: Option<u64>) -> Pick {
        Box::pin(self.child.pick_hash(hash)).await
    }

    /// Record a successful dial through the child policy.
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_success(addr)).await;
    }

    /// Record a Watch starting through the child policy.
    /// Returns whether this call newly marked the address.
    pub async fn note_health_pending(&self, addr: &ResolvedAddress) -> bool {
        Box::pin(self.child.note_health_pending(addr)).await
    }

    /// Clear a Watch record without reporting, through the child.
    pub async fn note_health_gone(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_health_gone(addr)).await;
    }

    /// Record a Watch report through the child policy.
    pub async fn note_health(&self, addr: &ResolvedAddress, signal: HealthSignal) {
        Box::pin(self.child.note_health(addr, signal)).await;
    }

    /// Last reported health, through the child policy.
    pub async fn health_of(&self, addr: &ResolvedAddress) -> Option<HealthSignal> {
        Box::pin(self.child.health_of(addr)).await
    }

    /// Record a failed dial through the child policy. Wakes waiters.
    pub async fn note_failure(&self, addr: &ResolvedAddress, status: Status) {
        Box::pin(self.child.note_failure(addr, status)).await;
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
    }

    /// Fold one ORCA report through the child policy.
    pub async fn note_orca_report(
        &self,
        addr: &ResolvedAddress,
        report: &crate::orca::OrcaLoadReport,
        oob: bool,
    ) {
        Box::pin(self.child.note_orca_report(addr, report, oob)).await;
    }

    /// OOB interval through the child policy, if it wants OOB.
    pub async fn wants_oob(&self) -> Option<std::time::Duration> {
        Box::pin(self.child.wants_oob()).await
    }

    /// Claim the OOB pump slot through the child policy.
    pub async fn note_oob_started(&self, addr: &ResolvedAddress) -> bool {
        Box::pin(self.child.note_oob_started(addr)).await
    }

    /// Release the OOB pump slot through the child policy.
    pub async fn note_oob_gone(&self, addr: &ResolvedAddress) {
        Box::pin(self.child.note_oob_gone(addr)).await;
    }

    /// Start tracking one unary attempt through the child policy.
    pub async fn track_start(&self, addr: &ResolvedAddress) -> super::LrTrack {
        Box::pin(self.child.track_start(addr)).await
    }

    /// Request hash through the child policy, if it hashes.
    pub fn request_hash(&self, md: &crate::Metadata) -> Option<u64> {
        self.child.request_hash(md)
    }

    /// Movement notifications: subset updates and failures. The
    /// child bumps its own watchers too; waiters hold both.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.clone()
    }

    /// Completed-call outcome through the child (A50).
    pub fn note_call_status(&self, addr: &ResolvedAddress, status: &crate::status::Status) {
        self.child.note_call_status(addr, status);
    }

    /// Child readiness (for nesting): the child sees only the
    /// subset, so its observation is already subset-scoped.
    pub(crate) async fn readiness(&self) -> Readiness {
        // Boxed: the enum dispatches back here (E0733).
        Box::pin(self.child.readiness()).await
    }
}

/// Rendezvous subset like grpc-go: hash each address text with the
/// policy seed, sort by hash, keep the first `size` (all of them
/// when the list is shorter).
pub(crate) fn rendezvous_subset(
    addresses: &[ResolvedAddress],
    size: u64,
    seed: u64,
) -> Vec<ResolvedAddress> {
    let mut hashed: Vec<(u64, &ResolvedAddress)> = addresses
        .iter()
        .map(|addr| {
            let key = match addr {
                ResolvedAddress::Tcp(sock) => sock.to_string(),
                ResolvedAddress::Unix(path) => path.display().to_string(),
                ResolvedAddress::UnixAbstract(name) => {
                    format!("@{}", String::from_utf8_lossy(name))
                }
            };
            (super::xxh64(key.as_bytes(), seed), addr)
        })
        .collect();
    hashed.sort_by_key(|a| a.0);
    let keep = usize::try_from(size)
        .unwrap_or(usize::MAX)
        .min(hashed.len());
    hashed
        .into_iter()
        .take(keep)
        .map(|(_, addr)| addr.clone())
        .collect()
}

/// Factory registering `random_subsetting_experimental` for A24 selection.
#[derive(Debug, Default)]
pub struct RandomSubsettingFactory;

impl LbPolicyFactory for RandomSubsettingFactory {
    fn name(&self) -> &str {
        "random_subsetting_experimental"
    }
}

/// Register `random_subsetting_experimental`, once. Called on the
/// resolver-channel path so selected configs need no user setup.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(RandomSubsettingFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::{RandomSubsetting, rendezvous_subset};
    use crate::lb::{LbPolicy, RoundRobin};
    use crate::resolver::ResolvedAddress;
    use std::sync::Arc;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    fn subset_rr(size: u64, seed: u64) -> Arc<RandomSubsetting> {
        RandomSubsetting::with_seed(size, seed, LbPolicy::RoundRobin(RoundRobin::new()))
            .expect("subset")
    }

    #[test]
    fn subset_is_stable_and_bounded() {
        let addrs: Vec<ResolvedAddress> = (1..=8).map(tcp).collect();
        let subset = rendezvous_subset(&addrs, 3, 42);
        assert_eq!(subset.len(), 3);
        // Reordered input selects the same members.
        let mut rev = addrs.clone();
        rev.reverse();
        let mut a = subset.clone();
        let mut b = rendezvous_subset(&rev, 3, 42);
        a.sort_by(|x, y| format!("{x:?}").cmp(&format!("{y:?}")));
        b.sort_by(|x, y| format!("{x:?}").cmp(&format!("{y:?}")));
        assert_eq!(a, b);
        // Growing the list keeps most members (rendezvous stability):
        // only the disjoint range can change.
        let mut grown = addrs.clone();
        grown.push(tcp(9));
        let grown_subset = rendezvous_subset(&grown, 3, 42);
        let kept = subset.iter().filter(|a| grown_subset.contains(a)).count();
        assert!(kept >= 2, "kept {kept}/3 after growth");
        // Oversized subsets keep everything.
        assert_eq!(rendezvous_subset(&addrs, 100, 42).len(), 8);
    }

    #[tokio::test]
    async fn picks_stay_inside_subset() {
        let subset = subset_rr(2, 7);
        let addrs: Vec<ResolvedAddress> = (1..=6).map(tcp).collect();
        subset.update(addrs).await;
        let members = subset.subset_snapshot().await;
        assert_eq!(members.len(), 2);
        for _ in 0..20 {
            match subset.pick().await {
                super::Pick::Use(addr) => assert!(members.contains(&addr)),
                other => panic!("unexpected pick: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn child_config_selects_first_registered() {
        use crate::service_config::{LbPolicyConfig, RandomSubsettingConfig};
        // Unknown names are skipped by is_policy_registered, so RR wins.
        let config = RandomSubsettingConfig {
            subset_size: 2,
            child_policy: vec![
                LbPolicyConfig::Unknown("no_such_policy".to_owned()),
                LbPolicyConfig::RoundRobin,
            ],
        };
        // RoundRobin is registered by other tests' ensure calls... but
        // registration is global and order-dependent, so register here.
        super::super::register_lb_policy_factory(Arc::new(crate::lb::RoundRobinFactory));
        let subset = RandomSubsetting::with_config(&config).expect("child RR");
        subset.update(vec![tcp(1), tcp(2), tcp(3)]).await;
        assert_eq!(subset.subset_snapshot().await.len(), 2);
    }
}
