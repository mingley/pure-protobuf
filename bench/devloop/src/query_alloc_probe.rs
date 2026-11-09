//! Opt-in, synchronous allocation diagnostic for the adoption Query fixture.
//! This binary is a benchmark fixture, not part of the transport runtime.

use pbrs::{Parse, Serialize, WireOut};
use pbrs_adoption_corpus::{native, prost_types, query, touch_query_native, touch_query_prost};
use pbrs_grpc::{CodecMessage, codec::prost::Message as TransportProst};
use prost::{
    Message as ProstMessage,
    bytes::{BufMut, Bytes, BytesMut},
};
use serde::Serialize as SerdeSerialize;
use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

struct CountingAllocator;
static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicBool = AtomicBool::new(false);

fn add(counter: &AtomicU64, value: u64, overflowed: &AtomicBool) {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let (next, overflow) = match current.checked_add(value) {
            Some(next) => (next, false),
            None => (u64::MAX, true),
        };
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                if overflow {
                    overflowed.store(true, Ordering::Relaxed);
                }
                return;
            }
            Err(observed) => current = observed,
        }
    }
}

fn record(size: usize, realloc: bool) {
    add(&ALLOCS, 1, &OVERFLOW);
    match u64::try_from(size) {
        Ok(size) => add(&BYTES, size, &OVERFLOW),
        Err(_) => OVERFLOW.store(true, Ordering::Relaxed),
    }
    if realloc {
        add(&REALLOCS, 1, &OVERFLOW);
    }
}

// SAFETY: each operation forwards its arguments to System. Counting uses only
// atomics, does not allocate or panic, and ignores unsuccessful allocations.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarding the GlobalAlloc contract to System.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            record(layout.size(), false);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarding the GlobalAlloc contract to System.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            record(layout.size(), false);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarding the GlobalAlloc contract to System.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarding the GlobalAlloc contract to System.
        let next = unsafe { System.realloc(ptr, layout, new_size) };
        if !next.is_null() && ARMED.load(Ordering::Relaxed) {
            record(new_size, true);
        }
        next
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy, Debug, PartialEq, Eq, SerdeSerialize)]
struct Counts {
    allocations_including_reallocations: u64,
    requested_bytes: u64,
    reallocations: u64,
}

struct Guard(bool);

impl Guard {
    fn start() -> Self {
        assert!(!ARMED.load(Ordering::SeqCst), "nested measurement");
        ALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        REALLOCS.store(0, Ordering::Relaxed);
        OVERFLOW.store(false, Ordering::Relaxed);
        assert!(
            !ARMED.swap(true, Ordering::SeqCst),
            "concurrent measurement"
        );
        Self(true)
    }

    fn finish(mut self) -> Counts {
        ARMED.store(false, Ordering::SeqCst);
        self.0 = false;
        assert!(!OVERFLOW.load(Ordering::Relaxed), "counter overflow");
        Counts {
            allocations_including_reallocations: ALLOCS.load(Ordering::Relaxed),
            requested_bytes: BYTES.load(Ordering::Relaxed),
            reallocations: REALLOCS.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.0 {
            ARMED.store(false, Ordering::SeqCst);
        }
    }
}

fn allocator_controls() -> (Counts, Counts) {
    let guard = Guard::start();
    black_box(0_u8);
    let zero = guard.finish();
    assert_eq!(
        zero,
        Counts {
            allocations_including_reallocations: 0,
            requested_bytes: 0,
            reallocations: 0,
        },
        "unrelated allocation in empty scope"
    );
    let guard = Guard::start();
    let mut bytes = Vec::<u8>::with_capacity(black_box(37));
    bytes.push(black_box(0x5a));
    black_box(&bytes);
    let positive = guard.finish();
    assert!(
        positive.allocations_including_reallocations >= 1,
        "counting allocator positive control saw no allocation"
    );
    assert!(
        positive.requested_bytes >= 37,
        "counting allocator positive control saw too few requested bytes"
    );
    black_box(bytes);
    (zero, positive)
}

trait QueryValue: CodecMessage + Clone {
    fn read_all(&self) -> u64;
    fn prepared(fixture: &Fixture) -> Self;
}

impl QueryValue for native::Query {
    fn read_all(&self) -> u64 {
        touch_query_native(self)
    }

