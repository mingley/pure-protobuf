//! Server enforcement: an [`Interceptor`](crate::Interceptor) that denies
//! unauthorized calls with `PERMISSION_DENIED` before the handler runs.
//!
//! Attach with [`Server::authorization_policy`](crate::Server::authorization_policy)
//! or [`Router::authorization_policy`](crate::Router::authorization_policy).

use super::policy::{CallAttributes, Decision, Policy};
use super::principal::PeerPrincipals;
use super::provider::Provider;
use crate::server::Rpc;
use crate::status::Status;
use std::sync::Arc;

/// Enforces an A43 policy on every inbound call shape.
#[derive(Clone, Debug)]
pub struct AuthzInterceptor {
    provider: Provider,
}

impl AuthzInterceptor {
    /// Enforce `provider`'s current policy on every call.
    #[must_use]
    pub fn new(provider: impl Into<Provider>) -> Self {
        Self {
            provider: provider.into(),
        }
    }

    /// The provider in effect (a file watcher serves fresh policies).
    #[must_use]
    pub fn provider(&self) -> &Provider {
        &self.provider
    }
}

impl crate::Interceptor for AuthzInterceptor {
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status> {
        let policy = self.provider.policy();
        authorize(&policy, rpc)
    }
}

/// Run `policy` against `rpc`. Shared by the interceptor and tests.
pub(crate) fn authorize(policy: &Arc<Policy>, rpc: &Rpc) -> Result<(), Status> {
    let peer = peer_of(rpc);
    let headers = |key: &str| rpc.metadata().get_all(key).collect::<Vec<_>>();
    let call = CallAttributes {
        path: rpc.path(),
        headers: &headers,
        peer: &peer,
    };
    let decision = policy.decide(&call);
    policy.audit(&call, &decision);
    match decision {
        Decision::Allow { .. } => Ok(()),
        Decision::Deny { rule } if rule.is_empty() => Err(Status::permission_denied(format!(
            "denied by authorization policy {:?}: no allow rule matched",
            policy.name(),
        ))),
        Decision::Deny { rule } => Err(Status::permission_denied(format!(
            "denied by authorization policy {:?}: deny rule {rule:?} matched",
            policy.name(),
        ))),
    }
}

/// TLS facts for principal matching: `:scheme` says whether TLS was
/// used, and the mTLS leaf (when present) yields SANs and the subject.
fn peer_of(rpc: &Rpc) -> PeerPrincipals {
    if rpc.scheme() != Some("https") {
        return PeerPrincipals::plaintext();
    }
    match rpc.peer_identity().and_then(|id| id.leaf()) {
        Some(leaf) => PeerPrincipals::from_leaf_der(leaf),
        None => PeerPrincipals::tls_no_cert(),
    }
}
