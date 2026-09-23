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
    Cardinality, DescriptorPool, DynamicMessage, EnumDescriptor, FieldDescriptor, FieldType,
    FileDescriptor, MapKeyValue, MessageDescriptor, Presence, Serialize, Value,
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
fn exhaustive_descriptor_literals_remain_external_source_compatible() {
    use std::collections::{BTreeMap, BTreeSet};

    let message = MessageDescriptor {
        name: "Legacy".into(),
        full_name: "example.Legacy".into(),
        fields: BTreeMap::new(),
        fields_by_name: BTreeMap::new(),
        is_map_entry: false,
        oneofs: Vec::new(),
        fields_by_json_name: BTreeMap::new(),
        extension_ranges: Vec::new(),
        reserved_names: BTreeSet::new(),
        file_name: "legacy.proto".into(),
        message_set_wire_format: false,
        options: Vec::new(),
        comments: pbrs::codegen::Comments::default(),
        deprecated: false,
    };
    let en = EnumDescriptor {
        name: "Kind".into(),
        full_name: "example.Kind".into(),
        file_name: "legacy.proto".into(),
        values: BTreeMap::new(),
        names: BTreeMap::new(),
        listed: Vec::new(),
        closed: false,
        options: Vec::new(),
        comments: pbrs::codegen::Comments::default(),
        value_comments: BTreeMap::new(),
        value_comments_by_name: BTreeMap::new(),
        deprecated: false,
        deprecated_values: BTreeSet::new(),
    };
    let file = FileDescriptor {
        name: "legacy.proto".into(),
        package: "example".into(),
        options: Vec::new(),
        source_code_info: None,
        comments: pbrs::codegen::Comments::default(),
        deprecated: false,
    };
    assert_eq!(message.file_name, en.file_name);
    assert_eq!(en.file_name, file.name);
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

fn encode_location_for_test(
    out: &mut Vec<u8>,
    path: &[i32],
    span: &[i32],
    leading_comments: Option<&str>,
    trailing_comments: Option<&str>,
    leading_detached: &[&str],
) {
    let mut loc = Vec::new();
    let mut path_bytes = Vec::new();
    for &p in path {
        encode_varint_for_test(&mut path_bytes, p as u64);
    }
    protobuf_test_encode_len(&mut loc, 1, &path_bytes);

    let mut span_bytes = Vec::new();
    for &s in span {
        encode_varint_for_test(&mut span_bytes, s as u64);
    }
    protobuf_test_encode_len(&mut loc, 2, &span_bytes);

    if let Some(lc) = leading_comments {
        protobuf_test_encode_string(&mut loc, 3, lc);
    }
    if let Some(tc) = trailing_comments {
        protobuf_test_encode_string(&mut loc, 4, tc);
    }
    for &ld in leading_detached {
        protobuf_test_encode_string(&mut loc, 6, ld);
    }

    protobuf_test_encode_len(out, 1, &loc);
}

fn build_source_info_file_and_fds() -> (Vec<u8>, Vec<u8>) {
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "test_source.proto");
    protobuf_test_encode_string(&mut file, 2, "pkg");
    protobuf_test_encode_string(&mut file, 12, "proto3");

    // FileOptions: field 8 (deprecated = 23, custom = 50001)
    let mut file_opts = Vec::new();
    protobuf_test_encode_varint(&mut file_opts, 23, 1);
    protobuf_test_encode_len(&mut file_opts, 50001, b"fl");
    protobuf_test_encode_len(&mut file, 8, &file_opts);

    // Message Outer: field 4 of file
    let mut outer = Vec::new();
    protobuf_test_encode_string(&mut outer, 1, "Outer");

    // MessageOptions: field 7 (deprecated = 3, custom = 51206)
    let mut msg_opts = Vec::new();
    protobuf_test_encode_varint(&mut msg_opts, 3, 1);
    protobuf_test_encode_len(&mut msg_opts, 51206, b"abc");
    protobuf_test_encode_len(&mut outer, 7, &msg_opts);

    // outer_field: field 2 of Outer (index 0)
    let mut f1 = Vec::new();
    protobuf_test_encode_string(&mut f1, 1, "outer_field");
    protobuf_test_encode_varint(&mut f1, 3, 1);
    protobuf_test_encode_varint(&mut f1, 4, 1);
    protobuf_test_encode_varint(&mut f1, 5, 9);
    let mut f1_opts = Vec::new();
    protobuf_test_encode_varint(&mut f1_opts, 3, 1);
    protobuf_test_encode_len(&mut f1_opts, 9999, b"xyz");
    protobuf_test_encode_len(&mut f1, 8, &f1_opts);
    protobuf_test_encode_len(&mut outer, 2, &f1);

    // Nested Message Inner: field 3 of Outer (index 0 in nested_type)
    let mut inner = Vec::new();
    protobuf_test_encode_string(&mut inner, 1, "Inner");
    let mut inner_f = Vec::new();
    protobuf_test_encode_string(&mut inner_f, 1, "inner_field");
    protobuf_test_encode_varint(&mut inner_f, 3, 1);
    protobuf_test_encode_varint(&mut inner_f, 4, 1);
    protobuf_test_encode_varint(&mut inner_f, 5, 5);
    protobuf_test_encode_len(&mut inner, 2, &inner_f);
    protobuf_test_encode_len(&mut outer, 3, &inner);

    // Nested Enum NestedEnum: field 4 of Outer (index 0 in enum_type)
    let mut nested_enum = Vec::new();
    protobuf_test_encode_string(&mut nested_enum, 1, "NestedEnum");
    let mut ne_val = Vec::new();
    protobuf_test_encode_string(&mut ne_val, 1, "NESTED_ZERO");
    protobuf_test_encode_varint(&mut ne_val, 2, 0);
    protobuf_test_encode_len(&mut nested_enum, 2, &ne_val);
    protobuf_test_encode_len(&mut outer, 4, &nested_enum);

    protobuf_test_encode_len(&mut file, 4, &outer);

    // TopEnum: field 5 of file (index 0 in enum_type)
    let mut top_enum = Vec::new();
    protobuf_test_encode_string(&mut top_enum, 1, "TopEnum");
    let mut enum_opts = Vec::new();
    protobuf_test_encode_varint(&mut enum_opts, 2, 1);
    protobuf_test_encode_len(&mut enum_opts, 50002, b"en");
    protobuf_test_encode_len(&mut top_enum, 3, &enum_opts);
    let mut ev0 = Vec::new();
    protobuf_test_encode_string(&mut ev0, 1, "ZERO");
    protobuf_test_encode_varint(&mut ev0, 2, 0);
    let mut ev0_opts = Vec::new();
    protobuf_test_encode_varint(&mut ev0_opts, 1, 1);
    protobuf_test_encode_len(&mut ev0, 3, &ev0_opts);
    protobuf_test_encode_len(&mut top_enum, 2, &ev0);
    let mut ev1 = Vec::new();
    protobuf_test_encode_string(&mut ev1, 1, "ONE");
    protobuf_test_encode_varint(&mut ev1, 2, 1);
    protobuf_test_encode_len(&mut top_enum, 2, &ev1);
    protobuf_test_encode_len(&mut file, 5, &top_enum);

    // Service: field 6 of file (index 0 in service)
    let mut svc = Vec::new();
    protobuf_test_encode_string(&mut svc, 1, "TestService");
    let mut svc_opts = Vec::new();
    protobuf_test_encode_varint(&mut svc_opts, 33, 1);
    protobuf_test_encode_len(&mut svc, 3, &svc_opts);
    let mut m = Vec::new();
    protobuf_test_encode_string(&mut m, 1, "DoSomething");
    protobuf_test_encode_string(&mut m, 2, ".pkg.Outer");
    protobuf_test_encode_string(&mut m, 3, ".pkg.Outer");
    let mut m_opts = Vec::new();
    protobuf_test_encode_varint(&mut m_opts, 33, 1);
    protobuf_test_encode_len(&mut m_opts, 50003, b"mn");
    protobuf_test_encode_len(&mut m, 4, &m_opts);
    protobuf_test_encode_len(&mut svc, 2, &m);
    protobuf_test_encode_len(&mut file, 6, &svc);

    // SourceCodeInfo: field 9 of file
    let mut sci = Vec::new();
    encode_location_for_test(
        &mut sci,
        &[],
        &[1, 0, 50, 0],
        Some(" File leading comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0],
        &[5, 0, 20, 1],
        Some(" Outer message comment\n"),
        Some(" Outer trailing comment"),
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0, 2, 0],
        &[7, 2, 7, 30],
        Some(" outer_field comment\n"),
        Some(" field trailing"),
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0, 3, 0],
        &[10, 2, 15, 2],
        Some(" Inner message comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0, 3, 0, 2, 0],
        &[12, 4, 12, 25],
        Some(" inner_field comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0, 4, 0],
        &[17, 2, 19, 2],
        Some(" NestedEnum comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[4, 0, 4, 0, 2, 0],
        &[18, 4, 18, 20],
        Some(" NESTED_ZERO comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[5, 0],
        &[22, 0, 26, 1],
        Some(" TopEnum comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[5, 0, 2, 0],
        &[23, 2, 23, 15],
        Some(" ZERO comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[5, 0, 2, 1],
        &[24, 2, 24, 15],
        Some(" ONE comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[6, 0],
        &[28, 0, 32, 1],
        Some(" TestService comment\n"),
        None,
        &[],
    );
    encode_location_for_test(
        &mut sci,
        &[6, 0, 2, 0],
        &[30, 2, 30, 45],
        Some(" DoSomething comment\n"),
        Some(" method trailing"),
        &[],
    );

    protobuf_test_encode_len(&mut file, 9, &sci);

    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &file);
    (file, fds)
}

#[test]
fn source_code_info_and_deprecation_preserved() {
    let (_file, fds) = build_source_info_file_and_fds();
    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("fds");

    // 1. FileDescriptor
    let file = pool.get_file("test_source.proto").expect("file");
    assert!(file.is_deprecated());
    assert_eq!(file.custom_option(50001), Some(b"fl".as_slice()));
    assert_eq!(file.leading_comments(), Some(" File leading comment\n"));
    assert!(file.source_code_info().is_some());
    let sci = file.source_code_info().unwrap();
    assert!(sci.find_location(&[4, 0]).is_some());
    assert_eq!(sci.find_location(&[4, 0]).unwrap().span, vec![5, 0, 20, 1]);

    // 2. Top-level message and field
    let outer = pool.get_message("pkg.Outer").expect("pkg.Outer");
    assert!(outer.is_deprecated());
    assert_eq!(outer.custom_option(51206), Some(b"abc".as_slice()));
    assert_eq!(outer.leading_comments(), Some(" Outer message comment\n"));
    assert_eq!(outer.trailing_comments(), Some(" Outer trailing comment"));
    assert_eq!(outer.span(), &[5, 0, 20, 1]);

    let f1 = outer.field(1).expect("outer_field");
    assert!(f1.is_deprecated());
    assert_eq!(f1.custom_option(9999), Some(b"xyz".as_slice()));
    assert_eq!(f1.leading_comments(), Some(" outer_field comment\n"));
    assert_eq!(f1.trailing_comments(), Some(" field trailing"));
    assert_eq!(f1.span(), &[7, 2, 7, 30]);

    // 3. Nested message and field
    let inner = pool
        .get_message("pkg.Outer.Inner")
        .expect("pkg.Outer.Inner");
    assert!(!inner.is_deprecated());
    assert_eq!(inner.leading_comments(), Some(" Inner message comment\n"));
    assert_eq!(inner.span(), &[10, 2, 15, 2]);

    let inner_f = inner.field(1).expect("inner_field");
    assert_eq!(inner_f.leading_comments(), Some(" inner_field comment\n"));
    assert_eq!(inner_f.span(), &[12, 4, 12, 25]);

    // 4. Nested enum and value
    let ne = pool.get_enum("pkg.Outer.NestedEnum").expect("NestedEnum");
    assert_eq!(ne.leading_comments(), Some(" NestedEnum comment\n"));
    assert_eq!(ne.span(), &[17, 2, 19, 2]);
    let ne_v0 = ne.value_comments(0).expect("NESTED_ZERO comments");
    assert_eq!(ne_v0.leading(), Some(" NESTED_ZERO comment\n"));

    // 5. Top-level enum and values
    let te = pool.get_enum("pkg.TopEnum").expect("TopEnum");
    assert!(te.is_deprecated());
    assert_eq!(te.custom_option(50002), Some(b"en".as_slice()));
    assert_eq!(te.leading_comments(), Some(" TopEnum comment\n"));
    assert_eq!(te.span(), &[22, 0, 26, 1]);
    assert!(te.is_value_deprecated(0));
    assert!(!te.is_value_deprecated(1));
    let te_v0 = te.value_comments(0).expect("ZERO comments");
    assert_eq!(te_v0.leading(), Some(" ZERO comment\n"));
    let te_v1 = te.value_comments(1).expect("ONE comments");
    assert_eq!(te_v1.leading(), Some(" ONE comment\n"));

    // 6. Service and method
    let svc = pool.get_service("pkg.TestService").expect("TestService");
    assert!(svc.is_deprecated());
    assert_eq!(svc.leading_comments(), Some(" TestService comment\n"));
    assert_eq!(svc.span(), &[28, 0, 32, 1]);
    assert_eq!(svc.methods.len(), 1);

    let m = &svc.methods[0];
    assert!(m.is_deprecated());
    assert_eq!(m.custom_option(50003), Some(b"mn".as_slice()));
    assert_eq!(m.leading_comments(), Some(" DoSomething comment\n"));
    assert_eq!(m.trailing_comments(), Some(" method trailing"));
    assert_eq!(m.span(), &[30, 2, 30, 45]);
}

#[test]
fn source_code_info_codegen_doc_comments() {
    let (file, _fds) = build_source_info_file_and_fds();

    let mut req = Vec::new();
    protobuf_test_encode_string(&mut req, 1, "test_source.proto");
    protobuf_test_encode_len(&mut req, 15, &file);

    let files = pbrs::codegen::generate_from_code_generator_request(&req)
        .expect("generate_from_code_generator_request");
    assert!(!files.is_empty());
    let (_filename, content) = files
        .iter()
        .find(|(name, _)| name.ends_with("test_source.rs"))
        .or_else(|| files.first())
        .expect("generated file content");

    // Message Outer doc comments and deprecation
    assert!(
        content.contains("/// Outer message comment"),
        "missing Outer doc comment: {content}"
    );
    assert!(
        content.contains("/// Outer trailing comment"),
        "missing Outer trailing doc: {content}"
    );
    assert!(
        content.contains("#[deprecated]\n#[derive(Clone, Debug)]\npub struct Outer"),
        "missing Outer #[deprecated]: {content}"
    );

    // Field outer_field doc comments and deprecation
    assert!(
        content.contains("/// outer_field comment"),
        "missing outer_field doc: {content}"
    );
    assert!(
        content.contains("/// field trailing"),
        "missing outer_field trailing doc: {content}"
    );
    assert!(
        content.contains("#[deprecated]\n    pub fn outer_field"),
        "missing outer_field #[deprecated]: {content}"
    );

    // Nested message Inner
    assert!(
        content.contains("/// Inner message comment"),
        "missing Inner doc: {content}"
    );
    assert!(
        content.contains("/// inner_field comment"),
        "missing inner_field doc: {content}"
    );

    // TopEnum doc comments and deprecation
    assert!(
        content.contains("/// TopEnum comment"),
        "missing TopEnum doc: {content}"
    );
    assert!(
        content.contains("#[deprecated]\n#[repr(transparent)]"),
        "missing TopEnum #[deprecated]: {content}"
    );
    assert!(
        content.contains("pub struct TopEnum(pub i32);"),
        "missing TopEnum struct: {content}"
    );
    assert!(
        content.contains("/// ZERO comment"),
        "missing ZERO doc: {content}"
    );
    assert!(
        content.contains("#[deprecated]\n    pub const Zero"),
        "missing ZERO #[deprecated]: {content}"
    );
    assert!(
        content.contains("/// ONE comment"),
        "missing ONE doc: {content}"
    );

    // NestedEnum doc comments
    assert!(
        content.contains("/// NestedEnum comment"),
        "missing NestedEnum doc: {content}"
    );
    assert!(
        content.contains("/// NESTED_ZERO comment"),
        "missing NESTED_ZERO doc: {content}"
    );

    // Service doc comments and deprecation
    assert!(
        content.contains("/// TestService comment"),
        "missing TestService doc: {content}"
    );
    assert!(
        content.contains("/// DoSomething comment"),
        "missing DoSomething doc: {content}"
    );
}

#[test]
fn codegen_config_include_source_info() {
    let mut config = pbrs::codegen::Config::new();
    config.include_source_info(true);
    config.preserve_comments(true);
}

fn edition2024_fds(name: &str) -> DescriptorPool {
    let path = format!("tests/fixtures/edition2024/fds/{name}.fds");
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .expect("pinned Edition 2024 descriptor set");
    DescriptorPool::from_file_descriptor_set(&bytes).expect("valid Edition 2024 descriptor set")
}

fn file_feature_values(pool: &DescriptorPool, name: &str) -> (i32, u32, u32, u32) {
    (
        pool.file_edition(name).expect("file edition"),
        pool.file_json_format(name).expect("file JSON format"),
        pool.file_naming_style(name).expect("file naming style"),
        pool.file_default_symbol_visibility(name)
            .expect("file default visibility"),
    )
}

fn symbol_feature_values(pool: &DescriptorPool, name: &str) -> (u32, u32, u32, u32) {
    (
        pool.symbol_json_format(name).expect("symbol JSON format"),
        pool.symbol_naming_style(name).expect("symbol naming style"),
        pool.declared_symbol_visibility(name)
            .expect("declared visibility"),
        pool.effective_symbol_visibility(name)
            .expect("effective visibility"),
    )
}

fn edition2024_wire(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/edition2024/bin/{name}.bin");
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .expect("pinned Edition 2024 wire vector")
}

#[test]
fn edition2024_field_defaults_and_wire_match_pinned_fixture() {
    let pool = std::sync::Arc::new(edition2024_fds("defaults"));
    let desc = pool
        .get_message("edition2024.defaults.DefaultMessage")
        .expect("DefaultMessage");
    assert_eq!(desc.field(1).expect("int32").presence, Presence::Explicit);
    assert_eq!(desc.field(5).expect("bool").presence, Presence::Explicit);
    assert!(desc.field(6).expect("string").utf8_validate);
    assert!(!desc.field(9).expect("submessage").delimited);
    assert!(desc.field(10).expect("repeated int32").packed);
    assert!(
        !pool
            .get_enum("edition2024.defaults.DefaultEnum")
            .expect("DefaultEnum")
            .closed
    );

    let empty = edition2024_wire("defaults_empty");
    let zero_set = edition2024_wire("defaults_zero_set");
    assert_eq!(
        DynamicMessage::parse_with(desc.clone(), &empty)
            .expect("empty")
            .serialize()
            .expect("serialize empty"),
        empty
    );
    let parsed = DynamicMessage::parse_with(desc.clone(), &zero_set).expect("explicit zero");
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(0)));
    assert_eq!(parsed.get_singular(5), Some(&Value::Bool(false)));
    assert!(parsed.has(6), "empty string still has explicit presence");
    assert_eq!(parsed.serialize().expect("serialize zero"), zero_set);
    let populated = edition2024_wire("defaults_populated");
    let parsed = DynamicMessage::parse_with_pool(desc, Some(pool), &populated)
        .expect("populated reference vector");
    assert_eq!(parsed.get_repeated(10).map(|items| items.len()), Some(3));
    assert_eq!(parsed.serialize().expect("serialize populated"), populated);
}

#[test]
fn edition2024_field_and_enum_overrides_match_pinned_descriptors() {
    let pool = std::sync::Arc::new(edition2024_fds("overrides"));
    let desc = pool
        .get_message("edition2024.overrides.OverridesMessage")
        .expect("OverridesMessage");
    assert_eq!(
        desc.field(1).expect("implicit").presence,
        Presence::Implicit
    );
    assert_eq!(
        desc.field(2).expect("required").cardinality,
        Cardinality::Required
    );
    assert!(!desc.field(3).expect("expanded").packed);
    assert!(!desc.field(4).expect("unverified").utf8_validate);
    assert!(desc.field(5).expect("delimited").delimited);
    assert!(
        pool.get_enum("edition2024.overrides.ClosedEnum")
            .expect("ClosedEnum")
            .closed
    );

    let mut expanded = vec![0x10, 0x01];
    expanded.extend(edition2024_wire("overrides_expanded_repeated"));
    let parsed = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), &expanded)
        .expect("expanded");
    assert_eq!(parsed.get_repeated(3).map(|items| items.len()), Some(3));
    assert_eq!(parsed.serialize().expect("serialize expanded"), expanded);

    let mut delimited = vec![0x10, 0x01];
    delimited.extend(edition2024_wire("overrides_delimited_message"));
    let parsed = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), &delimited)
        .expect("group");
    assert!(parsed.has(5));
    assert_eq!(parsed.serialize().expect("serialize group"), delimited);

    let known = [0x10, 0x01, 0x38, 0x01];
    let parsed = DynamicMessage::parse_with(desc.clone(), &known).expect("known closed enum");
    assert_eq!(parsed.get_singular(7), Some(&Value::Enum(1)));
    let unknown = [0x10, 0x01, 0x38, 0x63];
    let parsed = DynamicMessage::parse_with(desc, &unknown).expect("unknown closed enum");
    assert!(
        !parsed.has(7),
        "unknown closed enum must not become a value"
    );
    assert_eq!(parsed.serialize().expect("preserve unknown enum"), unknown);

    let inherited = edition2024_fds("inheritance");
    let file = inherited
        .get_message("edition2024.inheritance.FileDefaultsConsumer")
        .expect("file defaults");
    assert_eq!(
        file.field(1).expect("implicit").presence,
        Presence::Implicit
    );
    assert!(!file.field(2).expect("expanded").packed);
    assert!(!file.field(3).expect("unverified").utf8_validate);
    let fields = inherited
        .get_message("edition2024.inheritance.FieldLevelOverrides")
        .expect("field overrides");
    assert_eq!(
        fields.field(1).expect("explicit").presence,
        Presence::Explicit
    );
    assert!(fields.field(2).expect("packed").packed);
    assert!(fields.field(3).expect("verified").utf8_validate);
    assert!(DynamicMessage::parse_with(file, &[0x1a, 0x01, 0xff]).is_ok());
    assert!(DynamicMessage::parse_with(fields, &[0x1a, 0x01, 0xff]).is_err());

    for (name, closed) in [
        ("TopLevelEnumDefault", false),
        ("TopLevelEnumClosed", true),
        ("MessageWithNestedEnums.NestedDefaultEnum", false),
        ("MessageWithNestedEnums.NestedClosedEnum", true),
    ] {
        let name = format!("edition2024.inheritance.{name}");
        assert_eq!(inherited.get_enum(&name).expect(&name).closed, closed);
    }
    assert!(
        edition2024_fds("extensions")
            .get_enum("edition2024.extensions.ExtensionClosedEnum")
            .expect("extension enum")
            .closed
    );
    assert!(edition2024_fds("visibility")
        .get_message("edition2024.visibility.DefaultTopLevelMessage.ExportedNestedMessage")
        .is_some());
}

