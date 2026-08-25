//! DynamicMessage and FileDescriptorSet bootstrap.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]
use pbrs::prelude::*;
use pbrs::testdata::Person;
use pbrs::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, MapKeyValue,
    MessageDescriptor, Presence, Serialize, Value,
};

fn person_desc() -> std::sync::Arc<MessageDescriptor> {
    let address = std::sync::Arc::new(
        MessageDescriptor::builder("example.Address")
            .field(FieldDescriptor::new(
                "city",
                1,
                FieldType::String,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .build(),
    );
    let mut city = FieldDescriptor::new(
        "address",
        6,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    city.message = Some(address);

    let scores_entry = MessageDescriptor::builder("example.Person.ScoresEntry")
        .map_entry(true)
        .field(FieldDescriptor::new(
            "key",
            1,
            FieldType::String,
            Cardinality::Optional,
            Presence::Implicit,
        ))
        .field(FieldDescriptor::new(
            "value",
            2,
            FieldType::Int32,
            Cardinality::Optional,
            Presence::Implicit,
        ))
        .build();
    let scores_entry = std::sync::Arc::new(scores_entry);
    let mut scores = FieldDescriptor::new(
        "scores",
        5,
        FieldType::Message,
        Cardinality::Repeated,
        Presence::Explicit,
    );
    scores.is_map = true;
    scores.packed = false;
    scores.message = Some(scores_entry);

    std::sync::Arc::new(
        MessageDescriptor::builder("example.Person")
            .field(FieldDescriptor::new(
                "id",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .field(FieldDescriptor::new(
                "name",
                2,
                FieldType::String,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .field(FieldDescriptor::new(
                "email",
                3,
                FieldType::String,
                Cardinality::Optional,
                Presence::Explicit,
            ))
            .field(FieldDescriptor::new(
                "tags",
                4,
                FieldType::String,
                Cardinality::Repeated,
                Presence::Explicit,
            ))
            .field(scores)
            .field(city)
            .build(),
    )
}

#[test]
fn dynamic_matches_typed_wire() {
    let typed = proto!(Person {
        id: 1,
        name: "ada",
        email: "ada@ex",
    });
    let typed_bytes = typed.serialize().unwrap();

    let mut dyn_msg = DynamicMessage::new(person_desc());
    dyn_msg.set(1, Value::Int32(1));
    dyn_msg.set(2, Value::String("ada".into()));
    dyn_msg.set(3, Value::String("ada@ex".into()));
    let dyn_bytes = dyn_msg.serialize().unwrap();
    assert_eq!(dyn_bytes, typed_bytes);

    let parsed = DynamicMessage::parse_with(person_desc(), &typed_bytes).unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(1)));
    match parsed.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "ada"),
        other => panic!("bad name {other:?}"),
    }
}

#[test]
fn dynamic_message_trait_parse() {
    let bytes = proto!(Person { id: 4, name: "x" }).serialize().unwrap();
    // Default DynamicMessage has an empty descriptor; parse_with is the real entry.
    let msg = DynamicMessage::parse_with(person_desc(), &bytes).unwrap();
    assert_eq!(msg.get_singular(1), Some(&Value::Int32(4)));
    let _ = msg.as_view();
}

#[test]
fn dynamic_unknown_and_repeated() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.push(4, Value::String("a".into()));
    msg.push(4, Value::String("b".into()));
    let bytes = msg.serialize().unwrap();
    let parsed = DynamicMessage::parse_with(person_desc(), &bytes).unwrap();
    let tags = parsed.get_repeated(4).unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn dynamic_map() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.insert_map(5, MapKeyValue::String("k".into()), Value::Int32(3));
    let bytes = msg.serialize().unwrap();
    let parsed = DynamicMessage::parse_with(person_desc(), &bytes).unwrap();
    let map = parsed.get_map(5).unwrap();
    assert_eq!(
        map.get(&MapKeyValue::String("k".into())),
        Some(&Value::Int32(3))
    );
}

#[test]
fn file_descriptor_set_bootstrap() {
    // Hand-rolled FileDescriptorSet for:
    //   syntax = "proto3";
    //   package example;
    //   message Mini { int32 id = 1; }
    let mut fds = Vec::new();
    let mut file = Vec::new();
    // package = "example" (field 2)
    protobuf_test_encode_string(&mut file, 2, "example");
    // syntax = "proto3" (field 12)
    protobuf_test_encode_string(&mut file, 12, "proto3");
    // message_type (field 4)
    let mut msg = Vec::new();
    protobuf_test_encode_string(&mut msg, 1, "Mini");
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "id");
    protobuf_test_encode_varint(&mut field, 3, 1); // number
    protobuf_test_encode_varint(&mut field, 4, 1); // LABEL_OPTIONAL
    protobuf_test_encode_varint(&mut field, 5, 5); // TYPE_INT32
    protobuf_test_encode_len(&mut msg, 2, &field);
    protobuf_test_encode_len(&mut file, 4, &msg);
    protobuf_test_encode_len(&mut fds, 1, &file);

    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("fds");
    let desc = pool.get_message("example.Mini").expect("example.Mini");
    let mut dyn_msg = DynamicMessage::new(desc);
    dyn_msg.set(1, Value::Int32(42));
    let bytes = dyn_msg.serialize().unwrap();
    assert_eq!(bytes, vec![0x08, 42]);
    let parsed =
        DynamicMessage::parse_with(pool.get_message("example.Mini").unwrap(), &bytes).unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(42)));
}

