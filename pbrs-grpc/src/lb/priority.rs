//! priority: failover across named child policies (A56, A115).
//!
//! Each priority owns a lazily created child policy over its own
//! address set. Picks use the highest-priority READY child; with no
//! READY child, a child whose 10s failover timer is still pending
//! holds the picks (it is still trying to connect), else the first
//! CONNECTING child, else the last child (whose error surfaces).
//! Lower priorities deactivate behind the current one and destroy
//! after 15 minutes unless reactivated; children removed from the
//! config are destroyed immediately (A115: no retention cache).
//!
//! Timers are lazy: expirations evaluate on every pick, signal,
//! update, and snapshot against `tokio::time::Instant`, so
//! fake-clock tests observe exact A56 timelines with no background
//! tasks. Flat resolver updates (no hierarchy attributes) feed the
//! highest priority only; per-priority membership arrives via
//! [`Priority::update_priorities`] until resolvers carry hierarchy
//! paths (xDS attributes lane).

use super::{
    HealthSignal, LbPolicy, LbPolicyFactory, LrTrack, Pick, Readiness, is_policy_registered,
    register_lb_policy_factory,
};
use crate::resolver::ResolvedAddress;
use crate::service_config::{PriorityChildConfig, PriorityConfig, ServiceConfig};
use crate::status::Status;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::time::Instant;

/// A56 child connectivity tracking: a child that is still trying to
/// connect holds the picks for 10s before failover moves on.
const FAILOVER_TIMEOUT: Duration = Duration::from_secs(10);
/// A56 child lifetime management: deactivated children destroy after
/// 15 minutes unless reactivated.
const DEACTIVATION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// One priority child: its policy plus A56 lifecycle state.
#[derive(Debug)]
struct PriorityChild {
    policy: LbPolicy,
    /// Last addresses published to the child.
    addresses: Vec<ResolvedAddress>,
    /// Failover timer deadline while the child tries to connect.
    failover_at: Option<Instant>,
    /// Destroy deadline while deactivated.
    deactivate_at: Option<Instant>,
    deactivated: bool,
    /// Last observed readiness, for the CONNECTING-reset rule.
    last: Readiness,
    /// Reported READY more recently than TRANSIENT_FAILURE.
    seen_ready_since_tf: bool,
}

/// Priority failover state shared by a channel's picks.
#[derive(Debug)]
pub struct Priority {
    state: Mutex<State>,
    /// First non-empty `request_hash_header` in the child configs,
    /// for the sync [`Priority::request_hash`] path (which cannot
    /// lock the async state). Plain mutex: cloned, never held across
    /// await. Recomputed by [`Priority::update_config`].
    hash_header: StdMutex<Option<String>>,
    changed: watch::Receiver<u64>,
    bump: watch::Sender<u64>,
}

#[derive(Debug)]
struct State {
    config: PriorityConfig,
    /// Last flat resolver list (remaps to P0 on change).
    flat: Vec<ResolvedAddress>,
    /// Per-child address sets.
    membership: BTreeMap<String, Vec<ResolvedAddress>>,
    /// Reverse index, rebuilt on every republish.
    owner: HashMap<ResolvedAddress, String>,
    children: HashMap<String, PriorityChild>,
    /// Index into `config.priorities`.
    current: Option<usize>,
    last_error: Option<Status>,
}

/// A56 state snapshot for tests and observability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrioritySnapshot {
    /// Currently serving priority name, if any.
    pub current: Option<String>,
    /// Per-priority (name, readiness, deactivated) in slot order.
    /// Slots whose children were never created report no readiness.
    pub slots: Vec<(String, Option<Readiness>, bool)>,
}

impl Priority {
    /// Build from a selected service config, like the other policies.
    pub fn from_config(config: Option<&ServiceConfig>) -> Result<Arc<Self>, Status> {
        let entry = config
            .and_then(|parsed| {
                parsed.lb_policies().iter().find_map(|entry| match entry {
                    crate::service_config::LbPolicyConfig::Priority(config) => Some(config.clone()),
                    _ => None,
                })
            })
            .ok_or_else(|| Status::invalid_argument("priority selected but not configured"))?;
        Self::with_config(&entry)
    }

