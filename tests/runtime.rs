//! MessageSet, generated WKT accessors, and view reads.

#![allow(
    unsafe_code,
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
use pbrs::gencode::TestAllTypesProto2;
use pbrs::gencode::TestAllTypesProto3;
use pbrs::gencode::{
    Any, BoolValue, Duration, Empty, FieldMask, ListValue, PbValue, Struct, Timestamp,
};
use pbrs::prelude::*;
use pbrs::{Parse, Serialize};

#[test]
fn tat_default_is_empty_and_zeroed() {
    let a = TestAllTypesProto3::new();
    let b = TestAllTypesProto3::default();
    assert_eq!(a, b);
    assert!(Serialize::serialize(&a).unwrap().is_empty());
    assert!(std::mem::size_of::<TestAllTypesProto3>() < 4096);
}

#[test]
fn generated_wkt_timestamp_roundtrip() {
    let mut t = Timestamp::new();
    t.set_seconds(1);
    t.set_nanos(2);
    let b = Serialize::serialize(&t).unwrap();
    let q = Timestamp::parse(&b).unwrap();
    assert_eq!(q.seconds(), 1);
    assert_eq!(q.nanos(), 2);
    assert_eq!(
        <Timestamp as pbrs::MessageName>::FULL_NAME,
        "google.protobuf.Timestamp"
    );
}

#[test]
fn generated_wkt_duration_any_empty_mask() {
    let mut d = Duration::new();
    d.set_seconds(3);
    d.set_nanos(4);
    assert_eq!(
        Duration::parse(&Serialize::serialize(&d).unwrap())
            .unwrap()
            .seconds(),
        3
    );

    let mut a = Any::new();
    a.set_type_url("type.googleapis.com/google.protobuf.Empty");
    assert_eq!(
        Any::parse(&Serialize::serialize(&a).unwrap())
            .unwrap()
            .type_url(),
        "type.googleapis.com/google.protobuf.Empty"
    );

    let e = Empty::new();
    assert!(Serialize::serialize(&e).unwrap().is_empty());

    let mut m = FieldMask::new();
    m.paths_mut().push("foo");
    assert_eq!(
        FieldMask::parse(&Serialize::serialize(&m).unwrap())
            .unwrap()
            .paths()
            .len(),
        1
    );

    let mut w = BoolValue::new();
    w.set_value(true);
    assert!(
        BoolValue::parse(&Serialize::serialize(&w).unwrap())
            .unwrap()
            .value()
    );

    let s = Struct::new();
    let _ = ListValue::new();
    let _ = PbValue::new();
    assert_eq!(s.fields().len(), 0);
}

#[test]
fn packed_truncated_is_err() {
    // packed int32 field 31: tag 0xFA 0x01, length 2, one-byte overlong/truncated varint 0x80
    let buf = vec![0xFA, 0x01, 0x02, 0x80, 0x80];
    assert!(pbrs::gencode::TestAllTypesProto3::parse(&buf).is_err());
}

