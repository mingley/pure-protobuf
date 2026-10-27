//! Fresh generated-code controls for final-tree required initialization.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "bounded generated-code regression controls"
)]
mod generated {
    include!("required_merge.rs");
}

#[cfg(test)]
mod tests {
    use super::generated::{Child, ColdRequired, Envelope, MessageSetHost, Parent, Wide};
    use pbrs::{Parse, Serialize};

    fn validate_parent(bytes: &[u8]) -> bool {
        let mut pos = 0;
        let result = Parent::validate_inner(&pbrs::rt::Wire::from_slice(bytes), &mut pos, 0);
        if result.is_ok() {
            assert_eq!(pos, bytes.len());
        }
        result.is_ok()
    }

    fn stable_parent(bytes: &[u8]) -> Parent {
        let value = Parent::parse(bytes).expect("valid final initialized tree");
        let encoded = value.serialize().unwrap();
        assert_eq!(
            Parent::parse(&encoded).unwrap().serialize().unwrap(),
            encoded
        );
        value
    }

    #[test]
    fn split_required_fields_merge_in_one_singular_child() {
        let bytes = [0x0a, 2, 8, 1, 0x0a, 2, 16, 2];
        let value = stable_parent(&bytes);
        assert_eq!((value.child().a(), value.child().b()), (1, 2));
    }

    #[test]
    fn validator_accepts_split_required_fields_after_merge() {
        assert!(validate_parent(&[0x0a, 2, 8, 1, 0x0a, 2, 16, 2]));
    }

    #[test]
    fn split_merge_survives_a_lazy_enclosing_message() {
        let bytes = [0x0a, 8, 0x0a, 2, 8, 1, 0x0a, 2, 16, 2];
        let value = Envelope::parse(&bytes).expect("valid nested final tree");
        assert_eq!(
            (value.inner().child().a(), value.inner().child().b()),
            (1, 2)
        );
    }

    #[test]
    fn split_merge_across_duplicate_ancestors_uses_the_final_tree() {
        let bytes = [0x0a, 4, 0x0a, 2, 8, 1, 0x0a, 4, 0x0a, 2, 16, 2];
        let value = Envelope::parse(&bytes).expect("duplicate ancestors merge");
        assert_eq!(
            (value.inner().child().a(), value.inner().child().b()),
            (1, 2)
        );
    }

    #[test]
    fn same_oneof_message_case_merges_required_fields() {
        let value = stable_parent(&[0x1a, 2, 8, 1, 0x1a, 2, 16, 2]);
        assert_eq!((value.selected().a(), value.selected().b()), (1, 2));
    }

    #[test]
    fn discarded_partial_oneof_message_is_not_a_live_required_failure() {
        let bytes = [0x1a, 2, 8, 1, 0x20, 7];
        assert_eq!(stable_parent(&bytes).other(), 7);
        assert!(validate_parent(&bytes));
    }

    #[test]
    fn switching_oneof_case_does_not_merge_discarded_required_fields() {
        let bytes = [0x1a, 2, 8, 1, 0x20, 7, 0x1a, 2, 16, 2];
        assert!(Parent::parse(&bytes).is_err());
        assert!(!validate_parent(&bytes));
    }

    #[test]
    fn separate_repeated_elements_cannot_complete_each_others_required_fields() {
        let bytes = [0x12, 2, 8, 1, 0x12, 2, 16, 2];
        assert!(Parent::parse(&bytes).is_err());
        assert!(!validate_parent(&bytes));
    }

    #[test]
    fn present_zero_fields_count_as_initialized_but_missing_fields_do_not() {
        assert!(Child::parse(&[8, 0, 16, 0]).is_ok());
        assert!(Child::parse(&[8, 0]).is_err());
        assert!(Parent::parse(&[0x0a, 2, 8, 0]).is_err());
        assert!(!validate_parent(&[0x0a, 2, 8, 0]));
        assert!(Parent::parse(&[]).is_ok());
    }

