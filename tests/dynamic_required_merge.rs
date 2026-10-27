//! Required initialization is a property of the final merged dynamic tree.
#![cfg(feature = "reflect")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "small descriptor and wire fixtures with detecting assertions"
)]

use pbrs::{
    AsMut, AsView, Cardinality, ClearAndParse, DescriptorPool, DynamicMessage, FieldDescriptor,
    FieldType, MapKeyValue, MergeFrom, MessageDescriptor, Presence, Serialize, Value,
};
use std::sync::Arc;

fn scalar(name: &str, number: u32, required: bool) -> FieldDescriptor {
    FieldDescriptor::new(
        name,
        number,
        FieldType::Int32,
        if required {
            Cardinality::Required
        } else {
            Cardinality::Optional
        },
        Presence::Explicit,
    )
}

fn child_field(name: &str, number: u32, desc: &Arc<MessageDescriptor>) -> FieldDescriptor {
    let mut field = FieldDescriptor::new(
        name,
        number,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    field.message = Some(desc.clone());
    field
}

fn schema() -> (Arc<MessageDescriptor>, Arc<MessageDescriptor>) {
    let mut left = scalar("left", 3, false);
    left.oneof_index = Some(0);
    let mut right = scalar("right", 4, false);
    right.oneof_index = Some(0);
    let mut text = FieldDescriptor::new(
        "text",
        6,
        FieldType::String,
        Cardinality::Optional,
        Presence::Explicit,
    );
    text.utf8_validate = true;
    let mut child = MessageDescriptor::builder("dynamic_required.Child")
        .field(scalar("a", 1, true))
        .field(scalar("b", 2, true))
        .field(left)
        .field(right)
        .field(FieldDescriptor::new(
            "payload",
            5,
            FieldType::Bytes,
            Cardinality::Optional,
            Presence::Explicit,
        ))
        .field(text)
        .build();
    child.oneofs.push(vec![3, 4]);
    let child = Arc::new(child);
    let mut repeated = child_field("children", 2, &child);
    repeated.cardinality = Cardinality::Repeated;
    let mut selected = child_field("selected", 3, &child);
    selected.oneof_index = Some(0);
    let mut other = scalar("other", 4, false);
    other.oneof_index = Some(0);
    let mut group = child_field("group", 5, &child);
    group.field_type = FieldType::Group;
    let entry = Arc::new(
        MessageDescriptor::builder("dynamic_required.Entry")
            .map_entry(true)
            .field(scalar("key", 1, false))
            .field(child_field("value", 2, &child))
            .build(),
    );
    let mut map = child_field("by_key", 6, &entry);
    map.cardinality = Cardinality::Repeated;
    map.is_map = true;
    let mut root = MessageDescriptor::builder("dynamic_required.Root")
        .field(child_field("child", 1, &child))
        .field(repeated)
        .field(selected)
        .field(other)
        .field(group)
        .field(map)
        .build();
    root.oneofs.push(vec![3, 4]);
    (Arc::new(root), child)
}

fn len(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, number, payload);
    out
}

fn varint(number: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_tag(&mut out, number, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut out, value);
    out
}

