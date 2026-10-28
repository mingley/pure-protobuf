//! Resource caps applied to every RPC.
//!
//! Frame lengths are checked from the 5-byte header before allocating carry
//! buffers. Decompression bounds output reservations and probes one extra
//! byte for overflow. Decoder workspace, codec scratch and allocator overhead
//! are separate costs; these caps do not bound total process memory.
//! See [the threat model](crate#threat-model).

use crate::status::Status;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Default inbound message cap: 4 MiB, matching gRPC's cross-language default.
pub const DEFAULT_MAX_DECODING_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Tracks allocated transport buffer bytes against a configured budget.
///
/// Permits are acquired via [`ByteBudgetTracker::try_acquire`] or [`ByteBudgetTracker::acquire`].
/// When a [`BytePermit`] is dropped or explicitly released, the allocated bytes are returned
/// to the tracker.
#[derive(Clone, Debug)]
pub struct ByteBudgetTracker {
    inner: Arc<ByteBudgetInner>,
}

#[derive(Debug)]
struct ByteBudgetInner {
    limit: Option<usize>,
    allocated: AtomicUsize,
    peak_allocated: AtomicUsize,
    active_byte_permit_tokens: AtomicUsize,
    peak_active_byte_permit_tokens: AtomicUsize,
}

// Never publish a wrapped value, including briefly between an overflowing
// add and a rollback. Relaxed ordering is sufficient for independent counts;
// ownership and buffer synchronization are managed by the permit itself.
#[inline]
fn checked_atomic_add(counter: &AtomicUsize, bytes: usize) -> Result<usize, usize> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(bytes) else {
            return Err(current);
        };
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(current),
            Err(actual) => current = actual,
        }
    }
}

impl Default for ByteBudgetTracker {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl ByteBudgetTracker {
    /// Create a new byte budget tracker with an optional limit.
    /// `None` indicates an unlimited budget (bytes are tracked, but never rejected).
    #[must_use]
    pub fn new(limit: Option<usize>) -> Self {
        Self {
            inner: Arc::new(ByteBudgetInner {
                limit,
                allocated: AtomicUsize::new(0),
                peak_allocated: AtomicUsize::new(0),
                active_byte_permit_tokens: AtomicUsize::new(0),
                peak_active_byte_permit_tokens: AtomicUsize::new(0),
            }),
        }
    }

    /// Create an unlimited tracker that tracks allocations without rejecting.
    #[must_use]
    pub fn unlimited() -> Self {
        Self::new(None)
    }

    /// Create a tracker capped at `limit` bytes.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self::new(Some(limit))
    }

    /// The configured byte limit, if any.
    #[must_use]
    pub fn limit(&self) -> Option<usize> {
        self.inner.limit
    }

    /// Bytes currently charged to this budget, including any deliberately forgotten permits.
    #[must_use]
    pub fn allocated(&self) -> usize {
        self.inner.allocated.load(Ordering::SeqCst)
    }

    /// High-water mark of bytes charged to this tracker over its lifetime.
    ///
    /// Exact after acquisitions finish; a concurrent read can briefly precede
    /// an acquiring task's peak update. Includes warmup and earlier calls when
    /// a tracker is reused; it is not process RSS or an interval-specific
    /// memory measurement.
    #[must_use]
    pub fn peak_allocated(&self) -> usize {
        self.inner.peak_allocated.load(Ordering::SeqCst)
    }

    /// Live [`BytePermit`] tokens sharing this tracker, including zero-byte permits.
    ///
    /// This counts byte-budget handles, not HTTP/2 streams, active RPCs,
    /// semaphore slots, allocated bytes, or process memory. Merging permits
    /// on the same tracker reduces the token count without releasing bytes.
    #[must_use]
    pub fn active_byte_permit_tokens(&self) -> usize {
        self.inner.active_byte_permit_tokens.load(Ordering::SeqCst)
    }

    /// Lifetime high-water mark of live byte-permit tokens across clones.
    ///
    /// Exact after acquisitions finish; a concurrent read can briefly precede
    /// an acquiring task's peak update. Includes warmup and earlier calls
    /// when a tracker is reused, not just a benchmark measurement interval.
    #[must_use]
    pub fn peak_active_byte_permit_tokens(&self) -> usize {
        self.inner
            .peak_active_byte_permit_tokens
            .load(Ordering::SeqCst)
    }

