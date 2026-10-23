//! Wire message compression: the `grpc-encoding` registry (RX-06).
//!
//! gRPC compresses each message independently and names the coding once per
//! RPC in `grpc-encoding`. Supported codings are `gzip` (RFC 1952) and
//! `deflate` (zlib, RFC 1950), subject to the RPC's compression settings.
//! Enabling the `zstd` feature adds `zstd` (RFC 8878).
//! `identity` is the absence of a coding, not a [`CompressionAlgorithm`].
//!
//! Backend: `miniz_oxide` through `flate2` (`rust_backend` +
//! `runtime_detection`), measured faster than `zlib-rs` at the kernel
//! default level on the dev host. See
//! `docs/decisions/compression.md` for the shootout numbers and the
//! optional zstd decision.
//!
//! [`CompressionAlgorithm::Gzip`] is the default outbound coding. Configs can
//! select another coding. Compressed inbound frames require a supported
//! `grpc-encoding` header; omitted or identity encodings require flag zero.

mod deflate;
mod gzip;
#[cfg(feature = "zstd")]
mod zstd;

#[cfg(feature = "tonic")]
pub(crate) use gzip::GzipMembers;
pub use gzip::{decode, decode_limited, encode, encode_level};

use crate::limits::MessageLimits;
use crate::status::Status;

/// Decoded bytes examined before the bounded reader enters its growth loop.
/// Cold stream messages within this lookahead keep the ordinary codec path.
pub(crate) const DECODE_LOOKAHEAD_BYTES: usize = 8192;

/// Read at most the decoded message budget, probing one more byte for overflow.
///
/// Decode a bounded chunk before allocating. A short final chunk reserves only
/// the bytes it produced; full chunks retain geometric growth for large output.
/// The buffered compressed length is an initial hint capped at 256 KiB. Neither
/// gzip trailers nor other untrusted decoded-size fields drive reservations.
#[cfg(test)]
fn read_bounded(
    decoder: impl std::io::Read,
    budget: usize,
    compressed_length: usize,
    coding: &str,
) -> Result<Vec<u8>, Status> {
    read_bounded_into(decoder, budget, compressed_length, coding, Vec::new())
}

