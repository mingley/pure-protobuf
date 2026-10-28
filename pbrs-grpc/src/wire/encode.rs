//! Outbound message framing: length-prefix encoding.

use crate::codec;
use crate::codec::CodecMessage;
use crate::compression::CompressionAlgorithm;
use crate::limits::MessageLimits;
use crate::status::Status;
use crate::workspace::RetainedEncodeLease;
use bytes::{BufMut, Bytes, BytesMut};
use pbrs::rt::WireOut;
use std::io::{self, Write};

/// Segmented outbound sink (PK-11): small writes accumulate in `head`;
/// large shared field buffers handed to `put_shared` become their own
/// segments without copying. With no shared field the sink holds exactly
/// one segment and behaves like the old single buffer.
pub(crate) struct SegSink {
    head: BytesMut,
    // head drops before the charge for its complete original backing.
    retained_head: Option<Box<RetainedHead>>,
    segs: Vec<Bytes>,
    total_len: usize,
    shared_len: usize,
}

// An empty tail still owns the whole allocation, not just its spare suffix.
struct RetainedHead {
    capacity: usize,
    _lease: Option<RetainedEncodeLease>,
}

/// Rollback point for a failed message encode inside a batch.
pub(crate) struct SinkCheckpoint {
    segs: usize,
    head: usize,
    total_len: usize,
    shared_len: usize,
}

impl SegSink {
    pub(crate) fn new() -> Self {
        Self {
            head: BytesMut::new(),
            retained_head: None,
            segs: Vec::new(),
            total_len: 0,
            shared_len: 0,
        }
    }

    // Idle output becomes active before the next encode. Keep the shared
    // spare tail even if HTTP/2 still owns the previous immutable frame:
    // a later BytesMut reservation can reclaim after that frame is consumed.
    pub(crate) fn prepare_encode(&mut self) {
        if let Some(retained) = self.retained_head.as_mut() {
            if retained._lease.take().is_some() {
                debug_assert!(self.head.is_empty());
                let _reclaimed = self.head.try_reclaim(retained.capacity);
            }
        }
    }

    // A split tail may expose less capacity than its complete backing.
    // Keep the preceding full-capacity bound until another contiguous output
    // is taken. If a new allocation replaces it, max(old full bound, current
    // capacity) can overcharge, but never hides an allocation behind tail size.
    pub(crate) fn take_reusing(&mut self, compressed_only: bool) -> SegFrame {
        let capacity = self
            .retained_head
            .as_ref()
            .map_or(0, |retained| retained.capacity)
            .max(self.head.capacity());
        if compressed_only
            && self.segs.is_empty()
            && !self.head.is_empty()
            && capacity > MAX_RETAINED_SCRATCH_BYTES
        {
            debug_assert!(
                self.retained_head
                    .as_ref()
                    .is_none_or(|retained| retained._lease.is_none())
            );
            if let Some(lease) = RetainedEncodeLease::try_acquire(capacity) {
                let length = self.head.len();
                let first = self.head.split_to(length).freeze();
                if let Some(retained) = self.retained_head.as_mut() {
                    retained.capacity = capacity;
                    retained._lease = Some(lease);
                } else {
                    self.retained_head = Some(Box::new(RetainedHead {
                        capacity,
                        _lease: Some(lease),
                    }));
                }
                self.total_len = 0;
                self.shared_len = 0;
                return SegFrame {
                    first,
                    rest: Vec::new(),
                    total_len: length,
                };
            }
        }
        // Declined/oversized retention drops the mutable owner before its
        // lease; any immutable output remains active transport ownership.
        std::mem::replace(self, Self::new()).finish()
    }

    #[cfg(test)]
    pub(crate) fn output_cache_state(&self) -> (usize, Option<usize>, bool) {
        (
            self.head.capacity(),
            self.retained_head
                .as_ref()
                .and_then(|retained| retained._lease.as_ref().map(|_| retained.capacity)),
            self.retained_head.is_some(),
        )
    }

    pub(crate) fn reserve(&mut self, n: usize) {
        self.head.reserve(n);
    }

    /// First buffer for a frame. An empty head has nothing to preserve, so
    /// this allocates the `Vec` directly instead of going through `reserve`.
    pub(crate) fn reserve_new(&mut self, n: usize) {
        if self.head.capacity() == 0 {
            self.head = BytesMut::with_capacity(n);
        } else {
            self.head.reserve(n);
        }
    }