#[test]
fn edition2024_metadata_matches_pinned_feature_inheritance() {
    let defaults = edition2024_fds("defaults");
    assert_eq!(
        file_feature_values(&defaults, "defaults.proto"),
        (1001, 1, 1, 2)
    );
    assert_eq!(defaults.file_json_format("proto/defaults.proto"), Some(1));
    assert_eq!(
        defaults.file_default_symbol_visibility("proto/defaults.proto"),
        Some(2)
    );
    let message = defaults
        .get_message("edition2024.defaults.DefaultMessage")
        .expect("default message");
    assert_eq!(
        symbol_feature_values(&defaults, &message.full_name),
        (1, 1, 0, 2)
    );

    let overrides = edition2024_fds("overrides");
    assert_eq!(
        symbol_feature_values(&overrides, "edition2024.overrides.SubJsonBestEffort"),
        (2, 1, 0, 2)
    );
    assert_eq!(
        symbol_feature_values(&overrides, "edition2024.overrides.OverridesMessage"),
        (1, 1, 0, 2)
    );
    let inherited = edition2024_fds("inheritance");
    assert_eq!(
        symbol_feature_values(&inherited, "edition2024.inheritance.MessageJsonConsumer"),
        (2, 1, 0, 2)
    );
    assert_eq!(
        symbol_feature_values(
            &inherited,
            "edition2024.inheritance.MessageJsonConsumer.NestedMessageJsonOverride"
        ),
        (1, 1, 0, 1)
    );

    let legacy = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/differential/differential.fds"
    ))
    .expect("legacy reference FDS");
    let legacy = DescriptorPool::from_file_descriptor_set(&legacy).expect("legacy pool");
    assert_eq!(
        file_feature_values(&legacy, "differential_proto2.proto"),
        (0, 2, 2, 1)
    );
    assert_eq!(
        file_feature_values(&legacy, "differential_proto3.proto"),
        (0, 1, 2, 1)
    );
}

