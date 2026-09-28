//! Bench-only user-space copy counters (SB-13, Phase 0 attribution).
//!
//! Every counted site is one `memcpy`-class copy of message bytes on the
//! gRPC data path. The counters exist only to attribute copies to source
//! locations in benchmarks; production builds leave the feature off and
//! every note call compiles to nothing.
//!
//! Sites:
//!
//! - `carry_*`: `wire::FrameReader::push` extending a straddling frame
//!   into `carry` (plus regrowth copies, which land here too).
//! - `chunk_slice_*`: `codec::pop_from_chunk` slicing a whole frame out
//!   of one DATA chunk. A zero-copy witness, not a copy.
//! - `encode_*`: uncompressed `wire::encode_msg` serializing the message
//!   straight into the framed buffer. Counts only bytes actually copied;
//!   shared segments are witnessed under `shared_*` instead.
//! - `serialize_*`: compressed `encode_msg` materializing `T::serialize`
//!   before compression.
//! - `shared_*`: large bytes fields handed to the segmented send sink via
//!   `WireOut::put_shared` (PK-11). A zero-copy witness, not a copy.

/// Snapshot of the process-wide copy counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyCounts {
    /// `carry.extend_from_slice` calls (frame straddled a chunk boundary).
    pub carry_calls: u64,
    /// Bytes moved into `carry` (includes `BytesMut` regrowth).
    pub carry_bytes: u64,
    /// Whole frames sliced out of one chunk (zero-copy path taken).
    pub chunk_slices: u64,
    /// Payload bytes sliced out of one chunk (never copied).
    pub chunk_slice_bytes: u64,
    /// Uncompressed messages serialized into framed buffers.
    pub encode_calls: u64,
    /// Message bytes serialized into framed buffers.
    pub encode_bytes: u64,
    /// Compressed-path `T::serialize` materializations.
    pub serialize_calls: u64,
    /// Bytes materialized by compressed-path `T::serialize`.
    pub serialize_bytes: u64,
    /// Large bytes fields sent as shared segments (zero-copy path taken).
    pub shared_segments: u64,
    /// Payload bytes sent as shared segments (never copied).
    pub shared_bytes: u64,
}

#[cfg(feature = "copy-counts")]
mod state {
    use super::CopyCounts;
    use std::sync::atomic::{AtomicU64, Ordering};

    static CARRY_CALLS: AtomicU64 = AtomicU64::new(0);
    static CARRY_BYTES: AtomicU64 = AtomicU64::new(0);
    static CHUNK_SLICES: AtomicU64 = AtomicU64::new(0);
    static CHUNK_SLICE_BYTES: AtomicU64 = AtomicU64::new(0);
    static ENCODE_CALLS: AtomicU64 = AtomicU64::new(0);
    static ENCODE_BYTES: AtomicU64 = AtomicU64::new(0);
    static SERIALIZE_CALLS: AtomicU64 = AtomicU64::new(0);
    static SERIALIZE_BYTES: AtomicU64 = AtomicU64::new(0);
    static SHARED_SEGMENTS: AtomicU64 = AtomicU64::new(0);
    static SHARED_BYTES: AtomicU64 = AtomicU64::new(0);

    pub(super) fn snapshot() -> CopyCounts {
        CopyCounts {
            carry_calls: CARRY_CALLS.load(Ordering::Relaxed),
            carry_bytes: CARRY_BYTES.load(Ordering::Relaxed),
            chunk_slices: CHUNK_SLICES.load(Ordering::Relaxed),
            chunk_slice_bytes: CHUNK_SLICE_BYTES.load(Ordering::Relaxed),
            encode_calls: ENCODE_CALLS.load(Ordering::Relaxed),
            encode_bytes: ENCODE_BYTES.load(Ordering::Relaxed),
            serialize_calls: SERIALIZE_CALLS.load(Ordering::Relaxed),
            serialize_bytes: SERIALIZE_BYTES.load(Ordering::Relaxed),
            shared_segments: SHARED_SEGMENTS.load(Ordering::Relaxed),
            shared_bytes: SHARED_BYTES.load(Ordering::Relaxed),
        }
    }

    pub(super) fn reset() {
        CARRY_CALLS.store(0, Ordering::Relaxed);
        CARRY_BYTES.store(0, Ordering::Relaxed);
        CHUNK_SLICES.store(0, Ordering::Relaxed);
        CHUNK_SLICE_BYTES.store(0, Ordering::Relaxed);
        ENCODE_CALLS.store(0, Ordering::Relaxed);
        ENCODE_BYTES.store(0, Ordering::Relaxed);
        SERIALIZE_CALLS.store(0, Ordering::Relaxed);
        SERIALIZE_BYTES.store(0, Ordering::Relaxed);
        SHARED_SEGMENTS.store(0, Ordering::Relaxed);
        SHARED_BYTES.store(0, Ordering::Relaxed);
    }