/// Reuse an exclusively owned output allocation. Existing spare capacity does
/// not increase the decoded-byte allowance or drive a new reservation.
fn read_bounded_into(
    mut decoder: impl std::io::Read,
    budget: usize,
    compressed_length: usize,
    coding: &str,
    mut out: Vec<u8>,
) -> Result<Vec<u8>, Status> {
    out.clear();
    fn read_retry(
        decoder: &mut impl std::io::Read,
        target: &mut [u8],
        coding: &str,
    ) -> Result<usize, Status> {
        loop {
            match decoder.read(target) {
                Ok(n) if n <= target.len() => return Ok(n),
                Ok(_) => return Err(Status::internal("compression decode: invalid read length")),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(Status::internal(format!("{coding} decode: {error}"))),
            }
        }
    }
    // A short Read is not EOF. Fill the bounded lookahead so adversarial short
    // reads cannot cause one reallocation per byte, or bypass trailer checks.
    fn read_chunk(
        decoder: &mut impl std::io::Read,
        target: &mut [u8],
        coding: &str,
    ) -> Result<(usize, bool), Status> {
        let mut used = 0;
        while used < target.len() {
            let tail = target
                .get_mut(used..)
                .ok_or_else(|| Status::internal("compression decode: short buffer"))?;
            let n = read_retry(decoder, tail, coding)?;
            if n == 0 {
                return Ok((used, true));
            }
            used += n;
        }
        Ok((used, false))
    }
    fn reserve(out: &mut Vec<u8>, capacity: usize, budget: usize) -> Result<(), Status> {
        if capacity <= out.capacity() {
            return Ok(());
        }
        out.try_reserve_exact(capacity - out.len())
            .map_err(|_| Status::resource_exhausted("decompressed message allocation failed"))?;
        if out.capacity() > budget {
            return Err(Status::resource_exhausted(
                "decompressed message allocation exceeds limit",
            ));
        }
        Ok(())
    }
    let exceeded =
        || Status::resource_exhausted(format!("decompressed message exceeds limit {budget}"));
    let mut chunk = [0u8; DECODE_LOOKAHEAD_BYTES];
    let want = budget.saturating_add(1).min(chunk.len());
    let target = chunk
        .get_mut(..want)
        .ok_or_else(|| Status::internal("compression decode: short buffer"))?;
    let (n, eof) = read_chunk(&mut decoder, target, coding)?;
    if n > budget {
        return Err(exceeded());
    }
    if n == 0 {
        return Ok(out);
    }
    let capacity = if eof {
        n
    } else {
        n.max(compressed_length.min(256 * 1024)).min(budget)
    };
    reserve(&mut out, capacity, budget)?;
    out.extend_from_slice(
        chunk
            .get(..n)
            .ok_or_else(|| Status::internal("compression decode: short buffer"))?,
    );
    if eof {
        return Ok(out);
    }
    // Keep used length separately and initialize each unread tail once. Reads
    // between growths go directly into output, without per-chunk heap copies.
    out.resize(capacity, 0);
    let mut used = n;
    loop {
        if used == out.len() {
            // Observe the final tail before growing. For output just beyond a
            // capacity boundary (for example a 64 KiB protobuf plus headers),
            // EOF gives an exact reservation instead of doubling the buffer.
            // At the budget this is exactly the required one-byte probe.
            let want = (budget - used).saturating_add(1).min(chunk.len());
            let target = chunk
                .get_mut(..want)
                .ok_or_else(|| Status::internal("compression decode: short buffer"))?;
            let (n, eof) = read_chunk(&mut decoder, target, coding)?;
            if n > budget - used {
                return Err(exceeded());
            }
            if n == 0 {
                return Ok(out);
            }
            let decoded = used + n;
            let capacity = if eof {
                decoded
            } else {
                used.saturating_mul(2).max(decoded).min(budget)
            };
            reserve(&mut out, capacity, budget)?;
            out.extend_from_slice(
                chunk
                    .get(..n)
                    .ok_or_else(|| Status::internal("compression decode: short buffer"))?,
            );
            if eof {
                return Ok(out);
            }
            out.resize(capacity, 0);
            used = decoded;
            // A clipped budget may now be full. Loop back for the overflow /
            // EOF probe; an empty read target would falsely prove completion.
            continue;
        }
        let target = out
            .get_mut(used..)
            .ok_or_else(|| Status::internal("compression decode: short output"))?;
        let n = read_retry(&mut decoder, target, coding)?;
        if n == 0 {
            out.truncate(used);
            return Ok(out);
        }
        used += n;
    }
}

/// The published gzip/deflate compression selector.
///
/// This enum stays exhaustive so existing callers can match both variants.
/// Use [`CompressionAlgorithm`] for the extensible wire-coding registry,
/// including optional Zstd support.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Codec {
    /// `gzip`: RFC 1952 member with header, CRC, and length trailer.
    #[default]
    Gzip,
    /// `deflate`: zlib wrapper (RFC 1950) around a DEFLATE stream.
    Deflate,
}

