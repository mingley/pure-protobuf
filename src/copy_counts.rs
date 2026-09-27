//! Bench-only user-space copy counters (SB-13, Phase 0 attribution).
//!
//! `Wire::from_slice` is the single funnel for `Arc<[u8]>` copies of
//! message bytes (`Wire::ensure`, `from_utf8_payload`, direct field
//! copies). Counting it attributes every runtime backing-buffer copy to
//! one source location. `WireOut::put_slice` (plus the packed fixed-width
//! fast paths, which bypass it) is the single funnel for message payload
//! bytes emitted into encode output. Production builds leave the feature
//! off and the note calls compile to nothing.

/// Snapshot of the process-wide runtime copy counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyCounts {
    /// `Wire::from_slice` calls with non-empty input.
    pub wire_calls: u64,
    /// Message bytes copied into `Arc<[u8]>` backing buffers.
    pub wire_bytes: u64,
    /// Encode payload emissions (`WireOut::put_slice` + packed fixed).
    pub emit_calls: u64,
    /// Message payload bytes emitted into encode output.
    pub emit_bytes: u64,
}

#[cfg(feature = "copy-counts")]
mod state {
    use super::CopyCounts;
    use std::sync::atomic::{AtomicU64, Ordering};

    static WIRE_CALLS: AtomicU64 = AtomicU64::new(0);
    static WIRE_BYTES: AtomicU64 = AtomicU64::new(0);
    static EMIT_CALLS: AtomicU64 = AtomicU64::new(0);
    static EMIT_BYTES: AtomicU64 = AtomicU64::new(0);

    pub(super) fn snapshot() -> CopyCounts {
        CopyCounts {
            wire_calls: WIRE_CALLS.load(Ordering::Relaxed),
            wire_bytes: WIRE_BYTES.load(Ordering::Relaxed),
            emit_calls: EMIT_CALLS.load(Ordering::Relaxed),
            emit_bytes: EMIT_BYTES.load(Ordering::Relaxed),
        }
    }

    pub(super) fn reset() {
        WIRE_CALLS.store(0, Ordering::Relaxed);
        WIRE_BYTES.store(0, Ordering::Relaxed);
        EMIT_CALLS.store(0, Ordering::Relaxed);
        EMIT_BYTES.store(0, Ordering::Relaxed);
    }

    pub(super) fn add_wire(bytes: u64) {
        WIRE_CALLS.fetch_add(1, Ordering::Relaxed);
        WIRE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_emit(bytes: u64) {
        EMIT_CALLS.fetch_add(1, Ordering::Relaxed);
        EMIT_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Read the process-wide counters. All zeros unless the `copy-counts`
/// feature is enabled and the counted paths ran.
pub fn copy_counts() -> CopyCounts {
    #[cfg(feature = "copy-counts")]
    return state::snapshot();
    #[cfg(not(feature = "copy-counts"))]
    CopyCounts::default()
}

/// Zero every counter. Bench harnesses call this around the timed phase.
pub fn reset_copy_counts() {
    #[cfg(feature = "copy-counts")]
    state::reset();
}

/// Record one `Wire::from_slice` copy of `bytes` message bytes.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_wire(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_wire(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

/// Record one encode payload emission of `bytes` message bytes
/// (`WireOut::put_slice` or a packed fixed-width fast path).
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_emit(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_emit(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

#[cfg(test)]
mod tests {
    use super::copy_counts;
    use crate::rt::Wire;

    #[test]
    #[cfg(feature = "copy-counts")]
    fn counts_wire_backing_copies() {
        // Lower bounds: the counters are process-wide and other tests in
        // this binary also copy through Wire::from_slice.
        let before = copy_counts();
        let w = Wire::from_slice(&[9u8; 128]);
        assert_eq!(w.as_slice().len(), 128);
        // Empty input shares the static and copies nothing.
        let e = Wire::from_slice(&[]);
        assert!(e.as_slice().is_empty());
        let after = copy_counts();
        assert!(after.wire_calls - before.wire_calls >= 1);
        assert!(after.wire_bytes - before.wire_bytes >= 128);
    }

    #[test]
    #[cfg(feature = "copy-counts")]
    fn counts_encode_payload_emissions() {
        use crate::rt::WireOut;
        let before = copy_counts();
        let mut out = Vec::new();
        out.put_slice(&[7u8; 64]);
        let after = copy_counts();
        assert!(after.emit_calls - before.emit_calls >= 1);
        assert!(after.emit_bytes - before.emit_bytes >= 64);
    }

    #[test]
    #[cfg(not(feature = "copy-counts"))]
    fn counters_stay_zero_without_feature() {
        let w = Wire::from_slice(&[9u8; 128]);
        assert_eq!(w.as_slice().len(), 128);
        assert_eq!(copy_counts(), super::CopyCounts::default());
    }
}