    fn prepared(fixture: &Fixture) -> Self {
        fixture.native.clone()
    }
}

impl QueryValue for TransportProst<prost_types::Query> {
    fn read_all(&self) -> u64 {
        touch_query_prost(&self.0)
    }

    fn prepared(fixture: &Fixture) -> Self {
        Self(fixture.prost.clone())
    }
}

struct Fixture {
    name: &'static str,
    wire: Vec<u8>,
    native: native::Query,
    prost: prost_types::Query,
    checksum: u64,
}

fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn fixture(name: &'static str, source: prost_types::Query) -> Fixture {
    let wire = source.encode_to_vec();
    let native = native::Query::parse(wire.as_slice()).expect("native fixture");
    let prost = prost_types::Query::decode(wire.as_slice()).expect("prost fixture");
    assert_eq!(prost, source);
    let checksum = touch_query_prost(&prost);
    // Match the RPC fixture: serialize once before its complete-read oracle,
    // then serialize the same message again during qualification.
    let native_wire = native.serialize().expect("native fixture encoding");
    assert_eq!(
        native_wire.as_slice(),
        wire.as_slice(),
        "native request bytes"
    );
    assert_eq!(touch_query_native(&native), checksum);
    let validated_wire = native.serialize().expect("validated native encoding");
    assert_eq!(
        validated_wire.as_slice(),
        wire.as_slice(),
        "validated native request bytes"
    );
    assert_eq!(
        prost_types::Query::decode(validated_wire.as_slice()).expect("round trip"),
        prost
    );
    let cloned_native = native.clone();
    assert_eq!(touch_query_native(&cloned_native), checksum);
    let cloned_wire = cloned_native.serialize().expect("native clone encoding");
    assert_eq!(
        cloned_wire.as_slice(),
        wire.as_slice(),
        "cloned request bytes"
    );
    assert_eq!(
        prost_types::Query::decode(cloned_wire.as_slice()).expect("clone round trip"),
        prost
    );
    let cloned_prost = prost.clone();
    assert_eq!(cloned_prost, prost);
    assert_eq!(touch_query_prost(&cloned_prost), checksum);
    Fixture {
        name,
        wire,
        native,
        prost,
        checksum,
    }
}

/// Small contiguous stand-in for the private native SegSink. The fixture
/// payloads are below the shared-segment threshold; abort if that changes.
struct FrameSink {
    bytes: BytesMut,
    shared: bool,
}

impl WireOut for FrameSink {
    fn put_u8(&mut self, byte: u8) {
        BufMut::put_u8(&mut self.bytes, byte);
    }

    fn put_slice(&mut self, bytes: &[u8]) {
        BufMut::put_slice(&mut self.bytes, bytes);
    }

    fn write_contiguous<R>(&mut self, write: impl FnOnce(&mut BytesMut) -> R) -> R {
        write(&mut self.bytes)
    }

    fn put_shared(&mut self, bytes: &Bytes) {
        self.shared = true;
        BufMut::put_slice(&mut self.bytes, bytes);
    }

    fn supports_shared(&self) -> bool {
        true
    }
}

fn frame<M: QueryValue>(message: &M) -> Bytes {
    let len = message.encoded_len();
    assert!(len < 32 * 1024, "fixture must not need shared segments");
    let prefix = u32::try_from(len).expect("fixture length");
    let mut sink = FrameSink {
        bytes: BytesMut::with_capacity(len + 5),
        shared: false,
    };
    sink.put_u8(0);
    sink.put_slice(&prefix.to_be_bytes());
    message.encode_payload(&mut sink).expect("encode payload");
    assert!(!sink.shared, "unexpected shared segment");
    assert_eq!(sink.bytes.len(), len + 5);
    sink.bytes.freeze()
}

fn response_wire<M: QueryValue>(fixture: &Fixture) -> Bytes {
    let message =
        M::decode_payload(Bytes::copy_from_slice(&fixture.wire)).expect("decode response");
    assert_eq!(message.read_all(), fixture.checksum);
    let encoded = frame(&message);
    let payload = encoded.slice(5..);
    assert_eq!(payload.len(), fixture.wire.len());
    assert_eq!(
        prost_types::Query::decode(payload.clone()).expect("response round trip"),
        fixture.prost
    );
    payload
}

#[derive(Clone, Copy)]
enum Mode {
    Unique,
    Shared,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::Shared => "shared",
        }
    }
}

