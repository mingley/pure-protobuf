//! Same-schema encode/decode vs prost, protobuf v4 (upb), and buffa.

use buffa::{Message as BuffaMessage, MessageView};
use buffa_tat::protobuf_test_messages::proto3::{
    test_all_types_proto3::NestedMessage as BuffaNested, TestAllTypesProto3 as BuffaTat,
    TestAllTypesProto3View as BuffaTatView,
};
use prost::Message;
use protobuf::gencode::{NestedMessage, TestAllTypesProto3};
use protobuf::prelude::*;
use protobuf::testdata::{Address, Person};
use protobuf_v4::{Parse as V4Parse, Serialize as V4Serialize};
use std::time::Instant;

fn median_ns<F: FnMut()>(samples: usize, iters: u32, mut f: F) -> f64 {
    let mut xs: Vec<f64> = (0..samples).map(|_| bench_ns(iters, &mut f)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[samples / 2]
}

fn bench_ns<F: FnMut()>(iters: u32, mut f: F) -> f64 {
    for _ in 0..iters / 10 {
        f();
    }
    let t = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(f());
    }
    t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

fn tat_populated() -> TestAllTypesProto3 {
    let mut nested = NestedMessage::new();
    nested.set_a(9);
    let mut m = TestAllTypesProto3::new();
    m.set_optional_int32(7);
    m.set_optional_int64(1 << 40);
    m.set_optional_uint32(99);
    m.set_optional_string("ada lovelace");
    m.set_optional_bytes(&b"notes"[..]);
    m.set_optional_nested_message(nested);
    for i in 0..8 {
        m.repeated_int32_mut().push(i);
        m.packed_int32_mut().push(i * 3);
    }
    for i in 0..4 {
        m.map_int32_int32_mut().insert(i, i * i);
    }
    m
}

fn tat_packed() -> TestAllTypesProto3 {
    let mut m = TestAllTypesProto3::new();
    for i in 0..256 {
        m.packed_int32_mut().push(i);
    }
    m
}

fn tat_map() -> TestAllTypesProto3 {
    let mut m = TestAllTypesProto3::new();
    for i in 0..64 {
        m.map_int32_int32_mut().insert(i, i * i);
    }
    m
}

fn tat_nested() -> TestAllTypesProto3 {
    let mut inner = TestAllTypesProto3::new();
    inner.set_optional_int32(1);
    for _ in 0..8 {
        let mut outer = TestAllTypesProto3::new();
        outer.set_recursive_message(inner);
        inner = outer;
    }
    inner
}

fn tat_strings() -> TestAllTypesProto3 {
    let mut m = TestAllTypesProto3::new();
    m.set_optional_string("the quick brown fox jumps over the lazy dog");
    m.set_optional_string_piece("string piece payload for encode/decode");
    m.set_optional_cord("cord-shaped string used as a singular field");
    for s in ["alpha", "beta", "gamma", "delta"] {
        m.repeated_string_mut().push(s.into());
    }
    m
}

fn prost_of(m: &TestAllTypesProto3) -> prost_tat::TestAllTypesProto3 {
    let nested = m.optional_nested_message_opt().map(|n| {
        Box::new(prost_tat::test_all_types_proto3::NestedMessage {
            a: n.a(),
            ..Default::default()
        })
    });
    let rec = m.recursive_message_opt().map(|r| Box::new(prost_of(r)));
    prost_tat::TestAllTypesProto3 {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested,
        optional_string_piece: m.optional_string_piece().to_str().unwrap_or("").to_string(),
        optional_cord: m.optional_cord().to_str().unwrap_or("").to_string(),
        recursive_message: rec,
        repeated_int32: m.repeated_int32().iter().copied().collect(),
        map_int32_int32: m.map_int32_int32().iter().map(|(k, v)| (*k, *v)).collect(),
        packed_int32: m.packed_int32().iter().copied().collect(),
        repeated_string: m
            .repeated_string()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        ..Default::default()
    }
}

fn v4_of(m: &TestAllTypesProto3) -> v4_tat::TestAllTypesProto3 {
    let mut v = v4_tat::TestAllTypesProto3::new();
    v.set_optional_int32(m.optional_int32());
    v.set_optional_int64(m.optional_int64());
    v.set_optional_uint32(m.optional_uint32());
    v.set_optional_string(m.optional_string().to_str().unwrap_or(""));
    v.set_optional_bytes(m.optional_bytes());
    if let Some(n) = m.optional_nested_message_opt() {
        v.optional_nested_message_mut().set_a(n.a());
    }
    if let Some(s) = m
        .optional_string_piece()
        .to_str()
        .ok()
        .filter(|s| !s.is_empty())
    {
        v.set_optional_string_piece(s);
    }
    if let Some(s) = m.optional_cord().to_str().ok().filter(|s| !s.is_empty()) {
        v.set_optional_cord(s);
    }
    if let Some(r) = m.recursive_message_opt() {
        v.set_recursive_message(v4_of(r));
    }
    for i in m.repeated_int32().iter() {
        v.repeated_int32_mut().push(*i);
    }
    for (k, val) in m.map_int32_int32().iter() {
        v.map_int32_int32_mut().insert(*k, *val);
    }
    for i in m.packed_int32().iter() {
        v.packed_int32_mut().push(*i);
    }
    for s in m.repeated_string().iter() {
        v.repeated_string_mut()
            .push(s.as_view().to_str().unwrap_or(""));
    }
    v
}

fn buffa_of(m: &TestAllTypesProto3) -> BuffaTat {
    let nested = m.optional_nested_message_opt().map(|n| BuffaNested {
        a: n.a(),
        ..Default::default()
    });
    BuffaTat {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested.into(),
        optional_string_piece: m.optional_string_piece().to_str().unwrap_or("").to_string(),
        optional_cord: m.optional_cord().to_str().unwrap_or("").to_string(),
        recursive_message: m.recursive_message_opt().map(buffa_of).into(),
        repeated_int32: m.repeated_int32().iter().copied().collect(),
        map_int32_int32: m.map_int32_int32().iter().map(|(k, v)| (*k, *v)).collect(),
        packed_int32: m.packed_int32().iter().copied().collect(),
        repeated_string: m
            .repeated_string()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        ..Default::default()
    }
}

fn person_ours() -> Person {
    let mut addr = Address::new();
    addr.set_city("nyc");
    let mut p = Person::new();
    p.set_id(7);
    p.set_name("ada lovelace");
    p.set_email("ada@example.com");
    p.tags_mut().push("math".into());
    p.tags_mut().push("eng".into());
    p.scores_mut().insert("notes", 12);
    p.set_address(addr);
    p
}

#[derive(Clone, PartialEq, Message)]
struct ProstAddress {
    #[prost(string, tag = "1")]
    city: String,
}

#[derive(Clone, PartialEq, Message)]
struct ProstPerson {
    #[prost(int32, tag = "1")]
    id: i32,
    #[prost(string, tag = "2")]
    name: String,
    #[prost(string, optional, tag = "3")]
    email: Option<String>,
    #[prost(string, repeated, tag = "4")]
    tags: Vec<String>,
    #[prost(map = "string, int32", tag = "5")]
    scores: std::collections::HashMap<String, i32>,
    #[prost(message, optional, tag = "6")]
    address: Option<ProstAddress>,
}

struct Case {
    name: &'static str,
    ours_enc: f64,
    prost_enc: f64,
    v4_enc: f64,
    buffa_enc: f64,
    ours_dec: f64,
    prost_dec: f64,
    v4_dec: f64,
    buffa_dec: f64,
    buffa_view: Option<f64>,
    payload: usize,
    ours_def: Option<f64>,
}

fn run_tat(name: &'static str, msg: TestAllTypesProto3, iters: u32) -> Case {
    let bytes = protobuf::Serialize::serialize(&msg).expect("ours encode");
    let prost_msg = prost_of(&msg);
    let v4_msg = v4_of(&msg);
    let buffa_msg = buffa_of(&msg);
    let ours_def = if name == "tat_populated" {
        Some(median_ns(15, iters, || {
            let _ = TestAllTypesProto3::new();
        }))
    } else {
        None
    };
    Case {
        name,
        payload: bytes.len(),
        ours_enc: median_ns(15, iters, || {
            let _ = protobuf::Serialize::serialize(&msg).unwrap();
        }),
        prost_enc: median_ns(15, iters, || {
            let _ = prost_msg.encode_to_vec();
        }),
        v4_enc: median_ns(15, iters, || {
            let _ = V4Serialize::serialize(&v4_msg).unwrap();
        }),
        buffa_enc: median_ns(15, iters, || {
            let _ = BuffaMessage::encode_to_vec(&buffa_msg);
        }),
        ours_dec: median_ns(15, iters, || {
            let _ = TestAllTypesProto3::parse(&bytes).unwrap();
        }),
        prost_dec: median_ns(15, iters, || {
            let _ = prost_tat::TestAllTypesProto3::decode(bytes.as_slice()).unwrap();
        }),
        v4_dec: median_ns(15, iters, || {
            let _ = v4_tat::TestAllTypesProto3::parse(&bytes).unwrap();
        }),
        buffa_dec: median_ns(15, iters, || {
            let _ = BuffaTat::decode_from_slice(&bytes).unwrap();
        }),
        buffa_view: Some(median_ns(15, iters, || {
            let _ = BuffaTatView::decode_view(&bytes).unwrap();
        })),
        ours_def,
    }
}

fn run_person(iters: u32) -> Case {
    let msg = person_ours();
    let bytes = protobuf::Serialize::serialize(&msg).expect("person encode");
    let prost_msg = ProstPerson {
        id: msg.id(),
        name: msg.name().to_str().unwrap_or("").to_string(),
        email: msg
            .email_opt()
            .map(|s| s.to_str().unwrap_or("").to_string()),
        tags: msg
            .tags()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        scores: msg
            .scores()
            .iter()
            .map(|(k, v)| (k.as_view().to_str().unwrap_or("").to_string(), *v))
            .collect(),
        address: Some(ProstAddress {
            city: msg.address().city().to_str().unwrap_or("").to_string(),
        }),
    };
    let mut v4_msg = v4_person::Person::new();
    v4_msg.set_id(msg.id());
    v4_msg.set_name(msg.name().to_str().unwrap_or(""));
    v4_msg.set_email(msg.email().to_str().unwrap_or(""));
    for t in msg.tags().iter() {
        v4_msg.tags_mut().push(t.as_view().to_str().unwrap_or(""));
    }
    for (k, v) in msg.scores().iter() {
        v4_msg
            .scores_mut()
            .insert(k.as_view().to_str().unwrap_or(""), *v);
    }
    v4_msg
        .address_mut()
        .set_city(msg.address().city().to_str().unwrap_or(""));
    let buffa_msg = buffa_person::example::Person {
        id: msg.id(),
        name: msg.name().to_str().unwrap_or("").to_string(),
        email: msg
            .email_opt()
            .map(|s| s.to_str().unwrap_or("").to_string()),
        tags: msg
            .tags()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        scores: msg
            .scores()
            .iter()
            .map(|(k, v)| (k.as_view().to_str().unwrap_or("").to_string(), *v))
            .collect(),
        address: Some(buffa_person::example::Address {
            city: msg.address().city().to_str().unwrap_or("").to_string(),
            ..Default::default()
        })
        .into(),
        ..Default::default()
    };
    Case {
        name: "person",
        payload: bytes.len(),
        ours_enc: median_ns(15, iters, || {
            let _ = protobuf::Serialize::serialize(&msg).unwrap();
        }),
        prost_enc: median_ns(15, iters, || {
            let _ = prost_msg.encode_to_vec();
        }),
        v4_enc: median_ns(15, iters, || {
            let _ = V4Serialize::serialize(&v4_msg).unwrap();
        }),
        buffa_enc: median_ns(15, iters, || {
            let _ = BuffaMessage::encode_to_vec(&buffa_msg);
        }),
        ours_dec: median_ns(15, iters, || {
            let _ = Person::parse(&bytes).unwrap();
        }),
        prost_dec: median_ns(15, iters, || {
            let _ = ProstPerson::decode(bytes.as_slice()).unwrap();
        }),
        v4_dec: median_ns(15, iters, || {
            let _ = v4_person::Person::parse(&bytes).unwrap();
        }),
        buffa_dec: median_ns(15, iters, || {
            let _ = buffa_person::example::Person::decode_from_slice(&bytes).unwrap();
        }),
        buffa_view: None,
        ours_def: None,
    }
}

fn main() {
    let iters = 40_000u32;
    let cases = [
        run_tat("empty", TestAllTypesProto3::new(), iters),
        run_person(iters),
        run_tat("tat_populated", tat_populated(), iters),
        run_tat("packed_256", tat_packed(), iters),
        run_tat("map_64", tat_map(), iters),
        run_tat("nested_8", tat_nested(), iters),
        run_tat("strings", tat_strings(), iters),
    ];

    println!("{{");
    println!(
        "  \"sizeof_tat\": {},",
        std::mem::size_of::<TestAllTypesProto3>()
    );
    println!("  \"iters\": {iters},");
    println!("  \"cases\": [");
    for (i, c) in cases.iter().enumerate() {
        let comma = if i + 1 == cases.len() { "" } else { "," };
        println!("    {{");
        println!("      \"name\": \"{}\",", c.name);
        println!("      \"payload_bytes\": {},", c.payload);
        println!("      \"ours_encode_ns\": {:.3},", c.ours_enc);
        println!("      \"prost_encode_ns\": {:.3},", c.prost_enc);
        println!("      \"v4_encode_ns\": {:.3},", c.v4_enc);
        println!("      \"buffa_encode_ns\": {:.3},", c.buffa_enc);
        println!("      \"ours_decode_ns\": {:.3},", c.ours_dec);
        println!("      \"prost_decode_ns\": {:.3},", c.prost_dec);
        println!("      \"v4_decode_ns\": {:.3},", c.v4_dec);
        println!("      \"buffa_decode_ns\": {:.3},", c.buffa_dec);
        match c.buffa_view {
            Some(v) => println!("      \"buffa_view_decode_ns\": {v:.3},"),
            None => println!("      \"buffa_view_decode_ns\": null,"),
        }
        match c.ours_def {
            Some(v) => println!("      \"ours_default_ns\": {v:.3},"),
            None => println!("      \"ours_default_ns\": null,"),
        }
        println!(
            "      \"ours_faster_v4_encode\": {},",
            c.ours_enc < c.v4_enc
        );
        println!(
            "      \"ours_faster_v4_decode\": {},",
            c.ours_dec < c.v4_dec
        );
        println!(
            "      \"ours_faster_buffa_encode\": {},",
            c.ours_enc < c.buffa_enc
        );
        println!(
            "      \"ours_faster_buffa_decode\": {},",
            c.ours_dec < c.buffa_dec
        );
        println!(
            "      \"ours_faster_prost_encode\": {},",
            c.ours_enc < c.prost_enc
        );
        println!(
            "      \"ours_faster_prost_decode\": {},",
            c.ours_dec < c.prost_dec
        );
        match c.buffa_view {
            Some(v) => println!(
                "      \"ours_faster_buffa_view_decode\": {}",
                c.ours_dec < v
            ),
            None => println!("      \"ours_faster_buffa_view_decode\": null"),
        }
        println!("    }}{comma}");
    }
    println!("  ]");
    println!("}}");

    let mut failed = false;
    for c in &cases {
        if c.ours_enc >= c.prost_enc || c.ours_dec >= c.prost_dec {
            eprintln!("perf gate failed: {} vs prost", c.name);
            failed = true;
        }
        if c.ours_enc >= c.v4_enc || c.ours_dec >= c.v4_dec {
            eprintln!("perf gate failed: {} vs v4", c.name);
            failed = true;
        }
        if c.ours_enc >= c.buffa_enc || c.ours_dec >= c.buffa_dec {
            eprintln!("perf gate failed: {} vs buffa owned", c.name);
            failed = true;
        }
        if let Some(v) = c.buffa_view {
            if c.ours_dec >= v {
                eprintln!("perf gate failed: {} vs buffa view", c.name);
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