#[test]
fn edition2024_visibility_matches_pinned_descriptor_oracle() {
    let pool = edition2024_fds("visibility");
    assert_eq!(
        file_feature_values(&pool, "visibility.proto"),
        (1001, 1, 1, 2)
    );
    for (name, declared, effective) in [
        ("ExportedTopLevelMessage", 2, 2),
        ("LocalTopLevelMessage", 1, 1),
        ("DefaultTopLevelMessage", 0, 2),
        ("DefaultTopLevelMessage.DefaultNestedLocalMessage", 0, 1),
        ("DefaultTopLevelMessage.ExportedNestedMessage", 2, 2),
        ("DefaultTopLevelMessage.ExplicitLocalNestedMessage", 1, 1),
    ] {
        let full = format!("edition2024.visibility.{name}");
        assert!(pool.get_message(&full).is_some(), "missing {full}");
        assert_eq!(
            symbol_feature_values(&pool, &full),
            (1, 1, declared, effective),
            "{full}"
        );
    }
    for (name, declared, effective) in [
        ("ExportedTopLevelEnum", 2, 2),
        ("LocalTopLevelEnum", 1, 1),
        ("DefaultTopLevelEnum", 0, 2),
    ] {
        let full = format!("edition2024.visibility.{name}");
        assert!(pool.get_enum(&full).is_some(), "missing {full}");
        assert_eq!(
            symbol_feature_values(&pool, &full),
            (1, 1, declared, effective),
            "{full}"
        );
    }
    let inherited = edition2024_fds("inheritance");
    assert_eq!(
        symbol_feature_values(
            &inherited,
            "edition2024.inheritance.MessageWithNestedEnums.NestedDefaultEnum"
        ),
        (1, 1, 0, 1)
    );
}

