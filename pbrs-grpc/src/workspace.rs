//! Capacity accounting for idle message scratch.
//!
//! These private cache limits are an initial retention policy: at most 4 MiB
//! per buffer and 16 MiB across the process, independently for decode and large
//! encode scratch. They do not bound active output, application-owned messages,
//! small encode scratch, codec state, or total process memory.
//! A full cache only prevents retention; it never fails an RPC.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

const RETAINED_DECODE_CAPACITY: usize = 16 * 1024 * 1024;
const MAX_RETAINED_DECODE_BUFFER: usize = 4 * 1024 * 1024;

static RETAINED_DECODE_BUDGET: Budget =
    Budget::new(RETAINED_DECODE_CAPACITY, MAX_RETAINED_DECODE_BUFFER);

static RETAINED_ENCODE_BUDGET: Budget = Budget::new(16 * 1024 * 1024, 4 * 1024 * 1024);

/// Full capacity of a canonical Vec allocation idle between compressed sends.
/// Store the allocation before its lease so storage is destroyed first.
#[derive(Debug)]
pub(crate) struct RetainedEncodeLease {
    capacity: NonZeroUsize,
}

impl RetainedEncodeLease {
    pub(crate) fn try_acquire(capacity: usize) -> Option<Self> {
        let capacity = NonZeroUsize::new(capacity)?;
        RETAINED_ENCODE_BUDGET
            .try_acquire(capacity.get())
            .then(|| Self { capacity })
    }
}

impl Drop for RetainedEncodeLease {
    fn drop(&mut self) {
        RETAINED_ENCODE_BUDGET.release(self.capacity.get());
    }
}

/// A charge for one idle, uniquely owned decoded-message allocation.
///
/// The caller must pass the allocation's full capacity, rather than its used
/// length. Drop this lease when taking the buffer for active decoding, then
/// acquire a new lease for its actual capacity after reclaiming unique backing.
/// Drop the buffer before its lease when discarding a retained workspace.
#[derive(Debug)]
pub(crate) struct RetainedDecodeLease {
    capacity: usize,
}

impl RetainedDecodeLease {
    /// Reserve idle capacity, or decline retention without failing the call.
    pub(crate) fn try_acquire(capacity: usize) -> Option<Self> {
        if RETAINED_DECODE_BUDGET.try_acquire(capacity) {
            Some(Self { capacity })
        } else {
            None
        }
    }
}

impl Drop for RetainedDecodeLease {
    fn drop(&mut self) {
        RETAINED_DECODE_BUDGET.release(self.capacity);
    }
}

#[derive(Debug)]
struct Budget {
    limit: usize,
    max_buffer_capacity: usize,
    retained_capacity: AtomicUsize,
    #[cfg(any(test, feature = "copy-counts"))]
    diagnostics: Diagnostics,
}

impl Budget {
    const fn new(limit: usize, max_buffer_capacity: usize) -> Self {
        Self {
            limit,
            max_buffer_capacity,
            retained_capacity: AtomicUsize::new(0),
            #[cfg(any(test, feature = "copy-counts"))]
            diagnostics: Diagnostics::new(),
        }
    }

    fn try_acquire(&self, capacity: usize) -> bool {
        #[cfg(any(test, feature = "copy-counts"))]
        self.diagnostics
            .acquire_attempts
            .fetch_add(1, Ordering::Relaxed);
        if capacity == 0 {
            #[cfg(any(test, feature = "copy-counts"))]
            self.diagnostics
                .rejected_zero
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if capacity > self.max_buffer_capacity {
            #[cfg(any(test, feature = "copy-counts"))]
            self.diagnostics
                .rejected_oversize
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }

        // This counter only arbitrates cache capacity. Buffer ownership and
        // publication use normal Rust ownership, so no acquire/release fence is
        // needed here. checked_add also covers injected near-usize::MAX limits.
        let mut used = self.retained_capacity.load(Ordering::Relaxed);
        loop {
            let Some(next) = used
                .checked_add(capacity)
                .filter(|next| *next <= self.limit)
            else {
                #[cfg(any(test, feature = "copy-counts"))]
                self.diagnostics
                    .rejected_budget
                    .fetch_add(1, Ordering::Relaxed);
                return false;
            };
            match self.retained_capacity.compare_exchange_weak(
                used,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    #[cfg(any(test, feature = "copy-counts"))]
                    self.diagnostics.acquired(next);
                    return true;
                }
                Err(current) => used = current,
            }
        }
    }

    fn release(&self, capacity: usize) {
        // Every successful reservation is owned by exactly one non-Clone
        // lease. Only that lease can release it, exactly once, from Drop.
        self.retained_capacity
            .fetch_sub(capacity, Ordering::Relaxed);
        #[cfg(any(test, feature = "copy-counts"))]
        {
            self.diagnostics.released.fetch_add(1, Ordering::Relaxed);
            self.diagnostics
                .retained_buffers
                .fetch_sub(1, Ordering::Relaxed);
        }
    }

