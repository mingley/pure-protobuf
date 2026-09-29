//! `zstd` coding (RFC 8878) for [`Codec`](super::Codec).
//!
//! Backed by `ruzstd`, a pure-Rust encoder/decoder with broad adoption. The
//! dependency is optional behind the `zstd` feature so default builds keep
//! their dependency graph unchanged. `ruzstd` declares MSRV 1.87, so this
//! feature has a higher MSRV than pbrs-grpc's default 1.85 profile.

use crate::limits::MessageLimits;
use crate::status::Status;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};
use std::io::{Read, Write};

/// Compress `payload` at the kernel default zstd level.
pub(super) fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    encode_level(payload, crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL)
}

/// Compress `payload` at a zstd level.
///
/// `ruzstd` 0.9 implements `CompressionLevel::Fastest` (roughly zstd level 1);
/// its Default/Better/Best levels are still unimplemented. The shared public
/// compression-level knob therefore maps every requested zstd level to Fastest.
pub(super) fn encode_level(payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
    let _ = level;
    Ok(compress_to_vec(payload, CompressionLevel::Fastest))
}

/// Compress `payload` at `level`, appending to `out`.
pub(super) fn encode_into(payload: &[u8], level: u32, out: &mut impl Write) -> Result<(), Status> {
    let encoded = encode_level(payload, level)?;
    out.write_all(&encoded)
        .map_err(|e| Status::internal(format!("zstd encode: {e}")))?;
    Ok(())
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
    let read_cap = budget.saturating_add(1);
    let mut out = Vec::new();
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(payload)
        .map_err(|e| Status::internal(format!("zstd decode: {e}")))?;
    let mut buf = [0u8; 8192];
    loop {
        let remaining = read_cap.saturating_sub(out.len());
        if remaining == 0 {
            return Err(Status::resource_exhausted(format!(
                "decompressed message exceeds limit {budget}"
            )));
        }
        let want = remaining.min(buf.len());
        let chunk = buf
            .get_mut(..want)
            .ok_or_else(|| Status::internal("zstd decode: short buffer"))?;
        let n = decoder
            .read(chunk)
            .map_err(|e| Status::internal(format!("zstd decode: {e}")))?;
        if n == 0 {
            return Ok(out);
        }
        let decoded = buf
            .get(..n)
            .ok_or_else(|| Status::internal("zstd decode: short buffer"))?;
        out.extend_from_slice(decoded);
        if out.len() > budget {
            return Err(Status::resource_exhausted(format!(
                "decompressed message exceeds limit {budget}"
            )));
        }
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
    fn all_levels_map_to_fastest() {
        let payload = vec![0u8; 64 * 1024];
        let fast = encode_level(&payload, 1).expect("fast");
        let best = encode_level(&payload, 9).expect("best");
        assert_eq!(best, fast, "ruzstd 0.9 only implements Fastest");
        assert_eq!(decode(&best).expect("decode best"), payload);
    }
}
