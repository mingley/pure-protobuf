//! Google `rust/test/shared` behavioral suite, compiled against this crate's
//! plugin (not googletest / upb gencode).
//!
//! SKIP: `ctype_cord_test.rs` (cord), `gtest_matchers_test.rs` (protobuf_gtest_matchers),
//! `no_internal_access_test.rs` (`__internal` is a module, not `()`),
//! `package_disambiguation_test.rs` (empty), `extensions_test.rs` (empty; proto is Edition 2024).
//! SKIP edition2023 `str_view` cpp `pb.cpp.string_type=VIEW` (tested as ordinary string).
//! SKIP proto! `__{}` / `..spread` / qualified-path (set-only proto! is covered).
//! SKIP cross-crate `import public` type identity (single-crate generated modules).
//! SKIP `bad_names_test.rs` — View/Mut suffix vs type name and `clear_x` accessor collisions.

#[path = "google_gen/child.rs"]
mod child;
#[path = "google_gen/edition2023.rs"]
mod edition2023;
#[path = "google_gen/enums.rs"]
mod enums;
#[path = "google_gen/feature_verify.rs"]
mod feature_verify;
#[path = "google_gen/fields_with_imported_types.rs"]
mod fields_with_imported_types;
#[path = "google_gen/import_public.rs"]
mod import_public;
#[path = "google_gen/map_unittest.rs"]
mod map_unittest;
#[path = "google_gen/nested.rs"]
mod nested;
#[path = "google_gen/no_features_proto2.rs"]
mod no_features_proto2;
#[path = "google_gen/no_features_proto3.rs"]
mod no_features_proto3;
#[path = "google_gen/no_package.rs"]
mod no_package;
#[path = "google_gen/package.rs"]
mod package;
#[path = "google_gen/parent.rs"]
mod parent;
#[path = "google_gen/unittest.rs"]
mod unittest;
#[path = "google_gen/unittest_proto3.rs"]
mod unittest_proto3;
#[path = "google_gen/unittest_proto3_optional.rs"]
mod unittest_proto3_optional;

use protobuf::prelude::*;
use unittest::TestRequired;
use unittest_proto3::TestAllTypes;
use unittest_proto3_optional::TestProto3Optional;

#[test]
fn serialization_zero_length_proto3() {
    let msg = TestAllTypes::new();
    assert_eq!(msg.serialize().unwrap().len(), 0);
    assert_eq!(msg.as_view().serialize().unwrap().len(), 0);
}

#[test]
fn serialize_deserialize_message_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bool(true);
    msg.set_optional_bytes(b"serialize deserialize test");
    let serialized = msg.serialize().unwrap();
    let msg2 = TestAllTypes::parse(&serialized).unwrap();
    assert_eq!(msg.optional_int64(), msg2.optional_int64());
    assert_eq!(msg.optional_bool(), msg2.optional_bool());
    assert_eq!(msg.optional_bytes(), msg2.optional_bytes());
}

#[test]
fn deserialize_empty_proto3() {
    assert!(TestAllTypes::parse(&[]).is_ok());
    assert!(TestProto3Optional::parse(&[]).is_ok());
}

#[test]
fn deserialize_error_proto3() {
    assert!(TestAllTypes::parse(b"not a serialized proto").is_err());
}

#[test]
fn set_bytes_with_serialized_data_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bool(true);
    let mut msg2 = TestAllTypes::new();
    msg2.set_optional_bytes(msg.serialize().unwrap());
    assert_eq!(msg2.optional_bytes(), msg.serialize().unwrap());
}

#[test]
fn deserialize_on_previously_allocated_message_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bytes(b"serialize deserialize test");
    let serialized = msg.serialize().unwrap();
    let mut msg2 = TestAllTypes::new();
    msg2.set_optional_bool(true);
    assert!(msg2.clear_and_parse(&serialized).is_ok());
    assert_eq!(msg2.optional_int64(), msg.optional_int64());
    assert_eq!(msg2.optional_bytes(), msg.optional_bytes());
    assert!(!msg2.optional_bool());
}

#[test]
fn proto3_optional_roundtrip() {
    let mut msg = TestProto3Optional::new();
    msg.set_optional_int64(7);
    msg.set_optional_bytes(b"opt");
    let again = TestProto3Optional::parse(&msg.serialize().unwrap()).unwrap();
    assert_eq!(again.optional_int64(), 7);
    assert_eq!(again.optional_bytes(), b"opt");
}