impl Codec {
    /// Parse a gzip or deflate token, ignoring case, whitespace and parameters.
    /// `identity` and optional codings are not published `Codec` variants.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        CompressionAlgorithm::parse(token).and_then(CompressionAlgorithm::legacy_codec)
    }

    /// The wire token: `gzip` or `deflate`.
    #[must_use]
    pub fn name(self) -> &'static str {
        CompressionAlgorithm::from(self).name()
    }

    /// Encode `payload` at the kernel default level for this coding.
    pub fn encode(self, payload: &[u8]) -> Result<Vec<u8>, Status> {
        CompressionAlgorithm::from(self).encode(payload)
    }

    /// Encode `payload` at DEFLATE effort `level` (clamped to 9).
    pub fn encode_level(self, payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
        CompressionAlgorithm::from(self).encode_level(payload, level)
    }

    /// Inflate `payload` with no cap. Only use with a trusted peer;
    /// prefer [`Self::decode_limited`].
    pub fn decode(self, payload: &[u8]) -> Result<Vec<u8>, Status> {
        CompressionAlgorithm::from(self).decode(payload)
    }

    /// Inflate `payload`, refusing to allocate past the inbound cap.
    pub fn decode_limited(self, payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
        CompressionAlgorithm::from(self).decode_limited(payload, limits)
    }
}

/// A `grpc-encoding` wire coding this kernel can inflate and emit.
///
/// Use this extensible registry for optional codings such as Zstd. The
/// published [`Codec`] remains exhaustive over gzip and deflate.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompressionAlgorithm {
    /// `gzip`: RFC 1952 member with header, CRC, and length trailer.
    #[default]
    Gzip,
    /// `deflate`: zlib wrapper (RFC 1950) around a DEFLATE stream.
    Deflate,
    /// `zstd`: Zstandard frame (RFC 8878). Requires the `zstd` feature.
    #[cfg(feature = "zstd")]
    Zstd,
}

impl From<Codec> for CompressionAlgorithm {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::Gzip => Self::Gzip,
            Codec::Deflate => Self::Deflate,
        }
    }
}

impl CompressionAlgorithm {
    /// The corresponding published codec, when this is gzip or deflate.
    #[must_use]
    pub fn legacy_codec(self) -> Option<Codec> {
        match self {
            Self::Gzip => Some(Codec::Gzip),
            Self::Deflate => Some(Codec::Deflate),
            #[cfg(feature = "zstd")]
            Self::Zstd => None,
        }
    }