    /// Build from parsed config, validating that every child lists
    /// at least one registered policy (fail fast; children are still
    /// created lazily).
    pub fn with_config(config: &PriorityConfig) -> Result<Arc<Self>, Status> {
        for (name, child) in &config.children {
            if !child
                .config
                .iter()
                .any(|entry| is_policy_registered(entry.name()))
            {
                return Err(Status::invalid_argument(format!(
                    "priority.children[{name}] lists no registered policy"
                )));
            }
        }
        let hash_header = config
            .children
            .values()
            .flat_map(|child| child.config.iter())
            .find_map(|entry| entry.first_hash_header())
            .map(str::to_owned);
        let (bump, changed) = watch::channel(0);
        Ok(Arc::new(Self {
            hash_header: StdMutex::new(hash_header),
            state: Mutex::new(State {
                config: config.clone(),
                flat: Vec::new(),
                membership: BTreeMap::new(),
                owner: HashMap::new(),
                children: HashMap::new(),
                current: None,
                last_error: None,
            }),
            changed,
            bump,
        }))
    }

    /// Flat resolver update: all addresses feed the highest priority;
    /// every other child goes empty. Explicit per-priority membership
    /// sticks until the flat list changes.
    pub async fn update(&self, addresses: Vec<ResolvedAddress>) {
        let mut state = self.state.lock().await;
        if addresses == state.flat {
            return;
        }
        state.flat = addresses.clone();
        let mut membership = BTreeMap::new();
        for name in state.config.children.keys() {
            membership.insert(name.clone(), Vec::new());
        }
        if let Some(p0) = state.config.priorities.first() {
            membership.insert(p0.clone(), addresses);
        }
        state.membership = membership;
        self.republish(&mut state).await;
        self.choose(&mut state, Instant::now()).await;
    }

    /// Explicit per-priority address sets (xDS lane, tests). Unknown
    /// names are ignored; configured children without an entry go
    /// empty.
    pub async fn update_priorities(&self, membership: BTreeMap<String, Vec<ResolvedAddress>>) {
        let mut state = self.state.lock().await;
        let mut map = BTreeMap::new();
        for name in state.config.children.keys() {
            map.insert(
                name.clone(),
                membership.get(name).cloned().unwrap_or_default(),
            );
        }
        if map == state.membership {
            return;
        }
        state.membership = map;
        self.republish(&mut state).await;
        self.choose(&mut state, Instant::now()).await;
    }

    /// A56 configuration update, applied atomically with one choose
    /// at the end. Children removed from the config are destroyed
    /// immediately (A115); kept children whose policy list changed
    /// are destroyed for lazy recreation. Rejects unknown priority
    /// names and children with no registered policy, keeping the old
    /// config on error.
    pub async fn update_config(&self, config: PriorityConfig) -> Result<(), Status> {
        for (name, child) in &config.children {
            if !child
                .config
                .iter()
                .any(|entry| is_policy_registered(entry.name()))
            {
                return Err(Status::invalid_argument(format!(
                    "priority.children[{name}] lists no registered policy"
                )));
            }
        }
        for name in &config.priorities {
            if !config.children.contains_key(name) {
                return Err(Status::invalid_argument(format!(
                    "priority.priorities references unknown child {name:?}"
                )));
            }
        }
        let mut state = self.state.lock().await;
        state
            .children
            .retain(|name, _| config.children.contains_key(name));
        for (name, child) in &config.children {
            let changed = state
                .config
                .children
                .get(name)
                .is_none_or(|old| format!("{:?}", old.config) != format!("{:?}", child.config));
            if changed {
                state.children.remove(name);
            }
        }
        state.config = config;
        // Owned first: borrows through the guard cannot cross a
        // mutation of another field.
        let keep: Vec<String> = state.config.children.keys().cloned().collect();
        state.membership.retain(|name, _| keep.contains(name));
        for name in keep {
            state.membership.entry(name).or_default();
        }
        let hash_header = state
            .config
            .children
            .values()
            .flat_map(|child| child.config.iter())
            .find_map(|entry| entry.first_hash_header())
            .map(str::to_owned);
        if let Ok(mut guard) = self.hash_header.lock() {
            *guard = hash_header;
        }
        self.republish(&mut state).await;
        self.choose(&mut state, Instant::now()).await;
        self.bump
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        Ok(())
    }