#[test]
fn manual_registration_does_not_inherit_stale_descriptor_metadata() {
    let mut pool = edition2024_fds("visibility");
    let message = "edition2024.visibility.LocalTopLevelMessage";
    let en = "edition2024.visibility.LocalTopLevelEnum";
    assert_eq!(pool.effective_symbol_visibility(message), Some(1));
    assert_eq!(pool.effective_symbol_visibility(en), Some(1));

    drop(pool.register_message(MessageDescriptor::builder(message).build()));
    drop(pool.register_enum(EnumDescriptor {
        full_name: en.into(),
        ..EnumDescriptor::default()
    }));
    assert_eq!(pool.effective_symbol_visibility(message), None);
    assert_eq!(pool.effective_symbol_visibility(en), None);
}

#[test]
fn edition2024_file_metadata_overrides_reach_nested_symbols() {
    let mut features = Vec::new();
    protobuf_test_encode_varint(&mut features, 6, 2);
    protobuf_test_encode_varint(&mut features, 7, 2);
    let fds = edition_feature_fds_with_features("file", &features, 1001);
    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("file features");
    assert_eq!(
        file_feature_values(&pool, "edition_feature.proto"),
        (1001, 2, 2, 2)
    );
    assert_eq!(
        symbol_feature_values(&pool, "features.Message"),
        (2, 2, 0, 2)
    );

    let mut message_features = Vec::new();
    protobuf_test_encode_varint(&mut message_features, 6, 2);
    protobuf_test_encode_varint(&mut message_features, 7, 2);
    let fds = edition_feature_fds_with_features("message", &message_features, 1001);
    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("message features");
    assert_eq!(
        symbol_feature_values(&pool, "features.Message"),
        (2, 2, 0, 2)
    );

    let mut enum_features = Vec::new();
    protobuf_test_encode_varint(&mut enum_features, 6, 2);
    protobuf_test_encode_varint(&mut enum_features, 7, 2);
    let fds = edition_feature_fds_with_features("enum", &enum_features, 1001);
    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("enum features");
    assert_eq!(symbol_feature_values(&pool, "features.Kind"), (2, 2, 0, 2));

    for (default_visibility, top, nested) in [(1, 2, 2), (2, 2, 1), (3, 1, 1)] {
        let mut file = Vec::new();
        protobuf_test_encode_string(&mut file, 1, "scoped.proto");
        protobuf_test_encode_string(&mut file, 2, "scoped");
        protobuf_test_encode_string(&mut file, 12, "editions");
        protobuf_test_encode_varint(&mut file, 14, 1001);
        let mut features = Vec::new();
        protobuf_test_encode_varint(&mut features, 8, default_visibility);
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 50, &features);
        protobuf_test_encode_len(&mut file, 8, &options);
        let mut outer = Vec::new();
        protobuf_test_encode_string(&mut outer, 1, "Outer");
        let mut inner = Vec::new();
        protobuf_test_encode_string(&mut inner, 1, "Inner");
        protobuf_test_encode_len(&mut outer, 3, &inner);
        protobuf_test_encode_len(&mut file, 4, &outer);
        let mut en = Vec::new();
        protobuf_test_encode_string(&mut en, 1, "Kind");
        protobuf_test_encode_len(&mut file, 5, &en);
        let mut fds = Vec::new();
        protobuf_test_encode_len(&mut fds, 1, &file);
        let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("visibility override");
        assert_eq!(
            file_feature_values(&pool, "scoped.proto"),
            (1001, 1, 1, default_visibility as u32)
        );
        assert_eq!(symbol_feature_values(&pool, "scoped.Outer"), (1, 1, 0, top));
        assert_eq!(
            symbol_feature_values(&pool, "scoped.Outer.Inner"),
            (1, 1, 0, nested)
        );
        assert_eq!(symbol_feature_values(&pool, "scoped.Kind"), (1, 1, 0, top));
    }
}

