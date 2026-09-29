//! Additional pbrs regressions; the 19 original upstream test files stay intact.

use crate::map_unittest_rust_proto::{MapEnum, TestMap};
use pbrs::{AsMut, AsView, IntoMut, IntoView, Parse, Serialize};

#[test]
fn enum_map_insert_iterate_mutate_and_roundtrip_unknown_values() {
    let mut msg = TestMap::new();
    {
        let mut map = msg.map_int32_enum_mut();
        assert!(map.insert(0, MapEnum::Foo));
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.as_view().iter().collect::<Vec<_>>(),
            [(0, MapEnum::Foo)]
        );
        assert_eq!(map.get(0), Some(MapEnum::Foo));
        assert!(!map.insert(0, MapEnum::Baz));
        assert_eq!(map.as_view().values().collect::<Vec<_>>(), [MapEnum::Baz]);
        for number in [i32::MIN, -1, 42, i32::MAX] {
            assert!(map.insert(number, MapEnum::from(number)));
        }
        // Every mutable reborrow/view conversion must retain the enum codec.
        assert_eq!(map.as_mut().get(-1), Some(MapEnum::from(-1)));
        assert_eq!(map.as_mut().into_mut().get(42), Some(MapEnum::from(42)));
        assert_eq!(
            map.as_mut().into_view().get(i32::MAX),
            Some(MapEnum::from(i32::MAX))
        );
        assert_eq!(
            map.as_view().into_view().get(i32::MIN),
            Some(MapEnum::from(i32::MIN))
        );
    }
    let encoded = msg.serialize().unwrap();
    let mut decoded = TestMap::parse(&encoded).unwrap();
    let expected: Vec<_> = msg.map_int32_enum().iter().collect();
    assert_eq!(
        decoded.map_int32_enum().iter().collect::<Vec<_>>(),
        expected
    );
    assert_eq!(decoded.map_int32_enum().len(), expected.len());
    for (key, value) in expected {
        assert_eq!(decoded.map_int32_enum().get(key), Some(value));
    }
    assert!(decoded.map_int32_enum_mut().remove(-1));
    assert_eq!(decoded.map_int32_enum().get(-1), None);
    decoded.map_int32_enum_mut().clear();
    assert!(decoded.map_int32_enum().is_empty());
}

#[test]
fn enum_map_field_setter_and_read_views_preserve_codec() {
    let mut owned = pbrs::Map::<i32, MapEnum>::new();
    owned.insert(9, MapEnum::from(901));
    owned.insert(-9, MapEnum::Bar);
    let mut msg = TestMap::new();
    msg.set_map_int32_enum(owned);
    let first = msg.map_int32_enum();
    let second = first;
    assert_eq!(first.get(9), Some(MapEnum::from(901)));
    assert_eq!(
        second.iter().collect::<Vec<_>>(),
        [(9, MapEnum::from(901)), (-9, MapEnum::Bar)]
    );
    let parsed = TestMap::parse(&msg.serialize().unwrap()).unwrap();
    assert_eq!(parsed.map_int32_enum().get(9), Some(MapEnum::from(901)));
}

#[test]
fn repeated_enum_views_and_replacement_preserve_unknown_values() {
    use crate::unittest_proto3_rust_proto::{TestAllTypes, test_all_types::NestedEnum};
    let mut msg = TestAllTypes::new();
    {
        let mut values = msg.repeated_nested_enum_mut();
        values.push(NestedEnum::Foo);
        values.push(NestedEnum::from(i32::MIN));
        values.set(0, NestedEnum::from(42));
        assert_eq!(values.get(0), Some(NestedEnum::from(42)));
        assert_eq!(
            values.iter().collect::<Vec<_>>(),
            [NestedEnum::from(42), NestedEnum::from(i32::MIN)]
        );
        assert_eq!(
            values.as_mut().into_mut().get(1),
            Some(NestedEnum::from(i32::MIN))
        );
        assert_eq!(
            values.as_mut().into_view().get(0),
            Some(NestedEnum::from(42))
        );
        assert_eq!(
            values.as_view().into_view().get(1),
            Some(NestedEnum::from(i32::MIN))
        );
    }
    let parsed = TestAllTypes::parse(&msg.serialize().unwrap()).unwrap();
    assert_eq!(
        parsed.repeated_nested_enum().iter().collect::<Vec<_>>(),
        msg.repeated_nested_enum().iter().collect::<Vec<_>>()
    );
    msg.set_repeated_nested_enum(pbrs::Repeated::from_vec(vec![
        NestedEnum::from(-1),
        NestedEnum::Bar,
    ]));
    assert_eq!(
        msg.repeated_nested_enum().iter().collect::<Vec<_>>(),
        [NestedEnum::from(-1), NestedEnum::Bar]
    );
}

