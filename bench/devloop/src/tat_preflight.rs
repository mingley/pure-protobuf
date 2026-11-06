//! Complete known-field preflight for the registered codec specimens.
//!
//! This runs only during preparation. Expected values come from a separate
//! freshly constructed specimen; both decoders use private copies, so getters
//! cannot materialize or warm any message used by a timed operation. Ordered
//! repeated fields remain ordered; protobuf map values are order independent.

use super::{PbrsTat, pbrs_specimen_for_cell};
use pbrs::prelude::*;
use prost013::Message as _;
use std::collections::BTreeMap;

/// The nine fields populated by the ordinary TAT specimen, plus repeated
/// strings used by the registered tags fixture. A nested value must preserve
/// presence and `a`; unexpected recursion is rejected for these specimens.
#[derive(Debug, PartialEq, Eq)]
struct KnownFields {
    optional_int32: i32,
    optional_int64: i64,
    optional_uint32: u32,
    optional_string: Vec<u8>,
    optional_bytes: Vec<u8>,
    optional_nested_message: Option<(i32, bool)>,
    repeated_int32: Vec<i32>,
    packed_int32: Vec<i32>,
    map_int32_int32: BTreeMap<i32, i32>,
    map_cardinality: usize,
    zero_map_value: Option<i32>,
    repeated_string: Vec<Vec<u8>>,
}

impl KnownFields {
    fn native(message: &PbrsTat) -> Self {
        let map = message.map_int32_int32();
        Self {
            optional_int32: message.optional_int32(),
            optional_int64: message.optional_int64(),
            optional_uint32: message.optional_uint32(),
            optional_string: message.optional_string().as_bytes().to_vec(),
            optional_bytes: message.optional_bytes().to_vec(),
            optional_nested_message: message
                .optional_nested_message_opt()
                .map(|nested| (nested.a(), nested.has_corecursive())),
            repeated_int32: message.repeated_int32().iter().collect(),
            packed_int32: message.packed_int32().iter().collect(),
            map_int32_int32: map.iter().collect(),
            map_cardinality: map.len(),
            zero_map_value: map.get(0),
            repeated_string: message
                .repeated_string()
                .iter()
                .map(|value| value.as_view().as_bytes().to_vec())
                .collect(),
        }
    }

    fn prost(message: &prost_tat::TestAllTypesProto3) -> Self {
        Self {
            optional_int32: message.optional_int32,
            optional_int64: message.optional_int64,
            optional_uint32: message.optional_uint32,
            optional_string: message.optional_string.as_bytes().to_vec(),
            optional_bytes: message.optional_bytes.clone(),
            optional_nested_message: message
                .optional_nested_message
                .as_ref()
                .map(|nested| (nested.a, nested.corecursive.is_some())),
            repeated_int32: message.repeated_int32.clone(),
            packed_int32: message.packed_int32.clone(),
            map_int32_int32: message
                .map_int32_int32
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect(),
            map_cardinality: message.map_int32_int32.len(),
            zero_map_value: message.map_int32_int32.get(&0).copied(),
            repeated_string: message
                .repeated_string
                .iter()
                .map(|value| value.as_bytes().to_vec())
                .collect(),
        }
    }
}

fn check_values(
    expected: &KnownFields,
    native: &PbrsTat,
    prost: &prost_tat::TestAllTypesProto3,
) -> Result<(), &'static str> {
    if &KnownFields::native(native) != expected {
        return Err("native known-field values differ from the constructed specimen");
    }
    if &KnownFields::prost(prost) != expected {
        return Err("Prost known-field values differ from the constructed specimen");
    }
    Ok(())
}