    /// Publish changed address sets to existing children and rebuild
    /// the reverse index. Missing children stay missing (lazy).
    async fn republish(&self, state: &mut State) {
        let mut changed = false;
        for (name, addresses) in &state.membership {
            let Some(child) = state.children.get_mut(name) else {
                continue;
            };
            if child.addresses != *addresses {
                Box::pin(child.policy.update(addresses.clone())).await;
                child.addresses.clone_from(addresses);
                changed = true;
            }
        }
        state.owner.clear();
        for (name, addresses) in &state.membership {
            for addr in addresses {
                state.owner.insert(addr.clone(), name.clone());
            }
        }
        if changed {
            self.bump
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }

    /// A56 algorithm for choosing a priority, evaluated lazily:
    /// expired deactivated children destroy first, then the walk
    /// creates and reactivates as needed. Idempotent for fixed child
    /// states.
    async fn choose(&self, state: &mut State, now: Instant) {
        state.children.retain(|_, child| {
            !(child.deactivated && child.deactivate_at.is_some_and(|at| at <= now))
        });
        if state.config.priorities.is_empty() {
            state.current = None;
            return;
        }
        // Index iteration with split borrows: `observe` takes the maps
        // separately so the hot path clones neither the slot list nor
        // the per-slot observations.
        let len = state.config.priorities.len();
        let mut first_connecting: Option<usize> = None;
        for index in 0..len {
            let observed = {
                let Some(name) = state.config.priorities.get(index) else {
                    continue;
                };
                self.observe(
                    &mut state.children,
                    &state.membership,
                    &state.config.children,
                    name,
                    now,
                )
                .await
            };
            let Some((readiness, failover_pending)) = observed else {
                continue;
            };
            if readiness == Readiness::Ready {
                self.set_current(state, index, true, now);
                return;
            }
            if failover_pending {
                self.set_current(state, index, false, now);
                return;
            }
            if readiness == Readiness::Connecting && first_connecting.is_none() {
                first_connecting = Some(index);
            }
        }
        if let Some(index) = first_connecting {
            self.set_current(state, index, false, now);
            return;
        }
        self.set_current(state, len - 1, false, now);
    }

    /// Create-or-get a child, reactivate it, publish pending address
    /// changes, and observe its readiness, applying the A56 timer
    /// rules. Returns the readiness plus whether its failover timer
    /// is pending, or `None` when creation cannot proceed (only when
    /// registration changed underfoot; configs are validated). The
    /// state maps arrive split so callers can borrow the slot name
    /// without cloning it.
    async fn observe(
        &self,
        children: &mut HashMap<String, PriorityChild>,
        membership: &BTreeMap<String, Vec<ResolvedAddress>>,
        child_configs: &BTreeMap<String, PriorityChildConfig>,
        name: &str,
        now: Instant,
    ) -> Option<(Readiness, bool)> {
        if !children.contains_key(name) {
            let entry = child_configs.get(name).and_then(|child| {
                child
                    .config
                    .iter()
                    .find(|entry| is_policy_registered(entry.name()))
            })?;
            let policy = super::instantiate(entry).ok()?;
            let addresses = membership.get(name).cloned().unwrap_or_default();
            Box::pin(policy.update(addresses.clone())).await;
            children.insert(
                name.to_owned(),
                PriorityChild {
                    policy,
                    addresses,
                    failover_at: Some(now + FAILOVER_TIMEOUT),
                    deactivate_at: None,
                    deactivated: false,
                    last: Readiness::TransientFailure,
                    seen_ready_since_tf: false,
                },
            );
        }
        let child = children.get_mut(name)?;
        child.deactivated = false;
        child.deactivate_at = None;
        if let Some(addresses) = membership.get(name) {
            if child.addresses != *addresses {
                Box::pin(child.policy.update(addresses.clone())).await;
                child.addresses.clone_from(addresses);
            }
        }
        let readiness = child.policy.readiness_direct().await;
        match readiness {
            Readiness::Ready => {
                child.failover_at = None;
                child.seen_ready_since_tf = true;
            }
            Readiness::TransientFailure => {
                child.failover_at = None;
                child.seen_ready_since_tf = false;
            }
            Readiness::Connecting => {
                if child.last != Readiness::Connecting && child.seen_ready_since_tf {
                    child.failover_at = Some(now + FAILOVER_TIMEOUT);
                }
            }
        }
        child.last = readiness;
        let pending = child.failover_at.is_some_and(|at| at > now);
        Some((readiness, pending))
    }

    /// A56 SetCurrentPriority: deactivate lower slots, record the new
    /// current, and wake waiters on change.
    fn set_current(&self, state: &mut State, index: usize, deactivate_lower: bool, now: Instant) {
        if deactivate_lower {
            for slot in index + 1..state.config.priorities.len() {
                // Split borrows: the name borrows the config while the
                // child map mutates, so no per-pick clone.
                let Some(name) = state.config.priorities.get(slot) else {
                    continue;
                };
                if let Some(child) = state.children.get_mut(name) {
                    child.deactivated = true;
                    child.deactivate_at = Some(now + DEACTIVATION_TIMEOUT);
                }
            }
        }
        if state.current != Some(index) {
            state.current = Some(index);
            self.bump
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }

    /// Owning child policy for an address, if it exists.
    async fn route(&self, addr: &ResolvedAddress) -> Option<LbPolicy> {
        let state = self.state.lock().await;
        let name = state.owner.get(addr)?;
        state.children.get(name).map(|child| child.policy.clone())
    }

    /// Pick through the chosen child (A56 delegates the picker).
    pub async fn pick(&self) -> Pick {
        self.pick_hash(None).await
    }

    /// Hashed pick through the chosen child (ring children hash).
    pub async fn pick_hash(&self, hash: Option<u64>) -> Pick {
        let child = {
            let mut state = self.state.lock().await;
            self.choose(&mut state, Instant::now()).await;
            let Some(index) = state.current else {
                return Pick::Fail(Status::unavailable(
                    "priority policy has empty priority list",
                ));
            };
            let Some(name) = state.config.priorities.get(index) else {
                return Pick::Fail(Status::unavailable(
                    "priority policy has empty priority list",
                ));
            };
            state.children.get(name).map(|child| child.policy.clone())
        };
        let Some(child) = child else {
            return Pick::Fail(Status::unavailable(
                "priority policy has empty priority list",
            ));
        };
        child.pick_hash_direct(hash).await
    }

    /// Record a successful dial through the owning child, then
    /// re-choose (failback when a higher priority recovers).
    pub async fn note_success(&self, addr: &ResolvedAddress) {
        let child = self.route(addr).await;
        if let Some(child) = child {
            Box::pin(child.note_success(addr)).await;
            let mut state = self.state.lock().await;
            self.choose(&mut state, Instant::now()).await;
        }
    }

    /// Record a failed dial through the owning child, then re-choose
    /// (failover when the current priority fails).
    pub async fn note_failure(&self, addr: &ResolvedAddress, status: Status) {
        {
            let mut state = self.state.lock().await;
            state.last_error = Some(status.clone());
        }
        let child = self.route(addr).await;
        if let Some(child) = child {
            Box::pin(child.note_failure(addr, status)).await;
            let mut state = self.state.lock().await;
            self.choose(&mut state, Instant::now()).await;
        }
    }

    /// Health Watch started: forward, then re-choose.
    pub async fn note_health_pending(&self, addr: &ResolvedAddress) -> bool {
        let child = self.route(addr).await;
        let Some(child) = child else { return false };
        let marked = Box::pin(child.note_health_pending(addr)).await;
        let mut state = self.state.lock().await;
        self.choose(&mut state, Instant::now()).await;
        marked
    }

    /// Health Watch ended: forward, then re-choose.
    pub async fn note_health_gone(&self, addr: &ResolvedAddress) {
        let child = self.route(addr).await;
        if let Some(child) = child {
            Box::pin(child.note_health_gone(addr)).await;
            let mut state = self.state.lock().await;
            self.choose(&mut state, Instant::now()).await;
        }
    }

    /// Health report: forward, then re-choose.
    pub async fn note_health(&self, addr: &ResolvedAddress, signal: HealthSignal) {
        let child = self.route(addr).await;
        if let Some(child) = child {
            Box::pin(child.note_health(addr, signal)).await;
            let mut state = self.state.lock().await;
            self.choose(&mut state, Instant::now()).await;
        }
    }

    /// Latest health signal through the owning child.
    pub async fn health_of(&self, addr: &ResolvedAddress) -> Option<HealthSignal> {
        let child = self.route(addr).await?;
        Box::pin(child.health_of(addr)).await
    }

    /// ORCA report through the owning child (weights never affect
    /// readiness, so no re-choose).
    pub async fn note_orca_report(
        &self,
        addr: &ResolvedAddress,
        report: &crate::orca::OrcaLoadReport,
        oob: bool,
    ) {
        if let Some(child) = self.route(addr).await {
            Box::pin(child.note_orca_report(addr, report, oob)).await;
        }
    }

    /// Movement notifications: current changes and republishes.
    /// Child-internal movement (backoff expiry) surfaces on the next
    /// pick; the pool re-polls.
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.clone()
    }

    /// OOB interval of the current child, if any.
    pub async fn wants_oob(&self) -> Option<Duration> {
        let child = {
            let state = self.state.lock().await;
            let index = state.current?;
            let name = state.config.priorities.get(index)?;
            state.children.get(name).map(|child| child.policy.clone())
        };
        let child = child?;
        // Boxed: the enum dispatches back here (E0733).
        Box::pin(child.wants_oob()).await
    }

    /// Claim the OOB pump slot through the owning child.
    pub async fn note_oob_started(&self, addr: &ResolvedAddress) -> bool {
        let child = self.route(addr).await;
        let Some(child) = child else { return false };
        Box::pin(child.note_oob_started(addr)).await
    }

    /// Release the OOB pump slot through the owning child.
    pub async fn note_oob_gone(&self, addr: &ResolvedAddress) {
        if let Some(child) = self.route(addr).await {
            Box::pin(child.note_oob_gone(addr)).await;
        }
    }

    /// Request hash from the first configured hash header in the
    /// child configs (over-hashing is harmless: non-ring children
    /// ignore the value, while ring children need it). `None` when
    /// no child names a header.
    pub fn request_hash(&self, md: &crate::Metadata) -> Option<u64> {
        let header = self.hash_header.lock().ok()?.clone()?;
        super::ring_hash::hash_metadata_header(&header, md)
    }

    /// Least-request tracking through the owning child.
    pub async fn track_start(&self, addr: &ResolvedAddress) -> LrTrack {
        let child = self.route(addr).await;
        let Some(child) = child else {
            return LrTrack::empty();
        };
        Box::pin(child.track_start(addr)).await
    }

    /// Completed-call outcome through the owning child (A50).
    /// Never blocks: a contended lock drops the sample. No
    /// re-choose: outcomes never affect readiness.
    pub fn note_call_status(&self, addr: &ResolvedAddress, status: &Status) {
        let child = {
            let Ok(state) = self.state.try_lock() else {
                return;
            };
            state
                .owner
                .get(addr)
                .and_then(|name| state.children.get(name))
                .map(|child| child.policy.clone())
        };
        if let Some(child) = child {
            child.note_call_status(addr, status);
        }
    }

    /// Current child's readiness (for nesting), else TRANSIENT_FAILURE.
    pub(crate) async fn readiness(&self) -> Readiness {
        let child = {
            let state = self.state.lock().await;
            let Some(index) = state.current else {
                return Readiness::TransientFailure;
            };
            let Some(name) = state.config.priorities.get(index) else {
                return Readiness::TransientFailure;
            };
            state.children.get(name).map(|child| child.policy.clone())
        };
        let Some(child) = child else {
            return Readiness::TransientFailure;
        };
        child.readiness_direct().await
    }

    /// A56 state snapshot, timers evaluated first.
    pub async fn priority_snapshot(&self) -> PrioritySnapshot {
        let mut state = self.state.lock().await;
        self.choose(&mut state, Instant::now()).await;
        let mut slots = Vec::with_capacity(state.config.priorities.len());
        for name in &state.config.priorities {
            match state.children.get(name) {
                Some(child) => {
                    let readiness = Box::pin(child.policy.readiness()).await;
                    slots.push((name.clone(), Some(readiness), child.deactivated));
                }
                None => slots.push((name.clone(), None, false)),
            }
        }
        let current = state
            .current
            .and_then(|index| state.config.priorities.get(index).cloned());
        PrioritySnapshot { current, slots }
    }
}

/// Factory for the `priority` policy name.
#[derive(Debug, Default)]
pub struct PriorityFactory;

impl LbPolicyFactory for PriorityFactory {
    fn name(&self) -> &str {
        "priority"
    }
}

/// Register the `priority` policy name once.
pub(crate) fn ensure_registered() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        register_lb_policy_factory(Arc::new(PriorityFactory));
    });
}

