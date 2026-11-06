//! Pool-backed JSON must resolve nested messages through canonical descriptors.
#![cfg(feature = "json")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "small schema and JSON fixtures with detecting assertions"
)]

use pbrs::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, MapKeyValue,
    MessageDescriptor, Presence, Serialize, Value,
};
use std::sync::Arc;

fn pool() -> Arc<DescriptorPool> {
    Arc::new(
        DescriptorPool::from_file_descriptor_set(include_bytes!(
            "fixtures/json_nested_descriptors.fds"
        ))
        .unwrap(),
    )
}

fn decode(json: &str, ignore_unknown: bool) -> Result<DynamicMessage, pbrs::ParseError> {
    let pool = pool();
    DynamicMessage::from_json_with_pool(
        pool.get_message("json_nested.Parent").unwrap(),
        Some(pool),
        json,
        ignore_unknown,
    )
}

fn message(value: &Value) -> &DynamicMessage {
    let Value::Message(message) = value else {
        panic!("expected message value")
    };
    message
}

fn map_value<'a>(message: &'a DynamicMessage, field: u32, key: &str) -> &'a Value {
    message
        .get_map(field)
        .unwrap()
        .get(&MapKeyValue::String(key.into()))
        .unwrap()
}

fn assert_roundtrip(parsed: &DynamicMessage) {
    let json = parsed.to_json().unwrap();
    assert_eq!(decode(&json, false).unwrap(), *parsed);
    let pool = pool();
    assert_eq!(
        DynamicMessage::parse_with_pool(
            pool.get_message("json_nested.Parent").unwrap(),
            Some(pool),
            &parsed.serialize().unwrap(),
        )
        .unwrap(),
        *parsed
    );
}

#[test]
fn nested_maps_resolve_singular_repeated_map_and_oneof_children() {
    for json in [
        r#"{"child":{"numbers":{"x":7}}}"#,
        r#"{"children":[{"numbers":{"x":7}}]}"#,
        r#"{"byName":{"x":{"numbers":{"x":7}}}}"#,
        r#"{"selected":{"numbers":{"x":7}}}"#,
    ] {
        assert_roundtrip(&decode(json, false).unwrap());
    }
    let parsed = decode(r#"{"child":{"numbers":{"x":7}}}"#, false).unwrap();
    let child = message(parsed.get_singular(1).unwrap());
    assert_eq!(map_value(child, 1, "x"), &Value::Int32(7));
    assert!(child.descriptor().field(1).unwrap().is_map);
}

#[test]
fn recursive_and_imported_map_values_use_resolved_children() {
    let parsed = decode(
        r#"{"child":{"descendants":{"left":{"descendants":{"right":{"numbers":{"x":11}}}}},"imported":{"foreign":{"numbers":{"y":13}}},"states":{"ready":"READY"}}}"#,
        false,
    )
    .unwrap();
    let child = message(parsed.get_singular(1).unwrap());
    let left = message(map_value(child, 2, "left"));
    let right = message(map_value(left, 2, "right"));
    assert_eq!(map_value(right, 1, "x"), &Value::Int32(11));
    let imported = message(map_value(child, 3, "foreign"));
    assert_eq!(map_value(imported, 1, "y"), &Value::Int32(13));
    assert_eq!(map_value(child, 4, "ready"), &Value::Enum(1));
    assert_roundtrip(&parsed);
}

#[test]
fn nested_json_keeps_unknown_duplicate_oneof_and_enum_policies() {
    let unknown = r#"{"child":{"numbers":{"x":7},"extra":1}}"#;
    assert!(decode(unknown, false).is_err());
    let ignored = decode(unknown, true).unwrap();
    let child = message(ignored.get_singular(1).unwrap());
    assert_eq!(map_value(child, 1, "x"), &Value::Int32(7));
    assert!(decode(r#"{"child":{"numbers":{"x":1,"x":2}}}"#, false).is_err());
    assert!(decode(r#"{"selected":{},"scalar":1}"#, false).is_err());
    let unknown_enum = r#"{"child":{"states":{"x":"NO_SUCH_VALUE"}}}"#;
    assert!(decode(unknown_enum, false).is_err());
    assert!(decode(unknown_enum, true).is_ok());
}

#[test]
fn null_value_and_required_binary_validation_are_unchanged() {
    let parsed = decode(r#"{"child":null,"jsonValue":null}"#, false).unwrap();
    assert!(!parsed.has(1));
    let json_value = message(parsed.get_singular(6).unwrap());
    assert_eq!(json_value.get_singular(1), Some(&Value::Enum(0)));
    assert_roundtrip(&parsed);
    for (json, binary_accepts) in [
        (r#"{"child":{"requiredChild":{}}}"#, false),
        (r#"{"child":{"requiredChild":{"id":17}}}"#, true),
    ] {
        // Dynamic JSON construction permits partial messages. Binary parsing
        // and generated from_json's final Parse step enforce initialization.
        let parsed = decode(json, false).unwrap();
        let pool = pool();
        assert_eq!(
            DynamicMessage::parse_with_pool(
                pool.get_message("json_nested.Parent").unwrap(),
                Some(pool),
                &parsed.serialize().unwrap(),
            )
            .is_ok(),
            binary_accepts
        );
    }
}

#[test]
fn manually_linked_descriptors_work_without_a_pool() {
    let leaf = Arc::new(
        MessageDescriptor::builder("manual.Child")
            .field(FieldDescriptor::new(
                "number",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Explicit,
            ))
            .build(),
    );
    let mut field = FieldDescriptor::new(
        "child",
        1,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    field.message = Some(leaf);
    let parent = Arc::new(
        MessageDescriptor::builder("manual.Parent")
            .field(field)
            .build(),
    );
    let parsed = DynamicMessage::from_json(parent, r#"{"child":{"number":19}}"#).unwrap();
    assert_eq!(
        message(parsed.get_singular(1).unwrap()).get_singular(1),
        Some(&Value::Int32(19))
    );
}
