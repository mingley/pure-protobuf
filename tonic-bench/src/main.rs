//! Codec survey: `Serialize::encode` into `BytesMut` vs prost `Message::encode`
//! vs v4 `Serialize::serialize` (Arena+FFI, no EncodeBuf). Same-process, no
//! transport. Not kernel `./bench`. Timing is not in CI; correctness tests are.
//!
//! `hello` / `hello_4kib` stay the published 1-string rows. Common unary
//! shapes come from `proto/codec_cases.proto` (specialized gencode, not
//! TestAllTypes); separate Person layout diagnostics use `proto/person.proto`.

use bytes::BytesMut;
use pbrs::testdata::{Address as PbrsAddress, Person as PbrsPerson};
use pbrs::{AsView, Parse, Serialize};
use protobuf::{Parse as V4Parse, Serialize as V4Serialize};
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

mod helloworld {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/prost/helloworld.rs"));
}
mod pbrs_cases {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs/codec_cases.rs"));
}
mod pbrs_person {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs_person/person.rs"));
}
mod prost_cases {
    #![allow(dead_code, unused, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/prost/cases.rs"));
}
mod v4_cases {
    #![allow(clippy::all, dead_code, unused, nonstandard_style)]
    include!(concat!(env!("OUT_DIR"), "/v4/generated.rs"));
}
mod v4_person {
    #![allow(clippy::all, dead_code, unused, nonstandard_style)]
    mod internal_do_not_use_person {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rust_out_person/src/person.u.pb.rs"
        ));
    }
    pub(crate) use internal_do_not_use_person::*;
}

use helloworld::HelloRequest as ProstHello;

#[derive(Clone, PartialEq, prost::Message)]
struct ProstAddress {
    #[prost(string, tag = "1")]
    city: String,
}

#[derive(Clone, PartialEq, prost::Message)]
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
    #[prost(map = "string, int32", tag = "16")]
    extras: std::collections::HashMap<String, i32>,
}

/// Median plus the raw per-sample values in sample order, so published runs
/// can be re-analyzed with uncertainty instead of trusting one median.
/// Samples are fixed-order and sequential across codecs, not interleaved or
/// randomized pairs.
fn median_ns_raw<F, R>(samples: usize, iters: u32, mut f: F) -> (f64, Vec<f64>)
where
    F: FnMut() -> R,
{
    let raw: Vec<f64> = (0..samples).map(|_| bench_ns(iters, &mut f)).collect();
    let mut sorted = raw.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (sorted[samples / 2], raw)
}

/// One measured row's raw per-sample timings in sample order. Survey rows
/// carry fifteen codec columns; mutation rows carry three (`pbrs`, `prost`,
/// `v4` mutation+encode) with `first_iters` 0 and the field transition in
/// `detail`.
struct RawRow {
    name: &'static str,
    detail: &'static str,
    holdout: bool,
    payload: usize,
    iters: u32,
    samples: usize,
    first_iters: u32,
    cols: Vec<(&'static str, Vec<f64>)>,
}

#[cfg(test)]
impl RawRow {
    fn col(&self, label: &str) -> &[f64] {
        self.cols
            .iter()
            .find(|(name, _)| *name == label)
            .map(|(_, values)| values.as_slice())
            .unwrap_or_else(|| panic!("raw column {label} missing for {}", self.name))
    }

    fn col_median(&self, label: &str) -> f64 {
        let mut sorted = self.col(label).to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[sorted.len() / 2]
    }
}

thread_local! {
    static RAW_ROWS: RefCell<Vec<RawRow>> = RefCell::new(Vec::new());
}

fn push_raw_row(row: RawRow) {
    RAW_ROWS.with(|rows| rows.borrow_mut().push(row));
}

fn take_raw_rows() -> Vec<RawRow> {
    RAW_ROWS.with(|rows| std::mem::take(&mut *rows.borrow_mut()))
}

/// Publishable raw paired results (schema `tonic-raw/1`): every timed
/// column's per-sample values, so medians can be recomputed and uncertainty
/// bounded without rerunning. Fixed-order sequential samples, not
/// interleaved or randomized pairs; construction-cache effects are included
/// via the separately reported construct column, not hidden.
fn print_raw_block() {
    let rows = take_raw_rows();
    print!("{{\"schema\": \"tonic-raw/1\", ");
    print!(
        "\"samples\": \"fixed-order sequential in sample order, not interleaved or randomized\", "
    );
    println!("\"rows\": [");
    for (i, row) in rows.iter().enumerate() {
        let comma = if i + 1 == rows.len() { "" } else { "," };
        print!(
            "  {{\"name\": \"{}\", \"detail\": \"{}\", \"holdout\": {}, \"payload\": {}, \"iters\": {}, \"samples\": {}, \"first_iters\": {}",
            row.name, row.detail, row.holdout, row.payload, row.iters, row.samples, row.first_iters
        );
        for (label, values) in &row.cols {
            print!(", \"{label}\": [");
            for (j, value) in values.iter().enumerate() {
                if j > 0 {
                    print!(", ");
                }
                print!("{value:.3}");
            }
            print!("]");
        }
        println!("}}{comma}");
    }
    println!("]}}");
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

/// Byte-exhaustive string/bytes read for parse-and-touch checksums. Every
/// payload byte is loaded and folded into the checksum, so lazy or
/// wire-backed fields cannot hide materialization cost behind a length call.
fn bytes_sum(bytes: &[u8]) -> usize {
    bytes.iter().map(|byte| usize::from(*byte)).sum()
}

/// Process-wide Rust-heap counters (BM-03 retained-memory reporting).
/// Only `System` (Rust) allocations are counted: the v4 upb Arena lives on
/// the C heap and is NOT included in `v4` cells. That limit is labeled in
/// the retained section and in docs/benchmarks.md.
struct CountingAlloc;

static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static FREE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwards to System; only the accounting is added.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come from a matching `alloc` call.
        unsafe { System.dealloc(ptr, layout) };
        FREE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAlloc = CountingAlloc;

/// Rust heap retained while one parsed message is alive.
#[derive(Clone, Copy, Default)]
struct Retained {
    bytes: u64,
    allocs: u64,
}

fn alloc_snapshot() -> (usize, usize, usize) {
    (
        ALLOC_BYTES.load(Ordering::SeqCst),
        FREE_BYTES.load(Ordering::SeqCst),
        ALLOC_CALLS.load(Ordering::SeqCst),
    )
}

/// Parse one message from an already-owned wire buffer and report the Rust
/// heap retained while the message is alive. `parse` must not allocate
/// beyond the message itself; one-time per-codec initialization is warmed up
/// by the equivalence pre-checks, which always run before this.
fn measure_retained<M>(parse: impl FnOnce() -> M) -> (M, Retained) {
    let (a0, f0, c0) = alloc_snapshot();
    let msg = parse();
    std::hint::black_box(&msg);
    let (a1, f1, c1) = alloc_snapshot();
    let bytes = a1.saturating_sub(a0).saturating_sub(f1.saturating_sub(f0));
    let allocs = c1.saturating_sub(c0);
    (
        msg,
        Retained {
            bytes: bytes as u64,
            allocs: allocs as u64,
        },
    )
}

#[cfg(test)]
fn median_first_encode_ns<M, F, E, O>(samples: usize, iters: u32, prepare: F, encode: E) -> f64
where
    F: FnMut() -> M,
    E: FnMut(&M) -> O,
{
    median_first_encode_ns_raw(samples, iters, prepare, encode).0
}

fn median_first_encode_ns_raw<M, F, E, O>(
    samples: usize,
    iters: u32,
    mut prepare: F,
    mut encode: E,
) -> (f64, Vec<f64>)
where
    F: FnMut() -> M,
    E: FnMut(&M) -> O,
{
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        for _ in 0..iters / 10 {
            let message = prepare();
            std::hint::black_box(encode(&message));
        }
        let messages: Vec<M> = (0..iters).map(|_| prepare()).collect();
        let start = Instant::now();
        for message in &messages {
            std::hint::black_box(encode(message));
        }
        times.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(iters));
    }
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (sorted[samples / 2], times)
}

fn first_encode_budget<P, R, V>(iters: u32, payload: usize) -> u32 {
    let estimated_bytes = payload
        .saturating_add(
            std::mem::size_of::<P>()
                .max(std::mem::size_of::<R>())
                .max(std::mem::size_of::<V>()),
        )
        .max(1);
    let by_memory = u32::try_from(32 * 1024 * 1024 / estimated_bytes)
        .expect("32 MiB divided by at least one byte fits u32");
    assert!(iters > 0, "first encode needs a positive iteration count");
    assert!(
        by_memory > 0,
        "first encode exceeds the 32 MiB preparation budget"
    );
    iters.min(10_000).min(by_memory)
}

#[cfg(test)]
fn median_mutated_encode_ns<M, F, U, E, O>(
    samples: usize,
    iters: u32,
    prepare: F,
    mutate: U,
    encode: E,
) -> f64
where
    F: FnMut() -> M,
    U: FnMut(&mut M, bool),
    E: FnMut(&M) -> O,
{
    median_mutated_encode_ns_raw(samples, iters, prepare, mutate, encode).0
}

fn median_mutated_encode_ns_raw<M, F, U, E, O>(
    samples: usize,
    iters: u32,
    mut prepare: F,
    mut mutate: U,
    mut encode: E,
) -> (f64, Vec<f64>)
where
    F: FnMut() -> M,
    U: FnMut(&mut M, bool),
    E: FnMut(&M) -> O,
{
    assert!(
        samples > 0 && iters > 0,
        "mutation samples and iters must be positive"
    );
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut message = prepare();
        let mut alternate = false;
        let mut step = || {
            alternate = !alternate;
            mutate(&mut message, alternate);
            std::hint::black_box(encode(&message));
        };
        for _ in 0..iters / 10 {
            step();
        }
        let start = Instant::now();
        for _ in 0..iters {
            step();
        }
        times.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(iters));
    }
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite mutation timings"));
    (sorted[samples / 2], times)
}

fn assert_person_mutation_output(
    codec: &str,
    actual: &[u8],
    expected_wire: &[u8],
    expected_id: i32,
) {
    assert_eq!(actual, expected_wire, "person mutation: {codec} wire");
    let parsed = PbrsPerson::parse(actual).expect("person mutation must reparse");
    let expected = PbrsPerson::parse(expected_wire).expect("checked person reference must parse");
    assert_eq!(parsed.id(), expected_id, "person mutation: {codec} id");
    assert_eq!(parsed, expected, "person mutation: {codec} fields");
    let generated = pbrs_person::Person::parse(actual).expect("generated pbrs reparses person");
    let generated_expected =
        pbrs_person::Person::parse(expected_wire).expect("generated pbrs reference must parse");
    assert_eq!(
        generated.id(),
        expected_id,
        "person mutation: {codec} generated id"
    );
    assert_eq!(
        generated, generated_expected,
        "person mutation: {codec} generated fields"
    );
    let prost: ProstPerson = prost::Message::decode(actual).expect("prost reparses mutated person");
    assert_eq!(prost.id, expected_id, "person mutation: {codec} prost id");
    assert_eq!(
        prost,
        prost::Message::decode(expected_wire).expect("prost reference must parse"),
        "person mutation: {codec} prost fields"
    );
    let v4 = v4_person::Person::parse(actual).expect("v4 reparses mutated person");
    assert_eq!(v4.id(), expected_id, "person mutation: {codec} v4 id");
}

