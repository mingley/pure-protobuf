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

pub use gzip::{decode, decode_limited, encode, encode_level};

use crate::limits::MessageLimits;
use crate::status::Status;

/// Read at most the decoded message budget, probing one more byte for overflow.
///
/// Allocate after the decoder produces bytes. The buffered compressed length is
/// an initial hint capped at 256 KiB; gzip length trailers are never trusted.
/// Exact reservations stay within the budget when normal Vec growth would
/// double past it. Small messages use one output allocation.
fn read_bounded(
    mut decoder: impl std::io::Read,
    budget: usize,
    compressed_length: usize,
    coding: &str,
) -> Result<Vec<u8>, Status> {
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
    let exceeded =
        || Status::resource_exhausted(format!("decompressed message exceeds limit {budget}"));
    let mut first = [0u8; 8192];
    let want = budget.saturating_add(1).min(first.len());
    let target = first
        .get_mut(..want)
        .ok_or_else(|| Status::internal("compression decode: short buffer"))?;
    let n = read_retry(&mut decoder, target, coding)?;
    if n == 0 {
        return Ok(Vec::new());
    }
    if n > budget {
        return Err(exceeded());
    }
    let mut out = Vec::new();
    let capacity = n.max(compressed_length.min(256 * 1024)).min(budget);
    out.try_reserve_exact(capacity)
        .map_err(|_| Status::resource_exhausted("decompressed message allocation failed"))?;
    // Initialize each new tail once, then read directly into it. Keeping the
    // used length separately avoids repeated clearing and per-chunk copies.
    out.resize(capacity, 0);
    out.get_mut(..n)
        .ok_or_else(|| Status::internal("compression decode: short output"))?
        .copy_from_slice(
            first
                .get(..n)
                .ok_or_else(|| Status::internal("compression decode: short buffer"))?,
        );
    let mut used = n;
    loop {
        if used == out.len() {
            // Probe before growing: exact fits need no spare heap capacity,
            // and EOF must still validate the coding's checksum/trailer.
            let probe = first
                .get_mut(..1)
                .ok_or_else(|| Status::internal("compression decode: short buffer"))?;
            let n = read_retry(&mut decoder, probe, coding)?;
            if n == 0 {
                out.truncate(used);
                return Ok(out);
            }
            if used == budget {
                return Err(exceeded());
            }
            let capacity = out.len().saturating_mul(2).max(used + 1).min(budget);
            out.try_reserve_exact(capacity - out.len()).map_err(|_| {
                Status::resource_exhausted("decompressed message allocation failed")
            })?;
            out.resize(capacity, 0);
            *out.get_mut(used)
                .ok_or_else(|| Status::internal("compression decode: short output"))? =
                probe.first().copied().unwrap_or_default();
            used += 1;
            // A cap one byte past the previous capacity is now full. Loop
            // back to probe it; reading an empty tail would falsely mean EOF.
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
        match self {
            Self::Gzip => gzip::decode_limited(payload, limits),
            Self::Deflate => deflate::decode_limited(payload, limits),
            #[cfg(feature = "zstd")]
            Self::Zstd => zstd::decode_limited(payload, limits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CompressionAlgorithm, read_bounded};
    use crate::{Code, MessageLimits};
    use std::io::Read;

    // The pre-RX-11 input-buffering path is an independent wrapper oracle.
    // Preserve the production output reservation and cap/error precedence.
    fn buffered_decode(
        codec: CompressionAlgorithm,
        payload: &[u8],
        limits: MessageLimits,
    ) -> Result<Vec<u8>, Code> {
        let budget = limits.inflate_budget();
        let read_cap = u64::try_from(budget.saturating_add(1)).unwrap_or(u64::MAX);
        let mut out =
            Vec::with_capacity(payload.len().saturating_mul(4).min(256 * 1024).min(budget));
        let result = match codec {
            CompressionAlgorithm::Gzip => flate2::read::GzDecoder::new(payload)
                .take(read_cap)
                .read_to_end(&mut out),
            CompressionAlgorithm::Deflate => flate2::read::ZlibDecoder::new(payload)
                .take(read_cap)
                .read_to_end(&mut out),
            #[cfg(feature = "zstd")]
            CompressionAlgorithm::Zstd => return Err(Code::Unimplemented),
        };
        result.map_err(|_| Code::Internal)?;
        if out.len() > budget {
            return Err(Code::ResourceExhausted);
        }
        Ok(out)
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
        let limits = MessageLimits::unlimited().with_max_decoding(budget);
        for codec in [CompressionAlgorithm::Gzip, CompressionAlgorithm::Deflate] {
            for payload in [b"".as_slice(), b"x".as_slice()] {
                let mut encoded = codec.encode(payload).expect("compress");
                // The compatibility decoder stops after the first member.
                // Padding must not turn a tiny result into a full-limit allocation.
                encoded.resize(budget, 0);
                let decoded = codec.decode_limited(&encoded, limits).expect("decode");
                assert_eq!(decoded, payload);
                if payload.is_empty() {
                    assert_eq!(decoded.capacity(), 0, "empty output needs no buffer");
                } else {
                    assert!(decoded.capacity() <= 256 * 1024, "bounded input-size hint");
                }
            }
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
            // Both wrappers decode the first member and ignore an encoded tail.
            let mut members = compressed.clone();
            members.extend_from_slice(&codec.encode(b"second member").expect("encode"));
            inputs.push(members);
            let mut garbage_tail = compressed;
            garbage_tail.extend_from_slice(b"unrelated tail");
            inputs.push(garbage_tail);
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