fn edition2024_visibility_consumer(type_name: &str, field_type: u64) -> Vec<u8> {
    let mut fds = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/edition2024/fds/visibility.fds"
    ))
    .expect("pinned visibility FDS");
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "consumer.proto");
    protobuf_test_encode_string(&mut file, 2, "edition2024.consumer");
    protobuf_test_encode_string(&mut file, 3, "visibility.proto");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, 1001);
    let mut message = Vec::new();
    protobuf_test_encode_string(&mut message, 1, "UsesType");
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "value");
    protobuf_test_encode_varint(&mut field, 3, 1);
    protobuf_test_encode_varint(&mut field, 4, 1);
    protobuf_test_encode_varint(&mut field, 5, field_type);
    protobuf_test_encode_string(&mut field, 6, type_name);
    protobuf_test_encode_len(&mut message, 2, &field);
    protobuf_test_encode_len(&mut file, 4, &message);
    protobuf_test_encode_len(&mut fds, 1, &file);
    fds
}

fn edition2024_visibility_service_consumer(input_type: &str) -> Vec<u8> {
    let mut fds = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/edition2024/fds/visibility.fds"
    ))
    .expect("pinned visibility FDS");
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "service_consumer.proto");
    protobuf_test_encode_string(&mut file, 2, "edition2024.consumer");
    protobuf_test_encode_string(&mut file, 3, "visibility.proto");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, 1001);
    let mut service = Vec::new();
    protobuf_test_encode_string(&mut service, 1, "CallService");
    let mut method = Vec::new();
    protobuf_test_encode_string(&mut method, 1, "Call");
    protobuf_test_encode_string(&mut method, 2, input_type);
    protobuf_test_encode_string(
        &mut method,
        3,
        ".edition2024.visibility.ExportedTopLevelMessage",
    );
    protobuf_test_encode_len(&mut service, 2, &method);
    protobuf_test_encode_len(&mut file, 6, &service);
    protobuf_test_encode_len(&mut fds, 1, &file);
    fds
}

