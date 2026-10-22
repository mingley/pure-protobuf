//! `zstd` coding (RFC 8878) for [`CompressionAlgorithm`](super::CompressionAlgorithm).
//!
//! Backed by `ruzstd`. The dependency is optional behind the `zstd` feature so default builds keep
//! their dependency graph unchanged. `ruzstd` declares MSRV 1.87, so this
//! feature has a higher MSRV than pbrs-grpc's default 1.85 profile.

use crate::limits::MessageLimits;
use crate::status::Status;
use ruzstd::decoding::errors::FrameDecoderError;
use ruzstd::decoding::{DEFAULT_MAX_WINDOW_SIZE, FrameDecoder, StreamingDecoder};
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

/// Decode all RFC 8878 standard and skippable frames under one message cap.
/// Each standard frame independently validates its window, length and checksum.
pub(super) fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    if limits.max_compression_frames() == Some(0) {
        return Err(Status::resource_exhausted(
            "zstd frame count exceeds limit 0",
        ));
    }
    let mut decoder = ZstdFrames::new(payload, limits);
    let result = super::read_bounded(&mut decoder, limits.inflate_budget(), payload.len(), "zstd");
    if let Some(status) = decoder.failure.take() {
        return Err(status);
    }
    result
}

// Consume all RFC 8878 frames in this gRPC payload through one output limiter.
struct ZstdFrames<'a> {
    remaining: &'a [u8],
    current: FrameState<'a>,
    budget: usize,
    emitted: usize,
    seen_frame: bool,
    frames: usize,
    max_frames: Option<usize>,
    failure: Option<Status>,
}

// Move one decoder between idle and active states. Its tables/history are
// reused only within this message, then dropped with ZstdFrames.
enum FrameState<'a> {
    Idle(FrameDecoder),
    Active(ZstdFrame<'a>),
    Moving,
}

struct ZstdFrame<'a> {
    decoder: StreamingDecoder<&'a [u8], FrameDecoder>,
    declared: Option<u64>,
    emitted: usize,
}

impl<'a> ZstdFrames<'a> {
    fn new(payload: &'a [u8], limits: MessageLimits) -> Self {
        let mut decoder = FrameDecoder::new();
        decoder.set_max_window_size(limits.max_zstd_window().unwrap_or(DEFAULT_MAX_WINDOW_SIZE));
        Self {
            remaining: payload,
            current: FrameState::Idle(decoder),
            budget: limits.inflate_budget(),
            emitted: 0,
            seen_frame: false,
            frames: 0,
            max_frames: limits.max_compression_frames(),
            failure: None,
        }
    }

