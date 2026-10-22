//! Outbound message framing: length-prefix encoding.

use crate::codec;
use crate::codec::CodecMessage;
use crate::compression::CompressionAlgorithm;
use crate::limits::MessageLimits;
use crate::status::Status;
use bytes::{BufMut, Bytes, BytesMut};
use pbrs::rt::WireOut;

/// Segmented outbound sink (PK-11): small writes accumulate in `head`;
/// large shared field buffers handed to `put_shared` become their own
/// segments without copying. With no shared field the sink holds exactly
/// one segment and behaves like the old single buffer.
pub(crate) struct SegSink {
    head: BytesMut,
    segs: Vec<Bytes>,
    total_len: usize,
    shared_len: usize,
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
            segs: Vec::new(),
            total_len: 0,
            shared_len: 0,
        }
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
            if self.segs.capacity() == 0 {
                std::mem::forget(self);
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
/// mutable codec state. Large messages release their scratch allocation after
/// compression instead of retaining a message-sized buffer for the RPC lifetime.
pub(crate) struct EncodeScratch {
    bytes: BytesMut,
}

const MAX_RETAINED_SCRATCH_BYTES: usize = 64 * 1024;

impl EncodeScratch {
    pub(crate) fn new() -> Self {
        Self {
            bytes: BytesMut::new(),
        }
    }

    fn with_payload<T: CodecMessage, R>(
        &mut self,
        msg: &T,
        len: usize,
        use_payload: impl FnOnce(&[u8]) -> Result<R, Status>,
    ) -> Result<R, Status> {
        self.bytes.clear();
        self.bytes.reserve(len);
        let result = msg.encode_payload(self).and_then(|()| {
            crate::copy_counts::note_serialize(self.bytes.len());
            use_payload(&self.bytes)
        });
        if self.bytes.capacity() > MAX_RETAINED_SCRATCH_BYTES {
            self.bytes = BytesMut::new();
        } else {
            self.bytes.clear();
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
            std::mem::forget(rest);
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
    // Compress straight into the framed buffer: one allocation instead of
    // two, and no second copy of the compressed bytes. The length prefix
    // is patched once the stream ends. The gzip path stays contiguous.
    let mut buf = BytesMut::with_capacity(codec::HEADER_LEN + body.len() / 2 + 32);
    BufMut::put_u8(&mut buf, 1);
    buf.put_u32(0);
    {
        let mut writer = (&mut buf).writer();
        codec.encode_into(&body, gzip_level, &mut writer)?;
    }
    let len = u32::try_from(buf.len() - codec::HEADER_LEN)
        .map_err(|_| Status::internal("message too large"))?;
    patch_frame_len(&mut buf, 0, len);
    Ok(SegFrame::single(buf.freeze()))
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
pub(crate) fn append_frame<T: CodecMessage>(
    sink: &mut SegSink,
    scratch: &mut EncodeScratch,
    msg: &T,
    codec: Option<CompressionAlgorithm>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<(), Status> {
    let len = msg.encoded_len();
    limits.check_encode(len)?;
    if let Some(codec) = codec {
        return scratch.with_payload(msg, len, |body| {
            let head = sink.head_mut();
            head.reserve(codec::HEADER_LEN + body.len() / 2 + 32);
            let at = head.len();
            BufMut::put_u8(&mut *head, 1);
            head.put_u32(0);
            {
                let mut writer = (&mut *head).writer();
                codec.encode_into(body, gzip_level, &mut writer)?;
            }
            let len = u32::try_from(head.len() - at - codec::HEADER_LEN)
                .map_err(|_| Status::internal("message too large"))?;
            patch_frame_len(head, at, len);
            let wrote = head.len() - at;
            sink.note_head_wrote(wrote);
            Ok(())
        });
    }
    let need = codec::HEADER_LEN + len;
    // A batch of equal frames otherwise reallocates and copies the prefix on
    // every message. The first small frame reserves room for three more.
    // `frame_from_msg` keeps the exact size for a single message.
    if sink.is_empty() && need <= 8 * 1024 {
        sink.reserve_new(need.saturating_mul(4));
    } else {
        sink.reserve(need);
    }
    write_uncompressed_prefix(sink, len)?;
    msg.encode_payload(sink)?;
    Ok(())
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
}
