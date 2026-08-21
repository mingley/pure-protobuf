use pbrs::prelude::*;
use pbrs::testdata::{Address, Person};
use pbrs::{Parse, Serialize};

#[test]
fn proto_macro_roundtrip_bytes() {
    let msg = proto!(Person {
        id: 1,
        name: "ada",
        email: "ada@ex",
        address: Address { city: "nyc" },
    });

    assert_eq!(msg.id(), 1);
    assert_eq!(msg.name(), "ada");
    assert!(msg.has_email());
    assert_eq!(msg.email(), "ada@ex");
    assert!(msg.has_address());
    assert_eq!(msg.address().city(), "nyc");

    let bytes = msg.serialize().expect("serialize");
    // id=1: 08 01
    // name="ada": 12 03 61 64 61
    // email="ada@ex": 1a 06 61 64 61 40 65 78
    // address { city="nyc" }: 32 05 0a 03 6e 79 63
    let expected = [
        0x08, 0x01, 0x12, 0x03, b'a', b'd', b'a', 0x1a, 0x06, b'a', b'd', b'a', b'@', b'e', b'x',
        0x32, 0x05, 0x0a, 0x03, b'n', b'y', b'c',
    ];
    assert_eq!(bytes, expected, "typed encode must match canonical wire");

    let parsed = Person::parse(&bytes).expect("parse");
    assert_eq!(parsed.id(), 1);
    assert_eq!(parsed.name(), "ada");
    assert_eq!(parsed.email(), "ada@ex");
    assert_eq!(parsed.address().city(), "nyc");
    assert_eq!(parsed.serialize().unwrap(), bytes);
}

#[test]
fn implicit_presence_omits_defaults() {
    let msg = Person::new();
    assert_eq!(msg.serialize().unwrap(), Vec::<u8>::new());
}

#[test]
fn repeated_and_map() {
    let mut msg = Person::new();
    msg.tags_mut().push("a".into());
    msg.tags_mut().push("b".into());
    msg.scores_mut().insert("x", 7);

    let bytes = msg.serialize().unwrap();
    let parsed = Person::parse(&bytes).unwrap();
    assert_eq!(parsed.tags().len(), 2);
    assert_eq!(parsed.tags().get(0).unwrap().as_view(), "a");
    assert_eq!(parsed.tags().get(1).unwrap().as_view(), "b");
    assert_eq!(*parsed.scores().get(&"x".into()).unwrap(), 7);
}

#[test]
fn unknown_fields_roundtrip() {
    // field 99 varint 5: 0xc8 0x06 0x05  (99<<3 | 0 = 792 = 0x318 → varint c8 06)
    let mut raw = proto!(Person { id: 3 }).serialize().unwrap();
    raw.extend_from_slice(&[0xc8, 0x06, 0x05]);

    let parsed = Person::parse(&raw).unwrap();
    assert_eq!(parsed.id(), 3);
    let out = parsed.serialize().unwrap();
    assert_eq!(out, raw);
}

#[test]
fn parse_trait_is_v4_shaped() {
    fn takes_parse<T: Parse>(bytes: &[u8]) -> T {
        T::parse(bytes).unwrap()
    }
    let bytes = proto!(Person { id: 9 }).serialize().unwrap();
    let msg: Person = takes_parse(&bytes);
    assert_eq!(msg.id(), 9);
}