    /// Remaining bytes before the limit is reached, or `None` if unlimited.
    #[must_use]
    pub fn available(&self) -> Option<usize> {
        self.inner
            .limit
            .map(|lim| lim.saturating_sub(self.allocated()))
    }

    /// Whether there are currently zero bytes allocated.
    #[must_use]
    pub fn is_quiescent(&self) -> bool {
        self.allocated() == 0
    }

    fn acquire_token_count(&self) -> Result<(), Status> {
        // Never publish a wrapped value: another acquisition or release can
        // observe it before a fetch_add overflow is rolled back.
        let prev = checked_atomic_add(&self.inner.active_byte_permit_tokens, 1).map_err(|_| {
            Status::resource_exhausted("transport byte permit token counter overflow")
        })?;
        let next = prev + 1;
        if next
            > self
                .inner
                .peak_active_byte_permit_tokens
                .load(Ordering::Relaxed)
        {
            self.inner
                .peak_active_byte_permit_tokens
                .fetch_max(next, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Try to acquire a permit for `bytes`.
    ///
    /// If the allocation would exceed the configured limit, returns
    /// `Status::resource_exhausted`. Otherwise, returns an RAII [`BytePermit`]
    /// that will release the bytes back to this tracker when dropped.
    pub fn try_acquire(&self, bytes: usize) -> Result<BytePermit, Status> {
        // The unlimited tracker still checks representational overflow.
        match self.inner.limit {
            None => self.acquire_unlimited(bytes),
            Some(limit) => self.acquire_limited(bytes, limit),
        }
    }

    fn acquire_unlimited(&self, bytes: usize) -> Result<BytePermit, Status> {
        if bytes == 0 {
            self.acquire_token_count()?;
            return Ok(BytePermit {
                tracker: Some(self.clone()),
                bytes: 0,
            });
        }
        // A checked atomic update keeps overflow rejection linearizable with
        // concurrent acquisitions and releases; no wrapped value is visible.
        let current = checked_atomic_add(&self.inner.allocated, bytes)
            .map_err(|current| Self::budget_counter_overflow(bytes, current))?;
        let next = current + bytes;
        if let Err(error) = self.acquire_token_count() {
            self.inner.allocated.fetch_sub(bytes, Ordering::Relaxed);
            return Err(error);
        }
        if next > self.inner.peak_allocated.load(Ordering::Relaxed) {
            self.inner.peak_allocated.fetch_max(next, Ordering::Relaxed);
        }
        Ok(BytePermit {
            tracker: Some(self.clone()),
            bytes,
        })
    }

    #[cold]
    #[inline(never)]
    fn acquire_limited(&self, bytes: usize, limit: usize) -> Result<BytePermit, Status> {
        if bytes == 0 {
            self.acquire_token_count()?;
            return Ok(BytePermit {
                tracker: Some(self.clone()),
                bytes: 0,
            });
        }

        let mut current = self.inner.allocated.load(Ordering::SeqCst);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return Err(Self::budget_counter_overflow(bytes, current));
            };
            if next > limit {
                return Err(Self::budget_limit_exceeded(bytes, current, limit));
            }
            match self.inner.allocated.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    if let Err(error) = self.acquire_token_count() {
                        self.release_bytes(bytes);
                        return Err(error);
                    }
                    // A stale low read only causes an extra fetch_max; peaks never decrease.
                    if next > self.inner.peak_allocated.load(Ordering::Relaxed) {
                        self.inner.peak_allocated.fetch_max(next, Ordering::SeqCst);
                    }
                    break;
                }
                Err(actual) => current = actual,
            }
        }

