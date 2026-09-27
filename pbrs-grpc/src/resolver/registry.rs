//! Scheme registry and resolver configuration.
//!
//! Factories register by scheme name at initialization
//! ([`register_resolver_factory`]); [`resolver_for`] builds the
//! resolver for a parsed target. Built-in schemes (`dns`,
//! `passthrough`, `ipv4`, `ipv6`, `unix`, `unix-abstract`) register on
//! first use.

#![allow(
    clippy::disallowed_types,
    reason = "init-time factory registry; reads happen at resolver build, never across await"
)]

use super::dns::{DnsConfig, DnsLookup};
use super::snapshot::Resolution;
use super::target::ParsedTarget;
use crate::status::Status;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;

/// Builds the resolver for one scheme.
pub trait ResolverFactory: Send + Sync + 'static {
    /// Scheme name this factory builds (`dns`, `passthrough`, …).
    fn scheme(&self) -> &str;

    /// Resolve the initial snapshot and start refreshing. The initial
    /// lookup runs now, so an unresolvable target fails here instead of
    /// failing the first RPC with a worse error.
    fn build(
        &self,
        target: &ParsedTarget,
        config: &ResolverConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<BuiltResolver, Status>> + Send + '_>,
    >;
}

/// Configuration shared by every resolver scheme.
#[derive(Clone, Default)]
pub struct ResolverConfig {
    dns: Option<DnsConfig>,
    dns_provider: Option<Arc<dyn DnsLookup>>,
}

impl core::fmt::Debug for ResolverConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResolverConfig")
            .field("dns", &self.dns)
            .field("dns_provider", &self.dns_provider.is_some())
            .finish()
    }
}

impl ResolverConfig {
    /// Static schemes only (`passthrough:`, `ipv4:`, `ipv6:`, `unix:`,
    /// `unix-abstract:`). `dns:` targets are rejected: DNS needs
    /// explicit refresh bounds, and no numerical default is approved.
    #[must_use]
    pub fn static_only() -> Self {
        Self::default()
    }

    /// Full configuration with explicit DNS bounds.
    #[must_use]
    pub fn with_dns(dns: DnsConfig) -> Self {
        Self {
            dns: Some(dns),
            dns_provider: None,
        }
    }

    /// Full configuration with an injected DNS provider (tests and
    /// custom DNS stacks; production uses [`SystemDns`](super::SystemDns)).
    #[must_use]
    pub fn with_dns_provider(dns: DnsConfig, provider: Arc<dyn DnsLookup>) -> Self {
        Self {
            dns: Some(dns),
            dns_provider: Some(provider),
        }
    }

    /// DNS bounds, or `None` for [`Self::static_only`].
    #[must_use]
    pub fn dns(&self) -> Option<&DnsConfig> {
        self.dns.as_ref()
    }

    /// Injected DNS provider, if any.
    #[must_use]
    pub fn dns_provider(&self) -> Option<&Arc<dyn DnsLookup>> {
        self.dns_provider.as_ref()
    }
}

/// A built resolver: the initial snapshot, the update stream, and the
/// refresh task's lifetime. Dropping the last owner stops refreshing.
pub struct BuiltResolver {
    /// First snapshot; empty only when the lookup authoritatively is.
    /// Construction fails rather than serving a failure as empty.
    pub initial: Arc<Resolution>,
    /// Latest-value update stream. Receivers `borrow()` the current
    /// `Arc` without awaiting, so updates never block picks.
    pub watch: watch::Receiver<Arc<Resolution>>,
    task: Option<ResolverTask>,
}

impl core::fmt::Debug for BuiltResolver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BuiltResolver")
            .field("initial", &self.initial)
            .field("refreshing", &self.task.is_some())
            .finish()
    }
}

impl BuiltResolver {
    /// A resolver with no refresh task (static schemes).
    #[must_use]
    pub fn once(initial: Arc<Resolution>) -> Self {
        let (_, watch) = watch::channel(Arc::clone(&initial));
        Self {
            initial,
            watch,
            task: None,
        }
    }

    /// A resolver whose `task` publishes into `watch`.
    #[must_use]
    pub fn refreshing(
        initial: Arc<Resolution>,
        watch: watch::Receiver<Arc<Resolution>>,
        task: ResolverTask,
    ) -> Self {
        Self {
            initial,
            watch,
            task: Some(task),
        }
    }

    /// Split off the parts a channel holds: the update stream plus the
    /// task guard that keeps refreshing alive.
    #[must_use]
    pub fn into_handle(self) -> ResolverHandle {
        ResolverHandle {
            watch: self.watch,
            task: self.task,
        }
    }
}

/// The channel-held half of a built resolver.
#[derive(Debug)]
pub struct ResolverHandle {
    /// Update stream for dial-time snapshots (CH-02) and subchannels (CH-03+).
    pub watch: watch::Receiver<Arc<Resolution>>,
    #[allow(dead_code, reason = "task guard: dropping the handle aborts refresh")]
    task: Option<ResolverTask>,
}

/// A refresh task that aborts when its last owner drops, so no lookup
/// or timer outlives the channel (contract t7).
#[derive(Debug)]
pub struct ResolverTask {
    handle: tokio::task::JoinHandle<()>,
}

impl ResolverTask {
    /// Guard `handle`.
    #[must_use]
    pub fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self { handle }
    }
}

impl Drop for ResolverTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[allow(
    clippy::disallowed_types,
    reason = "init-time factory registry; reads happen at resolver build, never across await"
)]
fn registry() -> &'static Mutex<HashMap<String, Arc<dyn ResolverFactory>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<dyn ResolverFactory>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map: HashMap<String, Arc<dyn ResolverFactory>> = HashMap::new();
        for factory in super::dns::factories()
            .into_iter()
            .chain(super::fixed::factories())
        {
            map.insert(factory.scheme().to_owned(), factory);
        }
        Mutex::new(map)
    })
}

/// Register a resolver factory for [`ResolverFactory::scheme`]. Call
/// during initialization, before connecting; re-registering a scheme
/// replaces its factory.
pub fn register_resolver_factory(factory: Arc<dyn ResolverFactory>) {
    if let Ok(mut guard) = registry().lock() {
        guard.insert(factory.scheme().to_owned(), factory);
    }
}

/// Build the resolver for `target` from the scheme registry.
pub async fn resolver_for(
    target: &ParsedTarget,
    config: &ResolverConfig,
) -> Result<BuiltResolver, Status> {
    let factory = registry()
        .lock()
        .ok()
        .and_then(|guard| guard.get(&target.scheme).cloned());
    match factory {
        Some(factory) => factory.build(target, config).await,
        None => Err(Status::invalid_argument(format!(
            "no resolver registered for scheme {:?}",
            target.scheme
        ))),
    }
}