    /// Total bytes sunk so far (copied + shared).
    pub(crate) fn len(&self) -> usize {
        self.total_len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Bytes retained as shared segments (never copied).
    pub(crate) fn shared_len(&self) -> usize {
        self.shared_len
    }

    /// New segments since a checkpoint, plus the live head tail.
    pub(crate) fn segments_since(
        &self,
        cp: &SinkCheckpoint,
    ) -> (impl Iterator<Item = &Bytes>, &[u8]) {
        let segs = self.segs.get(cp.segs..).unwrap_or(&[]);
        (segs.iter(), self.head.get(cp.head..).unwrap_or_default())
    }

    /// Direct access to the live head for the compressed path, which must
    /// patch its length prefix after encoding.
    pub(crate) fn head_mut(&mut self) -> &mut BytesMut {
        &mut self.head
    }

    /// Bytes appended straight to the head (compressed path bypass).
    pub(crate) fn note_head_wrote(&mut self, n: usize) {
        self.total_len += n;
    }

    pub(crate) fn checkpoint(&self) -> SinkCheckpoint {
        SinkCheckpoint {
            segs: self.segs.len(),
            head: self.head.len(),
            total_len: self.total_len,
            shared_len: self.shared_len,
        }
    }

    pub(crate) fn rollback(&mut self, cp: &SinkCheckpoint) {
        if let Some(first_added) = self.segs.get(cp.segs) {
            // put_shared freezes the live head before adding its shared
            // segment. Its first new segment therefore starts with the head
            // from this checkpoint, followed by any partial message bytes.
            // Recover that prefix before removing the failed message's
            // segments. Copy only on this error path; checkpoints allocate
            // nothing during successful encoding.
            self.head = BytesMut::from(first_added.get(..cp.head).unwrap_or_default());
        } else {
            self.head.truncate(cp.head);
        }
        self.segs.truncate(cp.segs);
        self.total_len = cp.total_len;
        self.shared_len = cp.shared_len;
    }

    fn freeze_head(&mut self) {
        if !self.head.is_empty() {
            self.segs.push(std::mem::take(&mut self.head).freeze());
        }
    }

    pub(crate) fn finish(mut self) -> SegFrame {
        if self.segs.is_empty() {
            // Fast path: nothing was ever shared, so the head is the whole
            // frame and no segment Vec was ever allocated.
            let first = std::mem::take(&mut self.head).freeze();
            let total_len = first.len();
            // `Vec::new` and `BytesMut::new` own no heap. Dropping them is a
            // kind check and a capacity check on every frame.
            if self.segs.capacity() == 0 && self.retained_head.is_none() {
                let _empty = std::mem::ManuallyDrop::new(self);
            }
            return SegFrame {
                first,
                rest: Vec::new(),
                total_len,
            };
        }
        self.freeze_head();
        let mut segs = self.segs.into_iter();
        let first = segs.next().unwrap_or_default();
        let rest: Vec<Bytes> = segs.collect();
        let total_len = first.len() + rest.iter().map(Bytes::len).sum::<usize>();
        SegFrame {
            first,
            rest,
            total_len,
        }
    }
}

impl WireOut for SegSink {
    fn put_u8(&mut self, b: u8) {
        BufMut::put_u8(&mut self.head, b);
        self.total_len += 1;
    }

    fn put_slice(&mut self, data: &[u8]) {
        BufMut::put_slice(&mut self.head, data);
        self.total_len += data.len();
    }

    fn write_contiguous<R>(&mut self, write: impl FnOnce(&mut BytesMut) -> R) -> R {
        let before = self.head.len();
        let result = write(&mut self.head);
        self.total_len += self.head.len() - before;
        result
    }

    fn put_shared(&mut self, b: &Bytes) {
        crate::copy_counts::note_shared(b.len());
        self.freeze_head();
        self.total_len += b.len();
        self.shared_len += b.len();
        self.segs.push(b.clone());
    }

    fn supports_shared(&self) -> bool {
        true
    }
}

/// Reusable protobuf bytes for independently compressed stream messages.
///
/// This buffer belongs to one outbound stream, so concurrent RPCs never share
/// mutable codec state. Append-only codecs may retain large canonical buffers
/// under an independent finite capacity budget. Custom contiguous codecs keep
/// their existing path and release large buffers after compression.
pub(crate) struct EncodeScratch {
    bytes: BytesMut,
    append: Option<Box<AppendWorkspace>>,
}

// Field order frees the allocation before releasing its idle-capacity charge.
struct AppendWorkspace {
    bytes: Vec<u8>,
    lease: Option<RetainedEncodeLease>,
}

const MAX_RETAINED_SCRATCH_BYTES: usize = 64 * 1024;

impl EncodeScratch {
    pub(crate) fn new() -> Self {
        Self {
            bytes: BytesMut::new(),
            append: None,
        }
    }

    pub(crate) fn discard(&mut self) {
        *self = Self::new();
    }

