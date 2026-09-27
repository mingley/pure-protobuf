//! Outbound message framing: length-prefix encoding.

use crate::codec;
use crate::compression::Codec;
use crate::limits::MessageLimits;
use crate::status::Status;
use bytes::{BufMut, Bytes, BytesMut};
use pbrs::Serialize;

/// Serialize straight into the framed buffer: one allocation, no intermediate
/// `Vec`, and the length prefix is known before encoding starts.
pub(crate) fn frame_from_msg<T: Serialize>(msg: &T, len: usize) -> Result<Bytes, Status> {
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    let mut buf = BytesMut::with_capacity(codec::HEADER_LEN + len);
    buf.put_u8(0);
    buf.put_u32(prefix);
    T::encode(msg, &mut buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(buf.freeze())
}

pub(crate) fn encode_msg<T: Serialize>(
    msg: &T,
    codec: Option<Codec>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<Bytes, Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    let Some(codec) = codec else {
        return frame_from_msg(msg, len);
    };
    let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
    // Compress straight into the framed buffer: one allocation instead of
    // two, and no second copy of the compressed bytes. The length prefix
    // is patched once the stream ends.
    let mut buf = BytesMut::with_capacity(codec::HEADER_LEN + body.len() / 2 + 32);
    buf.put_u8(1);
    buf.put_u32(0);
    {
        let mut writer = (&mut buf).writer();
        codec.encode_into(&body, gzip_level, &mut writer)?;
    }
    let len = u32::try_from(buf.len() - codec::HEADER_LEN)
        .map_err(|_| Status::internal("message too large"))?;
    patch_frame_len(&mut buf, 0, len);
    Ok(buf.freeze())
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

/// Append one length-prefixed message to `buf`.
///
/// The uncompressed path serializes straight into `buf`, so a batch of `n`
/// messages costs one buffer rather than `n`.
pub(crate) fn append_frame<T: Serialize>(
    buf: &mut BytesMut,
    msg: &T,
    codec: Option<Codec>,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<(), Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if let Some(codec) = codec {
        let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
        buf.reserve(codec::HEADER_LEN + body.len() / 2 + 32);
        let at = buf.len();
        buf.put_u8(1);
        buf.put_u32(0);
        {
            let mut writer = (&mut *buf).writer();
            codec.encode_into(&body, gzip_level, &mut writer)?;
        }
        let len = u32::try_from(buf.len() - at - codec::HEADER_LEN)
            .map_err(|_| Status::internal("message too large"))?;
        patch_frame_len(buf, at, len);
        return Ok(());
    }
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    buf.reserve(codec::HEADER_LEN + len);
    buf.put_u8(0);
    buf.put_u32(prefix);
    T::encode(msg, buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}
