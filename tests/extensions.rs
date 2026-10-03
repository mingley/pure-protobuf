//! Native scalar primitives; generated message behavior is checked in plugin.rs.

use pbrs::rt::{CachedSize, UnknownField};
use pbrs::{Extension, ExtensionHost, UnknownFields};

#[derive(Default)]
struct Host {
    unknown: UnknownFields,
    cached_size: CachedSize,
}

impl ExtensionHost for Host {
    fn __extension_fields(&self) -> &UnknownFields {
        &self.unknown
    }

    fn __extension_fields_mut(&mut self) -> &mut UnknownFields {
        self.cached_size.dirty();
        &mut self.unknown
    }
}

const VALUE: Extension<Host, i32> = match Extension::__new(101, "test.value", 42) {
    Some(value) => value,
    None => panic!("test tag is valid"),
};

#[test]
fn identifier_validates_tag_in_const_context() {
    const VALID: Option<Extension<Host, i32>> = Extension::__new(536_870_911, "max", 0);
    const RESERVED: Option<Extension<Host, i32>> = Extension::__new(19_000, "reserved", 0);
    assert!(VALID.is_some());
    assert!(RESERVED.is_none());
    for tag in [0, 19_000, 19_999, 536_870_912, u32::MAX] {
        assert!(Extension::<Host, i32>::__new(tag, "invalid", 0).is_none());
    }
    assert!(Extension::<Host, i32>::__new(18_999, "before", 0).is_some());
    assert!(Extension::<Host, i32>::__new(20_000, "after", 0).is_some());
    assert_eq!(VALUE.number(), 101);
    assert_eq!(VALUE.full_name(), "test.value");
    assert_eq!(VALUE.default_value(), 42);
}

#[test]
fn absence_and_mutation_preserve_one_word_empty_bag() {
    assert_eq!(std::mem::size_of::<UnknownFields>(), std::mem::size_of::<usize>());
    let mut host = Host::default();
    assert_eq!(VALUE.get(&host), 42);
    assert!(!VALUE.has(&host));
    host.cached_size.set(0);
    VALUE.clear(&mut host);
    assert!(host.unknown.fields.is_empty());
    assert_eq!(host.cached_size.get(), None);
    VALUE.set(&mut host, 0);
    assert!(VALUE.has(&host));
    assert_eq!(VALUE.get(&host), 0);
    VALUE.clear(&mut host);
    assert_eq!(host.unknown, UnknownFields::default());
}

#[test]
fn reads_do_not_mutate_and_writes_preserve_wrong_wire_records() {
    let retained = [
        UnknownField::Fixed32 { number: 101, value: 11 },
        UnknownField::Varint { number: 200, value: 9 },
        UnknownField::LengthDelimited { number: 101, value: vec![8] },
        UnknownField::Fixed64 { number: 101, value: 7 },
        UnknownField::Group { number: 101, fields: UnknownFields::default() },
    ];
    let mut host = Host::default();
    host.unknown.fields.extend(retained.clone());
    host.unknown.fields.push(UnknownField::Varint { number: 101, value: 7 });
    host.unknown.fields.push(UnknownField::Varint { number: 101, value: 11 });
    let before = host.unknown.clone();
    assert_eq!(VALUE.get(&host), 11);
    assert!(VALUE.has(&host));
    assert_eq!(host.unknown, before);
    VALUE.set(&mut host, -1);
    assert_eq!(VALUE.get(&host), -1);
    let mut expected = retained.to_vec();
    expected.push(UnknownField::Varint { number: 101, value: u64::MAX });
    assert_eq!(host.unknown.fields.as_slice(), expected);
    VALUE.clear(&mut host);
    assert!(!VALUE.has(&host));
    assert_eq!(host.unknown.fields.as_slice(), retained);
}

#[test]
fn int32_reads_match_existing_wire_cast_semantics() {
    let mut host = Host::default();
    for (wire, expected) in [(u64::MAX, -1), (0x1_0000_0000, 0), (0xffff_ffff, -1)] {
        host.unknown.fields.clear();
        host.unknown.fields.push(UnknownField::Varint { number: 101, value: wire });
        assert_eq!(VALUE.get(&host), expected);
    }
}