fn joined(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

fn group(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_tag(&mut out, number, pbrs::rt::WIRE_SGROUP);
    out.extend_from_slice(payload);
    pbrs::rt::encode_tag(&mut out, number, pbrs::rt::WIRE_EGROUP);
    out
}

fn child(msg: &DynamicMessage, number: u32) -> &DynamicMessage {
    match msg.get_singular(number).unwrap() {
        Value::Message(child) => child,
        _ => panic!("expected child message"),
    }
}

fn initialized(msg: &DynamicMessage) {
    assert_eq!(msg.get_singular(1), Some(&Value::Int32(1)));
    assert_eq!(msg.get_singular(2), Some(&Value::Int32(2)));
}

#[test]
fn required_children_merge_across_occurrences_and_enforcing_merges() {
    let (root, _) = schema();
    let first = len(1, &varint(1, 1));
    let second = len(1, &varint(2, 2));
    let wire = joined(&[first.clone(), second.clone()]);
    let parsed = DynamicMessage::parse_with(root.clone(), &wire).expect("merged initialization");
    initialized(child(&parsed, 1));
    let mut merged = DynamicMessage::new(root);
    merged
        .merge_from_bytes_dont_enforce_required(&first)
        .expect("partial child accepted explicitly");
    merged.merge_from_bytes(&second).expect("completed child");
    initialized(child(&merged, 1));
}

#[test]
fn enforcing_an_empty_merge_checks_preexisting_partial_children() {
    let (root, _) = schema();
    let mut parsed = DynamicMessage::new(root);
    parsed
        .merge_from_bytes_dont_enforce_required(&len(1, &varint(1, 1)))
        .expect("partial parse");
    assert!(parsed.merge_from_bytes(&[]).is_err());
    assert!(parsed.merge_from_bytes_dont_enforce_required(&[]).is_ok());
}

#[test]
fn non_enforcing_mutable_view_preserves_the_same_contract() {
    let (root, _) = schema();
    let mut parsed = DynamicMessage::new(root);
    parsed
        .as_mut()
        .merge_from_bytes_dont_enforce_required(&len(1, &varint(1, 1)))
        .expect("partial mutable-view merge");
    assert!(parsed.as_mut().merge_from_bytes(&[]).is_err());
}

#[test]
fn same_oneof_message_case_merges_but_a_different_case_replaces_it() {
    let (root, _) = schema();
    let split = joined(&[len(3, &varint(1, 1)), len(3, &varint(2, 2))]);
    let parsed =
        DynamicMessage::parse_with(root.clone(), &split).expect("same-case initialization");
    initialized(child(&parsed, 3));
    let replaced = joined(&[len(3, &varint(1, 1)), varint(4, 7)]);
    let parsed = DynamicMessage::parse_with(root.clone(), &replaced)
        .expect("discarded child has no required obligation");
    assert!(!parsed.has(3));
    assert_eq!(parsed.get_singular(4), Some(&Value::Int32(7)));
    let switched_back = joined(&[len(3, &varint(1, 1)), varint(4, 7), len(3, &varint(2, 2))]);
    assert!(DynamicMessage::parse_with(root, &switched_back).is_err());
}

#[test]
fn nested_oneof_replacement_survives_singular_message_merge() {
    let (root, _) = schema();
    let first = joined(&[varint(1, 1), varint(3, 7)]);
    let second = joined(&[varint(2, 2), varint(4, 9)]);
    let parsed = DynamicMessage::parse_with(root, &joined(&[len(1, &first), len(1, &second)]))
        .expect("merged child");
    let inner = child(&parsed, 1);
    initialized(inner);
    assert!(!inner.has(3));
    assert_eq!(inner.get_singular(4), Some(&Value::Int32(9)));
}

#[test]
fn merge_from_dynamic_clears_replaced_oneof_members_recursively() {
    let (root, _) = schema();
    let full = joined(&[varint(1, 1), varint(2, 2)]);
    let mut destination = DynamicMessage::parse_with(
        root.clone(),
        &joined(&[
            len(1, &joined(&[full.clone(), varint(3, 7)])),
            len(3, &full),
        ]),
    )
    .unwrap();
    let source = DynamicMessage::parse_with(
        root,
        &joined(&[len(1, &joined(&[full, varint(4, 9)])), varint(4, 8)]),
    )
    .unwrap();
    destination.merge_from(source.as_view());
    assert!(!destination.has(3));
    assert_eq!(destination.get_singular(4), Some(&Value::Int32(8)));
    assert!(!child(&destination, 1).has(3));
    assert_eq!(
        child(&destination, 1).get_singular(4),
        Some(&Value::Int32(9))
    );
}

#[test]
fn repeated_children_are_checked_independently() {
    let (root, _) = schema();
    let wire = joined(&[len(2, &varint(1, 1)), len(2, &varint(2, 2))]);
    assert!(DynamicMessage::parse_with(root.clone(), &wire).is_err());
    let mut partial = DynamicMessage::new(root);
    partial
        .merge_from_bytes_dont_enforce_required(&wire)
        .expect("non-enforcing repeated children");
    assert_eq!(partial.get_repeated(2).unwrap().len(), 2);
    assert!(partial.merge_from_bytes(&[]).is_err());
}

#[test]
fn group_children_merge_and_missing_required_fields_are_rejected() {
    let (root, _) = schema();
    let parsed = DynamicMessage::parse_with(
        root.clone(),
        &joined(&[group(5, &varint(1, 1)), group(5, &varint(2, 2))]),
    )
    .unwrap();
    initialized(child(&parsed, 5));
    assert!(DynamicMessage::parse_with(root.clone(), &group(5, &varint(1, 1))).is_err());
    assert!(DynamicMessage::parse_with(root.clone(), &[0x2b, 0x08, 0x01]).is_err());
    assert!(DynamicMessage::parse_with(root, &[0x2b, 0x08, 0x01, 0x34]).is_err());
}

#[test]
fn maps_merge_within_entries_and_replace_across_entries() {
    let (root, _) = schema();
    let complete = joined(&[varint(1, 1), varint(2, 2)]);
    let split_entry = joined(&[varint(1, 7), len(2, &varint(1, 1)), len(2, &varint(2, 2))]);
    let parsed = DynamicMessage::parse_with(root.clone(), &len(6, &split_entry))
        .expect("map value merges within one entry");
    match parsed
        .get_map(6)
        .unwrap()
        .get(&MapKeyValue::I32(7))
        .unwrap()
    {
        Value::Message(inner) => initialized(inner),
        _ => panic!("message map value"),
    }
    let partial_entry = joined(&[varint(1, 7), len(2, &varint(1, 1))]);
    let full_entry = joined(&[varint(1, 7), len(2, &complete)]);
    assert!(
        DynamicMessage::parse_with(
            root.clone(),
            &joined(&[len(6, &partial_entry), len(6, &full_entry)])
        )
        .is_ok()
    );
    assert!(
        DynamicMessage::parse_with(
            root.clone(),
            &joined(&[len(6, &full_entry), len(6, &partial_entry)])
        )
        .is_err()
    );
    let malformed_entry = joined(&[varint(1, 7), len(2, &[0x08, 0x80])]);
    assert!(
        DynamicMessage::parse_with(
            root.clone(),
            &joined(&[len(6, &malformed_entry), len(6, &full_entry)])
        )
        .is_err(),
        "replacement cannot hide a malformed wire value"
    );
    assert!(
        DynamicMessage::parse_with(root, &len(6, &varint(1, 7))).is_err(),
        "default message value is uninitialized"
    );
}

#[test]
fn message_set_extensions_merge_and_are_checked_after_all_items() {
    let (_, inner) = schema();
    let mut descriptor = MessageDescriptor::builder("dynamic_required.MessageSet")
        .field(child_field("extension", 100, &inner))
        .build();
    descriptor.message_set_wire_format = true;
    let descriptor = Arc::new(descriptor);
    let item = |bytes: &[u8]| group(1, &joined(&[varint(2, 100), len(3, bytes)]));
    let parsed = DynamicMessage::parse_with(
        descriptor.clone(),
        &joined(&[item(&varint(1, 1)), item(&varint(2, 2))]),
    )
    .expect("merged extension");
    initialized(child(&parsed, 100));
    assert!(DynamicMessage::parse_with(descriptor.clone(), &item(&varint(1, 1))).is_err());
    let payload_first = |bytes: &[u8]| group(1, &joined(&[len(3, bytes), varint(2, 100)]));
    let parsed = DynamicMessage::parse_with(
        descriptor.clone(),
        &joined(&[payload_first(&varint(1, 1)), payload_first(&varint(2, 2))]),
    )
    .expect("payload may precede the extension number");
    initialized(child(&parsed, 100));
    assert!(
        DynamicMessage::parse_with(
            descriptor.clone(),
            &joined(&[
                item(&[0x08, 0x80]),
                item(&joined(&[varint(1, 1), varint(2, 2)])),
            ]),
        )
        .is_err()
    );
    let unknown = group(1, &joined(&[varint(2, 101), len(3, &[0x08, 0x80])]));
    let parsed = DynamicMessage::parse_with(descriptor, &unknown)
        .expect("unknown MessageSet payload is opaque");
    assert_eq!(parsed.serialize().unwrap(), unknown);
}

#[test]
fn message_set_uses_first_payload_and_ignores_items_without_one() {
    let (_, inner) = schema();
    let mut descriptor = MessageDescriptor::builder("dynamic_required.MessageSetFirst")
        .field(child_field("extension", 100, &inner))
        .build();
    descriptor.message_set_wire_format = true;
    let descriptor = Arc::new(descriptor);
    let complete = joined(&[varint(1, 1), varint(2, 2)]);
    let item = |first: &[u8], second: &[u8]| {
        group(1, &joined(&[varint(2, 100), len(3, first), len(3, second)]))
    };
    // protoc's C++ MessageSet parser retains the first payload in an item.
    for first in [varint(1, 1), Vec::new()] {
        let wire = item(&first, &complete);
        assert!(DynamicMessage::parse_with(descriptor.clone(), &wire).is_err());
        let mut partial = DynamicMessage::new(descriptor.clone());
        partial
            .merge_from_bytes_dont_enforce_required(&wire)
            .unwrap();
        assert!(!child(&partial, 100).has(2));
    }
    assert!(
        DynamicMessage::parse_with(descriptor.clone(), &item(&[0x08, 0x80], &complete)).is_err()
    );
    let parsed = DynamicMessage::parse_with(descriptor.clone(), &item(&complete, &[0x08, 0x80]))
        .expect("later payload contents are opaque");
    initialized(child(&parsed, 100));

    let payload_first = group(
        1,
        &joined(&[len(3, &varint(1, 1)), len(3, &complete), varint(2, 100)]),
    );
    assert!(DynamicMessage::parse_with(descriptor.clone(), &payload_first).is_err());
    for prefix in [Vec::new(), len(3, &complete)] {
        let duplicate_id = group(
            1,
            &joined(&[prefix, varint(2, 100), varint(2, 101), len(3, &complete)]),
        );
        let parsed = DynamicMessage::parse_with(descriptor.clone(), &duplicate_id)
            .expect("the first extension number wins");
        initialized(child(&parsed, 100));
    }
    let unknown_id_first = group(
        1,
        &joined(&[varint(2, 101), varint(2, 100), len(3, &complete)]),
    );
    let parsed = DynamicMessage::parse_with(descriptor.clone(), &unknown_id_first).unwrap();
    assert!(!parsed.has(100));
    assert_eq!(
        parsed.serialize().unwrap(),
        group(1, &joined(&[varint(2, 101), len(3, &complete)]))
    );
    for type_ids in [
        varint(2, 0),
        joined(&[varint(2, 0), varint(2, 100)]),
        joined(&[varint(2, 100), varint(2, 0)]),
    ] {
        for payload in [Vec::new(), len(3, &complete)] {
            assert!(
                DynamicMessage::parse_with(
                    descriptor.clone(),
                    &group(1, &joined(&[type_ids.clone(), payload]))
                )
                .is_err(),
                "every explicit zero extension number is invalid"
            );
        }
    }
    let parsed = DynamicMessage::parse_with(descriptor.clone(), &group(1, &len(3, &complete)))
        .expect("a payload without an extension number is ignored");
    assert_eq!(parsed.serialize().unwrap(), Vec::<u8>::new());
    for type_id in [100, 101] {
        let parsed = DynamicMessage::parse_with(descriptor.clone(), &group(1, &varint(2, type_id)))
            .expect("an item with no payload has no message value");
        assert_eq!(parsed.serialize().unwrap(), Vec::<u8>::new());
    }
    let unknown_first = group(1, &joined(&[varint(2, 101), len(3, &varint(1, 1))]));
    let unknown_duplicate = group(
        1,
        &joined(&[varint(2, 101), len(3, &varint(1, 1)), len(3, &varint(2, 2))]),
    );
    let parsed = DynamicMessage::parse_with(descriptor.clone(), &unknown_duplicate).unwrap();
    assert_eq!(parsed.serialize().unwrap(), unknown_first);
    let malformed_length = group(
        1,
        &joined(&[varint(2, 100), len(3, &complete), vec![0x1a, 0x80]]),
    );
    assert!(DynamicMessage::parse_with(descriptor, &malformed_length).is_err());
}

#[test]
fn structural_failure_discards_new_children_and_keeps_oneof_siblings() {
    let (root, _) = schema();
    let mut parsed = DynamicMessage::parse_with(root, &varint(4, 7)).unwrap();
    assert!(parsed.merge_from_bytes(&len(3, &[0x08, 0x80])).is_err());
    assert!(!parsed.has(3));
    assert_eq!(parsed.get_singular(4), Some(&Value::Int32(7)));
    assert!(parsed.merge_from_bytes(&[0x2b, 0x08, 0x80]).is_err());
    assert!(!parsed.has(5));
    assert!(parsed.merge_from_bytes(&[]).is_ok());
}

#[test]
fn deferred_initialization_keeps_utf8_and_required_root_checks() {
    let (root, _) = schema();
    let wire = len(1, &joined(&[varint(1, 1), varint(2, 2), len(6, &[0xff])]));
    assert!(DynamicMessage::parse_with(root.clone(), &wire).is_err());
    assert!(
        DynamicMessage::new(root)
            .merge_from_bytes_dont_enforce_required(&wire)
            .is_err()
    );

    let required_root = Arc::new(
        MessageDescriptor::builder("dynamic_required.RequiredRoot")
            .field(scalar("required", 1, true))
            .build(),
    );
    assert!(DynamicMessage::parse_with(required_root.clone(), &[]).is_err());
    assert!(DynamicMessage::parse_with(required_root.clone(), &len(1, &[])).is_err());
    assert!(DynamicMessage::parse_with(required_root, &varint(1, 0)).is_ok());
}

#[test]
fn malformed_wire_and_unknown_groups_remain_structural_errors() {
    let (root, _) = schema();
    for wire in [
        vec![0x0a, 2, 0x08],
        vec![0x0a, 2, 0x08, 0x80],
        vec![0xfb, 0x03, 0xfc, 0x02],
    ] {
        assert!(DynamicMessage::parse_with(root.clone(), &wire).is_err());
        let mut partial = DynamicMessage::new(root.clone());
        assert!(
            partial
                .merge_from_bytes_dont_enforce_required(&wire)
                .is_err()
        );
    }
    let opaque = len(99, &[0x08, 0x80]);
    let parsed = DynamicMessage::parse_with(root.clone(), &opaque).expect("unknown LEN is opaque");
    assert_eq!(parsed.serialize().unwrap(), opaque);
    let with_unknown = joined(&[
        len(1, &joined(&[varint(1, 1), varint(90, 3)])),
        len(1, &varint(2, 2)),
    ]);
    let parsed = DynamicMessage::parse_with(root, &with_unknown)
        .expect("unknown child fields survive merge");
    assert_eq!(child(&parsed, 1).unknown_fields().fields.iter().count(), 1);
}

#[test]
fn implicit_map_message_defaults_do_not_consume_wire_recursion() {
    let mut recursive = FieldDescriptor::new(
        "child",
        1,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    recursive.type_name = Some("dynamic_required.Recursive".into());
    let mut value = recursive.clone();
    value.name = "value".into();
    value.json_name = "value".into();
    value.number = 2;
    let mut pool = DescriptorPool::new();
    let entry = pool.register_message(
        MessageDescriptor::builder("dynamic_required.Recursive.Entry")
            .map_entry(true)
            .field(scalar("key", 1, false))
            .field(value)
            .build(),
    );
    let mut map = child_field("entries", 2, &entry);
    map.cardinality = Cardinality::Repeated;
    map.is_map = true;
    let root = pool.register_message(
        MessageDescriptor::builder("dynamic_required.Recursive")
            .field(recursive)
            .field(map)
            .build(),
    );
    let pool = Arc::new(pool);
    // These outcomes also match protoc's C++ --decode parser: the map entry
    // consumes level 100, while only an explicit value consumes level 101.
    for (wrappers, explicit_value, accepted) in [
        (98, false, true),
        (98, true, true),
        (99, false, true),
        (99, true, false),
        (100, false, false),
    ] {
        let mut entry = varint(1, 7);
        if explicit_value {
            entry.extend(len(2, &[]));
        }
        let mut wire = len(2, &entry);
        for _ in 0..wrappers {
            wire = len(1, &wire);
        }
        let parsed = DynamicMessage::parse_with_pool(root.clone(), Some(pool.clone()), &wire);
        assert_eq!(
            parsed.is_ok(),
            accepted,
            "wrappers={wrappers}, explicit_value={explicit_value}"
        );
    }
}

#[test]
fn pooled_types_and_recursion_limits_follow_final_tree_validation() {
    let (_, inner) = schema();
    let mut pool = DescriptorPool::new();
    pool.register_message((*inner).clone());
    let mut field = child_field("child", 1, &inner);
    field.message = None;
    field.type_name = Some(inner.full_name.clone());
    let root = pool.register_message(
        MessageDescriptor::builder("dynamic_required.Pooled")
            .field(field)
            .build(),
    );
    let pool = Arc::new(pool);
    let parsed = DynamicMessage::parse_with_pool(
        root,
        Some(pool),
        &joined(&[len(1, &varint(1, 1)), len(1, &varint(2, 2))]),
    )
    .unwrap();
    initialized(child(&parsed, 1));

    let (_, mut desc) = schema();
    let mut wire = joined(&[varint(1, 1), varint(2, 2)]);
    for level in 1..=101 {
        desc = Arc::new(
            MessageDescriptor::builder(format!("dynamic_required.Level{level}"))
                .field(child_field("child", 1, &desc))
                .build(),
        );
        wire = len(1, &wire);
        let result = DynamicMessage::parse_with(desc.clone(), &wire);
        assert_eq!(result.is_ok(), level <= 100, "depth {level}");
    }
}