fn edition2024_visibility_extension_consumer(local_type: bool) -> Vec<u8> {
    let mut defs = Vec::new();
    protobuf_test_encode_string(&mut defs, 1, "defs.proto");
    protobuf_test_encode_string(&mut defs, 2, "scoped");
    protobuf_test_encode_string(&mut defs, 12, "editions");
    protobuf_test_encode_varint(&mut defs, 14, 1001);
    let mut target = Vec::new();
    protobuf_test_encode_string(&mut target, 1, "Target");
    let mut range = Vec::new();
    protobuf_test_encode_varint(&mut range, 1, 100);
    protobuf_test_encode_varint(&mut range, 2, 1000);
    protobuf_test_encode_len(&mut target, 5, &range);
    protobuf_test_encode_len(&mut defs, 4, &target);
    let mut extension_type = Vec::new();
    protobuf_test_encode_string(&mut extension_type, 1, "ExtensionType");
    protobuf_test_encode_varint(&mut extension_type, 11, if local_type { 1 } else { 2 });
    protobuf_test_encode_len(&mut defs, 4, &extension_type);

    let mut consumer = Vec::new();
    protobuf_test_encode_string(&mut consumer, 1, "extension_consumer.proto");
    protobuf_test_encode_string(&mut consumer, 2, "scoped");
    protobuf_test_encode_string(&mut consumer, 3, "defs.proto");
    protobuf_test_encode_string(&mut consumer, 12, "editions");
    protobuf_test_encode_varint(&mut consumer, 14, 1001);
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "external");
    protobuf_test_encode_string(&mut field, 2, ".scoped.Target");
    protobuf_test_encode_varint(&mut field, 3, 101);
    protobuf_test_encode_varint(&mut field, 4, 1);
    protobuf_test_encode_varint(&mut field, 5, 11);
    protobuf_test_encode_string(&mut field, 6, ".scoped.ExtensionType");
    protobuf_test_encode_len(&mut consumer, 7, &field);

    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &defs);
    protobuf_test_encode_len(&mut fds, 1, &consumer);
    fds
}

