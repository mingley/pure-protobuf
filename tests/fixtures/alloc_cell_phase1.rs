//! Ownership, synchronization, extraction and unwind controls for the shipping cell.

use crate::alloc_cell::PublicationCell;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

#[derive(Debug)]
struct Tracked {
    id: usize,
    drops: Arc<AtomicUsize>,
    panic_on_drop: bool,
}

impl Tracked {
    fn new(id: usize, drops: &Arc<AtomicUsize>) -> Self {
        Self {
            id,
            drops: Arc::clone(drops),
            panic_on_drop: false,
        }
    }

    fn panics(id: usize, drops: &Arc<AtomicUsize>) -> Self {
        Self {
            id,
            drops: Arc::clone(drops),
            panic_on_drop: true,
        }
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
        assert!(!self.panic_on_drop, "intentional candidate drop panic");
    }
}

#[test]
fn empty_and_const_construction() {
    static EMPTY: PublicationCell<u32> = PublicationCell::new();
    assert!(EMPTY.get().is_none());
    let mut cell = PublicationCell::<String>::default();
    assert!(cell.get().is_none());
    assert!(cell.get_mut().is_none());
    assert!(cell.take().is_none());
    assert!(cell.into_inner().is_none());
}

#[test]
fn warm_get_skips_constructor_and_keeps_address() {
    let cell = PublicationCell::new();
    let first = cell.get_or_init(|| String::from("published"));
    let second = cell.get_or_init(|| panic!("warm constructor ran"));
    assert!(std::ptr::eq(first, second));
    assert!(std::ptr::eq(first, cell.get().unwrap()));
    assert_eq!(first, "published");
}

#[test]
fn get_mut_changes_the_published_value_exclusively() {
    let mut cell = PublicationCell::new();
    assert_eq!(cell.get_or_init(|| vec![1_u32, 2]), &[1, 2]);
    cell.get_mut().unwrap().push(3);
    assert_eq!(cell.get().unwrap(), &[1, 2, 3]);
}

#[test]
fn take_moves_non_clone_value_reinitializes_and_preserves_drop_ownership() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut cell = PublicationCell::new();
    assert_eq!(cell.get_or_init(|| Tracked::new(1, &drops)).id, 1);
    let extracted = cell.take().unwrap();
    assert_eq!(extracted.id, 1);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(cell.get().is_none());
    assert!(cell.get_mut().is_none());
    assert!(cell.take().is_none());
    assert_eq!(cell.get_or_init(|| Tracked::new(2, &drops)).id, 2);
    drop(extracted);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(cell);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn into_inner_moves_non_clone_value_and_does_not_drop_it_early() {
    let drops = Arc::new(AtomicUsize::new(0));
    let cell = PublicationCell::new();
    cell.get_or_init(|| Tracked::new(9, &drops));
    let value = cell.into_inner().unwrap();
    assert_eq!(value.id, 9);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(value);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn dropping_full_cell_drops_exactly_one_owned_value() {
    let drops = Arc::new(AtomicUsize::new(0));
    let cell = PublicationCell::new();
    cell.get_or_init(|| Tracked::new(8, &drops));
    drop(cell);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn panicking_constructor_leaves_empty_cell_for_retry() {
    let cell = PublicationCell::new();
    assert!(catch_unwind(|| cell.get_or_init(|| panic!("constructor failed"))).is_err());
    assert!(cell.get().is_none());
    assert_eq!(*cell.get_or_init(|| 17_u32), 17);
}

#[test]
fn extraction_then_panicking_drop_does_not_poison_next_generation() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut cell = PublicationCell::new();
    cell.get_or_init(|| Tracked::panics(1, &drops));
    let extracted = cell.take().unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| drop(extracted))).is_err());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert!(cell.get().is_none());
    cell.get_or_init(|| Tracked::new(2, &drops));
    drop(cell);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn full_cell_destructor_panic_does_not_double_drop_or_leak_box() {
    let drops = Arc::new(AtomicUsize::new(0));
    let cell = PublicationCell::new();
    cell.get_or_init(|| Tracked::panics(3, &drops));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(cell))).is_err());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_cas_drops_one_loser_and_all_callers_read_one_winner() {
    const CALLERS: usize = 4;
    let cell = Arc::new(PublicationCell::new());
    let barrier = Arc::new(Barrier::new(CALLERS));
    let constructors = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for id in 0..CALLERS {
        let cell = Arc::clone(&cell);
        let barrier = Arc::clone(&barrier);
        let constructors = Arc::clone(&constructors);
        let drops = Arc::clone(&drops);
        threads.push(thread::spawn(move || {
            let value = cell.get_or_init(|| {
                constructors.fetch_add(1, Ordering::Relaxed);
                // No candidate can publish until every caller has entered its
                // initializer, so all four allocations/CAS attempts are real.
                barrier.wait();
                Tracked::new(id, &drops)
            });
            (value.id, std::ptr::from_ref(value).addr())
        }));
    }
    let values: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(values.iter().all(|value| *value == values[0]));
    assert_eq!(constructors.load(Ordering::Relaxed), CALLERS);
    assert_eq!(drops.load(Ordering::Relaxed), CALLERS - 1);
    assert_eq!(cell.get().unwrap().id, values[0].0);
    drop(cell);
    assert_eq!(drops.load(Ordering::Relaxed), CALLERS);
}

