#![allow(unsafe_code, reason = "diagnostic counts allocator requests")]
use bytes::Bytes;
use pbrs::{Parse, ProtoBytes};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

include!(concat!(env!("OUT_DIR"), "/native-bytes/mod.rs"));

struct Counting;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ACTIVE.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ACTIVE.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() && ACTIVE.load(Ordering::Relaxed) {
            REALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size, Ordering::Relaxed);
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;
static STATIC_DATA: [u8; 65_536] = [0xa5; 65_536];

struct Input {
    value: ProtoBytes,
    keeper: Option<Bytes>,
    root: *const u8,
    root_capacity: usize,
}

fn write_varint(mut n: usize, out: &mut Vec<u8>) {
    while n >= 128 {
        out.push(((n & 127) as u8) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}

fn make(case: &str, n: usize) -> Input {
    match case {
        "exact" | "spare" | "shared" | "generated_set" => {
            let capacity = if case == "spare" { n * 8 } else { n };
            let mut raw = Vec::with_capacity(capacity);
            raw.resize(n, 0xa5);
            let root = raw.as_ptr();
            let root_capacity = raw.capacity();
            if case == "generated_set" {
                let mut child = proof::bytes::Child::new();
                child.set_payload(raw);
                let value = child.payload_shared();
                drop(child);
                Input { value, keeper: None, root, root_capacity }
            } else {
                let b = Bytes::from(raw);
                let keeper = if case == "shared" { Some(b.clone()) } else { None };
                Input { value: ProtoBytes::from(b), keeper, root, root_capacity }
            }
        }
        "slice" => {
            let mut raw = Vec::with_capacity(n * 8 + 256);
            raw.resize(n + 32, 0xa5);
            let root = raw.as_ptr();
            let root_capacity = raw.capacity();
            let b = Bytes::from(raw);
            let slice = b.slice(16..16+n);
            drop(b);
            Input { value: ProtoBytes::from(slice), keeper: None, root, root_capacity }
        }
        "static" => Input {
            value: ProtoBytes::from(Bytes::from_static(&STATIC_DATA[..n])),
            keeper: None, root: STATIC_DATA.as_ptr(), root_capacity: 0,
        },
        "generated_parse" => {
            let mut raw = Vec::with_capacity(n * 8 + 256);
            raw.push(0x0a);
            write_varint(n, &mut raw);
            raw.resize(raw.len() + n, 0xa5);
            let root = raw.as_ptr();
            let root_capacity = raw.capacity();
            let child = proof::bytes::Child::parse_bytes(Bytes::from(raw)).expect("parse");
            let value = child.payload_shared();
            drop(child);
            Input { value, keeper: None, root, root_capacity }
        }
        _ => panic!("unknown case"),
    }
}

fn measure(case: &str, method: &str, n: usize, round: usize) {
    let Input { value, keeper, root, root_capacity } = make(case, n);
    assert_eq!(value.as_bytes().len(), n);
    assert!(value.as_bytes().iter().all(|v| *v == 0xa5));
    let payload = value.as_bytes().as_ptr();
    ALLOCS.store(0, Ordering::Relaxed);
    REALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::SeqCst);
    let mut result: Vec<u8> = match method {
        "copy" => value.into_vec(),
        "public" => {
            let shared = value.as_shared();
            drop(value);
            shared.into()
        }
        "direct" => value.__diagnostic_into_vec_reuse(),
        _ => panic!("unknown method"),
    };
    ACTIVE.store(false, Ordering::SeqCst);
    let allocs = ALLOCS.load(Ordering::Relaxed);
    let reallocs = REALLOCS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    assert_eq!(result.len(), n);
    assert!(result.iter().all(|v| *v == 0xa5));
    let same_root = result.as_ptr() == root;
    let same_payload = result.as_ptr() == payload;
    if method == "copy" {
        assert!(!same_payload);
    }
    if let Some(keeper) = keeper {
        result[0] = 0x44;
        assert_eq!(keeper[0], 0xa5, "converted output must be independent");
    }
    println!("PBRS_OWNERSHIP {case} {method} {n} {round} {allocs} {reallocs} {bytes} {root_capacity} {} {same_root} {same_payload}", result.capacity());
}

fn main() {
    for case in ["exact", "spare", "slice", "shared", "static", "generated_set", "generated_parse"] {
        for n in [64usize, 4096, 65_536] {
            for method in ["copy", "public", "direct"] {
                for round in 0..5 {
                    measure(case, method, n, round);
                }
            }
        }
    }
}
