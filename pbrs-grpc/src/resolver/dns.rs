//! The `dns:` resolver: initial lookup plus a bounded refresh task.
//!
//! `tokio::net::lookup_host` exposes no TTL, so successful answers
//! refresh every `refresh_without_ttl`, failures back off from
//! `retry_min` to `retry_max`, and the last success stays eligible
//! until `valid_until + max_stale`. All bounds are explicit in
//! [`DnsConfig`]; nothing is defaulted.

use super::registry::{BuiltResolver, ResolverConfig, ResolverFactory, ResolverTask};
use super::snapshot::{Resolution, ResolvedAddress};
use super::target::{ParsedTarget, split_host_port};
use super::txt::{SystemTxt, TxtLookup, grpc_config_name};
use crate::status::Status;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Explicit DNS refresh bounds. Every field is caller-supplied; see
/// [the resolver contract](https://github.com/mingley/pure-protobuf/blob/main/docs/resolver-contract.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DnsConfig {
    min_refresh: Duration,
    max_refresh: Duration,
    refresh_without_ttl: Duration,
    retry_min: Duration,
    retry_max: Duration,
    max_stale: Duration,
}

impl DnsConfig {
    /// Build validated bounds. Rejects zero (except `max_stale`, where
    /// zero disables stale reuse), inverted, or overflowing bounds.
    pub fn new(
        min_refresh: Duration,
        max_refresh: Duration,
        refresh_without_ttl: Duration,
        retry_min: Duration,
        retry_max: Duration,
        max_stale: Duration,
    ) -> Result<Self, Status> {
        for (name, value) in [
            ("min_refresh", min_refresh),
            ("max_refresh", max_refresh),
            ("refresh_without_ttl", refresh_without_ttl),
            ("retry_min", retry_min),
            ("retry_max", retry_max),
        ] {
            if value.is_zero() {
                return Err(Status::invalid_argument(format!(
                    "dns: {name} must be positive"
                )));
            }
        }
        if min_refresh > max_refresh {
            return Err(Status::invalid_argument(format!(
                "dns: min_refresh {min_refresh:?} exceeds max_refresh {max_refresh:?}"
            )));
        }
        if retry_min > retry_max {
            return Err(Status::invalid_argument(format!(
                "dns: retry_min {retry_min:?} exceeds retry_max {retry_max:?}"
            )));
        }
        if refresh_without_ttl.checked_add(max_stale).is_none()
            || retry_max.checked_add(retry_max).is_none()
        {
            return Err(Status::invalid_argument(
                "dns: refresh bounds overflow".to_owned(),
            ));
        }
        Ok(Self {
            min_refresh,
            max_refresh,
            refresh_without_ttl: refresh_without_ttl.clamp(min_refresh, max_refresh),
            retry_min,
            retry_max,
            max_stale,
        })
    }

    /// How long past `valid_until` the last success stays eligible.
    /// Zero disables stale reuse.
    #[must_use]
    pub fn max_stale(&self) -> Duration {
        self.max_stale
    }
}

/// One DNS lookup. The system implementation uses
/// `tokio::net::lookup_host`; tests inject scripted fakes.
pub trait DnsLookup: Send + Sync + 'static {
    /// Resolve `host:port` to socket addresses.
    fn lookup(
        &self,
        host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>>;
}

/// Production DNS via `tokio::net::lookup_host`.
#[derive(Debug, Default)]
pub struct SystemDns;

impl DnsLookup for SystemDns {
    fn lookup(
        &self,
        host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        let host = host.to_owned();
        Box::pin(async move { tokio::net::lookup_host(host).await.map(Iterator::collect) })
    }
}

/// Whether `host` must skip DNS TXT service-config lookup (A10): IP
/// literals and `localhost` never issue one. CH-02 performs no TXT
/// lookups at all; this predicate is where CH-03's TXT fetch hangs.
#[must_use]
pub fn skips_txt_lookup(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.eq_ignore_ascii_case("localhost") || bare.eq_ignore_ascii_case("localhost.") {
        return true;
    }
    bare.parse::<std::net::IpAddr>().is_ok()
}

