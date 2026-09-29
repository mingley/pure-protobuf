//! Wire message compression: the `grpc-encoding` registry (RX-06).
//!
//! gRPC compresses each message independently and names the coding once per
//! RPC in `grpc-encoding`. This kernel always speaks the two codings every
//! peer implements: `gzip` (RFC 1952) and `deflate` (zlib, RFC 1950).
//! Enabling the `zstd` feature adds pure-Rust `zstd` (RFC 8878).
//! `identity` is the absence of a coding, not a [`Codec`].
//!
//! Backend: `miniz_oxide` through `flate2` (`rust_backend` +
//! `runtime_detection`), measured faster than `zlib-rs` at the kernel
//! default level on the dev host. See
//! `docs/decisions/compression.md` for the shootout numbers and the
//! deferred zstd decision.
//!
//! [`Codec::Gzip`] is the default in both directions: outbound uses it
//! unless a channel/server config picks [`Codec::Deflate`], and inbound
//! flag-set frames without a usable token inflate as gzip, matching the
//! pre-deflate behavior.

mod deflate;
mod gzip;
#[cfg(feature = "zstd")]
mod zstd;

pub use gzip::{decode, decode_limited, encode, encode_level};

use crate::limits::MessageLimits;
use crate::status::Status;

/// A `grpc-encoding` wire coding this kernel can inflate and emit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Codec {
    /// `gzip`: RFC 1952 member with header, CRC, and length trailer.
    #[default]
    Gzip,
    /// `deflate`: zlib wrapper (RFC 1950) around a DEFLATE stream.
    Deflate,
    /// `zstd`: Zstandard frame (RFC 8878). Requires the `zstd` feature.
    #[cfg(feature = "zstd")]
    Zstd,
}

impl Codec {
    /// Parse a `grpc-encoding` token. Case-insensitive; surrounding
    /// whitespace and a trailing `;parameter` are ignored, mirroring
    /// the wire-layer encoding-token parser. `identity` is not a `Codec`.
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

    /// The wire token: `gzip` or `deflate`.
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
    /// clamped to 9). The 0-9 scale is the same DEFLATE effort for both
    /// codings; only the framing differs.
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
    use super::Codec;

    fn codecs() -> Vec<Codec> {
        #[cfg(feature = "zstd")]
        {
            vec![Codec::Gzip, Codec::Deflate, Codec::Zstd]
        }
        #[cfg(not(feature = "zstd"))]
        {
            vec![Codec::Gzip, Codec::Deflate]
        }
    }

    #[test]
    fn parse_tokens() {
        assert_eq!(Codec::parse("gzip"), Some(Codec::Gzip));
        assert_eq!(Codec::parse("GZIP"), Some(Codec::Gzip));
        assert_eq!(Codec::parse("  deflate;q=0.5 "), Some(Codec::Deflate));
        #[cfg(feature = "zstd")]
        assert_eq!(Codec::parse(" ZSTD;q=0.5 "), Some(Codec::Zstd));
        #[cfg(not(feature = "zstd"))]
        assert_eq!(Codec::parse("zstd"), None);
        assert_eq!(Codec::parse("identity"), None);
        assert_eq!(Codec::parse(""), None);
        assert_eq!(Codec::parse("snappy"), None);
        assert_eq!(Codec::Gzip.name(), "gzip");
        assert_eq!(Codec::Deflate.name(), "deflate");
        #[cfg(feature = "zstd")]
        assert_eq!(Codec::Zstd.name(), "zstd");
        assert_eq!(Codec::default(), Codec::Gzip);
    }

    #[test]
    fn both_codecs_round_trip() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(16);
        for codec in codecs() {
            let enc = codec.encode(&payload).expect("encode");
            let dec = codec
                .decode_limited(&enc, crate::limits::MessageLimits::unlimited())
                .expect("decode");
            assert_eq!(dec, payload, "codec {:?}", codec);
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
        let gz = Codec::Gzip.encode(&payload).expect("gzip");
        let df = Codec::Deflate.encode(&payload).expect("deflate");
        let limits = crate::limits::MessageLimits::unlimited();
        assert!(Codec::Deflate.decode_limited(&gz, limits).is_err());
        assert!(Codec::Gzip.decode_limited(&df, limits).is_err());
        #[cfg(feature = "zstd")]
        {
            let zst = Codec::Zstd.encode(&payload).expect("zstd");
            assert!(Codec::Gzip.decode_limited(&zst, limits).is_err());
            assert!(Codec::Deflate.decode_limited(&zst, limits).is_err());
            assert!(Codec::Zstd.decode_limited(&gz, limits).is_err());
            assert!(Codec::Zstd.decode_limited(&df, limits).is_err());
        }
    }
}
