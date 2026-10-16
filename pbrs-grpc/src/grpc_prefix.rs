//! Five-byte gRPC prefix written into a buffer that already has spare room.

#![allow(
    unsafe_code,
    reason = "the length check guarantees five initialized spare bytes in the head"
)]

use bytes::{BufMut, BytesMut};

/// Write a cleartext gRPC prefix. The grow path stays out of line.
#[inline(always)]
pub(crate) fn write_uncompressed(head: &mut BytesMut, len: u32) {
    if head.remaining_mut() >= crate::codec::HEADER_LEN {
        // SAFETY: `remaining_mut` is the spare capacity `chunk_mut` exposes.
        unsafe {
            let dst = head.chunk_mut().as_mut_ptr();
            dst.write(0);
            dst.add(1).cast::<u32>().write_unaligned(len.to_be());
            head.advance_mut(crate::codec::HEADER_LEN);
        }
    } else {
        write_uncompressed_slow(head, len);
    }
}

#[inline(never)]
fn write_uncompressed_slow(head: &mut BytesMut, len: u32) {
    head.put_u8(0);
    head.put_u32(len);
}
