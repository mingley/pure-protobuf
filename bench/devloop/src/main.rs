//! Dev-loop measurement harness (SB-03): deterministic,
//! iteration-friendly metrics for codec operations and loopback RPC
//! shapes.
//!
//! Each cell runs in a child process (`devloop run-cell <id>`), which
//! reports exact heap allocations/bytes (counting `GlobalAlloc`) and
//! wall time as JSON. The parent optionally wraps the child in
//! `perf stat` (instructions, syscalls), `strace -c` (syscalls), or
//! valgrind/callgrind (instructions), and aggregates repeats into
//! versioned JSON. Instruction counts are differential: the parent runs
//! the same child at N and 2N measured-loop iterations with identical
//! preparation and reports `(count_2n - count_n) / N`, removing process
//! startup and setup from the per-op value. Missing tools yield
//! `not_run` for that metric, never a pass. `--baseline` compares two
//! reports with the win-rule thresholds from the scoreboard.
#![allow(
    clippy::disallowed_methods,
    reason = "devloop is a synchronous CLI harness, not async runtime code"
)]

use serde::{Deserialize, Serialize};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

mod blob;

/// Report schema version. Bump on any breaking JSON change.
const SCHEMA: &str = "devloop/1";

/// Counting allocator: exact per-run heap allocations and bytes.
/// Only the timed phase counts (see [`AllocGuard`]); setup, teardown
/// and JSON printing happen outside the window.
struct CountingAlloc;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOC_ARMED: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ALLOC_ARMED.load(Ordering::Relaxed) == 1 {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        if !out.is_null() && ALLOC_ARMED.load(Ordering::Relaxed) == 1 {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        out
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Arms the counting window; disarms on drop and reports totals.
struct AllocGuard;

impl AllocGuard {
    fn arm() -> Self {
        ALLOC_COUNT.store(0, Ordering::Relaxed);
        ALLOC_BYTES.store(0, Ordering::Relaxed);
        ALLOC_ARMED.store(1, Ordering::Relaxed);
        // The copy window matches the alloc window exactly: warmup runs
        // before arming on every cell.
        pbrs::copy_counts::reset_copy_counts();
        pbrs_grpc::reset_copy_counts();
        AllocGuard
    }

    fn totals(&self) -> (u64, u64) {
        (
            ALLOC_COUNT.load(Ordering::Relaxed),
            ALLOC_BYTES.load(Ordering::Relaxed),
        )
    }
}

impl Drop for AllocGuard {
    fn drop(&mut self) {
        ALLOC_ARMED.store(0, Ordering::Relaxed);
    }
}

/// One metric value: measured, or explicitly not run with a reason.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
enum Metric {
    Measured { value: f64, unit: String },
    NotRun { reason: String },
}

impl Metric {
    fn measured(value: f64, unit: &str) -> Self {
        Metric::Measured {
            value,
            unit: unit.to_owned(),
        }
    }

    fn not_run(reason: impl Into<String>) -> Self {
        Metric::NotRun {
            reason: reason.into(),
        }
    }
}

/// Aggregate of one cell over its repeats.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CellResult {
    id: String,
    kind: String,
    codec: String,
    iters: u64,
    repeats: u32,
    /// Per-operation medians across repeats.
    instructions: Metric,
    /// How the instruction metric was collected. Added without changing
    /// the schema version so older devloop/1 readers can ignore it.
    #[serde(default = "default_instruction_method")]
    instruction_method: String,
    allocs: Metric,
    alloc_bytes: Metric,
    syscalls: Metric,
    /// Blocking lock waits (futex) per op. Uncontended acquisitions are
    /// pure atomics and land in `instructions`; a hot path must never
    /// block. Missing on reports predating the metric.
    #[serde(default = "metric_predates_locks")]
    locks: Metric,
    wall_ns: Metric,
    /// Relative stddev of wall time across repeats (0..1), when known.
    wall_cv: Option<f64>,
    /// Per-op medians of the SB-13 copy counters across repeats.
    /// Missing on reports predating the metric.
    #[serde(default)]
    copy_counts: Option<CopyCountsMed>,
}

/// Per-op medians of one cell's copy counters.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CopyCountsMed {
    wire_calls: f64,
    wire_bytes: f64,
    #[serde(default)]
    emit_calls: f64,
    #[serde(default)]
    emit_bytes: f64,
    carry_calls: f64,
    carry_bytes: f64,
    chunk_slices: f64,
    chunk_slice_bytes: f64,
    encode_calls: f64,
    encode_bytes: f64,
    serialize_calls: f64,
    serialize_bytes: f64,
}

fn copy_medians(counts: &[CopyCountsJson], iters: u64) -> CopyCountsMed {
    macro_rules! med {
        ($field:ident) => {
            median(
                counts
                    .iter()
                    .map(|c| c.$field as f64 / iters as f64)
                    .collect(),
            )
        };
    }
    CopyCountsMed {
        wire_calls: med!(wire_calls),
        wire_bytes: med!(wire_bytes),
        emit_calls: med!(emit_calls),
        emit_bytes: med!(emit_bytes),
        carry_calls: med!(carry_calls),
        carry_bytes: med!(carry_bytes),
        chunk_slices: med!(chunk_slices),
        chunk_slice_bytes: med!(chunk_slice_bytes),
        encode_calls: med!(encode_calls),
        encode_bytes: med!(encode_bytes),
        serialize_calls: med!(serialize_calls),
        serialize_bytes: med!(serialize_bytes),
    }
}

fn metric_predates_locks() -> Metric {
    Metric::not_run("report predates the locks metric")
}

fn default_instruction_method() -> String {
    "whole_process_legacy".to_owned()
}

/// Whole-run report.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Report {
    schema: String,
    host: HostInfo,
    devloop_commit: String,
    cells: Vec<CellResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HostInfo {
    os: String,
    arch: String,
    cpu: String,
    rustc: String,
    perf: bool,
    strace: bool,
    valgrind: bool,
}

/// Child output: one JSON line on stdout.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChildOutput {
    cell: String,
    iters: u64,
    allocs: u64,
    alloc_bytes: u64,
    wall_ns: u64,
    /// Per-site user-space copy counts over the timed window (SB-13).
    /// Missing on outputs predating the metric; all zeros when the
    /// `copy-counts` features are off.
    #[serde(default)]
    copy_counts: Option<CopyCountsJson>,
}

/// Serializable union of the pbrs and pbrs-grpc copy counters.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CopyCountsJson {
    wire_calls: u64,
    wire_bytes: u64,
    #[serde(default)]
    emit_calls: u64,
    #[serde(default)]
    emit_bytes: u64,
    carry_calls: u64,
    carry_bytes: u64,
    chunk_slices: u64,
    chunk_slice_bytes: u64,
    encode_calls: u64,
    encode_bytes: u64,
    serialize_calls: u64,
    serialize_bytes: u64,
    #[serde(default)]
    shared_segments: u64,
    #[serde(default)]
    shared_bytes: u64,
}

fn read_copy_counts() -> CopyCountsJson {
    let rt = pbrs::copy_counts::copy_counts();
    let grpc = pbrs_grpc::copy_counts();
    CopyCountsJson {
        wire_calls: rt.wire_calls,
        wire_bytes: rt.wire_bytes,
        emit_calls: rt.emit_calls,
        emit_bytes: rt.emit_bytes,
        carry_calls: grpc.carry_calls,
        carry_bytes: grpc.carry_bytes,
        chunk_slices: grpc.chunk_slices,
        chunk_slice_bytes: grpc.chunk_slice_bytes,
        encode_calls: grpc.encode_calls,
        encode_bytes: grpc.encode_bytes,
        serialize_calls: grpc.serialize_calls,
        serialize_bytes: grpc.serialize_bytes,
        shared_segments: grpc.shared_segments,
        shared_bytes: grpc.shared_bytes,
    }
}