    fn with_payload<T: CodecMessage, R>(
        &mut self,
        msg: &T,
        len: usize,
        use_payload: impl FnOnce(&[u8]) -> Result<R, Status>,
    ) -> Result<R, Status> {
        // Arbitrary codec/compressor code may unwind. Leave no allocation in
        // self until encoding and compression succeed and retention is charged.
        let mut active = std::mem::replace(self, Self::new());
        if T::ENCODE_APPEND_ONLY && (len > MAX_RETAINED_SCRATCH_BYTES || active.append.is_some()) {
            active.bytes = BytesMut::new();
            let mut workspace = active.append.take().unwrap_or_else(|| {
                Box::new(AppendWorkspace {
                    bytes: Vec::new(),
                    lease: None,
                })
            });
            // This storage is now active encoding, governed by message limits.
            workspace.lease = None;
            workspace.bytes.clear();
            workspace.bytes.reserve(len);
            let result = msg
                .encode_payload_bufmut(&mut workspace.bytes)
                .and_then(|()| {
                    crate::copy_counts::note_serialize(workspace.bytes.len());
                    use_payload(&workspace.bytes)
                });
            if result.is_ok() {
                if let Some(lease) = RetainedEncodeLease::try_acquire(workspace.bytes.capacity()) {
                    workspace.bytes.clear();
                    workspace.lease = Some(lease);
                    active.append = Some(workspace);
                    *self = active;
                }
                // Oversize/full retention is a cache miss, never an RPC error.
            }
            return result;
        }
        active.append = None;
        active.bytes.clear();
        active.bytes.reserve(len);
        let result = msg.encode_payload(&mut active).and_then(|()| {
            crate::copy_counts::note_serialize(active.bytes.len());
            use_payload(&active.bytes)
        });
        if result.is_ok() && active.bytes.capacity() <= MAX_RETAINED_SCRATCH_BYTES {
            active.bytes.clear();
            *self = active;
        }
        result
    }
}

impl WireOut for EncodeScratch {
    fn put_u8(&mut self, byte: u8) {
        BufMut::put_u8(&mut self.bytes, byte);
    }

    fn put_slice(&mut self, data: &[u8]) {
        WireOut::put_slice(&mut self.bytes, data);
    }

    fn write_contiguous<R>(&mut self, write: impl FnOnce(&mut BytesMut) -> R) -> R {
        write(&mut self.bytes)
    }
}

/// One framed gRPC message as an ordered segment list (PK-11).
///
/// The first segment is stored inline so the unsegmented case costs no
/// extra allocation versus the old contiguous frame.
#[derive(Clone, Debug)]
pub(crate) struct SegFrame {
    first: Bytes,
    rest: Vec<Bytes>,
    total_len: usize,
}

impl SegFrame {
    pub(crate) fn single(frame: Bytes) -> Self {
        let total_len = frame.len();
        Self {
            first: frame,
            rest: Vec::new(),
            total_len,
        }
    }

    pub(crate) fn total_len(&self) -> usize {
        self.total_len
    }

    pub(crate) fn seg_count(&self) -> usize {
        1 + self.rest.len()
    }

    pub(crate) fn segments(&self) -> impl Iterator<Item = &Bytes> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    pub(crate) fn into_segments(self) -> impl Iterator<Item = Bytes> {
        std::iter::once(self.first).chain(self.rest)
    }

    /// The one-segment frame. The empty `rest` vec owns no heap.
    pub(crate) fn into_first(self) -> Bytes {
        let SegFrame { first, rest, .. } = self;
        if rest.capacity() == 0 {
            let _empty = std::mem::ManuallyDrop::new(rest);
        }
        first
    }

    #[cfg(test)]
    pub(crate) fn concat(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_len);
        for seg in self.segments() {
            out.extend_from_slice(seg);
        }
        out
    }
}

/// Serialize straight into the framed buffer: one allocation, no intermediate
/// `Vec`, and the length prefix is known before encoding starts.
///
/// Large shared bytes fields are retained as extra segments instead of
/// being copied (PK-11); the concatenated bytes are identical.
pub(crate) fn frame_from_msg<T: CodecMessage>(msg: &T, len: usize) -> Result<SegFrame, Status> {
    let mut sink = SegSink::new();
    sink.reserve_new(codec::HEADER_LEN + len);
    write_uncompressed_prefix(&mut sink, len)?;
    msg.encode_payload(&mut sink)?;
    let shared = sink.shared_len();
    let frame = sink.finish();
    crate::copy_counts::note_encode(len.saturating_sub(shared));
    Ok(frame)
}

pub(crate) fn encode_msg<T: CodecMessage>(
    msg: &T,
    codec: Option<CompressionAlgorithm>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<SegFrame, Status> {
    let len = msg.encoded_len();
    limits.check_encode(len)?;
    let Some(codec) = codec else {
        return frame_from_msg(msg, len);
    };
    let body = msg.encode_to_vec()?;
    crate::copy_counts::note_serialize(body.len());
    let mut buf = BytesMut::new();
    append_compressed(&mut buf, &body, codec, gzip_level)?;
    Ok(SegFrame::single(buf.freeze()))
}

const COMPRESSED_LOOKAHEAD_BYTES: usize = 4096;

/// Grow toward the known plaintext size once output reaches half that size.
/// Smaller output grows normally, including highly compressed multi-MiB bodies.
/// This is a capacity hint, not a codec bound: unusual output
/// can still grow normally. A half-size initial guess previously doubled just
/// short of a complete incompressible frame, then doubled again for its footer.
struct CompressedWriter<'a> {
    buffer: &'a mut BytesMut,
    growth_target: usize,
    grow_after: usize,
}

