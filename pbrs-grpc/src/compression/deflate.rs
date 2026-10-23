//! `deflate` coding (zlib, RFC 1950) for [`Codec`](super::Codec).
//!
//! gRPC's `deflate` is the zlib wrapper around a DEFLATE stream (2-byte
//! header plus adler32 trailer), matching HTTP's `deflate` content-coding
//! and C-core's `GRPC_COMPRESS_DEFLATE` (`inflateInit2(15)`): a raw
//! RFC 1951 stream is *not* valid here. Same backend and 0-9 effort scale
//! as [`super::gzip`]; only the framing differs. C-core peers advertise
//! and accept this coding, so it must decode here even though gzip stays
//! the outbound default.
//!
//! Inflation is bounded exactly like gzip: [`decode_limited`] stops one
//! byte past the cap.

use crate::limits::MessageLimits;
use crate::status::Status;
use flate2::Compression;
use flate2::bufread::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::Write;

/// Compress `payload` at the kernel default (deflate level 1,
/// [`Compression::fast`]).
pub(super) fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    encode_level(payload, crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL)
}

/// Compress `payload` at deflate `level` (0 stores, 1 is fast, 9 is best).
///
/// Values above 9 are clamped to 9.
pub(super) fn encode_level(payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
    let mut out = Vec::with_capacity(payload.len() / 2 + 32);
    encode_into(payload, level, &mut out)?;
    Ok(out)
}

/// Compress `payload` at `level`, appending to `out`. See
/// [`super::gzip::encode_into`] for why the framing layer prefers this.
pub(super) fn encode_into(payload: &[u8], level: u32, out: &mut impl Write) -> Result<(), Status> {
    let mut enc = ZlibEncoder::new(&mut *out, Compression::new(level.min(9)));
    enc.write_all(payload)
        .map_err(|e| Status::internal(format!("deflate encode: {e}")))?;
    enc.finish()
        .map_err(|e| Status::internal(format!("deflate encode: {e}")))?;
    Ok(())
}

/// Inflate `payload` with no cap.
///
/// Prefer [`decode_limited`]. This is only safe against a trusted peer.
pub(super) fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    decode_limited(payload, MessageLimits::unlimited())
}

/// Inflate one RFC 1950 zlib stream under the inbound cap.
/// Bytes after its checksum are rejected rather than silently ignored.
///
/// The output buffer capacity stays within the cap. A fixed 8 KiB stack
/// chunk probes one extra decoded byte to detect overflow.
/// Exceeding it is [`Code::ResourceExhausted`](crate::Code::ResourceExhausted).
pub(super) fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    decode_limited_into(payload, limits, Vec::new())
}