fn host_info() -> HostInfo {
    HostInfo {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu: cpu_name(),
        rustc: rustc_version(),
        perf: tool_exists("perf"),
        strace: tool_exists("strace"),
        valgrind: tool_exists("valgrind"),
    }
}

fn tool_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn cpu_name() -> String {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|info| {
                info.lines().find_map(|line| {
                    line.strip_prefix("model name")
                        .and_then(|rest| rest.strip_prefix("\t: "))
                        .map(str::to_owned)
                })
            })
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .arg("-n")
            .arg("machdep.cpu.brand_string")
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        "unknown".to_owned()
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn git_commit() -> String {
    if let Ok(commit) = std::env::var("PBRS_DEVLOOP_COMMIT")
        && !commit.trim().is_empty()
    {
        return commit;
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

fn wall_cv(samples: &[f64]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let var = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    Some(var.sqrt() / mean)
}

fn usage() -> String {
    "usage:\n\
     \x20 devloop list\n\
     \x20 devloop run-cell <id> --iters N [--warmup N] [--prepare-iters N]\n\
     \x20 devloop run [--cells a,b] [--iters N] [--repeats N] [--out FILE]\n\
     \x20 devloop compare --baseline FILE [--current FILE] [--rpc] [--budget FILE]\n\
     \x20 devloop sizes\n"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Codec cells: one populated TestAllTypesProto3 specimen per codec.
// The pbrs specimen is built by hand (mirroring bench's tat_populated);
// prost and v4 specimens parse the same wire bytes, which also proves
// wire compatibility. Touch walks a fixed representative field set.

use pbrs::gencode::{NestedMessage, TestAllTypesProto3 as PbrsTat};
use pbrs::prelude::*;
use prost013::Message as _;
use protobuf::Parse as _;

mod pbrs_cases {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs_cases/codec_cases.rs"));
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProstEmpty {}

#[derive(Clone, PartialEq, prost::Message)]
struct ProstId {
    #[prost(int64, tag = "1")]
    id: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProstName {
    #[prost(string, tag = "1")]
    name: String,
}

fn pbrs_specimen() -> PbrsTat {
    let mut nested = NestedMessage::new();
    nested.set_a(9);
    let mut m = PbrsTat::new();
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

fn pbrs_packed_256() -> PbrsTat {
    let mut m = PbrsTat::new();
    for i in 0..256 {
        m.packed_int32_mut().push(i);
    }
    m
}

fn pbrs_unpacked_256() -> PbrsTat {
    let mut m = PbrsTat::new();
    for i in 0..256 {
        m.repeated_int32_mut().push(i);
    }
    m
}

fn pbrs_tags_32() -> PbrsTat {
    let mut m = PbrsTat::new();
    for i in 0..32 {
        m.repeated_string_mut().push(format!("t{i:02}"));
    }
    m
}

fn pbrs_specimen_for_cell(cell: &str) -> PbrsTat {
    match cell {
        "codec.pbrs.packed_256_owned_decode" | "codec.pbrs.packed_256_parse_touch" => {
            pbrs_packed_256()
        }
        "codec.pbrs.unpacked_256_owned_decode" | "codec.pbrs.unpacked_256_parse_touch" => {
            pbrs_unpacked_256()
        }
        "codec.pbrs.tags_32_owned_decode" | "codec.pbrs.tags_32_parse_touch" => pbrs_tags_32(),
        _ => pbrs_specimen(),
    }
}

fn touch_pbrs(m: &PbrsTat) -> u64 {
    let mut acc = m.optional_int32() as u64;
    acc = acc.wrapping_add(m.optional_int64() as u64);
    acc = acc.wrapping_add(m.optional_uint32() as u64);
    acc = acc.wrapping_add(m.optional_string().as_bytes().len() as u64);
    acc = acc.wrapping_add(m.optional_bytes().len() as u64);
    if let Some(n) = m.optional_nested_message_opt() {
        acc = acc.wrapping_add(n.a() as u64);
    }
    for i in m.repeated_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for (k, v) in m.map_int32_int32().iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for s in m.repeated_string().iter() {
        acc = acc.wrapping_add(s.as_view().as_bytes().len() as u64);
    }
    acc
}

fn touch_prost(m: &prost_tat::TestAllTypesProto3) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = &m.optional_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    for i in m.repeated_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for (k, v) in m.map_int32_int32.iter() {
        acc = acc.wrapping_add(*k as u64).wrapping_add(*v as u64);
    }
    for i in m.packed_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in m.repeated_string.iter() {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc
}

fn touch_v4(m: &v4_tat::TestAllTypesProto3) -> u64 {
    touch_v4_view(m.as_view())
}

fn touch_v4_view(m: v4_tat::TestAllTypesProto3View<'_>) -> u64 {
    let mut acc = m.optional_int32() as u64;
    acc = acc.wrapping_add(m.optional_int64() as u64);
    acc = acc.wrapping_add(m.optional_uint32() as u64);
    acc = acc.wrapping_add(m.optional_string().len() as u64);
    acc = acc.wrapping_add(m.optional_bytes().len() as u64);
    if m.has_optional_nested_message() {
        acc = acc.wrapping_add(m.optional_nested_message().a() as u64);
    }
    for i in m.repeated_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for (k, v) in m.map_int32_int32().iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for s in m.repeated_string().iter() {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc
}

struct CodecCase {
    wire: Vec<u8>,
    pbrs_msg: PbrsTat,
    prost_msg: prost_tat::TestAllTypesProto3,
    v4_msg: v4_tat::TestAllTypesProto3,
    pbrs_empty: pbrs_cases::Empty,
    pbrs_id: pbrs_cases::Id,
    pbrs_name80: pbrs_cases::Name,
    prost_empty: ProstEmpty,
    prost_id: ProstId,
    prost_name80: ProstName,
    small_empty_wire: Vec<u8>,
    small_id_wire: Vec<u8>,
    small_name80_wire: Vec<u8>,
    small_scratch: RefCell<Vec<u8>>,
    /// Separately parsed messages for fresh-encode (parsed outside the
    /// timer, each encoded exactly once).
    pbrs_fresh: Vec<PbrsTat>,
    prost_fresh: Vec<prost_tat::TestAllTypesProto3>,
    v4_fresh: Vec<v4_tat::TestAllTypesProto3>,
}

impl CodecCase {
    fn prepare(cell: &str, iters: u64) -> Self {
        let pbrs_msg = pbrs_specimen_for_cell(cell);
        let wire = pbrs::Serialize::serialize(&pbrs_msg).expect("pbrs wire");
        let prost_msg =
            prost_tat::TestAllTypesProto3::decode(wire.as_slice()).expect("prost cross-parse");
        let v4_msg = v4_tat::TestAllTypesProto3::parse(&wire).expect("v4 cross-parse");
        let pbrs_empty = pbrs_cases::Empty::new();
        let prost_empty = ProstEmpty::default();
        let mut pbrs_id = pbrs_cases::Id::new();
        pbrs_id.set_id(7);
        let prost_id = ProstId { id: 7 };
        let mut pbrs_name80 = pbrs_cases::Name::new();
        let name80 = "x".repeat(80);
        pbrs_name80.set_name(name80.as_str());
        let prost_name80 = ProstName { name: name80 };
        let small_empty_wire = pbrs_empty.serialize().expect("empty wire");
        let small_id_wire = pbrs_id.serialize().expect("id wire");
        let small_name80_wire = pbrs_name80.serialize().expect("name80 wire");
        assert_eq!(
            prost::Message::encode_to_vec(&prost_empty),
            small_empty_wire
        );
        assert_eq!(prost::Message::encode_to_vec(&prost_id), small_id_wire);
        assert_eq!(
            prost::Message::encode_to_vec(&prost_name80),
            small_name80_wire
        );
        // Checksums must agree: same observable content on all three.
        let (a, b, c) = (
            touch_pbrs(&PbrsTat::parse(&wire).expect("pbrs cross-parse")),
            touch_prost(&prost_msg),
            touch_v4(&v4_msg),
        );
        assert_eq!((a, b, c), (a, a, a), "touch checksums must agree");
        let n = iters as usize;
        let mut pbrs_fresh = Vec::with_capacity(n);
        let mut prost_fresh = Vec::with_capacity(n);
        let mut v4_fresh = Vec::with_capacity(n);
        for _ in 0..n {
            pbrs_fresh.push(PbrsTat::parse(&wire).expect("fresh pbrs"));
            prost_fresh
                .push(prost_tat::TestAllTypesProto3::decode(wire.as_slice()).expect("fresh prost"));
            v4_fresh.push(v4_tat::TestAllTypesProto3::parse(&wire).expect("fresh v4"));
        }
        CodecCase {
            wire,
            pbrs_msg,
            prost_msg,
            v4_msg,
            pbrs_empty,
            pbrs_id,
            pbrs_name80,
            prost_empty,
            prost_id,
            prost_name80,
            small_empty_wire,
            small_id_wire,
            small_name80_wire,
            small_scratch: RefCell::new(Vec::new()),
            pbrs_fresh,
            prost_fresh,
            v4_fresh,
        }
    }
}

/// Run one codec work unit; returns a sink to defeat DCE.
fn codec_work(cell: &str, case: &CodecCase, i: usize) -> u64 {
    match cell {
        "codec.pbrs.fresh_encode" => black_box(
            pbrs::Serialize::serialize(&case.pbrs_fresh[i])
                .expect("enc")
                .len() as u64,
        ),
        "codec.pbrs.cached_encode" => black_box(
            pbrs::Serialize::serialize(&case.pbrs_msg)
                .expect("enc")
                .len() as u64,
        ),
        "codec.pbrs.owned_decode" => {
            black_box(PbrsTat::parse(&case.wire).expect("dec").optional_int32() as u64)
        }
        "codec.pbrs.parse_touch" => {
            black_box(touch_pbrs(&PbrsTat::parse(&case.wire).expect("dec")))
        }
        "codec.pbrs.packed_256_owned_decode"
        | "codec.pbrs.unpacked_256_owned_decode"
        | "codec.pbrs.tags_32_owned_decode" => {
            black_box(PbrsTat::parse(&case.wire).expect("dec").optional_int32() as u64)
        }
        "codec.pbrs.packed_256_parse_touch"
        | "codec.pbrs.unpacked_256_parse_touch"
        | "codec.pbrs.tags_32_parse_touch" => {
            black_box(touch_pbrs(&PbrsTat::parse(&case.wire).expect("dec")))
        }
        "codec.pbrs.small_empty_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            pbrs::Serialize::encode(&case.pbrs_empty, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.pbrs.small_id_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            pbrs::Serialize::encode(&case.pbrs_id, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.pbrs.small_name80_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            pbrs::Serialize::encode(&case.pbrs_name80, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.pbrs.small_empty_decode" => black_box(
            pbrs_cases::Empty::parse(&case.small_empty_wire)
                .expect("dec")
                .compute_size(),
        ),
        "codec.pbrs.small_id_decode" => black_box(
            pbrs_cases::Id::parse(&case.small_id_wire)
                .expect("dec")
                .id() as u64,
        ),
        "codec.pbrs.small_name80_decode" => black_box(
            pbrs_cases::Name::parse(&case.small_name80_wire)
                .expect("dec")
                .name()
                .as_bytes()
                .len() as u64,
        ),
        "codec.prost.small_empty_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            prost::Message::encode(&case.prost_empty, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.prost.small_id_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            prost::Message::encode(&case.prost_id, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.prost.small_name80_encode" => {
            let mut out = case.small_scratch.borrow_mut();
            out.clear();
            prost::Message::encode(&case.prost_name80, &mut *out).expect("enc");
            black_box(out.len() as u64)
        }
        "codec.prost.small_empty_decode" => black_box(
            <ProstEmpty as prost::Message>::decode(case.small_empty_wire.as_slice())
                .expect("dec")
                .eq(&case.prost_empty) as u64,
        ),
        "codec.prost.small_id_decode" => black_box(
            <ProstId as prost::Message>::decode(case.small_id_wire.as_slice())
                .expect("dec")
                .id as u64,
        ),
        "codec.prost.small_name80_decode" => black_box(
            <ProstName as prost::Message>::decode(case.small_name80_wire.as_slice())
                .expect("dec")
                .name
                .len() as u64,
        ),
        "codec.prost.fresh_encode" => {
            let mut buf = Vec::new();
            prost013::Message::encode(&case.prost_fresh[i], &mut buf).expect("enc");
            black_box(buf.len() as u64)
        }
        "codec.prost.cached_encode" => {
            let mut buf = Vec::new();
            prost013::Message::encode(&case.prost_msg, &mut buf).expect("enc");
            black_box(buf.len() as u64)
        }
        "codec.prost.owned_decode" => black_box(
            prost_tat::TestAllTypesProto3::decode(case.wire.as_slice())
                .expect("dec")
                .optional_int32 as u64,
        ),
        "codec.prost.parse_touch" => black_box(touch_prost(
            &prost_tat::TestAllTypesProto3::decode(case.wire.as_slice()).expect("dec"),
        )),
        "codec.v4.fresh_encode" => black_box(
            protobuf::Serialize::serialize(&case.v4_fresh[i])
                .expect("enc")
                .len() as u64,
        ),
        "codec.v4.cached_encode" => black_box(
            protobuf::Serialize::serialize(&case.v4_msg)
                .expect("enc")
                .len() as u64,
        ),
        "codec.v4.owned_decode" => black_box(
            v4_tat::TestAllTypesProto3::parse(&case.wire)
                .expect("dec")
                .optional_int32() as u64,
        ),
        "codec.v4.parse_touch" => black_box(touch_v4(
            &v4_tat::TestAllTypesProto3::parse(&case.wire).expect("dec"),
        )),
        _ => panic!("unknown codec cell {cell}"),
    }
}

fn codec_cells() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![
        ("codec.pbrs.fresh_encode", "pbrs"),
        ("codec.pbrs.cached_encode", "pbrs"),
        ("codec.pbrs.owned_decode", "pbrs"),
        ("codec.pbrs.parse_touch", "pbrs"),
        ("codec.pbrs.packed_256_owned_decode", "pbrs"),
        ("codec.pbrs.packed_256_parse_touch", "pbrs"),
        ("codec.pbrs.unpacked_256_owned_decode", "pbrs"),
        ("codec.pbrs.unpacked_256_parse_touch", "pbrs"),
        ("codec.pbrs.tags_32_owned_decode", "pbrs"),
        ("codec.pbrs.tags_32_parse_touch", "pbrs"),
        ("codec.pbrs.small_empty_encode", "pbrs"),
        ("codec.pbrs.small_empty_decode", "pbrs"),
        ("codec.pbrs.small_id_encode", "pbrs"),
        ("codec.pbrs.small_id_decode", "pbrs"),
        ("codec.pbrs.small_name80_encode", "pbrs"),
        ("codec.pbrs.small_name80_decode", "pbrs"),
        ("codec.prost.fresh_encode", "prost"),
        ("codec.prost.cached_encode", "prost"),
        ("codec.prost.owned_decode", "prost"),
        ("codec.prost.parse_touch", "prost"),
        ("codec.prost.small_empty_encode", "prost"),
        ("codec.prost.small_empty_decode", "prost"),
        ("codec.prost.small_id_encode", "prost"),
        ("codec.prost.small_id_decode", "prost"),
        ("codec.prost.small_name80_encode", "prost"),
        ("codec.prost.small_name80_decode", "prost"),
        ("codec.v4.fresh_encode", "v4-upb"),
        ("codec.v4.cached_encode", "v4-upb"),
        ("codec.v4.owned_decode", "v4-upb"),
        ("codec.v4.parse_touch", "v4-upb"),
    ];
    out.extend(blob::blob_cells());
    out
}

// ---------------------------------------------------------------------------
// RPC cells: closed-loop loopback (concurrency 1) over 127.0.0.1.
// pbrs-grpc serves the in-tree helloworld Greeter; tonic serves the
// echo.proto service. Codecs differ per stack by construction, so
// cross-stack deltas are NOT fair transport comparisons until SB-01
// qualifies the tonic setup; within-stack repeats are exact.

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, ChannelConfig, Request, Streaming};

struct Echod;

fn hello_req(payload: &[u8]) -> HelloRequest {
    let mut r = HelloRequest::new();
    r.set_name(String::from_utf8_lossy(payload));
    r
}

fn hello_reply(payload: &[u8]) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(String::from_utf8_lossy(payload));
    r
}

impl Greeter for Echod {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<pbrs_grpc::Response<HelloReply>, pbrs_grpc::Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        Ok(pbrs_grpc::Response::new(hello_reply(name.as_bytes())))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<pbrs_grpc::Response<HelloReply>, pbrs_grpc::Status> {
        let mut inbound = request.into_inner();
        let mut last = String::new();
        while let Some(msg) = inbound.message().await? {
            last = msg.name().to_str().unwrap_or("").to_owned();
        }
        Ok(pbrs_grpc::Response::new(hello_reply(last.as_bytes())))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<pbrs_grpc::Response<Streaming<HelloReply>>, pbrs_grpc::Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(hello_reply(part.as_bytes())).await.is_err() {
                    break;
                }
            }
        }));
        Ok(pbrs_grpc::Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<pbrs_grpc::Response<Streaming<HelloReply>>, pbrs_grpc::Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                let name = msg.name().to_str().unwrap_or("").to_owned();
                if tx.send(hello_reply(name.as_bytes())).await.is_err() {
                    break;
                }
            }
        }));
        Ok(pbrs_grpc::Response::new(stream))
    }
}

