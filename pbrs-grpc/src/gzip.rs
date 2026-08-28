//! gzip for the gRPC Compressed-Flag and `grpc-encoding: gzip`.
//!
//! Backed by `miniz_oxide` through `flate2`'s `rust_backend`, so there is no
//! C in the compression path.
//!
//! Inflation is always bounded. A gzip stream can expand by roughly 1000x, so
//! an unbounded `read_to_end` turns a 4 KiB frame into a multi-gigabyte
//! allocation; [`decode_limited`] stops one byte past the cap instead.

use crate::limits::MessageLimits;
use crate::status::Status;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Initial output reservation as a multiple of the compressed size.
///
/// Text-like protobuf typically inflates 3-5x, so this avoids the realloc
/// chain for normal traffic without letting a peer dictate a large upfront
/// allocation.
const INFLATE_GUESS_RATIO: usize = 4;

/// Never reserve more than this up front, however large the input.
const INFLATE_GUESS_CAP: usize = 256 * 1024;

/// gzip `payload`.
///
/// Uses [`Compression::fast`]: at gRPC message sizes the extra CPU of higher
/// levels costs more latency than the saved bytes buy back.
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    let mut enc = GzEncoder::new(
        Vec::with_capacity(payload.len() / 2 + 32),
        Compression::fast(),
    );
    enc.write_all(payload)
        .map_err(|e| Status::internal(format!("gzip encode: {e}")))?;
    enc.finish()
        .map_err(|e| Status::internal(format!("gzip encode: {e}")))
}

/// Inflate `payload` with no cap.
///
/// Prefer [`decode_limited`]. This is only safe against a trusted peer.
pub fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    decode_limited(payload, MessageLimits::unlimited())
}

/// Inflate `payload`, refusing to allocate past the inbound cap in `limits`.
///
/// Peak memory is the cap plus one byte, whatever the compression ratio.
/// Exceeding it is [`Code::ResourceExhausted`](crate::Code::ResourceExhausted),
/// reported against the cap rather than the true inflated size, because
/// learning the true size is exactly the work being refused.
pub fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    let budget = limits.inflate_budget();
    // One byte past the cap distinguishes "fits exactly" from "overflows".
    let read_cap = u64::try_from(budget.saturating_add(1)).unwrap_or(u64::MAX);
    let guess = payload
        .len()
        .saturating_mul(INFLATE_GUESS_RATIO)
        .min(INFLATE_GUESS_CAP)
        .min(budget);
    let mut out = Vec::with_capacity(guess);
    GzDecoder::new(payload)
        .take(read_cap)
        .read_to_end(&mut out)
        .map_err(|e| Status::internal(format!("gzip decode: {e}")))?;
    if out.len() > budget {
        return Err(Status::resource_exhausted(format!(
            "decompressed message exceeds limit {budget}"
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_limited, encode};
    use crate::limits::MessageLimits;
    use crate::status::Code;

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
    fn truncated_stream_is_an_error() {
        let gz = encode(&vec![3u8; 4096]).expect("encode");
        let err = decode(&gz[..gz.len() / 2]).expect_err("truncated");
        assert_eq!(err.code(), Code::Internal);
    }
}
