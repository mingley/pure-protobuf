//! gRPC authorization policies (gRFC A43).
//!
//! A [`Policy`] is JSON with `allow_rules` and optional `deny_rules`.
//! Each rule matches a source (TLS principal) and a request (paths and
//! headers); deny rules run first, then allow rules, and anything
//! unmatched is denied. Servers enforce a policy with
//! [`Server::authorization_policy`](crate::Server::authorization_policy) /
//! [`Router::authorization_policy`](crate::Router::authorization_policy),
//! which reject denied calls with `PERMISSION_DENIED` before the handler
//! runs, on every call shape.
//!
//! ```
//! use pbrs_grpc::authz::{AuthzInterceptor, StaticDataProvider};
//!
//! let provider = StaticDataProvider::new(
//!     r#"{"name":"open","allow_rules":[{"name":"all"}]}"#,
//! )
//! .expect("policy");
//! let _enforce = AuthzInterceptor::new(provider);
//! ```
//!
//! Parsing is strict and fails closed: unknown fields, duplicate rule
//! names, bad patterns, and unsupported header keys (`Host`, hop-by-hop,
//! `:` pseudo-headers, `grpc-` headers) all reject the policy.

mod interceptor;
mod matcher;
mod policy;
mod principal;
mod provider;

pub use interceptor::AuthzInterceptor;
pub use policy::{HeaderMatcher, Policy, PolicyError, Request, Rule, Source};
pub use principal::PeerPrincipals;
pub use provider::{FileWatcherProvider, Provider, StaticDataProvider};
