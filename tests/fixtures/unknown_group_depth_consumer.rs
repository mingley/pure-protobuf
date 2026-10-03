// Fresh generated consumer, supplied by tests/unknown_group_depth.rs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "bounded regression oracles assert parse outcomes"
)]
pub mod generated {
    include!("qg18.rs");
}
#[allow(dead_code, reason = "descriptor builder is shared with the parent integration oracle")]
mod vectors;

#[cfg(test)]
mod tests {
    use super::{generated::Node, vectors};
    use pbrs::{Parse, Serialize};

    fn stable_parse(wire: &[u8]) {
        let message = Node::parse(wire).unwrap();
        let canonical = message.serialize().unwrap();
        assert_eq!(message.serialized_len(), canonical.len());
        assert_eq!(Node::parse(&canonical).unwrap().serialize().unwrap(), canonical);
    }

    #[test]
    fn pure_unknown_boundary_accepts_100() {
        let wire = vectors::unknown_groups(100, 99);
        let message = Node::parse(&wire).unwrap();
        assert_eq!(message.serialize().unwrap(), wire);
    }

    #[test]
    fn pure_unknown_boundary_rejects_101() {
        assert!(Node::parse(&vectors::unknown_groups(101, 99)).is_err());
    }

    #[test]
    fn known_and_unknown_total_100_accepts_lazy_eager_and_wrong_wire() {
        for child in [1, 3] {
            for number in [99, 2] {
                for (known, groups) in [(99, 1), (100, 0)] {
                    stable_parse(&vectors::known_then_unknown(known, child, groups, number));
                }
            }
        }
    }

    #[test]
    fn known_and_unknown_total_101_rejects_lazy_eager_and_wrong_wire() {
        for child in [1, 3] {
            for number in [99, 2] {
                for (known, groups) in [(99, 2), (100, 1)] {
                    assert!(Node::parse(&vectors::known_then_unknown(known, child, groups, number)).is_err());
                }
            }
        }
    }

    #[test]
    fn known_groups_and_messages_share_unknown_budget() {
        stable_parse(&vectors::known_group_pairs_then_unknown(50, 0));
        stable_parse(&vectors::known_group_pairs_then_unknown(49, 2));
        assert!(Node::parse(&vectors::known_group_pairs_then_unknown(50, 1)).is_err());
    }

    #[test]
    fn validator_unknown_group_depth_matches_merge_boundary() {
        for (known, groups, valid) in [(0, 100, true), (0, 101, false), (99, 1, true), (99, 2, false)] {
            let bytes = vectors::known_then_unknown(known, 1, groups, 99);
            let wire = pbrs::rt::Wire::from_slice(&bytes);
            let mut pos = 0;
            assert_eq!(Node::validate_inner(&wire, &mut pos, 0).is_ok(), valid);
            if valid {
                assert_eq!(pos, bytes.len());
            }
        }
    }
}
