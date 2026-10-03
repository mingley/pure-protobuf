//! Fresh actual generated-consumer oracles. Baseline semantic reds are NOT_RUN.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "bounded regression oracles inspect actual generated parse outcomes"
)]
pub mod generated {
    include!("qg20.rs");
}
#[allow(
    dead_code,
    reason = "descriptor builder is also used by parent integration driver"
)]
mod vectors;

#[cfg(test)]
mod tests {
    use super::{generated::Node, vectors};
    use pbrs::{Parse, Serialize};

    fn validate(bytes: &[u8], depth: u32) -> bool {
        let mut pos = 0;
        let valid =
            Node::validate_inner(&pbrs::rt::Wire::from_slice(bytes), &mut pos, depth).is_ok();
        if valid {
            assert_eq!(pos, bytes.len());
        }
        valid
    }

    fn stable_parse(bytes: &[u8]) {
        let message = Node::parse(bytes).unwrap();
        let canonical = message.serialize().unwrap();
        assert_eq!(message.serialized_len(), canonical.len());
        assert_eq!(
            Node::parse(&canonical).unwrap().serialize().unwrap(),
            canonical
        );
    }

    #[test]
    fn present_message_values_total_100_accept_eager_lazy_and_validator() {
        for child in [1, 3] {
            for (known, groups) in [(98, 0), (97, 1)] {
                let bytes = vectors::message_value(known, child, groups);
                stable_parse(&bytes);
                assert!(validate(&bytes, 0));
            }
        }
    }

    #[test]
    fn present_message_values_total_101_reject_eager_and_lazy() {
        for child in [1, 3] {
            for (known, groups) in [(99, 0), (98, 1)] {
                assert!(Node::parse(&vectors::message_value(known, child, groups)).is_err());
            }
        }
    }

    #[test]
    fn known_message_value_validator_rejects_total_101() {
        for (known, groups) in [(99, 0), (98, 1)] {
            assert!(!validate(&vectors::message_value(known, 1, groups), 0));
        }
    }

    #[test]
    fn scalar_map_entry_total_100_accepts_eager_lazy_and_validator() {
        for child in [1, 3] {
            let bytes = vectors::scalar_entry(99, child);
            stable_parse(&bytes);
            assert!(validate(&bytes, 0));
        }
    }

    #[test]
    fn map_entries_total_101_reject_even_with_no_value() {
        for child in [1, 3] {
            for field in [5, 6] {
                let bytes = vectors::empty_entry(100, child, field);
                assert!(Node::parse(&bytes).is_err());
                assert!(!validate(&bytes, 0));
            }
        }
    }

    #[test]
    fn supplied_parent_depth_100_rejects_a_known_map_entry() {
        for field in [5, 6] {
            let bytes = vectors::empty_entry(0, 1, field);
            assert!(Node::new().merge_bytes(&bytes, 99).is_ok());
            assert!(Node::new().merge_bytes(&bytes, 100).is_err());
            assert!(!validate(&bytes, 100));
        }
    }

    #[test]
    fn malformed_known_values_reject_through_lazy_ancestors() {
        let mut truncated_group = Vec::new();
        pbrs::rt::encode_tag(&mut truncated_group, 99, pbrs::rt::WIRE_SGROUP);
        for value in [&[0x10, 0x80][..], &[0x0a, 2, 0x10][..], &truncated_group] {
            // Root/direct/eager and lazy ancestors must agree on malformed known values.
            assert!(Node::parse(&vectors::malformed_value(0, 1, value)).is_err());
            for child in [1, 3] {
                assert!(Node::parse(&vectors::malformed_value(1, child, value)).is_err());
            }
        }
    }

    #[test]
    fn known_map_value_validator_checks_malformed_structure() {
        let mut truncated_group = Vec::new();
        pbrs::rt::encode_tag(&mut truncated_group, 99, pbrs::rt::WIRE_SGROUP);
        for value in [&[0x10, 0x80][..], &[0x0a, 2, 0x10][..], &truncated_group] {
            assert!(!validate(&vectors::malformed_value(0, 1, value), 0));
        }
        // Skip-based validators retain QG18's legacy mismatched end-number acceptance.
        // This test deliberately does not add stricter skip end-tag semantics.
    }

