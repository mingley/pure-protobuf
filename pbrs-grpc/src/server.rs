//! Serving: the [`Service`] trait, per-RPC dispatch through [`Rpc`], and the
//! [`Server`] / [`Router`] accept loops.
//!
//! Generated code implements [`Service`]; you implement the generated service
//! trait. Writing either by hand is supported and documented, because a
//! kernel you cannot drive by hand is a kernel you cannot debug.

pub(crate) mod accept;
pub(crate) mod connection;
pub(crate) mod dispatch;
pub(crate) mod drain;
pub(crate) mod router;
pub(crate) mod rpc;

pub use accept::{Incoming, IncomingAccept, PeerCred, Server};
pub use connection::ConnectionInfo;
pub use dispatch::Service;
pub use router::Router;
pub(crate) use router::split_path;
pub use rpc::Rpc;

#[cfg(test)]
mod tests {
    use super::{ConnectionInfo, PeerCred, split_path};
    use crate::tls::PeerIdentity;

    #[test]
    fn connection_info_debug_masks_peer_details_without_hiding_getters() {
        let remote = "192.0.2.100:51401".parse().expect("remote");
        let local = "127.0.0.1:51402".parse().expect("local");
        let cred = PeerCred::new(914_217, 914_218, Some(914_219));
        let peer = ConnectionInfo::new()
            .with_remote_addr(remote)
            .with_local_addr(local)
            .with_peer_identity(
                PeerIdentity::from_der_certs([b"private-cert-leaf"]).expect("identity"),
            )
            .with_peer_cred(cred)
            .with_scheme("https");
        let shown = format!("{peer:?}");
        for field in ["remote", "local", "identity", "cred", "scheme"] {
            assert!(
                shown.contains(&format!("{field}: Some(\"[REDACTED]\")")),
                "{shown}"
            );
        }
        for secret in ["192.0.2.100", "127.0.0.1:51402", "914217", "PeerIdentity"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert_eq!(peer.remote_addr(), Some(remote));
        assert_eq!(peer.local_addr(), Some(local));
        assert_eq!(peer.peer_cred(), Some(cred));
        assert_eq!(peer.scheme(), Some("https"));
    }

    #[test]
    fn splits_service_and_method() {
        assert_eq!(
            split_path("/helloworld.Greeter/SayHello"),
            ("helloworld.Greeter", "SayHello")
        );
        assert_eq!(split_path("/a.B/C"), ("a.B", "C"));
    }

    #[test]
    fn unparseable_paths_route_nowhere() {
        assert_eq!(split_path("/"), ("", ""));
        assert_eq!(split_path(""), ("", ""));
        assert_eq!(split_path("/nomethod"), ("", ""));
    }
}
