//! `zstd` coding (RFC 8878) for [`Codec`](super::Codec).
//!
//! Backed by `zstd-rs`, a pure-Rust `no_std` encoder/decoder with MSRV 1.85.
//! The dependency is optional behind the `zstd` feature so default builds keep
//! their dependency graph unchanged.

use crate::limits::MessageLimits;
use crate::status::Status;
use std::io::Write;
use zstd_rs::{CompressionConfig, Compressor, Decompressor, Error, MAX_LEVEL};

/// Compress `payload` at the kernel default zstd level.
pub(super) fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    encode_level(payload, crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL)
}

/// Compress `payload` at a zstd level.
///
/// The public compression-level knob is a `u32` shared with gzip/deflate; zstd
/// level 0 is not valid, so 0 maps to level 1 and values above the backend's
/// maximum are clamped.
pub(super) fn encode_level(payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
    let mut out = Vec::with_capacity(payload.len() / 2 + 32);
    encode_vec(payload, level, &mut out)?;
    Ok(out)
}

/// Compress `payload` at `level`, appending to `out`.
pub(super) fn encode_into(payload: &[u8], level: u32, out: &mut impl Write) -> Result<(), Status> {
    let mut encoded = Vec::with_capacity(payload.len() / 2 + 32);
    encode_vec(payload, level, &mut encoded)?;
    out.write_all(&encoded)
        .map_err(|e| Status::internal(format!("zstd encode: {e}")))?;
    Ok(())
}

fn encode_vec(payload: &[u8], level: u32, out: &mut Vec<u8>) -> Result<(), Status> {
    let level = i32::try_from(level)
        .unwrap_or(MAX_LEVEL)
        .clamp(1, MAX_LEVEL);
    let cfg = CompressionConfig {
        level,
        ..CompressionConfig::FAST
    };
    Compressor::new(cfg)
        .and_then(|mut compressor| compressor.compress(payload, None, out))
        .map_err(|e| Status::internal(format!("zstd encode: {e}")))
}

/// Inflate `payload` with no cap.
///
/// Prefer [`decode_limited`]. This is only safe against a trusted peer.
pub(super) fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    decode_limited(payload, MessageLimits::unlimited())
}

/// Inflate `payload`, refusing to allocate past the inbound cap in `limits`.
pub(super) fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    let budget = limits.inflate_budget();
    let mut out = Vec::new();
    match Decompressor::new().decompress(payload, None, budget, &mut out) {
        Ok(_) => Ok(out),
        Err(Error::OutputLimit) => Err(Status::resource_exhausted(format!(
            "decompressed message exceeds limit {budget}"
        ))),
        Err(e) => Err(Status::internal(format!("zstd decode: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_limited, encode, encode_level};
    use crate::limits::MessageLimits;
    use crate::status::Code;

    #[test]
    fn roundtrip() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(8);
        let zst = encode(&payload).expect("encode");
        assert_eq!(decode(&zst).expect("decode"), payload);
    }

    #[test]
    fn roundtrip_empty() {
        let zst = encode(b"").expect("encode");
        assert!(decode(&zst).expect("decode").is_empty());
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
    fn exactly_at_the_cap_is_accepted() {
        let payload = vec![7u8; 4096];
        let zst = encode(&payload).expect("encode");
        let limits = MessageLimits::unlimited().with_max_decoding(4096);
        assert_eq!(decode_limited(&zst, limits).expect("decode"), payload);
    }

    #[test]
    fn one_byte_over_the_cap_is_refused() {
        let zst = encode(&vec![7u8; 4097]).expect("encode");
        let limits = MessageLimits::unlimited().with_max_decoding(4096);
        let err = decode_limited(&zst, limits).expect_err("over");
        assert_eq!(err.code(), Code::ResourceExhausted);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let err = decode(&[0xff; 32]).expect_err("not zstd");
        assert_eq!(err.code(), Code::Internal);
    }

    #[test]
    fn higher_level_compresses_zeros_tighter() {
        let payload = vec![0u8; 64 * 1024];
        let fast = encode_level(&payload, 1).expect("fast");
        let best = encode_level(&payload, 9).expect("best");
        assert!(
            best.len() <= fast.len(),
            "best={} fast={}",
            best.len(),
            fast.len()
        );
        assert_eq!(decode(&best).expect("decode best"), payload);
    }
}