    #[test]
    fn legacy_group_end_number_difference_remains_explicit() {
        let mut mismatched = Vec::new();
        pbrs::rt::encode_tag(&mut mismatched, 99, pbrs::rt::WIRE_SGROUP);
        pbrs::rt::encode_tag(&mut mismatched, 98, pbrs::rt::WIRE_EGROUP);
        assert!(Node::parse(&vectors::malformed_value(0, 1, &mismatched)).is_err());
        assert!(Node::parse(&vectors::malformed_value(1, 3, &mismatched)).is_err());
        assert!(Node::parse(&vectors::malformed_value(1, 1, &mismatched)).is_ok());
        assert!(validate(&vectors::malformed_value(0, 1, &mismatched), 0));
    }

    #[test]
    fn unknown_length_delimited_values_remain_opaque() {
        for payload in [&[0xff][..], &vectors::unknown_groups(101)] {
            let mut opaque = Vec::new();
            pbrs::rt::encode_len_field(&mut opaque, 99, payload);
            assert_eq!(Node::parse(&opaque).unwrap().serialize().unwrap(), opaque);
            assert!(validate(&opaque, 0));
            let mut value = vec![0x10, 7];
            value.extend_from_slice(&opaque);
            let mut entry = vectors::message_entry(1, &[&value]);
            entry.extend_from_slice(&opaque);
            let bytes = vectors::wrap_known(vectors::map_node(5, &[&entry]), 1, 1);
            stable_parse(&bytes);
            assert!(validate(&bytes, 0));
        }
    }

    #[test]
    fn omitted_keys_values_and_scalar_defaults_are_preserved() {
        let bytes = vectors::map_node(5, &[&[]]);
        let message = Node::parse(&bytes).unwrap();
        let value = message.nodes().get(0).unwrap();
        assert_eq!(value.value(), 0);
        assert!(!value.has_value());
        assert_eq!(message.nodes().len(), 1);
        stable_parse(&bytes);
        let bytes = vectors::map_node(6, &[&[]]);
        let message = Node::parse(&bytes).unwrap();
        assert_eq!(message.scores().get(0), Some(0));
        stable_parse(&bytes);
    }

    #[test]
    fn duplicate_keys_keep_last_value_and_complete_wire_order() {
        let first = vectors::message_entry(1, &[&[0x10, 11]]);
        let second = vectors::message_entry(2, &[&[0x10, 22]]);
        let third = vectors::message_entry(1, &[&[0x10, 33]]);
        let bytes = vectors::map_node(5, &[&first, &second, &third]);
        let message = Node::parse(&bytes).unwrap();
        assert_eq!(message.nodes().len(), 2);
        assert_eq!(message.nodes().get(1).unwrap().value(), 33);
        assert_eq!(message.nodes().get(2).unwrap().value(), 22);
        assert_eq!(message.serialize().unwrap(), bytes);
        stable_parse(&bytes);
    }

    #[test]
    fn repeated_message_values_in_one_entry_deep_merge() {
        let entry = vectors::message_entry(1, &[&[0x10, 11], &[0x22, 1, b'x']]);
        let bytes = vectors::map_node(5, &[&entry]);
        let message = Node::parse(&bytes).unwrap();
        let value = message.nodes().get(1).unwrap();
        assert_eq!(value.value(), 11);
        assert_eq!(value.label().as_bytes(), b"x");
        let merged = vectors::message_entry(1, &[&[0x10, 11, 0x22, 1, b'x']]);
        assert_eq!(
            message.serialize().unwrap(),
            vectors::map_node(5, &[&merged])
        );
        stable_parse(&bytes);
    }

    #[test]
    fn required_known_payloads_validate_through_lazy_ancestors() {
        let valid = vectors::message_entry(1, &[&[0x08, 7]]);
        let invalid = vectors::message_entry(1, &[&[0x12, 1, b'x']]);
        for child in [1, 3] {
            let bytes = vectors::wrap_known(vectors::map_node(7, &[&valid]), 1, child);
            stable_parse(&bytes);
            assert!(validate(&bytes, 0));
            let bytes = vectors::wrap_known(vectors::map_node(7, &[&invalid]), 1, child);
            assert!(Node::parse(&bytes).is_err());
            assert!(!validate(&bytes, 0));
        }
    }

    #[test]
    fn proto2_unchecked_utf8_in_known_values_remains_accepted() {
        let entry = vectors::message_entry(1, &[&[0x22, 1, 0xff]]);
        let bytes = vectors::wrap_known(vectors::map_node(5, &[&entry]), 1, 1);
        stable_parse(&bytes);
        assert!(validate(&bytes, 0));
    }
}
