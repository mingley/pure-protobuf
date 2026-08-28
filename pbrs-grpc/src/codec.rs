//! gRPC length-prefixed framing.
//!
//! Every gRPC message travels as a 5-byte header followed by its payload:
//!
//! ```text
//! +-----------------+-------------------------+----------------+
//! | Compressed-Flag | Message-Length (u32 BE)  |    Payload     |
//! |     1 byte      |         4 bytes          |  Length bytes  |
//! +-----------------+-------------------------+----------------+
//! ```
//!
//! The length is validated against the inbound cap the moment the header is
//! complete, so an oversize claim is refused before any payload memory is
//! reserved.

use crate::limits::MessageLimits;
use crate::status::Status;
use bytes::{BufMut, Bytes, BytesMut};

/// Size of the gRPC length-prefix header.
pub const HEADER_LEN: usize = 5;

/// One length-prefixed frame lifted off the wire.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Compressed-Flag: the payload is gzip of the protobuf bytes.
    pub compressed: bool,
    /// Frame payload, still compressed if [`Self::compressed`] is set.
    pub payload: Bytes,
}

/// Frame `payload`, setting the Compressed-Flag to `compressed`.
///
/// ```
/// use pbrs_grpc::codec;
///
/// let frame = codec::encode(b"hi", false)?;
/// assert_eq!(&frame[..], &[0, 0, 0, 0, 2, b'h', b'i']);
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
pub fn encode(payload: &[u8], compressed: bool) -> Result<Bytes, Status> {
    let len = u32::try_from(payload.len()).map_err(|_| Status::internal("message too large"))?;
    let mut buf = BytesMut::with_capacity(HEADER_LEN + payload.len());
    buf.put_u8(u8::from(compressed));
    buf.put_u32(len);
    buf.extend_from_slice(payload);
    Ok(buf.freeze())
}

/// Take one complete frame off the front of `buf`.
///
/// `Ok(None)` means the frame is not fully buffered yet and `buf` was left
/// untouched.
///
/// ```
/// use bytes::BytesMut;
/// use pbrs_grpc::codec;
///
/// let wire = codec::encode(b"hi", false)?;
/// let mut buf = BytesMut::from(&wire[..4]);
/// assert!(codec::pop(&mut buf)?.is_none());
///
/// buf.extend_from_slice(&wire[4..]);
/// let frame = codec::pop(&mut buf)?.expect("complete");
/// assert_eq!(&frame.payload[..], b"hi");
/// # Ok::<(), pbrs_grpc::Status>(())
/// ```
pub fn pop(buf: &mut BytesMut) -> Result<Option<Frame>, Status> {
    pop_limited(buf, MessageLimits::unlimited())
}

/// [`pop`] with an inbound size cap.
///
/// The claimed length is checked against `limits` as soon as the 5-byte header
/// is present, so a hostile header cannot make the caller buffer more than the
/// cap allows. Oversize is
/// [`Code::ResourceExhausted`](crate::Code::ResourceExhausted).
pub fn pop_limited(buf: &mut BytesMut, limits: MessageLimits) -> Result<Option<Frame>, Status> {
    let Some(header) = buf.get(..HEADER_LEN) else {
        return Ok(None);
    };
    let (flag, len_bytes) = header.split_at(1);
    let flag = flag.first().copied().unwrap_or_default();
    if flag > 1 {
        return Err(Status::internal(format!(
            "invalid gRPC compressed-flag {flag}"
        )));
    }
    let mut len_be = [0u8; 4];
    len_be.copy_from_slice(len_bytes);
    let len = usize::try_from(u32::from_be_bytes(len_be))
        .map_err(|_| Status::internal("message too large"))?;
    limits.check_decode(len)?;
    let total = HEADER_LEN
        .checked_add(len)
        .ok_or_else(|| Status::internal("message too large"))?;
    if buf.len() < total {
        return Ok(None);
    }
    drop(buf.split_to(HEADER_LEN));
    Ok(Some(Frame {
        compressed: flag == 1,
        payload: buf.split_to(len).freeze(),
    }))
}