#[test]
fn edition2024_cross_file_local_symbols_are_rejected() {
    for (name, field_type) in [
        (".edition2024.visibility.ExportedTopLevelMessage", 11),
        (
            ".edition2024.visibility.DefaultTopLevelMessage.ExportedNestedMessage",
            11,
        ),
        (".edition2024.visibility.ExportedTopLevelEnum", 14),
    ] {
        assert!(
            DescriptorPool::from_file_descriptor_set(&edition2024_visibility_consumer(
                name, field_type
            ))
            .is_ok(),
            "cross-file exported symbol {name} must remain accessible"
        );
    }
    for (name, field_type) in [
        (".edition2024.visibility.LocalTopLevelMessage", 11),
        (
            ".edition2024.visibility.DefaultTopLevelMessage.DefaultNestedLocalMessage",
            11,
        ),
        (".edition2024.visibility.LocalTopLevelEnum", 14),
    ] {
        assert!(
            DescriptorPool::from_file_descriptor_set(&edition2024_visibility_consumer(
                name, field_type
            ))
            .is_err(),
            "cross-file local symbol {name} must be rejected"
        );
    }
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition2024_visibility_service_consumer(
            ".edition2024.visibility.ExportedTopLevelMessage"
        ))
        .is_ok(),
        "service may reference an exported request type"
    );
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition2024_visibility_service_consumer(
            ".edition2024.visibility.LocalTopLevelMessage"
        ))
        .is_err(),
        "service must not reference a cross-file local request type"
    );
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition2024_visibility_extension_consumer(false))
            .is_ok(),
        "extension may reference an exported cross-file type"
    );
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition2024_visibility_extension_consumer(true))
            .is_err(),
        "extension must not reference a cross-file local type"
    );

    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "same_file.proto");
    protobuf_test_encode_string(&mut file, 2, "scoped");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, 1001);
    let mut local = Vec::new();
    protobuf_test_encode_string(&mut local, 1, "Hidden");
    protobuf_test_encode_varint(&mut local, 11, 1);
    protobuf_test_encode_len(&mut file, 4, &local);
    let mut owner = Vec::new();
    protobuf_test_encode_string(&mut owner, 1, "Owner");
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "hidden");
    protobuf_test_encode_varint(&mut field, 3, 1);
    protobuf_test_encode_varint(&mut field, 4, 1);
    protobuf_test_encode_varint(&mut field, 5, 11);
    protobuf_test_encode_string(&mut field, 6, ".scoped.Hidden");
    protobuf_test_encode_len(&mut owner, 2, &field);
    protobuf_test_encode_len(&mut file, 4, &owner);
    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &file);
    assert!(
        DescriptorPool::from_file_descriptor_set(&fds).is_ok(),
        "same-file references to local types remain valid"
    );
}

#[test]
fn edition2024_visibility_rejects_unknown_declarations() {
    for (kind, number, value) in [("message", 11, 3), ("enum", 6, 3), ("message", 11, 99)] {
        let mut file = Vec::new();
        protobuf_test_encode_string(&mut file, 1, "bad_visibility.proto");
        protobuf_test_encode_string(&mut file, 12, "editions");
        protobuf_test_encode_varint(&mut file, 14, 1001);
        let mut desc = Vec::new();
        protobuf_test_encode_string(&mut desc, 1, "Kind");
        protobuf_test_encode_varint(&mut desc, number, value);
        protobuf_test_encode_len(&mut file, if kind == "message" { 4 } else { 5 }, &desc);
        let mut fds = Vec::new();
        protobuf_test_encode_len(&mut fds, 1, &file);
        assert!(
            DescriptorPool::from_file_descriptor_set(&fds).is_err(),
            "accepted {kind} visibility {value}"
        );
    }
}

#[test]
fn edition2024_packed_closed_enum_retains_unknown_numbers() {
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "packed_closed.proto");
    protobuf_test_encode_string(&mut file, 2, "features");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, 1001);
    let mut message = Vec::new();
    protobuf_test_encode_string(&mut message, 1, "Collection");
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "values");
    protobuf_test_encode_varint(&mut field, 3, 1);
    protobuf_test_encode_varint(&mut field, 4, 3);
    protobuf_test_encode_varint(&mut field, 5, 14);
    protobuf_test_encode_string(&mut field, 6, ".features.Kind");
    protobuf_test_encode_len(&mut message, 2, &field);
    protobuf_test_encode_len(&mut file, 4, &message);
    let mut en = Vec::new();
    protobuf_test_encode_string(&mut en, 1, "Kind");
    for (number, name) in [(0, "KIND_ZERO"), (1, "KIND_ONE")] {
        let mut value = Vec::new();
        protobuf_test_encode_string(&mut value, 1, name);
        protobuf_test_encode_varint(&mut value, 2, number);
        protobuf_test_encode_len(&mut en, 2, &value);
    }
    let mut features = Vec::new();
    protobuf_test_encode_varint(&mut features, 2, 2);
    let mut options = Vec::new();
    protobuf_test_encode_len(&mut options, 7, &features);
    protobuf_test_encode_len(&mut en, 3, &options);
    protobuf_test_encode_len(&mut file, 5, &en);
    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &file);

    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("closed enum FDS");
    let desc = pool.get_message("features.Collection").expect("collection");
    assert!(desc.field(1).expect("values").packed);
    let parsed =
        DynamicMessage::parse_with(desc, &[0x0a, 0x02, 0x01, 0x63]).expect("packed closed enum");
    assert_eq!(parsed.get_repeated(1), Some(&[Value::Enum(1)][..]));
    assert_eq!(parsed.unknown_fields().fields.iter().count(), 1);
}

fn edition_feature_fds(target: &str, tag: u32, value: u64, edition: u64) -> Vec<u8> {
    let mut features = Vec::new();
    protobuf_test_encode_varint(&mut features, tag, value);
    edition_feature_fds_with_features(target, &features, edition)
}

