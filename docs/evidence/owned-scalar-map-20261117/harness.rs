//! Isolated source-pinned owned map diagnostic; not a production benchmark gate.
use super::{allocated, bytes, scalar};
use pbrs::Parse;
use std::hint::black_box;
use std::time::Instant;

pub(super) fn run() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    assert_eq!(args.len(), 6);
    let case = &args[0];
    let implementation = &args[1];
    let input = &args[2];
    let operation = &args[3];
    let iterations: usize = args[4].parse().unwrap();
    let warmup: usize = args[5].parse().unwrap();
    assert!(iterations > 0);
    assert!(matches!(input.as_str(), "borrowed" | "shared"));
    assert!(matches!(operation.as_str(), "owned_decode" | "parse_touch"));
    let (wire, expected) = corpus(case);
    let shared = pbrs::rt::Bytes::from(wire.clone());
    // Promote a Vec-backed Bytes outside the measured region.
    drop(shared.clone());
    let touch = operation == "parse_touch";
    macro_rules! scalar_run {
        ($type:path, $kind:ident) => {{
            let check = |message: &$type| -> u64 { scalar_check!(message, $kind) };
            let decode = || {
                let message = if input == "shared" {
                    <$type>::parse_bytes(black_box(shared.clone())).unwrap()
                } else {
                    <$type>::parse(black_box(wire.as_slice())).unwrap()
                };
                message
            };
            let parsed = decode();
            assert_eq!(check(&parsed), expected);
            measure(|| { let parsed = decode(); if touch { black_box(check(&parsed)); } black_box(parsed); }, iterations, warmup)
        }};
    }
    macro_rules! scalar_check {
        ($message:ident, native) => {{
            $message.values().iter().fold(0u64, |sum, (key, value)| sum.wrapping_add(key as u64)
                .wrapping_add(value.n() as u64).wrapping_add(value.stamp()).wrapping_add(u64::from(value.ready())))
        }};
    }
    macro_rules! blob_run {
        ($type:path) => {{
            let check = |message: &$type| -> u64 {
                message.values().iter().fold(0u64, |sum, (key, value)| {
                    let data = value.data();
                    sum.wrapping_add(key as u64).wrapping_add(data.len() as u64)
                        .wrapping_add(u64::from(data[0])).wrapping_add(u64::from(data[data.len()-1]))
                })
            };
            let decode = || {
                if input == "shared" { <$type>::parse_bytes(black_box(shared.clone())).unwrap() }
                else { <$type>::parse(black_box(wire.as_slice())).unwrap() }
            };
            assert_eq!(check(&decode()), expected);
            measure(|| { let parsed = decode(); if touch { black_box(check(&parsed)); } black_box(parsed); }, iterations, warmup)
        }};
    }
    macro_rules! node_run {
        ($type:path) => {{
            let check = |message: &$type| -> u64 {
                let mut total = message.n() as u64;
                let mut cursor = message;
                while let Some(child) = cursor.child_opt() {
                    total = total.wrapping_add(child.n() as u64);
                    cursor = child;
                }
                total
            };
            let decode = || {
                if input == "shared" { <$type>::parse_bytes(black_box(shared.clone())).unwrap() }
                else { <$type>::parse(black_box(wire.as_slice())).unwrap() }
            };
            assert_eq!(check(&decode()), expected);
            measure(|| { let parsed = decode(); if touch { black_box(check(&parsed)); } black_box(parsed); }, iterations, warmup)
        }};
    }
    let (wall_ns, allocations) = if case.starts_with("scalar") {
        match implementation.as_str() {
            "owned" => scalar_run!(super::owned::strict_types::ScalarRoot, native),
            "default" => scalar_run!(super::defaulted::strict_types::ScalarRoot, native),
            "prost" => {
                use prost::Message;
                let check = |message: &super::strict_reference::ScalarRoot| -> u64 {
                    message.values.iter().fold(0u64, |sum, (key, value)| sum.wrapping_add(*key as u64)
                        .wrapping_add(value.n as u64).wrapping_add(value.stamp).wrapping_add(u64::from(value.ready)))
                };
                let decode = || super::strict_reference::ScalarRoot::decode(black_box(wire.as_slice())).unwrap();
                assert_eq!(check(&decode()), expected);
                measure(|| { let parsed = decode(); if touch { black_box(check(&parsed)); } black_box(parsed); }, iterations, warmup)
            }
            _ => panic!("unknown implementation"),
        }
    } else if case.starts_with("blob") {
        match implementation.as_str() {
            "owned" => blob_run!(super::owned::strict_types::BlobRoot),
            "default" => blob_run!(super::defaulted::strict_types::BlobRoot),
            _ => panic!("unsupported blob implementation"),
        }
    } else if case == "node64" {
        match implementation.as_str() {
            "owned" => node_run!(super::owned::strict_types::CompactNode),
            _ => panic!("unsupported node implementation"),
        }
    } else { panic!("unknown case") };
    let wire_hex: String = wire.iter().map(|byte| format!("{byte:02x}")).collect();
    println!("{{\"schema\":\"owned-scalar-map/1\",\"case\":\"{case}\",\"implementation\":\"{implementation}\",\"input\":\"{input}\",\"operation\":\"{operation}\",\"iterations\":{iterations},\"warmup\":{warmup},\"wire_bytes\":{},\"wire_hex\":\"{wire_hex}\",\"expected_checksum\":{expected},\"wall_ns\":{wall_ns},\"allocation_iterations\":100,\"allocations\":{allocations}}}", wire.len());
}

fn measure(mut operation: impl FnMut(), iterations: usize, warmup: usize) -> (u128, usize) {
    for _ in 0..warmup { operation(); }
    let started = Instant::now();
    for _ in 0..iterations { operation(); }
    let elapsed = started.elapsed().as_nanos();
    let allocations = allocated(|| { for _ in 0..100 { operation(); } });
    (elapsed, allocations)
}

fn corpus(case: &str) -> (Vec<u8>, u64) {
    if let Some(count) = case.strip_prefix("scalar") {
        let unknown = count.ends_with("u");
        let count: u64 = count.trim_end_matches("u").parse().unwrap();
        assert!([1, 32, 512].contains(&count));
        let mut wire = Vec::new();
        let mut checksum = 0u64;
        for index in 1..=count {
            let stamp = index * 17;
            let ready = index % 2;
            let mut value = [scalar(1, index * 3), vec![0x11], stamp.to_le_bytes().to_vec(), scalar(3, ready)].concat();
            if unknown {
                value.extend(bytes(99, &vec![0xa5; 384]));
                value.extend(scalar(101, index));
            }
            let entry = [scalar(1, index), bytes(2, &value)].concat();
            wire.extend(bytes(1, &entry));
            checksum += index + index * 3 + stamp + ready;
        }
        (wire, checksum)
    } else if let Some(size) = case.strip_prefix("blob") {
        let size: usize = size.parse().unwrap();
        assert!([32, 8192].contains(&size));
        let mut wire = Vec::new();
        let mut checksum = 0u64;
        for index in 1..=32u64 {
            let payload = vec![index as u8; size];
            let entry = [scalar(1, index), bytes(2, &bytes(1, &payload))].concat();
            wire.extend(bytes(3, &entry));
            checksum += index * 3 + size as u64;
        }
        (wire, checksum)
    } else {
        assert_eq!(case, "node64");
        let mut wire = scalar(1, 1);
        for index in 2..=64 { wire = [scalar(1, index), bytes(2, &wire)].concat(); }
        (wire, 64 * 65 / 2)
    }
}