#[test]
fn panicking_loser_drop_keeps_published_winner_valid() {
    let cell = Arc::new(PublicationCell::new());
    let barrier = Arc::new(Barrier::new(2));
    let drops = Arc::new(AtomicUsize::new(0));
    let worker = {
        let cell = Arc::clone(&cell);
        let barrier = Arc::clone(&barrier);
        let drops = Arc::clone(&drops);
        thread::spawn(move || {
            cell.get_or_init(|| {
                barrier.wait();
                while cell.get().is_none() {
                    thread::yield_now();
                }
                Tracked::panics(2, &drops)
            })
            .id
        })
    };
    barrier.wait();
    assert_eq!(cell.get_or_init(|| Tracked::new(1, &drops)).id, 1);
    assert!(worker.join().is_err());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(cell.get().unwrap().id, 1);
    drop(cell);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn acquire_get_observes_complete_non_atomic_payload() {
    let cell = Arc::new(PublicationCell::new());
    let reader = {
        let cell = Arc::clone(&cell);
        thread::spawn(move || {
            let payload: &Vec<u32> = loop {
                if let Some(payload) = cell.get() {
                    break payload;
                }
                thread::yield_now();
            };
            assert_eq!(payload.len(), 32);
            for (index, value) in payload.iter().enumerate() {
                assert_eq!(*value, u32::try_from(index).unwrap() * 7 + 5);
            }
        })
    };
    // Construction happens after the reader was spawned; no separate flag,
    // lock, channel, or barrier publishes this payload to that reader.
    cell.get_or_init(|| (0..32).map(|n| n * 7 + 5).collect::<Vec<u32>>());
    reader.join().unwrap();
}

#[test]
fn synchronized_exclusive_owner_can_take_and_publish_a_second_generation() {
    let cell = Arc::new(PublicationCell::new());
    let first = {
        let cell = Arc::clone(&cell);
        thread::spawn(move || assert_eq!(*cell.get_or_init(|| 11_u32), 11))
    };
    first.join().unwrap();
    let mut cell = match Arc::try_unwrap(cell) {
        Ok(cell) => cell,
        Err(_) => panic!("all shared owners should have ended"),
    };
    assert_eq!(cell.take(), Some(11));
    let cell = Arc::new(cell);
    let second = {
        let cell = Arc::clone(&cell);
        thread::spawn(move || assert_eq!(*cell.get_or_init(|| 22), 22))
    };
    second.join().unwrap();
    assert_eq!(cell.get(), Some(&22));
}

#[test]
fn bounded_reentrant_constructor_returns_the_first_published_value() {
    let cell = PublicationCell::new();
    let value = cell.get_or_init(|| {
        assert_eq!(*cell.get_or_init(|| 29_u32), 29);
        31
    });
    assert_eq!(*value, 29);
}

#[test]
fn zero_sized_values_own_one_logical_value_per_generation() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Zero;
    impl Drop for Zero {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }
    assert_eq!(std::mem::size_of::<Zero>(), 0);
    let mut cell = PublicationCell::new();
    let first = cell.get_or_init(|| Zero);
    assert!(std::ptr::eq(first, cell.get().unwrap()));
    let value = cell.take().unwrap();
    assert!(cell.get().is_none());
    assert_eq!(DROPS.load(Ordering::Relaxed), 0);
    drop(value);
    assert_eq!(DROPS.load(Ordering::Relaxed), 1);
    cell.get_or_init(|| Zero);
    drop(cell);
    assert_eq!(DROPS.load(Ordering::Relaxed), 2);
}

#[test]
fn racing_zero_sized_candidates_drop_losers_even_when_addresses_match() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct RacingZero;
    impl Drop for RacingZero {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }
    let cell = Arc::new(PublicationCell::new());
    let barrier = Arc::new(Barrier::new(2));
    let worker = {
        let cell = Arc::clone(&cell);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            cell.get_or_init(|| {
                barrier.wait();
                RacingZero
            });
        })
    };
    cell.get_or_init(|| {
        barrier.wait();
        RacingZero
    });
    worker.join().unwrap();
    assert_eq!(DROPS.load(Ordering::Relaxed), 1);
    drop(cell);
    assert_eq!(DROPS.load(Ordering::Relaxed), 2);
}

#[test]
fn over_aligned_payload_keeps_alignment_through_mutation_and_extraction() {
    #[repr(align(4096))]
    struct Aligned([u8; 37]);
    let mut cell = PublicationCell::new();
    let value = cell.get_or_init(|| Aligned([7; 37]));
    assert_eq!(std::ptr::from_ref(value).addr() % 4096, 0);
    cell.get_mut().unwrap().0[36] = 9;
    let value = cell.take().unwrap();
    assert_eq!(value.0[36], 9);
    assert!(cell.get().is_none());
}

#[test]
fn over_aligned_zero_sized_payload_does_not_need_a_real_allocation() {
    #[repr(align(4096))]
    struct AlignedZero;
    let mut cell = PublicationCell::new();
    assert_eq!(std::mem::size_of::<AlignedZero>(), 0);
    assert_eq!(
        std::ptr::from_ref(cell.get_or_init(|| AlignedZero)).addr() % 4096,
        0
    );
    assert!(cell.get_mut().is_some());
    assert!(cell.into_inner().is_some());
}

#[test]
fn send_cell_supports_a_send_but_non_sync_payload() {
    let cell = PublicationCell::new();
    cell.get_or_init(|| Cell::new(3_u32));
    let result = thread::spawn(move || {
        let mut cell = cell;
        cell.get_mut().unwrap().set(5);
        cell.into_inner().unwrap().get()
    })
    .join()
    .unwrap();
    assert_eq!(result, 5);
}