    #[test]
    fn initialized_elements_and_oneof_cases_keep_their_values() {
        let value = stable_parent(&[0x12, 4, 8, 1, 16, 2, 0x1a, 4, 8, 3, 16, 4]);
        assert_eq!(value.children().len(), 1);
        assert_eq!(
            (value.children().get(0).unwrap().a(), value.selected().b()),
            (1, 4)
        );
    }

    #[test]
    fn discarded_message_wire_must_still_be_structurally_valid() {
        let bytes = [0x1a, 2, 8, 0x80, 0x20, 7];
        assert!(Parent::parse(&bytes).is_err());
        assert!(!validate_parent(&bytes));
    }

    fn map_entry(values: &[&[u8]]) -> Vec<u8> {
        let mut entry = vec![8, 1];
        for value in values {
            pbrs::rt::encode_len_field(&mut entry, 2, value);
        }
        entry
    }

    fn map_parent(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut wire = Vec::new();
        for entry in entries {
            pbrs::rt::encode_len_field(&mut wire, 6, entry);
        }
        wire
    }

    #[test]
    fn message_map_value_fragments_merge_within_one_entry() {
        let wire = map_parent(&[map_entry(&[&[8, 1], &[16, 2]])]);
        let value = stable_parent(&wire);
        let child = value.by_key().get(1).unwrap();
        assert_eq!((child.a(), child.b()), (1, 2));
        assert!(validate_parent(&wire));
    }

    #[test]
    fn message_map_validates_the_last_value_for_a_duplicate_key() {
        let wire = map_parent(&[map_entry(&[&[8, 1]]), map_entry(&[&[8, 1, 16, 2]])]);
        let value = stable_parent(&wire);
        assert_eq!(value.by_key().len(), 1);
        assert_eq!(
            (
                value.by_key().get(1).unwrap().a(),
                value.by_key().get(1).unwrap().b()
            ),
            (1, 2)
        );
        assert!(validate_parent(&wire));
    }

    #[test]
    fn message_map_separate_values_do_not_complete_each_other() {
        let wire = map_parent(&[map_entry(&[&[8, 1]]), map_entry(&[&[16, 2]])]);
        assert!(Parent::parse(&wire).is_err());
        assert!(!validate_parent(&wire));
    }

    #[test]
    fn omitted_map_value_must_be_initialized_if_it_survives() {
        let missing = map_parent(&[map_entry(&[])]);
        assert!(Parent::parse(&missing).is_err());
        assert!(!validate_parent(&missing));
        let overwritten = map_parent(&[map_entry(&[]), map_entry(&[&[8, 1, 16, 2]])]);
        assert!(Parent::parse(&overwritten).is_ok());
        assert!(validate_parent(&overwritten));
    }

    #[test]
    fn overwritten_map_value_wire_must_still_be_valid() {
        let wire = map_parent(&[map_entry(&[&[8, 0x80]]), map_entry(&[&[8, 1, 16, 2]])]);
        assert!(Parent::parse(&wire).is_err());
        assert!(!validate_parent(&wire));
    }

    #[test]
    fn split_group_fields_merge_and_missing_group_fields_reject() {
        let split = [43, 8, 1, 44, 43, 16, 2, 44];
        let value = stable_parent(&split);
        assert_eq!((value.g().a(), value.g().b()), (1, 2));
        assert!(validate_parent(&split));
        let missing = [43, 8, 1, 44];
        assert!(Parent::parse(&missing).is_err());
        assert!(!validate_parent(&missing));
    }

    #[test]
    fn required_cold_fields_preserve_presence_and_zero_defaults() {
        let valid = [8, 0, 18, 0, 34, 4, 8, 1, 16, 2];
        let value = ColdRequired::parse(&valid).expect("present zero fields remain initialized");
        assert!(value.has_tag());
        assert!(value.has_name());
        assert!(ColdRequired::parse(&[8, 1]).is_err());
        assert!(ColdRequired::parse(&[18, 0]).is_err());
    }