fn assert_same_output<P: Parse + PartialEq>(
    name: &str,
    codec: &str,
    actual: &[u8],
    expected_wire: &[u8],
    expected: &P,
    byte_stable: bool,
) {
    if byte_stable {
        assert_eq!(actual, expected_wire, "{name}: {codec} wire");
    } else {
        let parsed = P::parse(actual).expect("pbrs parses comparator wire");
        assert!(&parsed == expected, "{name}: {codec} decoded fields");
    }
}

fn timer_budget(payload: usize) -> (u32, usize) {
    if payload >= 32_000 {
        (4_000, 9)
    } else {
        (40_000, 15)
    }
}

#[derive(Clone, Copy)]
struct Row {
    name: &'static str,
    /// True for holdout validation rows (contract §3.5): measured shapes the
    /// tuned survey was not optimized against. Never gated.
    holdout: bool,
    payload: usize,
    iters: u32,
    samples: usize,
    pbrs_mem: Retained,
    prost_mem: Retained,
    v4_mem: Retained,
    pbrs_enc: f64,       // cached encode (pre-warmed length/canonical cache)
    pbrs_fresh_enc: f64, // direct first encode after parse, before canonical cache
    first_iters: u32,
    pbrs_dec: f64,       // parse only (message dropped)
    pbrs_touch: f64,     // parse-and-touch (reading parsed fields)
    pbrs_construct: f64, // fresh construction via new/setters (no parse)
    prost_enc: f64,
    prost_first_enc: f64,
    prost_dec: f64,
    prost_touch: f64,
    prost_construct: f64,
    v4_enc: f64,
    v4_first_enc: f64,
    v4_dec: f64,
    v4_touch: f64,
    v4_construct: f64,
}

fn run<P, R, V, TP, TR, TV, BP, BR, BV>(
    name: &'static str,
    build_pbrs: BP,
    build_prost: BR,
    build_v4: BV,
    check_wire: bool,
    touch_pbrs: TP,
    touch_prost: TR,
    touch_v4: TV,
) -> Row
where
    P: Parse + Serialize + PartialEq,
    R: prost::Message + Default,
    V: V4Parse + V4Serialize,
    TP: Fn(&P) -> usize,
    TR: Fn(&R) -> usize,
    TV: Fn(&V) -> usize,
    BP: Fn() -> P,
    BR: Fn() -> R,
    BV: Fn() -> V,
{
    run_with_budget(
        name,
        build_pbrs,
        build_prost,
        build_v4,
        check_wire,
        touch_pbrs,
        touch_prost,
        touch_v4,
        None,
        false,
    )
}