#[test]
fn file_descriptor_set_keeps_custom_options() {
    // FileDescriptorProto.options is field 8 (FileOptions).
    // DescriptorProto.options is field 7 (MessageOptions).
    // FieldDescriptorProto.options is field 8 (FieldOptions).
    // EnumDescriptorProto.options is field 3 (EnumOptions).
    // MethodDescriptorProto.options is field 4 (MethodOptions).
    // Tags below are not standard option fields; they must survive
    // from_file_descriptor_set instead of being skip_field'd.
    let mut fds = Vec::new();
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "example.proto");
    protobuf_test_encode_string(&mut file, 2, "example");
    protobuf_test_encode_string(&mut file, 12, "proto3");
    let mut msg = Vec::new();
    protobuf_test_encode_string(&mut msg, 1, "Mini");
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "id");
    protobuf_test_encode_varint(&mut field, 3, 1);
    protobuf_test_encode_varint(&mut field, 4, 1);
    protobuf_test_encode_varint(&mut field, 5, 5);
    let mut field_opts = Vec::new();
    protobuf_test_encode_len(&mut field_opts, 9999, b"xyz");
    protobuf_test_encode_len(&mut field, 8, &field_opts);
    protobuf_test_encode_len(&mut msg, 2, &field);
    let mut msg_opts = Vec::new();
    protobuf_test_encode_len(&mut msg_opts, 51206, b"abc");
    protobuf_test_encode_len(&mut msg, 7, &msg_opts);
    protobuf_test_encode_len(&mut file, 4, &msg);
    let mut enum_ty = Vec::new();
    protobuf_test_encode_string(&mut enum_ty, 1, "Kind");
    let mut enum_val = Vec::new();
    protobuf_test_encode_string(&mut enum_val, 1, "ZERO");
    protobuf_test_encode_varint(&mut enum_val, 2, 0);
    protobuf_test_encode_len(&mut enum_ty, 2, &enum_val);
    let mut enum_opts = Vec::new();
    protobuf_test_encode_len(&mut enum_opts, 50002, b"en");
    protobuf_test_encode_len(&mut enum_ty, 3, &enum_opts);
    protobuf_test_encode_len(&mut file, 5, &enum_ty);
    let mut method = Vec::new();
    protobuf_test_encode_string(&mut method, 1, "Ping");
    protobuf_test_encode_string(&mut method, 2, ".example.Mini");
    protobuf_test_encode_string(&mut method, 3, ".example.Mini");
    let mut method_opts = Vec::new();
    protobuf_test_encode_len(&mut method_opts, 50003, b"mn");
    protobuf_test_encode_len(&mut method, 4, &method_opts);
    let mut service = Vec::new();
    protobuf_test_encode_string(&mut service, 1, "Greeter");
    protobuf_test_encode_len(&mut service, 2, &method);
    protobuf_test_encode_len(&mut file, 6, &service);
    let mut file_opts = Vec::new();
    protobuf_test_encode_len(&mut file_opts, 50001, b"fl");
    protobuf_test_encode_len(&mut file, 8, &file_opts);
    protobuf_test_encode_len(&mut fds, 1, &file);

    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("fds");
    let desc = pool.get_message("example.Mini").expect("example.Mini");
    assert_eq!(desc.custom_option(51206), Some(b"abc".as_slice()));
    let id = desc.field(1).expect("id");
    assert_eq!(id.custom_option(9999), Some(b"xyz".as_slice()));
    let file_desc = pool.get_file("example.proto").expect("example.proto");
    assert_eq!(file_desc.custom_option(50001), Some(b"fl".as_slice()));
    let kind = pool.get_enum("example.Kind").expect("example.Kind");
    assert_eq!(kind.custom_option(50002), Some(b"en".as_slice()));
    let svc = pool
        .get_service("example.Greeter")
        .expect("example.Greeter");
    assert_eq!(svc.methods.len(), 1);
    assert_eq!(svc.methods[0].custom_option(50003), Some(b"mn".as_slice()));
}

fn protobuf_test_encode_varint(out: &mut Vec<u8>, number: u32, value: u64) {
    encode_tag_for_test(out, number, 0);
    encode_varint_for_test(out, value);
}

fn protobuf_test_encode_string(out: &mut Vec<u8>, number: u32, s: &str) {
    protobuf_test_encode_len(out, number, s.as_bytes());
}

fn protobuf_test_encode_len(out: &mut Vec<u8>, number: u32, payload: &[u8]) {
    encode_tag_for_test(out, number, 2);
    encode_varint_for_test(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn encode_tag_for_test(out: &mut Vec<u8>, number: u32, wire: u32) {
    encode_varint_for_test(out, u64::from((number << 3) | wire));
}

fn encode_varint_for_test(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut b = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            b |= 0x80;
        }
        out.push(b);
        if value == 0 {
            break;
        }
    }
}
