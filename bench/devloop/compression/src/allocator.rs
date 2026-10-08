//! Benchmark-only forwarding allocator, qualified independently with Miri.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct CountingAlloc;
pub(crate) static ARMED: AtomicBool = AtomicBool::new(false);
pub(crate) static ALLOCS: AtomicU64 = AtomicU64::new(0);
pub(crate) static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every operation forwards the caller's allocation contract unchanged
// to System. Counters never access allocated memory and do not allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: layout is forwarded unchanged to the system allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: pointer and layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: pointer, layout and new size are forwarded unchanged.
        let out = unsafe { System.realloc(ptr, layout, size) };
        if !out.is_null() && ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size as u64, Ordering::Relaxed);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_alignment_zeroing_reallocation_and_deallocation() {
        ALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        ARMED.store(true, Ordering::Relaxed);
        let original = Layout::from_size_align(37, 64).expect("layout");
        // SAFETY: positive valid layout; pointers are checked before access,
        // accessed only within their initialized allocations, and freed once
        // with the matching final layout. Successful realloc invalidates ptr.
        unsafe {
            let ptr = CountingAlloc.alloc_zeroed(original);
            assert!(!ptr.is_null());
            assert_eq!(ptr.addr() % 64, 0);
            assert_eq!(std::slice::from_raw_parts(ptr, 37), &[0; 37]);
            ptr.write_bytes(0x5a, 37);
            let resized = CountingAlloc.realloc(ptr, original, 53);
            assert!(!resized.is_null());
            assert_eq!(resized.addr() % 64, 0);
            assert_eq!(std::slice::from_raw_parts(resized, 37), &[0x5a; 37]);
            CountingAlloc.dealloc(resized, Layout::from_size_align(53, 64).expect("layout"));
        }
        ARMED.store(false, Ordering::Relaxed);
        // Other test-harness activity may share a globally installed allocator.
        assert!(ALLOCS.load(Ordering::Relaxed) >= 2);
        assert!(BYTES.load(Ordering::Relaxed) >= 90);
    }
}