#[test]
fn test_required_field_enforced() {
    assert!(TestRequired::parse(&[]).is_err());
    let mut msg = TestRequired::new();
    assert!(msg.clear_and_parse(&[]).is_err());
}

#[test]
fn test_required_field_not_enforced() {
    let mut msg = TestRequired::parse_dont_enforce_required(&[]).unwrap();
    assert!(!msg.has_a());
    msg.set_a(1);
    msg.clear_and_parse_dont_enforce_required(&[]).unwrap();
    assert!(!msg.has_a());
}

#[test]
fn test_int32_byte_size_proto3_optional() {
    for (value, expected_value_size) in [(0, 1), (127, 1), (128, 2), (-1, 10)] {
        let mut msg = TestProto3Optional::new();
        msg.set_optional_int32(value);
        let serialized = msg.serialize().unwrap();
        assert_eq!(serialized.len(), expected_value_size + 1, "value={value}");
    }
}

#[test]
fn test_fixed32_accessors() {
    let mut msg = TestAllTypes::new();
    assert_eq!(msg.optional_fixed32(), 0);
    msg.set_optional_fixed32(42);
    assert_eq!(msg.optional_fixed32(), 42);
    msg.set_optional_fixed32(u32::default());
    assert_eq!(msg.optional_fixed32(), 0);
    msg.set_optional_fixed32(43);
    assert_eq!(msg.optional_fixed32(), 43);
}

#[test]
fn test_bool_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(!msg.optional_bool());
    msg.set_optional_bool(true);
    assert!(msg.optional_bool());
    msg.set_optional_bool(bool::default());
    assert!(!msg.optional_bool());
}

#[test]
fn test_bytes_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(msg.optional_bytes().is_empty());
    msg.set_optional_bytes(b"accessors_test");
    assert_eq!(msg.optional_bytes(), b"accessors_test");
    {
        let s = Vec::from(&b"hello world"[..]);
        msg.set_optional_bytes(&s[..]);
    }
    assert_eq!(msg.optional_bytes(), b"hello world");
    msg.set_optional_bytes(b"");
    assert!(msg.optional_bytes().is_empty());
}

#[test]
fn test_optional_bytes_accessors() {
    let mut msg = TestProto3Optional::new();
    assert!(msg.optional_bytes().is_empty());
    {
        let s = Vec::from(&b"hello world"[..]);
        msg.set_optional_bytes(&s[..]);
    }
    assert_eq!(msg.optional_bytes(), b"hello world");
    msg.set_optional_bytes(b"");
    assert!(msg.optional_bytes().is_empty());
    msg.set_optional_bytes(b"\xffbinary\x85non-utf8");
    assert_eq!(msg.optional_bytes(), b"\xffbinary\x85non-utf8");
}

#[test]
fn test_string_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(msg.optional_string().as_bytes().is_empty());
    msg.set_optional_string("accessors_test");
    assert_eq!(msg.optional_string(), "accessors_test");
}

use protobuf::{message_eq, Enum, Parse, ParseError, ProtoStr, Serialize, View};
use unittest::TestAllTypes as Proto2;
use unittest::{test_all_types, NestedMessage, NestedTestAllTypes};

#[test]
fn serialization_zero_length_proto2() {
    let mut msg = Proto2::new();
    assert_eq!(msg.serialize().unwrap().len(), 0);
    assert_eq!(msg.as_view().serialize().unwrap().len(), 0);
    assert_eq!(msg.as_mut().serialize().unwrap().len(), 0);
}

#[test]
fn serialize_default_view() {
    assert_eq!(View::<Proto2>::default().serialize().unwrap().len(), 0);
    assert_eq!(
        View::<TestAllTypes>::default().serialize().unwrap().len(),
        0
    );
}

#[test]
fn proto2_required_and_optional_parse() {
    assert!(unittest::TestRequired::parse(&[]).is_err());
    assert!(unittest::TestRequired::parse_dont_enforce_required(&[]).is_ok());
}

