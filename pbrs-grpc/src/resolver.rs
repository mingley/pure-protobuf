//! Client resolvers: target URIs, snapshot publication, refresh (CH-02).
//!
//! [`parse_target_uri`] accepts `dns:///`, `passthrough:`, `ipv4:`,
//! `ipv6:`, `unix:`, and `unix-abstract:` targets. Each scheme has a
//! [`ResolverFactory`] in the [`registry`]; factories build a resolver
//! that publishes [`Resolution`] snapshots over a
//! `tokio::sync::watch` channel. Readers hold an `Arc` snapshot, so
//! updates never block RPC picks: dialing clones the current `Arc`
//! without awaiting.
//!
//! Plain `host:port` is not a URI and keeps its exact behavior on
//! [`Channel::connect`](crate::Channel::connect); URI channels opt in
//! with [`Channel::connect_uri`](crate::Channel::connect_uri). DNS
//! refresh follows [the resolver
//! contract](https://github.com/mingley/pure-protobuf/blob/main/docs/resolver-contract.md):
//! explicit bounds, atomic generations, stale budgets, bounded drain of
//! removed addresses by the caller.
//!
//! IP literals and `localhost` never consult DNS TXT records (A10); TXT
//! service-config delivery itself arrives with CH-03.

mod dns;
mod fixed;
mod registry;
mod snapshot;
mod target;

pub use dns::{DnsConfig, DnsLookup, DnsScheme, SystemDns, skips_txt_lookup};
pub use fixed::{Ipv4Scheme, Ipv6Scheme, PassthroughScheme, UnixAbstractScheme, UnixScheme};
pub use registry::{
    BuiltResolver, ResolverConfig, ResolverFactory, ResolverHandle, ResolverTask,
    register_resolver_factory, resolver_for,
};
pub use snapshot::{Resolution, ResolvedAddress};
pub use target::{ParsedTarget, parse_target_uri};