    #[cfg(any(test, feature = "copy-counts"))]
    fn snapshot(&self) -> RetainedDecodeCounts {
        let counters = &self.diagnostics;
        RetainedDecodeCounts {
            capacity_limit: self.limit,
            max_buffer_capacity: self.max_buffer_capacity,
            acquire_attempts: counters.acquire_attempts.load(Ordering::Relaxed),
            acquired: counters.acquired.load(Ordering::Relaxed),
            released: counters.released.load(Ordering::Relaxed),
            rejected_zero: counters.rejected_zero.load(Ordering::Relaxed),
            rejected_oversize: counters.rejected_oversize.load(Ordering::Relaxed),
            rejected_budget: counters.rejected_budget.load(Ordering::Relaxed),
            retained_capacity: self.retained_capacity.load(Ordering::Relaxed),
            peak_retained_capacity: counters.peak_retained_capacity.load(Ordering::Relaxed),
            retained_buffers: counters.retained_buffers.load(Ordering::Relaxed),
            peak_retained_buffers: counters.peak_retained_buffers.load(Ordering::Relaxed),
        }
    }
}

#[cfg(any(test, feature = "copy-counts"))]
#[derive(Debug)]
struct Diagnostics {
    acquire_attempts: AtomicUsize,
    acquired: AtomicUsize,
    released: AtomicUsize,
    rejected_zero: AtomicUsize,
    rejected_oversize: AtomicUsize,
    rejected_budget: AtomicUsize,
    peak_retained_capacity: AtomicUsize,
    retained_buffers: AtomicUsize,
    peak_retained_buffers: AtomicUsize,
}

#[cfg(any(test, feature = "copy-counts"))]
impl Diagnostics {
    const fn new() -> Self {
        Self {
            acquire_attempts: AtomicUsize::new(0),
            acquired: AtomicUsize::new(0),
            released: AtomicUsize::new(0),
            rejected_zero: AtomicUsize::new(0),
            rejected_oversize: AtomicUsize::new(0),
            rejected_budget: AtomicUsize::new(0),
            peak_retained_capacity: AtomicUsize::new(0),
            retained_buffers: AtomicUsize::new(0),
            peak_retained_buffers: AtomicUsize::new(0),
        }
    }

    fn acquired(&self, retained_capacity: usize) {
        self.acquired.fetch_add(1, Ordering::Relaxed);
        self.peak_retained_capacity
            .fetch_max(retained_capacity, Ordering::Relaxed);
        let buffers = self.retained_buffers.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_retained_buffers
            .fetch_max(buffers, Ordering::Relaxed);
    }
}

/// Lifetime cache-reservation counters and current retention gauges.
///
/// Fields are sampled separately. Concurrent snapshots can reflect different
/// instants; use a quiescent diagnostic process for interval comparisons. These
/// are reservations, not decode/reuse counts or process-memory measurements.
#[cfg(any(test, feature = "copy-counts"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetainedDecodeCounts {
    /// Maximum idle capacity retained across the process, in bytes.
    pub capacity_limit: usize,
    /// Maximum capacity retained for one stream, in bytes.
    pub max_buffer_capacity: usize,
    /// Reservation attempts, including rejected attempts.
    pub acquire_attempts: usize,
    /// Successful reservations, including repeated reservations of one buffer.
    pub acquired: usize,
    /// Reservations released by their owners.
    pub released: usize,
    /// Attempts to retain an empty allocation.
    pub rejected_zero: usize,
    /// Attempts exceeding the per-stream capacity ceiling.
    pub rejected_oversize: usize,
    /// Attempts exceeding remaining process capacity, including overflow.
    pub rejected_budget: usize,
    /// Current capacity charged to idle buffers, in bytes.
    pub retained_capacity: usize,
    /// Lifetime high-water mark of charged idle capacity, in bytes.
    pub peak_retained_capacity: usize,
    /// Current number of idle buffer reservations.
    pub retained_buffers: usize,
    /// Lifetime high-water mark of the idle reservation counter.
    pub peak_retained_buffers: usize,
}

#[cfg(feature = "copy-counts")]
pub(crate) fn retained_decode_counts() -> RetainedDecodeCounts {
    RETAINED_DECODE_BUDGET.snapshot()
}

/// Independent large encode-scratch ledger. Small scratch and active encoding
/// are excluded. Fields have the same sampling semantics as decode counters.
#[cfg(feature = "copy-counts")]
pub type RetainedEncodeCounts = RetainedDecodeCounts;

#[cfg(feature = "copy-counts")]
pub(crate) fn retained_encode_counts() -> RetainedEncodeCounts {
    RETAINED_ENCODE_BUDGET.snapshot()
}