#[test]
fn closed_repeated_enum_parse_preserves_unknowns_without_holes() {
    use crate::unittest_rust_proto::{TestAllTypes, test_all_types::NestedEnum};
    // The upstream edition-2023 fixture has proto2-compatible CLOSED enums
    // and EXPANDED repeated encoding. Both wire encodings must be accepted.
    for wire in [
        &[0x98, 0x03, 1, 0x98, 0x03, 42, 0x98, 0x03, 2][..],
        &[0x9a, 0x03, 3, 1, 42, 2][..],
    ] {
        let mut msg = TestAllTypes::parse(wire).unwrap();
        let values = msg.repeated_nested_enum();
        assert_eq!(values.len(), 2);
        assert_eq!(values.get(0), Some(NestedEnum::Foo));
        assert_eq!(values.get(1), Some(NestedEnum::Bar));
        assert_eq!(values.get(2), None);
        let mut iter = values.iter();
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.next(), Some(NestedEnum::Foo));
        assert_eq!(iter.size_hint(), (1, Some(1)));
        assert_eq!(iter.next(), Some(NestedEnum::Bar));
        assert_eq!(iter.len(), 0);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        let roundtrip = TestAllTypes::parse(&msg.serialize().unwrap()).unwrap();
        assert_eq!(
            roundtrip.repeated_nested_enum().iter().collect::<Vec<_>>(),
            [NestedEnum::Foo, NestedEnum::Bar]
        );
        {
            let mut values = msg.repeated_nested_enum_mut();
            values.set(1, NestedEnum::Neg);
            assert_eq!(values.as_mut().into_view().get(1), Some(NestedEnum::Neg));
            assert_eq!(values.len(), 2);
        }
        let with_negative = TestAllTypes::parse(&msg.serialize().unwrap()).unwrap();
        assert_eq!(
            with_negative
                .repeated_nested_enum()
                .iter()
                .collect::<Vec<_>>(),
            [NestedEnum::Foo, NestedEnum::Neg]
        );
        msg.repeated_nested_enum_mut().clear();
        // Unknown wire data belongs to the message, not the visible list;
        // clearing the list must not discard it.
        assert_eq!(msg.serialize().unwrap(), [0x98, 0x03, 42]);
        let only_unknown = TestAllTypes::parse(&msg.serialize().unwrap()).unwrap();
        assert!(only_unknown.repeated_nested_enum().is_empty());
    }
}

#[test]
fn closed_singular_enum_unknown_has_no_presence_and_survives_roundtrip() {
    use crate::unittest_rust_proto::{
        TestAllTypes, TestRequiredEnum, TestRequiredOpenEnum, test_all_types::NestedEnum,
    };
    // optional_nested_enum = 21; the second unknown has bits above i32 width.
    for unknown in [
        &[0xa8, 0x01, 42][..],
        &[0xa8, 0x01, 0xaa, 0x80, 0x80, 0x80, 0x10][..],
    ] {
        let msg = TestAllTypes::parse(unknown).unwrap();
        assert!(!msg.has_optional_nested_enum());
        assert_eq!(msg.optional_nested_enum(), NestedEnum::Foo);
        assert_eq!(msg.optional_nested_enum_opt(), None);
        assert_eq!(msg.serialize().unwrap(), unknown);
    }
    let wire = [0xa8, 0x01, 2, 0xa8, 0x01, 42];
    let msg = TestAllTypes::parse(&wire).unwrap();
    assert!(msg.has_optional_nested_enum());
    assert_eq!(msg.optional_nested_enum(), NestedEnum::Bar);
    let parsed = TestAllTypes::parse(&msg.serialize().unwrap()).unwrap();
    assert_eq!(parsed.optional_nested_enum(), NestedEnum::Bar);
    assert_eq!(parsed.serialize().unwrap(), wire);
    assert!(TestRequiredEnum::parse(&[8, 42]).is_err());
    let open = TestRequiredOpenEnum::parse(&[8, 42]).unwrap();
    assert!(open.has_required_enum());
    assert_eq!(i32::from(open.required_enum()), 42);
    assert_eq!(open.serialize().unwrap(), [8, 42]);
}

#[test]
fn closed_sparse_enum_descriptor_accepts_all_named_signed_numbers() {
    use crate::unittest_rust_proto::{SparseEnumMessage, TestSparseEnum};
    let enum_wire = |number: i32| {
        let mut wire = vec![8];
        let mut value = number as i64 as u64;
        while value >= 128 {
            wire.push((value as u8 & 127) | 128);
            value >>= 7;
        }
        wire.push(value as u8);
        wire
    };
    for number in [123i32, 62374, 12589234, -15, -53452, 0, 2] {
        let wire = enum_wire(number);
        let msg = SparseEnumMessage::parse(&wire).unwrap();
        assert!(msg.has_sparse_enum());
        assert_eq!(msg.sparse_enum(), TestSparseEnum::try_from(number).unwrap());
        assert_eq!(msg.serialize().unwrap(), wire);
    }
    for number in [1i32, -1, i32::MIN, i32::MAX] {
        let wire = enum_wire(number);
        let msg = SparseEnumMessage::parse(&wire).unwrap();
        assert!(!msg.has_sparse_enum());
        assert_eq!(msg.sparse_enum(), TestSparseEnum::SparseA);
        assert_eq!(msg.serialize().unwrap(), wire);
    }
}
