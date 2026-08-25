//! gRPC length-prefix framing: Compressed-Flag + 4-byte big-endian length.

use crate::status::Status;
use bytes::{BufMut, Bytes, BytesMut};

/// One decoded gRPC data frame.
#[derive(Debug)]
pub struct Frame {
    /// Compressed-Flag (1 = gzip of the protobuf bytes).
    pub compressed: bool,
    /// Frame payload (still compressed if [`Self::compressed`] is set).
    pub payload: Bytes,
}

/// Encode one gRPC data frame. `compressed` is the Compressed-Flag.
pub fn encode(payload: &[u8], compressed: bool) -> Result<Bytes, Status> {
    let len = u32::try_from(payload.len()).map_err(|_| Status::internal("message too large"))?;
    let mut buf = BytesMut::with_capacity(5 + payload.len());
    buf.put_u8(u8::from(compressed));
    buf.put_u32(len);
    buf.extend_from_slice(payload);
    Ok(buf.freeze())
}

/// Pop one complete frame from `buf`. `Ok(None)` if more bytes are needed.
pub fn pop(buf: &mut BytesMut) -> Result<Option<Frame>, Status> {
    if buf.len() < 5 {
        return Ok(None);
    }
    let flag = buf
        .first()
        .copied()
        .ok_or_else(|| Status::internal("short frame"))?;
    if flag > 1 {
        return Err(Status::internal("invalid compressed-flag"));
    }
    let mut len_bytes = [0u8; 4];
    let slice = buf
        .get(1..5)
        .ok_or_else(|| Status::internal("short frame"))?;
    len_bytes.copy_from_slice(slice);
    let len = usize::try_from(u32::from_be_bytes(len_bytes))
        .map_err(|_| Status::internal("message too large"))?;
    let total = 5usize
        .checked_add(len)
        .ok_or_else(|| Status::internal("message too large"))?;
    if buf.len() < total {
        return Ok(None);
    }
    drop(buf.split_to(5));
    Ok(Some(Frame {
        compressed: flag == 1,
        payload: buf.split_to(len).freeze(),
    }))
}

#[cfg(test)]
mod tests {
    use super::{encode, pop};
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
    fn incomplete_waits() {
        let framed = encode(&[1, 2, 3], false).expect("encode");
        let mut buf = BytesMut::from(&framed[..4]);
        assert!(pop(&mut buf).expect("pop").is_none());
        buf.extend_from_slice(&framed[4..]);
        let got = pop(&mut buf).expect("pop").expect("frame");
        assert_eq!(&got.payload[..], &[1, 2, 3]);
    }
}