pub mod echo {
    tonic::include_proto!("devloop");
}

use echo::echo_server::{Echo as TonicEcho, EchoServer};
use echo::{EchoReply as TonicReply, EchoRequest as TonicRequest};

struct TonicEchod;

#[tonic::async_trait]
impl TonicEcho for TonicEchod {
    async fn unary(
        &self,
        request: tonic::Request<TonicRequest>,
    ) -> Result<tonic::Response<TonicReply>, tonic::Status> {
        let payload = request.into_inner().payload;
        Ok(tonic::Response::new(TonicReply { payload }))
    }

    type ServerStreamStream =
        tokio_stream::wrappers::ReceiverStream<Result<TonicReply, tonic::Status>>;

    async fn server_stream(
        &self,
        request: tonic::Request<TonicRequest>,
    ) -> Result<tonic::Response<Self::ServerStreamStream>, tonic::Status> {
        let req = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        drop(tokio::spawn(async move {
            for _ in 0..req.replies.max(1) {
                if tx
                    .send(Ok(TonicReply {
                        payload: req.payload.clone(),
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(tonic::Response::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
    }
}

fn rpc_cells() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rpc.pbrs.unary", "pbrs-grpc"),
        ("rpc.pbrs.server_stream", "pbrs-grpc"),
        ("rpc.pbrs.unary_compressed", "pbrs-grpc"),
        ("rpc.pbrs.server_stream_compressed", "pbrs-grpc"),
        ("rpc.tonic.unary", "tonic"),
        ("rpc.tonic.server_stream", "tonic"),
    ]
}

/// 1 KiB ASCII payload; valid UTF-8 so both codecs carry it as a string.
fn rpc_payload() -> Vec<u8> {
    vec![b'x'; 1024]
}

async fn rpc_pbrs_unary(iters: u64, payload: &[u8]) -> u64 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        GreeterServer::new(Echod)
            .serve_listener(listener)
            .await
            .ok();
    }));
    let client = GreeterClient::connect(addr).await.expect("connect");
    // Warmup outside the window: one call to settle the connection.
    client
        .say_hello(Request::new(hello_req(payload)))
        .await
        .expect("warmup");
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let resp = client
            .say_hello(Request::new(hello_req(payload)))
            .await
            .expect("unary");
        sink = sink.wrapping_add(resp.into_inner().message().to_str().unwrap_or("").len() as u64);
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("rpc.pbrs.unary", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

async fn rpc_pbrs_server_stream(iters: u64, payload: &[u8]) -> u64 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        GreeterServer::new(Echod)
            .serve_listener(listener)
            .await
            .ok();
    }));
    let client = GreeterClient::connect(addr).await.expect("connect");
    // Four comma-separated chunks -> four replies per RPC.
    let chunk = String::from_utf8_lossy(&payload[..256]).into_owned();
    let name = format!("{chunk},{chunk},{chunk},{chunk}");
    let resp = client
        .server_hello(Request::new(hello_req(name.as_bytes())))
        .await
        .expect("warmup headers");
    let mut inbound = resp.into_inner();
    while inbound.message().await.expect("warmup msg").is_some() {}
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let resp = client
            .server_hello(Request::new(hello_req(name.as_bytes())))
            .await
            .expect("headers");
        let mut inbound = resp.into_inner();
        while let Some(msg) = inbound.message().await.expect("msg") {
            sink = sink.wrapping_add(msg.message().to_str().unwrap_or("").len() as u64);
        }
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("rpc.pbrs.server_stream", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

/// 32 KiB mixed payload: compressible text plus deterministic
/// incompressible bytes, so the compressed cells exercise the codec at a
/// realistic ratio instead of memcpy-of-zeros or pure entropy.
fn rpc_compressed_payload() -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * 1024);
    let mut x: u64 = 0x243f_6a88_85a3_08d3;
    while out.len() < 32 * 1024 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if out.len() % 64 < 32 {
            out.extend_from_slice(b"the quick brown fox jumps over ");
        } else {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    out.truncate(32 * 1024);
    out
}

async fn rpc_pbrs_unary_compressed(iters: u64) -> u64 {
    let payload = rpc_compressed_payload();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        GreeterServer::new(Echod)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    }));
    let channel = Channel::connect_with(addr, ChannelConfig::new().send_compressed(true))
        .await
        .expect("connect");
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(hello_req(&payload)))
        .await
        .expect("warmup");
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let resp = client
            .say_hello(Request::new(hello_req(&payload)))
            .await
            .expect("unary");
        sink = sink.wrapping_add(resp.into_inner().message().to_str().unwrap_or("").len() as u64);
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("rpc.pbrs.unary_compressed", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

async fn rpc_pbrs_server_stream_compressed(iters: u64) -> u64 {
    let payload = rpc_compressed_payload();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        GreeterServer::new(Echod)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    }));
    let channel = Channel::connect_with(addr, ChannelConfig::new().send_compressed(true))
        .await
        .expect("connect");
    let client = GreeterClient::new(channel);
    // Four comma-separated chunks -> four replies per RPC.
    let chunk = String::from_utf8_lossy(&payload[..8192]).into_owned();
    let name = format!("{chunk},{chunk},{chunk},{chunk}");
    let resp = client
        .server_hello(Request::new(hello_req(name.as_bytes())))
        .await
        .expect("warmup headers");
    let mut inbound = resp.into_inner();
    while inbound.message().await.expect("warmup msg").is_some() {}
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let resp = client
            .server_hello(Request::new(hello_req(name.as_bytes())))
            .await
            .expect("headers");
        let mut inbound = resp.into_inner();
        while let Some(msg) = inbound.message().await.expect("msg") {
            sink = sink.wrapping_add(msg.message().to_str().unwrap_or("").len() as u64);
        }
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json(
            "rpc.pbrs.server_stream_compressed",
            iters,
            allocs,
            bytes,
            wall
        )
    );
    black_box(sink)
}