pub(crate) fn factories() -> Vec<Arc<dyn ResolverFactory>> {
    vec![Arc::new(DnsScheme)]
}

/// Factory for `dns:` targets.
#[derive(Debug, Default)]
pub struct DnsScheme;

impl ResolverFactory for DnsScheme {
    fn scheme(&self) -> &str {
        "dns"
    }

    fn build(
        &self,
        target: &ParsedTarget,
        config: &ResolverConfig,
    ) -> Pin<Box<dyn Future<Output = Result<BuiltResolver, Status>> + Send + '_>> {
        let target = target.clone();
        let config = config.clone();
        Box::pin(async move {
            if split_host_port(&target.remainder).is_none() {
                return Err(Status::invalid_argument(format!(
                    "dns: target needs host:port, got {:?}",
                    target.remainder
                )));
            }
            // IP literals never touch DNS (A10): static by construction.
            if let Ok(addr) = target.remainder.parse::<SocketAddr>() {
                let resolution = Arc::new(Resolution::new(vec![ResolvedAddress::Tcp(addr)], 0));
                return Ok(BuiltResolver::once(resolution));
            }
            let dns = config.dns().cloned().ok_or_else(|| {
                Status::invalid_argument(
                    "dns: needs explicit DnsConfig bounds via ResolverConfig::with_dns; static schemes use ResolverConfig::static_only",
                )
            })?;
            let provider: Arc<dyn DnsLookup> = config
                .dns_provider()
                .cloned()
                .unwrap_or_else(|| Arc::new(SystemDns));
            let dial = target.remainder.clone();
            let (host, _) = split_host_port(&dial).ok_or_else(|| {
                Status::invalid_argument(format!("dns: target needs host:port, got {dial:?}"))
            })?;
            // Literals and localhost never issue TXT (A10); without a
            // readable resolv.conf there is simply no TXT source.
            let txt_name = (!skips_txt_lookup(host)).then(|| grpc_config_name(host));
            let txt: Option<Arc<dyn TxtLookup>> = match config.txt_provider().cloned() {
                Some(provider) => Some(provider),
                None => SystemTxt::new().await.ok().map(|t| {
                    let t: Arc<dyn TxtLookup> = Arc::new(t);
                    t
                }),
            };
            let initial = lookup_snapshot(&provider, &dial).await.map_err(|e| {
                Status::unavailable(format!("dns: initial lookup of {dial:?} failed: {e}"))
            })?;
            let initial_txt = fetch_txt(&txt, txt_name.as_deref()).await;
            let initial =
                Arc::new(Resolution::new(initial, 0).with_service_config(initial_txt.clone()));
            let (tx, rx) = watch::channel(Arc::clone(&initial));
            let task = tokio::spawn(refresh_loop(
                provider,
                dial,
                dns,
                tx,
                Arc::clone(&initial),
                initial_txt,
                txt,
                txt_name,
            ));
            Ok(BuiltResolver::refreshing(
                initial,
                rx,
                ResolverTask::new(task),
            ))
        })
    }
}

async fn lookup_snapshot(
    provider: &Arc<dyn DnsLookup>,
    dial: &str,
) -> Result<Vec<ResolvedAddress>, io::Error> {
    Ok(provider
        .lookup(dial)
        .await?
        .into_iter()
        .map(ResolvedAddress::Tcp)
        .collect())
}

/// Fetch the service-config TXT set, joined into one document.
/// `None` when TXT is skipped, fails, or answers empty: failures and
/// empty answers keep the previous config rather than clearing it.
async fn fetch_txt(txt: &Option<Arc<dyn TxtLookup>>, name: Option<&str>) -> Option<String> {
    let (provider, name) = (txt.as_ref()?, name?);
    let records = provider.fetch_txt(name).await.ok()?;
    if records.iter().all(|r| r.is_empty()) {
        return None;
    }
    Some(records.concat())
}

