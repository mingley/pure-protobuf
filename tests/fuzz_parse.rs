//! In-tree wire-corpus parse. ParseError is ignored; panics are crashes.

use pbrs::gencode::TestAllTypesProto3;
use pbrs::testdata::Person;
use pbrs::{Parse, Serialize};

const EMPTY: &[u8] = &[];
const TRUNCATED_VARINT: &[u8] = &[0x08, 0xff];
// id=1 name=ada email=ada@ex address.city=nyc (tests/typed.rs)
const PERSON: &[u8] = &[
    0x08, 0x01, 0x12, 0x03, b'a', b'd', b'a', 0x1a, 0x06, b'a', b'd', b'a', b'@', b'e', b'x', 0x32,
    0x05, 0x0a, 0x03, b'n', b'y', b'c',
];

fn valid_tat_bytes() -> Vec<u8> {
    let mut m = TestAllTypesProto3::new();
    m.set_optional_int32(7);
    m.set_optional_string("ada");
    m.serialize().expect("serialize TAT")
}

fn feed(bytes: &[u8]) {
    let _ = Person::parse(bytes);
    let _ = TestAllTypesProto3::parse(bytes);
}

#[test]
fn fuzz_parse_short_campaign() {
    let tat = valid_tat_bytes();
    let corpus: [&[u8]; 4] = [EMPTY, TRUNCATED_VARINT, PERSON, &tat];
    for bytes in corpus {
        feed(bytes);
    }
}