fn lb_cells() -> Vec<(&'static str, &'static str)> {
    // One cell per shipped policy (CH-11); each follows
    // `lb_pick_first_pick`. pick_first single-ready is the
    // passthrough-equivalent hot path; the rest share a three-address
    // ready steady state, wrappers over a round_robin child.
    vec![
        ("lb.pick_first.pick", "pbrs-grpc"),
        ("lb.round_robin.pick", "pbrs-grpc"),
        ("lb.weighted_round_robin.pick", "pbrs-grpc"),
        ("lb.ring_hash.pick", "pbrs-grpc"),
        ("lb.least_request.pick", "pbrs-grpc"),
        ("lb.priority.pick", "pbrs-grpc"),
        ("lb.outlier_detection.pick", "pbrs-grpc"),
        ("lb.random_subsetting_experimental.pick", "pbrs-grpc"),
    ]
}

/// Shared three-address ready steady state for the multi-endpoint
/// picker cells.
fn lb_ready_addrs() -> Vec<pbrs_grpc::resolver::ResolvedAddress> {
    use pbrs_grpc::resolver::ResolvedAddress;
    (0..3)
        .map(|i| ResolvedAddress::Tcp(format!("127.0.0.1:{}", 50_051 + i).parse().expect("addr")))
        .collect()
}

