//! Encode/decode throughput vs prost (and protobuf v4 when it builds).

use prost::Message;
use protobuf::prelude::*;
use protobuf::testdata::{Address, Person};
use protobuf_v4::{Parse as V4Parse, Serialize as V4Serialize};
use std::time::Instant;

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

#[derive(Clone, PartialEq, Message)]
struct ProstAddress {
    #[prost(string, tag = "1")]
    city: String,
}

fn ours() -> Person {
    let mut p = proto!(Person {
        id: 42,
        name: "ada lovelace",
        email: "ada@analytical.engine",
        address: Address { city: "london" },
    });
    for t in [
        "math",
        "poet",
        "analyst",
        "programmer",
        "translator",
        "note-g",
        "first computer",
        "bernoulli",
    ] {
        p.tags_mut().push(t.into());
    }
    p
}

fn prost_of(p: &Person) -> ProstPerson {
    ProstPerson {
        id: p.id(),
        name: p.name().to_str().unwrap_or("").to_string(),
        email: if p.has_email() {
            Some(p.email().to_str().unwrap_or("").to_string())
        } else {
            None
        },
        tags: p
            .tags()
            .iter()
            .map(|t| t.to_str().unwrap_or("").to_string())
            .collect(),
        scores: Default::default(),
        address: Some(ProstAddress {
            city: p.address().city().to_str().unwrap_or("").to_string(),
        }),
    }
}

fn v4_of(p: &Person) -> v4_person::Person {
    let mut v = v4_person::Person::new();
    v.set_id(p.id());
    v.set_name(p.name().to_str().unwrap_or(""));
    if p.has_email() {
        v.set_email(p.email().to_str().unwrap_or(""));
    }
    for t in p.tags().iter() {
        v.tags_mut().push(t.to_str().unwrap_or(""));
    }
    v.address_mut()
        .set_city(p.address().city().to_str().unwrap_or(""));
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
    let mut prost_buf = Vec::new();
    prost_msg.encode(&mut prost_buf).unwrap();
    assert_eq!(
        bytes.len(),
        prost_buf.len(),
        "payloads must be the same size for a fair bench"
    );

    let iters = 80_000u32;
    let ours_enc = median_ns(9, iters, || {
        let _ = protobuf::Serialize::serialize(&msg).unwrap();
    });
    let prost_enc = median_ns(9, iters, || {
        let _ = prost_msg.encode_to_vec();
    });
    let ours_dec = median_ns(9, iters, || {
        let _ = Person::parse(&bytes).unwrap();
    });
    let prost_dec = median_ns(9, iters, || {
        let _ = ProstPerson::decode(bytes.as_slice()).unwrap();
    });
    let v4_msg = v4_of(&msg);
    let v4_bytes = V4Serialize::serialize(&v4_msg).expect("v4 encode");
    assert_eq!(
        bytes.len(),
        v4_bytes.len(),
        "v4 payload size must match ours for a fair bench"
    );
    let v4_enc = median_ns(9, iters, || {
        let _ = V4Serialize::serialize(&v4_msg).unwrap();
    });
    let v4_dec = median_ns(9, iters, || {
        let _ = v4_person::Person::parse(&bytes).unwrap();
    });

    let ours_enc_mbs = (bytes.len() as f64) / (ours_enc / 1e9) / 1e6;
    let prost_enc_mbs = (bytes.len() as f64) / (prost_enc / 1e9) / 1e6;
    let ours_dec_mbs = (bytes.len() as f64) / (ours_dec / 1e9) / 1e6;
    let prost_dec_mbs = (bytes.len() as f64) / (prost_dec / 1e9) / 1e6;

    println!("{{");
    println!("  \"payload_bytes\": {},", bytes.len());
    println!("  \"iters\": {iters},");
    println!("  \"ours_encode_ns\": {ours_enc:.3},");
    println!("  \"prost_encode_ns\": {prost_enc:.3},");
    println!("  \"ours_decode_ns\": {ours_dec:.3},");
    println!("  \"prost_decode_ns\": {prost_dec:.3},");
    println!("  \"ours_encode_MBps\": {ours_enc_mbs:.3},");
    println!("  \"prost_encode_MBps\": {prost_enc_mbs:.3},");
    println!("  \"ours_decode_MBps\": {ours_dec_mbs:.3},");
    println!("  \"prost_decode_MBps\": {prost_dec_mbs:.3},");
    println!("  \"ours_faster_encode\": {},", ours_enc < prost_enc);
    println!("  \"ours_faster_decode\": {},", ours_dec < prost_dec);
    println!("  \"v4_encode_ns\": {v4_enc:.3},");
    println!("  \"v4_decode_ns\": {v4_dec:.3},");
    println!("  \"v4_encode_MBps\": {:.3},", (bytes.len() as f64) / (v4_enc / 1e9) / 1e6);
    println!("  \"v4_decode_MBps\": {:.3},", (bytes.len() as f64) / (v4_dec / 1e9) / 1e6);
    println!("  \"ours_faster_v4_encode\": {},", ours_enc < v4_enc);
    println!("  \"ours_faster_v4_decode\": {},", ours_dec < v4_dec);
    println!("  \"v4\": \"typed Person via protoc --rust_out kernel=upb, protobuf =4.35.1-release\"");
    println!("}}");

    if ours_enc >= prost_enc || ours_dec >= prost_dec {
        eprintln!("perf gate failed: must beat prost encode and decode");
        std::process::exit(1);
    }
    if ours_enc >= v4_enc || ours_dec >= v4_dec {
        eprintln!("perf gate failed: must beat protobuf v4 encode and decode");
        std::process::exit(1);
    }
}
