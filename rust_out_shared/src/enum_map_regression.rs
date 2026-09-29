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
