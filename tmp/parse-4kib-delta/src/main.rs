//! Throwaway leftover 4 KiB hello Parse Δ timing after #34.
//! Not a workspace member.
//!
//! MEASURE ONLY. Still a loss. Does not change the kernel.
//! Same Instant median as tonic-bench (40000 × 15, thin LTO).
//! Inventory: `tmp/parse-4kib-delta.md`.

use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::mem::size_of;
use std::sync::Arc;
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

/// Explicit Wire::ensure + from_span (the long-string body of from_parse_span).
fn reconstruct_ensure_arm(data: &[u8]) -> pbrs::rt::LazyStr {
    let mut pos = 0;
    let mut wire = None;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    let b = &data[s..e];
    std::str::from_utf8(b).expect("utf8");
    pbrs::rt::LazyStr::from_span(pbrs::rt::Wire::ensure(&mut wire, data), s, e)
}

/// Same walk + UTF-8, then one String copy of the name (prost materialize).
fn reconstruct_string_copy(data: &[u8]) -> String {
    let mut pos = 0;
    let (_n, _w) = pbrs::rt::decode_tag(data, &mut pos).expect("tag");
    let (s, e) = pbrs::rt::read_len_span(data, &mut pos).expect("len");
    let b = &data[s..e];
    std::str::from_utf8(b).expect("utf8");
    String::from(std::str::from_utf8(b).unwrap())
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
    // skip the 1-byte tag so this is the length varint + span.
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
    assert_eq!(kib_name.len(), 4096);
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
    assert_eq!(hello4, prost4, "4 KiB hello wire must match");
    // tag 0x0a + 2-byte varint length 4096 (0x80 0x20) + 4096× 'x'
    assert_eq!(hello4.len(), 4099);
    assert_eq!(&hello4[..3], &[0x0a, 0x80, 0x20]);
    assert!(hello4[3..].iter().all(|&b| b == b'x'));
    assert_eq!(hello4[3..].len(), 4096);

    // Confirm the leftover 4 KiB path still Wire::ensures (len > 23).
    {
        let mut ada_slot = None;
        let ada = pbrs::rt::LazyStr::from_parse_span(&mut ada_slot, &hello, 2, 5);
        assert!(
            ada_slot.is_none(),
            "from_parse_span on ada must leave the parent Wire slot empty"
        );
        assert!(matches!(ada, pbrs::rt::LazyStr::Owned(_)));

        let mut long_slot = None;
        let long = pbrs::rt::LazyStr::from_parse_span(&mut long_slot, &hello4, 3, hello4.len());
        assert!(
            long_slot.is_some(),
            "4 KiB name still Wire::ensure the parent frame"
        );
        assert!(
            matches!(long, pbrs::rt::LazyStr::Wire(_)),
            "4 KiB name stays a Wire window, not a second String copy"
        );
        assert_eq!(long_slot.as_ref().unwrap().as_slice(), hello4.as_slice());
        assert_eq!(long.as_bytes().len(), 4096);

        let parsed = <PbrsHello as Parse>::parse(&hello4).expect("parse 4kib");
        assert_eq!(parsed.name(), kib_name.as_str());
        let _ = reconstruct_string_arm(&hello4);
        let _ = reconstruct_ensure_arm(&hello4);
    }

    println!("# Leftover 4 KiB hello Parse Δ vs prost (after #34 inline Arc skip)");
    println!();
    println!("MEASURE ONLY. Still a loss. Not a win. Do not merge as done.");
    println!("from_parse_span is present. 4 KiB (len>23) still Wire::ensure.");
    println!("iters={iters} samples={samples} median release thin-LTO");
    println!();
    println!(
        "hello bytes ({}): {}",
        hello.len(),
        hex_prefix(&hello, hello.len())
    );
    println!(
        "hello_4kib bytes: {}  prefix {}  name_len {}",
        hello4.len(),
        hex_prefix(&hello4, 3),
        hello4.len() - 3
    );
    println!(
        "size_of PbrsHello={} ProstHello={} LazyStr={} Wire={} ProtoString={} UnknownFields={} CachedSize={} String={} Arc<[u8]>={}",
        size_of::<PbrsHello>(),
        size_of::<ProstHello>(),
        size_of::<pbrs::rt::LazyStr>(),
        size_of::<pbrs::rt::Wire>(),
        size_of::<pbrs::ProtoString>(),
        size_of::<pbrs::UnknownFields>(),
        size_of::<pbrs::rt::CachedSize>(),
        size_of::<String>(),
        size_of::<Arc<[u8]>>(),
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
        static C: std::sync::OnceLock<pbrs::rt::CachedSize> = std::sync::OnceLock::new();
        let c = C.get_or_init(pbrs::rt::CachedSize::default);
        c.dirty();
    });
    let utf8_ada = median_ns(samples, iters, || std::str::from_utf8(b"ada").unwrap());
    let utf8_4k = median_ns(samples, iters, || {
        std::str::from_utf8(kib_name.as_bytes()).unwrap()
    });
    let string_ada = median_ns(samples, iters, || String::from("ada"));
    let string_4k = median_ns(samples, iters, || String::from(kib_name.as_str()));
    let vec_4k = median_ns(samples, iters, || Vec::from(kib_name.as_bytes()));
    let arc_name = median_ns(samples, iters, || Arc::<[u8]>::from(kib_name.as_bytes()));
    let arc_msg = median_ns(samples, iters, || Arc::<[u8]>::from(hello4.as_slice()));
    let memcpy_name = median_ns(samples, iters, || {
        let mut buf = vec![0u8; 4096];
        buf.copy_from_slice(kib_name.as_bytes());
        buf
    });
    let memcpy_msg = median_ns(samples, iters, || {
        let mut buf = vec![0u8; hello4.len()];
        buf.copy_from_slice(&hello4);
        buf
    });
    let memcpy_pre_name = {
        let mut buf = vec![0u8; 4096];
        median_ns(samples, iters, || {
            buf.copy_from_slice(kib_name.as_bytes());
            buf.len()
        })
    };
    let memcpy_pre_msg = {
        let mut buf = vec![0u8; hello4.len()];
        median_ns(samples, iters, || {
            buf.copy_from_slice(&hello4);
            buf.len()
        })
    };
    let parse_span_ada = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &hello, 2, 5)
    });
    let parse_span_4k = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::LazyStr::from_parse_span(&mut slot, &hello4, 3, hello4.len())
    });
    let wire_hello = median_ns(samples, iters, || pbrs::rt::Wire::from_slice(&hello));
    let wire_4k = median_ns(samples, iters, || pbrs::rt::Wire::from_slice(&hello4));
    let wire_name = median_ns(samples, iters, || {
        pbrs::rt::Wire::from_slice(kib_name.as_bytes())
    });
    let ensure_4k = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::Wire::ensure(&mut slot, &hello4).as_slice().len()
    });
    let window_4k = {
        let parent = pbrs::rt::Wire::from_slice(&hello4);
        median_ns(samples, iters, || parent.window(3, hello4.len()))
    };
    let ensure_window_4k = median_ns(samples, iters, || {
        let mut slot = None;
        pbrs::rt::Wire::ensure(&mut slot, &hello4)
            .window(3, hello4.len())
    });
    let walk = median_ns(samples, iters, || tag_walk_only(&hello));
    let walk4 = median_ns(samples, iters, || tag_walk_only(&hello4));
    let tag_only = median_ns(samples, iters, || decode_tag_only(&hello4));
    let len_only = median_ns(samples, iters, || read_len_span_only(&hello4));
    let recon = median_ns(samples, iters, || reconstruct_string_arm(&hello));
    let recon4 = median_ns(samples, iters, || reconstruct_string_arm(&hello4));
    let recon4_ensure = median_ns(samples, iters, || reconstruct_ensure_arm(&hello4));
    let recon4_string = median_ns(samples, iters, || reconstruct_string_copy(&hello4));
    let merge_bytes = median_ns(samples, iters, || {
        let mut m = PbrsHello::default();
        ClearAndParse::merge_from_bytes(&mut m, &hello4).expect("merge");
        m
    });
    let glue4 = median_ns(samples, iters, || {
        let _m = PbrsHello::default();
        let c = pbrs::rt::CachedSize::default();
        c.dirty();
        let s = reconstruct_string_arm(&hello4);
        (_m, c, s)
    });

    println!("## Component proxies (not a clean additive split)");
    println!("These are isolated public-API timings. They overlap. Do not sum to the leftover Δ.");
    println!("pbrs HelloRequest::default:     {:.1} ns", default_pbrs);
    println!("prost HelloRequest::default:    {:.1} ns", default_prost);
    println!(
        "ClearAndParse::merge_from_bytes 4kib (w/ Default): {:.1} ns",
        merge_bytes
    );
    println!("CachedSize::default+dirty:      {:.1} ns", cached_dirty);
    println!("CachedSize::dirty (reused):     {:.1} ns", cached_dirty_only);
    println!("from_utf8(\"ada\"):             {:.1} ns", utf8_ada);
    println!("from_utf8(4KiB):                {:.1} ns", utf8_4k);
    println!("String::from(\"ada\"):          {:.1} ns", string_ada);
    println!("String::from(4KiB name):        {:.1} ns  (prost materialize)", string_4k);
    println!("Vec::from(4KiB name):           {:.1} ns", vec_4k);
    println!("Arc<[u8]>::from(4KiB name):     {:.1} ns", arc_name);
    println!("Arc<[u8]>::from(4099 B msg):    {:.1} ns", arc_msg);
    println!("alloc+memcpy 4096:              {:.1} ns", memcpy_name);
    println!("alloc+memcpy 4099:              {:.1} ns", memcpy_msg);
    println!("prealloc memcpy 4096:           {:.1} ns", memcpy_pre_name);
    println!("prealloc memcpy 4099:           {:.1} ns", memcpy_pre_msg);
    println!(
        "LazyStr::from_parse_span(ada):  {:.1} ns  (OFF 4 KiB path)",
        parse_span_ada
    );
    println!(
        "LazyStr::from_parse_span(4kib): {:.1} ns  (ON; ensure+window)",
        parse_span_4k
    );
    println!("Wire::from_slice(hello 5B):     {:.1} ns  (OFF 4 KiB path)", wire_hello);
    println!("Wire::from_slice(hello_4kib):   {:.1} ns  (ON; parent Arc)", wire_4k);
    println!("Wire::from_slice(4KiB name):    {:.1} ns", wire_name);
    println!("Wire::ensure(hello_4kib):       {:.1} ns", ensure_4k);
    println!("Wire::window(prebuilt 4kib):    {:.1} ns", window_4k);
    println!("Wire::ensure+window 4kib:       {:.1} ns", ensure_window_4k);
    println!("decode_tag only (4kib):         {:.1} ns", tag_only);
    println!("read_len_span only (4kib):      {:.1} ns", len_only);
    println!("decode_tag + read_len_span hello: {:.1} ns", walk);
    println!("decode_tag + read_len_span 4kib:  {:.1} ns", walk4);
    println!("reconstruct from_parse_span hello: {:.1} ns", recon);
    println!("reconstruct from_parse_span 4kib:  {:.1} ns", recon4);
    println!("reconstruct ensure+from_span 4kib: {:.1} ns  (same body as from_parse_span)", recon4_ensure);
    println!(
        "reconstruct + String::from 4kib: {:.1} ns  (prost-style materialize)",
        recon4_string
    );
    println!("Default + dirty + reconstruct 4kib: {:.1} ns", glue4);
    println!();
    println!("reconstruct ≈ generated string arm after #34 (tag, utf8, from_parse_span).");
    println!(
        "Parse − reconstruct hello ≈ {:.1} ns (wrapper).",
        pbrs_hello - recon
    );
    println!(
        "Parse − reconstruct 4kib ≈ {:.1} ns (wrapper).",
        pbrs_4k - recon4
    );
    println!(
        "reconstruct − prost decode 4kib ≈ {:.1} ns (string arm vs prost full).",
        recon4 - prost_4k
    );
    println!(
        "Wire::from_slice(msg) − String::from(name) ≈ {:.1} ns (Arc vs one String copy).",
        wire_4k - string_4k
    );
    println!(
        "from_parse_span 4kib − String::from(name) ≈ {:.1} ns",
        parse_span_4k - string_4k
    );
    println!(
        "leftover 4kib Δ (Parse − prost) ≈ {:.1} ns",
        pbrs_4k - prost_4k
    );
}