/// Steady-state pick cost for one policy (CH-10 harness).
///
/// The policy is preloaded to its ready steady state outside the guard;
/// the loop measures one `pick()` per op: allocations (must be zero),
/// instructions, and blocking lock waits. A `current_thread` runtime
/// keeps worker-park futexes out of the locks metric. CH-11 adds the
/// remaining policies by constructing theirs here.
async fn lb_pick_first_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::{Pick, PickFirst};
    use pbrs_grpc::resolver::ResolvedAddress;

    let policy = PickFirst::from_config(None);
    policy
        .update(vec![ResolvedAddress::Tcp(
            "127.0.0.1:50051".parse().expect("addr"),
        )])
        .await;
    for _ in 0..2000 {
        assert!(
            matches!(policy.pick().await, Pick::Use(_)),
            "steady pick must stay Use"
        );
    }
    // Warmup absorbs tokio's lazy runtime growth (the semaphore defer
    // queue reaches steady capacity), so the guarded loop measures the
    // pick alone: it must allocate nothing.
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        match policy.pick().await {
            Pick::Use(_) => sink = sink.wrapping_add(1),
            _ => panic!("steady pick must stay Use"),
        }
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("lb.pick_first.pick", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

/// Guarded steady-state pick loop shared by the multi-endpoint
/// cells: `pick` runs once per op and must return `Pick::Use`
/// without allocating. The hash lane feeds ring hashing a varying
/// request hash (the wrapping op counter) so the loop cannot
/// over-fit one ring position.
macro_rules! lb_guarded_loop {
    ($cell:literal, $iters:expr, $pick:expr) => {{
        let iters: u64 = $iters;
        for _ in 0..2000 {
            assert!(
                matches!($pick, pbrs_grpc::lb::Pick::Use(_)),
                "steady pick must stay Use"
            );
        }
        // Warmup absorbs lazy growth (runtime, scheduler builds,
        // child creation) so the guarded loop measures the pick
        // alone: it must allocate nothing.
        let guard = AllocGuard::arm();
        let start = Instant::now();
        let mut sink = 0u64;
        for _ in 0..iters {
            match $pick {
                pbrs_grpc::lb::Pick::Use(_) => sink = sink.wrapping_add(1),
                _ => panic!("steady pick must stay Use"),
            }
        }
        let wall = start.elapsed();
        let (allocs, bytes) = guard.totals();
        drop(guard);
        eprintln!(
            "__CHILD__ {}",
            child_json($cell, iters, allocs, bytes, wall)
        );
        black_box(sink)
    }};
}

async fn lb_round_robin_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::RoundRobin;
    let policy = RoundRobin::new();
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!("lb.round_robin.pick", iters, policy.pick().await)
}

async fn lb_weighted_round_robin_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::WeightedRoundRobin;
    use pbrs_grpc::service_config::WeightedRoundRobinConfig;
    // Hourly weight recompute: the scheduler builds once in warmup
    // and the guarded loop measures steady EDF picks between
    // recomputes (the cadence is deployment-tunable; default 1s).
    let policy = WeightedRoundRobin::with_config(WeightedRoundRobinConfig {
        weight_update_period: Duration::from_secs(3600),
        ..Default::default()
    });
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!("lb.weighted_round_robin.pick", iters, policy.pick().await)
}

async fn lb_ring_hash_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::RingHash;
    let policy = RingHash::from_config(None);
    policy.update(lb_ready_addrs()).await;
    let cell = "lb.ring_hash.pick";
    for _ in 0..2000 {
        assert!(
            matches!(
                policy.pick_hash(Some(0x9E37_79B9)).await,
                pbrs_grpc::lb::Pick::Use(_)
            ),
            "steady pick must stay Use"
        );
    }
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        // Varying request hash, as real RPCs hash distinct metadata.
        match policy
            .pick_hash(Some(sink.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
            .await
        {
            pbrs_grpc::lb::Pick::Use(_) => sink = sink.wrapping_add(1),
            _ => panic!("steady pick must stay Use"),
        }
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!("__CHILD__ {}", child_json(cell, iters, allocs, bytes, wall));
    black_box(sink)
}

async fn lb_least_request_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::LeastRequest;
    let policy = LeastRequest::from_config(None);
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!("lb.least_request.pick", iters, policy.pick().await)
}

async fn lb_priority_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::{Priority, ensure_default_policies_registered};
    use pbrs_grpc::service_config::{LbPolicyConfig, PriorityChildConfig, PriorityConfig};
    ensure_default_policies_registered();
    let policy = Priority::with_config(&PriorityConfig {
        children: [(
            "p0".to_owned(),
            PriorityChildConfig {
                config: vec![LbPolicyConfig::RoundRobin],
                ignore_reresolution_requests: false,
            },
        )]
        .into_iter()
        .collect(),
        priorities: vec!["p0".to_owned()],
    })
    .expect("priority config");
    // Flat update: all addresses feed the single priority.
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!("lb.priority.pick", iters, policy.pick().await)
}

async fn lb_outlier_detection_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::{OutlierDetection, ensure_default_policies_registered};
    use pbrs_grpc::service_config::{LbPolicyConfig, OutlierDetectionConfig};
    ensure_default_policies_registered();
    let policy = OutlierDetection::with_config(&OutlierDetectionConfig {
        interval: Duration::from_secs(3600),
        child_policy: vec![LbPolicyConfig::RoundRobin],
        ..Default::default()
    })
    .expect("outlier config");
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!("lb.outlier_detection.pick", iters, policy.pick().await)
}

async fn lb_random_subsetting_pick(iters: u64) -> u64 {
    use pbrs_grpc::lb::{RandomSubsetting, ensure_default_policies_registered};
    use pbrs_grpc::service_config::{LbPolicyConfig, RandomSubsettingConfig};
    ensure_default_policies_registered();
    let policy = RandomSubsetting::with_config(&RandomSubsettingConfig {
        subset_size: 2,
        child_policy: vec![LbPolicyConfig::RoundRobin],
    })
    .expect("subset config");
    policy.update(lb_ready_addrs()).await;
    lb_guarded_loop!(
        "lb.random_subsetting_experimental.pick",
        iters,
        policy.pick().await
    )
}

