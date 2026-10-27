//! Diagnostic same-schema comparison. Not a world ranking.
use pbrs::{Parse, Serialize};
use prost::Message;
use std::hint::black_box;

#[allow(dead_code, unused, nonstandard_style, clippy::all)]
mod p {
    include!(concat!(env!("OUT_DIR"), "/pbrs/codec_cases.rs"));
}
#[allow(dead_code, clippy::all)]
mod r {
    include!(concat!(env!("OUT_DIR"), "/prost/cases.rs"));
}

#[cfg(feature = "allocations")]
mod count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    pub static ENABLED: AtomicBool = AtomicBool::new(false);
    pub static CALLS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub struct Alloc;
    unsafe impl GlobalAlloc for Alloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            ptr
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            ptr
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new: usize) -> *mut u8 {
            let result = unsafe { System.realloc(ptr, old, new) };
            if !result.is_null() && ENABLED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(new as u64, Ordering::Relaxed);
            }
            result
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    pub fn start() {
        CALLS.store(0, Ordering::SeqCst);
        BYTES.store(0, Ordering::SeqCst);
        ENABLED.store(true, Ordering::SeqCst);
    }
    pub fn stop() -> (u64, u64) {
        ENABLED.store(false, Ordering::SeqCst);
        (CALLS.load(Ordering::SeqCst), BYTES.load(Ordering::SeqCst))
    }
}
#[cfg(feature = "allocations")]
#[global_allocator]
static ALLOC: count::Alloc = count::Alloc;

