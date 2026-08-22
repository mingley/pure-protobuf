//! Same-process Codec bench: `ProtobufCodec` vs tonic+prost `ProstCodec`.
//!
//! The Codec table is the result. Print it; do not treat this crate as a win
//! unless the table is a win.
//!
//! No unary RPC table. A serial pbrs-then-prost unary run (n=5) produced
//! an order artifact (896 µs vs 2.20 ms). That table was dropped rather
//! than interleaved.
//!
//! This is **not** `bench/` (kernel encode/decode vs prost / v4 / buffa).
//! This is **not** a Google C++/Go peer. hello.proto strings only; not
//! official interop `SimpleRequest.payload.body`.
//!
//! tonic 0.14 `EncodeBuf` / `DecodeBuf` have no public constructor. The
//! loops below run the same Encoder/Decoder bodies those codecs use:
//!
//! - `ProtobufCodec` encode: `Serialize::encode` into `BytesMut` (the
//!   `EncodeBuf` inner buffer). No per-message `Vec`.
//! - `ProtobufCodec` decode: `Parse` from contiguous bytes (`DecodeBuf::chunk`
//!   when the frame is one piece; typical unary). No pre-copy `Vec`.
//! - `ProstCodec` encode: `prost::Message::encode` into `BytesMut`.
//! - `ProstCodec` decode: `prost::Message::decode` from the buffer.

use bytes::BytesMut;
use pbrs::{Parse, Serialize};
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::time::Instant;

pub mod helloworld {
    include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
}

use helloworld::HelloRequest as ProstHello;

fn median_ns<F, R>(samples: usize, iters: u32, mut f: F) -> f64
where
    F: FnMut() -> R,
{
    let mut xs: Vec<f64> = (0..samples).map(|_| bench_ns(iters, &mut f)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[samples / 2]
}

fn bench_ns<F, R>(iters: u32, mut f: F) -> f64
where
    F: FnMut() -> R,
{
    for _ in 0..iters / 10 {
        std::hint::black_box(f());
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

/// `ProtobufEncoder`: write into the dst buffer (no per-message `Vec`).
fn pbrs_codec_encode(item: &PbrsHello, dst: &mut BytesMut) {
    dst.clear();
    Serialize::encode(item, dst).expect("pbrs encode");
}

/// `ProtobufDecoder`: parse contiguous frame bytes (no pre-copy `Vec`).
fn pbrs_codec_decode(src: &[u8]) -> PbrsHello {
    Parse::parse(src).expect("pbrs parse")
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
        dst.len()
    });
    let pbrs_dec = median_ns(samples, iters, || pbrs_codec_decode(&pbrs_wire));
    let prost_enc = median_ns(samples, iters, || {
        prost_codec_encode(&prost, &mut dst);
        dst.len()
    });
    let prost_dec = median_ns(samples, iters, || prost_codec_decode(&prost_wire));

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
    let codec_iters = 40_000u32;
    let codec_samples = 15usize;
    let codec_rows = [
        run_codec("hello", "ada", codec_iters, codec_samples),
        run_codec("hello_4kib", &"x".repeat(4096), codec_iters, codec_samples),
    ];

    println!("# Codec encode+decode (one unary HelloRequest; no transport)");
    println!();
    println!("This is the result. Encoder/Decoder body only.");
    println!("tonic EncodeBuf/DecodeBuf are crate-private newtypes over");
    println!("BytesMut / Buf; these are the same operations ProtobufCodec");
    println!("and ProstCodec run. Not kernel encode vs prost (see bench/).");
    println!("Not a Google C++/Go peer.");
    println!("hello.proto name/message strings only. Not interop payload.body.");
    println!("No unary RPC table (serial pbrs-then-prost was an order artifact).");
    println!("Do not treat this binary as a win unless the table is a win.");
    println!();
    println!("iters={codec_iters} samples={codec_samples} (median) release thin-LTO");
    println!();
    println!("| case | payload | ProtobufCodec enc / dec | ProstCodec enc / dec |");
    println!("|---|---:|---:|---:|");
    for r in &codec_rows {
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} |",
            r.name, r.payload, r.pbrs_enc, r.pbrs_dec, r.prost_enc, r.prost_dec
        );
    }
    println!();
    println!("ns/op. Combined encode+decode:");
    for r in &codec_rows {
        println!(
            "  {}  ProtobufCodec {:.1}  ProstCodec {:.1}",
            r.name,
            r.pbrs_enc + r.pbrs_dec,
            r.prost_enc + r.prost_dec
        );
    }
}
