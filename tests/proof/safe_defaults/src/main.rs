#![cfg_attr(not(feature = "ordinary-control"), forbid(unsafe_code))]

use pbrs::{Clear, Parse, Serialize};
use std::hint::black_box;
use std::time::Instant;

include!(concat!(env!("OUT_DIR"), "/mod.rs"));

use safe::defaults::{Child, Presence, Record, Required, State};

fn populated_record() -> Record {
    let mut value = Record::new();
    value.set_id(91);
    value.set_text("message");
    value.set_data(vec![1, 2, 3]);
    value.tags_mut().push("tag");
    value.blobs_mut().push(vec![4, 5]);
    value.numbers_mut().push(-7);
    value.numbers_mut().push(300);
    value.counts_mut().insert("key", 19);
    value.payloads_mut().insert(1, vec![8, 9]);
    value.child_mut().set_value(13);
    let mut child = Child::new();
    child.set_value(11);
    value.children_mut().push(child.clone());
    value.set_flag(false);
    assert!(value.has_flag());
    value.set_chosen(child);
    assert!(!value.has_flag() && value.has_chosen());
    value.set_enabled(false);
    value.set_total(0);
    value.set_ratio(1.25);
    value.set_sample(2.5);
    value
}

fn populated_presence() -> Presence {
    let mut value = Presence::new();
    value.set_enabled(false);
    value.set_count(23);
    value.set_sample(1.25);
    value.set_ratio(2.5);
    value.set_name("changed");
    value.set_data(vec![9, 8]);
    value.child_mut().set_value(29);
    value.values_mut().push(-3);
    value.flags_mut().push(true);
    value.weights_mut().push(0.5);
    value.samples_mut().push(1.25);
    value.offsets_mut().push(-5);
    value.counts_mut().insert("entry", 37);
    value.set_flag(false);
    assert!(value.has_flag());
    let mut child = Child::new();
    child.set_value(41);
    value.set_picked(child.clone());
    value.children_mut().push(child);
    assert!(!value.has_flag() && value.has_picked());
    value.legacy_mut().set_value(43);
    value
}

fn check() {
    let mut value = Record::new();
    assert_eq!(value, Record::default());
    assert!(value.serialize().unwrap().is_empty());
    assert_eq!(Record::parse(&[]).unwrap(), value);
    assert!(!value.has_child() && !value.has_enabled() && !value.has_total());
    assert_eq!(value.child().value(), 7);
    assert!(!value.enabled() && value.total() == 0);
    value.set_enabled(false);
    value.set_total(0);
    assert!(value.has_enabled() && value.has_total());
    assert_eq!(value.serialize().unwrap(), [0x78, 0, 0x80, 0x01, 0]);
    value.clear();
    assert_eq!(value, Record::default());
    println!("PASS proto3 empty/default/presence/clear");

    let value = Presence::new();
    assert!(value.enabled() && !value.has_enabled());
    assert_eq!(
        (value.count(), value.delta(), value.limit(), value.maximum()),
        (17, -7, 19, 23)
    );
    assert!(value.sample().is_nan() && value.ratio().is_infinite());
    assert_eq!(value.name(), "fallback");
    assert_eq!(value.data(), [1, 0]);
    assert_eq!(i32::from(value.state()), 9);
    assert_eq!(i32::from(State::default()), 3);
    assert_eq!(value.child().value(), 7);
    assert_eq!(value.legacy().value(), 11);
    assert!(value.serialize().unwrap().is_empty());
    assert_eq!(value, Presence::parse(&[]).unwrap());
    assert!(Required::parse(&[]).is_err());
    let mut required = Required::new();
    required.set_value(0);
    assert_eq!(
        Required::parse(&required.serialize().unwrap())
            .unwrap()
            .value(),
        0
    );
    println!("PASS proto2 declared defaults/nonzero enum/required/group");

    let value = populated_record();
    let wire = value.serialize().unwrap();
    assert_eq!(Record::parse(&wire).unwrap(), value);
    assert_eq!(Record::parse_bytes(wire.clone().into()).unwrap(), value);
    let cloned = value.clone();
    drop(value);
    assert_eq!(cloned.serialize().unwrap(), wire);
    let mut unknown = wire;
    unknown.extend_from_slice(&[0xa0, 0x06, 0x7b]);
    assert_eq!(
        Record::parse(&unknown).unwrap().serialize().unwrap(),
        unknown
    );
    assert!(Record::parse(&[0x12, 2, 0xff]).is_err());
    println!("PASS proto3 scalar/bytes/repeated/map/oneof/unknown/clone");

    let mut value = populated_presence();
    assert_eq!(Presence::parse(&value.serialize().unwrap()).unwrap(), value);
    value.clear();
    assert_eq!(value, Presence::default());
    println!("PASS cold-storage defaults/mutation/roundtrip/clear");
}