/// Take one complete frame off the front of an immutable chunk.
///
/// Returns the frame plus the unconsumed remainder. This is the zero-copy
/// path: the payload is a slice of `chunk`, so a frame that arrived whole in
/// one HTTP/2 DATA frame is never copied into an intermediate buffer.
pub(crate) fn pop_from_chunk(
    chunk: &mut Bytes,
    limits: MessageLimits,
) -> Result<Option<Frame>, Status> {
    let Some(header) = chunk.get(..HEADER_LEN) else {
        return Ok(None);
    };
    let (flag, len_bytes) = header.split_at(1);
    let flag = flag.first().copied().unwrap_or_default();
    if flag > 1 {
        return Err(Status::internal(format!(
            "invalid gRPC compressed-flag {flag}"
        )));
    }
    let mut len_be = [0u8; 4];
    len_be.copy_from_slice(len_bytes);
    let len = usize::try_from(u32::from_be_bytes(len_be))
        .map_err(|_| Status::internal("message too large"))?;
    limits.check_decode(len)?;
    let total = HEADER_LEN
        .checked_add(len)
        .ok_or_else(|| Status::internal("message too large"))?;
    if chunk.len() < total {
        return Ok(None);
    }
    drop(chunk.split_to(HEADER_LEN));
    Ok(Some(Frame {
        compressed: flag == 1,
        payload: chunk.split_to(len),
    }))
}

#[cfg(test)]
mod tests {
    use super::{encode, pop, pop_from_chunk, pop_limited};
    use crate::limits::MessageLimits;
    use crate::status::Code;
    use bytes::BytesMut;

    #[test]
    fn roundtrip_empty_and_payload() {
        let empty = encode(&[], false).expect("encode");
        let mut buf = BytesMut::from(empty.as_ref());
        let got = pop(&mut buf).expect("pop").expect("frame");
        assert!(!got.compressed);
        assert!(got.payload.is_empty());
        assert!(buf.is_empty());

        let payload = b"hello";
        let framed = encode(payload, true).expect("encode");
        buf.extend_from_slice(&framed);
        let got = pop(&mut buf).expect("pop").expect("frame");
        assert!(got.compressed);
        assert_eq!(&got.payload[..], payload);
    }

    #[test]
    fn incomplete_waits_without_consuming() {
        let framed = encode(&[1, 2, 3], false).expect("encode");
        let mut buf = BytesMut::from(&framed[..4]);
        assert!(pop(&mut buf).expect("pop").is_none());
        assert_eq!(buf.len(), 4);
        buf.extend_from_slice(&framed[4..]);
        let got = pop(&mut buf).expect("pop").expect("frame");
        assert_eq!(&got.payload[..], &[1, 2, 3]);
    }

    #[test]
    fn oversize_is_rejected_from_the_header_alone() {
        let framed = encode(&[0u8; 16], false).expect("encode");
        let mut buf = BytesMut::from(&framed[..5]);
        let err = pop_limited(&mut buf, MessageLimits::unlimited().with_max_decoding(8))
            .expect_err("oversize");
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn reserved_compressed_flag_values_are_rejected() {
        let mut buf = BytesMut::from(&[2u8, 0, 0, 0, 0][..]);
        let err = pop(&mut buf).expect_err("bad flag");
        assert_eq!(err.code(), Code::Internal);
    }

    #[test]
    fn chunk_pop_slices_without_copying() {
        let a = encode(b"one", false).expect("encode");
        let b = encode(b"two", false).expect("encode");
        let mut joined = BytesMut::from(a.as_ref());
        joined.extend_from_slice(&b);
        let mut chunk = joined.freeze();

        let first = pop_from_chunk(&mut chunk, MessageLimits::unlimited())
            .expect("pop")
            .expect("frame");
        assert_eq!(&first.payload[..], b"one");
        let second = pop_from_chunk(&mut chunk, MessageLimits::unlimited())
            .expect("pop")
            .expect("frame");
        assert_eq!(&second.payload[..], b"two");
        assert!(pop_from_chunk(&mut chunk, MessageLimits::unlimited())
            .expect("pop")
            .is_none());
        assert!(chunk.is_empty());
    }

    #[test]
    fn chunk_pop_leaves_partial_frames_intact() {
        let wire = encode(b"partial", false).expect("encode");
        let mut chunk = wire.slice(..6);
        assert!(pop_from_chunk(&mut chunk, MessageLimits::unlimited())
            .expect("pop")
            .is_none());
        assert_eq!(chunk.len(), 6);
    }
}