fn run_with_budget<P, R, V, TP, TR, TV, BP, BR, BV>(
    name: &'static str,
    build_pbrs: BP,
    build_prost: BR,
    build_v4: BV,
    check_wire: bool,
    touch_pbrs: TP,
    touch_prost: TR,
    touch_v4: TV,
    budget: Option<(u32, usize)>,
    holdout: bool,
) -> Row
where
    P: Parse + Serialize + PartialEq,
    R: prost::Message + Default,
    V: V4Parse + V4Serialize,
    TP: Fn(&P) -> usize,
    TR: Fn(&R) -> usize,
    TV: Fn(&V) -> usize,
    BP: Fn() -> P,
    BR: Fn() -> R,
    BV: Fn() -> V,
{
    // Builders are the single source of the reference messages: the timed
    // construction column and every equivalence check below observe the same
    // construction path, so no codec can be handed a cheaper specimen.
    let pbrs = build_pbrs();
    let prost = build_prost();
    let v4 = build_v4();
    let pbrs_wire = Serialize::serialize(&pbrs).expect("pbrs wire");
    let mut prost_wire = Vec::new();
    prost::Message::encode(&prost, &mut prost_wire).expect("prost wire");
    let v4_wire = V4Serialize::serialize(&v4).expect("v4 wire");
    let parsed_pbrs = P::parse(&pbrs_wire).expect("pbrs parses pbrs wire");
    let parsed_prost = R::decode(pbrs_wire.as_slice()).expect("prost parses pbrs wire");
    let parsed_v4 = V::parse(&pbrs_wire).expect("v4 parses pbrs wire");
    assert_same_output(
        name,
        "prost",
        &prost_wire,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    assert_same_output(name, "v4", &v4_wire, &pbrs_wire, &parsed_pbrs, check_wire);
    let mut dst = BytesMut::new();
    let first_pbrs = P::parse(&pbrs_wire).expect("pbrs first precheck parse");
    Serialize::encode(&first_pbrs, &mut dst).expect("pbrs first wire");
    assert_same_output(
        name,
        "pbrs first",
        &dst,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    dst.clear();
    prost::Message::encode(&parsed_prost, &mut dst).expect("prost first wire");
    assert_same_output(
        name,
        "prost first",
        &dst,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    let v4_first_wire = V4Serialize::serialize(&parsed_v4).expect("v4 first wire");
    assert_same_output(
        name,
        "v4 first",
        &v4_first_wire,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    let expected_touch = touch_pbrs(&parsed_pbrs);
    assert_eq!(
        expected_touch,
        touch_prost(&parsed_prost),
        "{name}: pbrs vs prost touch"
    );
    assert_eq!(
        expected_touch,
        touch_v4(&parsed_v4),
        "{name}: pbrs vs v4 touch"
    );
    // Retained-memory snapshot (not timed): one parse per codec from the same
    // wire, after the pre-checks above warmed any one-time initialization.
    let (_, pbrs_mem) = measure_retained(|| P::parse(&pbrs_wire).expect("pbrs retained parse"));
    let (_, prost_mem) =
        measure_retained(|| R::decode(pbrs_wire.as_slice()).expect("prost retained parse"));
    let (_, v4_mem) = measure_retained(|| V::parse(&pbrs_wire).expect("v4 retained parse"));

    let payload = pbrs_wire.len();
    let (iters, samples) = budget.unwrap_or_else(|| timer_budget(payload));
    assert!(
        iters > 0 && samples > 0,
        "benchmark needs a positive budget"
    );
    let first_iters = first_encode_budget::<P, R, V>(iters, payload);
    let (pbrs_enc, raw_pbrs_enc) = median_ns_raw(samples, iters, || {
        dst.clear();
        Serialize::encode(&pbrs, &mut dst).expect("pbrs encode");
        std::hint::black_box(&dst[..]);
    });
    let (pbrs_dec, raw_pbrs_dec) =
        median_ns_raw(samples, iters, || P::parse(&pbrs_wire).expect("pbrs parse"));
    let (pbrs_fresh_enc, raw_pbrs_fresh) = median_first_encode_ns_raw(
        samples,
        first_iters,
        || P::parse(&pbrs_wire).expect("pbrs first parse"),
        |message| {
            dst.clear();
            Serialize::encode(message, &mut dst).expect("pbrs first encode");
            std::hint::black_box(&dst[..]);
        },
    );
    let (pbrs_touch, raw_pbrs_touch) = median_ns_raw(samples, iters, || {
        let msg = P::parse(&pbrs_wire).expect("pbrs parse");
        touch_pbrs(&msg)
    });
    let (pbrs_construct, raw_pbrs_construct) = median_ns_raw(samples, iters, || build_pbrs());
    let (prost_enc, raw_prost_enc) = median_ns_raw(samples, iters, || {
        dst.clear();
        prost::Message::encode(&prost, &mut dst).expect("prost encode");
        std::hint::black_box(&dst[..]);
    });
    let (prost_dec, raw_prost_dec) = median_ns_raw(samples, iters, || {
        R::decode(pbrs_wire.as_slice()).expect("prost decode")
    });
    let (prost_first_enc, raw_prost_first) = median_first_encode_ns_raw(
        samples,
        first_iters,
        || R::decode(pbrs_wire.as_slice()).expect("prost first parse"),
        |message| {
            dst.clear();
            prost::Message::encode(message, &mut dst).expect("prost first encode");
            std::hint::black_box(&dst[..]);
        },
    );
    let (prost_touch, raw_prost_touch) = median_ns_raw(samples, iters, || {
        let msg = R::decode(pbrs_wire.as_slice()).expect("prost decode");
        touch_prost(&msg)
    });
    let (prost_construct, raw_prost_construct) = median_ns_raw(samples, iters, || build_prost());
    let (v4_enc, raw_v4_enc) = median_ns_raw(samples, iters, || {
        V4Serialize::serialize(&v4).expect("v4 encode")
    });
    let (v4_dec, raw_v4_dec) =
        median_ns_raw(samples, iters, || V::parse(&pbrs_wire).expect("v4 parse"));
    let (v4_first_enc, raw_v4_first) = median_first_encode_ns_raw(
        samples,
        first_iters,
        || V::parse(&pbrs_wire).expect("v4 first parse"),
        |message| V4Serialize::serialize(message).expect("v4 first encode"),
    );
    let (v4_touch, raw_v4_touch) = median_ns_raw(samples, iters, || {
        let msg = V::parse(&pbrs_wire).expect("v4 parse");
        touch_v4(&msg)
    });
    let (v4_construct, raw_v4_construct) = median_ns_raw(samples, iters, || build_v4());
    push_raw_row(RawRow {
        name,
        detail: "",
        holdout,
        payload,
        iters,
        samples,
        first_iters,
        cols: Vec::from([
            ("pbrs_enc", raw_pbrs_enc),
            ("pbrs_first_enc", raw_pbrs_fresh),
            ("pbrs_dec", raw_pbrs_dec),
            ("pbrs_touch", raw_pbrs_touch),
            ("pbrs_construct", raw_pbrs_construct),
            ("prost_enc", raw_prost_enc),
            ("prost_first_enc", raw_prost_first),
            ("prost_dec", raw_prost_dec),
            ("prost_touch", raw_prost_touch),
            ("prost_construct", raw_prost_construct),
            ("v4_enc", raw_v4_enc),
            ("v4_first_enc", raw_v4_first),
            ("v4_dec", raw_v4_dec),
            ("v4_touch", raw_v4_touch),
            ("v4_construct", raw_v4_construct),
        ]),
    });
    // Construction parity pre-check (not timed): a rebuilt message must
    // serialize to the same wire the checks above validated, so the timed
    // construction column cannot run a cheaper builder than the checks saw.
    assert_eq!(
        Serialize::serialize(&build_pbrs()).expect("pbrs rebuild wire"),
        pbrs_wire,
        "{name}: pbrs rebuilt wire"
    );
    if check_wire {
        let mut rebuilt_prost = Vec::new();
        prost::Message::encode(&build_prost(), &mut rebuilt_prost).expect("prost rebuild wire");
        assert_eq!(rebuilt_prost, pbrs_wire, "{name}: prost rebuilt wire");
        assert_eq!(
            V4Serialize::serialize(&build_v4()).expect("v4 rebuild wire"),
            pbrs_wire,
            "{name}: v4 rebuilt wire"
        );
    }
    Row {
        name,
        holdout,
        payload,
        iters,
        samples,
        pbrs_mem,
        prost_mem,
        v4_mem,
        pbrs_enc,
        pbrs_fresh_enc,
        first_iters,
        pbrs_dec,
        pbrs_touch,
        pbrs_construct,
        prost_enc,
        prost_first_enc,
        prost_dec,
        prost_touch,
        prost_construct,
        v4_enc,
        v4_first_enc,
        v4_dec,
        v4_touch,
        v4_construct,
    }
}

/// Holdout validation row (contract §3.5): identical measurement to
/// [`run_with_budget`], flagged so reports can separate tuned survey rows
/// from shapes the implementation was not optimized against. Never gated.
fn run_holdout<P, R, V, TP, TR, TV, BP, BR, BV>(
    name: &'static str,
    build_pbrs: BP,
    build_prost: BR,
    build_v4: BV,
    check_wire: bool,
    touch_pbrs: TP,
    touch_prost: TR,
    touch_v4: TV,
    budget: Option<(u32, usize)>,
) -> Row
where
    P: Parse + Serialize + PartialEq,
    R: prost::Message + Default,
    V: V4Parse + V4Serialize,
    TP: Fn(&P) -> usize,
    TR: Fn(&R) -> usize,
    TV: Fn(&V) -> usize,
    BP: Fn() -> P,
    BR: Fn() -> R,
    BV: Fn() -> V,
{
    run_with_budget(
        name,
        build_pbrs,
        build_prost,
        build_v4,
        check_wire,
        touch_pbrs,
        touch_prost,
        touch_v4,
        budget,
        true,
    )
}

/// Holdout shapes built from the existing `codec_cases.proto` types with
/// populations the tuned survey does not cover: deeper recursion than
/// `nest_d4`, the unmeasured `err` union variant, a sparse `Rpc` populated
/// at a different field than `rpc_sparse`, and a single-entry map instead of
/// the wide `map_8`. All four are byte-stable (single-entry maps serialize
/// deterministically), so wire equality is checked.
fn run_holdout_rows(budget: Option<(u32, usize)>) -> [Row; 4] {
    [
        run_holdout(
            "nest_d8",
            || pbrs_node(8),
            || prost_node(8),
            || v4_node(8),
            true,
            touch_node_pbrs,
            touch_node_prost,
            touch_node_v4,
            budget,
        ),
        run_holdout(
            "oneof_err",
            || {
                let mut m = pbrs_cases::PbResult::new();
                m.set_err("broken");
                m
            },
            || prost_cases::Result {
                kind: Some(prost_cases::result::Kind::Err("broken".into())),
            },
            || {
                let mut m = v4_cases::Result::new();
                m.set_err("broken");
                m
            },
            true,
            |m| m.err_opt().map(|s| bytes_sum(s.as_bytes())).unwrap_or(0),
            |m| match &m.kind {
                Some(prost_cases::result::Kind::Err(s)) => bytes_sum(s.as_bytes()),
                _ => 0,
            },
            |m| {
                if m.has_err() {
                    bytes_sum(m.err().as_bytes())
                } else {
                    0
                }
            },
            budget,
        ),
        run_holdout(
            "rpc_sparse_path",
            || {
                let mut m = pbrs_cases::Rpc::new();
                m.set_path("/v1/items");
                m
            },
            || prost_cases::Rpc {
                path: "/v1/items".into(),
                ..Default::default()
            },
            || {
                let mut m = v4_cases::Rpc::new();
                m.set_path("/v1/items");
                m
            },
            true,
            |m| bytes_sum(m.path().as_bytes()),
            |m| bytes_sum(m.path.as_bytes()),
            |m| bytes_sum(m.path().as_bytes()),
            budget,
        ),
        run_holdout(
            "headers_1",
            || {
                let mut m = pbrs_cases::Headers::new();
                m.h_mut().insert("content-type", "application/json");
                m
            },
            || prost_cases::Headers {
                h: [("content-type".to_owned(), "application/json".to_owned())]
                    .into_iter()
                    .collect(),
            },
            || {
                let mut m = v4_cases::Headers::new();
                m.h_mut().insert("content-type", "application/json");
                m
            },
            true,
            |m| {
                m.h()
                    .iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
            |m| {
                m.h.iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
            |m| {
                m.h()
                    .iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
            budget,
        ),
    ]
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

fn v4_name(name: &str) -> v4_cases::Name {
    let mut m = v4_cases::Name::new();
    m.set_name(name);
    m
}

fn meta_pbrs() -> pbrs_cases::Meta {
    let mut m = pbrs_cases::Meta::new();
    m.set_id(7);
    m.set_ts(1_700_000_000);
    m.set_trace("abc123");
    m
}

fn meta_prost() -> prost_cases::Meta {
    prost_cases::Meta {
        id: 7,
        ts: 1_700_000_000,
        trace: "abc123".into(),
    }
}

fn meta_v4() -> v4_cases::Meta {
    let mut m = v4_cases::Meta::new();
    m.set_id(7);
    m.set_ts(1_700_000_000);
    m.set_trace("abc123");
    m
}

fn pbrs_node(depth: i32) -> pbrs_cases::Node {
    let mut n = pbrs_cases::Node::new();
    n.set_n(depth);
    if depth > 1 {
        n.set_child(pbrs_node(depth - 1));
    }
    n
}

fn prost_node(depth: i32) -> prost_cases::Node {
    prost_cases::Node {
        n: depth,
        child: if depth > 1 {
            Some(Box::new(prost_node(depth - 1)))
        } else {
            None
        },
    }
}

fn v4_node(depth: i32) -> v4_cases::Node {
    let mut n = v4_cases::Node::new();
    n.set_n(depth);
    if depth > 1 {
        n.set_child(v4_node(depth - 1));
    }
    n
}

fn touch_node_pbrs(n: &pbrs_cases::Node) -> usize {
    let mut count = n.n() as usize;
    let mut cur = n;
    while cur.has_child() {
        cur = cur.child();
        count += cur.n() as usize;
    }
    count
}

fn touch_node_prost(n: &prost_cases::Node) -> usize {
    let mut count = n.n as usize;
    let mut cur = n;
    while let Some(c) = &cur.child {
        cur = c;
        count += cur.n as usize;
    }
    count
}

fn touch_node_v4(n: &v4_cases::Node) -> usize {
    let mut count = n.n() as usize;
    if n.has_child() {
        let mut cur = n.child();
        count += cur.n() as usize;
        while cur.has_child() {
            cur = cur.child();
            count += cur.n() as usize;
        }
    }
    count
}

fn print_table(title: &str, rows: &[Row]) {
    println!("{title}");
    println!();
    println!(
        "| case | payload | pbrs enc (fresh/cached) | pbrs dec (parse/touch) | prost enc/dec/touch | v4 enc/dec/touch | vs prost | vs v4 |"
    );
    println!("|---|---:|---:|---:|---:|---:|---|---|");
    for r in rows {
        let vs_prost = if r.pbrs_enc + r.pbrs_dec < r.prost_enc + r.prost_dec {
            "win"
        } else {
            "loss"
        };
        let vs_v4 = if r.pbrs_enc + r.pbrs_dec < r.v4_enc + r.v4_dec {
            "win"
        } else {
            "loss"
        };
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} / {:.1} | {:.1} / {:.1} / {:.1} | {vs_prost} | {vs_v4} |",
            r.name,
            r.payload,
            r.pbrs_fresh_enc,
            r.pbrs_enc,
            r.pbrs_dec,
            r.pbrs_touch,
            r.prost_enc,
            r.prost_dec,
            r.prost_touch,
            r.v4_enc,
            r.v4_dec,
            r.v4_touch
        );
    }
    println!();
}

fn print_first_encodes(rows: &[Row]) {
    println!("First encode after parse (diagnostic; ns, preparation excluded):");
    println!("| case | prepared messages/sample | pbrs | prost | v4 |");
    println!("|---|---:|---:|---:|---:|");
    for r in rows {
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} |",
            r.name, r.first_iters, r.pbrs_fresh_enc, r.prost_first_enc, r.v4_first_enc
        );
    }
    println!();
}

fn print_constructs(rows: &[Row]) {
    println!("Fresh construction via new/setters (diagnostic; ns, no parse):");
    println!("| case | pbrs | prost | v4 |");
    println!("|---|---:|---:|---:|");
    for r in rows {
        println!(
            "| {} | {:.1} | {:.1} | {:.1} |",
            r.name, r.pbrs_construct, r.prost_construct, r.v4_construct
        );
    }
    println!();
}

fn print_retained(rows: &[Row]) {
    println!("Retained Rust heap per parsed message (diagnostic; bytes / allocation calls):");
    println!("| case | pbrs bytes/allocs | prost bytes/allocs | v4 bytes/allocs |");
    println!("|---|---:|---:|---:|");
    for r in rows {
        println!(
            "| {} | {} / {} | {} / {} | {} / {} |",
            r.name,
            r.pbrs_mem.bytes,
            r.pbrs_mem.allocs,
            r.prost_mem.bytes,
            r.prost_mem.allocs,
            r.v4_mem.bytes,
            r.v4_mem.allocs,
        );
    }
    println!("v4 cells exclude the upb Arena (C heap); Rust-side only.");
    println!();
}

fn touch_handwritten_person(m: &PbrsPerson) -> usize {
    m.id() as usize
        + bytes_sum(m.name().as_bytes())
        + m.email_opt().map_or(0, |email| bytes_sum(email.as_bytes()))
        + m.tags()
            .iter()
            .map(|tag| bytes_sum(tag.as_view().as_bytes()))
            .sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| bytes_sum(key.as_view().as_bytes()) + score as usize)
            .sum::<usize>()
        + bytes_sum(m.address().city().as_bytes())
}

fn touch_generated_person(m: &pbrs_person::Person) -> usize {
    m.id() as usize
        + bytes_sum(m.name().as_bytes())
        + m.email_opt().map_or(0, |email| bytes_sum(email.as_bytes()))
        + m.tags()
            .iter()
            .map(|tag| bytes_sum(tag.as_view().as_bytes()))
            .sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| bytes_sum(key.as_view().as_bytes()) + score as usize)
            .sum::<usize>()
        + bytes_sum(m.address().city().as_bytes())
        + m.extras()
            .iter()
            .map(|(key, value)| bytes_sum(key.as_view().as_bytes()) + value as usize)
            .sum::<usize>()
}

fn touch_prost_person(m: &ProstPerson) -> usize {
    m.id as usize
        + bytes_sum(m.name.as_bytes())
        + m.email
            .as_ref()
            .map_or(0, |email| bytes_sum(email.as_bytes()))
        + m.tags
            .iter()
            .map(|tag| bytes_sum(tag.as_bytes()))
            .sum::<usize>()
        + m.scores
            .iter()
            .map(|(key, score)| bytes_sum(key.as_bytes()) + *score as usize)
            .sum::<usize>()
        + m.address
            .as_ref()
            .map_or(0, |address| bytes_sum(address.city.as_bytes()))
        + m.extras
            .iter()
            .map(|(key, value)| bytes_sum(key.as_bytes()) + *value as usize)
            .sum::<usize>()
}

fn touch_v4_person(m: &v4_person::Person) -> usize {
    m.id() as usize
        + bytes_sum(m.name().as_bytes())
        + (if m.has_email() {
            bytes_sum(m.email().as_bytes())
        } else {
            0
        })
        + m.tags()
            .iter()
            .map(|tag| bytes_sum(tag.as_bytes()))
            .sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| bytes_sum(key.as_bytes()) + score as usize)
            .sum::<usize>()
        + bytes_sum(m.address().city().as_bytes())
        + m.extras()
            .iter()
            .map(|(key, value)| bytes_sum(key.as_bytes()) + value as usize)
            .sum::<usize>()
}

/// Constructor-parity builders: every Person row builds its reference
/// messages through these, and the timed construction column runs them too.
fn build_handwritten_person() -> PbrsPerson {
    let mut address = PbrsAddress::new();
    address.set_city("nyc");
    let mut person = PbrsPerson::new();
    person.set_id(7);
    person.set_name("ada lovelace");
    person.set_email("ada@example.com");
    person.tags_mut().push("math");
    person.tags_mut().push("eng");
    person.scores_mut().insert("notes", 12);
    person.set_address(address);
    person
}

fn build_generated_person() -> pbrs_person::Person {
    let mut address = pbrs_person::Address::new();
    address.set_city("nyc");
    let mut person = pbrs_person::Person::new();
    person.set_id(7);
    person.set_name("ada lovelace");
    person.set_email("ada@example.com");
    person.tags_mut().push("math");
    person.tags_mut().push("eng");
    person.scores_mut().insert("notes", 12);
    person.set_address(address);
    person
}

fn build_prost_person() -> ProstPerson {
    ProstPerson {
        id: 7,
        name: "ada lovelace".into(),
        email: Some("ada@example.com".into()),
        tags: ["math".to_owned(), "eng".to_owned()].into(),
        scores: [("notes".to_owned(), 12)].into_iter().collect(),
        address: Some(ProstAddress { city: "nyc".into() }),
        extras: std::collections::HashMap::new(),
    }
}

fn build_v4_person() -> v4_person::Person {
    let mut address = v4_person::Address::new();
    address.set_city("nyc");
    let mut person = v4_person::Person::new();
    person.set_id(7);
    person.set_name("ada lovelace");
    person.set_email("ada@example.com");
    person.tags_mut().push("math");
    person.tags_mut().push("eng");
    person.scores_mut().insert("notes", 12);
    person.set_address(address);
    person
}

fn build_generated_person_extras() -> pbrs_person::Person {
    let mut person = build_generated_person();
    person.extras_mut().insert("project", 7);
    person
}

fn build_prost_person_extras() -> ProstPerson {
    let mut person = build_prost_person();
    person.extras.insert("project".into(), 7);
    person
}

fn build_v4_person_extras() -> v4_person::Person {
    let mut person = build_v4_person();
    person.extras_mut().insert("project", 7);
    person
}

fn person_input_wire() -> Vec<u8> {
    Serialize::serialize(&build_handwritten_person()).expect("person input wire")
}

fn person_extras_wire(base: &[u8]) -> Vec<u8> {
    let mut person = pbrs_person::Person::parse(base).expect("generated Person fixture");
    person.extras_mut().insert("project", 7);
    Serialize::serialize(&person).expect("Person with typed extras")
}

fn run_person_extras_row(input: &[u8], budget: Option<(u32, usize)>) -> Row {
    let generated = build_generated_person_extras();
    let prost = build_prost_person_extras();
    let v4 = build_v4_person_extras();
    assert_eq!(
        Serialize::serialize(&generated).expect("generated extras wire"),
        input,
        "generated extras builder must reproduce the shared input wire"
    );
    assert_eq!(generated.extras().iter().count(), 1);
    assert_eq!(prost.extras.get("project"), Some(&7));
    assert!(
        v4.extras()
            .iter()
            .any(|(key, value)| key == "project" && value == 7),
        "v4 must expose the same typed extras field"
    );
    run_with_budget(
        "person_generated_extras",
        build_generated_person_extras,
        build_prost_person_extras,
        build_v4_person_extras,
        true,
        touch_generated_person,
        touch_prost_person,
        touch_v4_person,
        budget,
        false,
    )
}

fn mutation_id(alternate: bool) -> i32 {
    if alternate { 42 } else { 43 }
}

fn mutation_name(alternate: bool) -> &'static str {
    if alternate {
        "ada"
    } else {
        "ada lovelace with a longer name"
    }
}

