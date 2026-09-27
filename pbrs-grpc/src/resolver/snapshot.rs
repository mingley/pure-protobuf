//! Published snapshots: one normalized address list per generation.

use std::net::SocketAddr;
use std::path::PathBuf;

/// One dialable address from a resolver.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResolvedAddress {
    /// TCP address.
    Tcp(SocketAddr),
    /// Unix-domain socket path.
    Unix(PathBuf),
    /// Linux abstract-namespace name (without the leading NUL).
    UnixAbstract(Vec<u8>),
}

/// A full resolution result: ordered, deduplicated addresses plus a
/// monotonic generation. Readers hold an `Arc<Resolution>`; publishers
/// bump the generation only when the address list actually changes, so
/// a repeated identical answer never disturbs healthy connections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    addresses: Vec<ResolvedAddress>,
    generation: u64,
    service_config: Option<String>,
}

impl Resolution {
    /// Normalize `addresses` (deduplicate exact repeats, keep order).
    #[must_use]
    pub fn new(addresses: Vec<ResolvedAddress>, generation: u64) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(addresses.len());
        for addr in addresses {
            if seen.insert(addr.clone()) {
                out.push(addr);
            }
        }
        Self {
            addresses: out,
            generation,
            service_config: None,
        }
    }

    /// Attach a resolver-delivered service-config document (raw JSON).
    #[must_use]
    pub fn with_service_config(mut self, json: Option<String>) -> Self {
        self.service_config = json;
        self
    }

    /// Authoritative empty result at `generation`: no address is
    /// eligible for new calls. Distinct from a lookup failure, which
    /// keeps serving the last snapshot inside the stale budget.
    /// Carries no service config.
    #[must_use]
    pub fn empty(generation: u64) -> Self {
        Self {
            addresses: Vec::new(),
            generation,
            service_config: None,
        }
    }

    /// Normalized addresses in resolver order.
    #[must_use]
    pub fn addresses(&self) -> &[ResolvedAddress] {
        &self.addresses
    }

    /// Monotonic publication counter.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Resolver-delivered service-config JSON, if the resolver provides
    /// one (DNS TXT via A2). Consumers parse and adopt with A21
    /// fallback: invalid replaces nothing.
    #[must_use]
    pub fn service_config(&self) -> Option<&str> {
        self.service_config.as_deref()
    }

    /// Whether no address is eligible.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Resolution, ResolvedAddress};
    use std::net::SocketAddr;

    fn tcp(s: &str) -> ResolvedAddress {
        ResolvedAddress::Tcp(s.parse::<SocketAddr>().expect("addr"))
    }

    #[test]
    fn dedupes_keeping_order() {
        let r = Resolution::new(
            vec![tcp("10.0.0.1:80"), tcp("10.0.0.2:80"), tcp("10.0.0.1:80")],
            3,
        );
        assert_eq!(r.addresses(), &[tcp("10.0.0.1:80"), tcp("10.0.0.2:80")]);
        assert_eq!(r.generation(), 3);
        assert!(!r.is_empty());
        assert!(Resolution::empty(4).is_empty());
    }
}