pub(super) fn decode_limited_into(
    payload: &[u8],
    limits: MessageLimits,
    out: Vec<u8>,
) -> Result<Vec<u8>, Status> {
    if limits.max_compression_frames() == Some(0) {
        return Err(Status::resource_exhausted(
            "deflate frame count exceeds limit 0",
        ));
    }
    let mut decoder = ZlibDecoder::new(payload);
    let out = super::read_bounded_into(
        &mut decoder,
        limits.inflate_budget(),
        payload.len(),
        "deflate",
        out,
    )?;
    if !decoder.get_ref().is_empty() {
        return Err(Status::internal("trailing bytes after zlib stream"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_limited, encode, encode_level};
    use crate::limits::MessageLimits;
    use crate::status::Code;

    #[test]
    fn a_zlib_stream_requires_one_compression_frame() {
        let encoded = encode(b"hello").expect("zlib frame");
        assert_eq!(
            decode_limited(
                &encoded,
                MessageLimits::new().with_max_compression_frames(0)
            )
            .expect_err("no frame admitted")
            .code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            decode_limited(
                &encoded,
                MessageLimits::new().with_max_compression_frames(1)
            )
            .expect("one admitted frame"),
            b"hello"
        );
    }

    #[test]
    fn a_zlib_stream_must_consume_the_entire_message_payload() {
        let encoded = encode(b"hello").expect("encode");
        for tail in [
            vec![0],
            b"junk".to_vec(),
            encode(b"other").expect("second stream"),
        ] {
            let joined = [encoded.as_slice(), tail.as_slice()].concat();
            assert_eq!(
                decode(&joined).expect_err("trailing zlib bytes").code(),
                Code::Internal
            );
        }
        assert_eq!(
            decode_limited(&encoded, MessageLimits::new().with_max_decoding(5)).expect("exact cap"),
            b"hello"
        );
    }

    #[test]
    fn roundtrip() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(8);
        let df = encode(&payload).expect("encode");
        assert_eq!(decode(&df).expect("decode"), payload);
    }

    #[test]
    fn roundtrip_empty() {
        let df = encode(b"").expect("encode");
        assert!(decode(&df).expect("decode").is_empty());
    }

    #[test]
    fn zlib_framing_has_a_header_not_gzip_magic() {
        // Gzip members start with 0x1f 0x8b; zlib starts with a 2-byte
        // header (0x78 ...). A raw RFC 1951 stream is not valid here.
        let df = encode(b"hello").expect("encode");
        assert_ne!(&df[..2], &[0x1f, 0x8b]);
        assert_eq!(df[0], 0x78);
    }

    #[test]
    fn decodes_zlib_vectors() {
        // zlib (`wbits=+15`, the C-core `GRPC_COMPRESS_DEFLATE` framing) of
        // b"the quick brown fox ..." x8, produced by CPython's zlib at
        // levels 1 and 6: an independent encoder, fixed here so backend
        // swaps stay honest.
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(8);
        let level1: &[u8] = &[
            0x78, 0x01, 0x2b, 0xc9, 0x48, 0x55, 0x28, 0x2c, 0xcd, 0x4c, 0xce, 0x56, 0x48, 0x2a,
            0xca, 0x2f, 0xcf, 0x53, 0x48, 0xcb, 0xaf, 0x50, 0xc8, 0x2a, 0xcd, 0x2d, 0x28, 0x56,
            0xc8, 0x2f, 0x4b, 0x2d, 0x52, 0x28, 0x01, 0x4a, 0xe7, 0x24, 0x56, 0x55, 0x2a, 0xa4,
            0xe4, 0xa7, 0x83, 0xd8, 0xa3, 0x4a, 0x89, 0x0c, 0x01, 0x00, 0x2d, 0x86, 0x7f, 0xc9,
        ];
        let level6: &[u8] = &[
            0x78, 0x9c, 0x2b, 0xc9, 0x48, 0x55, 0x28, 0x2c, 0xcd, 0x4c, 0xce, 0x56, 0x48, 0x2a,
            0xca, 0x2f, 0xcf, 0x53, 0x48, 0xcb, 0xaf, 0x50, 0xc8, 0x2a, 0xcd, 0x2d, 0x28, 0x56,
            0xc8, 0x2f, 0x4b, 0x2d, 0x52, 0x28, 0x01, 0x4a, 0xe7, 0x24, 0x56, 0x55, 0x2a, 0xa4,
            0xe4, 0xa7, 0x97, 0x8c, 0x2a, 0x25, 0x5e, 0x29, 0x00, 0x2d, 0x86, 0x7f, 0xc9,
        ];
        assert_eq!(decode(level1).expect("level1"), payload);
        assert_eq!(decode(level6).expect("level6"), payload);
    }

    #[test]
    fn raw_deflate_stream_is_rejected() {
        // A raw RFC 1951 stream (no zlib header) is not the `deflate`
        // coding: C-core's `inflateInit2(15)` rejects it too.
        let raw: &[u8] = &[
            0x2b, 0xc9, 0x48, 0x55, 0x28, 0x2c, 0xcd, 0x4c, 0xce, 0x56, 0x48, 0x2a, 0xca, 0x2f,
            0xcf, 0x53, 0x48, 0xcb, 0xaf, 0x50, 0xc8, 0x2a, 0xcd, 0x2d, 0x28, 0x56, 0xc8, 0x2f,
            0x4b, 0x2d, 0x52, 0x28, 0x01, 0x4a, 0xe7, 0x24, 0x56, 0x55, 0x2a, 0xa4, 0xe4, 0xa7,
            0x83, 0xd8, 0xa3, 0x4a, 0x89, 0x0c, 0x01, 0x00,
        ];
        let err = decode(raw).expect_err("raw stream");
        assert_eq!(err.code(), Code::Internal);
    }

    #[test]
    fn a_bomb_is_refused_at_the_cap() {
        let bomb = encode(&vec![0u8; 1024 * 1024]).expect("encode");
        assert!(bomb.len() < 64 * 1024);
        let err = decode_limited(&bomb, MessageLimits::unlimited().with_max_decoding(4096))
            .expect_err("bomb");
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let err = decode(&[0xff; 32]).expect_err("not deflate");
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
    }
}
