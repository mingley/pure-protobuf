//! Borrowed map views preserve last-wins behavior without temporary collections.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::let_underscore_must_use,
    reason = "tests assert map semantics and deterministic thread-local allocation counts"
)]

use pbrs::{Map, MapView, ProtoString};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountedSystem;

#[expect(
    unsafe_code,
    reason = "test allocator forwards unchanged layouts and pointers to System"
)]
unsafe impl GlobalAlloc for CountedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.try_with(Cell::get).unwrap_or_default() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if TRACKING.try_with(Cell::get).unwrap_or_default() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if TRACKING.try_with(Cell::get).unwrap_or_default() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountedSystem = CountedSystem;

fn allocations(operation: impl FnOnce()) -> usize {
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            let _ = TRACKING.try_with(|tracking| tracking.set(false));
        }
    }
    let before = ALLOCATIONS.with(Cell::get);
    TRACKING.with(|tracking| tracking.set(true));
    let scope = Scope;
    operation();
    drop(scope);
    ALLOCATIONS.with(Cell::get) - before
}

fn expected_last_wins(wire: &[(i32, i32)]) -> Vec<(i32, i32)> {
    wire.iter()
        .enumerate()
        .filter(|(i, (key, _))| wire[i + 1..].iter().all(|(later, _)| later != key))
        .map(|(_, pair)| *pair)
        .collect()
}

#[test]
fn typed_view_iteration_preserves_order_valid_hints_and_exhaustion() {
    for len in [0, 1, 4, 8, 15, 16, 17, 64, 256] {
        let wire: Vec<_> = (0..len).map(|i| (i % 7, i * 10)).collect();
        let expected = expected_last_wins(&wire);
        let mut map = Map::new();
        for &(key, value) in &wire {
            map.push_entry(key, value);
        }
        for view in [map.as_view(), MapView::from_slice(&wire)] {
            let mut iter = view.iter();
            for (i, pair) in expected.iter().enumerate() {
                let remaining = expected.len() - i;
                let (lower, upper) = iter.size_hint();
                assert!(lower <= remaining);
                assert!(upper.is_some_and(|bound| bound >= remaining));
                assert_eq!(iter.next(), Some(*pair));
            }
            assert_eq!(iter.size_hint(), (0, Some(0)));
            assert_eq!(iter.next(), None);
            assert_eq!(iter.next(), None);
            assert_eq!(view.into_iter().collect::<Vec<_>>(), expected);
            assert_eq!(
                view.keys().collect::<Vec<_>>(),
                expected.iter().map(|p| p.0).collect::<Vec<_>>()
            );
            assert_eq!(
                view.values().collect::<Vec<_>>(),
                expected.iter().map(|p| p.1).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn typed_view_iteration_does_not_allocate_a_projection_buffer() {
    for len in [0, 1, 4, 8, 15, 16, 64, 256] {
        let mut map = Map::new();
        for i in 0..len {
            map.push_entry(i % 7, i * 10);
        }
        let expected_sum: i32 = map.iter().map(|(_, value)| *value).sum();
        // Build a large-map index before tracking. The index is retained;
        // every subsequent iterator borrows it without another allocation.
        std::hint::black_box(map.len());
        let count = allocations(|| {
            let mut sum = 0;
            for (_, value) in map.as_view() {
                sum += value;
            }
            assert_eq!(sum, expected_sum);
            let mut first = map.as_view().iter();
            std::hint::black_box(first.next());
            std::hint::black_box(first.size_hint());
            drop(first);
            std::hint::black_box(map.as_view().keys().count());
            std::hint::black_box(map.as_view().values().count());
        });
        assert_eq!(count, 0, "length {len}");
    }
}

#[test]
fn borrowed_string_views_survive_iterator_drop_without_copying() {
    let key = "key-".repeat(32);
    let value = "value-".repeat(32);
    let mut map: Map<ProtoString, ProtoString> = Map::new();
    map.push_entry(
        ProtoString::from(key.as_str()),
        ProtoString::from(value.as_str()),
    );
    let original = map
        .get(&ProtoString::from(key.as_str()))
        .unwrap()
        .as_bytes()
        .as_ptr();
    let mut iter = map.as_view().iter();
    let (key_view, value_view) = iter.next().unwrap();
    drop(iter);
    assert_eq!(key_view.as_bytes(), key.as_bytes());
    assert_eq!(value_view.as_bytes(), value.as_bytes());
    assert_eq!(value_view.as_bytes().as_ptr(), original);
    assert_eq!(
        map.as_view().iter().next().unwrap().1.as_bytes().as_ptr(),
        original
    );
}

#[test]
fn small_borrowed_key_lookups_do_not_clone_string_storage() {
    let key = "long-borrowed-key-".repeat(16);
    let mut map: Map<ProtoString, i32> = Map::new();
    map.push_entry(ProtoString::from(key.as_str()), 1);
    map.push_entry(ProtoString::from("other"), 2);
    map.push_entry(ProtoString::from(key.as_str()), 3);
    let count = allocations(|| {
        assert_eq!(map.as_view().get(key.as_str()), Some(3));
        assert_eq!(map.as_view().get("absent"), None);
    });
    assert_eq!(count, 0);
}

#[test]
fn borrowed_lookup_stays_correct_across_index_activation_and_invalidation() {
    let key = "indexed-long-key-".repeat(8);
    let mut map: Map<ProtoString, i32> = Map::new();
    map.push_entry(ProtoString::from(key.as_str()), 77);
    for index in 1..15 {
        map.push_entry(ProtoString::from(format!("key-{index}")), index);
    }
    assert_eq!(
        allocations(|| assert_eq!(map.as_view().get(key.as_str()), Some(77))),
        0
    );
    map.push_entry(ProtoString::from("last"), 99);
    assert_eq!(map.len(), 16);
    assert_eq!(map.as_view().get(key.as_str()), Some(77));
    assert_eq!(map.remove(&ProtoString::from("last")), Some(99));
    assert_eq!(map.len(), 15);
    assert_eq!(
        allocations(|| assert_eq!(map.as_view().get(key.as_str()), Some(77))),
        0
    );
    map.push_entry(ProtoString::from(key.as_str()), 88);
    assert_eq!(map.as_view().get(key.as_str()), Some(88));
    assert_eq!(map.remove(&ProtoString::from(key.as_str())), Some(88));
    assert_eq!(
        allocations(|| assert_eq!(map.as_view().get(key.as_str()), Some(77))),
        0
    );
}

#[test]
fn borrowed_lookup_preserves_legacy_non_utf8_keys() {
    let key = [0xff; 64];
    let mut map: Map<ProtoString, i32> = Map::new();
    map.push_entry(ProtoString::from_bytes(&key), 7);
    assert_eq!(
        allocations(|| {
            assert_eq!(map.as_view().get(pbrs::ProtoStr::from_bytes(&key)), Some(7));
        }),
        0
    );
}

#[test]
fn supported_typed_iterators_keep_send_and_sync() {
    fn send_sync<T: Send + Sync>(_: T) {}
    send_sync(Map::<i32, i32>::new().as_view().iter());
    send_sync(Map::<ProtoString, ProtoString>::new().as_view().iter());
    send_sync(
        Map::<pbrs::rt::LazyStr, pbrs::rt::LazyStr>::new()
            .as_view()
            .iter(),
    );
}
