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