fn read_record(value: &Record) -> u64 {
    let bytes = |input: &[u8]| input.iter().map(|v| u64::from(*v)).sum::<u64>();
    let mut sum = value.id() as u64;
    sum = sum.wrapping_add(bytes(value.text().as_bytes()));
    sum = sum.wrapping_add(bytes(value.data()));
    for tag in value.tags().iter() {
        sum = sum.wrapping_add(bytes(tag.as_bytes()));
    }
    for blob in value.blobs().iter() {
        sum = sum.wrapping_add(bytes(blob.as_bytes()));
    }
    for number in value.numbers().iter() {
        sum = sum.wrapping_add(number as u64);
    }
    sum = sum.wrapping_add(value.child().value() as u64);
    for child in value.children().iter() {
        sum = sum.wrapping_add(child.value() as u64);
    }
    for (key, number) in value.counts().iter() {
        sum = sum.wrapping_add(bytes(key.as_bytes()));
        sum = sum.wrapping_add(number as u64);
    }
    for (key, payload) in value.payloads().iter() {
        sum = sum.wrapping_add(key as u64);
        sum = sum.wrapping_add(bytes(payload.as_bytes()));
    }
    sum = sum.wrapping_add(u64::from(value.has_chosen()));
    sum = sum.wrapping_add(value.chosen().value() as u64);
    sum = sum.wrapping_add(u64::from(value.has_enabled()));
    sum = sum.wrapping_add(u64::from(value.enabled()));
    sum = sum.wrapping_add(u64::from(value.has_total()));
    sum = sum.wrapping_add(value.total() as u64);
    sum = sum.wrapping_add(value.ratio().to_bits());
    sum.wrapping_add(u64::from(value.sample().to_bits()))
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 1 {
        check();
        return;
    }
    let iterations: u64 = args.get(2).expect("ITERATIONS").parse().unwrap();
    let record = populated_record();
    let wire = record.serialize().unwrap();
    let expected = read_record(&record);
    assert_eq!(read_record(&Record::parse(&wire).unwrap()), expected);
    let start = Instant::now();
    let mut sink = 0_u64;
    for _ in 0..iterations {
        match args[1].as_str() {
            "default" => {
                let value = Record::default();
                black_box(&value);
            }
            "empty_encode" => {
                let value = Record::default();
                black_box(value.serialize().unwrap());
            }
            "parse" => {
                black_box(Record::parse(black_box(&wire)).unwrap());
            }
            "read_all" => {
                let value = Record::parse(black_box(&wire)).unwrap();
                sink = sink.wrapping_add(black_box(read_record(&value)));
            }
            _ => panic!("expected default, empty_encode, parse or read_all"),
        }
    }
    let elapsed = start.elapsed().as_nanos();
    if args[1] == "read_all" {
        assert_eq!(sink, expected.wrapping_mul(iterations));
    }
    black_box(sink);
    let wire_hex: String = wire.iter().map(|byte| format!("{byte:02x}")).collect();
    println!(
        "{{\"case\":\"{}\",\"iterations\":{},\"wall_ns\":{},\"ordinary\":{},\"input_wire_hex\":\"{}\",\"expected_checksum\":{},\"record_size\":{},\"presence_size\":{}}}",
        args[1],
        iterations,
        elapsed,
        cfg!(feature = "ordinary-control"),
        wire_hex,
        expected,
        std::mem::size_of::<Record>(),
        std::mem::size_of::<Presence>(),
    );
}