        Ok(BytePermit {
            tracker: Some(self.clone()),
            bytes,
        })
    }

    /// Synonym for [`Self::try_acquire`].
    pub fn acquire(&self, bytes: usize) -> Result<BytePermit, Status> {
        self.try_acquire(bytes)
    }

    /// Overflow text stays out of the acquire loop. The hot path never formats.
    #[cold]
    #[inline(never)]
    fn budget_counter_overflow(bytes: usize, current: usize) -> Status {
        Status::resource_exhausted(format!(
            "transport byte budget counter overflow: requested {bytes} bytes, current allocated {current}"
        ))
    }

    /// Limit text stays out of the acquire loop. The hot path never formats.
    #[cold]
    #[inline(never)]
    fn budget_limit_exceeded(bytes: usize, current: usize, limit: usize) -> Status {
        Status::resource_exhausted(format!(
            "transport byte budget exceeded: requested {bytes} bytes, current allocated {current}, limit {limit}"
        ))
    }

    pub(crate) fn release(&self, bytes: usize) {
        self.release_bytes(bytes);
        self.release_token_count();
    }

    fn release_bytes(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        if self.inner.limit.is_none() {
            let current = self.inner.allocated.fetch_sub(bytes, Ordering::Relaxed);
            assert!(current >= bytes, "byte permit released more than allocated");
            return;
        }
        self.release_bytes_limited(bytes);
    }

    #[cold]
    #[inline(never)]
    fn release_bytes_limited(&self, bytes: usize) {
        let mut current = self.inner.allocated.load(Ordering::SeqCst);
        loop {
            assert!(current >= bytes, "byte permit released more than allocated");
            let next = current - bytes;
            match self.inner.allocated.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn release_token_count(&self) {
        let prev = self
            .inner
            .active_byte_permit_tokens
            .fetch_sub(1, Ordering::Relaxed);
        assert!(prev > 0, "byte permit token counter underflow");
    }
}

/// An RAII permit representing allocated transport buffer bytes.
///
/// When dropped or explicitly released with [`Self::release`], the allocated bytes
/// are deducted from the associated [`ByteBudgetTracker`].
#[derive(Debug)]
pub struct BytePermit {
    tracker: Option<ByteBudgetTracker>,
    bytes: usize,
}

impl BytePermit {
    /// An empty permit representing zero allocated bytes.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            tracker: None,
            bytes: 0,
        }
    }

    /// Number of bytes guarded by this permit.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Explicitly release the permit back to its tracker.
    pub fn release(mut self) {
        if let Some(tracker) = self.tracker.take() {
            tracker.release(self.bytes);
        }
    }

    /// Forget the permit without returning bytes to the tracker.
    ///
    /// The token is no longer active, but its bytes remain charged to the budget.
    pub fn forget(mut self) {
        if let Some(tracker) = self.tracker.take() {
            tracker.release_token_count();
        }
    }

    /// Merge another permit into this one, provided they belong to the same tracker.
    pub fn merge(&mut self, mut other: BytePermit) {
        if other.bytes == 0 {
            return;
        }
        if self.bytes == 0 {
            if let Some(tracker) = self.tracker.take() {
                tracker.release_token_count();
            }
            self.tracker = other.tracker.take();
            self.bytes = other.bytes;
            return;
        }
        if let (Some(t1), Some(t2)) = (&self.tracker, &other.tracker) {
            if Arc::ptr_eq(&t1.inner, &t2.inner) {
                assert!(
                    self.bytes <= usize::MAX - other.bytes,
                    "merged byte permits exceed representable tracker accounting"
                );
                self.bytes += other.bytes;
                other.forget();
            }
        }
    }
}

impl Drop for BytePermit {
    fn drop(&mut self) {
        if let Some(tracker) = self.tracker.take() {
            tracker.release(self.bytes);
        }
    }
}

/// Message-size, compression-work and optional decoder history limits.
///
/// Inbound limits apply to both wire payloads and decompressed protobuf bytes.
/// Outbound limits count uncompressed protobuf bytes. `None` removes the
/// corresponding message-size cap. With Zstd enabled, the history window has
/// a separate 8 MiB default cap, configurable with `with_max_zstd_window`.
/// Each message also admits at most 1,024 compressed frames or gzip members.
///
/// ```
/// # use pbrs_grpc::MessageLimits;
/// let limits = MessageLimits::default();
/// assert_eq!(limits.max_decoding(), Some(4 * 1024 * 1024));
/// assert_eq!(limits.max_encoding(), None);
///
/// let unlimited = MessageLimits::unlimited();
/// assert_eq!(unlimited.max_decoding(), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageLimits {
    max_decoding: Option<usize>,
    max_encoding: Option<usize>,
    max_compression_frames: Option<usize>,
    #[cfg(feature = "zstd")]
    max_zstd_window: Option<u64>,
}

