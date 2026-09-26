//! Outbound message framing: length-prefix encoding.

use crate::codec;
use crate::gzip;
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
    compress: bool,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<Bytes, Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if !compress {
        return frame_from_msg(msg, len);
    }
    let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
    let gz = gzip::encode_level(&body, gzip_level)?;
    codec::encode(&gz, true)
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
    compress: bool,
    limits: MessageLimits,
    gzip_level: u32,
) -> Result<(), Status> {
    let len = T::serialized_len(msg);
    limits.check_encode(len)?;
    if compress {
        let body = T::serialize(msg).map_err(|e| Status::internal(e.to_string()))?;
        let gz = gzip::encode_level(&body, gzip_level)?;
        let prefix = u32::try_from(gz.len()).map_err(|_| Status::internal("message too large"))?;
        buf.reserve(codec::HEADER_LEN + gz.len());
        buf.put_u8(1);
        buf.put_u32(prefix);
        buf.extend_from_slice(&gz);
        return Ok(());
    }
    let prefix = u32::try_from(len).map_err(|_| Status::internal("message too large"))?;
    buf.reserve(codec::HEADER_LEN + len);
    buf.put_u8(0);
    buf.put_u32(prefix);
    T::encode(msg, buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(())
}
