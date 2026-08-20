//! TestAllTypes encode/decode vs prost, protobuf v4 (upb), and buffa.

use buffa::{Message as BuffaMessage, MessageView};
use buffa_tat::protobuf_test_messages::proto3::{
    test_all_types_proto3::NestedMessage as BuffaNested, TestAllTypesProto3 as BuffaTat,
    TestAllTypesProto3View as BuffaTatView,
};
use prost::Message;
use protobuf::gencode::{NestedMessage, TestAllTypesProto3};
use protobuf::prelude::*;
use protobuf_v4::{Parse as V4Parse, Serialize as V4Serialize};
use std::time::Instant;

#[derive(Clone, PartialEq, Message)]
struct ProstNested {
    #[prost(int32, tag = "1")]
    a: i32,
}

#[derive(Clone, PartialEq, Message)]
struct ProstTat {
    #[prost(int32, tag = "1")]
    optional_int32: i32,
    #[prost(int64, tag = "2")]
    optional_int64: i64,
    #[prost(uint32, tag = "3")]
    optional_uint32: u32,
    #[prost(string, tag = "14")]
    optional_string: String,
    #[prost(bytes, tag = "15")]
    optional_bytes: Vec<u8>,
    #[prost(message, optional, tag = "18")]
    optional_nested_message: Option<ProstNested>,
    #[prost(int32, repeated, packed = "true", tag = "31")]
    repeated_int32: Vec<i32>,
    #[prost(map = "int32, int32", tag = "56")]
    map_int32_int32: std::collections::HashMap<i32, i32>,
    #[prost(int32, repeated, packed = "true", tag = "75")]
    packed_int32: Vec<i32>,
}

fn ours() -> TestAllTypesProto3 {
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

fn prost_of(m: &TestAllTypesProto3) -> ProstTat {
    ProstTat {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: m
            .optional_nested_message_opt()
            .map(|n| ProstNested { a: n.a() }),
        repeated_int32: m.repeated_int32().iter().copied().collect(),
        map_int32_int32: m.map_int32_int32().iter().map(|(k, v)| (*k, *v)).collect(),
        packed_int32: m.packed_int32().iter().copied().collect(),
    }
}

fn buffa_of(m: &TestAllTypesProto3) -> BuffaTat {
    let nested = BuffaNested {
        a: m.optional_nested_message_opt().map(|n| n.a()).unwrap_or(0),
        ..Default::default()
    };
    BuffaTat {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested.into(),
        repeated_int32: m.repeated_int32().iter().copied().collect(),
        map_int32_int32: m.map_int32_int32().iter().map(|(k, v)| (*k, *v)).collect(),
        packed_int32: m.packed_int32().iter().copied().collect(),
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
    v.optional_nested_message_mut()
        .set_a(m.optional_nested_message_opt().map(|n| n.a()).unwrap_or(0));
    for i in m.repeated_int32().iter() {
        v.repeated_int32_mut().push(*i);
    }
    for (k, val) in m.map_int32_int32().iter() {
        v.map_int32_int32_mut().insert(*k, *val);
    }
    for i in m.packed_int32().iter() {
        v.packed_int32_mut().push(*i);
    }
    v
}

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

fn main() {
    let msg = ours();
    let bytes = protobuf::Serialize::serialize(&msg).expect("ours encode");
    let prost_msg = prost_of(&msg);
    let prost_buf = prost_msg.encode_to_vec();
    let v4_msg = v4_of(&msg);
    let v4_bytes = V4Serialize::serialize(&v4_msg).expect("v4 encode");
    let buffa_msg = buffa_of(&msg);
    let buffa_buf = BuffaMessage::encode_to_vec(&buffa_msg);

    let iters = 40_000u32;
    let ours_enc = median_ns(9, iters, || {
        let _ = protobuf::Serialize::serialize(&msg).unwrap();
    });
    let prost_enc = median_ns(9, iters, || {
        let _ = prost_msg.encode_to_vec();
    });
    let ours_def = median_ns(9, iters, || {
        let _ = TestAllTypesProto3::new();
    });
    let ours_dec = median_ns(9, iters, || {
        let _ = TestAllTypesProto3::parse(&bytes).unwrap();
    });
    let prost_dec = median_ns(9, iters, || {
        let _ = ProstTat::decode(bytes.as_slice()).unwrap();
    });
    let v4_enc = median_ns(9, iters, || {
        let _ = V4Serialize::serialize(&v4_msg).unwrap();
    });
    let v4_dec = median_ns(9, iters, || {
        let _ = v4_tat::TestAllTypesProto3::parse(&bytes).unwrap();
    });
    let buffa_enc = median_ns(9, iters, || {
        let _ = BuffaMessage::encode_to_vec(&buffa_msg);
    });
    let buffa_dec = median_ns(9, iters, || {
        let _ = BuffaTat::decode_from_slice(&bytes).unwrap();
    });
    let buffa_view_dec = median_ns(9, iters, || {
        let _ = BuffaTatView::decode_view(&bytes).unwrap();
    });

    println!("{{");
    println!(
        "  \"sizeof_tat\": {},",
        std::mem::size_of::<TestAllTypesProto3>()
    );
    println!("  \"payload_bytes_ours\": {},", bytes.len());
    println!("  \"payload_bytes_prost\": {},", prost_buf.len());
    println!("  \"payload_bytes_v4\": {},", v4_bytes.len());
    println!("  \"payload_bytes_buffa\": {},", buffa_buf.len());
    println!("  \"iters\": {iters},");
    println!("  \"ours_encode_ns\": {ours_enc:.3},");
    println!("  \"prost_encode_ns\": {prost_enc:.3},");
    println!("  \"v4_encode_ns\": {v4_enc:.3},");
    println!("  \"buffa_encode_ns\": {buffa_enc:.3},");
    println!("  \"ours_default_ns\": {ours_def:.3},");
    println!("  \"ours_decode_ns\": {ours_dec:.3},");
    println!("  \"prost_decode_ns\": {prost_dec:.3},");
    println!("  \"v4_decode_ns\": {v4_dec:.3},");
    println!("  \"buffa_decode_ns\": {buffa_dec:.3},");
    println!("  \"buffa_view_decode_ns\": {buffa_view_dec:.3},");
    println!("  \"ours_faster_prost_encode\": {},", ours_enc < prost_enc);
    println!("  \"ours_faster_prost_decode\": {},", ours_dec < prost_dec);
    println!("  \"ours_faster_v4_encode\": {},", ours_enc < v4_enc);
    println!("  \"ours_faster_v4_decode\": {},", ours_dec < v4_dec);
    println!("  \"ours_faster_buffa_encode\": {},", ours_enc < buffa_enc);
    println!("  \"ours_faster_buffa_decode\": {},", ours_dec < buffa_dec);
    println!("  \"v4\": \"typed TestAllTypesProto3 via protoc --rust_out kernel=upb, protobuf =4.35.1-release\",");
    println!("  \"buffa\": \"buffa 0.9.1 generated TestAllTypesProto3 (owned encode/decode; view decode separate)\"");
    println!("}}");

    if ours_enc >= v4_enc || ours_dec >= v4_dec {
        eprintln!("perf gate failed: must beat protobuf v4 TAT encode and decode");
        std::process::exit(1);
    }
    if ours_enc >= buffa_enc || ours_dec >= buffa_dec {
        eprintln!("perf gate failed: must beat buffa owned TAT encode and decode");
        std::process::exit(1);
    }
}