    fn start_frame(&mut self) -> Result<(), Status> {
        if let Some(max) = self.max_frames {
            if self.frames >= max {
                return Err(Status::resource_exhausted(format!(
                    "zstd frame count exceeds limit {max}"
                )));
            }
        }
        self.frames = self
            .frames
            .checked_add(1)
            .ok_or_else(|| Status::resource_exhausted("zstd frame count overflow"))?;
        let magic = self
            .remaining
            .get(..4)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes);
        if magic.is_some_and(|value| (0x184d2a50..=0x184d2a5f).contains(&value)) {
            // Skippable frames need no decoder initialization or scratch reset.
            let length = self
                .remaining
                .get(4..8)
                .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| Status::internal("truncated zstd skippable header"))?;
            let total = usize::try_from(length)
                .ok()
                .and_then(|length| length.checked_add(8))
                .ok_or_else(|| Status::internal("zstd skippable frame length overflow"))?;
            self.remaining = self
                .remaining
                .get(total..)
                .ok_or_else(|| Status::internal("truncated zstd skippable frame"))?;
            self.seen_frame = true;
            return Ok(());
        }
        let FrameState::Idle(decoder) = std::mem::replace(&mut self.current, FrameState::Moving)
        else {
            return Err(Status::internal("zstd frame decoder is not idle"));
        };
        // init/reset checks the next advertised window against this decoder's
        // configured limit BEFORE growing its reused history buffer.
        let decoder = StreamingDecoder::new_with_decoder(self.remaining, decoder).map_err(
            |error| match error {
                FrameDecoderError::WindowSizeTooBig { requested, max } => {
                    Status::resource_exhausted(format!(
                        "zstd window {requested} exceeds limit {max}"
                    ))
                }
                error => Status::internal(format!("zstd decode: {error}")),
            },
        )?;
        let descriptor = self
            .remaining
            .get(4)
            .copied()
            .ok_or_else(|| Status::internal("zstd missing frame descriptor"))?;
        // RFC 8878 reserves bit 3 and requires decoders to reject it.
        if descriptor & 0x08 != 0 {
            return Err(Status::internal("zstd reserved descriptor bit is set"));
        }
        let declared = (descriptor & 0xe0 != 0).then(|| decoder.decoder.content_size());
        if declared.is_some_and(|size| {
            size > u64::try_from(self.budget.saturating_sub(self.emitted)).unwrap_or(u64::MAX)
        }) {
            return Err(Status::resource_exhausted(format!(
                "decompressed message exceeds limit {}",
                self.budget
            )));
        }
        self.current = FrameState::Active(ZstdFrame {
            decoder,
            declared,
            emitted: 0,
        });
        self.seen_frame = true;
        Ok(())
    }

    fn read_checked(&mut self, target: &mut [u8]) -> Result<usize, Status> {
        use std::io::Read;
        if target.is_empty() {
            return Ok(0);
        }
        loop {
            if matches!(&self.current, FrameState::Idle(_)) {
                if self.remaining.is_empty() {
                    return if self.seen_frame {
                        Ok(0)
                    } else {
                        Err(Status::internal("empty zstd message has no frame"))
                    };
                }
                self.start_frame()?;
                if matches!(&self.current, FrameState::Idle(_)) {
                    continue; // A skippable frame produces no decoded bytes.
                }
            }
            let FrameState::Active(frame) = &mut self.current else {
                return Err(Status::internal("zstd missing active frame"));
            };
            let n = frame
                .decoder
                .read(target)
                .map_err(|error| Status::internal(format!("zstd decode: {error}")))?;
            if n != 0 {
                frame.emitted = frame
                    .emitted
                    .checked_add(n)
                    .ok_or_else(|| Status::resource_exhausted("zstd decoded size overflow"))?;
                self.emitted = self
                    .emitted
                    .checked_add(n)
                    .ok_or_else(|| Status::resource_exhausted("zstd decoded size overflow"))?;
                return Ok(n);
            }
            let FrameState::Active(frame) =
                std::mem::replace(&mut self.current, FrameState::Moving)
            else {
                return Err(Status::internal("zstd missing completed frame"));
            };
            if !frame.decoder.decoder.is_finished() {
                return Err(Status::internal("zstd frame ended before completion"));
            }
            if frame
                .declared
                .is_some_and(|size| u64::try_from(frame.emitted).ok() != Some(size))
            {
                return Err(Status::internal(
                    "zstd content size does not match decoded frame",
                ));
            }
            if let Some(expected) = frame.decoder.decoder.get_checksum_from_data() {
                if frame.decoder.decoder.get_calculated_checksum() != Some(expected) {
                    return Err(Status::internal("zstd content checksum mismatch"));
                }
            }
            let (remaining, decoder) = frame.decoder.into_parts();
            if remaining.len() >= self.remaining.len() {
                return Err(Status::internal(
                    "zstd frame decoder made no input progress",
                ));
            }
            self.remaining = remaining;
            // Resetting this same decoder clears frame-local checksum/FCS and
            // entropy state while preserving scratch allocations within the
            // current message. Nothing survives the enclosing decode call.
            self.current = FrameState::Idle(decoder);
        }
    }
}