#[test]
fn proto2_default_accessors() {
    let msg = Proto2::default();
    assert_eq!(msg.default_int32(), 41);
    assert_eq!(msg.default_int64(), 42);
    assert_eq!(msg.default_uint32(), 43);
    assert_eq!(msg.default_uint64(), 44);
    assert_eq!(msg.default_sint32(), -45);
    assert_eq!(msg.default_sint64(), 46);
    assert_eq!(msg.default_fixed32(), 47);
    assert_eq!(msg.default_fixed64(), 48);
    assert_eq!(msg.default_sfixed32(), 49);
    assert_eq!(msg.default_sfixed64(), -50);
    assert_eq!(msg.default_float(), 51.5);
    assert_eq!(msg.default_double(), 52000.0);
    assert!(msg.default_bool());
    assert_eq!(msg.default_string(), "hello");
    assert_eq!(msg.default_bytes(), b"world");
}

#[test]
fn proto2_optional_and_default_opt() {
    let mut msg = Proto2::new();
    assert!(!msg.has_optional_fixed32());
    assert_eq!(msg.optional_fixed32_opt(), None);
    assert_eq!(msg.optional_fixed32(), 0);
    msg.set_optional_fixed32(7);
    assert!(msg.has_optional_fixed32());
    assert_eq!(msg.optional_fixed32_opt(), Some(7));
    msg.clear_optional_fixed32();
    assert_eq!(msg.optional_fixed32_opt(), None);

    assert_eq!(msg.default_int32(), 41);
    assert_eq!(msg.default_int32_opt(), None);
    msg.set_default_int32(41);
    assert_eq!(msg.default_int32_opt(), Some(41));
    msg.clear_default_int32();
    assert_eq!(msg.default_int32(), 41);
    assert_eq!(msg.default_int32_opt(), None);
}

#[test]
fn proto2_nested_message_opt() {
    let mut msg = Proto2::new();
    assert!(!msg.has_optional_nested_message());
    assert!(msg.optional_nested_message_opt().is_none());
    assert_eq!(msg.optional_nested_message().bb(), 0);
    msg.optional_nested_message_mut().set_bb(5);
    assert!(msg.has_optional_nested_message());
    assert_eq!(msg.optional_nested_message_opt().unwrap().bb(), 5);
    msg.clear_optional_nested_message();
    assert!(!msg.has_optional_nested_message());
}

#[test]
fn proto3_optional_opt() {
    let mut msg = TestProto3Optional::new();
    assert_eq!(msg.optional_bytes_opt(), None);
    msg.set_optional_bytes(b"hello world");
    assert_eq!(msg.optional_bytes_opt(), Some(&b"hello world"[..]));
}

#[test]
fn repeated_numeric_set_and_iter() {
    let mut msg = Proto2::new();
    assert!(msg.repeated_int32().is_empty());
    let mut m = msg.repeated_int32_mut();
    m.push(1);
    m.set(0, 2);
    m.push(1);
    m.push(3);
    m.set(2, 4);
    m.set(2, 0);
    assert_eq!(m.iter().copied().collect::<Vec<_>>(), vec![2, 1, 0]);
    let mut iter = m.iter();
    assert_eq!(iter.len(), 3);
    assert_eq!(iter.next(), Some(&2));
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.next(), Some(&1));
    assert_eq!(iter.next(), Some(&0));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next(), None);

    let mut msg2 = Proto2::new();
    for i in 0..5 {
        msg2.repeated_int32_mut().push(i);
    }
    msg.set_repeated_int32(msg2.repeated_int32());
    assert_eq!(
        msg.repeated_int32().iter().copied().collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
}