async fn rpc_tonic_unary(iters: u64, payload: &[u8]) -> u64 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(EchoServer::new(TonicEchod))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    }));
    let mut client = echo::echo_client::EchoClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");
    client
        .unary(TonicRequest {
            payload: payload.to_vec(),
            replies: 0,
        })
        .await
        .expect("warmup");
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let resp = client
            .unary(TonicRequest {
                payload: payload.to_vec(),
                replies: 0,
            })
            .await
            .expect("unary");
        sink = sink.wrapping_add(resp.into_inner().payload.len() as u64);
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("rpc.tonic.unary", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

async fn rpc_tonic_server_stream(iters: u64, payload: &[u8]) -> u64 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(EchoServer::new(TonicEchod))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    }));
    let mut client = echo::echo_client::EchoClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");
    let mut warmup = client
        .server_stream(TonicRequest {
            payload: payload.to_vec(),
            replies: 4,
        })
        .await
        .expect("warmup headers")
        .into_inner();
    while warmup.message().await.expect("warmup msg").is_some() {}
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        let mut stream = client
            .server_stream(TonicRequest {
                payload: payload.to_vec(),
                replies: 4,
            })
            .await
            .expect("headers")
            .into_inner();
        while let Some(msg) = stream.message().await.expect("msg") {
            sink = sink.wrapping_add(msg.payload.len() as u64);
        }
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json("rpc.tonic.server_stream", iters, allocs, bytes, wall)
    );
    black_box(sink)
}

// ---------------------------------------------------------------------------
// Child protocol: `run-cell` prints exactly one `__CHILD__ <json>` line
// on stderr (stdout stays clean for tool wrappers that merge streams).

fn child_json(cell: &str, iters: u64, allocs: u64, bytes: u64, wall: Duration) -> String {
    serde_json::to_string(&ChildOutput {
        cell: cell.to_owned(),
        iters,
        allocs,
        alloc_bytes: bytes,
        wall_ns: wall.as_nanos() as u64,
        copy_counts: Some(read_copy_counts()),
    })
    .expect("child json")
}

fn run_codec_cell(cell: &str, iters: u64, warmup: u64, prepare_iters: u64) {
    let case = CodecCase::prepare(cell, prepare_iters.max(iters));
    let n = iters as usize;
    for i in 0..warmup as usize {
        black_box(codec_work(cell, &case, i % n.max(1)));
    }
    // Fresh-encode cells consume one message per iter; decode cells reuse
    // the wire bytes. All allocation happens inside the window.
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for i in 0..n {
        sink = sink.wrapping_add(codec_work(cell, &case, i));
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!("__CHILD__ {}", child_json(cell, iters, allocs, bytes, wall));
    black_box(sink);
}

fn run_blob_cell(cell: &str, iters: u64, warmup: u64) {
    let size = blob::blob_size_of(cell).expect("blob size");
    let case = blob::BlobCase::prepare(size);
    let n = iters as usize;
    for i in 0..warmup.min(iters) as usize {
        black_box(blob::blob_work(cell, &case, i % n.max(1)));
    }
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for i in 0..n {
        sink = sink.wrapping_add(blob::blob_work(cell, &case, i));
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!("__CHILD__ {}", child_json(cell, iters, allocs, bytes, wall));
    black_box(sink);
}

fn all_cells() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for (id, codec) in codec_cells() {
        out.push((id, "codec", codec));
    }
    for (id, codec) in rpc_cells() {
        out.push((id, "rpc", codec));
    }
    for (id, codec) in lb_cells() {
        out.push((id, "lb", codec));
    }
    out
}

// ---------------------------------------------------------------------------
// Tool probes. Each returns a prefix argv to wrap the child, or None.
// Instructions: perf first, valgrind/callgrind second, else not_run.
// Syscalls: strace -c on Linux, else not_run.

struct Tools {
    perf: bool,
    strace: bool,
    valgrind: bool,
}

struct InstructionTool {
    wrapper: Vec<String>,
    method: &'static str,
}

struct InstructionTotal {
    total: f64,
    method: &'static str,
}

fn probe_tools() -> Tools {
    Tools {
        perf: tool_exists("perf"),
        strace: tool_exists("strace"),
        valgrind: tool_exists("valgrind"),
    }
}

fn instruction_tool(tools: &Tools) -> Option<InstructionTool> {
    if tools.perf {
        Some(InstructionTool {
            wrapper: vec![
                "perf".to_owned(),
                "stat".to_owned(),
                "-x,".to_owned(),
                "-e".to_owned(),
                "instructions".to_owned(),
                "--".to_owned(),
            ],
            method: "differential_perf_2n_minus_n",
        })
    } else if tools.valgrind {
        Some(InstructionTool {
            wrapper: vec![
                "valgrind".to_owned(),
                "--tool=callgrind".to_owned(),
                "--cache-sim=no".to_owned(),
                "--callgrind-out-file=/tmp/devloop-callgrind.%p".to_owned(),
            ],
            method: "differential_callgrind_2n_minus_n",
        })
    } else {
        None
    }
}

fn run_cell_process(
    exe: &std::path::Path,
    cell: &str,
    iters: u64,
    prepare_iters: u64,
    wrapper: &[String],
) -> (ChildOutput, String) {
    let mut cmd = std::process::Command::new(if wrapper.is_empty() {
        exe.as_os_str().to_owned()
    } else {
        wrapper[0].clone().into()
    });
    if !wrapper.is_empty() {
        cmd.args(&wrapper[1..]);
        cmd.arg(exe);
    }
    cmd.arg("run-cell")
        .arg(cell)
        .arg("--iters")
        .arg(iters.to_string())
        .arg("--prepare-iters")
        .arg(prepare_iters.to_string());
    let out = cmd.output().expect("spawn child");
    assert!(
        out.status.success(),
        "cell {cell} child failed: {}",
        String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(500)
            .collect::<String>()
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let child_line = stderr
        .lines()
        .find_map(|line| line.strip_prefix("__CHILD__ "))
        .unwrap_or_else(|| panic!("cell {cell} printed no __CHILD__ line"));
    let child: ChildOutput = serde_json::from_str(child_line).expect("child json parses");
    (child, stderr)
}

/// Run the child for one repeat, optionally under a wrapper. Returns
/// the child output plus optional (instructions, syscalls).
fn run_child(
    exe: &std::path::Path,
    cell: &str,
    iters: u64,
    tools: &Tools,
) -> (
    ChildOutput,
    Option<InstructionTotal>,
    Option<f64>,
    Option<f64>,
) {
    let tool = instruction_tool(tools);
    let prepare_iters = if tool.is_some() {
        iters.checked_mul(2).expect("differential iters overflow")
    } else {
        iters
    };
    let wrapper: &[String] = tool.as_ref().map_or(&[], |t| t.wrapper.as_slice());
    let (child, stderr) = run_cell_process(exe, cell, iters, prepare_iters, wrapper);
    let first = parse_perf_instructions(&stderr).or_else(|| parse_callgrind_instructions(&stderr));
    let instructions = if let (Some(tool), Some(first)) = (tool.as_ref(), first) {
        let double_iters = iters.checked_mul(2).expect("differential iters overflow");
        let (double_child, double_stderr) =
            run_cell_process(exe, cell, double_iters, prepare_iters, &tool.wrapper);
        assert_eq!(double_child.iters, double_iters);
        parse_perf_instructions(&double_stderr)
            .or_else(|| parse_callgrind_instructions(&double_stderr))
            .map(|second| {
                assert!(
                    second >= first,
                    "cell {cell} differential instructions underflow: first={first} second={second}"
                );
                InstructionTotal {
                    total: second - first,
                    method: tool.method,
                }
            })
    } else {
        first.map(|total| InstructionTotal {
            total,
            method: "whole_process_legacy",
        })
    };
    // Syscalls need a second run under strace (perf stat -e syscalls
    // counts entry+exit pairs inconsistently across kernels). The locks
    // metric is parsed from the same output: no extra run.
    let (syscalls, locks) = if tools.strace {
        let sout = std::process::Command::new("strace")
            .arg("-c")
            .arg("-f")
            .arg(exe)
            .arg("run-cell")
            .arg(cell)
            .arg("--iters")
            .arg(iters.to_string())
            .output()
            .expect("spawn strace");
        if sout.status.success() {
            let text = String::from_utf8_lossy(&sout.stderr);
            (parse_strace_total(&text), Some(parse_strace_futex(&text)))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };
    (child, instructions, syscalls, locks)
}

/// Parse `perf stat -x,` output: `<count>,instructions,...`.
fn parse_perf_instructions(stderr: &str) -> Option<f64> {
    stderr.lines().find_map(|line| {
        let mut parts = line.split(',');
        let count = parts.next()?.trim().replace(' ', "");
        let event = parts.next()?.trim();
        if event == "instructions" {
            count.parse::<f64>().ok()
        } else {
            None
        }
    })
}

/// Parse callgrind's summary line (`-bbi` off): `events: Ir ...` plus
/// the `summary:` line callgrind prints with --quiet... valgrind's
/// callgrind prints `I refs:` in its final summary; use that.
fn parse_callgrind_instructions(stderr: &str) -> Option<f64> {
    stderr.lines().find_map(|line| {
        let line = line.trim();
        line.split_once("I   refs:").and_then(|(_, rest)| {
            rest.split_whitespace()
                .next()
                .and_then(|n| n.replace(',', "").parse::<f64>().ok())
        })
    })
}

/// Parse `strace -c` totals: the `total` line's first column is the
/// call count... actually `% time seconds usecs/call calls ...`;
/// use the `total` row's `calls` field (4th column).
fn parse_strace_total(stderr: &str) -> Option<f64> {
    stderr.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("total").and_then(|rest| {
            rest.split_whitespace()
                .nth(3)
                .and_then(|n| n.parse::<f64>().ok())
        })
    })
}

/// Parse blocking lock waits from `strace -c`: the `calls` column of the
/// `futex` row plus `futex_waitv` (newer kernels split the wait family).
/// No such row means no waits: strace has known `futex` since 2003, so a
/// missing row is a zero, not a skip.
fn parse_strace_futex(stderr: &str) -> f64 {
    let mut total = 0.0;
    for line in stderr.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next_back() else {
            continue;
        };
        if name == "futex" || name == "futex_waitv" {
            let mut cols = line.split_whitespace();
            // %time seconds usecs/call calls errors syscall
            if let Some(calls) = cols.nth(3).and_then(|n| n.parse::<f64>().ok()) {
                total += calls;
            }
        }
    }
    total
}

