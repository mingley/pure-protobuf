//! Opt-in construction-and-encode allocation diagnostic for BlobMixed.
//!
//! This is separate from the historical devloop/1 cells. It checks complete
//! outputs before counting; it does not measure CPU time, latency or RPCs.

#[allow(
    dead_code,
    reason = "reuse the exact BlobMixed schema and Prost oracle from devloop"
)]
#[path = "blob.rs"]
mod blob;

use blob::{ProstBlobMixed, pbrs_blob::BlobMixed};
use pbrs::{Parse, Serialize};
use prost::Message as _;
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PARTS: usize = 8;
const PART_BYTES: usize = 4096;
const MAX_ITERS: u64 = 100_000;

struct CountingAlloc;
static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicBool = AtomicBool::new(false);

// Allocator callbacks must not allocate or panic.
fn add(counter: &AtomicU64, amount: u64, overflowed: &AtomicBool) {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let (next, overflow) = match current.checked_add(amount) {
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

fn record(size: usize, reallocated: bool) {
    add(&ALLOCS, 1, &OVERFLOW);
    match u64::try_from(size) {
        Ok(size) => add(&BYTES, size, &OVERFLOW),
        Err(_) => OVERFLOW.store(true, Ordering::Relaxed),
    }
    if reallocated {
        add(&REALLOCS, 1, &OVERFLOW);
    }
}

// SAFETY: each operation forwards its arguments to System. Counters neither
// allocate nor panic, and unsuccessful allocations are not counted.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's contract is forwarded to System.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            record(layout.size(), false);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's contract is forwarded to System.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            record(layout.size(), false);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: GlobalAlloc's contract is forwarded to System.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: GlobalAlloc's contract is forwarded to System.
        let next = unsafe { System.realloc(ptr, layout, new_size) };
        if !next.is_null() && ARMED.load(Ordering::Relaxed) {
            record(new_size, true);
        }
        next
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

struct AllocGuard;

impl AllocGuard {
    fn arm() -> Self {
        ALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        REALLOCS.store(0, Ordering::Relaxed);
        OVERFLOW.store(false, Ordering::Relaxed);
        ARMED.store(true, Ordering::SeqCst);
        Self
    }

    fn finish(self) -> (u64, u64, u64, bool) {
        ARMED.store(false, Ordering::SeqCst);
        (
            ALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
            REALLOCS.load(Ordering::Relaxed),
            OVERFLOW.load(Ordering::Relaxed),
        )
    }
}

impl Drop for AllocGuard {
    fn drop(&mut self) {
        ARMED.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Codec {
    Pbrs,
    Prost,
}

impl Codec {
    fn name(self) -> &'static str {
        match self {
            Self::Pbrs => "pbrs",
            Self::Prost => "prost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Borrowed,
    Moved,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Borrowed => "borrowed",
            Self::Moved => "moved",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capacity {
    Exact,
    Spare,
}

impl Capacity {
    fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Spare => "spare",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Case {
    codec: Codec,
    source: Source,
    capacity: Capacity,
}

const CASES: [Case; 8] = [
    Case {
        codec: Codec::Pbrs,
        source: Source::Borrowed,
        capacity: Capacity::Exact,
    },
    Case {
        codec: Codec::Pbrs,
        source: Source::Moved,
        capacity: Capacity::Exact,
    },
    Case {
        codec: Codec::Pbrs,
        source: Source::Borrowed,
        capacity: Capacity::Spare,
    },
    Case {
        codec: Codec::Pbrs,
        source: Source::Moved,
        capacity: Capacity::Spare,
    },
    Case {
        codec: Codec::Prost,
        source: Source::Borrowed,
        capacity: Capacity::Exact,
    },
    Case {
        codec: Codec::Prost,
        source: Source::Moved,
        capacity: Capacity::Exact,
    },
    Case {
        codec: Codec::Prost,
        source: Source::Borrowed,
        capacity: Capacity::Spare,
    },
    Case {
        codec: Codec::Prost,
        source: Source::Moved,
        capacity: Capacity::Spare,
    },
];

fn payload(index: usize, capacity: Capacity) -> Vec<u8> {
    let value = u8::try_from(index).expect("part index fits u8");
    let bytes = match capacity {
        Capacity::Exact => vec![value; PART_BYTES],
        Capacity::Spare => {
            let mut bytes = Vec::with_capacity(PART_BYTES * 2);
            bytes.resize(PART_BYTES, value);
            bytes
        }
    };
    black_box(bytes)
}

fn pbrs_part(index: usize, source: Source, capacity: Capacity) -> pbrs::rt::LazyBytes {
    let bytes = payload(index, capacity);
    match source {
        Source::Borrowed => pbrs::rt::LazyBytes::from_bytes(bytes.as_slice()),
        Source::Moved => pbrs::rt::LazyBytes::from(bytes),
    }
}

fn prost_part(index: usize, source: Source, capacity: Capacity) -> Vec<u8> {
    let bytes = payload(index, capacity);
    match source {
        Source::Borrowed => bytes.as_slice().to_vec(),
        Source::Moved => bytes,
    }
}

// A case constructs its payloads and message inside the measured operation.
// Output and message are dropped in that order for every case. The callback
// receives the complete encoded output while it is alive.
fn work(case: Case, id: u64, inspect: impl FnOnce(&[u8])) {
    match case.codec {
        Codec::Pbrs => {
            let mut message = BlobMixed::new();
            message.set_id(id);
            let parts = (0..PARTS)
                .map(|index| pbrs_part(index, case.source, case.capacity))
                .collect::<Vec<_>>();
            message.set_parts(parts);
            let encoded = message.serialize().expect("pbrs serialize");
            inspect(black_box(encoded.as_slice()));
            drop(encoded);
            drop(message);
        }
        Codec::Prost => {
            let message = ProstBlobMixed {
                id,
                parts: (0..PARTS)
                    .map(|index| prost_part(index, case.source, case.capacity))
                    .collect(),
            };
            let encoded = message.encode_to_vec();
            inspect(black_box(encoded.as_slice()));
            drop(encoded);
            drop(message);
        }
    }
}

fn check_part(index: usize, bytes: &[u8]) {
    assert_eq!(bytes.len(), PART_BYTES, "part {index} length");
    let value = u8::try_from(index).expect("part index fits u8");
    assert!(
        bytes.iter().all(|byte| *byte == value),
        "part {index} contents"
    );
}

fn verify(id: u64) {
    let reference = Case {
        codec: Codec::Prost,
        source: Source::Moved,
        capacity: Capacity::Exact,
    };
    let mut expected = Vec::new();
    work(reference, id, |wire| expected.extend_from_slice(wire));
    for case in CASES {
        work(case, id, |wire| {
            assert_eq!(
                wire,
                expected.as_slice(),
                "complete wire mismatch: {case:?}"
            );

            let decoded_prost = ProstBlobMixed::decode(wire).expect("prost decode");
            assert_eq!(decoded_prost.id, id);
            assert_eq!(decoded_prost.parts.len(), PARTS);
            for (index, part) in decoded_prost.parts.iter().enumerate() {
                check_part(index, part);
            }

            let decoded_pbrs = BlobMixed::parse(wire).expect("pbrs decode");
            assert_eq!(decoded_pbrs.id(), id);
            assert_eq!(decoded_pbrs.parts().len(), PARTS);
            for (index, part) in decoded_pbrs.parts().into_iter().enumerate() {
                check_part(index, part.as_bytes());
            }
        });
    }
}

fn verify_ownership() {
    for capacity in [Capacity::Exact, Capacity::Spare] {
        let bytes = payload(3, capacity);
        assert_eq!(bytes.len(), PART_BYTES);
        match capacity {
            Capacity::Exact => assert_eq!(bytes.capacity(), PART_BYTES, "exact capacity"),
            Capacity::Spare => assert!(bytes.capacity() > bytes.len(), "spare capacity"),
        }
        let ptr = bytes.as_ptr();
        let moved = pbrs::rt::LazyBytes::from(bytes);
        assert_eq!(moved.as_bytes().as_ptr(), ptr, "moved payload copied");
        let alias = moved.clone();
        assert_eq!(alias.as_bytes().as_ptr(), ptr, "clone copied the payload");
        drop(moved);
        assert_eq!(alias.as_bytes().as_ptr(), ptr, "alias lost its payload");
        check_part(3, alias.as_bytes());

        let mut source_ptrs = Vec::with_capacity(PARTS);
        let mut parts = Vec::with_capacity(PARTS);
        for index in 0..PARTS {
            let bytes = payload(index, capacity);
            source_ptrs.push(bytes.as_ptr());
            parts.push(pbrs::rt::LazyBytes::from(bytes));
        }
        let mut message = BlobMixed::new();
        message.set_parts(parts);
        assert_eq!(message.parts().len(), PARTS);
        for (index, (part, ptr)) in message.parts().into_iter().zip(source_ptrs).enumerate() {
            assert_eq!(
                part.as_bytes().as_ptr(),
                ptr,
                "set_parts copied part {index}"
            );
            check_part(index, part.as_bytes());
        }
        drop(message);

        let bytes = payload(3, capacity);
        let ptr = bytes.as_ptr();
        let borrowed = pbrs::rt::LazyBytes::from_bytes(bytes.as_slice());
        assert_ne!(
            borrowed.as_bytes().as_ptr(),
            ptr,
            "borrowed input must be owned"
        );
        drop(bytes);
        check_part(3, borrowed.as_bytes());
    }
}

fn validate() {
    for id in [0, 1, 127, 128, 16_383, 16_384, u64::MAX] {
        verify(id);
    }
    verify_ownership();
}

// A separate positive control makes a broken or elided counter visible.
fn allocator_control() -> (u64, u64, u64) {
    let size = black_box(PART_BYTES * 2);
    let guard = AllocGuard::arm();
    let bytes = vec![black_box(0xa5_u8); size];
    black_box(bytes.as_slice());
    let (allocations, requested_bytes, reallocations, overflow) = guard.finish();
    drop(bytes);
    assert!(!overflow, "allocator control counter overflow");
    assert!(allocations > 0, "allocator control saw no allocations");
    assert!(
        requested_bytes >= u64::try_from(size).expect("control size fits u64"),
        "allocator control saw too few bytes"
    );
    (allocations, requested_bytes, reallocations)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [iterations_flag, iterations, warmup_flag, warmup] = args.as_slice() else {
        panic!("expected --iterations N --warmup N");
    };
    assert_eq!(iterations_flag, "--iterations");
    assert_eq!(warmup_flag, "--warmup");
    let iterations = iterations.parse::<u64>().expect("numeric iterations");
    let warmup = warmup.parse::<u64>().expect("numeric warmup");
    assert!(
        (1..=MAX_ITERS).contains(&iterations),
        "iterations out of bounds"
    );
    assert!(warmup <= MAX_ITERS, "warmup out of bounds");
    validate();
    let (control_allocations, control_bytes, control_reallocations) = allocator_control();

    let mut rows = Vec::with_capacity(CASES.len());
    for case in CASES {
        for id in 0..warmup {
            work(case, id, |wire| {
                black_box(wire.len());
            });
        }
        let guard = AllocGuard::arm();
        for id in 0..iterations {
            work(case, id, |wire| {
                black_box(wire.len());
            });
        }
        let (allocations, requested_bytes, reallocations, overflow) = guard.finish();
        assert!(!overflow, "allocation counter overflow");
        assert!(allocations > 0, "allocation row saw no allocations");
        assert!(requested_bytes > 0, "allocation row saw no bytes");
        rows.push(json!({
            "codec": case.codec.name(),
            "source": case.source.name(),
            "capacity": case.capacity.name(),
            "iterations": iterations,
            "allocations": allocations,
            "requested_bytes": requested_bytes,
            "reallocations": reallocations,
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "mixed-ownership-alloc/1",
            "scope": "process-wide allocations around construction, encode and drop",
            "workload": {
                "schema": "lpbench.BlobMixed",
                "parts": PARTS,
                "part_bytes": PART_BYTES,
                "id": "iteration index; varint length varies",
                "output": "complete encoded bytes passed to black_box",
                "warmup": warmup,
            },
            "limits": {
                "source_allocation": "included; this creates the input, not a pre-existing borrowed slice",
                "allocator": "System wrapped by a process-wide counting allocator",
                "snapshots": "relaxed atomic counters can include allocations from other threads",
                "reallocations": "counted as an allocation, full new size requested",
                "memory": "requested bytes, not live or peak bytes, retained capacity, or RSS",
                "instrumentation": "global counting allocator and pbrs copy-count hooks enabled",
                "timing": "not measured; CPU, latency and RPC performance remain unproven",
                "comparability": "new diagnostic; historical devloop/1 IDs and lifecycles unchanged",
            },
            "verified": "complete wire equality and both decoders for boundary IDs and all payload bytes",
            "allocator_positive_control": {
                "allocations": control_allocations,
                "requested_bytes": control_bytes,
                "reallocations": control_reallocations,
            },
            "rows": rows,
        }))
        .expect("diagnostic JSON")
    );
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[test]
    fn full_wire_and_owned_buffer_controls() {
        super::validate();
    }

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
    fn concurrent_updates_saturate_and_report_overflow() {
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