impl Write for CompressedWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buffer.capacity() - self.buffer.len() < data.len() {
            let additional = if self.buffer.len().saturating_add(data.len()) >= self.grow_after {
                self.growth_target.saturating_sub(self.buffer.len())
            } else {
                data.len()
            };
            self.buffer.reserve(additional.max(data.len()));
        }
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Compress directly into the final frame. Small messages keep their existing
/// capacity policy; large output starts small and grows only when needed.
fn append_compressed(
    buffer: &mut BytesMut,
    body: &[u8],
    codec: CompressionAlgorithm,
    level: u32,
) -> Result<usize, Status> {
    let initial = codec::HEADER_LEN + (body.len() / 2 + 32).min(COMPRESSED_LOOKAHEAD_BYTES);
    if buffer.capacity() == 0 {
        *buffer = BytesMut::with_capacity(initial);
    } else {
        buffer.reserve(initial);
    }
    let at = buffer.len();
    BufMut::put_u8(buffer, 1);
    buffer.put_u32(0);
    if body.len() <= COMPRESSED_LOOKAHEAD_BYTES {
        codec.encode_into(body, level, &mut (&mut *buffer).writer())?;
    } else {
        let growth_target = buffer
            .len()
            .saturating_add(body.len())
            .saturating_add(body.len() / 16)
            .saturating_add(64);
        let grow_after = buffer.len().saturating_add(body.len() / 2);
        codec.encode_into(
            body,
            level,
            &mut CompressedWriter {
                buffer,
                growth_target,
                grow_after,
            },
        )?;
    }
    let len = u32::try_from(buffer.len() - at - codec::HEADER_LEN)
        .map_err(|_| Status::internal("message too large"))?;
    patch_frame_len(buffer, at, len);
    Ok(buffer.len() - at)
}

/// Write `len` into the length prefix of the frame starting at `at`.
///
/// `split_at_mut` rather than indexing: `clippy::indexing_slicing` is
/// denied in this crate.
fn patch_frame_len(buf: &mut BytesMut, at: usize, len: u32) {
    let (_, tail) = buf.split_at_mut(at);
    let (_, tail) = tail.split_at_mut(1);
    let (len_bytes, _) = tail.split_at_mut(4);
    len_bytes.copy_from_slice(&len.to_be_bytes());
}

/// How many bytes of stream output to accumulate before handing them to HTTP/2.
///
/// gRPC messages are length-prefixed, so a DATA frame may carry any number of
/// them. Writing one frame per message costs a wakeup and often a syscall per
/// message, which dominates the cost of a small-message stream; batching to
/// 32 KiB amortises that without adding meaningful latency, because a batch is
/// flushed as soon as the producer has nothing more ready.
pub(crate) const STREAM_BATCH_BYTES: usize = 32 * 1024;

/// Append one length-prefixed message to `sink`.
///
/// The uncompressed path serializes straight into `sink`, so a batch of `n`
/// messages costs one head buffer rather than `n`; large shared fields
/// become segments of their own.
pub(crate) fn append_frame_with_ready<T: CodecMessage>(
    sink: &mut SegSink,
    scratch: &mut EncodeScratch,
    msg: &T,
    codec: Option<CompressionAlgorithm>,
    limits: MessageLimits,
    gzip_level: u32,
    ready_messages: usize,
) -> Result<(), Status> {
    let len = msg.encoded_len();
    limits.check_encode(len)?;
    if let Some(codec) = codec {
        return scratch.with_payload(msg, len, |body| {
            let wrote = append_compressed(sink.head_mut(), body, codec, gzip_level)?;
            sink.note_head_wrote(wrote);
            Ok(())
        });
    }
    let need = codec::HEADER_LEN + len;
    // A single already-ready message is flushed before polling for more.
    // Reserve only its frame. Multiple ready messages keep the existing
    // four-frame prefetch, including mixed-size bursts. No second length
    // traversal or speculative wait is needed. Unknown counts stay conservative.
    if sink.is_empty() && need <= 8 * 1024 {
        let prefetch = if ready_messages == 1 { 1 } else { 4 };
        sink.reserve_new(need.saturating_mul(prefetch));
    } else {
        sink.reserve(need);
    }
    write_uncompressed_prefix(sink, len)?;
    msg.encode_payload(sink)?;
    Ok(())
}

