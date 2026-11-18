//! Dynamic protobuf table-memo screen. Copied into examples/ by the driver.
#![allow(unexpected_cfgs, reason = "separate counter and timing binaries")]

use pbrs::{DescriptorPool, DynamicMessage, Serialize};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

#[cfg(memo_counts)]
#[allow(unsafe_code, reason = "benchmark-only forwarding allocator")]
#[path = "../bench/devloop/compression/src/allocator.rs"]
mod allocator;
#[cfg(memo_counts)]
#[global_allocator]
static ALLOCATOR: allocator::CountingAlloc = allocator::CountingAlloc;

const LEAF: &[u8] = b"\x08\x2a\x10\x01\x1a\x06sample";

fn len_field(wire: &mut Vec<u8>, field: u8, payload: &[u8]) {
    // All fixture field tags and lengths fit one byte.
    assert!(field < 16 && payload.len() < 128);
    wire.push((field << 3) | 2);
    wire.push(u8::try_from(payload.len()).expect("short fixture"));
    wire.extend_from_slice(payload);
}

fn fixture(name: &str) -> (&'static str, Vec<u8>, bool) {
    match name {
        "pooled_empty" => ("Empty", Vec::new(), true),
        "pooled_leaf" => ("Leaf", LEAF.to_vec(), true),
        "poolless_leaf" => ("Leaf", LEAF.to_vec(), false),
        "recursive" => {
            let mut child = vec![8, 1];
            for id in 2..=8 {
                let mut parent = vec![8, id];
                len_field(&mut parent, 2, &child);
                child = parent;
            }
            ("Recursive", child, true)
        }
        "repeated_child" => {
            let mut wire = Vec::new();
            for _ in 0..8 {
                len_field(&mut wire, 1, LEAF);
            }
            ("RepeatedChild", wire, true)
        }
        "wide_memo" => {
            let mut wire = Vec::new();
            for field in 1..=8 {
                len_field(&mut wire, field, LEAF);
            }
            ("WideMemo", wire, true)
        }
        _ => panic!("unknown case"),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let case = args.next().expect("case");
    let n: u64 = args.next().expect("iterations").parse().expect("integer");
    let fds = std::fs::read(args.next().expect("descriptor path")).expect("read descriptor");
    assert!(n > 0 && args.next().is_none());
    let (name, wire, pooled) = fixture(&case);
    let pool = Arc::new(DescriptorPool::from_file_descriptor_set(&fds).expect("descriptor set"));
    let descriptor = pool.get_message(&format!("memo.{name}")).expect("message descriptor");
    let pool = pooled.then_some(pool);
    let parse = || {
        DynamicMessage::parse_with_pool(
            black_box(Arc::clone(&descriptor)),
            black_box(pool.clone()),
            black_box(wire.as_slice()),
        )
        .expect("valid input")
    };
    assert_eq!(parse().serialize().expect("serialized"), wire, "round trip");
    for _ in 0..64 {
        drop(black_box(parse()));
    }

    #[cfg(memo_counts)]
    allocator::ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..n {
        drop(black_box(parse()));
    }
    let elapsed = start.elapsed().as_nanos();
    #[cfg(memo_counts)]
    allocator::ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
    #[cfg(memo_counts)]
    let (allocs, bytes) = (
        allocator::ALLOCS.load(std::sync::atomic::Ordering::Relaxed),
        allocator::BYTES.load(std::sync::atomic::Ordering::Relaxed),
    );
    #[cfg(not(memo_counts))]
    let (allocs, bytes) = (0_u64, 0_u64);
    let fingerprint = wire.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    });
    println!(
        "{{\"case\":\"{case}\",\"n\":{n},\"wall_ns\":{elapsed},\"alloc_events\":{allocs},\"requested_bytes\":{bytes},\"wire_len\":{},\"wire_hash\":\"{fingerprint:016x}\"}}",
        wire.len()
    );
}