    pub(super) fn add_carry(bytes: u64) {
        CARRY_CALLS.fetch_add(1, Ordering::Relaxed);
        CARRY_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_chunk_slice(bytes: u64) {
        CHUNK_SLICES.fetch_add(1, Ordering::Relaxed);
        CHUNK_SLICE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_encode(bytes: u64) {
        ENCODE_CALLS.fetch_add(1, Ordering::Relaxed);
        ENCODE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_serialize(bytes: u64) {
        SERIALIZE_CALLS.fetch_add(1, Ordering::Relaxed);
        SERIALIZE_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_shared(bytes: u64) {
        SHARED_SEGMENTS.fetch_add(1, Ordering::Relaxed);
        SHARED_BYTES.fetch_add(bytes, Ordering::Relaxed);
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

/// Record one `carry.extend_from_slice` of `bytes` message bytes.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_carry(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_carry(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

/// Record one zero-copy slice of `bytes` payload bytes out of a chunk.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_chunk_slice(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_chunk_slice(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

/// Record one uncompressed message encode of `bytes` message bytes.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_encode(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_encode(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

/// Record one compressed-path `T::serialize` of `bytes` message bytes.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_serialize(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_serialize(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

/// Record one zero-copy send of `bytes` payload bytes as a shared segment.
/// Compiles to nothing without the `copy-counts` feature.
pub(crate) fn note_shared(bytes: usize) {
    #[cfg(feature = "copy-counts")]
    state::add_shared(bytes as u64);
    #[cfg(not(feature = "copy-counts"))]
    let _ = bytes;
}

#[cfg(test)]
mod tests {
    use super::copy_counts;
    use crate::codec;
    use crate::hello::HelloRequest;
    use crate::limits::MessageLimits;
    use crate::wire::encode::frame_from_msg;
    use crate::wire::frame_reader::FrameReader;
    use bytes::BytesMut;
    use pbrs::Serialize;

    fn framed(payload: &[u8]) -> BytesMut {
        let mut buf = BytesMut::with_capacity(codec::HEADER_LEN + payload.len());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    #[cfg(feature = "copy-counts")]
    fn counts_carry_slices_and_encodes() {
        // Deltas with lower bounds: the counters are process-wide and
        // other tests in this binary also drive the counted paths, which
        // can only inflate a delta, never shrink it.
        let limits = MessageLimits::new();

        // Whole frame in one chunk: one zero-copy slice of 11 bytes.
        let before = copy_counts();
        let mut chunk = framed(b"hello world").freeze();
        let frame = codec::pop_from_chunk(&mut chunk, limits)
            .expect("pop")
            .expect("frame");
        assert_eq!(frame.payload.len(), 11);
        let after = copy_counts();
        assert!(after.chunk_slices - before.chunk_slices >= 1);
        assert!(after.chunk_slice_bytes - before.chunk_slice_bytes >= 11);

        // Frame straddling two chunks: two carry extends covering the
        // whole 69-byte frame.
        let before = copy_counts();
        let mut reader = FrameReader::new(limits);
        let mut full = framed(&[7u8; 64]);
        let head = full.split_to(10);
        reader.push(head.freeze());
        reader.push(full.freeze());
        let frame = reader.next_frame().expect("next").expect("frame");
        assert_eq!(frame.payload.len(), 64);
        let after = copy_counts();
        assert!(after.carry_calls - before.carry_calls >= 2);
        assert!(after.carry_bytes - before.carry_bytes >= codec::HEADER_LEN as u64 + 64);

        // Outbound encode: one logical copy of the message bytes.
        let before = copy_counts();
        let msg = HelloRequest::new();
        let len = HelloRequest::serialized_len(&msg);
        frame_from_msg(&msg, len).expect("frame");
        let after = copy_counts();
        assert!(after.encode_calls - before.encode_calls >= 1);
        assert!(after.encode_bytes - before.encode_bytes >= len as u64);
    }

    #[test]
    #[cfg(not(feature = "copy-counts"))]
    fn counters_stay_zero_without_feature() {
        let limits = MessageLimits::new();
        let mut chunk = framed(b"hello world").freeze();
        let _ = codec::pop_from_chunk(&mut chunk, limits).expect("pop");
        let msg = HelloRequest::new();
        let len = HelloRequest::serialized_len(&msg);
        frame_from_msg(&msg, len).expect("frame");
        assert_eq!(copy_counts(), super::CopyCounts::default());
    }
}
