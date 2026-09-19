//! Resource caps applied to every RPC.
//!
//! Every limit here is enforced *before* the memory it guards is committed:
//! a frame length is rejected from the 5-byte header, and a compressed frame
//! is inflated through a bounded reader that stops one byte past the cap.
//! See [the threat model](crate#threat-model).

use crate::status::Status;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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

    /// The number of bytes currently allocated across active permits.
    #[must_use]
    pub fn allocated(&self) -> usize {
        self.inner.allocated.load(Ordering::SeqCst)
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

    /// Try to acquire a permit for `bytes`.
    ///
    /// If the allocation would exceed the configured limit, returns
    /// `Status::resource_exhausted`. Otherwise, returns an RAII [`BytePermit`]
    /// that will release the bytes back to this tracker when dropped.
    pub fn try_acquire(&self, bytes: usize) -> Result<BytePermit, Status> {
        if bytes == 0 {
            return Ok(BytePermit {
                tracker: Some(self.clone()),
                bytes: 0,
            });
        }

        if let Some(limit) = self.inner.limit {
            let mut current = self.inner.allocated.load(Ordering::SeqCst);
            loop {
                let next = current.saturating_add(bytes);
                if next > limit {
                    return Err(Status::resource_exhausted(format!(
                        "transport byte budget exceeded: requested {bytes} bytes, current allocated {current}, limit {limit}"
                    )));
                }
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
        } else {
            self.inner.allocated.fetch_add(bytes, Ordering::SeqCst);
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

    pub(crate) fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let mut current = self.inner.allocated.load(Ordering::SeqCst);
        loop {
            let next = current.saturating_sub(bytes);
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
    pub fn forget(mut self) {
        self.tracker = None;
    }

    /// Merge another permit into this one, provided they belong to the same tracker.
    pub fn merge(&mut self, mut other: BytePermit) {
        if other.bytes == 0 {
            return;
        }
        if self.bytes == 0 {
            self.tracker = other.tracker.take();
            self.bytes = other.bytes;
            return;
        }
        if let (Some(t1), Some(t2)) = (&self.tracker, &other.tracker) {
            if Arc::ptr_eq(&t1.inner, &t2.inner) {
                self.bytes = self.bytes.saturating_add(other.bytes);
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

/// Per-message size caps. `None` means unlimited.
///
/// Both caps count *uncompressed* protobuf bytes, so a compressed frame is
/// measured by what it inflates to, not by what arrived on the wire.
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
}

impl Default for MessageLimits {
    /// 4 MiB inbound, unlimited outbound.
    ///
    /// Inbound is capped because a peer controls it; outbound is not because
    /// the local service does.
    fn default() -> Self {
        Self {
            max_decoding: Some(DEFAULT_MAX_DECODING_MESSAGE_SIZE),
            max_encoding: None,
        }
    }
}

impl MessageLimits {
    /// Defaults: 4 MiB inbound, unlimited outbound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// No caps in either direction.
    ///
    /// Only appropriate when every peer is trusted: a single hostile frame
    /// header can then ask for as much memory as `u32::MAX` allows.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_decoding: None,
            max_encoding: None,
        }
    }

    /// Cap inbound messages at `limit` uncompressed bytes.
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

    pub(crate) fn check_decode(self, n: usize) -> Result<(), Status> {
        match self.max_decoding {
            Some(max) if n > max => Err(Status::resource_exhausted(format!(
                "decoded message length {n} exceeds limit {max}"
            ))),
            _ => Ok(()),
        }
    }

    pub(crate) fn check_encode(self, n: usize) -> Result<(), Status> {
        match self.max_encoding {
            Some(max) if n > max => Err(Status::resource_exhausted(format!(
                "encoded message length {n} exceeds limit {max}"
            ))),
            _ => Ok(()),
        }
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
    use super::{ByteBudgetTracker, MessageLimits};
    use crate::status::Code;

    #[test]
    fn byte_budget_tracker_acquire_and_release() {
        let tracker = ByteBudgetTracker::with_limit(100);
        assert_eq!(tracker.limit(), Some(100));
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.available(), Some(100));
        assert!(tracker.is_quiescent());

        let p1 = tracker.acquire(40).expect("acquire 40");
        assert_eq!(p1.bytes(), 40);
        assert_eq!(tracker.allocated(), 40);
        assert_eq!(tracker.available(), Some(60));
        assert!(!tracker.is_quiescent());

        {
            let p2 = tracker.acquire(50).expect("acquire 50");
            assert_eq!(tracker.allocated(), 90);
            assert_eq!(tracker.available(), Some(10));

            let err = tracker.acquire(20).expect_err("exceed limit");
            assert_eq!(err.code(), Code::ResourceExhausted);
            assert_eq!(tracker.allocated(), 90);

            drop(p2);
        }

        assert_eq!(tracker.allocated(), 40);
        assert_eq!(tracker.available(), Some(60));

        p1.release();
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.available(), Some(100));
        assert!(tracker.is_quiescent());
    }

    #[test]
    fn byte_budget_tracker_unlimited() {
        let tracker = ByteBudgetTracker::unlimited();
        assert_eq!(tracker.limit(), None);
        assert_eq!(tracker.available(), None);

        let p1 = tracker.acquire(1_000_000).expect("acquire large");
        assert_eq!(tracker.allocated(), 1_000_000);
        drop(p1);
        assert_eq!(tracker.allocated(), 0);
    }

    #[test]
    fn byte_permit_merge_and_forget() {
        let tracker = ByteBudgetTracker::with_limit(200);
        let mut p1 = tracker.acquire(50).expect("p1");
        let p2 = tracker.acquire(60).expect("p2");
        assert_eq!(tracker.allocated(), 110);

        p1.merge(p2);
        assert_eq!(p1.bytes(), 110);
        assert_eq!(tracker.allocated(), 110);

        drop(p1);
        assert_eq!(tracker.allocated(), 0);

        let p3 = tracker.acquire(30).expect("p3");
        p3.forget();
        assert_eq!(tracker.allocated(), 30);
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
        assert_eq!(lifted, MessageLimits::unlimited());
    }
}