    /// Parse a `grpc-encoding` token. Case-insensitive; surrounding
    /// whitespace and a trailing `;parameter` are ignored, mirroring
    /// the wire-layer encoding-token parser. `identity` is not a coding.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        let coding = match token.split_once(';') {
            Some((coding, _)) => coding.trim(),
            None => token.trim(),
        };
        if coding.eq_ignore_ascii_case("gzip") {
            Some(Self::Gzip)
        } else if coding.eq_ignore_ascii_case("deflate") {
            Some(Self::Deflate)
        } else if cfg!(feature = "zstd") && coding.eq_ignore_ascii_case("zstd") {
            #[cfg(feature = "zstd")]
            {
                Some(Self::Zstd)
            }
            #[cfg(not(feature = "zstd"))]
            {
                None
            }
        } else {
            None
        }
    }

    /// The wire token: `gzip`, `deflate`, or optional `zstd`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
            #[cfg(feature = "zstd")]
            Self::Zstd => "zstd",
        }
    }

    /// Encode `payload` at the kernel default level for this coding.
    pub fn encode(self, payload: &[u8]) -> Result<Vec<u8>, Status> {
        match self {
            Self::Gzip => gzip::encode(payload),
            Self::Deflate => deflate::encode(payload),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::encode(payload),
        }
    }

    /// Encode `payload` at `level` (0 stores, 1 is fast, 9 is best;
    /// clamped to 9). The 0-9 scale is the same DEFLATE effort for gzip and
    /// deflate; only the framing differs. Zstd currently uses the backend's
    /// fastest level for every requested level.
    pub fn encode_level(self, payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
        match self {
            Self::Gzip => gzip::encode_level(payload, level),
            Self::Deflate => deflate::encode_level(payload, level),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::encode_level(payload, level),
        }
    }

    /// Encode `payload` at `level`, appending to `out`.
    ///
    /// Byte-identical to [`Self::encode_level`]; the framing layer uses
    /// this to compress straight into the frame buffer.
    pub(crate) fn encode_into(
        self,
        payload: &[u8],
        level: u32,
        out: &mut impl std::io::Write,
    ) -> Result<(), Status> {
        match self {
            Self::Gzip => gzip::encode_into(payload, level, out),
            Self::Deflate => deflate::encode_into(payload, level, out),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::encode_into(payload, level, out),
        }
    }

    /// Inflate `payload` with no cap.
    ///
    /// Prefer [`Self::decode_limited`]. This is only safe against a
    /// trusted peer.
    pub fn decode(self, payload: &[u8]) -> Result<Vec<u8>, Status> {
        match self {
            Self::Gzip => gzip::decode(payload),
            Self::Deflate => deflate::decode(payload),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::decode(payload),
        }
    }

    /// Inflate `payload`, refusing to allocate past the inbound cap.
    pub fn decode_limited(self, payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
        self.decode_limited_into(payload, limits, Vec::new())
    }

    /// Decode into a previously reclaimed, exclusively owned allocation.
    pub(crate) fn decode_limited_into(
        self,
        payload: &[u8],
        limits: MessageLimits,
        out: Vec<u8>,
    ) -> Result<Vec<u8>, Status> {
        match self {
            Self::Gzip => gzip::decode_limited_into(payload, limits, out),
            Self::Deflate => deflate::decode_limited_into(payload, limits, out),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::decode_limited_into(payload, limits, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CompressionAlgorithm, read_bounded, read_bounded_into};
    use crate::{Code, MessageLimits};
    use std::io::Read;

    // Input-buffering wrappers provide an independent I/O wrapper oracle.
    // Full member/stream consumption follows the new native format policy.
    fn buffered_decode(
        codec: CompressionAlgorithm,
        payload: &[u8],
        limits: MessageLimits,
    ) -> Result<Vec<u8>, Code> {
        let budget = limits.inflate_budget();
        let read_cap = u64::try_from(budget.saturating_add(1)).unwrap_or(u64::MAX);
        let mut out =
            Vec::with_capacity(payload.len().saturating_mul(4).min(256 * 1024).min(budget));
        let (result, consumed) = match codec {
            CompressionAlgorithm::Gzip => (
                flate2::read::MultiGzDecoder::new(payload)
                    .take(read_cap)
                    .read_to_end(&mut out),
                None,
            ),
            CompressionAlgorithm::Deflate => {
                let mut decoder = flate2::read::ZlibDecoder::new(payload);
                let result = (&mut decoder).take(read_cap).read_to_end(&mut out);
                (result, Some(decoder.total_in()))
            }
            #[cfg(feature = "zstd")]
            CompressionAlgorithm::Zstd => return Err(Code::Unimplemented),
        };
        result.map_err(|_| Code::Internal)?;
        if out.len() > budget {
            return Err(Code::ResourceExhausted);
        }
        if consumed.is_some_and(|n| u64::try_from(payload.len()).ok() != Some(n)) {
            return Err(Code::Internal);
        }
        Ok(out)
    }

    #[test]
    fn reused_output_preserves_decode_limits_and_complete_input_validation() {
        for codec in codecs() {
            for length in [0, 1, 8192, 8193, 65539] {
                let payload = vec![7; length];
                let encoded = codec.encode(&payload).expect("encode");
                let mut truncated = encoded.clone();
                truncated.pop();
                let mut trailing = encoded.clone();
                trailing.extend_from_slice(b"invalid trailing data");
                let mut corrupt = encoded.clone();
                if let Some(last) = corrupt.last_mut() {
                    *last ^= 0xff;
                }
                for bytes in [&encoded, &truncated, &trailing, &corrupt] {
                    for budget in [0, length.saturating_sub(1), length, length + 1] {
                        let limits = MessageLimits::unlimited().with_max_decoding(budget);
                        let fresh = codec.decode_limited(bytes, limits).map_err(|e| e.code());
                        let reused = codec
                            .decode_limited_into(bytes, limits, Vec::with_capacity(131072))
                            .map_err(|e| e.code());
                        assert_eq!(reused, fresh, "{codec:?}, length {length}, budget {budget}");
                    }
                }
                assert_eq!(
                    codec
                        .decode_limited_into(
                            &encoded,
                            MessageLimits::unlimited().with_max_compression_frames(0),
                            Vec::with_capacity(131072),
                        )
                        .expect_err("frame count applies to reused output")
                        .code(),
                    Code::ResourceExhausted
                );
            }
        }
    }

    #[test]
    fn reused_large_output_handles_smaller_allowances_without_reallocation() {
        for codec in codecs() {
            let mut output = Vec::with_capacity(131072);
            let pointer = output.as_ptr();
            let capacity = output.capacity();
            for length in [65539, 1, 8193, 0, 8192, 65539] {
                let payload = vec![7; length];
                let encoded = codec.encode(&payload).expect("encode");
                output = codec
                    .decode_limited_into(
                        &encoded,
                        MessageLimits::unlimited().with_max_decoding(length),
                        output,
                    )
                    .expect("decode with retained allocation larger than allowance");
                assert_eq!(output, payload);
                assert_eq!(output.as_ptr(), pointer, "{codec:?}, length {length}");
                assert_eq!(output.capacity(), capacity);
            }
        }
    }

    #[test]
    fn retained_capacity_does_not_expand_read_spans_for_a_modest_message() {
        struct ReadSpans<'a> {
            remaining: &'a [u8],
            largest_target: usize,
        }
        impl Read for ReadSpans<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                self.largest_target = self.largest_target.max(target.len());
                self.remaining.read(target)
            }
        }

        let payload = vec![7; 8193];
        let retained = Vec::with_capacity(1024 * 1024);
        let pointer = retained.as_ptr();
        let capacity = retained.capacity();
        let mut reader = ReadSpans {
            remaining: &payload,
            largest_target: 0,
        };
        let decoded = read_bounded_into(
            &mut reader,
            2 * 1024 * 1024,
            16,
            "read-span control",
            retained,
        )
        .expect("small output with a large retained allocation");
        assert_eq!(decoded, payload);
        assert_eq!(
            decoded.as_ptr(),
            pointer,
            "the retained allocation stays owned"
        );
        assert_eq!(decoded.capacity(), capacity);
        // A large allocation retained by an earlier message must not become
        // the next message's initialized output/read span. Keep the bound
        // proportional to this output rather than to a particular chunk size.
        assert!(
            reader.largest_target <= 2 * payload.len(),
            "a {}-byte message requested a {}-byte read target from retained capacity {}",
            payload.len(),
            reader.largest_target,
            capacity,
        );
    }

    #[test]
    fn bounded_decode_handles_short_reads_interrupts_and_clipped_growth() {
        struct ShortReader<'a> {
            remaining: &'a [u8],
            interrupt: bool,
            calls: usize,
        }
        impl Read for ShortReader<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                assert!(!target.is_empty(), "an empty read cannot prove decoder EOF");
                self.calls += 1;
                self.interrupt = !self.interrupt;
                if self.interrupt {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                if let Some(&byte) = self.remaining.first() {
                    *target.first_mut().expect("nonempty read target") = byte;
                    self.remaining = self.remaining.get(1..).unwrap_or_default();
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
        }
        let payload = b"short reads cross several buffer growth boundaries";
        for budget in [0, 1, 2, 5, payload.len() - 1, payload.len()] {
            let mut reader = ShortReader {
                remaining: payload,
                interrupt: false,
                calls: 0,
            };
            let result = read_bounded(&mut reader, budget, 0, "test");
            if budget == payload.len() {
                assert_eq!(result.expect("exact fit"), payload);
            } else {
                assert_eq!(
                    result.expect_err("over limit").code(),
                    Code::ResourceExhausted
                );
            }
            assert!(reader.calls <= 2 * (payload.len() + 1));
        }
    }

    #[test]
    fn short_final_chunks_do_not_double_output_capacity() {
        let budget = 4 * 1024 * 1024;
        for codec in codecs() {
            for length in [8191, 8192, 8193, 16385, 32769, 65536, 65539, 73727] {
                let payload = vec![7; length];
                let encoded = codec.encode(&payload).expect("encode");
                let decoded = codec
                    .decode_limited(
                        &encoded,
                        MessageLimits::unlimited().with_max_decoding(budget),
                    )
                    .expect("decode");
                assert_eq!(decoded, payload);
                assert_eq!(
                    decoded.capacity(),
                    length,
                    "{codec:?}: a final partial chunk must reserve only decoded bytes"
                );
            }
        }
    }

    #[test]
    fn bounded_lookahead_handles_short_reads_at_large_growth_boundaries() {
        struct ShortReader<'a>(&'a [u8]);
        impl Read for ShortReader<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                assert!(!target.is_empty());
                let n = self.0.len().min(target.len()).min(31);
                target[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let payload = vec![7; 65539];
        let decoded =
            read_bounded(ShortReader(&payload), 4 * 1024 * 1024, 0, "test").expect("short reads");
        assert_eq!(decoded, payload);
        assert_eq!(decoded.capacity(), payload.len());
        for budget in [8192, 8193, 16385, 32769, 65538] {
            assert_eq!(
                read_bounded(ShortReader(&payload), budget, 0, "test")
                    .expect_err("over limit")
                    .code(),
                Code::ResourceExhausted
            );
        }
    }

    #[test]
    fn final_lookahead_preserves_decoder_errors_and_limit_precedence() {
        struct ErrorAfter<'a>(&'a [u8]);
        impl Read for ErrorAfter<'_> {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                assert!(!target.is_empty());
                if self.0.is_empty() {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
                self.0.read(target)
            }
        }
        let payload = vec![7; 65539];
        for budget in [payload.len(), payload.len() + 1, 4 * 1024 * 1024] {
            assert_eq!(
                read_bounded(ErrorAfter(&payload), budget, 0, "test")
                    .expect_err("decoder error after final bytes")
                    .code(),
                Code::Internal
            );
        }
        assert_eq!(
            read_bounded(ErrorAfter(&payload), payload.len() - 1, 0, "test")
                .expect_err("overflow before decoder trailer")
                .code(),
            Code::ResourceExhausted
        );
    }

    #[test]
    fn bounded_decode_rejects_invalid_reader_lengths() {
        struct InvalidReader;
        impl Read for InvalidReader {
            fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
                Ok(target.len() + 1)
            }
        }
        assert_eq!(
            read_bounded(InvalidReader, 100, 0, "test")
                .expect_err("reader exceeded target length")
                .code(),
            Code::Internal
        );
    }

    #[test]
    fn growth_clipped_to_budget_still_checks_the_compressed_trailer() {
        for codec in codecs() {
            let mut encoded = codec.encode(&vec![7; 8193]).expect("compress");
            encoded.truncate(encoded.len() - 1);
            let limits = MessageLimits::unlimited().with_max_decoding(8193);
            assert_eq!(
                codec
                    .decode_limited(&encoded, limits)
                    .expect_err("truncated trailer")
                    .code(),
                Code::Internal
            );
        }
    }

    #[test]
    fn compressed_length_hint_is_bounded_and_waits_for_decoded_bytes() {
        let budget = 4 * 1024 * 1024;
        for payload in [b"".as_slice(), b"x".as_slice()] {
            let decoded = read_bounded(payload, budget, budget, "hint").expect("decode");
            assert_eq!(decoded, payload);
            assert_eq!(
                decoded.capacity(),
                payload.len(),
                "observed EOF must override a large compressed-size hint"
            );
        }
    }

    #[test]
    fn gzip_consumes_all_members_and_rejects_nonmember_padding() {
        let first = CompressionAlgorithm::Gzip.encode(b"hello").expect("first");
        let second = CompressionAlgorithm::Gzip
            .encode(b" world")
            .expect("second");
        let joined = [first.as_slice(), second.as_slice()].concat();
        assert_eq!(
            CompressionAlgorithm::Gzip
                .decode_limited(&joined, MessageLimits::new().with_max_decoding(11))
                .expect("members"),
            b"hello world"
        );
        assert_eq!(
            CompressionAlgorithm::Gzip
                .decode_limited(&joined, MessageLimits::new().with_max_decoding(10))
                .expect_err("aggregate cap")
                .code(),
            Code::ResourceExhausted
        );
        for tail in [
            b"garbage".as_slice(),
            &[0],
            second.get(..second.len() - 1).expect("truncated"),
        ] {
            let joined = [first.as_slice(), tail].concat();
            assert_eq!(
                CompressionAlgorithm::Gzip
                    .decode(&joined)
                    .expect_err("invalid tail")
                    .code(),
                Code::Internal
            );
        }
    }

    #[test]
    fn deflate_consumes_exactly_one_zlib_stream() {
        let first = CompressionAlgorithm::Deflate
            .encode(b"hello")
            .expect("first");
        for tail in [
            vec![0],
            b"garbage".to_vec(),
            CompressionAlgorithm::Deflate
                .encode(b" world")
                .expect("second"),
        ] {
            let joined = [first.as_slice(), tail.as_slice()].concat();
            assert_eq!(
                CompressionAlgorithm::Deflate
                    .decode(&joined)
                    .expect_err("trailing bytes")
                    .code(),
                Code::Internal
            );
        }
    }

    #[test]
    fn decoded_output_capacity_never_grows_past_the_limit() {
        // Non-power-of-two limits catch geometric Vec growth past the cap.
        // Highly compressed input also exercises growth far beyond a size guess.
        for codec in codecs() {
            for length in [0, 1, 1001, 8191, 8192, 8193, 8194, 32769, 32770, 65539] {
                let payload = vec![7; length];
                let encoded = codec.encode(&payload).expect("encode");
                for budget in [length, length + 1] {
                    let limits = MessageLimits::unlimited().with_max_decoding(budget);
                    let decoded = codec.decode_limited(&encoded, limits).expect("decode");
                    assert_eq!(decoded, payload);
                    assert!(
                        decoded.capacity() <= budget,
                        "{codec:?}: output capacity {} exceeds budget {budget}",
                        decoded.capacity()
                    );
                }
                if length != 0 {
                    let limits = MessageLimits::unlimited().with_max_decoding(length - 1);
                    assert_eq!(
                        codec
                            .decode_limited(&encoded, limits)
                            .expect_err("over limit")
                            .code(),
                        Code::ResourceExhausted
                    );
                }
            }
        }
    }

    #[test]
    fn slice_decoding_matches_buffered_input_on_malformed_data_and_limits() {
        for codec in [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate] {
            let payload = b"protobuf compression wrapper compatibility".repeat(100);
            let compressed = codec.encode(&payload).expect("encode");
            let mut inputs = vec![compressed.clone(), Vec::new(), vec![0xff; 32]];
            // Every truncation, and a bit mutation in every header/body/trailer byte.
            for end in 0..compressed.len() {
                inputs.push(compressed[..end].to_vec());
                let mut changed = compressed.clone();
                changed[end] ^= 0x80;
                inputs.push(changed);
            }
            for input in inputs {
                for budget in [0, 1, payload.len() - 1, payload.len(), payload.len() + 1] {
                    let limits = MessageLimits::unlimited().with_max_decoding(budget);
                    let actual = codec.decode_limited(&input, limits).map_err(|e| e.code());
                    assert_eq!(
                        actual,
                        buffered_decode(codec, &input, limits),
                        "codec {codec:?}, input {input:?}, budget {budget}"
                    );
                }
            }
        }
    }

    fn codecs() -> Vec<CompressionAlgorithm> {
        #[cfg(feature = "zstd")]
        {
            vec![
                CompressionAlgorithm::Gzip,
                CompressionAlgorithm::Deflate,
                CompressionAlgorithm::Zstd,
            ]
        }
        #[cfg(not(feature = "zstd"))]
        {
            vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate]
        }
    }

    #[test]
    fn parse_tokens() {
        assert_eq!(
            CompressionAlgorithm::parse("gzip"),
            Some(CompressionAlgorithm::Gzip)
        );
        assert_eq!(
            CompressionAlgorithm::parse("GZIP"),
            Some(CompressionAlgorithm::Gzip)
        );
        assert_eq!(
            CompressionAlgorithm::parse("  deflate;q=0.5 "),
            Some(CompressionAlgorithm::Deflate)
        );
        #[cfg(feature = "zstd")]
        assert_eq!(
            CompressionAlgorithm::parse(" ZSTD;q=0.5 "),
            Some(CompressionAlgorithm::Zstd)
        );
        #[cfg(not(feature = "zstd"))]
        assert_eq!(CompressionAlgorithm::parse("zstd"), None);
        assert_eq!(CompressionAlgorithm::parse("identity"), None);
        assert_eq!(CompressionAlgorithm::parse(""), None);
        assert_eq!(CompressionAlgorithm::parse("snappy"), None);
        assert_eq!(CompressionAlgorithm::Gzip.name(), "gzip");
        assert_eq!(CompressionAlgorithm::Deflate.name(), "deflate");
        #[cfg(feature = "zstd")]
        assert_eq!(CompressionAlgorithm::Zstd.name(), "zstd");
        assert_eq!(CompressionAlgorithm::default(), CompressionAlgorithm::Gzip);
    }

    #[test]
    fn both_codecs_round_trip() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(16);
        for codec in codecs() {
            let enc = codec.encode(&payload).expect("encode");
            let dec = codec
                .decode_limited(&enc, crate::limits::MessageLimits::unlimited())
                .expect("decode");
            assert_eq!(dec, payload, "codec {codec:?}");
        }
    }

    #[test]
    fn encode_into_matches_encode() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(16);
        for codec in codecs() {
            let mut into = Vec::new();
            codec
                .encode_into(&payload, 1, &mut into)
                .expect("encode_into");
            assert_eq!(into, codec.encode(&payload).expect("encode"));
        }
    }

    #[test]
    fn codings_are_not_cross_decodable() {
        // A gzip member is not a zlib stream and vice versa: the
        // decoder must follow the RPC's coding, not guess.
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(16);
        let gz = CompressionAlgorithm::Gzip.encode(&payload).expect("gzip");
        let df = CompressionAlgorithm::Deflate
            .encode(&payload)
            .expect("deflate");
        let limits = crate::limits::MessageLimits::unlimited();
        assert!(
            CompressionAlgorithm::Deflate
                .decode_limited(&gz, limits)
                .is_err()
        );
        assert!(
            CompressionAlgorithm::Gzip
                .decode_limited(&df, limits)
                .is_err()
        );
        #[cfg(feature = "zstd")]
        {
            let zst = CompressionAlgorithm::Zstd.encode(&payload).expect("zstd");
            assert!(
                CompressionAlgorithm::Gzip
                    .decode_limited(&zst, limits)
                    .is_err()
            );
            assert!(
                CompressionAlgorithm::Deflate
                    .decode_limited(&zst, limits)
                    .is_err()
            );
            assert!(
                CompressionAlgorithm::Zstd
                    .decode_limited(&gz, limits)
                    .is_err()
            );
            assert!(
                CompressionAlgorithm::Zstd
                    .decode_limited(&df, limits)
                    .is_err()
            );
        }
    }
}