    #[test]
    fn more_than_sixty_four_required_fields_validate_without_a_bitmask_limit() {
        let mut wire = Vec::new();
        for number in 1..=65 {
            pbrs::rt::encode_tag(&mut wire, number, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(&mut wire, 0);
        }
        assert!(Wide::parse(&wire).is_ok());
        let mut pos = 0;
        assert!(Wide::validate_inner(&pbrs::rt::Wire::from_slice(&wire), &mut pos, 0).is_ok());
        assert_eq!(pos, wire.len());
        wire.truncate(wire.len() - 3);
        assert!(Wide::parse(&wire).is_err());
        let mut pos = 0;
        assert!(Wide::validate_inner(&pbrs::rt::Wire::from_slice(&wire), &mut pos, 0).is_err());
    }

    #[test]
    fn dont_enforce_allows_a_partial_child_until_a_later_merge_completes_it() {
        let mut value = Parent::new();
        value.merge_bytes_dont_enforce(&[10, 2, 8, 1], 0).unwrap();
        assert!(value.check_required().is_err());
        value.merge_bytes(&[10, 2, 16, 2], 0).unwrap();
        assert_eq!((value.child().a(), value.child().b()), (1, 2));
        assert!(value.check_required().is_ok());
    }

    #[test]
    fn required_tree_parse_and_validator_keep_the_recursion_boundary() {
        let wire = [10, 4, 8, 1, 16, 2];
        assert!(Parent::new().merge_bytes(&wire, 99).is_ok());
        assert!(Parent::new().merge_bytes(&wire, 100).is_err());
        let mut pos = 0;
        assert!(Parent::validate_inner(&pbrs::rt::Wire::from_slice(&wire), &mut pos, 100).is_err());
    }

    #[test]
    fn empty_enforcing_merge_checks_an_existing_partial_child() {
        use pbrs::ClearAndParse;
        let mut partial = Parent::parse_dont_enforce_required(&[10, 2, 8, 1]).unwrap();
        assert!(partial.check_required().is_err());
        assert!(ClearAndParse::merge_from_bytes(&mut partial, &[]).is_err());
        partial
            .merge_bytes_dont_enforce(&[10, 2, 16, 2], 0)
            .unwrap();
        assert!(ClearAndParse::merge_from_bytes(&mut partial, &[]).is_ok());
        assert!(ClearAndParse::merge_from_bytes(&mut Parent::new(), &[]).is_ok());
    }

    #[test]
    fn empty_shared_enforcing_merge_checks_an_existing_partial_child() {
        use pbrs::ClearAndParse;
        let mut partial = Parent::parse_dont_enforce_required(&[10, 2, 8, 1]).unwrap();
        assert!(partial.check_required().is_err());
        assert!(
            ClearAndParse::merge_from_bytes_shared(&mut partial, pbrs::rt::Bytes::new()).is_err()
        );
        partial
            .merge_bytes_dont_enforce(&[10, 2, 16, 2], 0)
            .unwrap();
        assert!(
            ClearAndParse::merge_from_bytes_shared(&mut partial, pbrs::rt::Bytes::new()).is_ok()
        );
        assert!(
            ClearAndParse::merge_from_bytes_shared(&mut Parent::new(), pbrs::rt::Bytes::new())
                .is_ok()
        );
    }

    fn message_set_item(payload: &[u8]) -> Vec<u8> {
        let mut wire = vec![11, 16, 100];
        pbrs::rt::encode_len_field(&mut wire, 3, payload);
        wire.push(12);
        wire
    }

    #[test]
    fn known_message_set_extension_merges_required_fields_across_items() {
        // C++ protobuf35.1 merges these items. Python upb6.33.5 instead
        // replaces the value; that reference divergence is recorded separately.
        let mut wire = message_set_item(&[8, 1]);
        wire.extend_from_slice(&message_set_item(&[16, 2]));
        let value = MessageSetHost::parse(&wire).unwrap();
        assert!(value.check_required().is_ok());
        assert!(MessageSetHost::parse(&value.serialize().unwrap()).is_ok());
    }

    #[test]
    fn known_message_set_extension_rejects_a_final_partial_value() {
        assert!(MessageSetHost::parse(&message_set_item(&[8, 1])).is_err());
    }

    #[test]
    fn known_message_set_extension_rejects_malformed_payload_before_merging() {
        let mut wire = message_set_item(&[8, 0x80]);
        wire.extend_from_slice(&message_set_item(&[8, 1, 16, 2]));
        assert!(MessageSetHost::parse(&wire).is_err());
    }

    #[test]
    fn merging_required_fragments_keeps_unknown_fields_and_opaque_payloads() {
        let mut first = vec![8, 1];
        pbrs::rt::encode_tag(&mut first, 99, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut first, 7);
        let mut second = vec![16, 2];
        pbrs::rt::encode_len_field(&mut second, 98, &[0xff]);
        let mut wire = Vec::new();
        pbrs::rt::encode_len_field(&mut wire, 1, &first);
        pbrs::rt::encode_len_field(&mut wire, 1, &second);
        let value = stable_parent(&wire);
        let mut canonical = vec![8, 1, 16, 2];
        canonical.extend_from_slice(&first[2..]);
        canonical.extend_from_slice(&second[2..]);
        assert_eq!(value.child().serialize().unwrap(), canonical);
        assert!(validate_parent(&wire));
    }

    const SHARED_PAYLOAD_SIZE: usize = 2 * pbrs::rt::LazyBytes::SHARE_THRESHOLD;

    fn shared_payload_wire() -> Vec<u8> {
        let mut child = vec![8, 1, 16, 2];
        pbrs::rt::encode_len_field(&mut child, 3, &[0x5a; SHARED_PAYLOAD_SIZE]);
        let mut wire = Vec::new();
        pbrs::rt::encode_len_field(&mut wire, 1, &child);
        wire
    }

    #[test]
    fn shared_input_keeps_large_payload_backing_inside_required_children() {
        let shared = pbrs::rt::Bytes::from(shared_payload_wire());
        let expected = shared[shared.len() - SHARED_PAYLOAD_SIZE..].as_ptr();
        let value = Parent::parse_bytes(shared).unwrap();
        assert_eq!(value.child().payload(), &[0x5a; SHARED_PAYLOAD_SIZE]);
        assert_eq!(value.child().payload().as_ptr(), expected);
    }

    #[test]
    fn shared_input_keeps_backing_through_required_reachable_ancestors() {
        let mut wire = Vec::new();
        pbrs::rt::encode_len_field(&mut wire, 1, &shared_payload_wire());
        let shared = pbrs::rt::Bytes::from(wire);
        let expected = shared[shared.len() - SHARED_PAYLOAD_SIZE..].as_ptr();
        let value = Envelope::parse_bytes(shared).unwrap();
        assert_eq!(value.inner().child().payload(), &[0x5a; SHARED_PAYLOAD_SIZE]);
        assert_eq!(value.inner().child().payload().as_ptr(), expected);
    }

    #[test]
    fn required_child_retains_shared_owner_until_its_payload_is_dropped() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Owner {
            bytes: Vec<u8>,
            drops: Arc<AtomicUsize>,
        }
        impl AsRef<[u8]> for Owner {
            fn as_ref(&self) -> &[u8] {
                &self.bytes
            }
        }
        impl Drop for Owner {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let owner = Owner {
            bytes: shared_payload_wire(),
            drops: Arc::clone(&drops),
        };
        let value = Parent::parse_bytes(pbrs::rt::Bytes::from_owner(owner)).unwrap();
        assert_eq!(value.child().payload(), &[0x5a; SHARED_PAYLOAD_SIZE]);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(value);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
