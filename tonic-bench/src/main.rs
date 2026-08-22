//! Same-process unary Codec bench: `ProtobufCodec` vs tonic+prost `ProstCodec`.
//!
//! This is **not** `bench/` (kernel encode/decode vs prost / v4 / buffa).
//! This is **not** a Google C++/Go peer. hello.proto strings only; not
//! official interop `SimpleRequest.payload.body`.
//!
//! tonic 0.14 `EncodeBuf` / `DecodeBuf` have no public constructor. The
//! loops below run the same Encoder/Decoder bodies those codecs use:
//!
//! - `ProtobufCodec` encode: `Serialize` to a new `Vec`, then `put_slice`
//!   into `BytesMut` (the `EncodeBuf` inner buffer).
//! - `ProtobufCodec` decode: copy-all into a new `Vec`, then `Parse`.
//! - `ProstCodec` encode: `prost::Message::encode` into `BytesMut`.
//! - `ProstCodec` decode: `prost::Message::decode` from the buffer.
//!
//! `ProtobufCodec` currently allocates a `Vec` per message on both sides.

use bytes::{Buf, BufMut, BytesMut};
use pbrs::{Parse, Serialize};
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::time::Instant;

pub mod helloworld {
    include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
}

use helloworld::HelloRequest as ProstHello;

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

fn pbrs_hello(name: &str) -> PbrsHello {
    let mut m = PbrsHello::new();
    m.set_name(name);
    m
}

fn prost_hello(name: &str) -> ProstHello {
    ProstHello {
        name: name.to_string(),
    }
}

/// `ProtobufEncoder`: serialize-to-Vec, then `put_slice` into the dst buffer.
fn pbrs_codec_encode(item: &PbrsHello, dst: &mut BytesMut) {
    dst.clear();
    let bytes = Serialize::serialize(item).expect("pbrs serialize");
    dst.put_slice(&bytes);
}

/// `ProtobufDecoder`: copy remaining bytes into a fresh Vec, then `Parse`.
fn pbrs_codec_decode(src: &[u8]) -> PbrsHello {
    let mut buf = src;
    let mut copy = vec![0u8; buf.remaining()];
    buf.copy_to_slice(&mut copy);
    Parse::parse(&copy).expect("pbrs parse")
}

/// `ProstEncoder`: prost writes directly into the dst buffer.
fn prost_codec_encode(item: &ProstHello, dst: &mut BytesMut) {
    dst.clear();
    prost::Message::encode(item, dst).expect("prost encode");
}

/// `ProstDecoder`: prost parses from the buffer (no pre-copy Vec).
fn prost_codec_decode(src: &[u8]) -> ProstHello {
    prost::Message::decode(src).expect("prost decode")
}

struct CodecRow {
    name: &'static str,
    payload: usize,
    pbrs_enc: f64,
    pbrs_dec: f64,
    prost_enc: f64,
    prost_dec: f64,
}

fn run_codec(name: &'static str, field: &str, iters: u32, samples: usize) -> CodecRow {
    let pbrs = pbrs_hello(field);
    let prost = prost_hello(field);
    let pbrs_wire = Serialize::serialize(&pbrs).expect("pbrs wire");
    let mut prost_wire = Vec::new();
    prost::Message::encode(&prost, &mut prost_wire).expect("prost wire");
    assert_eq!(
        pbrs_wire, prost_wire,
        "pbrs and prost must encode the same hello.proto bytes"
    );
    let payload = pbrs_wire.len();

    let mut dst = BytesMut::new();
    let pbrs_enc = median_ns(samples, iters, || {
        pbrs_codec_encode(&pbrs, &mut dst);
        std::hint::black_box(dst.len());
    });
    let pbrs_dec = median_ns(samples, iters, || {
        std::hint::black_box(pbrs_codec_decode(&pbrs_wire));
    });
    let prost_enc = median_ns(samples, iters, || {
        prost_codec_encode(&prost, &mut dst);
        std::hint::black_box(dst.len());
    });
    let prost_dec = median_ns(samples, iters, || {
        std::hint::black_box(prost_codec_decode(&prost_wire));
    });

    CodecRow {
        name,
        payload,
        pbrs_enc,
        pbrs_dec,
        prost_enc,
        prost_dec,
    }
}

fn main() {
    let iters = 40_000u32;
    let samples = 15usize;
    let rows = [
        run_codec("hello", "ada", iters, samples),
        run_codec("hello_4kib", &"x".repeat(4096), iters, samples),
    ];

    println!("# Codec encode+decode (one unary HelloRequest; no transport)");
    println!();
    println!("Encoder/Decoder body only. tonic EncodeBuf/DecodeBuf are crate-private");
    println!("newtypes over BytesMut / Buf; these are the same operations");
    println!("ProtobufCodec and ProstCodec run. Not kernel encode vs prost (see bench/).");
    println!("Not a Google C++/Go peer. ProtobufCodec allocates a Vec per message.");
    println!("hello.proto name/message strings only. Not interop payload.body.");
    println!();
    println!("iters={iters} samples={samples} (median) release thin-LTO");
    println!();
    println!("| case | payload | ProtobufCodec enc / dec | ProstCodec enc / dec |");
    println!("|---|---:|---:|---:|");
    for r in &rows {
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} |",
            r.name, r.payload, r.pbrs_enc, r.pbrs_dec, r.prost_enc, r.prost_dec
        );
    }
    println!();
    println!("ns/op. Combined encode+decode:");
    for r in &rows {
        println!(
            "  {}  ProtobufCodec {:.1}  ProstCodec {:.1}",
            r.name,
            r.pbrs_enc + r.pbrs_dec,
            r.prost_enc + r.prost_dec
        );
    }
}
