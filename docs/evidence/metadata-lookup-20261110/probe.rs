//! Temporary benchmark: metadata first-value lookup.
#![allow(unsafe_code, reason = "the benchmark installs the repository counting allocator")]
#[path = "../../bench/devloop/compression/src/allocator.rs"]
mod allocator;

use pbrs_grpc::Metadata;
use std::hint::black_box;
use std::sync::atomic::Ordering;

#[global_allocator]
static ALLOC: allocator::CountingAlloc = allocator::CountingAlloc;

fn run(mut f: impl FnMut(), n: usize) {
    for _ in 0..n {
        f();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = &args[1];
    let n: usize = args[2].parse().unwrap();
    let count = args.get(3).is_some_and(|x| x == "count");
    let mut md = Metadata::new();
    md.insert("x-request-id", "first-value").unwrap();
    md.insert("x-request-id", "second-value").unwrap();
    md.insert_bin("x-trace-bin", b"abcdef0123456789").unwrap();
    for i in 0..32 {
        md.insert(format!("x-other-{i}"), "other").unwrap();
    }
    let long_hit = format!("x-{}", "a".repeat(128));
    let long_miss = format!("x-{}", "b".repeat(128));
    md.insert(&long_hit, "long").unwrap();
    let long_binary = format!("x-{}-bin", "b".repeat(128));
    let long_binary_upper = long_binary.to_ascii_uppercase();
    md.insert_bin(&long_binary, b"abcdef0123456789").unwrap();
    let long_upper_hit = long_hit.to_ascii_uppercase();
    let long_upper_miss = long_miss.to_ascii_uppercase();
    let key = match scenario.as_str() {
        "lower-hit" | "all-control" => "x-request-id",
        "upper-hit" => "X-Request-Id",
        "lower-miss" => "x-request-missing",
        "upper-miss" => "X-Request-Missing",
        "long-lower-hit" => long_hit.as_str(),
        "long-upper-hit" => long_upper_hit.as_str(),
        "long-lower-miss" => long_miss.as_str(),
        "long-upper-miss" => long_upper_miss.as_str(),
        "binary-hit" => "x-trace-bin",
        "binary-upper-hit" => "X-Trace-Bin",
        "binary-long-hit" => long_binary.as_str(),
        "binary-long-upper-hit" => long_binary_upper.as_str(),
        "reserved-control" => "grpc-timeout",
        _ => panic!("invalid scenario"),
    };
    // Reject setup or case-matching mistakes before starting the counters.
    match scenario.as_str() {
        "lower-hit" | "upper-hit" =>
            assert_eq!(md.get(key), Some("first-value")),
        "all-control" => assert_eq!(md.get_all(key).next(), Some("first-value")),
        "long-lower-hit" | "long-upper-hit" =>
            assert_eq!(md.get(key), Some("long")),
        "binary-hit" | "binary-upper-hit" | "binary-long-hit" | "binary-long-upper-hit" =>
            assert_eq!(md.get_bin(key).as_deref(), Some(&b"abcdef0123456789"[..])),
        _ => assert_eq!(md.get(key), None),
    }
    allocator::ALLOCS.store(0, Ordering::Relaxed);
    allocator::BYTES.store(0, Ordering::Relaxed);
    allocator::ARMED.store(count, Ordering::Relaxed);
    match scenario.as_str() {
        "binary-hit" | "binary-upper-hit" | "binary-long-hit" | "binary-long-upper-hit" => {
            run(|| { black_box(black_box(&md).get_bin(black_box(key))); }, n)
        }
        "all-control" => {
            run(|| { black_box(black_box(&md).get_all(black_box(key)).next()); }, n)
        }
        _ => run(|| { black_box(black_box(&md).get(black_box(key))); }, n),
    }
    allocator::ARMED.store(false, Ordering::Relaxed);
    println!("{} {}", allocator::ALLOCS.load(Ordering::Relaxed),
             allocator::BYTES.load(Ordering::Relaxed));
}
