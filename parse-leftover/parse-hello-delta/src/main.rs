//! Throwaway leftover hello Parse Δ timing after the inline Arc skip.
//! Not a workspace member.
//!
//! MEASURE ONLY. Not a win. Does not change the kernel.
//! Same Instant median as tonic-bench (40000 × 15, thin LTO).
//! Inventory: `docs/inventory/parse-hello-delta.md`.

use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::mem::size_of;
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

/// Generated string arm of HelloRequest::merge_inner after #34.
/// Public rt helpers only: tag, UTF-8, `from_parse_span`. No Default,
/// cached_size, trait layers, unknown, or required.
fn reconstruct_string_arm(data: &[u8]) -> pbrs::rt::LazyStr {
    let mut pos = 0;
    let mut wire = None;
    let (n, w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    debug_assert_eq!(n, 1);
    debug_assert_eq!(w, pbrs::rt::WIRE_LEN);
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    let b = &data[s..e];
    std::str::from_utf8(b).expect("utf8");
    pbrs::rt::LazyStr::from_parse_span(&mut wire, data, s, e)
}

/// Pre-#34 string arm (Wire::ensure + from_span). Off the hello path now.
/// Kept only to show the skipped Arc is not in the leftover.
fn reconstruct_old_ensure_arm(data: &[u8]) -> pbrs::rt::LazyStr {
    let mut pos = 0;
    let mut wire = None;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    let b = &data[s..e];
    std::str::from_utf8(b).expect("utf8");
    pbrs::rt::LazyStr::from_span(pbrs::rt::Wire::ensure(&mut wire, data), s, e)
}

fn tag_walk_only(data: &[u8]) -> (usize, usize) {
    let mut pos = 0;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    pbrs::rt::read_len_span(data, &mut pos).expect("len")
}

fn decode_tag_only(data: &[u8]) -> (u32, u32) {
    let mut pos = 0;
    pbrs::rt::decode_tag(data, &mut pos).expect("tag")
}

fn read_len_span_only(data: &[u8]) -> (usize, usize) {
    // hello: skip the 1-byte tag so this is the length varint + span.
    let mut pos = 1;
    pbrs::rt::read_len_span(data, &mut pos).expect("len")
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    let iters = 40_000u32;
    let samples = 15usize;

    let mut pbrs = PbrsHello::new();
    pbrs.set_name("ada");
    let hello = Serialize::serialize(&pbrs).expect("pbrs hello");
    let mut prost_hello = Vec::new();
    prost::Message::encode(
        &ProstHello {
            name: "ada".to_string(),
        },
        &mut prost_hello,
    )
    .expect("prost hello");
    assert_eq!(hello, prost_hello, "hello wire must match");
    assert_eq!(hello, [0x0a, 0x03, b'a', b'd', b'a']);

    let kib_name = "x".repeat(4096);
    let mut pbrs4 = PbrsHello::new();
    pbrs4.set_name(&kib_name);
    let hello4 = Serialize::serialize(&pbrs4).expect("pbrs 4kib");
    let mut prost4 = Vec::new();
    prost::Message::encode(
        &ProstHello {
            name: kib_name.clone(),
        },
        &mut prost4,
    )
    .expect("prost 4kib");
    assert_eq!(hello4, prost4);

    // Confirm the leftover hello path does not Wire::ensure.
    {
        let mut slot = None;
        let s = pbrs::rt::LazyStr::from_parse_span(&mut slot, &hello, 2, 5);
        assert!(
            slot.is_none(),
            "from_parse_span on ada must leave the parent Wire slot empty"
        );
        assert_eq!(s.as_view(), "ada");
        let mut long_slot = None;
        let long = pbrs::rt::LazyStr::from_parse_span(&mut long_slot, &hello4, 3, hello4.len());
        assert!(
            long_slot.is_some(),
            "4 KiB name still Wire::ensure the parent frame"
        );
        let _ = long;
        let _ = reconstruct_string_arm(&hello);
    }

    println!("# Leftover hello Parse Δ vs prost (after inline Arc skip)");
    println!();
    println!("MEASURE ONLY. Not a win. Not codec parity. Do not merge as done.");
    println!("from_parse_span is present. Hello reconstruct uses it, not Wire::ensure.");
    println!("iters={iters} samples={samples} median release thin-LTO");
    println!();
    println!("hello bytes ({}): {}", hello.len(), hex(&hello));
    println!("hello_4kib bytes: {}", hello4.len());
    println!(
        "size_of PbrsHello={} ProstHello={} LazyStr={} Wire={} ProtoString={} UnknownFields={} CachedSize={} String={}",
        size_of::<PbrsHello>(),
        size_of::<ProstHello>(),
        size_of::<pbrs::rt::LazyStr>(),
        size_of::<pbrs::rt::Wire>(),
        size_of::<pbrs::ProtoString>(),
        size_of::<pbrs::UnknownFields>(),
        size_of::<pbrs::rt::CachedSize>(),
        size_of::<String>(),
    );
    println!();

    let pbrs_hello = median_ns(samples, iters, || {
        <PbrsHello as Parse>::parse(&hello).expect("parse")
    });
    let prost_hello_ns = median_ns(samples, iters, || {
        ProstHello::decode(hello.as_slice()).expect("decode")
    });
    let pbrs_4k = median_ns(samples, iters, || {
        <PbrsHello as Parse>::parse(&hello4).expect("parse")
    });
    let prost_4k = median_ns(samples, iters, || {
        ProstHello::decode(hello4.as_slice()).expect("decode")
    });
    let pbrs_empty = median_ns(samples, iters, || {
        <PbrsHello as Parse>::parse(&[]).expect("empty")
    });
    let prost_empty = median_ns(samples, iters, || {
        ProstHello::decode([].as_slice()).expect("empty")
    });

    println!("## Parse-only (verbatim for the inventory)");
    println!("pbrs hello Parse:  {:.1} ns", pbrs_hello);
    println!("prost hello decode: {:.1} ns", prost_hello_ns);
    println!(
        "delta hello:        {:.1} ns (pbrs − prost)",
        pbrs_hello - prost_hello_ns
    );
    println!("pbrs hello_4kib Parse:  {:.1} ns", pbrs_4k);
    println!("prost hello_4kib decode: {:.1} ns", prost_4k);
    println!("delta 4kib:              {:.1} ns", pbrs_4k - prost_4k);
    println!("pbrs empty Parse:   {:.1} ns", pbrs_empty);
    println!("prost empty decode: {:.1} ns", prost_empty);
    println!();

    let default_pbrs = median_ns(samples, iters, PbrsHello::default);
    let default_prost = median_ns(samples, iters, ProstHello::default);
    let cached_dirty = median_ns(samples, iters, || {
        let c = pbrs::rt::CachedSize::default();
        c.dirty();
        c
    });
    let cached_dirty_only = median_ns(samples, iters, || {
        // reuse one CachedSize so this is the atomic store, not Default
        static C: std::sync::OnceLock<pbrs::rt::CachedSize> = std::sync::OnceLock::new();
        let c = C.get_or_init(pbrs::rt::CachedSize::default);
        c.dirty();
    });
    let utf8_ada = median_ns(samples, iters, || std::str::from_utf8(b"ada").unwrap());
    let utf8_4k = median_ns(samples, iters, || {
        std::str::from_utf8(kib_name.as_bytes()).unwrap()
    });
    let string_ada = median_ns(samples, iters, || String::from("ada"));
    let proto_ada = median_ns(samples, iters, || pbrs::ProtoString::from_bytes(b"ada"));
    let lazy_owned = median_ns(samples, iters, || pbrs::rt::LazyStr::from_bytes(b"ada"));
    let parse_span = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &hello, 2, 5)
    });
    let wire_hello = median_ns(samples, iters, || pbrs::rt::Wire::from_slice(&hello));
    let wire_4k = median_ns(samples, iters, || pbrs::rt::Wire::from_slice(&hello4));
    let walk = median_ns(samples, iters, || tag_walk_only(&hello));
    let tag_only = median_ns(samples, iters, || decode_tag_only(&hello));
    let len_only = median_ns(samples, iters, || read_len_span_only(&hello));
    let recon = median_ns(samples, iters, || reconstruct_string_arm(&hello));
    let recon4 = median_ns(samples, iters, || reconstruct_string_arm(&hello4));
    let recon_old = median_ns(samples, iters, || reconstruct_old_ensure_arm(&hello));
    let merge_bytes = median_ns(samples, iters, || {
        let mut m = PbrsHello::default();
        ClearAndParse::merge_from_bytes(&mut m, &hello).expect("merge");
        m
    });
    let glue = median_ns(samples, iters, || {
        // Default + dirty + current string arm. Still not the trait layers.
        let _m = PbrsHello::default();
        let c = pbrs::rt::CachedSize::default();
        c.dirty();
        let s = reconstruct_string_arm(&hello);
        (_m, c, s)
    });

    println!("## Component proxies (not a clean additive split)");
    println!("These are isolated public-API timings. They overlap. Do not sum to the leftover Δ.");
    println!("pbrs HelloRequest::default:     {:.1} ns", default_pbrs);
    println!("prost HelloRequest::default:    {:.1} ns", default_prost);
    println!(
        "ClearAndParse::merge_from_bytes (w/ Default): {:.1} ns",
        merge_bytes
    );
    println!("CachedSize::default+dirty:      {:.1} ns", cached_dirty);
    println!("CachedSize::dirty (reused):     {:.1} ns", cached_dirty_only);
    println!("from_utf8(\"ada\"):             {:.1} ns", utf8_ada);
    println!("from_utf8(4KiB):                {:.1} ns", utf8_4k);
    println!("String::from(\"ada\"):          {:.1} ns", string_ada);
    println!("ProtoString::from_bytes(\"ada\"): {:.1} ns", proto_ada);
    println!("LazyStr::from_bytes(\"ada\"):   {:.1} ns", lazy_owned);
    println!(
        "LazyStr::from_parse_span(ada):  {:.1} ns",
        parse_span
    );
    println!("Wire::from_slice(hello 5B):     {:.1} ns  (OFF hello path)", wire_hello);
    println!("Wire::from_slice(hello_4kib):   {:.1} ns  (ON 4 KiB path)", wire_4k);
    println!("decode_tag only:                {:.1} ns", tag_only);
    println!("read_len_span only:             {:.1} ns", len_only);
    println!("decode_tag + read_len_span:     {:.1} ns", walk);
    println!("reconstruct from_parse_span hello: {:.1} ns", recon);
    println!("reconstruct from_parse_span 4kib:  {:.1} ns", recon4);
    println!(
        "reconstruct old Wire::ensure hello: {:.1} ns  (OFF hello path)",
        recon_old
    );
    println!("Default + dirty + reconstruct:  {:.1} ns", glue);
    println!();
    println!("reconstruct ≈ generated string arm after #34 (tag, utf8, from_parse_span).");
    println!(
        "Parse − reconstruct hello ≈ {:.1} ns (Default + cached_size.dirty + merge_inner glue).",
        pbrs_hello - recon
    );
    println!(
        "reconstruct − (walk + utf8) hello ≈ {:.1} ns (from_parse_span / ProtoString).",
        recon - walk - utf8_ada
    );
    println!(
        "Parse − (Default + dirty + reconstruct) ≈ {:.1} ns (trait / required / group / loop).",
        pbrs_hello - glue
    );
    println!(
        "leftover hello Δ (Parse − prost) ≈ {:.1} ns",
        pbrs_hello - prost_hello_ns
    );
}