#[test]
fn repeated_bool_enum_message() {
    let mut msg = Proto2::new();
    let mut b = msg.repeated_bool_mut();
    b.push(true);
    b.set(0, false);
    b.push(true);
    b.extend([false]);
    assert_eq!(
        b.iter().copied().collect::<Vec<_>>(),
        vec![false, true, false]
    );

    let mut e = msg.repeated_nested_enum_mut();
    e.push(i32::from(test_all_types::NestedEnum::Foo));
    e.set(0, i32::from(test_all_types::NestedEnum::Bar));
    e.push(i32::from(test_all_types::NestedEnum::Baz));
    assert_eq!(
        e.iter().copied().collect::<Vec<_>>(),
        vec![
            i32::from(test_all_types::NestedEnum::Bar),
            i32::from(test_all_types::NestedEnum::Baz)
        ]
    );
    msg.set_repeated_nested_enum([
        i32::from(test_all_types::NestedEnum::Foo),
        i32::from(test_all_types::NestedEnum::Bar),
    ]);
    assert_eq!(msg.repeated_nested_enum().len(), 2);

    let mut nested = NestedMessage::new();
    nested.set_bb(1);
    msg.repeated_nested_message_mut().push(nested);
    assert_eq!(msg.repeated_nested_message().get(0).unwrap().bb(), 1);
    let mut nested2 = NestedMessage::new();
    nested2.set_bb(2);
    msg.repeated_nested_message_mut().set(0, nested2);
    assert_eq!(msg.repeated_nested_message().get(0).unwrap().bb(), 2);
    msg.repeated_nested_message_mut()
        .get_mut(0)
        .unwrap()
        .set_bb(3);
    assert_eq!(msg.repeated_nested_message().get(0).unwrap().bb(), 3);
    let nested3 = {
        let mut n = NestedMessage::new();
        n.set_bb(9);
        n
    };
    msg.set_repeated_nested_message([nested3]);
    assert_eq!(msg.repeated_nested_message().get(0).unwrap().bb(), 9);
}

#[test]
fn maps_insert_get_keys() {
    let mut msg = map_unittest::TestMap::new();
    assert!(msg.map_int32_int32().is_empty());
    assert!(msg.map_int32_int32_mut().insert(0, 0));
    assert!(!msg.map_int32_int32_mut().insert(0, 0));
    assert!(msg.map_int32_int32_mut().insert(1, 1));
    assert_eq!(msg.map_int32_int32().len(), 2);
    assert_eq!(msg.map_int32_int32().get(&1), Some(&1));
    assert_eq!(msg.map_int32_int32().keys().count(), 2);
    assert_eq!(msg.map_int32_int32().values().count(), 2);

    msg.map_string_string_mut().insert("hello", "world");
    msg.map_string_string_mut().insert("fizz", "buzz");
    assert_eq!(
        msg.map_string_string()
            .get(&"fizz".into())
            .unwrap()
            .as_view(),
        "buzz"
    );
    msg.map_string_string_mut().clear();
    assert!(msg.map_string_string().is_empty());

    assert!(msg
        .map_int32_enum_mut()
        .insert(1, i32::from(map_unittest::MapEnum::Baz)));
}

#[test]
fn proto_macro_literals() {
    let msg = proto!(Proto2 {
        optional_int32: 101,
        optional_int64: 102,
        optional_bool: true,
        optional_string: "foo",
        optional_bytes: b"bar",
        optional_nested_message: NestedMessage { bb: 42 },
        optional_nested_enum: test_all_types::NestedEnum::Baz,
    });
    assert_eq!(msg.optional_int32(), 101);
    assert_eq!(msg.optional_int64(), 102);
    assert!(msg.optional_bool());
    assert_eq!(msg.optional_string(), "foo");
    assert_eq!(msg.optional_bytes(), b"bar");
    assert_eq!(msg.optional_nested_message().bb(), 42);
    assert_eq!(msg.optional_nested_enum(), test_all_types::NestedEnum::Baz);
}

