use pbrs::{
    Cardinality, DynamicMessage, FieldDescriptor, FieldType, MessageDescriptor, Presence,
    Serialize, Value,
};
use std::sync::Arc;

fn person_desc() -> Arc<MessageDescriptor> {
    Arc::new(
        MessageDescriptor::builder("example.Person")
            .field(FieldDescriptor::new(
                "id",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .field({
                let mut f = FieldDescriptor::new(
                    "name",
                    2,
                    FieldType::String,
                    Cardinality::Optional,
                    Presence::Implicit,
                );
                f.json_name = "name".into();
                f
            })
            .field({
                let mut f = FieldDescriptor::new(
                    "email",
                    3,
                    FieldType::String,
                    Cardinality::Optional,
                    Presence::Explicit,
                );
                f.json_name = "email".into();
                f
            })
            .build(),
    )
}

fn ext_host_desc() -> Arc<MessageDescriptor> {
    let mut d = MessageDescriptor::builder("example.Host")
        .field(FieldDescriptor::new(
            "id",
            1,
            FieldType::Int32,
            Cardinality::Optional,
            Presence::Implicit,
        ))
        .build();
    d.extension_ranges.push((100, 200));
    Arc::new(d)
}

#[test]
fn json_roundtrip_shipped_api() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.set(1, Value::Int32(7));
    msg.set(2, Value::String("ada".into()));
    msg.set(3, Value::String("a@b".into()));
    let json = msg.to_json().expect("json encode");
    assert!(json.contains("\"id\":7"), "{json}");
    assert!(json.contains("\"name\":\"ada\""), "{json}");
    let parsed = DynamicMessage::from_json(person_desc(), &json).expect("json decode");
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(7)));
    match parsed.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "ada"),
        other => panic!("{other:?}"),
    }
    assert_eq!(parsed.serialize().unwrap(), msg.serialize().unwrap());
}

#[test]
fn text_roundtrip_shipped_api() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.set(1, Value::Int32(3));
    msg.set(2, Value::String("x".into()));
    let text = msg.to_text().expect("text encode");
    assert!(text.contains("id: 3"), "{text}");
    assert!(text.contains("name:"), "{text}");
    let parsed = DynamicMessage::from_text(person_desc(), &text).expect("text decode");
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(3)));
    assert_eq!(parsed.serialize().unwrap(), msg.serialize().unwrap());
}

#[test]
fn extension_get_set_roundtrip() {
    let mut msg = DynamicMessage::new(ext_host_desc());
    msg.set(1, Value::Int32(1));
    assert!(!msg.has_extension(101));
    msg.set_extension(101, Value::Int32(99));
    assert!(msg.has_extension(101));
    assert_eq!(msg.get_extension(101), Some(&Value::Int32(99)));
    let bytes = msg.serialize().unwrap();
    let parsed = DynamicMessage::parse_with(ext_host_desc(), &bytes).unwrap();
    // unregistered extension number 101 is preserved as unknown and re-encoded
    assert_eq!(parsed.serialize().unwrap(), bytes);
    msg.clear_extension(101);
    assert!(!msg.has_extension(101));
}

#[test]
fn editions_explicit_presence_zero() {
    // editions 2023 default: explicit presence — 0 is serialized
    let mut f = FieldDescriptor::new(
        "count",
        1,
        FieldType::Int32,
        Cardinality::Optional,
        Presence::Explicit,
    );
    f.json_name = "count".into();
    let desc = Arc::new(MessageDescriptor::builder("ed.Msg").field(f).build());
    let mut msg = DynamicMessage::new(desc.clone());
    msg.set(1, Value::Int32(0));
    assert_eq!(msg.serialize().unwrap(), vec![0x08, 0x00]);
    let json = msg.to_json().unwrap();
    assert!(json.contains("\"count\":0"), "{json}");
}