#[cfg(test)]
mod tests {
    use super::Priority;
    use crate::resolver::ResolvedAddress;
    use crate::service_config::{LbPolicyConfig, PriorityConfig, ServiceConfig};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn tcp(n: u8) -> ResolvedAddress {
        ResolvedAddress::Tcp(format!("10.0.0.{n}:80").parse().expect("addr"))
    }

    fn register_children() {
        super::super::ensure_round_robin_registered();
        super::super::ensure_ring_hash_registered();
    }

    fn two_priorities() -> ServiceConfig {
        ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"priority": {
                "children": {
                    "p0": {"config": [{"round_robin": {}}]},
                    "p1": {"config": [{"round_robin": {}}]}
                },
                "priorities": ["p0", "p1"]}}]}"#,
        )
        .expect("parses")
    }

    fn membership(
        p0: Vec<ResolvedAddress>,
        p1: Vec<ResolvedAddress>,
    ) -> BTreeMap<String, Vec<ResolvedAddress>> {
        BTreeMap::from([("p0".to_owned(), p0), ("p1".to_owned(), p1)])
    }

    fn priority_entry(doc: &ServiceConfig) -> PriorityConfig {
        doc.lb_policies()
            .iter()
            .find_map(|entry| match entry {
                LbPolicyConfig::Priority(config) => Some(config.clone()),
                _ => None,
            })
            .expect("priority entry")
    }

    #[tokio::test]
    async fn serves_highest_priority_first() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(1)),
            "p0 serves first, got {pick:?}"
        );
        let snapshot = policy.priority_snapshot().await;
        assert_eq!(snapshot.current.as_deref(), Some("p0"));
    }

    #[tokio::test(start_paused = true)]
    async fn fails_over_and_back() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        policy
            .note_failure(&tcp(1), crate::status::Status::unavailable("p0 down"))
            .await;
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(2)),
            "p0 down fails over to p1, got {pick:?}"
        );
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p1")
        );
        // Past the round_robin backoff (~1s): p0 recovers and takes over.
        tokio::time::advance(Duration::from_secs(5)).await;
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(1)),
            "p0 recovery fails back, got {pick:?}"
        );
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p0")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn connecting_child_holds_for_failover_timeout() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        // First pick creates p0 (Ready); the pending Watch flips it to
        // Connecting, which resets the 10s failover timer (A56).
        let _ = policy.pick().await;
        assert!(policy.note_health_pending(&tcp(1)).await);
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p0")
        );
        let pick = policy.pick().await;
        assert!(
            !matches!(pick, super::Pick::Use(ref addr) if *addr == tcp(2)),
            "connecting p0 holds the picks, got {pick:?}"
        );
        tokio::time::advance(Duration::from_secs(11)).await;
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(2)),
            "failover fires after 10s of connecting, got {pick:?}"
        );
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p1")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn deactivated_child_destroys_after_timeout() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        policy
            .note_failure(&tcp(1), crate::status::Status::unavailable("p0 down"))
            .await;
        let _ = policy.pick().await;
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p1")
        );
        // p0 recovers: current moves back up and p1 deactivates.
        tokio::time::advance(Duration::from_secs(5)).await;
        let _ = policy.pick().await;
        let snapshot = policy.priority_snapshot().await;
        assert_eq!(snapshot.current.as_deref(), Some("p0"));
        assert_eq!(
            snapshot.slots,
            vec![
                ("p0".to_owned(), Some(super::Readiness::Ready), false),
                ("p1".to_owned(), Some(super::Readiness::Ready), true),
            ]
        );
        // Past the 15-minute deactivation timeout p1 is destroyed.
        tokio::time::advance(Duration::from_secs(16 * 60)).await;
        let _ = policy.pick().await;
        let snapshot = policy.priority_snapshot().await;
        assert_eq!(snapshot.current.as_deref(), Some("p0"));
        assert_eq!(
            snapshot.slots,
            vec![
                ("p0".to_owned(), Some(super::Readiness::Ready), false),
                ("p1".to_owned(), None, false),
            ]
        );
    }

    #[tokio::test]
    async fn config_update_removes_child_immediately() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        let _ = policy.pick().await;
        let lone = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"priority": {
                "children": {"p0": {"config": [{"round_robin": {}}]}},
                "priorities": ["p0"]}}]}"#,
        )
        .expect("parses");
        policy
            .update_config(priority_entry(&lone))
            .await
            .expect("valid");
        let snapshot = policy.priority_snapshot().await;
        assert_eq!(snapshot.current.as_deref(), Some("p0"));
        assert_eq!(snapshot.slots.len(), 1);
        let pick = policy.pick().await;
        assert!(matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(1)));
    }

    #[tokio::test]
    async fn invalid_config_update_keeps_old_config() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        let bad = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"priority": {
                "children": {"p0": {"config": [{"round_robin": {}}]}},
                "priorities": ["p0", "ghost"]}}]}"#,
        );
        // Parse itself rejects the unknown priority name; build the
        // same shape by hand to exercise the runtime check.
        assert!(bad.is_err());
        let mut entry = priority_entry(&two_priorities());
        entry.priorities.push("ghost".to_owned());
        let result = policy.update_config(entry).await;
        assert!(result.is_err());
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(1)),
            "old config still serves, got {pick:?}"
        );
    }

    #[tokio::test]
    async fn empty_priorities_fail_picks() {
        register_children();
        let policy = Priority::with_config(&PriorityConfig::default()).expect("builds");
        policy.update(vec![tcp(1)]).await;
        let pick = policy.pick().await;
        assert!(matches!(pick, super::Pick::Fail(_)), "got {pick:?}");
        assert_eq!(policy.priority_snapshot().await.current, None);
    }

    #[tokio::test]
    async fn child_without_registered_policy_is_rejected() {
        let mut children = BTreeMap::new();
        children.insert(
            "p0".to_owned(),
            crate::service_config::PriorityChildConfig {
                config: vec![LbPolicyConfig::Unknown("lb_unit_no_such_policy".to_owned())],
                ignore_reresolution_requests: false,
            },
        );
        let config = PriorityConfig {
            children,
            priorities: vec!["p0".to_owned()],
        };
        assert!(Priority::with_config(&config).is_err());
    }

    #[tokio::test]
    async fn flat_update_feeds_highest_priority_only() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy.update(vec![tcp(1), tcp(2)]).await;
        // Both addresses land in p0; p1 stays empty.
        let pick = policy.pick().await;
        assert!(
            matches!(pick, super::Pick::Use(ref addr) if addr == &tcp(1) || addr == &tcp(2)),
            "flat list serves from p0, got {pick:?}"
        );
        policy
            .note_failure(&tcp(1), crate::status::Status::unavailable("down"))
            .await;
        policy
            .note_failure(&tcp(2), crate::status::Status::unavailable("down"))
            .await;
        let _ = policy.pick().await;
        // p0 is down and p1 is empty: current moves to the last child
        // and picks surface its error.
        assert_eq!(
            policy.priority_snapshot().await.current.as_deref(),
            Some("p1")
        );
        let pick = policy.pick().await;
        assert!(matches!(pick, super::Pick::Fail(_)), "got {pick:?}");
    }

    #[tokio::test]
    async fn request_hash_uses_first_configured_header() {
        register_children();
        let doc = ServiceConfig::parse(
            r#"{"loadBalancingConfig": [{"priority": {
                "children": {
                    "p0": {"config": [{"ring_hash": {"requestHashHeader": "x-rou-ting"}}]},
                    "p1": {"config": [{"round_robin": {}}]}
                },
                "priorities": ["p0", "p1"]}}]}"#,
        )
        .expect("parses");
        let policy = Priority::from_config(Some(&doc)).expect("builds");
        let mut md = crate::Metadata::new();
        md.insert("x-rou-ting", "session-7").expect("header");
        let first = policy.request_hash(&md);
        assert!(first.is_some());
        assert_eq!(policy.request_hash(&md), first);
        let bare = crate::Metadata::new();
        assert!(
            policy.request_hash(&bare).is_some(),
            "absent header hashes randomly (A76)"
        );
        let no_ring = Priority::from_config(Some(&two_priorities())).expect("builds");
        assert_eq!(no_ring.request_hash(&md), None);
    }

    #[tokio::test]
    async fn health_reports_forward_to_owning_child() {
        register_children();
        let policy = Priority::from_config(Some(&two_priorities())).expect("builds");
        policy
            .update_priorities(membership(vec![tcp(1)], vec![tcp(2)]))
            .await;
        let _ = policy.pick().await;
        policy
            .note_health(&tcp(1), super::HealthSignal::Unhealthy)
            .await;
        assert_eq!(
            policy.health_of(&tcp(1)).await,
            Some(super::HealthSignal::Unhealthy)
        );
        // Unknown addresses route nowhere and report nothing.
        assert_eq!(policy.health_of(&tcp(9)).await, None);
    }
}