struct Payloads {
    inputs: Vec<Bytes>,
    // Keep a shared input owner alive during every measured operation.
    keeper: Option<Bytes>,
}

fn payloads(wire: &[u8], count: usize, mode: Mode) -> Payloads {
    match mode {
        Mode::Unique => {
            let inputs: Vec<Bytes> = (0..count).map(|_| Bytes::copy_from_slice(wire)).collect();
            if !wire.is_empty() {
                assert!(inputs.iter().all(Bytes::is_unique));
            }
            Payloads {
                inputs,
                keeper: None,
            }
        }
        Mode::Shared => {
            let original = Bytes::copy_from_slice(wire);
            let inputs: Vec<Bytes> = (0..count).map(|_| original.clone()).collect();
            if !wire.is_empty() {
                assert!(!original.is_unique());
                assert!(inputs.iter().all(|input| !input.is_unique()));
            }
            Payloads {
                inputs,
                keeper: Some(original),
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Stage {
    Clone,
    RequestDecode,
    RequestFirstRead,
    RequestDecodeRead,
    ResponseDecodeRead,
    ResponseEncode,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Clone => "fully_read_request_clone",
            Self::RequestDecode => "request_decode",
            Self::RequestFirstRead => "request_first_read",
            Self::RequestDecodeRead => "request_decode_read",
            Self::ResponseDecodeRead => "response_decode_read",
            Self::ResponseEncode => "response_encode",
        }
    }
}

fn measure<M: QueryValue>(
    fixture: &Fixture,
    response: &[u8],
    mode: Mode,
    stage: Stage,
    count: usize,
) -> Counts {
    match stage {
        Stage::Clone => {
            let prepared = M::prepared(fixture);
            let guard = Guard::start();
            for _ in 0..count {
                let cloned = M::clone(black_box(&prepared));
                black_box(cloned);
            }
            guard.finish()
        }
        Stage::RequestDecode | Stage::RequestDecodeRead | Stage::ResponseDecodeRead => {
            let wire = if matches!(stage, Stage::ResponseDecodeRead) {
                response
            } else {
                &fixture.wire
            };
            let Payloads { inputs, keeper } = payloads(wire, count, mode);
            let guard = Guard::start();
            for input in inputs {
                let message = M::decode_payload(black_box(input)).expect("decode");
                if !matches!(stage, Stage::RequestDecode) {
                    assert_eq!(black_box(message.read_all()), fixture.checksum);
                }
                black_box(message);
            }
            black_box(&keeper);
            guard.finish()
        }
        Stage::RequestFirstRead => {
            let Payloads { inputs, keeper } = payloads(&fixture.wire, count, mode);
            let messages: Vec<M> = inputs
                .into_iter()
                .map(|input| M::decode_payload(input).expect("prepare first read"))
                .collect();
            let guard = Guard::start();
            for message in messages {
                assert_eq!(black_box(message.read_all()), fixture.checksum);
                black_box(message);
            }
            black_box(&keeper);
            guard.finish()
        }
        Stage::ResponseEncode => {
            // Fresh, fully read requests mimic the echo handler. Do not
            // serialize them during preparation: encode may fill size caches.
            let Payloads { inputs, keeper } = payloads(&fixture.wire, count, mode);
            let messages: Vec<M> = inputs
                .into_iter()
                .map(|input| {
                    let message = M::decode_payload(input).expect("prepare response");
                    assert_eq!(message.read_all(), fixture.checksum);
                    message
                })
                .collect();
            let guard = Guard::start();
            for message in messages {
                let encoded = frame(black_box(&message));
                let payload = encoded.get(5..).expect("frame prefix");
                assert_eq!(
                    payload, response,
                    "response bytes changed during measurement"
                );
                black_box(encoded);
            }
            black_box(&keeper);
            guard.finish()
        }
    }
}

#[derive(SerdeSerialize)]
struct Slope {
    allocations_including_reallocations: f64,
    requested_bytes: f64,
    reallocations: f64,
}

fn slope(low: Counts, high: Counts, n: usize) -> Slope {
    let per_call = |before: u64, after: u64| (after as f64 - before as f64) / n as f64;
    Slope {
        allocations_including_reallocations: per_call(
            low.allocations_including_reallocations,
            high.allocations_including_reallocations,
        ),
        requested_bytes: per_call(low.requested_bytes, high.requested_bytes),
        reallocations: per_call(low.reallocations, high.reallocations),
    }
}

#[derive(SerdeSerialize)]
struct Samples {
    at_n: [Counts; 2],
    at_2n: [Counts; 2],
    n_to_2n_per_operation: [Slope; 2],
    repeats_identical: bool,
    slopes_nonnegative: bool,
}

fn summarize(at_n: [Counts; 2], at_2n: [Counts; 2], n: usize) -> Samples {
    let nonnegative = |low: Counts, high: Counts| {
        high.allocations_including_reallocations >= low.allocations_including_reallocations
            && high.requested_bytes >= low.requested_bytes
            && high.reallocations >= low.reallocations
    };
    Samples {
        n_to_2n_per_operation: [slope(at_n[0], at_2n[0], n), slope(at_n[1], at_2n[1], n)],
        repeats_identical: at_n[0] == at_n[1] && at_2n[0] == at_2n[1],
        slopes_nonnegative: nonnegative(at_n[0], at_2n[0]) && nonnegative(at_n[1], at_2n[1]),
        at_n,
        at_2n,
    }
}

fn run_case(
    fixture: &Fixture,
    stage: Stage,
    mode: Mode,
    n: usize,
    native_response: &[u8],
    prost_response: &[u8],
) -> serde_json::Value {
    // Same process and ordering at each size: A B B A. Every measured stage
    // is synchronous; there is no Tokio runtime or concurrent RPC traffic.
    let sample = |count| {
        let a1 = measure::<native::Query>(fixture, native_response, mode, stage, count);
        let b1 = measure::<TransportProst<prost_types::Query>>(
            fixture,
            prost_response,
            mode,
            stage,
            count,
        );
        let b2 = measure::<TransportProst<prost_types::Query>>(
            fixture,
            prost_response,
            mode,
            stage,
            count,
        );
        let a2 = measure::<native::Query>(fixture, native_response, mode, stage, count);
        ([a1, a2], [b1, b2])
    };
    let (native_n, prost_n) = sample(n);
    let (native_2n, prost_2n) = sample(n * 2);
    let native = summarize(native_n, native_2n, n);
    let prost = summarize(prost_n, prost_2n, n);
    json!({
        "fixture": fixture.name,
        "stage": stage.name(),
        "input_mode": if matches!(stage, Stage::Clone) { "fully_read_prepared" } else { mode.name() },
        "n": n,
        "pbrs_codec": native,
        "prost_codec": prost,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args
        .next()
        .as_deref()
        .unwrap_or("200")
        .parse()
        .expect("N must be an integer");
    assert!((1..=10_000).contains(&n), "N must be in 1..=10000");
    assert!(args.next().is_none(), "expected at most N");
    let (zero_control, positive_control) = allocator_controls();

    let exact = query(4, 6);
    let mut single = exact.clone();
    single.children.clear();
    single.predicates.truncate(1);
    single.subqueries.clear();
    single.traversals.clear();
    let fixtures = [
        fixture("empty", prost_types::Query::default()),
        fixture("one_predicate", single),
        fixture("query.d4.v6", exact),
    ];
    let exact = fixtures.last().expect("exact fixture");
    assert_eq!(exact.wire.len(), 293, "historical query length");
    assert_eq!(
        fingerprint(&exact.wire),
        0xaf05_5fbb_c4e5_4c32,
        "historical prepared input"
    );

    let mut rows = Vec::new();
    let mut response_inputs = Vec::new();
    let stages = [
        Stage::Clone,
        Stage::RequestDecode,
        Stage::RequestFirstRead,
        Stage::RequestDecodeRead,
        Stage::ResponseDecodeRead,
        Stage::ResponseEncode,
    ];
    for fixture in &fixtures {
        let native_response = response_wire::<native::Query>(fixture);
        let prost_response = response_wire::<TransportProst<prost_types::Query>>(fixture);
        response_inputs.push(json!({
            "fixture": fixture.name,
            "pbrs_wire_bytes": native_response.len(),
            "pbrs_wire_fnv1a64": format!("{:016x}", fingerprint(&native_response)),
            "prost_wire_bytes": prost_response.len(),
            "prost_wire_fnv1a64": format!("{:016x}", fingerprint(&prost_response)),
        }));
        // Warm the operations with distinct inputs, outside measurement.
        for stage in stages {
            for mode in [Mode::Unique, Mode::Shared] {
                black_box(measure::<native::Query>(
                    fixture,
                    &native_response,
                    mode,
                    stage,
                    2,
                ));
                black_box(measure::<TransportProst<prost_types::Query>>(
                    fixture,
                    &prost_response,
                    mode,
                    stage,
                    2,
                ));
            }
        }
        for stage in stages {
            for mode in [Mode::Unique, Mode::Shared] {
                if matches!(stage, Stage::Clone) && matches!(mode, Mode::Shared) {
                    continue;
                }
                rows.push(run_case(
                    fixture,
                    stage,
                    mode,
                    n,
                    &native_response,
                    &prost_response,
                ));
            }
        }
    }
    let repeats_identical = rows.iter().all(|row| {
        row["pbrs_codec"]["repeats_identical"] == true
            && row["prost_codec"]["repeats_identical"] == true
    });
    let slopes_nonnegative = rows.iter().all(|row| {
        row["pbrs_codec"]["slopes_nonnegative"] == true
            && row["prost_codec"]["slopes_nonnegative"] == true
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "query-allocation-probe/1",
            "scope": "synchronous component allocation counts; no RPC transport",
            "instrumentation": "devloop enables pbrs and pbrs-grpc copy-counts; counting allocator active in measured windows",
            "allocation_events": "successful alloc, alloc_zeroed and realloc; realloc is included in total",
            "realloc_bytes": "full new size",
            "n": n,
            "all_repeats_identical": repeats_identical,
            "all_slopes_nonnegative": slopes_nonnegative,
            "allocator_controls": {"zero": zero_control, "positive": positive_control},
            "layout_bytes": {
                "pbrs_query": size_of::<native::Query>(),
                "prost_query": size_of::<prost_types::Query>(),
                "pbrs_repeated_query": size_of::<pbrs::Repeated<native::Query>>(),
                "prost_vec_query": size_of::<Vec<prost_types::Query>>(),
                "pbrs_lazy_query": size_of::<pbrs::rt::LazyMsg<native::Query>>(),
            },
            "inputs": fixtures.iter().map(|fixture| json!({
                "fixture": fixture.name,
                "wire_bytes": fixture.wire.len(),
                "wire_fnv1a64": format!("{:016x}", fingerprint(&fixture.wire)),
                "read_checksum": fixture.checksum.to_string(),
            })).collect::<Vec<_>>(),
            "response_inputs": response_inputs,
            "samples": rows,
        }))
        .expect("report")
    );
    assert!(
        repeats_identical,
        "allocation samples varied between repeats"
    );
    assert!(slopes_nonnegative, "N-to-2N counts decreased");
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[test]
    fn counter_saturates_without_wrapping() {
        let counter = AtomicU64::new(u64::MAX - 1);
        let overflowed = AtomicBool::new(false);
        super::add(&counter, 1, &overflowed);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(!overflowed.load(Ordering::Relaxed));

        super::add(&counter, 0, &overflowed);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(!overflowed.load(Ordering::Relaxed));

        super::add(&counter, 1, &overflowed);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(overflowed.load(Ordering::Relaxed));
    }

    #[test]
    fn concurrent_updates_conserve_then_saturate() {
        let counter = AtomicU64::new(0);
        let overflowed = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        for _ in 0..1_000 {
                            super::add(&counter, 1, &overflowed);
                        }
                    })
                })
                .collect::<Vec<_>>();
            for handle in handles {
                handle.join().expect("counter worker");
            }
        });
        assert_eq!(counter.load(Ordering::Relaxed), 8_000);
        assert!(!overflowed.load(Ordering::Relaxed));

        counter.store(u64::MAX - 4, Ordering::Relaxed);
        std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| scope.spawn(|| super::add(&counter, 1, &overflowed)))
                .collect::<Vec<_>>();
            for handle in handles {
                handle.join().expect("counter worker");
            }
        });
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(overflowed.load(Ordering::Relaxed));
    }
}