#[test]
fn copy_take_merge() {
    let mut dst = Proto2::new();
    let mut src = Proto2::new();
    src.set_optional_int32(42);
    src.optional_nested_message_mut().set_bb(10);
    dst.copy_from(src.as_view());
    assert!(dst.has_optional_int32());
    assert_eq!(src.optional_int32(), 42);

    let mut dst = Proto2::new();
    dst.take_from(src.as_mut());
    assert!(!src.has_optional_int32());
    assert_eq!(src.optional_nested_message().bb(), 0);
    assert!(dst.has_optional_int32());
    assert_eq!(dst.optional_nested_message().bb(), 10);

    let mut dst = Proto2::new();
    dst.merge_from(proto!(Proto2 { optional_int32: 42 }));
    assert_eq!(dst.optional_int32(), 42);

    let mut dst = Proto2::new();
    let mut src = Proto2::new();
    src.repeated_int32_mut().extend(0..5);
    dst.merge_from(src.as_view());
    assert_eq!(
        dst.repeated_int32().iter().copied().collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    dst.repeated_int32_mut().clear();
    dst.repeated_int32_mut().extend(0..5);
    src.repeated_int32_mut().clear();
    src.repeated_int32_mut().extend(5..10);
    dst.merge_from(src.as_view());
    assert_eq!(
        dst.repeated_int32().iter().copied().collect::<Vec<_>>(),
        (0..10).collect::<Vec<_>>()
    );

    let mut dst = NestedTestAllTypes::new();
    dst.merge_from(proto!(NestedTestAllTypes {
        child: NestedTestAllTypes {
            payload: Proto2 { optional_int32: 42 }
        }
    }));
    assert_eq!(dst.child().payload().optional_int32(), 42);
    assert!(message_eq(
        &dst,
        &proto!(NestedTestAllTypes {
            child: NestedTestAllTypes {
                payload: Proto2 { optional_int32: 42 }
            }
        })
    ));
}

#[test]
fn utf8_parse_rules() {
    let non = ProtoStr::from_utf8_unchecked(b"\x80");
    let mut p2 = no_features_proto2::NoFeaturesProto2::new();
    p2.set_my_field(non);
    let ser = p2.serialize().unwrap();
    assert!(no_features_proto2::NoFeaturesProto2::parse(&ser).is_ok());

    let mut p3 = no_features_proto3::NoFeaturesProto3::new();
    p3.set_my_field(non);
    let ser = p3.serialize().unwrap();
    assert!(no_features_proto3::NoFeaturesProto3::parse(&ser).is_err());

    let mut v = feature_verify::Verify::new();
    v.set_my_field(non);
    let ser = v.serialize().unwrap();
    let err = feature_verify::Verify::parse(&ser);
    assert!(err.is_err());
    let _: &ParseError = err.as_ref().unwrap_err();
}

#[test]
fn edition2023_presence() {
    let msg = edition2023::EditionsMessage::new();
    assert_eq!(msg.plain_field_opt(), None);
    assert_eq!(msg.implicit_presence_field(), 0);
    let mut msg = edition2023::EditionsMessage::new();
    assert_eq!(msg.str_view(), "");
    assert!(!msg.has_str_view());
    msg.set_str_view("hello");
    assert_eq!(msg.str_view(), "hello");
    assert!(msg.has_str_view());
    msg.repeated_str_view_mut().push("first".into());
    assert_eq!(msg.repeated_str_view().len(), 1);
}

#[test]
fn enums_closed_open_alias() {
    assert_eq!(i32::from(test_all_types::NestedEnum::Foo), 1);
    assert_eq!(i32::from(test_all_types::NestedEnum::Bar), 2);
    assert_eq!(i32::from(test_all_types::NestedEnum::Baz), 3);
    assert_eq!(i32::from(test_all_types::NestedEnum::Neg), -1);

    assert_eq!(
        unittest::TestSparseEnum::default(),
        unittest::TestSparseEnum::SparseA
    );
    assert_eq!(
        unittest::TestEnumWithDupValue::default(),
        unittest::TestEnumWithDupValue::Foo1
    );
    assert_eq!(
        unittest::TestEnumWithDupValue::Foo1,
        unittest::TestEnumWithDupValue::Foo2
    );
    assert_eq!(
        test_all_types::NestedEnum::default(),
        test_all_types::NestedEnum::Foo
    );

    assert_eq!(
        unittest::TestSparseEnum::try_from(123).unwrap(),
        unittest::TestSparseEnum::SparseA
    );
    assert!(unittest::TestSparseEnum::try_from(1).is_err());
    let err = unittest::TestSparseEnum::try_from(1).unwrap_err();
    assert_eq!(
        format!("{err}"),
        "1 is not a known value for TestSparseEnum"
    );
    let _e: &dyn std::error::Error = &err;

    assert!(!test_all_types::NestedEnum::is_known(0));
    assert!(test_all_types::NestedEnum::is_known(1));
    assert!(test_all_types::NestedEnum::is_known(-1));

    use enums::{
        TestEnumValueNameSameAsEnum, TestEnumWithDuplicateStrippedPrefixNames,
        TestEnumWithNumericNames,
    };
    assert_eq!(
        i32::from(TestEnumValueNameSameAsEnum::TestEnumValueNameSameAsEnum),
        1
    );
    assert_eq!(
        TestEnumWithNumericNames::from(1),
        TestEnumWithNumericNames::_2020
    );
    assert_eq!(
        format!("{:?}", TestEnumWithNumericNames::_2020),
        "TestEnumWithNumericNames::_2020"
    );
    assert_eq!(
        format!("{:?}", TestEnumWithNumericNames::from(42)),
        "TestEnumWithNumericNames::from(42)"
    );
    assert!(TestEnumWithNumericNames::is_known(0));
    assert!(!TestEnumWithNumericNames::is_known(4));
    assert_eq!(
        TestEnumWithDuplicateStrippedPrefixNames::from(2),
        TestEnumWithDuplicateStrippedPrefixNames::Bar
    );
    assert_eq!(
        TestEnumWithDuplicateStrippedPrefixNames::Bar,
        TestEnumWithDuplicateStrippedPrefixNames::DifferentNameAlias
    );

    let mut s = std::collections::HashSet::new();
    s.insert(test_all_types::NestedEnum::Foo);
    s.insert(test_all_types::NestedEnum::Bar);
    s.insert(test_all_types::NestedEnum::try_from(1).unwrap());
    assert_eq!(s.len(), 2);

    let mut m = std::collections::BTreeMap::new();
    m.insert(test_all_types::NestedEnum::Baz, 1);
    m.insert(test_all_types::NestedEnum::Bar, 2);
    m.insert(test_all_types::NestedEnum::Foo, 3);
    m.insert(test_all_types::NestedEnum::try_from(1).unwrap(), 4);
    assert_eq!(m.pop_first(), Some((test_all_types::NestedEnum::Foo, 4)));
    assert_eq!(m.pop_first(), Some((test_all_types::NestedEnum::Bar, 2)));
    assert_eq!(m.pop_first(), Some((test_all_types::NestedEnum::Baz, 1)));
}

#[test]
fn nested_types_accessible() {
    let _p: unittest::TestAllTypes;
    let _c: unittest::test_all_types::NestedMessage;
    let _e: unittest::test_all_types::NestedEnum;
    let deep =
        nested::outer::inner::super_inner::duper_inner::even_more_inner::CantBelieveItsSoInner::new(
        );
    assert_eq!(deep.num(), 0);
    let outermsg = nested::Outer::new();
    assert_eq!(outermsg.deep().num(), 0);
    use nested::outer::inner::InnerEnum;
    assert_eq!(outermsg.inner().inner_enum(), InnerEnum::Unspecified);
    assert_eq!(outermsg.inner().string(), "");
}

#[test]
fn child_parent_serialize() {
    assert!(parent::Parent::new().serialize().unwrap().is_empty());
    assert!(child::Child::new().serialize().unwrap().is_empty());
}

#[test]
fn threading_send() {
    let msg = std::sync::Arc::new(std::sync::Mutex::new(Proto2::default()));
    let clone = std::sync::Arc::clone(&msg);
    std::thread::spawn(move || {
        clone.lock().unwrap().set_optional_int32(123);
    })
    .join()
    .unwrap();
    assert_eq!(msg.lock().unwrap().optional_int32(), 123);

    let mut msg = Proto2::default();
    std::thread::scope(|scope| {
        let child = msg.optional_nested_message_mut();
        scope.spawn(move || {
            child.set_bb(123);
        });
    });
    assert_eq!(msg.optional_nested_message().bb(), 123);
}

#[test]
fn message_generics() {
    fn encoded_len<T: protobuf::Message>(msg: T) -> usize {
        msg.as_view().serialize().unwrap().len() + msg.serialize().unwrap().len()
    }
    assert_eq!(encoded_len(Proto2::new()), 0);
    assert_eq!(View::<Proto2>::default().serialize().unwrap().len(), 0);
    let mut msg = Proto2::new();
    msg.set_optional_int32(123);
    assert!(msg.has_optional_int32());
    msg.as_mut().clear();
    assert!(!msg.has_optional_int32());
}

#[test]
fn imported_and_package_types_exist() {
    let _ = fields_with_imported_types::MsgWithFieldsWithImportedTypes::new();
    let _ = package::MsgWithPackage::new();
    let _ = no_package::MsgWithoutPackage::new();
    let _ = import_public::PrimarySrcPubliclyImportedMsg::new();
}

#[test]
fn proto3_clear_and_parse_on_mut() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    let serialized = msg.serialize().unwrap();
    let mut msg2 = TestAllTypes::new();
    msg2.set_optional_bool(true);
    msg2.as_mut().clear_and_parse(&serialized).unwrap();
    assert_eq!(msg2.optional_int64(), 42);
    assert!(!msg2.optional_bool());
}
