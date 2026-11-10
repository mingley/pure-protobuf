//! Scratch-only singleton repeated lifecycle probe.
use pbrs::{Parse, Repeated, Serialize};
use pbrs_adoption_corpus::{native, prost_types, query, query_native, touch_query_native, touch_query_prost};
use prost::Message;
use std::hint::black_box;

#[cfg(feature = "singleton-count")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    pub static ACTIVE: AtomicBool = AtomicBool::new(false);
    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static REALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub struct Counted;
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                REALLOCS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(size as u64, Ordering::Relaxed);
            }
            unsafe { System.realloc(ptr, old, size) }
        }
    }
    pub fn begin() {
        ALLOCS.store(0, Ordering::SeqCst);
        REALLOCS.store(0, Ordering::SeqCst);
        BYTES.store(0, Ordering::SeqCst);
        ACTIVE.store(true, Ordering::SeqCst);
    }
    pub fn end() -> (u64, u64, u64) {
        ACTIVE.store(false, Ordering::SeqCst);
        (ALLOCS.load(Ordering::SeqCst), REALLOCS.load(Ordering::SeqCst),
         BYTES.load(Ordering::SeqCst))
    }
}
#[cfg(feature = "singleton-count")]
#[global_allocator]
static ALLOCATOR: counting::Counted = counting::Counted;
#[cfg(not(feature = "singleton-count"))]
mod counting {
    pub fn begin() {}
    pub fn end() -> (u64, u64, u64) { (0, 0, 0) }
}

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
unsafe extern "C" { fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32; }
fn thread_cpu_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(3, &mut ts) }, 0);
    (ts.sec as u64) * 1_000_000_000 + (ts.nsec as u64)
}

#[derive(Clone)]
struct Large([u8; 1024]);

struct Inputs {
    cached: native::Query,
    wire: Vec<u8>,
    identity: u64,
}

fn input() -> Inputs {
    for (depth, variant) in [(3, 0), (4, 6), (8, 6)] {
        let expected = query(depth, variant);
        let fresh = query_native(depth, variant);
        let native_wire = fresh.serialize().expect("native wire");
        assert_eq!(prost_types::Query::decode(native_wire.as_slice()).unwrap(), expected);
        assert_eq!(touch_query_native(&fresh), touch_query_prost(&expected));
        let prost_wire = expected.encode_to_vec();
        let parsed = native::Query::parse(prost_wire.as_slice()).unwrap();
        assert_eq!(touch_query_native(&parsed), touch_query_prost(&expected));
        assert_eq!(prost_types::Query::decode(parsed.serialize().unwrap().as_slice()).unwrap(), expected);
    }
    let cached = query_native(4, 6);
    let _ = cached.serialize().unwrap();
    let wire = query(4, 6).encode_to_vec();
    let identity = wire.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    Inputs { cached, wire, identity }
}

fn run(case: &str, n: usize, data: &Inputs) {
    match case {
        "query_empty" => for _ in 0..n {
            black_box(native::Query::new().serialize().unwrap());
        },
        "query3_fresh" => for _ in 0..n {
            black_box(query_native(3, 0).serialize().unwrap());
        },
        "query4_construct" => for _ in 0..n {
            black_box(query_native(4, 6));
        },
        "query4_fresh" => for _ in 0..n {
            black_box(query_native(4, 6).serialize().unwrap());
        },
        "query8_fresh" => for _ in 0..n {
            black_box(query_native(8, 6).serialize().unwrap());
        },
        "query4_mutate" => for _ in 0..n {
            let mut value = query_native(4, 6);
            value.children_mut().push(native::Query::new());
            black_box(value);
        },
        "query4_warm" => for _ in 0..n {
            black_box(data.cached.serialize().unwrap());
        },
        "query4_clone_encode" => for _ in 0..n {
            black_box(data.cached.clone().serialize().unwrap());
        },
        "query4_decode" => for _ in 0..n {
            black_box(native::Query::parse(black_box(data.wire.as_slice())).unwrap());
        },
        "query4_decode_read" => for _ in 0..n {
            let value = native::Query::parse(black_box(data.wire.as_slice())).unwrap();
            black_box(touch_query_native(&value));
        },
        "query4_prost_fresh" => for _ in 0..n {
            black_box(query(4, 6).encode_to_vec());
        },
        "repeat_one_collect" => for _ in 0..n {
            let value: Repeated<u64> = [black_box(7)].into_iter().collect();
            black_box(value);
        },
        "repeat_one_proxy" => for _ in 0..n {
            let mut value: Repeated<u64> = [black_box(7)].into_iter().collect();
            black_box(value.as_mut().len());
            black_box(value);
        },
        "repeat_one_into_vec" => for _ in 0..n {
            let value: Repeated<u64> = [black_box(7)].into_iter().collect();
            black_box(value.into_vec());
        },
        "repeat_eight_collect" => for _ in 0..n {
            let value: Repeated<u64> = black_box([1,2,3,4,5,6,7,8]).into_iter().collect();
            black_box(value);
        },
        "repeat_large_one" => for _ in 0..n {
            let value: Repeated<Large> = [Large([black_box(3); 1024])].into_iter().collect();
            black_box(value);
        },
        "repeat_large_many" => for _ in 0..n {
            let vec = vec![Large([black_box(3); 1024]), Large([black_box(4); 1024])];
            let value = Repeated::from_vec(vec);
            black_box(value.as_slice()[0].0[0]);
            black_box(value);
        },
        _ => panic!("unknown case {case}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let case = args.get(1).expect("case");
    let n: usize = args.get(2).expect("iterations").parse().expect("integer");
    assert!(n > 0);
    let data = input();
    run(case, 2, &data);
    counting::begin();
    let start = thread_cpu_ns();
    run(case, n, &data);
    let cpu_ns = thread_cpu_ns() - start;
    let (allocs, reallocs, bytes) = counting::end();
    println!("{{\"case\":\"{}\",\"n\":{},\"cpu_ns\":{},\"allocs\":{},\"reallocs\":{},\"bytes\":{},\"wire_len\":{},\"wire_fnv\":{}}}",
        case, n, cpu_ns, allocs, reallocs, bytes, data.wire.len(), data.identity);
}
