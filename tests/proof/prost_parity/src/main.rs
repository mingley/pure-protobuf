use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use serde_json::json;

mod native {
    include!(concat!(env!("OUT_DIR"), "/pbrs_proof.rs"));
}
mod reference {
    include!(concat!(env!("OUT_DIR"), "/proof.rs"));
}
mod native_required {
    include!(concat!(env!("OUT_DIR"), "/pbrs_required.rs"));
}
mod reference_required {
    include!(concat!(env!("OUT_DIR"), "/required.rs"));
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn value(field: u32, v: u64) -> Vec<u8> {
    let mut out = vec![];
    varint(&mut out, (field as u64) << 3);
    varint(&mut out, v);
    out
}
fn len(field: u32, data: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    varint(&mut out, ((field as u64) << 3) | 2);
    varint(&mut out, data.len() as u64);
    out.extend_from_slice(data);
    out
}
fn fixed(field: u32, bits: u64, wide: bool) -> Vec<u8> {
    let mut out = vec![];
    varint(&mut out, ((field as u64) << 3) | if wide { 1 } else { 5 });
    out.extend_from_slice(&bits.to_le_bytes()[..if wide { 8 } else { 4 }]);
    out
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn compare(
    name: &str,
    wire: &[u8],
    initial: Option<&[u8]>,
    force_merge: bool,
) -> serde_json::Value {
    let operation = if initial.is_some() {
        "merge-populated"
    } else if force_merge {
        "merge-empty"
    } else {
        "parse"
    };
    let expected_result = if initial.is_some() || force_merge {
        let mut value = reference::Corpus::decode(initial.unwrap_or_default()).unwrap();
        value.merge(wire).map(|()| value)
    } else {
        reference::Corpus::decode(wire)
    };
    let actual_result = if initial.is_some() || force_merge {
        let mut value = native::Corpus::parse(initial.unwrap_or_default()).unwrap();
        value.merge_from_bytes(wire).map(|()| value)
    } else {
        native::Corpus::parse(wire)
    };
    let expected_ok = expected_result.is_ok();
    let actual_ok = actual_result.is_ok();
    let expected_normalized = expected_result.as_ref().ok().map(|v| v.encode_to_vec());
    let native_encoded_result = actual_result.as_ref().ok().map(|v| v.serialize());
    let native_encoded = native_encoded_result.as_ref().and_then(|r| r.as_ref().ok());
    let native_normalized = native_encoded
        .as_ref()
        .and_then(|b| reference::Corpus::decode(b.as_slice()).ok())
        .map(|v| v.encode_to_vec());
    let same =
        expected_ok == actual_ok && (!expected_ok || expected_normalized == native_normalized);
    let row = json!({"case":name,"operation":operation,"input":hex(wire),"initial":initial.map(hex),
        "prost_accepts":expected_ok,"pbrs_accepts":actual_ok,"equivalent":same,
        "prost_normalized":expected_normalized.as_deref().map(hex),
        "pbrs_encoded":native_encoded.map(|b|hex(b)),
        "pbrs_encode_error":native_encoded_result.as_ref().and_then(|r|r.as_ref().err()).map(|e|e.to_string()),
        "pbrs_normalized":native_normalized.as_deref().map(hex),
        "prost_error":expected_result.err().map(|e|e.to_string()),
        "pbrs_error":actual_result.err().map(|e|e.to_string())});
    println!("{row}");
    row
}
fn known_wrong_wire(row: &serde_json::Value) -> bool {
    row["case"] == "wrong-known-wire"
        && row["input"] == "0a00"
        && row["initial"].is_null()
        && (row["operation"] == "parse" || row["operation"] == "merge-empty")
        && row["prost_accepts"] == false
        && row["pbrs_accepts"] == true
        && row["pbrs_encoded"] == "0a00"
        && row["pbrs_encode_error"].is_null()
}

fn corpus() -> Vec<(String, Vec<u8>, Option<Vec<u8>>)> {
    let mut cases = vec![];
    let mut add = |name: &str, wire: Vec<u8>| cases.push((name.to_owned(), wire, None));
    add("empty", vec![]);
    for n in [
        0,
        1,
        127,
        128,
        255,
        16383,
        16384,
        u32::MAX as u64,
        (u32::MAX as u64) + 1,
        i64::MAX as u64,
        u64::MAX,
    ] {
        let mut wire = vec![];
        for field in [1, 2, 3, 4, 5, 6, 13, 21, 22, 24] {
            wire.extend(value(field, n));
        }
        add(&format!("varint-{n}"), wire);
    }
    for n in [
        0,
        1,
        u32::MAX as u64,
        u64::MAX,
        0x8000_0000,
        0x8000_0000_0000_0000,
    ] {
        let mut wire = vec![];
        for field in [7, 9, 11] {
            wire.extend(fixed(field, n, false));
        }
        for field in [8, 10, 12] {
            wire.extend(fixed(field, n, true));
        }
        add(&format!("fixed-{n:x}"), wire);
    }
    for bits in [
        0x7f80_0000,
        0xff80_0000,
        0x7fc0_0001,
        0x0000_0001,
        0x7f7f_ffff,
    ] {
        add(&format!("ieee-float-{bits:x}"), fixed(11, bits, false));
    }
    for bits in [
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_0000_0000_0001,
        0x0000_0000_0000_0001,
        0x7fef_ffff_ffff_ffff,
    ] {
        add(&format!("ieee-double-{bits:x}"), fixed(12, bits, true));
    }
    add("utf8", len(14, "Unicode 🦀 café \0".as_bytes()));
    add("opaque-bytes", len(15, &[0, 128, 255]));
    add("optional-zero", value(22, 0));
    add("unknown-enum", value(21, 987));
    add("unknown-varint", value(400, 987));
    add("unknown-len", len(401, &[255, 0]));
    add("unknown-fixed32", fixed(402, 1, false));
    add("unknown-fixed64", fixed(403, 1, true));
    // A well-formed unknown group, including an inner scalar.
    let mut group = vec![];
    varint(&mut group, 404 << 3 | 3);
    group.extend(value(1, 7));
    varint(&mut group, 404 << 3 | 4);
    add("unknown-group-valid", group);
    let child_a = value(1, 11);
    let child_b = len(2, b"right");
    add(
        "singular-merge",
        [len(18, &child_a), len(18, &child_b)].concat(),
    );
    add(
        "oneof-same-case-merge",
        [len(23, &child_a), len(23, &child_b)].concat(),
    );
    add(
        "oneof-switch",
        [len(23, &child_a), value(24, 9), len(23, &child_b)].concat(),
    );
    add(
        "repeated-children",
        [len(19, &child_a), len(19, &child_b)].concat(),
    );
    add(
        "packed-unpacked",
        [value(16, 1), len(16, &[2, 3]), value(16, u64::MAX)].concat(),
    );
    add("packed-empty", len(16, &[]));
    add(
        "fixed-packed-unpacked",
        [fixed(17, 1, false), len(17, &[2, 0, 0, 0, 3, 0, 0, 0])].concat(),
    );
    let entry = |key, child: &[u8]| [value(1, key), len(2, child)].concat();
    add(
        "map-duplicate",
        [len(20, &entry(7, &child_a)), len(20, &entry(7, &child_b))].concat(),
    );
    add(
        "map-unsigned-boundary",
        len(20, &entry(u32::MAX as u64, &child_a)),
    );
    add("map-default-key-value", len(20, &[]));
    add(
        "map-duplicate-value-merge",
        len(
            20,
            &[value(1, 7), len(2, &child_a), len(2, &child_b)].concat(),
        ),
    );
    for (name, wire) in [
        ("truncated-varint", vec![8, 128]),
        (
            "overflow-varint",
            [vec![8], vec![255; 10], vec![1]].concat(),
        ),
        ("zero-tag", vec![0]),
        ("invalid-wire6", vec![14]),
        ("wrong-known-wire", vec![10, 0]),
        ("truncated-len", vec![114, 5, 1]),
        ("invalid-utf8", vec![114, 1, 255]),
        ("truncated-fixed", vec![61, 1]),
        ("packed-truncated-fixed", len(17, &[1, 2, 3])),
        ("unmatched-end-group", vec![12]),
        ("unterminated-group", vec![251, 1, 8, 1]),
        ("nonminimal-varint", vec![8, 129, 0]),
        ("oversized-tag", vec![128, 128, 128, 128, 128, 1, 0]),
    ] {
        add(name, wire);
    }
    for depth in [1, 2, 32, 98, 99, 100, 101] {
        let mut nested = value(1, 7);
        for _ in 0..depth {
            nested = len(3, &nested);
        }
        add(&format!("recursion-{depth}"), len(18, &nested));
    }
    // Deterministic independent wire construction; each message includes all
    // scalar wire forms plus duplicate singular, packed and map occurrences.
    let mut state = 0xc013_2026_1027_0001u64;
    for seed in 0..256 {
        let mut wire = vec![];
        for _ in 0..24 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let n = state;
            let field = (n % 24 + 1) as u32;
            wire.extend(match field {
                7 | 9 | 11 | 17 => fixed(field, n, false),
                8 | 10 | 12 => fixed(field, n, true),
                14 => len(field, format!("s{n:x}").as_bytes()),
                15 => len(field, &n.to_le_bytes()),
                18 | 19 | 23 => len(
                    field,
                    &[value(1, n), len(2, format!("c{n:x}").as_bytes())].concat(),
                ),
                20 => len(field, &entry(n % 8, &value(1, n))),
                _ => value(field, n),
            });
        }
        cases.push((format!("seeded-{seed}"), wire, None));
    }
    let initial = [
        value(1, 4),
        value(16, 1),
        len(18, &child_a),
        len(20, &entry(7, &child_a)),
        len(23, &child_a),
    ]
    .concat();
    for (name, wire) in [
        ("merge-empty", vec![]),
        ("merge-scalars", value(1, 0)),
        ("merge-repeated", value(16, 9)),
        ("merge-child", len(18, &child_b)),
        ("merge-oneof", len(23, &child_b)),
        ("merge-map", len(20, &entry(7, &child_b))),
    ] {
        cases.push((name.into(), wire, Some(initial.clone())));
    }
    cases
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let check_known = match args.as_slice() {
        [] => false,
        [arg] if arg == "--check-known-differences" => true,
        _ => {
            eprintln!("usage: pbrs-prost-parity-proof [--check-known-differences]");
            std::process::exit(2);
        }
    };
    let cases = corpus();
    let mut mismatches = Vec::new();
    let mut operations = 0;
    for (name, wire, initial) in &cases {
        operations += 1;
        let row = compare(name, wire, initial.as_deref(), false);
        if row["equivalent"] == false {
            mismatches.push(row);
        }
        if initial.is_none() {
            operations += 1;
            let row = compare(name, wire, None, true);
            if row["equivalent"] == false {
                mismatches.push(row);
            }
        }
    }
    let empty_prost = reference_required::Required::decode(&[][..]).is_ok();
    let empty_pbrs = native_required::Required::parse(&[]).is_ok();
    println!(
        "{}",
        json!({"case":"required-missing","prost_accepts":empty_prost,"pbrs_accepts":empty_pbrs,"equivalent":empty_prost==empty_pbrs})
    );
    let wire = value(1, 987);
    let closed_prost = reference_required::Closed::decode(wire.as_slice()).unwrap();
    let closed_native = native_required::Closed::parse(&wire).unwrap();
    let closed_roundtrip =
        reference_required::Closed::decode(closed_native.serialize().unwrap().as_slice()).unwrap();
    println!(
        "{}",
        json!({"case":"proto2-unknown-enum","prost_value":closed_prost.state,"pbrs_getter":i32::from(closed_native.state()),"pbrs_roundtrip_value":closed_roundtrip.state,"wire_semantics_equivalent":closed_prost==closed_roundtrip})
    );
    let known_differences_match = cases.len() == 330
        && operations == 654
        && mismatches.len() == 2
        && mismatches.iter().all(known_wrong_wire)
        && mismatches.iter().any(|r| r["operation"] == "parse")
        && mismatches.iter().any(|r| r["operation"] == "merge-empty")
        && empty_prost
        && !empty_pbrs
        && closed_prost.state == Some(987)
        && i32::from(closed_native.state()) == 987
        && closed_prost == closed_roundtrip;
    let parity_equivalent = mismatches.is_empty()
        && empty_prost == empty_pbrs
        && closed_prost == closed_roundtrip
        && i32::from(closed_native.state()) == closed_prost.state.unwrap_or_default();
    let harness_passed = if check_known {
        known_differences_match
    } else {
        parity_equivalent
    };
    println!(
        "{}",
        json!({"summary":true,"proto3_wire_cases":cases.len(),"proto3_operations":operations,
        "proto3_mismatches":mismatches.len(),"required_missing_divergence":empty_prost!=empty_pbrs,
        "parity_equivalent":parity_equivalent,"harness_passed":harness_passed,
        "known_differences_match":known_differences_match,"check_known_differences_mode":check_known})
    );
    if !harness_passed {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn differential_corpus_retains_the_known_wrong_wire_counterexample() {
        let mut differences = Vec::new();
        for (name, wire, initial) in corpus() {
            if compare(&name, &wire, initial.as_deref(), false)["equivalent"] == false {
                differences.push((name.clone(), false));
            }
            if initial.is_none() && compare(&name, &wire, None, true)["equivalent"] == false {
                differences.push((name, true));
            }
        }
        assert_eq!(
            differences,
            vec![
                ("wrong-known-wire".into(), false),
                ("wrong-known-wire".into(), true)
            ]
        );
    }
    #[test]
    fn required_field_policy_divergence_is_visible() {
        assert!(reference_required::Required::decode(&[][..]).is_ok());
        assert!(native_required::Required::parse(&[]).is_err());
    }
}