#[cfg(test)]
fn append_frame<T: CodecMessage>(
    sink: &mut SegSink,
    scratch: &mut EncodeScratch,
    msg: &T,
    codec: Option<CompressionAlgorithm>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<(), Status> {
    append_frame_with_ready(sink, scratch, msg, codec, limits, gzip_level, 4)
}

/// Compression flag plus the 4-byte length, written into the reserved head.
#[inline(always)]
fn write_uncompressed_prefix(sink: &mut SegSink, len: usize) -> Result<(), Status> {
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    crate::grpc_prefix::write_uncompressed(sink.head_mut(), prefix);
    sink.note_head_wrote(codec::HEADER_LEN);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Payload;

    const ONE_MIB: usize = 1024 * 1024;

    #[test]
    fn large_compressed_output_preserves_bytes_without_geometric_overshoot() {
        let mut noise = vec![0u8; ONE_MIB];
        let mut state = 0x8f67_31abu32;
        for byte in &mut noise {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state.to_le_bytes().first().copied().unwrap_or_default();
        }
        let algorithms = [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate];
        #[cfg(feature = "zstd")]
        let algorithms = algorithms.into_iter().chain([CompressionAlgorithm::Zstd]);
        for algorithm in algorithms {
            for body in [vec![0u8; ONE_MIB], noise.clone(), vec![0u8; 5 * ONE_MIB]] {
                let mut frame = BytesMut::new();
                append_compressed(&mut frame, &body, algorithm, 6).expect("framed output");
                // A well-compressed payload should not reserve a half-megabyte
                // slab; incompressible output should not grow to two megabytes.
                if body.iter().all(|byte| *byte == 0) {
                    assert!(frame.capacity() < 64 * 1024);
                } else {
                    assert!(frame.capacity() < ONE_MIB + ONE_MIB / 4);
                }
                let framed = codec::pop(&mut frame).expect("framing").expect("complete");
                assert!(frame.is_empty());
                let mut reference = Vec::new();
                algorithm
                    .encode_into(&body, 6, &mut reference)
                    .expect("independent encoder");
                assert_eq!(framed.payload.as_ref(), reference);
                assert_eq!(algorithm.decode(&framed.payload).expect("decode"), body);
            }
        }
    }

    #[test]
    fn compressed_output_growth_hint_is_not_an_output_limit() {
        let mut buffer = BytesMut::new();
        let mut writer = CompressedWriter {
            buffer: &mut buffer,
            growth_target: 17,
            grow_after: 0,
        };
        for data in [b"prefix".as_slice(), &[0xAB; 8192], b"suffix".as_slice()] {
            writer.write_all(data).expect("hint can be exceeded");
        }
        let expected = [b"prefix".as_slice(), &[0xAB; 8192], b"suffix".as_slice()].concat();
        assert_eq!(buffer.as_ref(), expected);
    }

    struct ContiguousMessage(usize);

    impl CodecMessage for ContiguousMessage {
        fn encoded_len(&self) -> usize {
            self.0
        }

        fn encode_payload<W: WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.write_contiguous(|buf| {
                // A temporary created by WireOut's generic fallback would
                // have no capacity. Legacy codecs must retain direct access.
                assert!(buf.capacity() - buf.len() >= self.0);
                buf.resize(buf.len() + self.0, 0xAB);
            });
            Ok(())
        }

        fn encode_payload_bufmut<B: BufMut>(&self, _: &mut B) -> Result<(), Status> {
            panic!("legacy contiguous codecs must not enter the append-only path");
        }

        fn decode_payload(payload: Bytes) -> Result<Self, Status> {
            Ok(Self(payload.len()))
        }

        fn empty() -> Self {
            Self(0)
        }
    }

    #[test]
    fn generated_large_scratch_is_reused_across_flushes_and_mixed_sizes() {
        assert!(<Payload as CodecMessage>::ENCODE_APPEND_ONLY);
        let mut scratch = EncodeScratch::new();
        let mut previous = None;
        for size in [ONE_MIB, ONE_MIB, 17, 0, ONE_MIB] {
            let mut message = Payload::new();
            message.set_body(vec![0xAB; size]);
            let expected = pbrs::Serialize::serialize(&message).expect("payload");
            let pointer = scratch
                .with_payload(&message, expected.len(), |body| {
                    assert_eq!(body, expected);
                    Ok(body.as_ptr())
                })
                .expect("serialize");
            if let Some(previous) = previous {
                assert_eq!(pointer, previous, "same canonical allocation reused");
            }
            previous = Some(pointer);
            let workspace = scratch.append.as_ref().expect("retained large allocation");
            assert!(workspace.bytes.is_empty());
            assert!(workspace.lease.is_some());
            assert_eq!(scratch.bytes.capacity(), 0);
        }
    }

    #[test]
    fn custom_contiguous_codecs_keep_direct_encoding_and_release_large_scratch() {
        let mut scratch = EncodeScratch::new();
        for size in [32, ONE_MIB, 0, 70 * 1024, 7] {
            scratch
                .with_payload(&ContiguousMessage(size), size, |body| {
                    assert_eq!(body.len(), size);
                    assert!(body.iter().all(|byte| *byte == 0xAB));
                    Ok(())
                })
                .expect("legacy contiguous codec");
            assert!(scratch.append.is_none());
            assert!(scratch.bytes.capacity() <= MAX_RETAINED_SCRATCH_BYTES);
        }
    }

    #[test]
    fn oversize_retention_is_a_successful_cache_miss() {
        let mut message = Payload::new();
        message.set_body(vec![0xAB; 5 * ONE_MIB]);
        let mut scratch = EncodeScratch::new();
        let len = message.encoded_len();
        scratch
            .with_payload(&message, len, |body| {
                assert_eq!(body.len(), len);
                Ok(())
            })
            .expect("message limits and cache limits are independent");
        assert!(scratch.append.is_none());
        assert_eq!(scratch.bytes.capacity(), 0);
    }

    struct FailingAppend(u8);

    impl CodecMessage for FailingAppend {
        const ENCODE_APPEND_ONLY: bool = true;

        fn encoded_len(&self) -> usize {
            ONE_MIB
        }

        fn encode_payload<W: WireOut>(&self, _: &mut W) -> Result<(), Status> {
            panic!("large append-capable codecs must use their direct encoder");
        }

        fn encode_payload_bufmut<B: BufMut>(&self, out: &mut B) -> Result<(), Status> {
            out.put_u8(0xAB);
            match self.0 {
                1 => Err(Status::internal("partial encode")),
                2 => panic!("codec panic"),
                _ => Ok(()),
            }
        }

        fn decode_payload(_: Bytes) -> Result<Self, Status> {
            Ok(Self(0))
        }

        fn empty() -> Self {
            Self(0)
        }
    }

    #[test]
    fn encode_and_compressor_failures_discard_warm_scratch_even_when_unwinding() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        for mode in 1..=4 {
            let mut scratch = EncodeScratch::new();
            scratch
                .with_payload(&FailingAppend(0), ONE_MIB, |_| Ok(()))
                .expect("warm scratch");
            assert!(scratch.append.is_some());
            let result = catch_unwind(AssertUnwindSafe(|| {
                scratch.with_payload(&FailingAppend(mode), ONE_MIB, |_| match mode {
                    3 => Err(Status::internal("compressor failure")),
                    4 => panic!("compressor panic"),
                    _ => Ok(()),
                })
            }));
            if mode == 2 || mode == 4 {
                assert!(result.is_err());
            } else {
                assert!(result.expect("error without panic").is_err());
            }
            assert!(scratch.append.is_none());
            assert_eq!(scratch.bytes.capacity(), 0);
            scratch
                .with_payload(&FailingAppend(0), ONE_MIB, |body| {
                    assert_eq!(body, [0xAB]);
                    Ok(())
                })
                .expect("reuse after caught error");
        }
    }

    #[test]
    fn single_ready_identity_frame_uses_exact_capacity_at_boundaries() {
        for size in [0, 1, 1024, 8186, 8187, 8188, 32762, 32763, 32764, 65536] {
            // A flush transfers the old sink to HTTP/2, so the next turn has
            // another fresh sink even when the outbound stream stays alive.
            let mut scratch = EncodeScratch::new();
            for _ in 0..2 {
                let mut sink = SegSink::new();
                let message = ContiguousMessage(size);
                append_frame_with_ready(
                    &mut sink,
                    &mut scratch,
                    &message,
                    None,
                    MessageLimits::unlimited(),
                    0,
                    1,
                )
                .expect("single ready frame");
                assert_eq!(sink.head.capacity(), codec::HEADER_LEN + size);
                let expected = frame_from_msg(&message, size).expect("single frame");
                assert_eq!(sink.finish().concat(), expected.concat());
            }
        }
    }

    #[test]
    fn multiple_ready_messages_keep_the_existing_capacity_trace() {
        for count in [2, 3, 4, 64] {
            for mixed in [false, true] {
                let mut before = SegSink::new();
                let mut after = SegSink::new();
                let mut old_scratch = EncodeScratch::new();
                let mut new_scratch = EncodeScratch::new();
                for index in 0..count {
                    let size = if mixed {
                        match index % 5 {
                            0 => 1,
                            1 => 2048,
                            2 => 8192,
                            3 => 33,
                            _ => 32768,
                        }
                    } else {
                        1024
                    };
                    let message = ContiguousMessage(size);
                    append_frame(
                        &mut before,
                        &mut old_scratch,
                        &message,
                        None,
                        MessageLimits::unlimited(),
                        0,
                    )
                    .expect("existing burst frame");
                    append_frame_with_ready(
                        &mut after,
                        &mut new_scratch,
                        &message,
                        None,
                        MessageLimits::unlimited(),
                        0,
                        count - index,
                    )
                    .expect("ready burst frame");
                    assert_eq!(after.head.len(), before.head.len());
                    assert_eq!(after.head.capacity(), before.head.capacity());
                }
                assert_eq!(after.finish().concat(), before.finish().concat());
            }
        }
    }

    #[test]
    fn ready_hint_preserves_shared_segments_and_compressed_frames() {
        for ready in [0, 1, 2, 3, 4, 64, usize::MAX] {
            for size in [0, 32, 1024, 8192, 32768, 65536] {
                let mut message = Payload::new();
                message.set_body(vec![0xAB; size]);
                for compression in [None, Some(CompressionAlgorithm::Gzip)] {
                    let mut before = SegSink::new();
                    let mut after = SegSink::new();
                    append_frame(
                        &mut before,
                        &mut EncodeScratch::new(),
                        &message,
                        compression,
                        MessageLimits::unlimited(),
                        6,
                    )
                    .expect("existing shared or compressed frame");
                    append_frame_with_ready(
                        &mut after,
                        &mut EncodeScratch::new(),
                        &message,
                        compression,
                        MessageLimits::unlimited(),
                        6,
                        ready,
                    )
                    .expect("ready shared or compressed frame");
                    let before = before.finish();
                    let after = after.finish();
                    assert_eq!(after.seg_count(), before.seg_count());
                    assert_eq!(after.concat(), before.concat());
                }
            }
        }
    }

    fn big_payload() -> Payload {
        let mut p = Payload::new();
        p.set_body(vec![0xABu8; ONE_MIB]);
        p
    }

    #[test]
    fn segments_concat_to_identical_frame() {
        // PK-11 differential: segmented bytes == contiguous encoding.
        let msg = big_payload();
        let len = pbrs::Serialize::serialized_len(&msg);
        let frame = frame_from_msg(&msg, len).expect("frame");
        assert!(frame.seg_count() > 1);
        assert_eq!(frame.total_len(), codec::HEADER_LEN + len);
        let mut expect = Vec::with_capacity(codec::HEADER_LEN + len);
        expect.push(0);
        expect.extend_from_slice(&u32::try_from(len).expect("test len fits u32").to_be_bytes());
        expect.extend_from_slice(&pbrs::Serialize::serialize(&msg).expect("serialize"));
        assert_eq!(frame.concat(), expect);
    }

    #[test]
    fn four_small_frames_match_separate_encodes() {
        let mut p = Payload::new();
        p.set_body(vec![0xABu8; 32]);
        let len = pbrs::Serialize::serialized_len(&p);
        let mut sink = SegSink::new();
        let mut scratch = EncodeScratch::new();
        for _ in 0..4 {
            append_frame(
                &mut sink,
                &mut scratch,
                &p,
                None,
                MessageLimits::default(),
                0,
            )
            .expect("append");
        }
        let batched = sink.finish();
        let mut expect = Vec::new();
        for _ in 0..4 {
            expect.extend(frame_from_msg(&p, len).expect("frame").concat());
        }
        assert_eq!(batched.concat(), expect);
        assert_eq!(batched.seg_count(), 1);
    }

    #[test]
    fn compressed_batches_match_independent_message_encodes() {
        for compression in [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate] {
            for level in [0, 1, 9] {
                let mut scratch = EncodeScratch::new();
                let mut sink = SegSink::new();
                let mut expected = Vec::new();
                // Reuse after both growth and a large-message allocation release.
                for size in [0, 1, 1024, 70 * 1024, 2, 0] {
                    let mut message = Payload::new();
                    message.set_body(vec![0xAB; size]);
                    append_frame(
                        &mut sink,
                        &mut scratch,
                        &message,
                        Some(compression),
                        MessageLimits::unlimited(),
                        level,
                    )
                    .expect("batched frame");
                    expected.extend(
                        encode_msg(
                            &message,
                            Some(compression),
                            MessageLimits::unlimited(),
                            level,
                        )
                        .expect("independent frame")
                        .concat(),
                    );
                }
                assert_eq!(sink.finish().concat(), expected);
                assert!(scratch.bytes.capacity() <= MAX_RETAINED_SCRATCH_BYTES);
            }
        }
    }

    #[test]
    #[cfg(feature = "prost")]
    fn compressed_prost_batches_match_independent_message_encodes() {
        #[derive(Clone, PartialEq, prost::Message)]
        struct Sample {
            #[prost(bytes = "vec", tag = "1")]
            payload: Vec<u8>,
        }
        let mut scratch = EncodeScratch::new();
        let mut sink = SegSink::new();
        let mut expected = Vec::new();
        for size in [1024, 0, 32, 4096] {
            let message = crate::codec::prost::Message(Sample {
                payload: vec![0xAB; size],
            });
            append_frame(
                &mut sink,
                &mut scratch,
                &message,
                Some(CompressionAlgorithm::Gzip),
                MessageLimits::unlimited(),
                1,
            )
            .expect("batched prost frame");
            expected.extend(
                encode_msg(
                    &message,
                    Some(CompressionAlgorithm::Gzip),
                    MessageLimits::unlimited(),
                    1,
                )
                .expect("independent prost frame")
                .concat(),
            );
        }
        assert_eq!(sink.finish().concat(), expected);
    }

    #[test]
    #[cfg(feature = "prost")]
    fn reusable_large_frames_match_separate_encodes_for_both_message_families() {
        #[derive(Clone, PartialEq, prost::Message)]
        struct Sample {
            #[prost(bytes = "vec", tag = "1")]
            payload: Vec<u8>,
        }

        fn check<T: CodecMessage>(
            message: &T,
            scratch: &mut EncodeScratch,
            compression: CompressionAlgorithm,
        ) {
            let mut sink = SegSink::new();
            append_frame(
                &mut sink,
                scratch,
                message,
                Some(compression),
                MessageLimits::unlimited(),
                6,
            )
            .expect("reusable compressed frame");
            let expected = encode_msg(message, Some(compression), MessageLimits::unlimited(), 6)
                .expect("independent compressed frame");
            assert_eq!(sink.finish().concat(), expected.concat());
        }

        let algorithms = [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate];
        #[cfg(feature = "zstd")]
        let algorithms = algorithms.into_iter().chain([CompressionAlgorithm::Zstd]);
        for compression in algorithms {
            for noise in [false, true] {
                let mut native_scratch = EncodeScratch::new();
                let mut prost_scratch = EncodeScratch::new();
                for size in [ONE_MIB, 0, 1, 70 * 1024, ONE_MIB] {
                    let mut state = 0x1234_5678u32;
                    let body: Vec<u8> = (0..size)
                        .map(|_| {
                            state ^= state << 13;
                            state ^= state >> 17;
                            state ^= state << 5;
                            if noise {
                                state.to_le_bytes().first().copied().unwrap_or_default()
                            } else {
                                0
                            }
                        })
                        .collect();
                    let mut native = Payload::new();
                    native.set_body(body.clone());
                    check(&native, &mut native_scratch, compression);
                    check(
                        &crate::codec::prost::Message(Sample { payload: body }),
                        &mut prost_scratch,
                        compression,
                    );
                }
            }
        }
    }

    #[test]
    fn small_message_stays_single_segment() {
        // Fast-path guard: nothing over the threshold, one segment.
        let mut p = Payload::new();
        p.set_body(vec![0xABu8; 16]);
        let len = pbrs::Serialize::serialized_len(&p);
        let frame = frame_from_msg(&p, len).expect("frame");
        assert_eq!(frame.seg_count(), 1);
        assert_eq!(frame.total_len(), codec::HEADER_LEN + len);
    }

    #[test]
    #[cfg(feature = "copy-counts")]
    fn shared_send_removes_outbound_copy() {
        // SB-13: the 1 MiB field is witnessed as shared, and the encode
        // counter excludes it (only small head bytes remain). Lower bounds
        // for shared (parallel tests can only inflate); a generous upper
        // bound for encode (parallel tiny encodes add at most KiBs, while
        // a regression to full-copy would add the whole MiB).
        use crate::copy_counts::copy_counts;

        let msg = big_payload();
        let len = pbrs::Serialize::serialized_len(&msg);
        let before = copy_counts();
        let frame = frame_from_msg(&msg, len).expect("frame");
        let after = copy_counts();
        assert_eq!(frame.total_len(), codec::HEADER_LEN + len);
        assert!(after.shared_segments - before.shared_segments >= 1);
        assert!(after.shared_bytes - before.shared_bytes >= ONE_MIB as u64);
        assert!(after.encode_bytes - before.encode_bytes < 100_000);
    }

    #[test]
    fn retained_output_charges_full_backing_and_reclaims_after_frame_drop() {
        let payload = vec![0xAB; 131073];
        let mut sink = SegSink::new();
        sink.reserve_new(payload.len() * 2);
        sink.put_slice(&payload);
        let capacity = sink.head.capacity();
        let pointer = sink.head.as_ptr();
        let frame = sink.take_reusing(true);
        assert_eq!(frame.first.as_ref(), payload);
        assert!(sink.head.is_empty());
        assert!(sink.head.capacity() < capacity);
        assert_eq!(
            sink.retained_head
                .as_ref()
                .expect("bounded output lease")
                .capacity,
            capacity
        );
        drop(frame);
        sink.prepare_encode();
        assert!(
            sink.retained_head
                .as_ref()
                .expect("active metadata")
                ._lease
                .is_none()
        );
        assert_eq!(sink.head.as_ptr(), pointer);
        assert_eq!(sink.head.capacity(), capacity);
        sink.put_slice(&payload);
        let next = sink.take_reusing(true);
        assert_eq!(next.first.as_ptr(), pointer);
        assert_eq!(next.first.as_ref(), payload);
    }

    #[test]
    fn retained_immutable_output_survives_an_independent_next_encode() {
        let payload = vec![0xAB; 131073];
        let mut sink = SegSink::new();
        sink.reserve_new(payload.len() * 2);
        sink.put_slice(&payload);
        let first = sink.take_reusing(true);
        let pointer = first.first.as_ptr();
        sink.prepare_encode();
        assert!(
            sink.retained_head
                .as_ref()
                .expect("active metadata")
                ._lease
                .is_none()
        );
        assert!(
            sink.head.capacity() > 0,
            "shared spare tail remains available"
        );
        let next = vec![0xCD; payload.len()];
        sink.reserve_new(next.len());
        sink.put_slice(&next);
        let second = sink.take_reusing(true);
        assert_eq!(second.first.as_ref(), next);
        assert_eq!(first.first.as_ptr(), pointer);
        assert_eq!(first.first.as_ref(), payload);
    }

    #[test]
    fn oversized_backing_is_not_retained_for_a_short_output() {
        let mut sink = SegSink::new();
        sink.reserve_new(4 * 1024 * 1024 + 1);
        sink.put_slice(b"x");
        let frame = sink.take_reusing(true);
        assert_eq!(frame.first.as_ref(), b"x");
        assert!(sink.retained_head.is_none());
        assert_eq!(sink.head.capacity(), 0);
    }
}