fn run_person_rows(input: &[u8], budget: Option<(u32, usize)>) -> [Row; 2] {
    assert_eq!(
        Serialize::serialize(&build_handwritten_person()).expect("handwritten person wire"),
        input,
        "handwritten builder must reproduce the shared input wire"
    );
    assert_eq!(
        Serialize::serialize(&build_generated_person()).expect("generated person wire"),
        input,
        "generated builder must reproduce the shared input wire"
    );
    let handwritten_row = run_with_budget(
        "person_handwritten",
        build_handwritten_person,
        build_prost_person,
        build_v4_person,
        true,
        touch_handwritten_person,
        touch_prost_person,
        touch_v4_person,
        budget,
        false,
    );
    [
        handwritten_row,
        run_with_budget(
            "person_generated",
            build_generated_person,
            build_prost_person,
            build_v4_person,
            true,
            touch_generated_person,
            touch_prost_person,
            touch_v4_person,
            budget,
            false,
        ),
    ]
}

fn person_report(rows: &[Row; 2], extras: &Row) -> String {
    assert_eq!(rows[0].name, "person_handwritten");
    assert_eq!(rows[1].name, "person_generated");
    assert_eq!(extras.name, "person_generated_extras");
    let work = (
        rows[0].payload,
        rows[0].iters,
        rows[0].first_iters,
        rows[0].samples,
    );
    assert!(
        work.0 > 0 && work.1 > 0 && work.2 > 0 && work.2 <= work.1 && work.3 > 0,
        "person rows need measured work"
    );
    assert_eq!(
        work,
        (
            rows[1].payload,
            rows[1].iters,
            rows[1].first_iters,
            rows[1].samples
        ),
        "person rows must use the same input and sample counts"
    );
    assert!(
        extras.payload > work.0,
        "typed extras row must contain additional wire data"
    );
    assert_eq!(
        (extras.iters, extras.first_iters, extras.samples),
        (work.1, work.2, work.3),
        "typed extras row must use the same sample budget"
    );
    let mut report = String::from(
        "## Person layouts (proto/person.proto; diagnostic, ns, not gated)\n\
         | case (pbrs layout) | payload | repeated/parse iters | first-encode iters | samples | pbrs enc first/prewarmed | pbrs dec parse/touch | prost enc first/repeated | prost dec parse/touch | v4 enc first/repeated | v4 dec parse/touch | pbrs construct | prost construct | v4 construct |\n\
         |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for row in rows.iter().chain(std::iter::once(extras)) {
        for value in [
            row.pbrs_enc,
            row.pbrs_fresh_enc,
            row.pbrs_dec,
            row.pbrs_touch,
            row.pbrs_construct,
            row.prost_enc,
            row.prost_first_enc,
            row.prost_dec,
            row.prost_touch,
            row.prost_construct,
            row.v4_enc,
            row.v4_first_enc,
            row.v4_dec,
            row.v4_touch,
            row.v4_construct,
        ] {
            assert!(
                value.is_finite() && value > 0.0,
                "person report has an invalid measured time for {}",
                row.name
            );
        }
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} | {:.1} | {:.1} |\n",
            row.name,
            row.payload,
            row.iters,
            row.first_iters,
            row.samples,
            row.pbrs_fresh_enc,
            row.pbrs_enc,
            row.pbrs_dec,
            row.pbrs_touch,
            row.prost_first_enc,
            row.prost_enc,
            row.prost_dec,
            row.prost_touch,
            row.v4_first_enc,
            row.v4_enc,
            row.v4_dec,
            row.v4_touch,
            row.pbrs_construct,
            row.prost_construct,
            row.v4_construct,
        ));
    }
    report.push_str(
        "\nEach row uses a matched wire across its codecs; prost/v4 are timed independently. \
         The two layout rows share an empty-extras wire; the generated-only extras row has \
         one typed tag-16 entry and no handwritten comparator. \
         Touch reads every populated field and every string/bytes payload byte. \
         Construction builds the same specimen each codec encodes; rebuilt wire must match. \
         Fixed-order, same-process timings do not qualify a speed or memory claim.\n",
    );
    report
}

fn encode_pbrs_person<P: Serialize>(message: &P, dst: &mut BytesMut) {
    dst.clear();
    Serialize::encode(message, dst).expect("pbrs person encode");
}

fn encode_prost_person(message: &ProstPerson, dst: &mut BytesMut) {
    dst.clear();
    prost::Message::encode(message, dst).expect("prost person encode");
}

fn verify_person_mutations(input: &[u8]) {
    let mut pbrs = PbrsPerson::parse(input).expect("pbrs person input");
    let mut generated = pbrs_person::Person::parse(input).expect("generated pbrs person input");
    let mut prost: ProstPerson = prost::Message::decode(input).expect("prost person input");
    let mut v4 = v4_person::Person::parse(input).expect("v4 person input");
    let mut pbrs_dst = BytesMut::new();
    let mut generated_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    encode_pbrs_person(&pbrs, &mut pbrs_dst);
    assert_eq!(&pbrs_dst[..], input, "person: pbrs input wire");
    encode_pbrs_person(&generated, &mut generated_dst);
    assert_eq!(
        &generated_dst[..],
        input,
        "person: generated pbrs input wire"
    );
    encode_prost_person(&prost, &mut prost_dst);
    assert_eq!(&prost_dst[..], input, "person: prost input wire");
    assert_eq!(
        V4Serialize::serialize(&v4).expect("v4 person input wire"),
        input,
        "person: v4 input wire"
    );

    for id in [42, 43] {
        let mut expected = PbrsPerson::parse(input).expect("person mutation reference");
        expected.set_id(id);
        let expected_wire = Serialize::serialize(&expected).expect("person expected wire");
        pbrs.set_id(id);
        generated.set_id(id);
        prost.id = id;
        v4.set_id(id);

        encode_pbrs_person(&pbrs, &mut pbrs_dst);
        assert_person_mutation_output("pbrs", &pbrs_dst, &expected_wire, id);
        encode_pbrs_person(&generated, &mut generated_dst);
        assert_person_mutation_output("pbrs generated", &generated_dst, &expected_wire, id);
        encode_prost_person(&prost, &mut prost_dst);
        assert_person_mutation_output("prost", &prost_dst, &expected_wire, id);
        let v4_wire = V4Serialize::serialize(&v4).expect("v4 person mutation wire");
        assert_person_mutation_output("v4", &v4_wire, &expected_wire, id);
    }

    let mut pbrs = PbrsPerson::parse(input).expect("pbrs name mutation input");
    let mut generated = pbrs_person::Person::parse(input).expect("generated name mutation input");
    let mut prost: ProstPerson = prost::Message::decode(input).expect("prost name mutation input");
    let mut v4 = v4_person::Person::parse(input).expect("v4 name mutation input");
    for name in [mutation_name(true), mutation_name(false)] {
        let mut expected = PbrsPerson::parse(input).expect("name mutation reference");
        expected.set_name(name);
        let expected_wire = Serialize::serialize(&expected).expect("name mutation expected wire");
        pbrs.set_name(name);
        generated.set_name(name);
        prost.name = name.to_owned();
        v4.set_name(name);

        encode_pbrs_person(&pbrs, &mut pbrs_dst);
        assert_person_mutation_output("pbrs name", &pbrs_dst, &expected_wire, 7);
        encode_pbrs_person(&generated, &mut generated_dst);
        assert_person_mutation_output("pbrs generated name", &generated_dst, &expected_wire, 7);
        encode_prost_person(&prost, &mut prost_dst);
        assert_person_mutation_output("prost name", &prost_dst, &expected_wire, 7);
        let v4_wire = V4Serialize::serialize(&v4).expect("v4 name mutation wire");
        assert_person_mutation_output("v4 name", &v4_wire, &expected_wire, 7);
    }
}

#[derive(Clone, Copy)]
struct MutationRow {
    name: &'static str,
    transition: &'static str,
    payload: usize,
    iters: u32,
    samples: usize,
    pbrs_ns: f64,
    prost_ns: f64,
    v4_ns: f64,
}

struct MutationLabel {
    layout: &'static str,
    transition: &'static str,
}

fn person_mutation_budget(iters: u32, payload: usize) -> u32 {
    first_encode_budget::<PbrsPerson, ProstPerson, v4_person::Person>(iters, payload).min(
        first_encode_budget::<pbrs_person::Person, ProstPerson, v4_person::Person>(iters, payload),
    )
}