fn edition_feature_fds_with_features(target: &str, features: &[u8], edition: u64) -> Vec<u8> {
    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "edition_feature.proto");
    protobuf_test_encode_string(&mut file, 2, "features");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, edition);
    if target == "file" {
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 50, features);
        protobuf_test_encode_len(&mut file, 8, &options);
    }
    let mut message = Vec::new();
    protobuf_test_encode_string(&mut message, 1, "Message");
    if target == "message" {
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 12, features);
        protobuf_test_encode_len(&mut message, 7, &options);
    }
    let mut field = Vec::new();
    protobuf_test_encode_string(&mut field, 1, "value");
    protobuf_test_encode_varint(&mut field, 3, 1);
    protobuf_test_encode_varint(&mut field, 4, 1);
    protobuf_test_encode_varint(&mut field, 5, 5);
    if target == "field" {
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 21, features);
        protobuf_test_encode_len(&mut field, 8, &options);
    }
    protobuf_test_encode_len(&mut message, 2, &field);
    protobuf_test_encode_len(&mut file, 4, &message);
    let mut en = Vec::new();
    protobuf_test_encode_string(&mut en, 1, "Kind");
    let mut zero = Vec::new();
    protobuf_test_encode_string(&mut zero, 1, "KIND_ZERO");
    protobuf_test_encode_varint(&mut zero, 2, 0);
    protobuf_test_encode_len(&mut en, 2, &zero);
    if target == "enum" {
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 7, features);
        protobuf_test_encode_len(&mut en, 3, &options);
    }
    protobuf_test_encode_len(&mut file, 5, &en);
    if target == "method" {
        let mut service = Vec::new();
        protobuf_test_encode_string(&mut service, 1, "Service");
        let mut method = Vec::new();
        protobuf_test_encode_string(&mut method, 1, "call");
        protobuf_test_encode_string(&mut method, 2, ".features.Message");
        protobuf_test_encode_string(&mut method, 3, ".features.Message");
        let mut options = Vec::new();
        protobuf_test_encode_len(&mut options, 35, features);
        protobuf_test_encode_len(&mut method, 4, &options);
        protobuf_test_encode_len(&mut service, 2, &method);
        protobuf_test_encode_len(&mut file, 6, &service);
    }
    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &file);
    fds
}

#[test]
fn edition2024_rejects_unresolved_features_and_invalid_targets() {
    for (target, tag, value) in [
        ("file", 1, 0),
        ("file", 2, 99),
        ("file", 4, 1),
        ("file", 6, 99),
        ("file", 7, 3),
        ("file", 8, 4),
        ("file", 9, 1),
        ("file", 1, u64::from(u32::MAX) + 2),
        ("message", 1, 1),
        ("message", 8, 2),
        ("field", 2, 2),
        ("field", 6, 2),
        ("enum", 1, 1),
        ("enum", 8, 2),
        ("method", 1, 1),
        ("method", 9, 1),
    ] {
        let bytes = edition_feature_fds(target, tag, value, 1001);
        assert!(
            DescriptorPool::from_file_descriptor_set(&bytes).is_err(),
            "{target} accepted feature {tag}={value}"
        );
    }
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds("method", 7, 2, 1001))
            .is_ok(),
        "method-level legacy naming style is a supported target"
    );
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds("file", 1, 1, 1002)).is_err(),
        "unknown edition must not inherit Edition 2023 semantics"
    );
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds("file", 1, 1, 0)).is_err(),
        "an unnumbered editions file has no resolved defaults"
    );
    let mut wrong_wire = Vec::new();
    protobuf_test_encode_len(&mut wrong_wire, 1, b"\x01");
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds_with_features(
            "file",
            &wrong_wire,
            1001,
        ))
        .is_err(),
        "a known feature with the wrong wire type must fail"
    );
    let mut unknown_wire = Vec::new();
    protobuf_test_encode_len(&mut unknown_wire, 9, b"\x01");
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds_with_features(
            "file",
            &unknown_wire,
            1001,
        ))
        .is_err(),
        "an unknown length-delimited feature must fail"
    );
    let mut cpp_feature = Vec::new();
    protobuf_test_encode_len(&mut cpp_feature, 1000, b"\x08\x01");
    assert!(
        DescriptorPool::from_file_descriptor_set(&edition_feature_fds_with_features(
            "file",
            &cpp_feature,
            1001,
        ))
        .is_ok(),
        "language-specific feature extensions remain valid"
    );

    let mut file = Vec::new();
    protobuf_test_encode_string(&mut file, 1, "bad_feature_wire.proto");
    protobuf_test_encode_string(&mut file, 12, "editions");
    protobuf_test_encode_varint(&mut file, 14, 1001);
    let mut options = Vec::new();
    protobuf_test_encode_varint(&mut options, 50, 1);
    protobuf_test_encode_len(&mut file, 8, &options);
    let mut fds = Vec::new();
    protobuf_test_encode_len(&mut fds, 1, &file);
    assert!(
        DescriptorPool::from_file_descriptor_set(&fds).is_err(),
        "a malformed FileOptions.features field must not become a custom option"
    );
}

#[test]
fn edition2023_and_proto_defaults_remain_unchanged() {
    let earlier = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/differential/differential.fds"
    ))
    .expect("PB-03 differential FDS");
    let pool = DescriptorPool::from_file_descriptor_set(&earlier).expect("PB-03 FDS");
    assert_eq!(
        pool.get_message("differential.Proto2Presence")
            .expect("proto2")
            .field(1)
            .expect("optional int32")
            .presence,
        Presence::Explicit
    );
    assert_eq!(
        pool.get_message("differential.Proto3Presence")
            .expect("proto3")
            .field(1)
            .expect("implicit int32")
            .presence,
        Presence::Implicit
    );
    assert!(
        pool.get_enum("differential.ClosedEnum")
            .expect("closed")
            .closed
    );
    assert!(!pool.get_enum("differential.OpenEnum").expect("open").closed);

    let fds = edition_feature_fds("file", 1, 1, 1000);
    let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("Edition 2023 FDS");
    assert_eq!(
        pool.get_message("features.Message")
            .expect("Edition 2023")
            .field(1)
            .expect("value")
            .presence,
        Presence::Explicit
    );
}

#[test]
fn bundled_reference_pool_preserves_supported_editions() {
    let fds = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/google/conformance_fds.bin"
    ));
    let pool = DescriptorPool::from_file_descriptor_set(fds).expect("bundled reference FDS");
    for file in [
        "google/protobuf/test_messages_proto2.proto",
        "google/protobuf/test_messages_proto3.proto",
        "conformance/test_protos/test_messages_edition2023.proto",
    ] {
        assert!(pool.get_file(file).is_some(), "missing {file}");
    }
}