pub(super) fn check_wire(cell: &str, wire: &[u8]) {
    let expected = KnownFields::native(&pbrs_specimen_for_cell(cell));
    let native = PbrsTat::parse(wire).expect("preflight native decode");
    let prost = prost_tat::TestAllTypesProto3::decode(wire).expect("preflight Prost decode");
    check_values(&expected, &native, &prost).expect("complete codec fixture values must agree");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pbrs_specimen, touch_pbrs, touch_prost};

    fn populated_expected() -> KnownFields {
        KnownFields {
            optional_int32: 7,
            optional_int64: 1 << 40,
            optional_uint32: 99,
            optional_string: b"ada lovelace".to_vec(),
            optional_bytes: b"notes".to_vec(),
            optional_nested_message: Some((9, false)),
            repeated_int32: (0..8).collect(),
            packed_int32: (0..8).map(|value| value * 3).collect(),
            map_int32_int32: [(0, 0), (1, 1), (2, 4), (3, 9)].into_iter().collect(),
            map_cardinality: 4,
            zero_map_value: Some(0),
            repeated_string: Vec::new(),
        }
    }

    fn fixture() -> (KnownFields, PbrsTat, prost_tat::TestAllTypesProto3) {
        let expected = populated_expected();
        let native = pbrs_specimen();
        let wire = native.serialize().expect("fixture wire");
        let prost = prost_tat::TestAllTypesProto3::decode(wire.as_slice()).expect("fixture peer");
        assert_eq!(KnownFields::native(&native), expected);
        assert_eq!(KnownFields::prost(&prost), expected);
        (expected, native, prost)
    }

    #[test]
    fn complete_values_accept_legal_default_entry_wire_difference() {
        let (expected, native, prost) = fixture();
        assert_eq!(expected.map_int32_int32.len(), 4);
        assert_eq!(expected.map_int32_int32.get(&0), Some(&0));
        let native_wire = native.serialize().expect("native wire");
        let prost_wire = prost.encode_to_vec();
        assert_eq!(native_wire.len(), 87);
        assert_eq!(prost_wire.len(), 83);
        let native_from_prost = PbrsTat::parse(&prost_wire).expect("omitted entry defaults");
        assert_eq!(check_values(&expected, &native_from_prost, &prost), Ok(()));
    }

    #[test]
    fn dropping_zero_map_entry_fails_even_when_checksum_agrees() {
        for native_peer in [true, false] {
            let (expected, mut native, mut prost) = fixture();
            let old_checksums = (touch_pbrs(&native), touch_prost(&prost));
            if native_peer {
                assert!(native.map_int32_int32_mut().remove(0));
            } else {
                assert_eq!(prost.map_int32_int32.remove(&0), Some(0));
            }
            assert_eq!((touch_pbrs(&native), touch_prost(&prost)), old_checksums);
            assert!(check_values(&expected, &native, &prost).is_err());
        }
    }

    #[test]
    fn same_length_string_and_bytes_corruption_fails() {
        for native_peer in [true, false] {
            for string_field in [true, false] {
                let (expected, mut native, mut prost) = fixture();
                let old_checksums = (touch_pbrs(&native), touch_prost(&prost));
                match (native_peer, string_field) {
                    (true, true) => native.set_optional_string("Ada lovelace"),
                    (true, false) => native.set_optional_bytes(&b"Notes"[..]),
                    (false, true) => prost.optional_string = "Ada lovelace".to_owned(),
                    (false, false) => prost.optional_bytes = b"Notes".to_vec(),
                }
                assert_eq!((touch_pbrs(&native), touch_prost(&prost)), old_checksums);
                assert!(check_values(&expected, &native, &prost).is_err());
            }
        }
    }

    #[test]
    fn repeated_zero_loss_and_order_changes_fail() {
        for native_peer in [true, false] {
            for change in 0..4 {
                let (expected, mut native, mut prost) = fixture();
                let old_checksums = (touch_pbrs(&native), touch_prost(&prost));
                let mut repeated: Vec<i32> = (0..8).collect();
                let mut packed: Vec<i32> = (0..8).map(|value| value * 3).collect();
                match change {
                    0 => {
                        repeated.remove(0);
                    }
                    1 => {
                        packed.remove(0);
                    }
                    2 => repeated.swap(0, 1),
                    3 => packed.swap(0, 1),
                    _ => unreachable!(),
                }
                if native_peer {
                    native.set_repeated_int32(repeated);
                    native.set_packed_int32(packed);
                } else {
                    prost.repeated_int32 = repeated;
                    prost.packed_int32 = packed;
                }
                assert_eq!((touch_pbrs(&native), touch_prost(&prost)), old_checksums);
                assert!(check_values(&expected, &native, &prost).is_err());
            }
        }
    }

    #[test]
    fn scalars_and_nested_presence_and_values_are_exact() {
        for native_peer in [true, false] {
            for field in 0..6 {
                let (expected, mut native, mut prost) = fixture();
                if native_peer {
                    match field {
                        0 => native.set_optional_int32(8),
                        1 => native.set_optional_int64((1 << 40) + 1),
                        2 => native.set_optional_uint32(100),
                        3 => native.optional_nested_message_mut().set_a(10),
                        4 => native.clear_optional_nested_message(),
                        5 => native
                            .optional_nested_message_mut()
                            .corecursive_mut()
                            .set_optional_int32(1),
                        _ => unreachable!(),
                    }
                } else {
                    match field {
                        0 => prost.optional_int32 = 8,
                        1 => prost.optional_int64 = (1 << 40) + 1,
                        2 => prost.optional_uint32 = 100,
                        3 => prost.optional_nested_message.as_mut().unwrap().a = 10,
                        4 => prost.optional_nested_message = None,
                        5 => {
                            prost
                                .optional_nested_message
                                .as_mut()
                                .unwrap()
                                .corecursive
                                .get_or_insert_with(Default::default)
                                .optional_int32 = 1
                        }
                        _ => unreachable!(),
                    }
                }
                assert!(check_values(&expected, &native, &prost).is_err());
            }
        }
    }

    #[test]
    fn map_insertion_and_wire_order_are_not_value_differences() {
        let (expected, mut native, mut prost) = fixture();
        let original = native.serialize().expect("original wire");
        native.map_int32_int32_mut().clear();
        prost.map_int32_int32.clear();
        for key in [3, 1, 0, 2] {
            native.map_int32_int32_mut().insert(key, key * key);
            prost.map_int32_int32.insert(key, key * key);
        }
        let reordered = native.serialize().expect("reordered wire");
        assert_ne!(
            original, reordered,
            "this control actually changes map wire order"
        );
        let parsed = PbrsTat::parse(&reordered).expect("reordered native");
        let peer =
            prost_tat::TestAllTypesProto3::decode(reordered.as_slice()).expect("reordered peer");
        assert_eq!(check_values(&expected, &parsed, &peer), Ok(()));
        assert_eq!(check_values(&expected, &native, &prost), Ok(()));
    }

    fn tags_fixture() -> (KnownFields, PbrsTat, prost_tat::TestAllTypesProto3) {
        let specimen = pbrs_specimen_for_cell("codec.pbrs.tags_32_owned_decode");
        let expected = KnownFields::native(&specimen);
        let exact_tags: Vec<_> = (0..32)
            .map(|index| format!("t{index:02}").into_bytes())
            .collect();
        assert_eq!(expected.repeated_string, exact_tags);
        let wire = specimen.serialize().expect("tags fixture wire");
        let native = PbrsTat::parse(&wire).expect("tags native fixture");
        let prost =
            prost_tat::TestAllTypesProto3::decode(wire.as_slice()).expect("tags Prost fixture");
        assert_eq!(check_values(&expected, &native, &prost), Ok(()));
        (expected, native, prost)
    }

    #[test]
    fn repeated_string_same_length_corruption_fails_even_when_checksum_agrees() {
        for native_peer in [true, false] {
            let (expected, mut native, mut prost) = tags_fixture();
            let old_checksums = (touch_pbrs(&native), touch_prost(&prost));
            let mut tags: Vec<_> = (0..32).map(|index| format!("t{index:02}")).collect();
            tags[0] = "T00".to_owned();
            if native_peer {
                native.set_repeated_string(tags.into_iter().map(pbrs::rt::LazyStr::from));
            } else {
                prost.repeated_string = tags;
            }
            assert_eq!((touch_pbrs(&native), touch_prost(&prost)), old_checksums);
            assert!(check_values(&expected, &native, &prost).is_err());
        }
    }

    #[test]
    fn repeated_string_order_change_fails_even_when_checksum_agrees() {
        for native_peer in [true, false] {
            let (expected, mut native, mut prost) = tags_fixture();
            let old_checksums = (touch_pbrs(&native), touch_prost(&prost));
            let mut tags: Vec<_> = (0..32).map(|index| format!("t{index:02}")).collect();
            tags.swap(0, 31);
            if native_peer {
                native.set_repeated_string(tags.into_iter().map(pbrs::rt::LazyStr::from));
            } else {
                prost.repeated_string = tags;
            }
            assert_eq!((touch_pbrs(&native), touch_prost(&prost)), old_checksums);
            assert!(check_values(&expected, &native, &prost).is_err());
        }
    }

    #[test]
    fn special_codec_specimens_keep_their_distinct_values() {
        for cell in [
            "codec.pbrs.fresh_encode",
            "codec.pbrs.packed_256_owned_decode",
            "codec.pbrs.unpacked_256_parse_touch",
            "codec.pbrs.tags_32_owned_decode",
        ] {
            let input = pbrs_specimen_for_cell(cell).serialize().expect("cell wire");
            check_wire(cell, &input);
        }
    }
}