fn run_matrix(cells: &[&str], iters: u64, repeats: u32) -> Report {
    let exe = std::env::current_exe().expect("current exe");
    let tools = probe_tools();
    let registry: std::collections::HashMap<_, _> = all_cells()
        .into_iter()
        .map(|(id, kind, codec)| (id, (kind, codec)))
        .collect();
    let mut out = Vec::new();
    for cell in cells {
        let (kind, codec) = registry
            .get(cell)
            .unwrap_or_else(|| panic!("unknown cell {cell}"));
        let cell_iters = if kind == &"rpc" {
            iters.min(200)
        } else if blob::is_blob_cell(cell) {
            match blob::blob_size_of(cell) {
                Some(size) => size.matrix_iters(iters),
                None => iters,
            }
        } else {
            iters
        };
        let mut allocs = Vec::new();
        let mut alloc_bytes = Vec::new();
        let mut walls = Vec::new();
        let mut instrs = Vec::new();
        let mut syscalls = Vec::new();
        let mut locks = Vec::new();
        let mut copies = Vec::new();
        let mut instruction_method: Option<&'static str> = None;
        for _ in 0..repeats {
            let (child, instr, sys, futex) = run_child(&exe, cell, cell_iters, &tools);
            assert_eq!(child.iters, cell_iters);
            allocs.push(child.allocs as f64 / cell_iters as f64);
            alloc_bytes.push(child.alloc_bytes as f64 / cell_iters as f64);
            walls.push(child.wall_ns as f64 / cell_iters as f64);
            if let Some(c) = child.copy_counts {
                copies.push(c);
            }
            if let Some(v) = instr {
                instruction_method = Some(v.method);
                instrs.push(v.total / cell_iters as f64);
            }
            if let Some(v) = sys {
                syscalls.push(v / cell_iters as f64);
            }
            if let Some(v) = futex {
                locks.push(v / cell_iters as f64);
            }
        }
        // Codec allocation counts must be bit-exact across repeats;
        // anything else is nondeterminism in the cell, not noise. LB
        // picks are pure in-memory decisions with the same bar. RPC
        // cells legitimately jitter by ~1 allocation (ephemeral port
        // digits, hash seeds), so they report the median instead.
        let alloc_exact = allocs.windows(2).all(|w| w[0] == w[1]);
        let bytes_exact = alloc_bytes.windows(2).all(|w| w[0] == w[1]);
        if kind == &"codec" || kind == &"lb" {
            assert!(
                alloc_exact && bytes_exact,
                "cell {cell} allocations vary across repeats: {allocs:?} / {alloc_bytes:?}"
            );
        } else if !(alloc_exact && bytes_exact) {
            eprintln!("note: cell {cell} allocs vary across repeats, using median");
        }
        let (alloc_metric, bytes_metric) = if kind == &"codec" || kind == &"lb" {
            (
                Metric::measured(allocs[0], "heap allocs per op (exact)"),
                Metric::measured(alloc_bytes[0], "heap bytes per op (exact)"),
            )
        } else {
            (
                Metric::measured(median(allocs), "heap allocs per RPC (median)"),
                Metric::measured(median(alloc_bytes), "heap bytes per RPC (median)"),
            )
        };
        out.push(CellResult {
            id: cell.to_string(),
            kind: kind.to_string(),
            codec: codec.to_string(),
            iters: cell_iters,
            repeats,
            instructions: if instrs.is_empty() {
                Metric::not_run("no perf or valgrind on PATH")
            } else {
                Metric::measured(median(instrs), "retired/callgrind-ir per op")
            },
            instruction_method: instruction_method.unwrap_or("not_run").to_owned(),
            allocs: alloc_metric,
            alloc_bytes: bytes_metric,
            syscalls: if syscalls.is_empty() {
                Metric::not_run("no strace on PATH")
            } else {
                Metric::measured(median(syscalls), "syscalls per op")
            },
            locks: if locks.is_empty() {
                Metric::not_run("no strace on PATH")
            } else {
                Metric::measured(median(locks), "blocking lock waits per op (futex)")
            },
            wall_ns: Metric::measured(median(walls.clone()), "ns per op (secondary)"),
            wall_cv: wall_cv(&walls),
            copy_counts: if copies.is_empty() {
                None
            } else {
                Some(copy_medians(&copies, cell_iters))
            },
        });
    }
    Report {
        schema: SCHEMA.to_owned(),
        host: host_info(),
        devloop_commit: git_commit(),
        cells: out,
    }
}

// ---------------------------------------------------------------------------
// Baseline comparison with scoreboard thresholds: instructions or
// allocations fall ≥2% on targeted cells; no primary cell regresses
// >1% (2% for RPC cells). Exits nonzero on regression; not_run never
// passes or fails, it just skips.

fn metric_value(m: &Metric) -> Option<f64> {
    match m {
        Metric::Measured { value, .. } => Some(*value),
        Metric::NotRun { .. } => None,
    }
}

fn compare_reports(baseline: &Report, current: &Report, rpc: bool) -> bool {
    assert_eq!(baseline.schema, current.schema, "schema mismatch");
    let base: std::collections::HashMap<_, _> =
        baseline.cells.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut ok = true;
    for cell in &current.cells {
        let Some(old) = base.get(cell.id.as_str()) else {
            println!("{}: new cell, no baseline", cell.id);
            continue;
        };
        for (name, o, n, lower_better) in [
            ("instructions", &old.instructions, &cell.instructions, true),
            ("allocs", &old.allocs, &cell.allocs, true),
            ("alloc_bytes", &old.alloc_bytes, &cell.alloc_bytes, true),
            ("syscalls", &old.syscalls, &cell.syscalls, true),
            ("locks", &old.locks, &cell.locks, true),
        ] {
            let (Some(o), Some(n)) = (metric_value(o), metric_value(n)) else {
                continue;
            };
            if o == 0.0 {
                continue;
            }
            let delta = (n - o) / o;
            let limit = if rpc || cell.kind == "rpc" {
                0.02
            } else {
                0.01
            };
            let improved = if lower_better {
                delta < 0.0
            } else {
                delta > 0.0
            };
            let regressed = if lower_better {
                delta > limit
            } else {
                delta < -limit
            };
            if regressed {
                println!(
                    "{} {name}: REGRESSED {o:.1} -> {n:.1} ({:+.2}%, limit {:.0}%)",
                    cell.id,
                    delta * 100.0,
                    limit * 100.0
                );
                ok = false;
            } else if improved && delta.abs() >= 0.02 {
                println!(
                    "{} {name}: improved {o:.1} -> {n:.1} ({:+.2}%)",
                    cell.id,
                    delta * 100.0
                );
            }
        }
    }
    ok
}

