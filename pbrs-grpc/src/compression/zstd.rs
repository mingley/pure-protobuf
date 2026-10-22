//! `zstd` coding (RFC 8878) for [`CompressionAlgorithm`](super::CompressionAlgorithm).
//!
//! Backed by `ruzstd`. The
//! dependency is optional behind the `zstd` feature so default builds keep
//! their dependency graph unchanged. `ruzstd` declares MSRV 1.87, so this
//! feature has a higher MSRV than pbrs-grpc's default 1.85 profile.

use crate::limits::MessageLimits;
use crate::status::Status;
use ruzstd::decoding::errors::FrameDecoderError;
use ruzstd::decoding::{DEFAULT_MAX_WINDOW_SIZE, StreamingDecoder};
use ruzstd::encoding::{CompressionLevel, compress_to_vec};
use std::io::Write;

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
    let window = limits.max_zstd_window().unwrap_or(DEFAULT_MAX_WINDOW_SIZE);
    let mut decoder = StreamingDecoder::new_with_max_window_size(payload, window).map_err(
        |error| match error {
            FrameDecoderError::WindowSizeTooBig { requested, max } => {
                Status::resource_exhausted(format!("zstd window {requested} exceeds limit {max}"))
            }
            error => Status::internal(format!("zstd decode: {error}")),
        },
    )?;
    // The backend has validated the header. Its content_size accessor returns
    // zero for both unknown and known-empty sizes; the descriptor distinguishes
    // those cases (size flag bits 7..6, single-segment bit 5).
    let declared = payload
        .get(4)
        .is_some_and(|descriptor| descriptor & 0xe0 != 0)
        .then(|| decoder.decoder.content_size());
    if let Some(size) = declared {
        if size > u64::try_from(limits.inflate_budget()).unwrap_or(u64::MAX) {
            return Err(Status::resource_exhausted(format!(
                "decompressed message exceeds limit {}",
                limits.inflate_budget()
            )));
        }
    }
    let out = super::read_bounded(&mut decoder, limits.inflate_budget(), payload.len(), "zstd")?;
    if let Some(size) = declared {
        if u64::try_from(out.len()).ok() != Some(size) {
            return Err(Status::internal(
                "zstd content size does not match decoded message",
            ));
        }
    }
    // StreamingDecoder exposes the checksum but does not compare it itself.
    if let Some(expected) = decoder.decoder.get_checksum_from_data() {
        if decoder.decoder.get_calculated_checksum() != Some(expected) {
            return Err(Status::internal("zstd content checksum mismatch"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, decode_limited, encode, encode_level};
    use crate::limits::MessageLimits;
    use crate::status::Code;

    fn raw_frame(payload: &[u8], size: Option<u8>, window: u8) -> Vec<u8> {
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd];
        if let Some(size) = size {
            frame.extend_from_slice(&[0x20, size]);
        } else {
            frame.extend_from_slice(&[0x00, window]);
        }
        let header = (u32::try_from(payload.len()).expect("small fixture") << 3) | 1;
        frame.extend_from_slice(header.to_le_bytes().get(..3).expect("block header"));
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn an_unknown_size_frame_with_a_normal_window_is_accepted() {
        // RFC 8878 raw block, no declared content size, 8 MiB window.
        let frame = raw_frame(b"hello", None, 0x68);
        assert_eq!(
            decode_limited(&frame, MessageLimits::new()).expect("decode"),
            b"hello"
        );
    }

    #[test]
    fn oversized_windows_are_rejected_before_reading_blocks() {
        let mut frame = raw_frame(b"x", None, 0x30); // 64 KiB window, one byte output.
        let limits = MessageLimits::new().with_max_zstd_window(1024);
        assert_eq!(
            decode_limited(&frame, limits).expect_err("window").code(),
            Code::ResourceExhausted
        );
        // Header-only input proves admission happens before parsing a body.
        frame.truncate(6);
        assert_eq!(
            decode_limited(&frame, limits)
                .expect_err("header window")
                .code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            decode_limited(&raw_frame(b"x", None, 0x70), MessageLimits::new())
                .expect_err("16 MiB window exceeds default 8 MiB")
                .code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            decode_limited(
                &raw_frame(b"x", None, 0x70),
                MessageLimits::new().with_max_zstd_window(16 * 1024 * 1024)
            )
            .expect("explicitly raised window"),
            b"x"
        );
    }

    #[test]
    fn output_limit_does_not_replace_the_window_limit() {
        use std::io::Read;
        // Two RLE blocks produce 65,537 bytes. The first requested output byte
        // makes the legacy decoder inflate both blocks to retain its 64 KiB
        // history. This small negative control avoids a large test allocation.
        let frame = [
            0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x30, 0x02, 0x00, 0x08,
            b'x', // 65,536-byte non-final RLE block.
            0x0b, 0x00, 0x00, b'x', // One-byte final RLE block.
        ];
        let mut legacy =
            ruzstd::decoding::StreamingDecoder::new(frame.as_slice()).expect("legacy header");
        assert_eq!(legacy.read(&mut [0u8; 1]).expect("first byte"), 1);
        assert_eq!(
            legacy.decoder.blocks_decoded(),
            2,
            "history inflated before first byte"
        );
        let limits = MessageLimits::new()
            .with_max_decoding(1)
            .with_max_zstd_window(1024);
        let status = decode_limited(&frame, limits).expect_err("window admission");
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert!(status.message().contains("window"));
    }

    #[test]
    fn declared_size_is_checked_before_blocks_and_after_decode() {
        let mut frame = raw_frame(b"hello", Some(64), 0);
        frame.truncate(6);
        assert_eq!(
            decode_limited(&frame, MessageLimits::new().with_max_decoding(4))
                .expect_err("oversized declared content before absent blocks")
                .code(),
            Code::ResourceExhausted
        );
        for size in [0, 4, 6] {
            let frame = raw_frame(b"hello", Some(size), 0);
            assert_eq!(
                decode_limited(&frame, MessageLimits::new())
                    .expect_err("size mismatch")
                    .code(),
                Code::Internal
            );
        }
        let frame = raw_frame(b"hello", Some(5), 0);
        assert_eq!(
            decode_limited(&frame, MessageLimits::new()).expect("matching size"),
            b"hello"
        );
    }

    #[test]
    fn content_checksums_are_verified() {
        // RFC 8878 single-segment raw "hello"; low 32 bits of XXH64("hello", 0)
        // are 0x889f6da3. This fixture does not use the in-tree encoder.
        let mut frame = vec![
            0x28, 0xb5, 0x2f, 0xfd, 0x24, 0x05, 0x29, 0x00, 0x00, b'h', b'e', b'l', b'l', b'o',
            0xa3, 0x6d, 0x9f, 0x88,
        ];
        assert_eq!(
            decode_limited(&frame, MessageLimits::new()).expect("valid checksum"),
            b"hello"
        );
        *frame.last_mut().expect("checksum byte") ^= 1;
        let status = decode_limited(&frame, MessageLimits::new()).expect_err("forged checksum");
        assert_eq!(status.code(), Code::Internal);
        assert!(status.message().contains("checksum"));
    }

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
