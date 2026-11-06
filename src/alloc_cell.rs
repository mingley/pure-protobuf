//! Publication cell for the explicit alloc-only runtime profile.
//!
//! This is a racing memoization cell, not an exactly-once initializer. Several
//! callers can execute constructors and allocate candidate boxes; one candidate
//! is published and the others are dropped. Each nonzero-sized candidate uses
//! an additional `Box<T>` allocation. Constructor, allocator, and destructor behavior
//! is not claimed to be lock-free or safe inside an interrupt handler.

extern crate alloc;

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

/// An owned value published once per generation, with racing initialization.
///
/// Shared access never mutates or removes a published value. Extraction and
/// mutation require exclusive access to the cell. A value remains at its boxed
/// address until exclusive extraction or destruction of the cell.
pub struct PublicationCell<T> {
    value: AtomicPtr<T>,
    owns: PhantomData<T>,
}

// SAFETY: moving the cell transfers sole ownership of any stored T. Shared
// references cannot outlive the borrow that prevents moving the cell. T: Send
// permits ownership (including eventual destruction) to cross threads.
unsafe impl<T: Send> Send for PublicationCell<T> {}

// SAFETY: a pointer is published only after T is completely constructed, with
// Release/Acquire synchronization. Shared references never change or retire T;
// exclusive mutation/extraction cannot overlap them. T: Sync permits shared
// reads, and T: Send permits one thread's value to be destroyed by another.
unsafe impl<T: Send + Sync> Sync for PublicationCell<T> {}

// AtomicPtr's automatic unwind traits describe its raw pointer, not the owned
// value's contract. Match OnceLock explicitly rather than inherit those bounds.
impl<T: UnwindSafe> UnwindSafe for PublicationCell<T> {}
impl<T: RefUnwindSafe + UnwindSafe> RefUnwindSafe for PublicationCell<T> {}

impl<T> PublicationCell<T> {
    /// Construct an empty cell without allocating.
    pub const fn new() -> Self {
        Self {
            value: AtomicPtr::new(ptr::null_mut()),
            owns: PhantomData,
        }
    }

    /// Read the published value, without running a constructor.
    pub fn get(&self) -> Option<&T> {
        let value = self.value.load(Ordering::Acquire);
        if value.is_null() {
            None
        } else {
            // SAFETY: every non-null pointer came from Box::into_raw after
            // initialization. Acquire observes the publishing Release. The
            // shared borrow prevents extraction/drop throughout this reference.
            Some(unsafe { &*value })
        }
    }

    /// Read the value or construct and attempt to publish a boxed candidate.
    ///
    /// Concurrent or reentrant constructors can run more than once. Each failed
    /// CAS drops only that call's unpublished candidate, exactly once. All
    /// successful returns within a generation refer to the same published value.
    /// A constructor panic leaves the cell available for another initializer.
    pub fn get_or_init(&self, initialize: impl FnOnce() -> T) -> &T {
        if let Some(value) = self.get() {
            return value;
        }
        let candidate = Box::into_raw(Box::new(initialize()));
        let published = match self.value.compare_exchange(
            ptr::null_mut(),
            candidate,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => candidate,
            Err(winner) => {
                // SAFETY: failed CAS did not publish candidate, so this call
                // still owns its original Box uniquely. No other pointer is
                // reconstructed or dropped here, including if T::drop panics.
                drop(unsafe { Box::from_raw(candidate) });
                winner
            }
        };
        // SAFETY: successful CAS transfers candidate to the cell; failed CAS
        // returns a non-null winner and acquires its initialization. While this
        // shared borrow lives, no exclusive take/drop can retire that value.
        unsafe { &*published }
    }

    /// Publish an owned value, or return it unchanged if another value won.
    ///
    /// This can race constructors that have not published yet. Failure returns
    /// ownership to the caller without dropping the supplied value.
    pub fn set(&self, value: T) -> Result<(), T> {
        if self.get().is_some() {
            return Err(value);
        }
        let candidate = Box::into_raw(Box::new(value));
        match self.value.compare_exchange(
            ptr::null_mut(),
            candidate,
            Ordering::Release,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(_) => {
                // SAFETY: failed CAS did not publish this call's candidate.
                // It remains our unique Box, whose value moves to the caller;
                // reconstructing no other pointer leaves the winner intact.
                Err(*unsafe { Box::from_raw(candidate) })
            }
        }
    }

    /// Mutably borrow an initialized value through exclusive cell access.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        let value = *self.value.get_mut();
        if value.is_null() {
            None
        } else {
            // SAFETY: &mut self excludes all shared references and operations.
            // The non-null pointer is still the uniquely cell-owned Box<T>.
            Some(unsafe { &mut *value })
        }
    }

    /// Extract ownership and leave an empty cell that can be initialized again.
    pub fn take(&mut self) -> Option<T> {
        let value = core::mem::replace(self.value.get_mut(), ptr::null_mut());
        if value.is_null() {
            None
        } else {
            // SAFETY: exclusive access excludes readers and publishers. Nulling
            // the slot first transfers its unique Box ownership to this call;
            // the cell's Drop cannot free it again. Moving T out frees the box.
            Some(*unsafe { Box::from_raw(value) })
        }
    }

    /// Consume the cell and return its value without cloning it.
    pub fn into_inner(mut self) -> Option<T> {
        self.take()
    }
}

impl<T> Default for PublicationCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<T> for PublicationCell<T> {
    fn from(value: T) -> Self {
        Self {
            value: AtomicPtr::new(Box::into_raw(Box::new(value))),
            owns: PhantomData,
        }
    }
}