#[cfg(test)]
mod tests {
    use super::Budget;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn rejected_encode_leases_do_not_release_uncharged_capacity() {
        for capacity in [0, 4 * 1024 * 1024 + 1, usize::MAX] {
            assert!(super::RetainedEncodeLease::try_acquire(capacity).is_none());
            let counts = super::RETAINED_ENCODE_BUDGET.snapshot();
            assert!(counts.retained_capacity <= counts.capacity_limit);
        }
    }

    struct LocalLease<'a> {
        budget: &'a Budget,
        capacity: usize,
    }

    impl Budget {
        fn try_lease(&self, capacity: usize) -> Option<LocalLease<'_>> {
            if self.try_acquire(capacity) {
                Some(LocalLease {
                    budget: self,
                    capacity,
                })
            } else {
                None
            }
        }
    }

    impl Drop for LocalLease<'_> {
        fn drop(&mut self) {
            self.budget.release(self.capacity);
        }
    }

    #[test]
    fn rejects_zero_oversize_and_full_budget_without_charging() {
        let budget = Budget::new(16, 8);
        assert!(budget.try_lease(0).is_none());
        assert!(budget.try_lease(9).is_none());
        let first = budget.try_lease(8);
        let second = budget.try_lease(8);
        assert!(first.is_some());
        assert!(second.is_some());
        assert!(budget.try_lease(1).is_none());
        let counts = budget.snapshot();
        assert_eq!(counts.retained_capacity, 16);
        assert_eq!(counts.acquire_attempts, 5);
        assert_eq!(counts.acquired, 2);
        assert_eq!(counts.rejected_zero, 1);
        assert_eq!(counts.rejected_oversize, 1);
        assert_eq!(counts.rejected_budget, 1);
        drop(first);
        drop(second);
        assert_eq!(budget.snapshot().retained_capacity, 0);
        assert_eq!(budget.snapshot().released, 2);
        assert_eq!(budget.snapshot().retained_buffers, 0);
    }

    #[test]
    fn dropping_a_lease_makes_capacity_available_again() {
        let budget = Budget::new(8, 8);
        let retained = budget.try_lease(8);
        assert!(retained.is_some());
        assert!(budget.try_lease(8).is_none());
        drop(retained);
        let reacquired = budget.try_lease(8);
        assert!(reacquired.is_some());
        drop(reacquired);
        let counts = budget.snapshot();
        assert_eq!(counts.retained_capacity, 0);
        assert_eq!(counts.peak_retained_capacity, 8);
        assert_eq!(counts.acquired, 2);
        assert_eq!(counts.released, 2);
    }

    #[test]
    fn charges_full_allocation_capacity_instead_of_used_length() {
        let mut buffer = Vec::with_capacity(4096);
        buffer.extend_from_slice(b"small");
        let actual_capacity = buffer.capacity();
        let budget = Budget::new(actual_capacity, actual_capacity);
        let lease = budget.try_lease(actual_capacity);
        assert!(lease.is_some());
        assert_eq!(budget.snapshot().retained_capacity, actual_capacity);
        assert!(budget.snapshot().retained_capacity > buffer.len());
        drop(buffer);
        drop(lease);
        assert_eq!(budget.snapshot().retained_capacity, 0);
    }

    #[test]
    fn zero_budget_disables_retention() {
        let budget = Budget::new(0, 8);
        assert!(budget.try_lease(1).is_none());
        assert_eq!(budget.snapshot().retained_capacity, 0);
    }

    #[test]
    fn checked_add_rejects_capacity_overflow() {
        let budget = Budget::new(usize::MAX, usize::MAX);
        let full = budget.try_lease(usize::MAX);
        assert!(full.is_some());
        assert!(budget.try_lease(1).is_none());
        assert_eq!(budget.snapshot().retained_capacity, usize::MAX);
        drop(full);
        assert_eq!(budget.snapshot().retained_capacity, 0);
    }

    #[test]
    fn concurrent_reservations_never_exceed_aggregate_capacity() {
        let budget = Budget::new(256, 64);
        let attempted = Barrier::new(9);
        let release = Barrier::new(9);
        let in_flight = thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let lease = budget.try_lease(64);
                    attempted.wait();
                    release.wait();
                    drop(lease);
                });
            }
            attempted.wait();
            let counts = budget.snapshot();
            release.wait();
            counts
        });
        assert_eq!(in_flight.retained_capacity, 256);
        assert_eq!(in_flight.retained_buffers, 4);
        assert_eq!(in_flight.acquired, 4);
        assert_eq!(in_flight.rejected_budget, 4);
        let finished = budget.snapshot();
        assert_eq!(finished.retained_capacity, 0);
        assert_eq!(finished.retained_buffers, 0);
        assert_eq!(finished.peak_retained_capacity, 256);
        assert_eq!(finished.released, 4);
    }
}