fn cmd_list() {
    for (id, kind, codec) in all_cells() {
        println!("{id} {kind} {codec}");
    }
}

fn cmd_run_cell(args: &[String]) {
    let mut id: Option<String> = None;
    let mut iters = 1000u64;
    let mut prepare_iters: Option<u64> = None;
    let mut warmup = 100u64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--iters" => {
                iters = args[i + 1].parse().expect("--iters N");
                i += 2;
            }
            "--warmup" => {
                warmup = args[i + 1].parse().expect("--warmup N");
                i += 2;
            }
            "--prepare-iters" => {
                prepare_iters = Some(args[i + 1].parse().expect("--prepare-iters N"));
                i += 2;
            }
            other if id.is_none() => {
                id = Some(other.to_owned());
                i += 1;
            }
            other => panic!("unexpected arg {other}"),
        }
    }
    let id = id.expect("run-cell <id>");
    if blob::is_blob_cell(&id) {
        run_blob_cell(&id, iters, warmup);
    } else if id.starts_with("codec.") {
        run_codec_cell(&id, iters, warmup, prepare_iters.unwrap_or(iters));
    } else if id.starts_with("lb.") {
        // Single-threaded: worker parking would pollute the locks metric.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            match id.as_str() {
                "lb.pick_first.pick" => lb_pick_first_pick(iters).await,
                "lb.round_robin.pick" => lb_round_robin_pick(iters).await,
                "lb.weighted_round_robin.pick" => lb_weighted_round_robin_pick(iters).await,
                "lb.ring_hash.pick" => lb_ring_hash_pick(iters).await,
                "lb.least_request.pick" => lb_least_request_pick(iters).await,
                "lb.priority.pick" => lb_priority_pick(iters).await,
                "lb.outlier_detection.pick" => lb_outlier_detection_pick(iters).await,
                "lb.random_subsetting_experimental.pick" => lb_random_subsetting_pick(iters).await,
                _ => panic!("unknown lb cell {id}"),
            }
        });
    } else if id.starts_with("rpc.") {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let payload = rpc_payload();
        rt.block_on(async {
            match id.as_str() {
                "rpc.pbrs.unary" => rpc_pbrs_unary(iters, &payload).await,
                "rpc.pbrs.server_stream" => rpc_pbrs_server_stream(iters, &payload).await,
                "rpc.pbrs.unary_compressed" => rpc_pbrs_unary_compressed(iters).await,
                "rpc.pbrs.server_stream_compressed" => {
                    rpc_pbrs_server_stream_compressed(iters).await
                }
                "rpc.tonic.unary" => rpc_tonic_unary(iters, &payload).await,
                "rpc.tonic.server_stream" => rpc_tonic_server_stream(iters, &payload).await,
                _ => panic!("unknown rpc cell {id}"),
            }
        });
    } else {
        panic!("unknown cell {id}");
    }
}

fn cmd_run(args: &[String]) {
    let mut cells: Option<String> = None;
    let mut iters = 2000u64;
    let mut repeats = 3u32;
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cells" => {
                cells = Some(args[i + 1].clone());
                i += 2;
            }
            "--iters" => {
                iters = args[i + 1].parse().expect("--iters N");
                i += 2;
            }
            "--repeats" => {
                repeats = args[i + 1].parse().expect("--repeats N");
                i += 2;
            }
            "--out" => {
                out = Some(args[i + 1].clone());
                i += 2;
            }
            other => panic!("unexpected arg {other}"),
        }
    }
    let all: Vec<String> = all_cells()
        .into_iter()
        .map(|(id, _, _)| id.to_owned())
        .collect();
    let wanted: Vec<&str> = match &cells {
        Some(list) => list.split(',').collect(),
        None => all.iter().map(String::as_str).collect(),
    };
    let report = run_matrix(&wanted, iters, repeats);
    let json = serde_json::to_string_pretty(&report).expect("report json");
    match out {
        Some(path) => std::fs::write(&path, format!("{json}\n")).expect("write report"),
        None => println!("{json}"),
    }
}

fn cmd_compare(args: &[String]) {
    let mut baseline: Option<String> = None;
    let mut current: Option<String> = None;
    let mut budget: Option<String> = None;
    let mut rpc = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--baseline" => {
                baseline = Some(args[i + 1].clone());
                i += 2;
            }
            "--current" => {
                current = Some(args[i + 1].clone());
                i += 2;
            }
            "--budget" => {
                budget = Some(args[i + 1].clone());
                i += 2;
            }
            "--rpc" => {
                rpc = true;
                i += 1;
            }
            other => panic!("unexpected arg {other}"),
        }
    }
    let baseline = baseline.expect("--baseline FILE");
    let base: Report =
        serde_json::from_str(&std::fs::read_to_string(&baseline).expect("read baseline"))
            .expect("parse baseline");
    let current: Report = match current {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(&path).expect("read current"))
            .expect("parse current"),
        None => {
            let stdin = std::io::read_to_string(std::io::stdin()).expect("read stdin");
            serde_json::from_str(&stdin).expect("parse stdin")
        }
    };
    let mut ok = compare_reports(&base, &current, rpc);
    if let Some(path) = budget {
        ok &= check_budgets(&path, &current);
    }
    if !ok {
        std::process::exit(1);
    }
}

/// Absolute per-cell budgets for deterministic metrics (CH-10).
///
/// Unlike base-vs-head comparison, budgets pin exact ceilings that hold
/// on every host: allocations and blocking lock waits per op. Only
/// deterministic counts belong here; instructions and wall stay relative
/// (see `baselines/README.md`). `not_run` metrics skip, per policy.
#[derive(Debug, serde::Deserialize)]
struct BudgetFile {
    schema: String,
    cells: std::collections::HashMap<String, CellBudget>,
}

/// Ceilings per metric; absent entries are unbudgeted.
#[derive(Debug, serde::Deserialize)]
struct CellBudget {
    #[serde(default)]
    allocs: Option<f64>,
    #[serde(default)]
    locks: Option<f64>,
}

fn check_budgets(path: &str, current: &Report) -> bool {
    let file: BudgetFile =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read budget"))
            .expect("parse budget");
    assert_eq!(file.schema, "devloop-budget/1", "budget schema mismatch");
    let mut ok = true;
    for cell in &current.cells {
        let Some(budget) = file.cells.get(&cell.id) else {
            continue;
        };
        for (name, metric, ceiling) in [
            ("allocs", &cell.allocs, budget.allocs),
            ("locks", &cell.locks, budget.locks),
        ] {
            let (Some(value), Some(ceiling)) = (metric_value(metric), ceiling) else {
                continue;
            };
            if value > ceiling {
                println!(
                    "{} {name}: BUDGET EXCEEDED {value:.1} > {ceiling:.1}",
                    cell.id,
                );
                ok = false;
            }
        }
    }
    ok
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprint!("{}", usage());
        std::process::exit(2);
    }
    match args[1].as_str() {
        "list" => cmd_list(),
        "run-cell" => cmd_run_cell(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "compare" => cmd_compare(&args[2..]),
        "sizes" => blob::print_sizes(),
        _ => {
            eprint!("{}", usage());
            std::process::exit(2);
        }
    }
}