#[test]
fn packed_parse_roundtrip_without_touching_getters() {
    let mut m = pbrs::gencode::TestAllTypesProto3::new();
    for i in 0..8 {
        m.repeated_int32_mut().push(i);
        m.packed_int32_mut().push(i * 3);
    }
    let bytes = Serialize::serialize(&m).unwrap();
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    let again = Serialize::serialize(&parsed).unwrap();
    assert_eq!(bytes, again);
    assert_eq!(
        parsed.repeated_int32().iter().collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
}

#[test]
fn nested_merge_second_empty_does_not_wipe() {
    let mut nested = pbrs::gencode::NestedMessage::new();
    nested.set_a(9);
    let mut m = pbrs::gencode::TestAllTypesProto3::new();
    m.set_optional_nested_message(nested);
    let mut bytes = Serialize::serialize(&m).unwrap();
    // field 18 empty LEN
    bytes.extend_from_slice(&[0x92, 0x01, 0x00]);
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    assert_eq!(parsed.optional_nested_message().a(), 9);
}

#[test]
fn view_reads_string_and_nested_without_owned_child() {
    let mut nested = pbrs::gencode::TestAllTypesProto3::new();
    // NestedMessage lives in the proto3 module; use optional_string + recursive via generated TAT.
    nested.set_optional_string("ada");
    nested.set_optional_int32(7);
    let bytes = Serialize::serialize(&nested).unwrap();
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    let s: &pbrs::ProtoStr = parsed.optional_string();
    assert_eq!(s, "ada");
    let view = parsed.as_view();
    assert_eq!(view.0.optional_int32(), 7);
}

#[test]
fn message_set_item_roundtrip() {
    // type_id 1547769, message bytes for MessageSetCorrectExtension1 { str = "hi" }
    // Extension1 field 25 is string. Encode "hi" as field 25: tag 0xca 0x01, len 2, hi
    let mut inner = Vec::new();
    pbrs::rt::encode_len_field(&mut inner, 25, b"hi");
    let mut item = Vec::new();
    pbrs::rt::encode_tag(&mut item, 1, pbrs::rt::WIRE_SGROUP);
    pbrs::rt::encode_tag(&mut item, 2, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut item, 1_547_769);
    pbrs::rt::encode_len_field(&mut item, 3, &inner);
    pbrs::rt::encode_tag(&mut item, 1, pbrs::rt::WIRE_EGROUP);

    let parsed = TestAllTypesProto2::parse(&{
        let mut wrap = Vec::new();
        pbrs::rt::encode_len_field(&mut wrap, 500, &item);
        wrap
    })
    .unwrap();
    assert!(parsed.has_message_set_correct());
    let ms = parsed.message_set_correct();
    let out = Serialize::serialize(&parsed).unwrap();
    let again = TestAllTypesProto2::parse(&out).unwrap();
    assert_eq!(
        Serialize::serialize(again.message_set_correct()).unwrap(),
        Serialize::serialize(ms).unwrap()
    );
}

#[test]
fn raw_message_ptr_survives_arena_growth_and_fusion() {
    struct Example;
    // SAFETY: This test uses the dangling MiniTable only for fields added to MsgData dynamically.
    unsafe impl pbrs::runtime::AssociatedMiniTable for Example {
        fn mini_table() -> pbrs::runtime::MiniTablePtr {
            pbrs::runtime::MiniTablePtr::dangling()
        }
    }

    let source = pbrs::runtime::Arena::new();
    let first = pbrs::runtime::MessagePtr::<Example>::new(&source).expect("first message");
    // SAFETY: The message is owned by source until fusion and by destination afterward.
    unsafe { first.set_base_field_i32_at_index(0, 42) };
    for _ in 0..128 {
        assert!(pbrs::runtime::MessagePtr::<Example>::new(&source).is_some());
    }

    let destination = pbrs::runtime::Arena::new();
    destination.fuse(&source);
    drop(source);
    for _ in 0..128 {
        assert!(pbrs::runtime::MessagePtr::<Example>::new(&destination).is_some());
    }
    // SAFETY: Arena ownership has moved, but the boxed MsgData and its raw pointer stay live.
    unsafe { assert_eq!(first.get_i32_at_index(0, 0), 42) };
}

#[test]
fn raw_message_strings_survive_slot_and_owner_growth() {
    struct Example;
    // SAFETY: The dynamically added fields are owned by the test's arena.
    unsafe impl pbrs::runtime::AssociatedMiniTable for Example {
        fn mini_table() -> pbrs::runtime::MiniTablePtr {
            pbrs::runtime::MiniTablePtr::dangling()
        }
    }

    let arena = pbrs::runtime::Arena::new();
    let ptr = pbrs::runtime::MessagePtr::<Example>::new(&arena).expect("message");
    // SAFETY: The arena owns the message while each field is inserted and read.
    unsafe {
        ptr.set_base_field_string_at_index(0, pbrs::runtime::StringView::from(b"alpha"));
        for index in 1..128_u32 {
            let bytes = index.to_le_bytes();
            ptr.set_base_field_string_at_index(index, pbrs::runtime::StringView::from(&bytes));
        }
        let first = ptr.get_string_at_index(0, pbrs::runtime::StringView::empty());
        assert_eq!(first.as_ref(), b"alpha");
    }
}

#[test]
fn inner_proto_string_raw_parts_are_reclaimed_with_arena() {
    let (view, arena) = pbrs::runtime::InnerProtoString::from(b"alpha".as_slice()).into_raw_parts();
    // SAFETY: the returned Arena owns the StringView's backing bytes until it is dropped.
    unsafe { assert_eq!(view.as_ref(), b"alpha") };
    drop(arena);
}

#[test]
fn map_and_repeated_message_arena_adoption_fusion() {
    // 1. Low-level MiniTable / Arena adoption and fusion safety
    struct ChildMsg;
    // SAFETY: Stand-in MiniTable implementation for runtime qualification test.
    unsafe impl pbrs::runtime::AssociatedMiniTable for ChildMsg {
        fn mini_table() -> pbrs::runtime::MiniTablePtr {
            pbrs::runtime::MiniTablePtr::dangling()
        }
    }

    struct ParentMsg;
    // SAFETY: Stand-in MiniTable implementation for runtime qualification test.
    unsafe impl pbrs::runtime::AssociatedMiniTable for ParentMsg {
        fn mini_table() -> pbrs::runtime::MiniTablePtr {
            pbrs::runtime::MiniTablePtr::dangling()
        }
    }

    let child_arena = pbrs::runtime::Arena::new();
    let child_ptr = pbrs::runtime::MessagePtr::<ChildMsg>::new(&child_arena).expect("alloc child");
    // SAFETY: child_ptr points to valid allocated MsgData in child_arena.
    unsafe {
        child_ptr.set_base_field_i32_at_index(0, 42);
        child_ptr.set_base_field_string_at_index(
            1,
            pbrs::runtime::StringView::from(b"adopted child string"),
        );
    }

    let parent_arena = pbrs::runtime::Arena::new();
    let parent_ptr =
        pbrs::runtime::MessagePtr::<ParentMsg>::new(&parent_arena).expect("alloc parent");

    // SAFETY: allocating repeated array and map slots in parent_arena.
    unsafe {
        let arr = parent_ptr
            .get_or_create_mutable_array_at_index(5, &parent_arena)
            .expect("alloc array");
        assert!(!arr.is_null());

        let map = parent_ptr
            .get_or_create_mutable_map_at_index(6, &parent_arena)
            .expect("alloc map");
        assert!(!map.is_null());

        parent_ptr.set_base_field_message_at_index(7, child_ptr);
    }

    // Fuse child arena into parent arena.
    parent_arena.fuse(&child_arena);

    // Drop child_arena; parent_arena now holds ownership of child_ptr's allocation.
    drop(child_arena);

    // SAFETY: Verify child message fields remain valid after child_arena dropped (no use-after-free).
    unsafe {
        let retrieved = parent_ptr
            .get_message_at_index::<ChildMsg>(7)
            .expect("child message");
        assert_eq!(retrieved.get_i32_at_index(0, 0), 42);
        let s = retrieved.get_string_at_index(1, pbrs::runtime::StringView::empty());
        assert_eq!(s.as_ref(), b"adopted child string");
    }

    // 2. High-level Repeated message and Map adoption and fusion in generated messages
    let mut parent = TestAllTypesProto3::new();
    {
        let mut n1 = pbrs::gencode::NestedMessage::new();
        n1.set_a(101);
        parent.repeated_nested_message_mut().push(n1);

        let mut n2 = pbrs::gencode::NestedMessage::new();
        n2.set_a(202);
        parent.repeated_nested_message_mut().push(n2);
    }
    assert_eq!(parent.repeated_nested_message().len(), 2);
    assert_eq!(parent.repeated_nested_message().get(0).unwrap().a(), 101);
    assert_eq!(parent.repeated_nested_message().get(1).unwrap().a(), 202);

    let bytes = Serialize::serialize(&parent).expect("serialize parent");
    let parsed = TestAllTypesProto3::parse(&bytes).expect("parse parent");
    assert_eq!(parsed.repeated_nested_message().len(), 2);
    assert_eq!(parsed.repeated_nested_message().get(0).unwrap().a(), 101);
    assert_eq!(parsed.repeated_nested_message().get(1).unwrap().a(), 202);

    // Map adoption and clean lifetime management
    let mut map_msg = TestAllTypesProto3::new();
    map_msg.map_string_string_mut().insert("alpha", "one");
    map_msg.map_string_string_mut().insert("beta", "two");
    map_msg.map_int32_int32_mut().insert(1, 10);
    map_msg.map_int32_int32_mut().insert(2, 20);

    assert_eq!(map_msg.map_string_string().len(), 2);
    assert_eq!(
        map_msg.map_string_string().get("alpha").unwrap().as_view(),
        "one"
    );
    assert_eq!(
        map_msg.map_string_string().get("beta").unwrap().as_view(),
        "two"
    );
    assert_eq!(map_msg.map_int32_int32().get(1), Some(10));
    assert_eq!(map_msg.map_int32_int32().get(2), Some(20));

    let map_bytes = Serialize::serialize(&map_msg).expect("serialize map");
    let parsed_map = TestAllTypesProto3::parse(&map_bytes).expect("parse map");
    assert_eq!(
        parsed_map
            .map_string_string()
            .get("alpha")
            .unwrap()
            .as_view(),
        "one"
    );
    assert_eq!(parsed_map.map_int32_int32().get(2), Some(20));
}

#[test]
fn alignment_and_endianness_safety() {
    // 1. Verify little-endian wire integer roundtrip across fixed integer types
    let u32_val: u32 = 0x1234_5678;
    assert_eq!(u32_val.to_le_bytes(), [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(u32::from_le_bytes([0x78, 0x56, 0x34, 0x12]), u32_val);

    let u64_val: u64 = 0x0123_4567_89ab_cdef;
    assert_eq!(
        u64_val.to_le_bytes(),
        [0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]
    );
    assert_eq!(
        u64::from_le_bytes([0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]),
        u64_val
    );

    let i32_val: i32 = -12345;
    assert_eq!(i32::from_le_bytes(i32_val.to_le_bytes()), i32_val);

    let i64_val: i64 = -9876543210;
    assert_eq!(i64::from_le_bytes(i64_val.to_le_bytes()), i64_val);

    // 2. Unaligned buffer reads: ensure fixed readers handle odd byte offsets safely
    for pad in 1..=3 {
        let mut buf = vec![0xEE; pad];
        buf.extend_from_slice(&u32_val.to_le_bytes());
        let mut pos = pad;
        let read = pbrs::rt::read_fixed32(&buf, &mut pos).expect("read unaligned fixed32");
        assert_eq!(read, u32_val);
        assert_eq!(pos, pad + 4);

        let mut buf64 = vec![0xEE; pad];
        buf64.extend_from_slice(&u64_val.to_le_bytes());
        let mut pos64 = pad;
        let read64 = pbrs::rt::read_fixed64(&buf64, &mut pos64).expect("read unaligned fixed64");
        assert_eq!(read64, u64_val);
        assert_eq!(pos64, pad + 8);
    }

    // 3. Multi-byte scalar bitcasts: IEEE-754 floats
    let f32_cases: &[f32] = &[
        0.0f32,
        -0.0f32,
        1.0f32,
        -1.0f32,
        std::f32::consts::PI,
        1e-30f32,
        1e30f32,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    for &val in f32_cases {
        let bits = val.to_bits();
        let le_bytes = bits.to_le_bytes();
        let decoded = f32::from_bits(u32::from_le_bytes(le_bytes));
        assert_eq!(val.to_bits(), decoded.to_bits());
    }

    // NaN bitcast roundtrip
    let nan32 = f32::NAN;
    let decoded_nan = f32::from_bits(u32::from_le_bytes(nan32.to_bits().to_le_bytes()));
    assert!(decoded_nan.is_nan());

    let f64_cases: &[f64] = &[
        0.0f64,
        -0.0f64,
        1.0f64,
        -1.0f64,
        std::f64::consts::PI,
        1e-250f64,
        1e250f64,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    for &val in f64_cases {
        let bits = val.to_bits();
        let le_bytes = bits.to_le_bytes();
        let decoded = f64::from_bits(u64::from_le_bytes(le_bytes));
        assert_eq!(val.to_bits(), decoded.to_bits());
    }
    let nan64 = f64::NAN;
    let decoded_nan64 = f64::from_bits(u64::from_le_bytes(nan64.to_bits().to_le_bytes()));
    assert!(decoded_nan64.is_nan());

    // 4. Packed fixed-width scalar unaligned append & decode
    let mut packed_fx32 = pbrs::rt::PackedFx32::new();
    let mut unaligned_bytes = vec![0x77]; // 1-byte unaligned prefix
    unaligned_bytes.extend_from_slice(&100u32.to_le_bytes());
    unaligned_bytes.extend_from_slice(&200u32.to_le_bytes());
    unaligned_bytes.extend_from_slice(&300u32.to_le_bytes());
    packed_fx32
        .append_bytes(&unaligned_bytes[1..])
        .expect("append unaligned fixed");
    assert_eq!(packed_fx32.as_view().len(), 3);
    assert_eq!(packed_fx32.as_view().get(0).unwrap(), 100);
    assert_eq!(packed_fx32.as_view().get(1).unwrap(), 200);
    assert_eq!(packed_fx32.as_view().get(2).unwrap(), 300);

    // Mutate and check encode reproduces little-endian bytes
    packed_fx32.push(400);
    let encoded = packed_fx32.packed_bytes().expect("packed bytes");
    assert_eq!(encoded.len(), 16);
    let mut expected = Vec::new();
    for v in [100u32, 200, 300, 400] {
        expected.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(encoded, expected.as_slice());

    // 5. ZigZag bitcasts roundtrip for negative, positive, and extremal values
    for val in [
        i32::MIN,
        i32::MIN + 1,
        -12345,
        -2,
        -1,
        0,
        1,
        2,
        12345,
        i32::MAX - 1,
        i32::MAX,
    ] {
        let enc = pbrs::rt::encode_zigzag32(val);
        let dec = pbrs::rt::decode_zigzag32(enc);
        assert_eq!(val, dec, "zigzag32 failed for {val}");
    }
    assert_eq!(pbrs::rt::encode_zigzag32(0), 0);
    assert_eq!(pbrs::rt::encode_zigzag32(-1), 1);
    assert_eq!(pbrs::rt::encode_zigzag32(1), 2);
    assert_eq!(pbrs::rt::encode_zigzag32(-2), 3);

    for val in [
        i64::MIN,
        i64::MIN + 1,
        -9876543210,
        -2,
        -1,
        0,
        1,
        2,
        9876543210,
        i64::MAX - 1,
        i64::MAX,
    ] {
        let enc = pbrs::rt::encode_zigzag64(val);
        let dec = pbrs::rt::decode_zigzag64(enc);
        assert_eq!(val, dec, "zigzag64 failed for {val}");
    }
    assert_eq!(pbrs::rt::encode_zigzag64(0), 0);
    assert_eq!(pbrs::rt::encode_zigzag64(-1), 1);
    assert_eq!(pbrs::rt::encode_zigzag64(1), 2);
    assert_eq!(pbrs::rt::encode_zigzag64(-2), 3);
}

#[test]
fn invalidation_safety_cached_size_and_dirty_tracking() {
    // 1. CachedSize atomic dirtying contract
    let cs = pbrs::rt::CachedSize::default();
    assert_eq!(cs.get(), Some(0)); // 0 is initial clean size
    cs.dirty();
    assert_eq!(cs.get(), None); // None indicates dirty
    cs.set(42);
    assert_eq!(cs.get(), Some(42));
    let cs_clone = cs.clone();
    assert_eq!(cs_clone.get(), Some(42));
    // CachedSize is intentionally ignored in PartialEq
    assert_eq!(cs, pbrs::rt::CachedSize::default());

    // 2. Invalidation upon field mutations in generated message
    let mut msg = TestAllTypesProto3::new();
    let initial_len = msg.serialized_len();
    assert_eq!(initial_len, 0);

    // Mutate scalar field: invalidates size, increases length
    msg.set_optional_int32(100);
    let len_after_scalar = msg.serialized_len();
    assert!(len_after_scalar > initial_len);
    let bytes1 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes1.len(), len_after_scalar);

    // Mutate repeated field: invalidates size
    msg.repeated_int32_mut().push(555);
    let len_after_repeated = msg.serialized_len();
    assert!(len_after_repeated > len_after_scalar);
    let bytes2 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes2.len(), len_after_repeated);

    // Mutate string field: invalidates size
    msg.set_optional_string("cache invalidation string test");
    let len_after_string = msg.serialized_len();
    assert!(len_after_string > len_after_repeated);
    let bytes3 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes3.len(), len_after_string);

    // Mutate map field: invalidates size
    msg.map_int32_int32_mut().insert(1, 999);
    let len_after_map = msg.serialized_len();
    assert!(len_after_map > len_after_string);
    let bytes4 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes4.len(), len_after_map);

    // Mutate nested message: invalidates size
    let mut nested = pbrs::gencode::NestedMessage::new();
    nested.set_a(888);
    msg.set_optional_nested_message(nested);
    let len_after_nested = msg.serialized_len();
    assert!(len_after_nested > len_after_map);
    let bytes5 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes5.len(), len_after_nested);

    // Clear nested message field: invalidates size and reduces serialized length
    msg.clear_optional_nested_message();
    let len_after_clear = msg.serialized_len();
    assert!(len_after_clear < len_after_nested);
    let bytes6 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes6.len(), len_after_clear);

    // Setting scalar to proto3 default (0) invalidates size and omits from wire
    msg.set_optional_int32(0);
    let len_after_zero = msg.serialized_len();
    assert!(len_after_zero < len_after_clear);
    let bytes7 = Serialize::serialize(&msg).expect("serialize");
    assert_eq!(bytes7.len(), len_after_zero);

    // Proto2 explicit field clearing
    let mut p2 = TestAllTypesProto2::new();
    p2.set_optional_int32(42);
    let p2_len = p2.serialized_len();
    assert!(p2_len > 0);
    p2.clear_optional_int32();
    assert_eq!(p2.serialized_len(), 0);

    // 3. Packed collection cache invalidation on mutation
    let mut packed = pbrs::rt::PackedFx32::new();
    let wire_bytes = 42u32.to_le_bytes();
    packed
        .append_wire(pbrs::rt::Wire::from_slice(&wire_bytes))
        .expect("append wire");
    assert_eq!(packed.packed_bytes(), Some(wire_bytes.as_slice()));
    // Push mutates internal state, forcing OnceLock encoded bytes to be dropped
    packed.push(84);
    assert_eq!(packed.as_view().len(), 2);
    let new_packed = packed.packed_bytes().expect("new packed bytes");
    assert_eq!(new_packed.len(), 8);
    let mut expected_bytes = Vec::new();
    expected_bytes.extend_from_slice(&42u32.to_le_bytes());
    expected_bytes.extend_from_slice(&84u32.to_le_bytes());
    assert_eq!(new_packed, expected_bytes.as_slice());

    // 4. Nested message mutation through get_or_insert invalidates wire cache
    let mut lazy_msg_holder = TestAllTypesProto3::new();
    let mut sub_msg = pbrs::gencode::NestedMessage::new();
    sub_msg.set_a(12);
    lazy_msg_holder.set_optional_nested_message(sub_msg);
    let initial_bytes = Serialize::serialize(&lazy_msg_holder).expect("serialize");

    let mut roundtrip = TestAllTypesProto3::parse(&initial_bytes).expect("parse");
    // Mutate nested message in-place
    roundtrip.optional_nested_message_mut().set_a(99);
    let re_serialized = Serialize::serialize(&roundtrip).expect("re-serialize");
    assert_ne!(initial_bytes, re_serialized);
    let roundtrip2 = TestAllTypesProto3::parse(&re_serialized).expect("parse2");
    assert_eq!(roundtrip2.optional_nested_message().a(), 99);
}