/// One refresh task per resolver. Successful answers republish only on
/// change (addresses or service config); failures serve the last
/// success inside the stale budget, then publish authoritative empty
/// while retrying with capped backoff.
async fn refresh_loop(
    provider: Arc<dyn DnsLookup>,
    dial: String,
    dns: DnsConfig,
    tx: watch::Sender<Arc<Resolution>>,
    initial: Arc<Resolution>,
    mut service_config: Option<String>,
    txt: Option<Arc<dyn TxtLookup>>,
    txt_name: Option<String>,
) {
    let mut current = initial;
    let mut generation = 0u64;
    let mut valid_until = tokio::time::Instant::now() + dns.refresh_without_ttl;
    let mut backoff = dns.retry_min;
    loop {
        tokio::time::sleep(dns.refresh_without_ttl).await;
        match lookup_snapshot(&provider, &dial).await {
            Ok(addrs) => {
                backoff = dns.retry_min;
                valid_until = tokio::time::Instant::now() + dns.refresh_without_ttl;
                if let Some(txt) = fetch_txt(&txt, txt_name.as_deref()).await {
                    service_config = Some(txt);
                }
                let next = Arc::new(
                    Resolution::new(addrs, generation + 1)
                        .with_service_config(service_config.clone()),
                );
                if next.addresses() != current.addresses()
                    || next.service_config() != current.service_config()
                {
                    generation += 1;
                    current = next;
                    if tx.send(Arc::clone(&current)).is_err() {
                        return;
                    }
                }
            }
            Err(_) => {
                if tokio::time::Instant::now() > valid_until + dns.max_stale && !current.is_empty()
                {
                    generation += 1;
                    current = Arc::new(Resolution::empty(generation));
                    if tx.send(Arc::clone(&current)).is_err() {
                        return;
                    }
                }
                let wait = backoff.min(dns.retry_max);
                backoff = backoff.saturating_mul(2).min(dns.retry_max);
                tokio::time::sleep(wait).await;
            }
        }
        if tx.is_closed() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DnsConfig, skips_txt_lookup};
    use std::time::Duration;

    fn bounds() -> (Duration, Duration, Duration, Duration, Duration, Duration) {
        (
            Duration::from_secs(1),
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_millis(100),
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
    }

    #[test]
    fn validates_bounds() {
        let (a, b, c, d, e, f) = bounds();
        assert!(DnsConfig::new(a, b, c, d, e, f).is_ok());
        // max_stale may be zero (disables reuse); nothing else may.
        assert!(DnsConfig::new(a, b, c, d, e, Duration::ZERO).is_ok());
        assert!(DnsConfig::new(Duration::ZERO, b, c, d, e, f).is_err());
        assert!(DnsConfig::new(a, b, c, Duration::ZERO, e, f).is_err());
        // Inverted bounds rejected.
        assert!(DnsConfig::new(b, a, c, d, e, f).is_err());
        assert!(DnsConfig::new(a, b, c, e, d, f).is_err());
    }

    #[test]
    fn clamps_refresh_into_bounds() {
        let (a, b, _, d, e, f) = bounds();
        let tiny = Duration::from_nanos(1);
        let huge = Duration::from_secs(3600);
        let cfg = DnsConfig::new(a, b, tiny, d, e, f).expect("clamped up");
        assert_eq!(cfg.refresh_without_ttl, a);
        let cfg = DnsConfig::new(a, b, huge, d, e, f).expect("clamped down");
        assert_eq!(cfg.refresh_without_ttl, b);
    }

    #[test]
    fn a10_skip_rules() {
        assert!(skips_txt_lookup("127.0.0.1"));
        assert!(skips_txt_lookup("::1"));
        assert!(skips_txt_lookup("[::1]"));
        assert!(skips_txt_lookup("localhost"));
        assert!(skips_txt_lookup("LOCALHOST."));
        assert!(!skips_txt_lookup("example.com"));
        assert!(!skips_txt_lookup("localhost.example.com"));
    }
}
