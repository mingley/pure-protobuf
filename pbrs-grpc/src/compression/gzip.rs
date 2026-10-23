//! `gzip` coding (RFC 1952) for [`Codec`](super::Codec).
//!
//! Backed by `miniz_oxide` through `flate2`.
//!
//! All members in one payload contribute to the same decoded-message cap.
//! Each member's checksum and size footer are validated; non-member tails
//! are rejected. Use [`decode_limited`] for untrusted input.

use crate::limits::MessageLimits;
use crate::status::Status;
use flate2::Compression;
use flate2::bufread::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{self, BufRead, Read, Write};

/// Message-local member decoder shared by native and tonic inbound paths.
/// Admission precedes each reset, whose DEFLATE history clear has fixed cost.
pub(crate) struct GzipMembers<R> {
    state: MemberState<R>,
    at_member_end: bool,
    frames: usize,
    max_frames: Option<usize>,
    failure: Option<Status>,
}

enum MemberState<R> {
    First(R),
    Active(GzDecoder<R>),
}

impl<R: BufRead + Default> GzipMembers<R> {
    pub(crate) fn new(reader: R, max_frames: Option<usize>) -> Self {
        Self {
            state: MemberState::First(reader),
            at_member_end: false,
            frames: 0,
            max_frames,
            failure: None,
        }
    }

    #[cfg(any(feature = "tonic", test))]
    pub(crate) fn get_ref(&self) -> &R {
        match &self.state {
            MemberState::First(reader) => reader,
            MemberState::Active(decoder) => decoder.get_ref(),
        }
    }

    pub(crate) fn get_mut(&mut self) -> &mut R {
        match &mut self.state {
            MemberState::First(reader) => reader,
            MemberState::Active(decoder) => decoder.get_mut(),
        }
    }

    pub(crate) fn take_failure(&mut self) -> Option<Status> {
        self.failure.take()
    }

    fn admit_member(&mut self) -> io::Result<()> {
        if let Some(max) = self.max_frames.filter(|&max| self.frames >= max) {
            self.failure = Some(Status::resource_exhausted(format!(
                "gzip member count exceeds limit {max}"
            )));
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "gzip member limit",
            ));
        }
        self.frames = self.frames.checked_add(1).ok_or_else(|| {
            self.failure = Some(Status::resource_exhausted("gzip member count overflow"));
            io::Error::new(io::ErrorKind::InvalidData, "gzip member count overflow")
        })?;
        Ok(())
    }
}

impl<R: BufRead + Default> Read for GzipMembers<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            if matches!(self.state, MemberState::First(_)) {
                self.admit_member()?;
                let reader = std::mem::take(self.get_mut());
                self.state = MemberState::Active(GzDecoder::new(reader));
            }
            if self.at_member_end {
                // Preserve this state on WouldBlock/Interrupted: retrying must
                // inspect the next member instead of reading the completed one.
                if self.get_mut().fill_buf()?.is_empty() {
                    return Ok(0);
                }
                self.admit_member()?;
                if let MemberState::Active(decoder) = &mut self.state {
                    let reader = std::mem::take(decoder.get_mut());
                    let _ = decoder.reset(reader);
                }
                self.at_member_end = false;
            }
            if let MemberState::Active(decoder) = &mut self.state {
                let n = decoder.read(output)?;
                if n != 0 {
                    return Ok(n);
                }
                self.at_member_end = true;
            }
        }
    }
}

/// gzip `payload` at the kernel default (deflate level 1, [`Compression::fast`]).
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    encode_level(payload, crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL)
}

/// gzip `payload` at deflate `level` (0 stores, 1 is fast, 9 is best).
///
/// Values above 9 are clamped to 9. Default 1: at gRPC message sizes the extra
/// CPU of higher levels often costs more latency than the saved bytes buy back.
pub fn encode_level(payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
    let mut out = Vec::with_capacity(payload.len() / 2 + 32);
    encode_into(payload, level, &mut out)?;
    Ok(out)
}

/// gzip `payload` at `level`, appending to `out`.
///
/// The framing layer compresses straight into the frame buffer through
/// this, saving the intermediate `Vec` and its copy.
pub(super) fn encode_into(payload: &[u8], level: u32, out: &mut impl Write) -> Result<(), Status> {
    let mut enc = GzEncoder::new(&mut *out, Compression::new(level.min(9)));
    enc.write_all(payload)
        .map_err(|e| Status::internal(format!("gzip encode: {e}")))?;
    enc.finish()
        .map_err(|e| Status::internal(format!("gzip encode: {e}")))?;
    Ok(())
}

