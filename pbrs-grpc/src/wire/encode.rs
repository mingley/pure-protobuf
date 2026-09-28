//! Outbound message framing: length-prefix encoding.

use crate::codec;
use crate::compression::Codec;
use crate::limits::MessageLimits;
use crate::status::Status;
use bytes::{BufMut, Bytes, BytesMut};
use pbrs::Serialize;
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
        self.segs.truncate(cp.segs);
        self.head.truncate(cp.head);
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
pub(crate) fn frame_from_msg<T: Serialize>(msg: &T, len: usize) -> Result<SegFrame, Status> {
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    let mut sink = SegSink::new();
    sink.reserve(codec::HEADER_LEN + len);
    sink.put_u8(0);
    sink.put_slice(&prefix.to_be_bytes());
    T::encode(msg, &mut sink).map_err(|e| Status::internal(e.to_string()))?;
    let shared = sink.shared_len();
    let frame = sink.finish();
    crate::copy_counts::note_encode(len.saturating_sub(shared));
    Ok(frame)
}

pub(crate) fn encode_msg<T: Serialize>(
    msg: &T,
    codec: Option<Codec>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<SegFrame, Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    let Some(codec) = codec else {
        return frame_from_msg(msg, len);
    };
    let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
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
pub(crate) fn append_frame<T: Serialize>(
    sink: &mut SegSink,
    msg: &T,
    codec: Option<Codec>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<(), Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if let Some(codec) = codec {
        let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
        crate::copy_counts::note_serialize(body.len());
        let head = sink.head_mut();
        head.reserve(codec::HEADER_LEN + body.len() / 2 + 32);
        let at = head.len();
        BufMut::put_u8(&mut *head, 1);
        head.put_u32(0);
        {
            let mut writer = (&mut *head).writer();
            codec.encode_into(&body, gzip_level, &mut writer)?;
        }
        let len = u32::try_from(head.len() - at - codec::HEADER_LEN)
            .map_err(|_| Status::internal("message too large"))?;
        patch_frame_len(head, at, len);
        let wrote = head.len() - at;
        sink.note_head_wrote(wrote);
        return Ok(());
    }
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    sink.reserve(codec::HEADER_LEN + len);
    sink.put_u8(0);
    sink.put_slice(&prefix.to_be_bytes());
    T::encode(msg, sink).map_err(|e| Status::internal(e.to_string()))?;
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
        expect.extend_from_slice(&(len as u32).to_be_bytes());
        expect.extend_from_slice(&pbrs::Serialize::serialize(&msg).expect("serialize"));
        assert_eq!(frame.concat(), expect);
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