fn measure_person_mutation<P, UP, UR, UV>(
    label: MutationLabel,
    input: &[u8],
    iters: u32,
    samples: usize,
    mut set_pbrs: UP,
    mut set_prost: UR,
    mut set_v4: UV,
) -> MutationRow
where
    P: Parse + Serialize,
    UP: FnMut(&mut P, bool),
    UR: FnMut(&mut ProstPerson, bool),
    UV: FnMut(&mut v4_person::Person, bool),
{
    let mut pbrs_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    let (pbrs_ns, raw_pbrs) = median_mutated_encode_ns_raw(
        samples,
        iters,
        || {
            let message = P::parse(input).expect("pbrs mutation parse");
            let mut warm = BytesMut::new();
            encode_pbrs_person(&message, &mut warm);
            std::hint::black_box(&warm[..]);
            message
        },
        |message, alternate| set_pbrs(message, alternate),
        |message| {
            encode_pbrs_person(message, &mut pbrs_dst);
            std::hint::black_box(&pbrs_dst[..]);
        },
    );
    let (prost_ns, raw_prost) = median_mutated_encode_ns_raw(
        samples,
        iters,
        || {
            let message: ProstPerson = prost::Message::decode(input).expect("prost mutation parse");
            std::hint::black_box(prost::Message::encode_to_vec(&message));
            message
        },
        |message, alternate| set_prost(message, alternate),
        |message| {
            encode_prost_person(message, &mut prost_dst);
            std::hint::black_box(&prost_dst[..]);
        },
    );
    let (v4_ns, raw_v4) = median_mutated_encode_ns_raw(
        samples,
        iters,
        || {
            let message = v4_person::Person::parse(input).expect("v4 mutation parse");
            std::hint::black_box(V4Serialize::serialize(&message).expect("v4 person warmup"));
            message
        },
        |message, alternate| set_v4(message, alternate),
        |message| {
            std::hint::black_box(V4Serialize::serialize(message).expect("v4 person encode"));
        },
    );
    push_raw_row(RawRow {
        name: label.layout,
        detail: label.transition,
        holdout: false,
        payload: input.len(),
        iters,
        samples,
        first_iters: 0,
        cols: Vec::from([
            ("pbrs_mutated_enc", raw_pbrs),
            ("prost_mutated_enc", raw_prost),
            ("v4_mutated_enc", raw_v4),
        ]),
    });
    MutationRow {
        name: label.layout,
        transition: label.transition,
        payload: input.len(),
        iters,
        samples,
        pbrs_ns,
        prost_ns,
        v4_ns,
    }
}