impl Default for MessageLimits {
    /// 4 MiB inbound, unlimited outbound, 1,024 compressed frames per message,
    /// and an 8 MiB Zstd window when enabled.
    ///
    /// Inbound is capped because a peer controls it; outbound is not because
    /// the local service does.
    fn default() -> Self {
        Self {
            max_decoding: Some(DEFAULT_MAX_DECODING_MESSAGE_SIZE),
            max_encoding: None,
            max_compression_frames: Some(1024),
            #[cfg(feature = "zstd")]
            max_zstd_window: Some(8 * 1024 * 1024),
        }
    }
}

impl MessageLimits {
    /// Defaults: 4 MiB inbound, unlimited outbound, 1,024 compressed frames
    /// per message, and an 8 MiB Zstd window when enabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// No message-size or compression-frame count caps.
    ///
    /// With Zstd enabled, this also leaves the history window at the backend's
    /// 100 MiB cap. Use `with_max_zstd_window` to set a smaller cap.
    ///
    /// Only appropriate when every peer is trusted: a single hostile frame
    /// header can then ask for as much memory as `u32::MAX` allows.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_decoding: None,
            max_encoding: None,
            max_compression_frames: None,
            #[cfg(feature = "zstd")]
            max_zstd_window: None,
        }
    }

    /// Cap inbound frame payloads and decompressed messages at `limit` bytes.
    /// Applies to every call shape.
    #[must_use]
    pub fn with_max_decoding(mut self, limit: usize) -> Self {
        self.max_decoding = Some(limit);
        self
    }

    /// Cap outbound messages at `limit` uncompressed bytes.
    /// Applies to every call shape.
    #[must_use]
    pub fn with_max_encoding(mut self, limit: usize) -> Self {
        self.max_encoding = Some(limit);
        self
    }

    /// Lift the inbound cap.
    #[must_use]
    pub fn with_unlimited_decoding(mut self) -> Self {
        self.max_decoding = None;
        self
    }

    /// Lift the outbound cap.
    #[must_use]
    pub fn with_unlimited_encoding(mut self) -> Self {
        self.max_encoding = None;
        self
    }

    /// Inbound cap in bytes.
    #[must_use]
    pub fn max_decoding(self) -> Option<usize> {
        self.max_decoding
    }

    /// Outbound cap in bytes.
    #[must_use]
    pub fn max_encoding(self) -> Option<usize> {
        self.max_encoding
    }

    /// Limit gzip members and Zstd standard or skippable frames per gRPC message.
    ///
    /// Defaults to 1,024. Empty members and skippable frames count too. This
    /// bounds decoder setup work independently of decoded bytes; it does not
    /// preempt an admitted frame or limit the number of messages in an RPC.
    /// Exceeding the limit returns `RESOURCE_EXHAUSTED` before the next reset.
    #[must_use]
    pub fn with_max_compression_frames(mut self, limit: usize) -> Self {
        self.max_compression_frames = Some(limit);
        self
    }

    /// Lift the per-message compression-frame count limit for trusted input.
    #[must_use]
    pub fn with_unlimited_compression_frames(mut self) -> Self {
        self.max_compression_frames = None;
        self
    }

    /// Compression-frame count limit, independent of byte and history limits.
    ///
    /// [`Self::with_unlimited_decoding`] preserves this limit.
    #[must_use]
    pub fn max_compression_frames(self) -> Option<usize> {
        self.max_compression_frames
    }

    /// Cap the history window accepted from a Zstd frame, in bytes.
    ///
    /// The default is 8 MiB, independently of the decoded message limit.
    /// A larger advertised window fails with `RESOURCE_EXHAUSTED` before
    /// allocation, even when the message itself would be small. Raising this
    /// permits more decoder memory; it does not raise the decoded message cap.
    #[cfg(feature = "zstd")]
    #[must_use]
    pub fn with_max_zstd_window(mut self, limit: u64) -> Self {
        self.max_zstd_window = Some(limit);
        self
    }

    /// The configured Zstd window cap. `None` uses the backend's 100 MiB cap.
    ///
    /// Defaults to `Some(8 * 1024 * 1024)`. [`Self::unlimited`] uses `None`;
    /// [`Self::with_unlimited_decoding`] preserves this independent window cap.
    #[cfg(feature = "zstd")]
    #[must_use]
    pub fn max_zstd_window(self) -> Option<u64> {
        self.max_zstd_window
    }

    pub(crate) fn check_decode(self, n: usize) -> Result<(), Status> {
        if let Some(max) = self.max_decoding {
            if n > max {
                return Err(Self::decoding_limit_exceeded(n, max));
            }
        }
        Ok(())
    }

    pub(crate) fn check_encode(self, n: usize) -> Result<(), Status> {
        if let Some(max) = self.max_encoding {
            if n > max {
                return Err(Self::encoding_limit_exceeded(n, max));
            }
        }
        Ok(())
    }

    /// The limit strings stay off the per-message compare.
    #[cold]
    #[inline(never)]
    fn decoding_limit_exceeded(n: usize, max: usize) -> Status {
        Status::resource_exhausted(format!("decoded message length {n} exceeds limit {max}"))
    }

    #[cold]
    #[inline(never)]
    fn encoding_limit_exceeded(n: usize, max: usize) -> Status {
        Status::resource_exhausted(format!("encoded message length {n} exceeds limit {max}"))
    }

    /// How many decompressed bytes an inbound frame may produce.
    ///
    /// An unlimited configuration really is unlimited here: bounded inflate can
    /// only stop where a cap tells it to. That is why [`Self::unlimited`]
    /// documents itself as trusted-peer only.
    pub(crate) fn inflate_budget(self) -> usize {
        self.max_decoding.unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteBudgetTracker, BytePermit, MessageLimits};
    use crate::status::Code;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn byte_budget_tracker_acquire_and_release() {
        let tracker = ByteBudgetTracker::with_limit(100);
        assert_eq!(tracker.limit(), Some(100));
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.peak_allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 0);
        assert_eq!(tracker.available(), Some(100));
        assert!(tracker.is_quiescent());

        let p1 = tracker.acquire(40).expect("acquire 40");
        assert_eq!(p1.bytes(), 40);
        assert_eq!(tracker.allocated(), 40);
        assert_eq!(tracker.peak_allocated(), 40);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 1);
        assert_eq!(tracker.available(), Some(60));
        assert!(!tracker.is_quiescent());

        {
            let p2 = tracker.acquire(50).expect("acquire 50");
            assert_eq!(tracker.allocated(), 90);
            assert_eq!(tracker.peak_allocated(), 90);
            assert_eq!(tracker.active_byte_permit_tokens(), 2);
            assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);
            assert_eq!(tracker.available(), Some(10));

            let err = tracker.acquire(20).expect_err("exceed limit");
            assert_eq!(err.code(), Code::ResourceExhausted);
            assert_eq!(tracker.allocated(), 90);
            assert_eq!(tracker.peak_allocated(), 90);
            assert_eq!(tracker.active_byte_permit_tokens(), 2);

            drop(p2);
        }

        assert_eq!(tracker.allocated(), 40);
        assert_eq!(tracker.peak_allocated(), 90);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);
        assert_eq!(tracker.available(), Some(60));

        p1.release();
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.peak_allocated(), 90);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);
        assert_eq!(tracker.available(), Some(100));
        assert!(tracker.is_quiescent());
        let reacquired = tracker.acquire(10).expect("below prior high-water mark");
        assert_eq!(tracker.peak_allocated(), 90);
        drop(reacquired);
    }

    #[test]
    fn byte_budget_tracker_unlimited() {
        let tracker = ByteBudgetTracker::unlimited();
        assert_eq!(tracker.limit(), None);
        assert_eq!(tracker.available(), None);

        let p1 = tracker.acquire(1_000_000).expect("acquire large");
        assert_eq!(tracker.allocated(), 1_000_000);
        assert_eq!(tracker.peak_allocated(), 1_000_000);
        drop(p1);
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.peak_allocated(), 1_000_000);
    }

    #[test]
    fn byte_budget_peak_counts_overlapping_permits_and_rejects_overflow() {
        let tracker = ByteBudgetTracker::with_limit(100);
        let (ready_tx, ready_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let mut workers = Vec::new();
        for _ in 0..2 {
            let (release_tx, release_rx) = mpsc::channel();
            releases.push(release_tx);
            let ready_tx = ready_tx.clone();
            let tracker = tracker.clone();
            workers.push(thread::spawn(move || {
                let _permit = tracker.acquire(40).expect("within byte budget");
                ready_tx.send(()).expect("announce held permit");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release held permit");
            }));
        }
        for _ in 0..2 {
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("both acquisitions must complete");
        }
        assert_eq!(tracker.allocated(), 80);
        assert_eq!(tracker.peak_allocated(), 80);
        assert_eq!(tracker.active_byte_permit_tokens(), 2);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);
        for release in releases {
            release.send(()).expect("release worker");
        }
        for worker in workers {
            worker.join().expect("worker completed");
        }
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.peak_allocated(), 80);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);

        let unlimited = ByteBudgetTracker::unlimited();
        let max = unlimited
            .acquire(usize::MAX)
            .expect("largest representable count");
        let err = unlimited
            .acquire(1)
            .expect_err("counter overflow must reject");
        assert_eq!(err.code(), Code::ResourceExhausted);
        assert_eq!(unlimited.allocated(), usize::MAX);
        assert_eq!(unlimited.peak_allocated(), usize::MAX);
        assert_eq!(unlimited.active_byte_permit_tokens(), 1);
        drop(max);
        assert_eq!(unlimited.allocated(), 0);
        assert_eq!(unlimited.peak_allocated(), usize::MAX);
        assert_eq!(unlimited.active_byte_permit_tokens(), 0);
    }

    // Model the old fetch_add/rollback sequence at its dangerous scheduling
    // point. The overlapping public acquire demonstrates what another thread
    // could observe between those operations, without relying on a race test.
    #[test]
    fn overflow_rollback_counterfactual_publishes_a_false_byte_balance() {
        let tracker = ByteBudgetTracker::unlimited();
        let held = tracker.acquire(usize::MAX - 1).expect("initial permit");
        assert_eq!(
            tracker.acquire(2).expect_err("overflow").code(),
            Code::ResourceExhausted
        );
        assert_eq!(tracker.allocated(), usize::MAX - 1);

        // Old implementation: overflow wraps the shared counter to zero.
        assert_eq!(
            tracker.inner.allocated.fetch_add(2, Ordering::Relaxed),
            usize::MAX - 1
        );
        assert_eq!(tracker.allocated(), 0);
        let incorrectly_admitted = tracker
            .acquire(2)
            .expect("wrap admits an impossible permit");
        // The old failing caller rolls back, leaving zero despite held permits.
        tracker.inner.allocated.fetch_sub(2, Ordering::Relaxed);
        assert_eq!(tracker.allocated(), 0);

        // Remove the modeled permit and restore the deliberately corrupted
        // counter so the real held permit can be dropped normally.
        incorrectly_admitted.forget();
        tracker
            .inner
            .allocated
            .store(usize::MAX - 1, Ordering::Relaxed);
        drop(held);
        assert!(tracker.is_quiescent());
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[test]
    fn overflow_rollback_counterfactual_publishes_a_false_token_count() {
        let tracker = ByteBudgetTracker::unlimited();
        tracker
            .inner
            .active_byte_permit_tokens
            .store(usize::MAX, Ordering::Relaxed);
        assert_eq!(
            tracker.acquire(0).expect_err("overflow").code(),
            Code::ResourceExhausted
        );
        assert_eq!(tracker.active_byte_permit_tokens(), usize::MAX);

        assert_eq!(
            tracker
                .inner
                .active_byte_permit_tokens
                .fetch_add(1, Ordering::Relaxed),
            usize::MAX
        );
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        let incorrectly_admitted = tracker.acquire(0).expect("wrap admits another token");
        tracker
            .inner
            .active_byte_permit_tokens
            .fetch_sub(1, Ordering::Relaxed);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);

        // The synthetic original tokens are not real permits. Restore just
        // the modeled permit's charge before dropping it.
        tracker
            .inner
            .active_byte_permit_tokens
            .store(1, Ordering::Relaxed);
        drop(incorrectly_admitted);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    // Hold successful permits until both attempts complete, so exactly one
    // increment can fit. This exercises overlapping public acquisitions rather
    // than depending on scheduler timing to observe a transient wrap.
    #[test]
    fn concurrent_unlimited_byte_overflow_never_admits_two_permits() {
        use std::sync::{Arc, Barrier};

        let tracker = ByteBudgetTracker::unlimited();
        let original = tracker.acquire(usize::MAX - 1).expect("initial permit");
        let start = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        let (tx, rx) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..2 {
            let tracker = tracker.clone();
            let start = Arc::clone(&start);
            let release = Arc::clone(&release);
            let tx = tx.clone();
            workers.push(thread::spawn(move || {
                start.wait();
                let permit = tracker.acquire(1);
                tx.send(permit.is_ok()).expect("report acquisition");
                release.wait();
                drop(permit);
            }));
        }
        start.wait();
        let first = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first result");
        let second = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second result");
        let allocated = tracker.allocated();
        release.wait();
        for worker in workers {
            worker.join().expect("worker completed");
        }
        assert_ne!(first, second, "exactly one increment fits");
        assert_eq!(allocated, usize::MAX);
        assert_eq!(tracker.allocated(), usize::MAX - 1);
        drop(original);
        assert!(tracker.is_quiescent());
    }

    #[test]
    fn concurrent_token_overflow_never_admits_two_permits() {
        use std::sync::{Arc, Barrier};

        let tracker = ByteBudgetTracker::unlimited();
        // Simulate already-live permits without allocating usize::MAX objects.
        tracker
            .inner
            .active_byte_permit_tokens
            .store(usize::MAX - 1, Ordering::Relaxed);
        let start = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        let (tx, rx) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..2 {
            let tracker = tracker.clone();
            let start = Arc::clone(&start);
            let release = Arc::clone(&release);
            let tx = tx.clone();
            workers.push(thread::spawn(move || {
                start.wait();
                let permit = tracker.acquire(0);
                tx.send(permit.is_ok()).expect("report acquisition");
                release.wait();
                drop(permit);
            }));
        }
        start.wait();
        let first = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first result");
        let second = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("second result");
        let active = tracker.active_byte_permit_tokens();
        release.wait();
        for worker in workers {
            worker.join().expect("worker completed");
        }
        assert_ne!(first, second, "exactly one token fits");
        assert_eq!(active, usize::MAX);
        assert_eq!(tracker.active_byte_permit_tokens(), usize::MAX - 1);
        tracker
            .inner
            .active_byte_permit_tokens
            .store(0, Ordering::Relaxed);
    }

    #[test]
    fn zero_byte_tokens_and_counter_overflow_are_independent_of_byte_budget() {
        let tracker = ByteBudgetTracker::with_limit(0);
        let empty = BytePermit::empty();
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        drop(empty);

        let mut zero = tracker.acquire(0).expect("zero-byte permit");
        assert_eq!(tracker.allocated(), 0);
        assert!(tracker.is_quiescent());
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 1);
        let second_zero = tracker.acquire(0).expect("another zero-byte permit");
        assert_eq!(tracker.active_byte_permit_tokens(), 2);
        zero.merge(second_zero);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);
        assert_eq!(
            tracker.acquire(1).expect_err("byte limit").code(),
            Code::ResourceExhausted
        );
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        drop(zero);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);

        let unlimited = ByteBudgetTracker::unlimited();
        unlimited
            .inner
            .active_byte_permit_tokens
            .store(usize::MAX, Ordering::SeqCst);
        for bytes in [0, 7] {
            let error = unlimited
                .acquire(bytes)
                .expect_err("token counter overflow must reject");
            assert_eq!(error.code(), Code::ResourceExhausted);
            assert!(
                error
                    .message()
                    .contains("byte permit token counter overflow")
            );
            assert_eq!(unlimited.allocated(), 0);
            assert_eq!(unlimited.peak_allocated(), 0);
            assert_eq!(unlimited.peak_active_byte_permit_tokens(), 0);
        }
        unlimited
            .inner
            .active_byte_permit_tokens
            .store(0, Ordering::SeqCst);
        let permit = unlimited.acquire(7).expect("rollback restored byte budget");
        drop(permit);
        assert_eq!(unlimited.allocated(), 0);
        assert_eq!(unlimited.active_byte_permit_tokens(), 0);
    }

    #[test]
    fn byte_permit_merge_and_forget() {
        let tracker = ByteBudgetTracker::with_limit(200);
        let mut p1 = tracker.acquire(50).expect("p1");
        let p2 = tracker.acquire(60).expect("p2");
        assert_eq!(tracker.allocated(), 110);
        assert_eq!(tracker.active_byte_permit_tokens(), 2);

        p1.merge(p2);
        assert_eq!(p1.bytes(), 110);
        assert_eq!(tracker.allocated(), 110);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        assert_eq!(tracker.peak_active_byte_permit_tokens(), 2);

        drop(p1);
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);

        let p3 = tracker.acquire(30).expect("p3");
        p3.forget();
        assert_eq!(tracker.allocated(), 30);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[test]
    fn merging_a_zero_byte_token_transfers_only_the_held_owners_token() {
        let first = ByteBudgetTracker::unlimited();
        let second = ByteBudgetTracker::unlimited();
        let mut zero = first.acquire(0).expect("first zero-byte token");
        let held = second.acquire(4).expect("second byte token");
        assert_eq!(first.active_byte_permit_tokens(), 1);
        assert_eq!(second.active_byte_permit_tokens(), 1);
        zero.merge(held);
        assert_eq!(first.active_byte_permit_tokens(), 0);
        assert_eq!(second.active_byte_permit_tokens(), 1);
        assert_eq!(second.allocated(), 4);
        drop(zero);
        assert_eq!(second.active_byte_permit_tokens(), 0);
        assert_eq!(second.allocated(), 0);
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn zstd_window_is_separate_from_message_size_limits() {
        let limits = MessageLimits::new();
        assert_eq!(limits.max_zstd_window(), Some(8 * 1024 * 1024));
        assert_eq!(
            limits.with_unlimited_decoding().max_zstd_window(),
            Some(8 * 1024 * 1024)
        );
        let limits = limits.with_max_zstd_window(64 * 1024);
        assert_eq!(limits.max_zstd_window(), Some(64 * 1024));
        assert_eq!(limits.max_decoding(), Some(4 * 1024 * 1024));
        let lifted = limits.with_unlimited_decoding().with_unlimited_encoding();
        assert_eq!(lifted.max_decoding(), None);
        assert_eq!(lifted.max_encoding(), None);
        assert_eq!(lifted.max_zstd_window(), Some(64 * 1024));
        assert_eq!(
            lifted,
            MessageLimits::unlimited()
                .with_max_zstd_window(64 * 1024)
                .with_max_compression_frames(1024)
        );
        assert_eq!(MessageLimits::unlimited().max_zstd_window(), None);
    }

    #[test]
    fn default_caps_inbound_only() {
        let limits = MessageLimits::default();
        assert!(limits.check_decode(4 * 1024 * 1024).is_ok());
        let err = limits
            .check_decode(4 * 1024 * 1024 + 1)
            .expect_err("over cap");
        assert_eq!(err.code(), Code::ResourceExhausted);
        assert!(limits.check_encode(usize::MAX).is_ok());
    }

    #[test]
    fn unlimited_accepts_anything() {
        let limits = MessageLimits::unlimited();
        assert!(limits.check_decode(usize::MAX).is_ok());
        assert!(limits.check_encode(usize::MAX).is_ok());
    }

    #[test]
    fn builders_round_trip() {
        let limits = MessageLimits::new()
            .with_max_decoding(7)
            .with_max_encoding(9);
        assert_eq!(limits.max_decoding(), Some(7));
        assert_eq!(limits.max_encoding(), Some(9));
        let lifted = limits.with_unlimited_decoding().with_unlimited_encoding();
        assert_eq!(lifted.max_decoding(), None);
        assert_eq!(lifted.max_encoding(), None);
        assert!(lifted.check_decode(usize::MAX).is_ok());
        assert!(lifted.check_encode(usize::MAX).is_ok());
        let expected = MessageLimits::unlimited();
        let expected = expected.with_max_compression_frames(1024);
        #[cfg(feature = "zstd")]
        let expected = expected.with_max_zstd_window(8 * 1024 * 1024);
        assert_eq!(lifted, expected);
    }

    #[test]
    fn compression_frame_count_is_independent_of_byte_limits() {
        let limits = MessageLimits::new();
        assert_eq!(limits.max_compression_frames(), Some(1024));
        let limits = limits
            .with_max_compression_frames(7)
            .with_unlimited_decoding()
            .with_unlimited_encoding();
        assert_eq!(limits.max_compression_frames(), Some(7));
        assert_eq!(limits.max_decoding(), None);
        assert_eq!(limits.max_encoding(), None);
        assert_eq!(
            limits
                .with_max_compression_frames(0)
                .max_compression_frames(),
            Some(0)
        );
        assert_eq!(
            limits
                .with_unlimited_compression_frames()
                .max_compression_frames(),
            None
        );
        assert_eq!(MessageLimits::unlimited().max_compression_frames(), None);
    }
}