/// Inflate `payload` with no cap.
///
/// Prefer [`decode_limited`]. This is only safe against a trusted peer.
pub fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    decode_limited(payload, MessageLimits::unlimited())
}

/// Inflate every RFC 1952 member in `payload` under one decoded-message cap.
/// Non-member bytes and malformed later members are rejected.
///
/// The output buffer capacity stays within the cap. A fixed 8 KiB stack
/// chunk probes one extra decoded byte to detect overflow.
/// Exceeding it is [`Code::ResourceExhausted`](crate::Code::ResourceExhausted),
/// reported against the cap rather than the true inflated size, because
/// learning the true size is exactly the work being refused.
pub fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    decode_limited_into(payload, limits, Vec::new())
}

pub(super) fn decode_limited_into(
    payload: &[u8],
    limits: MessageLimits,
    out: Vec<u8>,
) -> Result<Vec<u8>, Status> {
    // Frame bytes already implement BufRead; no extra compressed-input buffer.
    let mut decoder = GzipMembers::new(payload, limits.max_compression_frames());
    let result = super::read_bounded_into(
        &mut decoder,
        limits.inflate_budget(),
        payload.len(),
        "gzip",
        out,
    );
    if let Some(status) = decoder.take_failure() {
        return Err(status);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{GzipMembers, decode, decode_limited, encode, encode_level};
    use crate::limits::MessageLimits;
    use crate::status::Code;

    // Independently specified RFC 1952 member with one stored DEFLATE block.
    // CRC32("hello") = 0x3610a686, ISIZE = 5.
    const HELLO_MEMBER: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01, 0x05, 0x00, 0xfa, 0xff,
        b'h', b'e', b'l', b'l', b'o', 0x86, 0xa6, 0x10, 0x36, 0x05, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn member_count_admission_is_independent_of_output_bytes() {
        let empty = encode(b"").expect("empty member");
        let limits = MessageLimits::new().with_max_decoding(0);
        assert!(
            decode_limited(&empty.repeat(1024), limits)
                .expect("default exact count")
                .is_empty()
        );
        assert_eq!(
            decode_limited(&empty.repeat(1025), limits)
                .expect_err("default count limit")
                .code(),
            Code::ResourceExhausted
        );
        assert!(
            decode_limited(
                &empty.repeat(1025),
                limits.with_unlimited_compression_frames()
            )
            .expect("explicit count opt-out")
            .is_empty()
        );
        let two = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let exact = MessageLimits::new().with_max_compression_frames(2);
        assert_eq!(
            decode_limited(&two, exact).expect("exact count"),
            b"hellohello"
        );
        assert_eq!(
            decode_limited(&two, exact.with_max_compression_frames(1))
                .expect_err("one extra member")
                .code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            decode_limited(HELLO_MEMBER, exact.with_max_compression_frames(0))
                .expect_err("no member admitted")
                .code(),
            Code::ResourceExhausted
        );
        let malformed = [HELLO_MEMBER, b"invalid"].concat();
        assert_eq!(
            decode_limited(&malformed, exact)
                .expect_err("admitted malformed later member")
                .code(),
            Code::Internal
        );
        assert_eq!(
            decode_limited(&malformed, exact.with_max_compression_frames(1))
                .expect_err("refused before next member reset")
                .code(),
            Code::ResourceExhausted
        );
    }

    #[derive(Default)]
    struct BoundaryYield<'a> {
        remaining: &'a [u8],
        consumed: usize,
        yielded: bool,
    }

    impl std::io::BufRead for BoundaryYield<'_> {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.consumed == HELLO_MEMBER.len() && !self.yielded {
                self.yielded = true;
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            Ok(self.remaining)
        }

        fn consume(&mut self, amount: usize) {
            self.remaining = self.remaining.get(amount..).expect("valid consumption");
            self.consumed += amount;
        }
    }

    impl std::io::Read for BoundaryYield<'_> {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            use std::io::BufRead;
            let input = self.fill_buf()?;
            let n = input.len().min(target.len());
            target
                .get_mut(..n)
                .expect("output")
                .copy_from_slice(input.get(..n).expect("input"));
            self.consume(n);
            Ok(n)
        }
    }

    #[test]
    fn member_end_survives_would_block_before_the_next_reset() {
        use std::io::Read;
        let joined = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let input = BoundaryYield {
            remaining: &joined,
            ..BoundaryYield::default()
        };
        let mut decoder = GzipMembers::new(input, Some(2));
        let mut output = Vec::new();
        loop {
            let mut chunk = [0; 16];
            match decoder.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(chunk.get(..n).expect("chunk")),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => panic!("unexpected decode failure: {error}"),
            }
        }
        assert!(decoder.get_ref().yielded);
        assert!(decoder.take_failure().is_none());
        assert_eq!(output, b"hellohello");
        assert_eq!(decoder.frames, 2);
    }

    #[test]
    fn every_member_contributes_to_the_same_decoded_message_limit() {
        let joined = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let exact = MessageLimits::new().with_max_decoding(10);
        assert_eq!(
            decode_limited(&joined, exact).expect("both members"),
            b"hellohello"
        );
        assert_eq!(
            decode_limited(&joined, exact.with_max_decoding(9))
                .expect_err("aggregate cap")
                .code(),
            Code::ResourceExhausted
        );
        let empty = encode(b"").expect("empty member");
        let joined = [empty.as_slice(), HELLO_MEMBER, empty.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, exact.with_max_decoding(5)).expect("empty members"),
            b"hello"
        );
    }

    #[test]
    fn later_member_crc_and_isize_are_validated() {
        for offset_from_end in [8, 4] {
            let mut later = HELLO_MEMBER.to_vec();
            let index = later.len() - offset_from_end;
            *later.get_mut(index).expect("footer byte") ^= 1;
            let joined = [HELLO_MEMBER, later.as_slice()].concat();
            assert_eq!(
                decode(&joined).expect_err("later footer").code(),
                Code::Internal
            );
        }
        // No extra output is available at this cap. The next empty member's
        // checksum must still be checked while probing for end-of-message.
        let mut empty = encode(b"").expect("empty member");
        let footer = empty.len() - 8;
        *empty.get_mut(footer).expect("empty CRC") ^= 1;
        let joined = [HELLO_MEMBER, empty.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(5))
                .expect_err("empty later CRC at exact cap")
                .code(),
            Code::Internal
        );
    }

    #[test]
    fn every_truncated_later_member_and_nonmember_tail_is_rejected() {
        for end in 1..HELLO_MEMBER.len() {
            let tail = HELLO_MEMBER.get(..end).expect("member prefix");
            let joined = [HELLO_MEMBER, tail].concat();
            assert_eq!(
                decode(&joined).expect_err("truncated later member").code(),
                Code::Internal,
                "tail length {end}"
            );
        }
        for tail in [b"junk".as_slice(), &[0], &[0, 0, 0, 0]] {
            let joined = [HELLO_MEMBER, tail].concat();
            assert_eq!(
                decode(&joined).expect_err("nonmember bytes").code(),
                Code::Internal
            );
        }
    }

    #[test]
    fn roundtrip() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(8);
        let gz = encode(&payload).expect("encode");
        assert_eq!(decode(&gz).expect("decode"), payload);
    }

    #[test]
    fn roundtrip_empty() {
        let gz = encode(b"").expect("encode");
        assert!(decode(&gz).expect("decode").is_empty());
    }

    #[test]
    fn a_bomb_is_refused_at_the_cap() {
        // 1 MiB of zeros compresses to about 1 KiB.
        let bomb = encode(&vec![0u8; 1024 * 1024]).expect("encode");
        assert!(bomb.len() < 64 * 1024);
        let err = decode_limited(&bomb, MessageLimits::unlimited().with_max_decoding(4096))
            .expect_err("bomb");
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn exactly_at_the_cap_is_accepted() {
        let payload = vec![7u8; 4096];
        let gz = encode(&payload).expect("encode");
        let limits = MessageLimits::unlimited().with_max_decoding(4096);
        assert_eq!(decode_limited(&gz, limits).expect("decode"), payload);
    }

    #[test]
    fn one_byte_over_the_cap_is_refused() {
        let gz = encode(&vec![7u8; 4097]).expect("encode");
        let limits = MessageLimits::unlimited().with_max_decoding(4096);
        let err = decode_limited(&gz, limits).expect_err("over");
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let err = decode(&[0xff; 32]).expect_err("not gzip");
        assert_eq!(err.code(), Code::Internal);
    }

    #[test]
    fn higher_level_compresses_zeros_tighter() {
        let payload = vec![0u8; 64 * 1024];
        let store = encode_level(&payload, 0).expect("store");
        let fast = encode_level(&payload, 1).expect("fast");
        let best = encode_level(&payload, 9).expect("best");
        assert!(
            best.len() <= fast.len(),
            "best={} fast={}",
            best.len(),
            fast.len()
        );
        assert!(
            store.len() > best.len(),
            "store={} best={}",
            store.len(),
            best.len()
        );
        assert_eq!(decode(&best).expect("decode best"), payload);
        assert_eq!(decode(&store).expect("decode store"), payload);
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let gz = encode(&vec![3u8; 4096]).expect("encode");
        let err = decode(&gz[..gz.len() / 2]).expect_err("truncated");
        assert_eq!(err.code(), Code::Internal);
    }
}