fn run_person_mutations(input: &[u8], iters: u32, samples: usize) -> [MutationRow; 4] {
    verify_person_mutations(input);
    let iters = person_mutation_budget(iters, input.len());
    [
        measure_person_mutation::<PbrsPerson, _, _, _>(
            MutationLabel {
                layout: "person_handwritten",
                transition: "id 42 <-> 43",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_id(mutation_id(alternate)),
            |message, alternate| message.id = mutation_id(alternate),
            |message, alternate| message.set_id(mutation_id(alternate)),
        ),
        measure_person_mutation::<pbrs_person::Person, _, _, _>(
            MutationLabel {
                layout: "person_generated",
                transition: "id 42 <-> 43",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_id(mutation_id(alternate)),
            |message, alternate| message.id = mutation_id(alternate),
            |message, alternate| message.set_id(mutation_id(alternate)),
        ),
        measure_person_mutation::<PbrsPerson, _, _, _>(
            MutationLabel {
                layout: "person_handwritten",
                transition: "name ada <-> longer",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_name(mutation_name(alternate)),
            |message, alternate| message.name = mutation_name(alternate).to_owned(),
            |message, alternate| message.set_name(mutation_name(alternate)),
        ),
        measure_person_mutation::<pbrs_person::Person, _, _, _>(
            MutationLabel {
                layout: "person_generated",
                transition: "name ada <-> longer",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_name(mutation_name(alternate)),
            |message, alternate| message.name = mutation_name(alternate).to_owned(),
            |message, alternate| message.set_name(mutation_name(alternate)),
        ),
    ]
}

fn mutation_report(rows: &[MutationRow; 4]) -> String {
    let work = (rows[0].payload, rows[0].iters, rows[0].samples);
    assert!(
        work.0 > 0 && work.1 > 0 && work.2 > 0,
        "mutation report needs measured work"
    );
    for (row, expected) in rows.iter().zip([
        ("person_handwritten", "id 42 <-> 43"),
        ("person_generated", "id 42 <-> 43"),
        ("person_handwritten", "name ada <-> longer"),
        ("person_generated", "name ada <-> longer"),
    ]) {
        assert_eq!(
            (row.name, row.transition),
            expected,
            "mutation row identity"
        );
        assert_eq!(
            work,
            (row.payload, row.iters, row.samples),
            "mutation rows must use the same input and sample counts"
        );
    }
    let mut report = String::from(
        "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):\n\
         | case (pbrs layout) | field transition | payload | iterations/sample | samples | pbrs | prost | v4 |\n\
         |---|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for row in rows {
        for (codec, value) in [
            ("pbrs", row.pbrs_ns),
            ("prost", row.prost_ns),
            ("v4", row.v4_ns),
        ] {
            assert!(
                value.is_finite() && value > 0.0,
                "mutation report has invalid {codec} time for {}",
                row.name
            );
        }
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1} | {:.1} | {:.1} |\n",
            row.name,
            row.transition,
            row.payload,
            row.iters,
            row.samples,
            row.pbrs_ns,
            row.prost_ns,
            row.v4_ns
        ));
    }
    report.push_str(
        "\nName states are \"ada\" and \"ada lovelace with a longer name\". \
         Each row times its own pbrs, prost and v4 mutation independently in fixed order; \
         no gate or speed claim.\n",
    );
    report
}

fn main() {
    let hello_short = "ada";
    let hello_4k = "x".repeat(4096);
    let name_80 = "x".repeat(80);
    let blob_32 = vec![0x5a; 32];
    let blob_4k = vec![0x5a; 4096];
    let blob_64k = vec![0x5a; 64 * 1024];

    // Constructor-parity builders: each row builds its reference messages
    // through these closures, and the timed construction column runs them.
    let build_p_hello = || pbrs_hello(hello_short);
    let build_r_hello = || prost_hello(hello_short);

    let build_p_hello4 = || pbrs_hello(&hello_4k);
    let build_r_hello4 = || prost_hello(&hello_4k);

    let build_p_name = || {
        let mut m = pbrs_cases::Name::new();
        m.set_name(hello_short);
        m
    };
    let build_r_name = || prost_cases::Name {
        name: hello_short.into(),
    };

    let build_p_name80 = || {
        let mut m = pbrs_cases::Name::new();
        m.set_name(name_80.as_str());
        m
    };
    let build_r_name80 = || prost_cases::Name {
        name: name_80.clone(),
    };
    let build_v_name80 = || v4_name(name_80.as_str());

    let build_p_name4k = || {
        let mut m = pbrs_cases::Name::new();
        m.set_name(hello_4k.as_str());
        m
    };
    let build_r_name4k = || prost_cases::Name {
        name: hello_4k.clone(),
    };
    let build_v_name4k = || v4_name(&hello_4k);

    let build_p_id = || {
        let mut m = pbrs_cases::Id::new();
        m.set_id(7);
        m
    };
    let build_r_id = || prost_cases::Id { id: 7 };
    let build_v_id = || {
        let mut m = v4_cases::Id::new();
        m.set_id(7);
        m
    };

    let build_p_sc = || {
        let mut m = pbrs_cases::Scalars::new();
        m.set_id(7);
        m.set_seq(3);
        m.set_ok(true);
        m.set_status(1);
        m.set_ts(1_700_000_000);
        m.set_lat(1.5);
        m
    };
    let build_r_sc = || prost_cases::Scalars {
        id: 7,
        seq: 3,
        ok: true,
        status: 1,
        ts: 1_700_000_000,
        lat: 1.5,
    };
    let build_v_sc = || {
        let mut m = v4_cases::Scalars::new();
        m.set_id(7);
        m.set_seq(3);
        m.set_ok(true);
        m.set_status(v4_cases::Status::Ok);
        m.set_ts(1_700_000_000);
        m.set_lat(1.5);
        m
    };

    let build_p_b32 = || {
        let mut m = pbrs_cases::Blob::new();
        m.set_payload(blob_32.as_slice());
        m
    };
    let build_r_b32 = || prost_cases::Blob {
        payload: blob_32.clone(),
    };
    let build_v_b32 = || {
        let mut m = v4_cases::Blob::new();
        m.set_payload(blob_32.as_slice());
        m
    };

    let build_p_b4k = || {
        let mut m = pbrs_cases::Blob::new();
        m.set_payload(blob_4k.as_slice());
        m
    };
    let build_r_b4k = || prost_cases::Blob {
        payload: blob_4k.clone(),
    };
    let build_v_b4k = || {
        let mut m = v4_cases::Blob::new();
        m.set_payload(blob_4k.as_slice());
        m
    };

    let build_p_b64 = || {
        let mut m = pbrs_cases::Blob::new();
        m.set_payload(blob_64k.as_slice());
        m
    };
    let build_r_b64 = || prost_cases::Blob {
        payload: blob_64k.clone(),
    };
    let build_v_b64 = || {
        let mut m = v4_cases::Blob::new();
        m.set_payload(blob_64k.as_slice());
        m
    };

    let build_p_env = || {
        let mut m = pbrs_cases::Envelope::new();
        m.set_meta(meta_pbrs());
        m.set_body("hello body");
        m
    };
    let build_r_env = || prost_cases::Envelope {
        meta: Some(meta_prost()),
        body: "hello body".into(),
    };
    let build_v_env = || {
        let mut m = v4_cases::Envelope::new();
        m.set_meta(meta_v4());
        m.set_body("hello body");
        m
    };

    let build_p_nest = || pbrs_node(4);
    let build_r_nest = || prost_node(4);
    let build_v_nest = || v4_node(4);

    let build_p_ids16 = || {
        let mut m = pbrs_cases::Ids::new();
        m.set_ids(0..16);
        m
    };
    let build_r_ids16 = || prost_cases::Ids {
        ids: (0..16).collect(),
    };
    let build_v_ids16 = || {
        let mut m = v4_cases::Ids::new();
        for i in 0..16 {
            m.ids_mut().push(i);
        }
        m
    };

    let build_p_ids256 = || {
        let mut m = pbrs_cases::Ids::new();
        m.set_ids(0..256);
        m
    };
    let build_r_ids256 = || prost_cases::Ids {
        ids: (0..256).collect(),
    };
    let build_v_ids256 = || {
        let mut m = v4_cases::Ids::new();
        for i in 0..256 {
            m.ids_mut().push(i);
        }
        m
    };

    let tags4 = ["alpha", "beta", "gamma", "delta"];
    let build_p_tags4 = || {
        let mut m = pbrs_cases::Tags::new();
        for t in tags4 {
            m.tags_mut().push(t);
        }
        m
    };
    let build_r_tags4 = || prost_cases::Tags {
        tags: tags4.iter().map(|s| (*s).to_string()).collect(),
    };
    let build_v_tags4 = || {
        let mut m = v4_cases::Tags::new();
        for t in tags4 {
            m.tags_mut().push(t);
        }
        m
    };

    let tag32: Vec<String> = (0..32).map(|i| format!("t{i:02}")).collect();
    let build_p_tags32 = || {
        let mut m = pbrs_cases::Tags::new();
        for t in &tag32 {
            m.tags_mut().push(t.as_str());
        }
        m
    };
    let build_r_tags32 = || prost_cases::Tags {
        tags: tag32.clone(),
    };
    let build_v_tags32 = || {
        let mut m = v4_cases::Tags::new();
        for t in &tag32 {
            m.tags_mut().push(t.as_str());
        }
        m
    };

    let hdrs = [
        ("content-type", "application/json"),
        ("accept", "*/*"),
        ("user-agent", "bench"),
        ("x-request-id", "abc"),
        ("x-trace", "1"),
        ("host", "example"),
        ("authorization", "none"),
        ("cache-control", "no-store"),
    ];
    let build_p_map = || {
        let mut m = pbrs_cases::Headers::new();
        for (k, v) in hdrs {
            m.h_mut().insert(k, v);
        }
        m
    };
    let build_r_map = || prost_cases::Headers {
        h: hdrs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
    };
    let build_v_map = || {
        let mut m = v4_cases::Headers::new();
        for (k, v) in hdrs {
            m.h_mut().insert(k, v);
        }
        m
    };

    let build_p_ok = || {
        let mut m = pbrs_cases::PbResult::new();
        m.set_ok("fine");
        m
    };
    let build_r_ok = || prost_cases::Result {
        kind: Some(prost_cases::result::Kind::Ok("fine".into())),
    };
    let build_v_ok = || {
        let mut m = v4_cases::Result::new();
        m.set_ok("fine");
        m
    };

    let build_p_rpc = || {
        let mut m = pbrs_cases::Rpc::new();
        m.set_id(99);
        m.set_method("Get");
        m.set_path("/v1/items");
        m.set_user("ada");
        m.set_meta(meta_pbrs());
        m.set_ids(0..8);
        for t in tags4 {
            m.tags_mut().push(t);
        }
        for (k, v) in hdrs.iter().take(4) {
            m.headers_mut().insert(*k, *v);
        }
        m.set_extra(&b"extra"[..]);
        m
    };
    let build_r_rpc = || prost_cases::Rpc {
        id: 99,
        method: "Get".into(),
        path: "/v1/items".into(),
        user: "ada".into(),
        meta: Some(meta_prost()),
        ids: (0..8).collect(),
        tags: tags4.iter().map(|s| (*s).to_string()).collect(),
        headers: hdrs
            .iter()
            .take(4)
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        extra: b"extra".to_vec(),
    };
    let build_v_rpc = || {
        let mut m = v4_cases::Rpc::new();
        m.set_id(99);
        m.set_method("Get");
        m.set_path("/v1/items");
        m.set_user("ada");
        m.set_meta(meta_v4());
        for i in 0..8 {
            m.ids_mut().push(i);
        }
        for t in tags4 {
            m.tags_mut().push(t);
        }
        for (k, v) in hdrs.iter().take(4) {
            m.headers_mut().insert(*k, *v);
        }
        m.set_extra(&b"extra"[..]);
        m
    };

    let build_p_sparse = || {
        let mut m = pbrs_cases::Rpc::new();
        m.set_id(99);
        m
    };
    let build_r_sparse = || prost_cases::Rpc {
        id: 99,
        ..Default::default()
    };
    let build_v_sparse = || {
        let mut m = v4_cases::Rpc::new();
        m.set_id(99);
        m
    };

    // hello has no v4 twin in this crate; Name is the same 1-string shape.
    let published = [
        run(
            "hello",
            build_p_hello,
            build_r_hello,
            || v4_name(hello_short),
            true,
            |m| bytes_sum(m.name().as_bytes()),
            |m| bytes_sum(m.name.as_bytes()),
            |m| bytes_sum(m.name().as_bytes()),
        ),
        run(
            "hello_4kib",
            build_p_hello4,
            build_r_hello4,
            || v4_name(&hello_4k),
            true,
            |m| bytes_sum(m.name().as_bytes()),
            |m| bytes_sum(m.name.as_bytes()),
            |m| bytes_sum(m.name().as_bytes()),
        ),
    ];

    let survey = [
        run(
            "empty",
            pbrs_cases::Empty::new,
            || prost_cases::Empty {},
            v4_cases::Empty::new,
            true,
            |_| 0,
            |_| 0,
            |_| 0,
        ),
        run(
            "id",
            build_p_id,
            build_r_id,
            build_v_id,
            true,
            |m| m.id() as usize,
            |m| m.id as usize,
            |m| m.id() as usize,
        ),
        run(
            "scalars",
            build_p_sc,
            build_r_sc,
            build_v_sc,
            true,
            |m| {
                m.id() as usize
                    + m.seq() as usize
                    + (m.ok() as usize)
                    + (i32::from(m.status()) as usize)
                    + m.ts() as usize
                    + m.lat() as usize
            },
            |m| {
                m.id as usize
                    + m.seq as usize
                    + (m.ok as usize)
                    + m.status as usize
                    + m.ts as usize
                    + m.lat as usize
            },
            |m| {
                m.id() as usize
                    + m.seq() as usize
                    + (m.ok() as usize)
                    + (i32::from(m.status()) as usize)
                    + m.ts() as usize
                    + m.lat() as usize
            },
        ),
        run(
            "name_short",
            build_p_name,
            build_r_name,
            || v4_name(hello_short),
            true,
            |m| bytes_sum(m.name().as_bytes()),
            |m| bytes_sum(m.name.as_bytes()),
            |m| bytes_sum(m.name().as_bytes()),
        ),
        run(
            "name_80",
            build_p_name80,
            build_r_name80,
            build_v_name80,
            true,
            |m| bytes_sum(m.name().as_bytes()),
            |m| bytes_sum(m.name.as_bytes()),
            |m| bytes_sum(m.name().as_bytes()),
        ),
        run(
            "name_4kib",
            build_p_name4k,
            build_r_name4k,
            build_v_name4k,
            true,
            |m| bytes_sum(m.name().as_bytes()),
            |m| bytes_sum(m.name.as_bytes()),
            |m| bytes_sum(m.name().as_bytes()),
        ),
        run(
            "blob_32",
            build_p_b32,
            build_r_b32,
            build_v_b32,
            true,
            |m| bytes_sum(m.payload()),
            |m| bytes_sum(&m.payload),
            |m| bytes_sum(m.payload()),
        ),
        run(
            "blob_4kib",
            build_p_b4k,
            build_r_b4k,
            build_v_b4k,
            true,
            |m| bytes_sum(m.payload()),
            |m| bytes_sum(&m.payload),
            |m| bytes_sum(m.payload()),
        ),
        run(
            "blob_64kib",
            build_p_b64,
            build_r_b64,
            build_v_b64,
            true,
            |m| bytes_sum(m.payload()),
            |m| bytes_sum(&m.payload),
            |m| bytes_sum(m.payload()),
        ),
        run(
            "envelope",
            build_p_env,
            build_r_env,
            build_v_env,
            true,
            |m| {
                m.meta().id() as usize
                    + m.meta().ts() as usize
                    + bytes_sum(m.meta().trace().as_bytes())
                    + bytes_sum(m.body().as_bytes())
            },
            |m| {
                m.meta
                    .as_ref()
                    .map(|x| x.id as usize + x.ts as usize + bytes_sum(x.trace.as_bytes()))
                    .unwrap_or(0)
                    + bytes_sum(m.body.as_bytes())
            },
            |m| {
                m.meta().id() as usize
                    + m.meta().ts() as usize
                    + bytes_sum(m.meta().trace().as_bytes())
                    + bytes_sum(m.body().as_bytes())
            },
        ),
        run(
            "nest_d4",
            build_p_nest,
            build_r_nest,
            build_v_nest,
            true,
            touch_node_pbrs,
            touch_node_prost,
            touch_node_v4,
        ),
        run(
            "packed_16",
            build_p_ids16,
            build_r_ids16,
            build_v_ids16,
            true,
            |m| m.ids().iter().sum::<i64>() as usize,
            |m| m.ids.iter().sum::<i64>() as usize,
            |m| m.ids().iter().sum::<i64>() as usize,
        ),
        run(
            "packed_256",
            build_p_ids256,
            build_r_ids256,
            build_v_ids256,
            true,
            |m| m.ids().iter().sum::<i64>() as usize,
            |m| m.ids.iter().sum::<i64>() as usize,
            |m| m.ids().iter().sum::<i64>() as usize,
        ),
        run(
            "tags_4",
            build_p_tags4,
            build_r_tags4,
            build_v_tags4,
            true,
            |m| m.tags().iter().map(|s| bytes_sum(s.as_bytes())).sum(),
            |m| m.tags.iter().map(|s| bytes_sum(s.as_bytes())).sum(),
            |m| m.tags().iter().map(|s| bytes_sum(s.as_bytes())).sum(),
        ),
        run(
            "tags_32",
            build_p_tags32,
            build_r_tags32,
            build_v_tags32,
            true,
            |m| m.tags().iter().map(|s| bytes_sum(s.as_bytes())).sum(),
            |m| m.tags.iter().map(|s| bytes_sum(s.as_bytes())).sum(),
            |m| m.tags().iter().map(|s| bytes_sum(s.as_bytes())).sum(),
        ),
        run(
            "map_8",
            build_p_map,
            build_r_map,
            build_v_map,
            false,
            |m| {
                m.h()
                    .iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
            |m| {
                m.h.iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
            |m| {
                m.h()
                    .iter()
                    .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                    .sum()
            },
        ),
        run(
            "oneof_ok",
            build_p_ok,
            build_r_ok,
            build_v_ok,
            true,
            |m| m.ok_opt().map(|s| bytes_sum(s.as_bytes())).unwrap_or(0),
            |m| match &m.kind {
                Some(prost_cases::result::Kind::Ok(s)) => bytes_sum(s.as_bytes()),
                _ => 0,
            },
            |m| {
                if m.has_ok() {
                    bytes_sum(m.ok().as_bytes())
                } else {
                    0
                }
            },
        ),
        run(
            "rpc_mixed",
            build_p_rpc,
            build_r_rpc,
            build_v_rpc,
            false,
            |m| {
                m.id() as usize
                    + bytes_sum(m.method().as_bytes())
                    + bytes_sum(m.path().as_bytes())
                    + bytes_sum(m.user().as_bytes())
                    + m.meta().id() as usize
                    + m.meta().ts() as usize
                    + bytes_sum(m.meta().trace().as_bytes())
                    + m.ids().iter().sum::<i64>() as usize
                    + m.tags()
                        .iter()
                        .map(|s| bytes_sum(s.as_bytes()))
                        .sum::<usize>()
                    + m.headers()
                        .iter()
                        .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                        .sum::<usize>()
                    + bytes_sum(m.extra())
            },
            |m| {
                m.id as usize
                    + bytes_sum(m.method.as_bytes())
                    + bytes_sum(m.path.as_bytes())
                    + bytes_sum(m.user.as_bytes())
                    + m.meta
                        .as_ref()
                        .map(|x| x.id as usize + x.ts as usize + bytes_sum(x.trace.as_bytes()))
                        .unwrap_or(0)
                    + m.ids.iter().sum::<i64>() as usize
                    + m.tags
                        .iter()
                        .map(|s| bytes_sum(s.as_bytes()))
                        .sum::<usize>()
                    + m.headers
                        .iter()
                        .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                        .sum::<usize>()
                    + bytes_sum(&m.extra)
            },
            |m| {
                m.id() as usize
                    + bytes_sum(m.method().as_bytes())
                    + bytes_sum(m.path().as_bytes())
                    + bytes_sum(m.user().as_bytes())
                    + m.meta().id() as usize
                    + m.meta().ts() as usize
                    + bytes_sum(m.meta().trace().as_bytes())
                    + m.ids().iter().sum::<i64>() as usize
                    + m.tags()
                        .iter()
                        .map(|s| bytes_sum(s.as_bytes()))
                        .sum::<usize>()
                    + m.headers()
                        .iter()
                        .map(|(k, v)| bytes_sum(k.as_bytes()) + bytes_sum(v.as_bytes()))
                        .sum::<usize>()
                    + bytes_sum(m.extra())
            },
        ),
        run(
            "rpc_sparse",
            build_p_sparse,
            build_r_sparse,
            build_v_sparse,
            true,
            |m| m.id() as usize,
            |m| m.id as usize,
            |m| m.id() as usize,
        ),
    ];

    println!("# Codec survey (encode into BytesMut; v4 serialize is Arena+FFI)");
    println!("iters=40000 samples=15 except payload>=32KiB (4000x9). median. release thin-LTO.");
    println!("pbrs vs prost vs crates.io protobuf 4.35.1-release (upb).");
    println!("Holdout shapes are measured but never tuned against or gated.");
    println!("map_8 / rpc_mixed skip byte-equal (HashMap order); decoded values checked.");
    println!(
        "First encode is directly timed from separately parsed messages with matched input counts."
    );
    println!("Construction builds the same specimen each codec encodes; rebuilt wire must match.");
    println!(
        "pbrs may retain lazy wire backing; prost owns fields; v4 uses an upb Arena. No views."
    );
    println!();
    print_table(
        "## Published 1-string (hello.proto; v4 uses wire-equivalent cases.Name)",
        &published,
    );
    print_first_encodes(&published);
    print_constructs(&published);
    print_table("## Common shapes (codec_cases.proto)", &survey);
    print_first_encodes(&survey);
    print_constructs(&survey);
    let holdouts = run_holdout_rows(None);
    assert!(
        published
            .iter()
            .chain(survey.iter())
            .all(|row| !row.holdout),
        "survey rows must not be flagged holdout"
    );
    assert!(
        holdouts.iter().all(|row| row.holdout),
        "holdout rows must all be flagged holdout"
    );
    print_table(
        "## Holdout shapes (codec_cases.proto; not tuned, not gated)",
        &holdouts,
    );
    print_first_encodes(&holdouts);
    print_constructs(&holdouts);
    let person_wire = person_input_wire();
    let person_rows = run_person_rows(&person_wire, None);
    let extras_wire = person_extras_wire(&person_wire);
    let extras_row = run_person_extras_row(&extras_wire, None);
    println!("{}", person_report(&person_rows, &extras_row));
    let (person_iters, person_samples) = timer_budget(person_wire.len());
    println!(
        "{}",
        mutation_report(&run_person_mutations(
            &person_wire,
            person_iters,
            person_samples
        ))
    );
    print_retained(&published);
    print_retained(&survey);
    print_retained(&holdouts);
    print_retained(&[person_rows[0], person_rows[1], extras_row]);
    println!("## Raw paired samples (schema tonic-raw/1; fixed-order sequential, not interleaved)");
    print_raw_block();

    let mut failed = false;
    for r in survey.iter() {
        if r.name == "rpc_sparse" {
            if r.pbrs_dec >= r.prost_dec {
                eprintln!(
                    "perf gate failed: rpc_sparse decode {:.1} vs prost {:.1}",
                    r.pbrs_dec, r.prost_dec
                );
                failed = true;
            }
            continue;
        }
        if r.name == "tags_32" {
            if r.pbrs_dec >= r.v4_dec {
                eprintln!(
                    "perf gate failed: tags_32 decode {:.1} vs v4 {:.1}",
                    r.pbrs_dec, r.v4_dec
                );
                failed = true;
            }
            continue;
        }
        if r.name != "name_4kib" && r.name != "blob_4kib" {
            continue;
        }
        let ours = r.pbrs_enc + r.pbrs_dec;
        let prost = r.prost_enc + r.prost_dec;
        if ours >= prost {
            eprintln!(
                "perf gate failed: {} combined {:.1} vs prost {:.1}",
                r.name, ours, prost
            );
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MutationRow, ProstPerson, assert_person_mutation_output, build_generated_person,
        build_generated_person_extras, build_prost_person, build_prost_person_extras,
        build_v4_person, build_v4_person_extras, first_encode_budget, median_first_encode_ns,
        median_mutated_encode_ns, mutation_report, pbrs_person, person_extras_wire,
        person_input_wire, person_mutation_budget, person_report, run_holdout_rows,
        run_person_extras_row, run_person_mutations, run_person_rows, take_raw_rows,
        touch_generated_person, touch_handwritten_person, touch_prost_person, touch_v4_person,
        v4_person, verify_person_mutations,
    };
    use pbrs::testdata::Person as PbrsPerson;
    use pbrs::{AsView, Parse, Serialize};
    use protobuf::{Parse as V4Parse, Serialize as V4Serialize};

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Hold for the whole body of every test in this module: the
    /// retained-memory counters are process-wide, so heap churn on any
    /// other test thread can shrink a `measure_retained` window to zero
    /// and fail a live-counter assertion spuriously.
    fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn first_encode_prepares_and_encodes_every_sample_once() {
        let _serial = serial_guard();
        let mut prepared = 0usize;
        let mut encoded = Vec::new();
        let measurement = median_first_encode_ns(
            3,
            10,
            || {
                prepared += 1;
                prepared
            },
            |message| {
                encoded.push(*message);
                *message
            },
        );
        assert!(measurement.is_finite());
        assert_eq!(prepared, 33);
        assert_eq!(encoded, (1..=33).collect::<Vec<_>>());
    }

    #[test]
    fn first_encode_budget_limits_the_estimated_prepared_footprint() {
        let _serial = serial_guard();
        let payload = 64 * 1024;
        let count = first_encode_budget::<u64, u64, u64>(40_000, payload);
        assert!(count <= 10_000);
        assert!(
            usize::try_from(count).expect("bounded count") * (payload + size_of::<u64>())
                <= 32 * 1024 * 1024
        );
        assert_eq!(
            first_encode_budget::<u64, u64, u64>(40_000, 20 * 1024 * 1024),
            1
        );
        assert!(
            std::panic::catch_unwind(|| {
                first_encode_budget::<u64, u64, u64>(40_000, 33 * 1024 * 1024)
            })
            .is_err()
        );
    }

    #[test]
    fn mutation_helper_reuses_each_parsed_message_and_alternates_before_every_encode() {
        let _serial = serial_guard();
        let mut prepared = 0usize;
        let mut encoded = Vec::new();
        let measurement = median_mutated_encode_ns(
            3,
            10,
            || {
                prepared += 1;
                7
            },
            |message, alternate| *message = super::mutation_id(alternate),
            |message| {
                encoded.push(*message);
                vec![*message as u8]
            },
        );
        assert!(measurement.is_finite());
        assert_eq!(prepared, 3);
        assert_eq!(encoded.len(), 33);
        assert_eq!(
            encoded,
            [42, 43, 42, 43, 42, 43, 42, 43, 42, 43, 42].repeat(3)
        );
    }

    #[test]
    fn generated_person_matches_the_populated_handwritten_fixture() {
        let _serial = serial_guard();
        let input = person_input_wire();
        assert_eq!(input.len(), 62);
        let handwritten = PbrsPerson::parse(&input).expect("handwritten person");
        let generated = pbrs_person::Person::parse(&input).expect("generated person");
        let prost: ProstPerson = prost::Message::decode(input.as_slice()).expect("prost person");
        let v4 = v4_person::Person::parse(&input).expect("v4 person");

        assert_eq!(generated.id(), 7);
        assert_eq!(generated.name().as_bytes(), b"ada lovelace");
        assert_eq!(
            generated.email_opt().expect("email is present").as_bytes(),
            b"ada@example.com"
        );
        assert_eq!(
            generated
                .tags()
                .iter()
                .map(|tag| tag.as_view().to_str().expect("valid tag").to_owned())
                .collect::<Vec<_>>(),
            ["math", "eng"]
        );
        assert_eq!(
            generated
                .scores()
                .iter()
                .map(|(key, value)| (
                    key.as_view().to_str().expect("valid score key").to_owned(),
                    value
                ))
                .collect::<Vec<_>>(),
            [("notes".to_owned(), 12)]
        );
        assert_eq!(generated.address().city().as_bytes(), b"nyc");
        assert_eq!(generated.extras().iter().count(), 0);
        assert_eq!(
            Serialize::serialize(&handwritten).expect("handwritten wire"),
            input
        );
        assert_eq!(
            Serialize::serialize(&generated).expect("generated wire"),
            input
        );
        assert_eq!(V4Serialize::serialize(&v4).expect("v4 wire"), input);
        let touched = touch_handwritten_person(&handwritten);
        assert_eq!(touched, 4282);
        assert_eq!(touch_generated_person(&generated), touched);
        assert_eq!(touch_prost_person(&prost), touched);
        assert_eq!(touch_v4_person(&v4), touched);
    }

    #[test]
    fn person_builders_reproduce_the_shared_input_wires() {
        let _serial = serial_guard();
        let input = person_input_wire();
        assert_eq!(input.len(), 62);
        assert_eq!(
            Serialize::serialize(&build_generated_person()).expect("generated wire"),
            input
        );
        assert_eq!(prost::Message::encode_to_vec(&build_prost_person()), input);
        assert_eq!(
            V4Serialize::serialize(&build_v4_person()).expect("v4 wire"),
            input
        );
        let extras = person_extras_wire(&input);
        assert_eq!(
            Serialize::serialize(&build_generated_person_extras()).expect("extras wire"),
            extras
        );
        assert_eq!(
            prost::Message::encode_to_vec(&build_prost_person_extras()),
            extras
        );
        assert_eq!(
            V4Serialize::serialize(&build_v4_person_extras()).expect("v4 extras wire"),
            extras
        );
    }

    #[test]
    fn holdout_rows_measure_untuned_shapes_with_live_retained_counters() {
        let _serial = serial_guard();
        let rows = run_holdout_rows(Some((10, 3)));
        assert_eq!(
            rows.iter().map(|row| row.name).collect::<Vec<_>>(),
            ["nest_d8", "oneof_err", "rpc_sparse_path", "headers_1"]
        );
        for row in &rows {
            assert!(row.holdout, "{} must be flagged holdout", row.name);
            assert!(row.payload > 0, "{} must carry wire bytes", row.name);
            for value in [
                row.pbrs_enc,
                row.pbrs_fresh_enc,
                row.pbrs_dec,
                row.pbrs_touch,
                row.pbrs_construct,
                row.prost_enc,
                row.prost_first_enc,
                row.prost_dec,
                row.prost_touch,
                row.prost_construct,
                row.v4_enc,
                row.v4_first_enc,
                row.v4_dec,
                row.v4_touch,
                row.v4_construct,
            ] {
                assert!(
                    value.is_finite() && value > 0.0,
                    "{} has an invalid measured time",
                    row.name
                );
            }
        }
        // The counting allocator must be live: prost boxes every nested Node,
        // so decoding depth-8 must retain heap. (v4 is assert-free here: its
        // Arena lives on the C heap, outside the counted window.)
        assert!(rows[0].prost_mem.bytes > 0);
        assert!(rows[0].prost_mem.allocs > 0);
    }

    #[test]
    fn raw_paired_samples_reproduce_row_medians() {
        let _serial = serial_guard();
        let rows = run_holdout_rows(Some((10, 3)));
        let mutations = run_person_mutations(&person_input_wire(), 10, 3);
        let raw = take_raw_rows();
        assert_eq!(raw.len(), 8);
        for (row, raw_row) in rows.iter().zip(raw.iter()) {
            assert_eq!(raw_row.name, row.name);
            assert!(raw_row.holdout);
            assert_eq!(raw_row.cols.len(), 15);
            for (label, values) in &raw_row.cols {
                assert_eq!(values.len(), row.samples, "{label}");
                assert!(
                    values.iter().all(|value| value.is_finite() && *value > 0.0),
                    "{label} must hold measured samples"
                );
            }
            assert_eq!(raw_row.col_median("pbrs_enc"), row.pbrs_enc);
            assert_eq!(raw_row.col_median("pbrs_first_enc"), row.pbrs_fresh_enc);
            assert_eq!(raw_row.col_median("pbrs_dec"), row.pbrs_dec);
            assert_eq!(raw_row.col_median("pbrs_touch"), row.pbrs_touch);
            assert_eq!(raw_row.col_median("pbrs_construct"), row.pbrs_construct);
            assert_eq!(raw_row.col_median("prost_enc"), row.prost_enc);
            assert_eq!(raw_row.col_median("prost_first_enc"), row.prost_first_enc);
            assert_eq!(raw_row.col_median("prost_dec"), row.prost_dec);
            assert_eq!(raw_row.col_median("prost_touch"), row.prost_touch);
            assert_eq!(raw_row.col_median("prost_construct"), row.prost_construct);
            assert_eq!(raw_row.col_median("v4_enc"), row.v4_enc);
            assert_eq!(raw_row.col_median("v4_first_enc"), row.v4_first_enc);
            assert_eq!(raw_row.col_median("v4_dec"), row.v4_dec);
            assert_eq!(raw_row.col_median("v4_touch"), row.v4_touch);
            assert_eq!(raw_row.col_median("v4_construct"), row.v4_construct);
        }
        for (mutation, raw_row) in mutations.iter().zip(raw.iter().skip(4)) {
            assert_eq!(raw_row.name, mutation.name);
            assert_eq!(raw_row.detail, mutation.transition);
            assert_eq!(raw_row.cols.len(), 3);
            assert_eq!(raw_row.col_median("pbrs_mutated_enc"), mutation.pbrs_ns);
            assert_eq!(raw_row.col_median("prost_mutated_enc"), mutation.prost_ns);
            assert_eq!(raw_row.col_median("v4_mutated_enc"), mutation.v4_ns);
        }
        assert!(take_raw_rows().is_empty());
    }

    #[test]
    fn person_layout_rows_use_same_wire_work_and_report_both_measurements() {
        let _serial = serial_guard();
        let input = person_input_wire();
        let rows = run_person_rows(&input, Some((10, 3)));
        let extras = run_person_extras_row(&person_extras_wire(&input), Some((10, 3)));
        assert_eq!(rows[0].name, "person_handwritten");
        assert_eq!(rows[1].name, "person_generated");
        assert_eq!(rows[0].payload, input.len());
        assert_eq!(rows[1].payload, input.len());
        assert_eq!(rows[0].iters, 10);
        assert_eq!(rows[1].iters, 10);
        assert_eq!(rows[0].first_iters, rows[1].first_iters);
        let report = person_report(&rows, &extras);
        assert!(report.contains("| person_handwritten | 62 | 10 | 10 | 3 |"));
        assert!(report.contains("| person_generated | 62 | 10 | 10 | 3 |"));
        assert!(report.contains("| person_generated_extras |"));
        assert!(report.contains("no handwritten comparator"));
        assert!(report.contains("prost/v4 are timed independently"));
        assert!(!report.contains(" | win |"));
        assert!(!report.contains(" | loss |"));

        let mut mismatched = rows;
        mismatched[1].first_iters -= 1;
        assert!(std::panic::catch_unwind(|| person_report(&mismatched, &extras)).is_err());
        let mut invalid = rows;
        invalid[1].v4_touch = f64::NAN;
        assert!(std::panic::catch_unwind(|| person_report(&invalid, &extras)).is_err());
        let mut mislabeled = extras;
        mislabeled.name = "person_handwritten";
        assert!(std::panic::catch_unwind(|| person_report(&rows, &mislabeled)).is_err());
    }

    #[test]
    fn generated_person_extras_compares_only_codecs_with_typed_tag_16() {
        let _serial = serial_guard();
        let base = person_input_wire();
        let wire = person_extras_wire(&base);
        assert!(wire.len() > base.len());
        let generated = pbrs_person::Person::parse(&wire).expect("generated extras");
        let (key, value) = generated.extras().iter().next().expect("typed extras");
        assert_eq!(key.as_view().as_bytes(), b"project");
        assert_eq!(value, 7);
        let prost: ProstPerson = prost::Message::decode(wire.as_slice()).expect("prost extras");
        let v4 = v4_person::Person::parse(&wire).expect("v4 extras");
        let touched = touch_generated_person(&generated);
        assert!(touched > touch_generated_person(&pbrs_person::Person::parse(&base).unwrap()));
        assert_eq!(touched, touch_prost_person(&prost));
        assert_eq!(touched, touch_v4_person(&v4));

        let row = run_person_extras_row(&wire, Some((10, 3)));
        assert_eq!(row.name, "person_generated_extras");
        assert_eq!((row.payload, row.iters, row.samples), (wire.len(), 10, 3));
        assert!(
            person_report(&run_person_rows(&base, Some((10, 3))), &row)
                .contains("| person_generated_extras |")
        );

        let mut wrong = generated;
        wrong.extras_mut().insert("project", 9);
        let wrong_wire = Serialize::serialize(&wrong).expect("wrong typed extras wire");
        assert!(
            std::panic::catch_unwind(|| run_person_extras_row(&wrong_wire, Some((10, 3)))).is_err()
        );
    }

    #[test]
    fn person_mutation_checks_all_codecs_and_rejects_mismatches() {
        let _serial = serial_guard();
        let input = person_input_wire();
        verify_person_mutations(&input);
        let iters = person_mutation_budget(40_000, input.len());
        let footprint = input.len()
            + size_of::<PbrsPerson>()
                .max(size_of::<pbrs_person::Person>())
                .max(size_of::<ProstPerson>())
                .max(size_of::<v4_person::Person>());
        assert!(iters <= 10_000);
        assert!(usize::try_from(iters).expect("bounded count") * footprint <= 32 * 1024 * 1024);

        let rows = run_person_mutations(&input, 10, 3);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].iters, 10);
        assert_eq!(rows[1].iters, 10);
        assert_eq!(rows[2].iters, 10);
        assert_eq!(rows[3].iters, 10);
        assert_eq!(rows[0].payload, input.len());
        assert_eq!(rows[1].payload, input.len());
        assert_eq!(rows[2].payload, input.len());
        assert_eq!(rows[3].payload, input.len());
        let report = mutation_report(&rows);
        assert!(report.contains("| person_handwritten | id 42 <-> 43 | 62 | 10 | 3 |"));
        assert!(report.contains("| person_generated | id 42 <-> 43 | 62 | 10 | 3 |"));
        assert!(report.contains("| person_handwritten | name ada <-> longer | 62 | 10 | 3 |"));
        assert!(report.contains("| person_generated | name ada <-> longer | 62 | 10 | 3 |"));

        let mut expected = PbrsPerson::parse(&input).expect("reference parse");
        expected.set_id(42);
        let expected_wire = Serialize::serialize(&expected).expect("reference wire");
        let mut wrong = PbrsPerson::parse(&input).expect("wrong parse");
        wrong.set_id(43);
        let wrong_wire = Serialize::serialize(&wrong).expect("wrong wire");
        assert!(
            std::panic::catch_unwind(|| {
                assert_person_mutation_output("prost", &wrong_wire, &expected_wire, 42)
            })
            .is_err()
        );
        let mut wrong_generated = pbrs_person::Person::parse(&input).expect("generated input");
        wrong_generated.set_id(42);
        wrong_generated.set_name("different");
        let wrong_wire = Serialize::serialize(&wrong_generated).expect("wrong generated wire");
        assert!(
            std::panic::catch_unwind(|| {
                assert_person_mutation_output("pbrs generated", &wrong_wire, &expected_wire, 42)
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| assert_person_mutation_output("v4", b"\xff", b"\xff", 42))
                .is_err()
        );
    }

    #[test]
    fn person_mutation_covers_non_id_field_for_both_layouts() {
        let _serial = serial_guard();
        let rows = run_person_mutations(&person_input_wire(), 10, 3);
        assert_eq!(rows.len(), 4, "name mutation must have two additional rows");
        let report = mutation_report(&rows);
        assert!(report.contains("| person_handwritten | name "));
        assert!(report.contains("| person_generated | name "));
    }

    #[test]
    fn mutation_report_is_separate_and_fails_closed_on_missing_measurements() {
        let _serial = serial_guard();
        let row = MutationRow {
            name: "person_handwritten",
            transition: "id 42 <-> 43",
            payload: 64,
            iters: 10,
            samples: 3,
            pbrs_ns: 3.5,
            prost_ns: 4.0,
            v4_ns: 9.2,
        };
        let generated = MutationRow {
            name: "person_generated",
            pbrs_ns: 5.3,
            prost_ns: 4.1,
            v4_ns: 8.0,
            ..row
        };
        let name_row = MutationRow {
            transition: "name ada <-> longer",
            pbrs_ns: 6.0,
            prost_ns: 6.5,
            v4_ns: 10.1,
            ..row
        };
        let generated_name_row = MutationRow {
            name: "person_generated",
            pbrs_ns: 7.0,
            prost_ns: 6.8,
            v4_ns: 11.2,
            ..name_row
        };
        let rows = [row, generated, name_row, generated_name_row];
        let report = mutation_report(&rows);
        assert_eq!(
            report.lines().next(),
            Some(
                "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):"
            )
        );
        assert_eq!(
            report.lines().nth(3),
            Some("| person_handwritten | id 42 <-> 43 | 64 | 10 | 3 | 3.5 | 4.0 | 9.2 |")
        );
        assert_eq!(
            report.lines().nth(4),
            Some("| person_generated | id 42 <-> 43 | 64 | 10 | 3 | 5.3 | 4.1 | 8.0 |")
        );
        assert_eq!(
            report.lines().nth(5),
            Some("| person_handwritten | name ada <-> longer | 64 | 10 | 3 | 6.0 | 6.5 | 10.1 |")
        );
        assert_eq!(
            report.lines().nth(6),
            Some("| person_generated | name ada <-> longer | 64 | 10 | 3 | 7.0 | 6.8 | 11.2 |")
        );
        assert!(report.contains("Each row times its own pbrs, prost and v4"));
        assert!(report.contains("Name states are \"ada\" and \"ada lovelace with a longer name\""));
        assert!(!report.contains("Excluded:"));
        assert!(!report.contains("First encode after parse"));
        for value in [0.0, f64::NAN, f64::INFINITY] {
            let mut invalid = rows;
            invalid[3].v4_ns = value;
            assert!(std::panic::catch_unwind(|| mutation_report(&invalid)).is_err());
        }
        let mut unmatched = rows;
        unmatched[3].iters = 9;
        assert!(std::panic::catch_unwind(|| mutation_report(&unmatched)).is_err());
        let mut mislabeled = rows;
        mislabeled[3].transition = "id 42 <-> 43";
        assert!(std::panic::catch_unwind(|| mutation_report(&mislabeled)).is_err());
    }
}
