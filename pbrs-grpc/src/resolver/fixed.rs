//! Static resolvers: `passthrough:`, `ipv4:`, `ipv6:`, `unix:`,
//! `unix-abstract:`. Single snapshot, no refresh task.

use super::dns::SystemDns;
use super::registry::{BuiltResolver, ResolverConfig, ResolverFactory};
use super::snapshot::{Resolution, ResolvedAddress};
use super::target::{ParsedTarget, parse_ip_list, unix_path};
use crate::status::Status;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub(crate) fn factories() -> Vec<Arc<dyn ResolverFactory>> {
    vec![
        Arc::new(PassthroughScheme),
        Arc::new(Ipv4Scheme),
        Arc::new(Ipv6Scheme),
        Arc::new(UnixScheme),
        Arc::new(UnixAbstractScheme),
    ]
}

fn boxed<'a>(
    fut: impl Future<Output = Result<BuiltResolver, Status>> + Send + 'a,
) -> Pin<Box<dyn Future<Output = Result<BuiltResolver, Status>> + Send + 'a>> {
    Box::pin(fut)
}

/// `passthrough:` resolves once with the system DNS (or the injected
/// provider) and never refreshes: the CH-02 spelling of plain dial.
#[derive(Debug, Default)]
pub struct PassthroughScheme;

impl ResolverFactory for PassthroughScheme {
    fn scheme(&self) -> &str {
        "passthrough"
    }

    fn build(
        &self,
        target: &ParsedTarget,
        config: &ResolverConfig,
    ) -> Pin<Box<dyn Future<Output = Result<BuiltResolver, Status>> + Send + '_>> {
        let dial = target.remainder.clone();
        let provider: Arc<dyn super::dns::DnsLookup> = config
            .dns_provider()
            .cloned()
            .unwrap_or_else(|| Arc::new(SystemDns));
        boxed(async move {
            let addrs = provider.lookup(&dial).await.map_err(|e| {
                Status::unavailable(format!("passthrough: lookup of {dial:?} failed: {e}"))
            })?;
            if addrs.is_empty() {
                return Err(Status::unavailable(format!(
                    "passthrough: lookup of {dial:?} returned no addresses"
                )));
            }
            Ok(BuiltResolver::once(Arc::new(Resolution::new(
                addrs.into_iter().map(ResolvedAddress::Tcp).collect(),
                0,
            ))))
        })
    }
}

macro_rules! static_scheme {
    ($name:ident, $scheme:literal, $doc:literal, $resolve:expr) => {
        #[derive(Debug, Default)]
        #[doc = $doc]
        pub struct $name;

        impl ResolverFactory for $name {
            fn scheme(&self) -> &str {
                $scheme
            }

            fn build(
                &self,
                target: &ParsedTarget,
                _config: &ResolverConfig,
            ) -> Pin<Box<dyn Future<Output = Result<BuiltResolver, Status>> + Send + '_>> {
                let target = target.clone();
                let resolve = $resolve;
                boxed(async move { resolve(&target) })
            }
        }
    };
}

static_scheme!(
    Ipv4Scheme,
    "ipv4",
    "Factory for `ipv4:` targets: static literal list.",
    |target: &ParsedTarget| {
        Ok(BuiltResolver::once(Arc::new(Resolution::new(
            parse_ip_list(&target.remainder)
                .into_iter()
                .map(ResolvedAddress::Tcp)
                .collect(),
            0,
        ))))
    }
);

static_scheme!(
    Ipv6Scheme,
    "ipv6",
    "Factory for `ipv6:` targets: static literal list.",
    |target: &ParsedTarget| {
        Ok(BuiltResolver::once(Arc::new(Resolution::new(
            parse_ip_list(&target.remainder)
                .into_iter()
                .map(ResolvedAddress::Tcp)
                .collect(),
            0,
        ))))
    }
);

static_scheme!(
    UnixScheme,
    "unix",
    "Factory for `unix:` targets: one socket path.",
    |target: &ParsedTarget| {
        Ok(BuiltResolver::once(Arc::new(Resolution::new(
            vec![ResolvedAddress::Unix(unix_path(target))],
            0,
        ))))
    }
);

static_scheme!(
    UnixAbstractScheme,
    "unix-abstract",
    "Factory for `unix-abstract:` targets: one abstract name (Linux dial).",
    |target: &ParsedTarget| {
        Ok(BuiltResolver::once(Arc::new(Resolution::new(
            vec![ResolvedAddress::UnixAbstract(
                target.remainder.as_bytes().to_vec(),
            )],
            0,
        ))))
    }
);