impl<T> Drop for PublicationCell<T> {
    fn drop(&mut self) {
        let value = core::mem::replace(self.value.get_mut(), ptr::null_mut());
        if !value.is_null() {
            // SAFETY: final exclusive access excludes live references and
            // concurrent operations. This non-null Box belongs only to the
            // cell and is reconstructed exactly once, even if T::drop panics.
            drop(unsafe { Box::from_raw(value) });
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::disallowed_methods,
        reason = "owned publication, intentional panic and thread contention controls"
    )]

    extern crate std;

    use self::std::panic::{AssertUnwindSafe, catch_unwind};
    use self::std::sync::Barrier;
    use self::std::thread;
    use super::PublicationCell;
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Tracked {
        value: Box<usize>,
        drops: Arc<AtomicUsize>,
        panic_on_drop: bool,
    }

    impl Tracked {
        fn new(value: usize, drops: &Arc<AtomicUsize>) -> Self {
            Self {
                value: Box::new(value),
                drops: Arc::clone(drops),
                panic_on_drop: false,
            }
        }
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
            assert!(!self.panic_on_drop, "intentional returned value drop panic");
        }
    }

    #[test]
    fn seeded_from_moves_non_clone_value_without_early_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let value = Tracked::new(7, &drops);
        let pointer = value.value.as_ref() as *const usize;
        let cell = PublicationCell::from(value);
        let published = cell.get_or_init(|| panic!("seeded constructor ran"));
        assert_eq!(published.value.as_ref() as *const usize, pointer);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        let extracted = cell.into_inner().expect("seeded value");
        assert_eq!(extracted.value.as_ref() as *const usize, pointer);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(extracted);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failed_set_returns_original_owned_value_and_keeps_winner() {
        let drops = Arc::new(AtomicUsize::new(0));
        let cell = PublicationCell::new();
        cell.set(Tracked::new(1, &drops)).expect("first set");
        let winner = cell.get().expect("winner") as *const Tracked;
        let value = Tracked::new(2, &drops);
        let pointer = value.value.as_ref() as *const usize;
        let returned = cell.set(value).expect_err("already initialized");
        assert_eq!(returned.value.as_ref() as *const usize, pointer);
        assert_eq!(cell.get().expect("same winner") as *const Tracked, winner);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(returned);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(cell);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn racing_set_returns_every_losing_owner_without_dropping_it() {
        const CALLERS: usize = 4;
        let cell = Arc::new(PublicationCell::new());
        let barrier = Arc::new(Barrier::new(CALLERS));
        let drops = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for id in 0..CALLERS {
            let (cell, barrier, drops) =
                (Arc::clone(&cell), Arc::clone(&barrier), Arc::clone(&drops));
            threads.push(thread::spawn(move || {
                let value = Tracked::new(id, &drops);
                barrier.wait();
                (id, cell.set(value))
            }));
        }
        let mut winners = 0;
        let mut returned = Vec::new();
        for thread in threads {
            let (id, result) = thread.join().expect("setter thread");
            match result {
                Ok(()) => {
                    winners += 1;
                    assert_eq!(*cell.get().expect("published").value, id);
                }
                Err(value) => {
                    assert_eq!(*value.value, id);
                    returned.push(value);
                }
            }
        }
        assert_eq!(winners, 1);
        assert_eq!(returned.len(), CALLERS - 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(returned);
        assert_eq!(drops.load(Ordering::Relaxed), CALLERS - 1);
        drop(cell);
        assert_eq!(drops.load(Ordering::Relaxed), CALLERS);
    }

    #[test]
    fn set_can_publish_while_another_constructor_is_running() {
        let cell = Arc::new(PublicationCell::new());
        let started = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let drops = Arc::new(AtomicUsize::new(0));
        let worker = {
            let (cell, started, resume, drops) = (
                Arc::clone(&cell),
                Arc::clone(&started),
                Arc::clone(&resume),
                Arc::clone(&drops),
            );
            thread::spawn(move || {
                *cell
                    .get_or_init(|| {
                        started.wait();
                        resume.wait();
                        Tracked::new(2, &drops)
                    })
                    .value
            })
        };
        started.wait();
        cell.set(Tracked::new(1, &drops)).expect("setter wins");
        resume.wait();
        assert_eq!(worker.join().expect("initializer thread"), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        drop(cell);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn returned_value_drop_panic_does_not_retire_the_winner() {
        let drops = Arc::new(AtomicUsize::new(0));
        let cell = PublicationCell::from(Tracked::new(1, &drops));
        let mut loser = Tracked::new(2, &drops);
        loser.panic_on_drop = true;
        let returned = cell.set(loser).expect_err("occupied");
        assert!(catch_unwind(AssertUnwindSafe(|| drop(returned))).is_err());
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(*cell.get().expect("surviving winner").value, 1);
        drop(cell);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn set_take_and_from_preserve_zero_sized_logical_ownership() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct ZeroSized;
        impl Drop for ZeroSized {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut cell = PublicationCell::from(ZeroSized);
        let returned = cell.set(ZeroSized).expect_err("ZST loser");
        assert_eq!(DROPS.load(Ordering::Relaxed), 0);
        drop(returned);
        assert_eq!(DROPS.load(Ordering::Relaxed), 1);
        drop(cell.take().expect("ZST extracted"));
        assert_eq!(DROPS.load(Ordering::Relaxed), 2);
        assert!(cell.set(ZeroSized).is_ok());
        drop(cell);
        assert_eq!(DROPS.load(Ordering::Relaxed), 3);
    }
}
