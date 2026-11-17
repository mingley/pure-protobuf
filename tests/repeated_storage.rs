//! Public storage and allocation contracts for repeated fields.
#![allow(
    unsafe_code,
    reason = "the test allocator forwards unchanged requests to System"
)]

use pbrs::Repeated;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::ops::DerefMut;
use std::rc::Rc;

thread_local! {
    // Const TLS initialization does not allocate. Other harness threads do not
    // contribute to the current measurement.
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn record_allocation(ptr: *mut u8) {
    if !ptr.is_null() {
        ALLOCATIONS
            .try_with(|count| {
                if let Some(value) = count.get() {
                    count.set(Some(value.saturating_add(1)));
                }
            })
            .unwrap_or(());
    }
}

// SAFETY: every operation forwards the caller's pointer and layout unchanged to
// System. Recording accesses only const-initialized TLS and does not allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's caller supplies a valid allocation layout.
        let ptr = unsafe { System.alloc(layout) };
        record_allocation(ptr);
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's caller supplies a valid allocation layout.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        record_allocation(ptr);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies the live System allocation and its layout.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller supplies the live allocation, layout, and valid size.
        let ptr = unsafe { System.realloc(ptr, layout, new_size) };
        record_allocation(ptr);
        ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct Disarm;

impl Drop for Disarm {
    fn drop(&mut self) {
        ALLOCATIONS.try_with(|count| count.set(None)).unwrap_or(());
    }
}

fn measure<R>(action: impl FnOnce() -> R) -> (R, usize) {
    ALLOCATIONS.with(|count| count.set(Some(0)));
    let guard = Disarm;
    let result = action();
    let allocations = ALLOCATIONS.with(|count| count.get().unwrap_or(usize::MAX));
    drop(guard);
    (result, allocations)
}

#[test]
fn empty_mutable_slices_do_not_allocate() {
    let (control, allocations) = measure(|| black_box(Vec::<u64>::with_capacity(37)));
    assert!(
        allocations > 0,
        "allocator positive control must observe a request"
    );
    drop(control);

    let mut values = Repeated::<u64>::new();
    let (len, allocations) = measure(|| black_box(values.as_mut_slice()).len());
    assert_eq!(len, 0);
    assert_eq!(allocations, 0, "fresh empty as_mut_slice must not allocate");
    let (len, allocations) = measure(|| black_box(values.deref_mut()).len());
    assert_eq!(len, 0);
    assert_eq!(allocations, 0, "empty DerefMut must not allocate");

    values.push(7);
    values.clear();
    let (len, allocations) = measure(|| black_box(values.as_mut_slice()).len());
    assert_eq!(len, 0);
    assert_eq!(
        allocations, 0,
        "cleared empty as_mut_slice must not allocate"
    );

    let mut zst = Repeated::<()>::new();
    let (len, allocations) = measure(|| black_box(zst.as_mut_slice()).len());
    assert_eq!((len, allocations), (0, 0));
    zst.push(());
    let (len, allocations) = measure(|| black_box(zst.as_mut_slice()).len());
    assert_eq!((len, allocations), (1, 0));

    #[repr(align(256))]
    struct Aligned;
    let mut aligned = Repeated::<Aligned>::new();
    let (alignment, allocations) = measure(|| {
        let slice = black_box(aligned.as_mut_slice());
        slice.as_ptr().align_offset(std::mem::align_of::<Aligned>())
    });
    assert_eq!((alignment, allocations), (0, 0));
}

#[test]
fn mutable_slices_preserve_buffers_and_reserved_capacity() {
    let mut original = Vec::with_capacity(23);
    original.extend([3_u64, 5, 7]);
    let ptr = original.as_ptr();
    let capacity = original.capacity();
    let mut values = Repeated::from_vec(original);
    let (_, allocations) = measure(|| {
        for value in black_box(values.as_mut_slice()) {
            *value += 1;
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(values.as_slice(), &[4, 6, 8]);
    let original = values.into_vec();
    assert_eq!(original.as_ptr(), ptr);
    assert_eq!(original.capacity(), capacity);

    let mut reserved = Repeated::<u64>::new();
    reserved.reserve(23);
    let (ptr, allocations) = measure(|| black_box(reserved.as_mut_slice()).as_ptr());
    assert_eq!(allocations, 0);
    assert!(reserved.is_empty());
    let (_, allocations) = measure(|| reserved.push(11));
    assert_eq!(
        allocations, 0,
        "empty slice must retain the reserved buffer"
    );
    assert_eq!(reserved.as_slice().as_ptr(), ptr);
    let original = reserved.into_vec();
    assert!(original.capacity() >= 23);
    assert_eq!(original.as_ptr(), ptr);
}

#[test]
fn aligned_values_are_mutated_cloned_and_dropped_once() {
    #[derive(Clone)]
    #[repr(align(128))]
    struct Element {
        value: u64,
        drops: Rc<Cell<usize>>,
    }

    impl Drop for Element {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let mut values = Repeated::from_vec(vec![
        Element {
            value: 1,
            drops: Rc::clone(&drops),
        },
        Element {
            value: 2,
            drops: Rc::clone(&drops),
        },
    ]);
    let ptr = values.as_slice().as_ptr();
    let (_, allocations) = measure(|| {
        for value in values.as_mut_slice() {
            value.value += 10;
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(values.as_slice().as_ptr(), ptr);
    assert_eq!(ptr.align_offset(std::mem::align_of::<Element>()), 0);
    let cloned = values.clone();
    values.clear();
    assert_eq!(drops.get(), 2);
    assert_eq!(
        cloned.iter().map(|value| value.value).collect::<Vec<_>>(),
        [11, 12]
    );
    drop(cloned);
    assert_eq!(drops.get(), 4);
}
