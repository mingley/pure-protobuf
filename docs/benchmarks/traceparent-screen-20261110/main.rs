use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::TraceContextExt;
use pbrs_grpc::Metadata;
use pbrs_grpc::otel::trace::W3CPropagator;
use std::hint::black_box;

#[cfg(feature = "count_alloc")]
mod allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static REALLOCS: AtomicU64 = AtomicU64::new(0);
    pub struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let result = unsafe { System.alloc(layout) };
            if !result.is_null() {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            result
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let result = unsafe { System.alloc_zeroed(layout) };
            if !result.is_null() {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            result
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let result = unsafe { System.realloc(ptr, layout, size) };
            if !result.is_null() {
                REALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            result
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    pub fn snapshot() -> (u64, u64) {
        (ALLOCS.load(Ordering::Relaxed), REALLOCS.load(Ordering::Relaxed))
    }
}
#[cfg(feature = "count_alloc")]
#[global_allocator]
static GLOBAL: allocations::Counting = allocations::Counting;

#[cfg(feature = "count_alloc")]
fn snapshot() -> (u64, u64) { allocations::snapshot() }
#[cfg(not(feature = "count_alloc"))]
fn snapshot() -> (u64, u64) { (0, 0) }

struct Carrier {
    metadata: Option<Metadata>,
    parent: Option<&'static str>,
    state: Option<&'static str>,
}
impl Extractor for Carrier {
    fn get(&self, key: &str) -> Option<&str> {
        match &self.metadata {
            Some(metadata) => metadata.get(key),
            None => match key {
                "traceparent" => self.parent,
                "tracestate" => self.state,
                _ => None,
            }
        }
    }
    fn keys(&self) -> Vec<&str> {
        vec!["traceparent", "tracestate"]
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let carrier_kind = args.next().expect("carrier");
    let scenario = args.next().expect("scenario");
    let iterations: u64 = args.next().expect("iterations").parse().expect("number");
    let valid = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let invalid = "00-4bf92f3577b34da6a3ce929d0e0e4736-xxx-01";
    let future = "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-a-b-c";
    let parent = match scenario.as_str() {
        "valid" | "state" => Some(valid),
        "invalid" => Some(invalid),
        "future" => Some(future),
        "missing" => None,
        _ => panic!("unknown scenario"),
    };
    let state = (scenario == "state").then_some("rojo=00f067aa0ba902b7,congo=t61rcWkgMzE");
    let metadata = if carrier_kind == "metadata" {
        let mut md = Metadata::new();
        if let Some(parent) = parent { md.insert("traceparent", parent).expect("valid metadata"); }
        if let Some(state) = state { md.insert("tracestate", state).expect("valid metadata"); }
        Some(md)
    } else {
        assert_eq!(carrier_kind, "direct");
        None
    };
    let carrier = Carrier { metadata, parent, state };
    let propagator = W3CPropagator::new();
    let before = snapshot();
    let mut checksum = 0u64;
    for _ in 0..iterations {
        let context = propagator.extract(black_box(&carrier));
        checksum += u64::from(black_box(context.span().span_context().is_valid()));
    }
    let after = snapshot();
    println!("{{\"checksum\":{},\"allocations\":{},\"reallocations\":{},\"iterations\":{}}}",
        black_box(checksum), after.0 - before.0, after.1 - before.1, iterations);
}
