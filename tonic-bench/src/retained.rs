use super::Retained;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

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
pub(super) fn measure<M>(parse: impl FnOnce() -> M) -> Retained {
    let (a0, f0, c0) = alloc_snapshot();
    let msg = parse();
    std::hint::black_box(&msg);
    let (a1, f1, c1) = alloc_snapshot();
    let bytes = a1.saturating_sub(a0).saturating_sub(f1.saturating_sub(f0));
    let allocs = c1.saturating_sub(c0);
    Retained {
        bytes: bytes as u64,
        allocs: allocs as u64,
    }
}
