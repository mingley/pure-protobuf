//! Throwaway leftover `name_80` combined (encode+decode) timing vs prost.
//! Not a workspace member.
//!
//! MEASURE ONLY. Still a loss unless same-host numbers say otherwise.
//! Same Instant median as tonic-bench (40000 × 15, thin LTO).
//! Inventory: `docs/inventory/parse-name80-delta.md`.

use bytes::BytesMut;
use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

mod pbrs_cases {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs/codec_cases.rs"));
}
mod prost_cases {
    #![allow(dead_code, unused, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/prost/cases.rs"));
}

use pbrs_cases::Name as PbrsName;
use prost_cases::Name as ProstName;

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

/// Generated Name::merge_inner string arm: tag, from_parse_span.
/// No Default, cached_size, trait layers, unknown, or required.
fn reconstruct_string_arm(data: &[u8]) -> pbrs::rt::LazyStr {
    let mut pos = 0;
    let mut wire = None;
    let (n, w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    debug_assert_eq!(n, 1);
    debug_assert_eq!(w, pbrs::rt::WIRE_LEN);
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    pbrs::rt::LazyStr::from_parse_span(&mut wire, data, s, e).expect("utf8")
}

/// Parent-frame ensure + from_span. Off the name_80 path (almost-whole
/// payload copy). Kept to show that leftover is not that Arc.
fn reconstruct_parent_ensure_arm(data: &[u8]) -> pbrs::rt::LazyStr {
    let mut pos = 0;
    let mut wire = None;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    pbrs::rt::require_utf8(&data[s..e]).expect("utf8");
    pbrs::rt::LazyStr::from_span(pbrs::rt::Wire::ensure(&mut wire, data), s, e)
}

/// Prost-style: tag walk + one String copy of the name.
fn reconstruct_string_copy(data: &[u8]) -> String {
    let mut pos = 0;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    let b = &data[s..e];
    String::from(std::str::from_utf8(b).expect("utf8"))
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
    let mut pos = 1;
    pbrs::rt::read_len_span(data, &mut pos).expect("len")
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    bytes
        .iter()
        .take(n)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn classify_parse_span(label: &str, data: &[u8], rel_start: usize, rel_end: usize) {
    let mut slot = None;
    let s = pbrs::rt::LazyStr::from_parse_span(&mut slot, data, rel_start, rel_end)
        .expect("from_parse_span");
    let kind = match &s {
        pbrs::rt::LazyStr::Empty => "Empty",
        pbrs::rt::LazyStr::Wire(_) => "Wire",
        pbrs::rt::LazyStr::Owned(_) => "Owned",
    };
    let parent = if slot.is_some() {
        "parent slot Some"
    } else {
        "parent slot None"
    };
    println!(
        "{label}: LazyStr::{kind} name_len={} wire_slot={parent} name_eq={}",
        s.len(),
        s.as_bytes() == &data[rel_start..rel_end]
    );
}

fn main() {
    let iters = 40_000u32;
    let samples = 15usize;

    let name_short = "ada";
    let name_80 = "x".repeat(80);
    let name_4k = "x".repeat(4096);

    let mut p_short = PbrsName::new();
    p_short.set_name(name_short);
    let mut p_80 = PbrsName::new();
    p_80.set_name(name_80.as_str());
    let mut p_4k = PbrsName::new();
    p_4k.set_name(name_4k.as_str());

    let r_short = ProstName {
        name: name_short.into(),
    };
    let r_80 = ProstName {
        name: name_80.clone(),
    };
    let r_4k = ProstName {
        name: name_4k.clone(),
    };

    let wire_short = Serialize::serialize(&p_short).expect("pbrs short");
    let wire_80 = Serialize::serialize(&p_80).expect("pbrs 80");
    let wire_4k = Serialize::serialize(&p_4k).expect("pbrs 4kib");

    let mut prost_short = Vec::new();
    Message::encode(&r_short, &mut prost_short).expect("prost short");
    let mut prost_80 = Vec::new();
    Message::encode(&r_80, &mut prost_80).expect("prost 80");
    let mut prost_4k = Vec::new();
    Message::encode(&r_4k, &mut prost_4k).expect("prost 4kib");

    assert_eq!(wire_short, prost_short);
    assert_eq!(wire_80, prost_80);
    assert_eq!(wire_4k, prost_4k);
    assert_eq!(wire_80.len(), 82);
    assert_eq!(&wire_80[..2], &[0x0a, 80]);
    assert_eq!(&wire_80[2..], name_80.as_bytes());

    println!("# Leftover name_80 combined Δ vs prost");
    println!();
    println!("MEASURE ONLY. Not a win. Not codec parity. Do not merge as done.");
    println!("iters={iters} samples={samples} median release thin-LTO");
    println!();
    println!(
        "name_short bytes ({}): {}",
        wire_short.len(),
        hex_prefix(&wire_short, wire_short.len())
    );
    println!(
        "name_80 bytes ({}): prefix {}",
        wire_80.len(),
        hex_prefix(&wire_80, 4)
    );
    println!("name_4kib bytes: {}", wire_4k.len());
    println!(
        "size_of PbrsName={} ProstName={} LazyStr={} Wire={} ProtoString={} UnknownFields={} CachedSize={} String={}",
        size_of::<PbrsName>(),
        size_of::<ProstName>(),
        size_of::<pbrs::rt::LazyStr>(),
        size_of::<pbrs::rt::Wire>(),
        size_of::<pbrs::ProtoString>(),
        size_of::<pbrs::UnknownFields>(),
        size_of::<pbrs::rt::CachedSize>(),
        size_of::<String>(),
    );
    println!();
    println!("## Generated path (from_parse_span classification)");
    classify_parse_span("name_short ada", &wire_short, 2, 5);
    classify_parse_span("name_80", &wire_80, 2, 82);
    classify_parse_span("name_4kib", &wire_4k, 3, wire_4k.len());
    {
        // Medium string inside a larger frame still shares the parent.
        let mut big = vec![0u8; 163];
        big[10..90].fill(b'x');
        classify_parse_span("medium-in-larger (80 of 163)", &big, 10, 90);
    }
    let _ = reconstruct_string_arm(&wire_80);
    println!();

    let pbrs_short = median_ns(samples, iters, || {
        <PbrsName as Parse>::parse(&wire_short).expect("parse")
    });
    let prost_short_ns = median_ns(samples, iters, || {
        ProstName::decode(wire_short.as_slice()).expect("decode")
    });
    let pbrs_80 = median_ns(samples, iters, || {
        <PbrsName as Parse>::parse(&wire_80).expect("parse")
    });
    let prost_80_ns = median_ns(samples, iters, || {
        ProstName::decode(wire_80.as_slice()).expect("decode")
    });
    let pbrs_4k = median_ns(samples, iters, || {
        <PbrsName as Parse>::parse(&wire_4k).expect("parse")
    });
    let prost_4k = median_ns(samples, iters, || {
        ProstName::decode(wire_4k.as_slice()).expect("decode")
    });
    let pbrs_empty = median_ns(samples, iters, || {
        <PbrsName as Parse>::parse(&[]).expect("empty")
    });
    let prost_empty = median_ns(samples, iters, || {
        ProstName::decode([].as_slice()).expect("empty")
    });

    let mut dst = BytesMut::new();
    let pbrs_enc_80 = median_ns(samples, iters, || {
        dst.clear();
        Serialize::encode(&p_80, &mut dst).expect("pbrs encode");
        dst.len()
    });
    let prost_enc_80 = median_ns(samples, iters, || {
        dst.clear();
        Message::encode(&r_80, &mut dst).expect("prost encode");
        dst.len()
    });
    let pbrs_enc_short = median_ns(samples, iters, || {
        dst.clear();
        Serialize::encode(&p_short, &mut dst).expect("pbrs encode");
        dst.len()
    });
    let prost_enc_short = median_ns(samples, iters, || {
        dst.clear();
        Message::encode(&r_short, &mut dst).expect("prost encode");
        dst.len()
    });
    let pbrs_enc_4k = median_ns(samples, iters, || {
        dst.clear();
        Serialize::encode(&p_4k, &mut dst).expect("pbrs encode");
        dst.len()
    });
    let prost_enc_4k = median_ns(samples, iters, || {
        dst.clear();
        Message::encode(&r_4k, &mut dst).expect("prost encode");
        dst.len()
    });

    println!("## Parse-only + encode (verbatim for the inventory)");
    println!("pbrs name_short Parse:  {:.1} ns", pbrs_short);
    println!("prost name_short decode: {:.1} ns", prost_short_ns);
    println!(
        "delta name_short decode: {:.1} ns (pbrs − prost)",
        pbrs_short - prost_short_ns
    );
    println!("pbrs name_80 Parse:  {:.1} ns", pbrs_80);
    println!("prost name_80 decode: {:.1} ns", prost_80_ns);
    println!(
        "delta name_80 decode: {:.1} ns (pbrs − prost)",
        pbrs_80 - prost_80_ns
    );
    println!("pbrs name_4kib Parse:  {:.1} ns", pbrs_4k);
    println!("prost name_4kib decode: {:.1} ns", prost_4k);
    println!("delta name_4kib decode: {:.1} ns", pbrs_4k - prost_4k);
    println!("pbrs empty Parse:   {:.1} ns", pbrs_empty);
    println!("prost empty decode: {:.1} ns", prost_empty);
    println!(
        "pbrs name_80 encode (set_name / BytesMut):  {:.1} ns",
        pbrs_enc_80
    );
    println!(
        "prost name_80 encode (BytesMut): {:.1} ns",
        prost_enc_80
    );
    println!(
        "delta name_80 encode: {:.1} ns (pbrs − prost)",
        pbrs_enc_80 - prost_enc_80
    );
    println!(
        "pbrs name_80 combined:  {:.1} ns",
        pbrs_enc_80 + pbrs_80
    );
    println!(
        "prost name_80 combined: {:.1} ns",
        prost_enc_80 + prost_80_ns
    );
    println!(
        "delta name_80 combined: {:.1} ns (pbrs − prost)",
        (pbrs_enc_80 + pbrs_80) - (prost_enc_80 + prost_80_ns)
    );
    println!(
        "name_short encode pbrs/prost: {:.1} / {:.1}",
        pbrs_enc_short, prost_enc_short
    );
    println!(
        "name_4kib encode pbrs/prost: {:.1} / {:.1}",
        pbrs_enc_4k, prost_enc_4k
    );
    println!();

    let default_pbrs = median_ns(samples, iters, PbrsName::default);
    let default_prost = median_ns(samples, iters, ProstName::default);
    let cached_dirty = median_ns(samples, iters, || {
        let c = pbrs::rt::CachedSize::default();
        c.dirty();
        c
    });
    let cached_dirty_only = median_ns(samples, iters, || {
        static C: std::sync::OnceLock<pbrs::rt::CachedSize> = std::sync::OnceLock::new();
        let c = C.get_or_init(pbrs::rt::CachedSize::default);
        c.dirty();
    });
    let utf8_ada = median_ns(samples, iters, || std::str::from_utf8(b"ada").unwrap());
    let utf8_80 = median_ns(samples, iters, || {
        std::str::from_utf8(name_80.as_bytes()).unwrap()
    });
    let utf8_4k = median_ns(samples, iters, || {
        std::str::from_utf8(name_4k.as_bytes()).unwrap()
    });
    let req_ada = median_ns(samples, iters, || {
        pbrs::rt::require_utf8(b"ada").unwrap()
    });
    let req_80 = median_ns(samples, iters, || {
        pbrs::rt::require_utf8(name_80.as_bytes()).unwrap()
    });
    let req_4k = median_ns(samples, iters, || {
        pbrs::rt::require_utf8(name_4k.as_bytes()).unwrap()
    });
    let string_ada = median_ns(samples, iters, || String::from("ada"));
    let string_80 = median_ns(samples, iters, || String::from(name_80.as_str()));
    let proto_80 = median_ns(samples, iters, || {
        pbrs::ProtoString::from_bytes(name_80.as_bytes())
    });
    let lazy_owned_80 = median_ns(samples, iters, || {
        pbrs::rt::LazyStr::from_bytes(name_80.as_bytes())
    });
    let parse_span_80 = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &wire_80, 2, 82)
    });
    let parse_span_short = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &wire_short, 2, 5)
    });
    let parse_span_4k = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &wire_4k, 3, wire_4k.len())
    });
    let wire_payload_80 = median_ns(samples, iters, || {
        pbrs::rt::Wire::from_slice(name_80.as_bytes())
    });
    let wire_msg_80 = median_ns(samples, iters, || pbrs::rt::Wire::from_slice(&wire_80));
    let wire_utf8_80 = median_ns(samples, iters, || {
        pbrs::rt::Wire::from_utf8_payload(name_80.as_bytes()).expect("utf8")
    });
    let arc_80 = median_ns(samples, iters, || Arc::<[u8]>::from(name_80.as_bytes()));
    let vec_80 = median_ns(samples, iters, || name_80.as_bytes().to_vec());
    let walk_80 = median_ns(samples, iters, || tag_walk_only(&wire_80));
    let tag_only = median_ns(samples, iters, || decode_tag_only(&wire_80));
    let len_only = median_ns(samples, iters, || read_len_span_only(&wire_80));
    let recon_80 = median_ns(samples, iters, || reconstruct_string_arm(&wire_80));
    let recon_short = median_ns(samples, iters, || reconstruct_string_arm(&wire_short));
    let recon_4k = median_ns(samples, iters, || reconstruct_string_arm(&wire_4k));
    let recon_parent = median_ns(samples, iters, || reconstruct_parent_ensure_arm(&wire_80));
    let recon_copy = median_ns(samples, iters, || reconstruct_string_copy(&wire_80));
    let merge_bytes = median_ns(samples, iters, || {
        let mut m = PbrsName::default();
        ClearAndParse::merge_from_bytes(&mut m, &wire_80).expect("merge");
        m
    });
    let glue = median_ns(samples, iters, || {
        let _m = PbrsName::default();
        let c = pbrs::rt::CachedSize::default();
        c.dirty();
        let s = reconstruct_string_arm(&wire_80);
        (_m, c, s)
    });
    let write_only = median_ns(samples, iters, || {
        dst.clear();
        pbrs::rt::encode_len_field(&mut dst, 1, name_80.as_bytes());
        dst.len()
    });
    let prost_write = median_ns(samples, iters, || {
        dst.clear();
        Message::encode(&r_80, &mut dst).expect("prost encode");
        dst.len()
    });

    println!("## Component proxies (not a clean additive split)");
    println!("These are isolated public-API timings. They overlap. Do not sum to the leftover Δ.");
    println!("pbrs Name::default:              {:.1} ns", default_pbrs);
    println!("prost Name::default:             {:.1} ns", default_prost);
    println!(
        "ClearAndParse::merge_from_bytes name_80 (w/ Default): {:.1} ns",
        merge_bytes
    );
    println!("CachedSize::default+dirty:       {:.1} ns", cached_dirty);
    println!("CachedSize::dirty (reused):      {:.1} ns", cached_dirty_only);
    println!("from_utf8(\"ada\"):              {:.1} ns", utf8_ada);
    println!("from_utf8(80):                   {:.1} ns", utf8_80);
    println!("from_utf8(4KiB):                 {:.1} ns", utf8_4k);
    println!("require_utf8(\"ada\"):           {:.1} ns", req_ada);
    println!("require_utf8(80):                {:.1} ns", req_80);
    println!("require_utf8(4KiB):              {:.1} ns", req_4k);
    println!("String::from(\"ada\"):           {:.1} ns", string_ada);
    println!("String::from(80):                {:.1} ns", string_80);
    println!("ProtoString::from_bytes(80):     {:.1} ns", proto_80);
    println!("LazyStr::from_bytes(80):         {:.1} ns", lazy_owned_80);
    println!(
        "LazyStr::from_parse_span(ada):   {:.1} ns  (OFF name_80; inline)",
        parse_span_short
    );
    println!(
        "LazyStr::from_parse_span(80):    {:.1} ns  (ON; payload Arc + utf8)",
        parse_span_80
    );
    println!(
        "LazyStr::from_parse_span(4kib):  {:.1} ns  (payload Arc + utf8)",
        parse_span_4k
    );
    println!(
        "Wire::from_slice(80 payload):    {:.1} ns  (ON name_80 path)",
        wire_payload_80
    );
    println!(
        "Wire::from_slice(82 msg):        {:.1} ns  (OFF; parent ensure)",
        wire_msg_80
    );
    println!("Wire::from_utf8_payload(80):     {:.1} ns", wire_utf8_80);
    println!("Arc<[u8]>::from(80):             {:.1} ns", arc_80);
    println!("Vec::from(80):                   {:.1} ns", vec_80);
    println!("decode_tag only:                 {:.1} ns", tag_only);
    println!("read_len_span only:              {:.1} ns", len_only);
    println!("decode_tag + read_len_span:      {:.1} ns", walk_80);
    println!(
        "reconstruct from_parse_span name_short: {:.1} ns",
        recon_short
    );
    println!("reconstruct from_parse_span name_80:   {:.1} ns", recon_80);
    println!("reconstruct from_parse_span 4kib:      {:.1} ns", recon_4k);
    println!(
        "reconstruct parent ensure name_80: {:.1} ns  (OFF name_80 path)",
        recon_parent
    );
    println!(
        "reconstruct String copy name_80:   {:.1} ns  (prost-style materialize)",
        recon_copy
    );
    println!("Default + dirty + reconstruct:   {:.1} ns", glue);
    println!(
        "encode_len_field(80) into BytesMut: {:.1} ns",
        write_only
    );
    println!("prost Message::encode (repeat):  {:.1} ns", prost_write);
    println!();
    println!("reconstruct ≈ generated string arm (tag, from_parse_span).");
    println!(
        "Parse − reconstruct name_80 ≈ {:.1} ns (Default + cached_size.dirty + merge_inner glue).",
        pbrs_80 - recon_80
    );
    println!(
        "reconstruct − prost decode name_80 ≈ {:.1} ns (string arm vs prost full).",
        recon_80 - prost_80_ns
    );
    println!(
        "from_parse_span(80) − String::from(80) ≈ {:.1} ns (overlaps Arc vs heap).",
        parse_span_80 - string_80
    );
    println!(
        "leftover name_80 decode Δ (Parse − prost) ≈ {:.1} ns",
        pbrs_80 - prost_80_ns
    );
    println!(
        "leftover name_80 encode Δ ≈ {:.1} ns",
        pbrs_enc_80 - prost_enc_80
    );
    println!(
        "leftover name_80 combined Δ ≈ {:.1} ns",
        (pbrs_enc_80 + pbrs_80) - (prost_enc_80 + prost_80_ns)
    );
}
