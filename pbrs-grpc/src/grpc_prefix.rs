//! Five-byte gRPC prefix written into a buffer that already has spare room.

#![allow(
    unsafe_code,
    reason = "the length check guarantees five initialized spare bytes in the head"
)]

use bytes::{BufMut, BytesMut};

/// Write a cleartext gRPC prefix. The grow path stays out of line.
#[inline(always)]
pub(crate) fn write_uncompressed(head: &mut BytesMut, len: u32) {
    let len_now = head.len();
    let spare = head.capacity() - len_now;
    if spare >= crate::codec::HEADER_LEN {
        // SAFETY: `spare` is `capacity - len`, so five bytes at `len` are in
        // the allocation. `set_len` publishes them after the stores.
        unsafe {
            let dst = head.spare_capacity_mut().as_mut_ptr().cast::<u8>();
            dst.write(0);
            dst.add(1).cast::<u32>().write_unaligned(len.to_be());
            head.set_len(len_now + crate::codec::HEADER_LEN);
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

#[cfg(test)]
mod tests {
    #[test]
    fn prefix_initializes_exactly_five_bytes_with_and_without_spare_capacity() {
        for offset in 0..8 {
            for spare in [0, 4, 5, 16] {
                for len in [0_u32, 1, 255, 65536, u32::MAX] {
                    let mut head = bytes::BytesMut::with_capacity(offset + spare);
                    head.extend_from_slice(&vec![0xab; offset]);
                    let mut expected = head.to_vec();
                    expected.push(0);
                    expected.extend_from_slice(&len.to_be_bytes());
                    super::write_uncompressed(&mut head, len);
                    assert_eq!(head.as_ref(), expected.as_slice());
                }
            }
        }
    }
}