fn cpu_ns() -> u128 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a valid timespec out pointer and PROCESS_CPUTIME_ID is read-only.
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) },
        0
    );
    ts.tv_sec as u128 * 1_000_000_000 + ts.tv_nsec as u128
}
fn sum(bytes: &[u8]) -> u64 {
    bytes.iter().map(|&b| u64::from(b)).sum()
}
fn hash(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100000001b3))
    )
}
// Canonicalize repeated map entries without changing their semantics. All
// other bytes remain identical; the fixture has no duplicate map keys.
fn sorted_map_wire(wire: &[u8]) -> Vec<u8> {
    fn varint(wire: &[u8], pos: &mut usize) -> u64 {
        let mut value = 0;
        for shift in (0..70).step_by(7) {
            let byte = wire[*pos];
            *pos += 1;
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return value;
            }
        }
        panic!("malformed generated fixture");
    }
    let mut fields = Vec::new();
    let mut maps = Vec::new();
    let mut pos = 0;
    while pos < wire.len() {
        let start = pos;
        let tag = varint(wire, &mut pos);
        match tag & 7 {
            0 => {
                varint(wire, &mut pos);
            }
            1 => pos += 8,
            2 => {
                let len = varint(wire, &mut pos) as usize;
                pos += len;
            }
            5 => pos += 4,
            _ => panic!("unexpected fixture wire type"),
        }
        assert!(pos <= wire.len());
        let field = wire[start..pos].to_vec();
        if tag == 66 {
            maps.push(field);
        } else {
            fields.push(field);
        }
    }
    maps.sort();
    fields.extend(maps);
    fields.concat()
}
fn node(depth: i32) -> r::Node {
    r::Node {
        n: depth,
        child: (depth > 1).then(|| Box::new(node(depth - 1))),
    }
}
fn touch_p_node(m: &p::Node) -> u64 {
    let mut cur = m;
    let mut v = cur.n() as u64;
    while cur.has_child() {
        cur = cur.child();
        v += cur.n() as u64;
    }
    v
}
fn touch_r_node(m: &r::Node) -> u64 {
    let mut cur = m;
    let mut v = cur.n as u64;
    while let Some(c) = &cur.child {
        cur = c;
        v += cur.n as u64;
    }
    v
}
fn touch_p_rpc(m: &p::Rpc) -> u64 {
    m.id() as u64
        + sum(m.method().as_bytes())
        + sum(m.path().as_bytes())
        + sum(m.user().as_bytes())
        + u64::from(m.has_meta())
        + m.meta().id() as u64
        + m.meta().ts() as u64
        + sum(m.meta().trace().as_bytes())
        + m.ids().iter().map(|v| v as u64).sum::<u64>()
        + m.tags().iter().map(|v| sum(v.as_bytes())).sum::<u64>()
        + m.headers()
            .iter()
            .map(|(k, v)| sum(k.as_bytes()) + sum(v.as_bytes()))
            .sum::<u64>()
        + sum(m.extra())
}
fn touch_r_rpc(m: &r::Rpc) -> u64 {
    m.id as u64
        + sum(m.method.as_bytes())
        + sum(m.path.as_bytes())
        + sum(m.user.as_bytes())
        + m.meta.as_ref().map_or(0, |v| {
            1 + v.id as u64 + v.ts as u64 + sum(v.trace.as_bytes())
        })
        + m.ids.iter().map(|&v| v as u64).sum::<u64>()
        + m.tags.iter().map(|v| sum(v.as_bytes())).sum::<u64>()
        + m.headers
            .iter()
            .map(|(k, v)| sum(k.as_bytes()) + sum(v.as_bytes()))
            .sum::<u64>()
        + sum(&m.extra)
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 6, "CASE BACKEND OP ITERATIONS SAMPLES");
    let case = &args[1];
    let backend = &args[2];
    let op = &args[3];
    let iters: u64 = args[4].parse().unwrap();
    let samples: usize = args[5].parse().unwrap();
    assert!(iters > 0 && samples > 0);
    assert!(backend == "pbrs" || backend == "prost");
    assert!(op == "parse_owned" || op == "read_all");
    macro_rules! measure {
        ($p:ty, $r:ty, $fixture:expr, $tp:expr, $tr:expr) => {
            run::<$p, $r, _, _>(case, backend, op, iters, samples, $fixture, $tp, $tr)
        };
    }
    match case.as_str() {
        "empty" => measure!(p::Empty, r::Empty, r::Empty {}, |_| 0, |_| 0),
        "scalars" => measure!(
            p::Scalars,
            r::Scalars,
            r::Scalars {
                id: 7,
                seq: 99,
                ok: true,
                status: 1,
                ts: 123456789,
                lat: 42.5
            },
            |m: &p::Scalars| m.id() as u64
                + u64::from(m.seq())
                + u64::from(m.ok())
                + i32::from(m.status()) as u64
                + m.ts() as u64
                + m.lat().to_bits(),
            |m: &r::Scalars| m.id as u64
                + u64::from(m.seq)
                + u64::from(m.ok)
                + m.status as u64
                + m.ts as u64
                + m.lat.to_bits()
        ),
        "blob_4kib" => measure!(
            p::Blob,
            r::Blob,
            r::Blob {
                payload: (0..4096).map(|i| (i % 251) as u8).collect()
            },
            |m: &p::Blob| sum(m.payload()),
            |m: &r::Blob| sum(&m.payload)
        ),
        "packed_256" => measure!(
            p::Ids,
            r::Ids,
            r::Ids {
                ids: (0..256).collect()
            },
            |m: &p::Ids| m.ids().iter().map(|v| v as u64).sum(),
            |m: &r::Ids| m.ids.iter().map(|&v| v as u64).sum()
        ),
        "nested_8" => measure!(p::Node, r::Node, node(8), touch_p_node, touch_r_node),
        "nested_64" => measure!(p::Node, r::Node, node(64), touch_p_node, touch_r_node),
        "rpc_sparse" | "rpc_mixed" => {
            let fixture = if case == "rpc_sparse" {
                r::Rpc {
                    id: 99,
                    ..Default::default()
                }
            } else {
                r::Rpc {
                    id: 99,
                    method: "Get".into(),
                    path: "/v1/items".into(),
                    user: "ada".into(),
                    meta: Some(r::Meta {
                        id: 77,
                        ts: 123456789,
                        trace: "trace-1234567890".into(),
                    }),
                    ids: (0..8).collect(),
                    tags: ["alpha", "beta", "gamma", "delta"]
                        .map(String::from)
                        .to_vec(),
                    headers: [
                        ("content-type", "application/json"),
                        ("accept", "*/*"),
                        ("user-agent", "probe"),
                        ("host", "example"),
                    ]
                    .map(|(k, v)| (k.into(), v.into()))
                    .into(),
                    extra: b"extra".to_vec(),
                }
            };
            measure!(p::Rpc, r::Rpc, fixture, touch_p_rpc, touch_r_rpc);
        }
        _ => panic!("unknown case {case}"),
    }
}
fn run<P, R, TP, TR>(
    case: &str,
    backend: &str,
    op: &str,
    iters: u64,
    samples: usize,
    reference: R,
    tp: TP,
    tr: TR,
) where
    P: Parse + Serialize,
    R: Message + Default + PartialEq + std::fmt::Debug,
    TP: Fn(&P) -> u64,
    TR: Fn(&R) -> u64,
{
    // Both parse the same deterministic bytes. Map wire order is sorted by key below,
    // so process-random HashMap seeds cannot change the fixture fingerprint.
    let mut wire = reference.encode_to_vec();
    if case == "rpc_mixed" {
        wire = sorted_map_wire(&wire);
    }
    let p = P::parse(&wire).unwrap();
    let r = R::decode(wire.as_slice()).unwrap();
    assert_eq!(&r, &reference, "reference parse");
    assert_eq!(
        R::decode(p.serialize().unwrap().as_slice()).unwrap(),
        reference,
        "pbrs round-trip"
    );
    let checksum = tp(&p);
    assert_eq!(checksum, tr(&r), "byte-exhaustive populated-field checksum");
    let execute = || {
        if backend == "pbrs" {
            let m = P::parse(black_box(&wire)).unwrap();
            if op == "read_all" {
                black_box(tp(&m));
            }
            black_box(m);
        } else {
            let m = R::decode(black_box(wire.as_slice())).unwrap();
            if op == "read_all" {
                black_box(tr(&m));
            }
            black_box(m);
        }
    };
    for _ in 0..(iters / 10).max(1) {
        execute();
    }
    let mut results = Vec::with_capacity(samples);
    for _ in 0..samples {
        #[cfg(feature = "allocations")]
        count::start();
        let cpu_start = cpu_ns();
        let wall_start = std::time::Instant::now();
        for _ in 0..iters {
            execute();
        }
        let wall_ns = wall_start.elapsed().as_nanos();
        let cpu_elapsed = cpu_ns() - cpu_start;
        #[cfg(feature = "allocations")]
        let (allocations, requested_bytes) = count::stop();
        #[cfg(not(feature = "allocations"))]
        let (allocations, requested_bytes) = (None::<u64>, None::<u64>);
        results.push(
            serde_json::json!({"cpu_ns": cpu_elapsed, "wall_ns": wall_ns, "iterations": iters,
            "allocation_calls": allocations, "requested_bytes": requested_bytes}),
        );
    }
    println!(
        "{}",
        serde_json::json!({"schema": "codec-proof/1", "case": case, "backend": backend,
        "operation": op, "input": "borrowed byte slice, each result owned and dropped", "wire_bytes": wire.len(),
        "wire_fnv1a64": hash(&wire), "checksum": checksum, "semantic_prechecks": true,
        "message_storage": if cfg!(feature = "owned-messages") { "owned" } else { "lazy" },
        "allocation_instrumentation": cfg!(feature = "allocations"), "samples": results})
    );
}