impl std::io::Read for ZstdFrames<'_> {
    fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
        match self.read_checked(target) {
            Ok(n) => Ok(n),
            Err(status) => {
                self.failure = Some(status);
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "zstd frame validation",
                ))
            }
        }
    }
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

    fn skippable(payload: &[u8]) -> Vec<u8> {
        let mut frame = 0x184d2a50u32.to_le_bytes().to_vec();
        frame.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("small skip")
                .to_le_bytes(),
        );
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn concatenated_frames_and_skippable_metadata_preserve_all_decoded_bytes() {
        let first = raw_frame(b"hello", Some(5), 0);
        let second = raw_frame(b" world", Some(6), 0);
        let skipped = skippable(b"metadata");
        let joined = [
            skipped.as_slice(),
            first.as_slice(),
            skipped.as_slice(),
            second.as_slice(),
            skipped.as_slice(),
        ]
        .concat();
        assert_eq!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(11)).expect("frames"),
            b"hello world"
        );
        assert_eq!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(10))
                .expect_err("aggregate cap")
                .code(),
            Code::ResourceExhausted
        );
        assert!(
            decode_limited(&skipped, MessageLimits::new().with_max_decoding(0))
                .expect("skip only")
                .is_empty()
        );
    }

    #[test]
    fn every_zstd_tail_is_parsed_and_every_frame_is_validated() {
        let first = raw_frame(b"hello", Some(5), 0);
        let invalid_size = raw_frame(b"tail", Some(5), 0);
        let oversized_window = raw_frame(b"tail", None, 0x70);
        let joined = [first.as_slice(), oversized_window.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, MessageLimits::new())
                .expect_err("second window")
                .code(),
            Code::ResourceExhausted
        );
        let invalid_checksum = vec![
            0x28, 0xb5, 0x2f, 0xfd, 0x24, 0x05, 0x29, 0x00, 0x00, b'h', b'e', b'l', b'l', b'o',
            0xa3, 0x6d, 0x9f, 0x89,
        ];
        let mut reserved_descriptor = raw_frame(b"tail", Some(4), 0);
        *reserved_descriptor.get_mut(4).expect("descriptor") |= 0x08;
        for tail in [
            b"junk".to_vec(),
            vec![0],
            invalid_size,
            invalid_checksum,
            reserved_descriptor,
            skippable(b"metadata")
                .get(..9)
                .expect("partial skip")
                .to_vec(),
        ] {
            let joined = [first.as_slice(), tail.as_slice()].concat();
            assert_eq!(
                decode_limited(&joined, MessageLimits::new())
                    .expect_err("invalid tail")
                    .code(),
                Code::Internal
            );
        }
        for empty in [Vec::new(), raw_frame(b"", Some(0), 0)] {
            let joined = [first.as_slice(), empty.as_slice()].concat();
            assert_eq!(
                decode_limited(&joined, MessageLimits::new().with_max_decoding(5))
                    .expect("exact cap plus empty frame"),
                b"hello"
            );
        }
    }

    #[test]
    fn every_truncated_later_frame_is_rejected() {
        let first = raw_frame(b"prefix", Some(6), 0);
        let later = [
            0x28, 0xb5, 0x2f, 0xfd, 0x24, 0x05, 0x29, 0x00, 0x00, b'h', b'e', b'l', b'l', b'o',
            0xa3, 0x6d, 0x9f, 0x88,
        ];
        for end in 1..later.len() {
            let tail = later.get(..end).expect("frame prefix");
            let joined = [first.as_slice(), tail].concat();
            assert_eq!(
                decode(&joined).expect_err("truncated later frame").code(),
                Code::Internal,
                "tail length {end}"
            );
        }
        let joined = [first.as_slice(), later.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(11))
                .expect("both checksummed frames"),
            b"prefixhello"
        );
    }

    #[test]
    fn all_skippable_tags_and_lengths_are_checked() {
        let content = raw_frame(b"hello", Some(5), 0);
        for tag in 0..=15u32 {
            let mut skip = skippable(b"ignored metadata");
            skip.get_mut(..4)
                .expect("magic")
                .copy_from_slice(&(0x184d2a50u32 + tag).to_le_bytes());
            let joined = [skip.as_slice(), content.as_slice(), skip.as_slice()].concat();
            assert_eq!(
                decode_limited(&joined, MessageLimits::new().with_max_decoding(5))
                    .expect("skippable frame tag"),
                b"hello"
            );
        }
        let mut oversized_skip = 0x184d2a50u32.to_le_bytes().to_vec();
        oversized_skip.extend_from_slice(&u32::MAX.to_le_bytes());
        let joined = [content.as_slice(), oversized_skip.as_slice()].concat();
        assert_eq!(
            decode(&joined).expect_err("truncated huge skip").code(),
            Code::Internal
        );
        let empty = raw_frame(b"", Some(0), 0);
        let joined = [
            empty.as_slice(),
            skippable(b"metadata").as_slice(),
            empty.as_slice(),
        ]
        .concat();
        assert!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(0))
                .expect("only empty and skippable frames")
                .is_empty()
        );
        assert_eq!(decode(&[]).expect_err("no frame").code(), Code::Internal);
    }

    #[test]
    fn reserved_descriptor_bit_is_rejected_but_unused_bit_is_ignored() {
        let mut frame = raw_frame(b"hello", Some(5), 0);
        *frame.get_mut(4).expect("descriptor") |= 0x08;
        assert_eq!(
            decode(&frame).expect_err("reserved bit").code(),
            Code::Internal
        );
        *frame.get_mut(4).expect("descriptor") ^= 0x08 | 0x10;
        assert_eq!(decode(&frame).expect("unused bit is ignored"), b"hello");
    }

    #[test]
    fn many_empty_frames_share_one_message_local_decoder() {
        // Each valid empty frame advertises a 64 KiB window. Scratch tables
        // and history must be reused within the message, not allocated anew
        // for each nine-byte frame. Allocation counts are checked separately.
        let empty = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x30, 0x01, 0x00, 0x00];
        let payload = empty.repeat(2048);
        let limits = MessageLimits::new()
            .with_max_decoding(0)
            .with_max_compression_frames(2048)
            .with_max_zstd_window(64 * 1024);
        assert!(
            decode_limited(&payload, limits)
                .expect("many empty frames")
                .is_empty()
        );
    }

    #[test]
    fn frame_count_admission_includes_empty_and_skippable_frames() {
        let empty = raw_frame(b"", Some(0), 0);
        let limits = MessageLimits::new().with_max_decoding(0);
        assert!(
            decode_limited(&empty.repeat(1024), limits)
                .expect("default exact count")
                .is_empty()
        );
        assert_eq!(
            decode_limited(&empty.repeat(1025), limits)
                .expect_err("default frame limit")
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
        let skip = skippable(b"metadata");
        let joined = [empty.as_slice(), skip.as_slice(), empty.as_slice()].concat();
        assert!(
            decode_limited(&joined, limits.with_max_compression_frames(3))
                .expect("standard and skippable exact count")
                .is_empty()
        );
        assert_eq!(
            decode_limited(&joined, limits.with_max_compression_frames(2))
                .expect_err("skip counted against budget")
                .code(),
            Code::ResourceExhausted
        );
        for frame in [empty.as_slice(), skip.as_slice()] {
            assert_eq!(
                decode_limited(frame, limits.with_max_compression_frames(0))
                    .expect_err("no frame admitted")
                    .code(),
                Code::ResourceExhausted
            );
        }
        let malformed = [empty.as_slice(), b"invalid"].concat();
        assert_eq!(
            decode_limited(&malformed, limits.with_max_compression_frames(2))
                .expect_err("admitted invalid frame")
                .code(),
            Code::Internal
        );
        assert_eq!(
            decode_limited(&malformed, limits.with_max_compression_frames(1))
                .expect_err("refused before decoder reset")
                .code(),
            Code::ResourceExhausted
        );
    }

    #[test]
    fn decoder_reuse_resets_history_and_checksums_between_frames() {
        let large = vec![b'z'; 64 * 1024];
        let small_frame = raw_frame(b"small", None, 0x00); // 1 KiB window.
        let large_frame = raw_frame(&large, None, 0x30); // 64 KiB window.
        let first_payload = b"first independently checksummed frame".repeat(40);
        let second_payload = b"a different independently checksummed frame".repeat(40);
        let first = encode(&first_payload).expect("first checksummed frame");
        let second = encode(&second_payload).expect("second checksummed frame");
        // The in-tree encoder uses 128 KiB windows, so allow that independently
        // of the output cap while switching large/small histories and hashes.
        let joined = [
            small_frame.as_slice(),
            large_frame.as_slice(),
            first.as_slice(),
            small_frame.as_slice(),
            large_frame.as_slice(),
            second.as_slice(),
        ]
        .concat();
        let expected = [
            b"small".as_slice(),
            large.as_slice(),
            first_payload.as_slice(),
            b"small".as_slice(),
            large.as_slice(),
            second_payload.as_slice(),
        ]
        .concat();
        let limits = MessageLimits::new()
            .with_max_decoding(expected.len())
            .with_max_zstd_window(128 * 1024);
        assert_eq!(
            decode_limited(&joined, limits).expect("reset frame state"),
            expected
        );
        let disallowed = raw_frame(b"x", None, 0x30);
        let joined = [small_frame.as_slice(), disallowed.as_slice()].concat();
        let status = decode_limited(&joined, MessageLimits::new().with_max_zstd_window(1024))
            .expect_err("next window checked before reset growth");
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert!(status.message().contains("window"));
    }

    #[test]
    fn independent_c_frames_reset_entropy_contexts() {
        // libzstd 1.5.7, level 3; provenance and hashes accompany the fixtures.
        let frames: [&[u8]; 3] = [
            include_bytes!("../../tests/fixtures/zstd-context/frame-0.zst"),
            include_bytes!("../../tests/fixtures/zstd-context/frame-1.zst"),
            include_bytes!("../../tests/fixtures/zstd-context/frame-2.zst"),
        ];
        let mut expected_frames = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            let mut expected: Vec<u8> = (0..8192 + i * 37)
                .map(|n| {
                    u8::try_from((n * n / 17 + n * (19 + i * 11) + (n / 64) % 31) % 251)
                        .expect("fixture byte fits u8")
                })
                .collect();
            expected.extend_from_within(..4096);
            expected.extend_from_slice(&b"context reset ".repeat(97));
            let limits = MessageLimits::new().with_max_decoding(expected.len());
            assert_eq!(
                decode_limited(frame, limits).expect("independent libzstd frame"),
                expected
            );
            expected_frames.push(expected);
        }
        // Reorder the independent entropy contexts as well as checking producer order.
        for order in [[0, 1, 2], [2, 0, 1]] {
            let encoded: Vec<u8> = order
                .iter()
                .flat_map(|&i| frames[i].iter().copied())
                .collect();
            let expected: Vec<u8> = order
                .iter()
                .flat_map(|&i| expected_frames[i].iter().copied())
                .collect();
            let limits = MessageLimits::new().with_max_decoding(expected.len());
            assert_eq!(
                decode_limited(&encoded, limits).expect("independent concatenated frames"),
                expected
            );
            assert_eq!(
                decode_limited(
                    &encoded,
                    MessageLimits::new().with_max_decoding(expected.len() - 1)
                )
                .expect_err("aggregate decoded cap applies to independent frames")
                .code(),
                Code::ResourceExhausted
            );
        }
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
