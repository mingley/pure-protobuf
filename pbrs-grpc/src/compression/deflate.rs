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
use flate2::{
    Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status as FlateStatus,
};
use std::cell::RefCell;
use std::io::{self, Read, Write};

const COMPRESS_SCRATCH_CAP: usize = 16 * 1024;
const MAX_POOLED_DECODERS: usize = 8;

struct CachedZlibCompressor {
    level: u32,
    comp: Compress,
    buf: Vec<u8>,
}

impl CachedZlibCompressor {
    fn new(level: u32) -> Self {
        Self {
            level,
            comp: Compress::new(Compression::new(level), true),
            buf: Vec::with_capacity(COMPRESS_SCRATCH_CAP),
        }
    }
}

thread_local! {
    static ZLIB_COMPRESSORS: RefCell<[Option<CachedZlibCompressor>; 2]> =
        const { RefCell::new([None, None]) };
    static ZLIB_DECOMPRESSORS: RefCell<Vec<Decompress>> = const { RefCell::new(Vec::new()) };
}

struct ZlibCompressorLease {
    inner: Option<CachedZlibCompressor>,
}

impl ZlibCompressorLease {
    fn acquire(level: u32) -> Self {
        let cached = ZLIB_COMPRESSORS
            .try_with(|cell| {
                let Ok(mut slots) = cell.try_borrow_mut() else {
                    return None;
                };
                for slot in slots.iter_mut() {
                    if slot.as_ref().is_some_and(|entry| entry.level == level) {
                        return slot.take();
                    }
                }
                None
            })
            .ok()
            .flatten()
            .unwrap_or_else(|| CachedZlibCompressor::new(level));
        Self {
            inner: Some(cached),
        }
    }
}

impl Drop for ZlibCompressorLease {
    fn drop(&mut self) {
        let Some(mut cached) = self.inner.take() else {
            return;
        };
        cached.comp.reset();
        cached.buf.clear();
        if cached.buf.capacity() > COMPRESS_SCRATCH_CAP * 4 {
            cached.buf.shrink_to(COMPRESS_SCRATCH_CAP);
        }
        ZLIB_COMPRESSORS
            .try_with(|cell| {
                let Ok(mut slots) = cell.try_borrow_mut() else {
                    return;
                };
                let [slot0, slot1] = &mut *slots;
                if slot0.is_none() {
                    *slot0 = Some(cached);
                } else if slot1.is_none() {
                    *slot1 = Some(cached);
                } else {
                    *slot1 = slot0.take();
                    *slot0 = Some(cached);
                }
            })
            .ok();
    }
}

struct PooledZlibReader<'a> {
    rem: &'a [u8],
    decomp: Option<Decompress>,
    done: bool,
}

impl<'a> PooledZlibReader<'a> {
    fn new(payload: &'a [u8]) -> Self {
        let decomp = ZLIB_DECOMPRESSORS
            .try_with(|cell| cell.try_borrow_mut().ok().and_then(|mut pool| pool.pop()))
            .ok()
            .flatten()
            .map(|mut d| {
                d.reset(true);
                d
            })
            .unwrap_or_else(|| Decompress::new(true));
        Self {
            rem: payload,
            decomp: Some(decomp),
            done: false,
        }
    }
}

impl Drop for PooledZlibReader<'_> {
    fn drop(&mut self) {
        let Some(mut decomp) = self.decomp.take() else {
            return;
        };
        ZLIB_DECOMPRESSORS
            .try_with(|cell| {
                if let Ok(mut pool) = cell.try_borrow_mut() {
                    if pool.len() < MAX_POOLED_DECODERS {
                        decomp.reset(true);
                        pool.push(decomp);
                    }
                }
            })
            .ok();
    }
}

impl Read for PooledZlibReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.done {
            return Ok(0);
        }
        let Some(decomp) = self.decomp.as_mut() else {
            return Ok(0);
        };
        loop {
            let eof = self.rem.is_empty();
            let before_in = decomp.total_in();
            let before_out = decomp.total_out();
            let flush = if eof {
                FlushDecompress::Finish
            } else {
                FlushDecompress::None
            };
            let res = decomp.decompress(self.rem, output, flush);
            let consumed = usize::try_from(decomp.total_in() - before_in).unwrap_or(self.rem.len());
            let written = usize::try_from(decomp.total_out() - before_out).unwrap_or(output.len());
            self.rem = self.rem.get(consumed..).unwrap_or(&[]);
            match res {
                Ok(FlateStatus::StreamEnd) => {
                    self.done = true;
                    return Ok(written);
                }
                Ok(FlateStatus::Ok | FlateStatus::BufError) => {
                    if written > 0 {
                        return Ok(written);
                    }
                    if eof {
                        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                    }
                    if consumed == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "corrupt zlib stream",
                        ));
                    }
                }
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "corrupt zlib stream",
                    ));
                }
            }
        }
    }
}

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
    let clamped = level.min(9);
    let mut lease = ZlibCompressorLease::acquire(clamped);
    let Some(cached) = lease.inner.as_mut() else {
        return Err(Status::internal("deflate encode: missing compressor"));
    };
    cached.buf.clear();

    let mut rem = payload;
    loop {
        if cached.buf.capacity().saturating_sub(cached.buf.len()) < 4096 {
            super::gzip::write_chunk(out, &cached.buf, "deflate")?;
            cached.buf.clear();
            if cached.buf.capacity() < COMPRESS_SCRATCH_CAP {
                cached.buf.reserve(COMPRESS_SCRATCH_CAP);
            }
        }
        let before_in = cached.comp.total_in();
        let before_len = cached.buf.len();
        let status = cached
            .comp
            .compress_vec(rem, &mut cached.buf, FlushCompress::Finish)
            .map_err(|e| Status::internal(format!("deflate encode: {e}")))?;
        let consumed = usize::try_from(cached.comp.total_in() - before_in).unwrap_or(rem.len());
        let produced = cached.buf.len().saturating_sub(before_len);
        rem = rem
            .get(consumed..)
            .ok_or_else(|| Status::internal("deflate encode: invalid input progress"))?;
        match status {
            FlateStatus::StreamEnd => break,
            FlateStatus::Ok | FlateStatus::BufError => {
                if consumed == 0 && produced == 0 && cached.buf.is_empty() {
                    return Err(Status::internal("deflate encode: compressor stalled"));
                }
                if !cached.buf.is_empty() {
                    super::gzip::write_chunk(out, &cached.buf, "deflate")?;
                    cached.buf.clear();
                }
            }
        }
    }

    if !cached.buf.is_empty() {
        super::gzip::write_chunk(out, &cached.buf, "deflate")?;
        cached.buf.clear();
    }
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
    let mut decoder = PooledZlibReader::new(payload);
    let out = super::read_bounded_into(
        &mut decoder,
        limits.inflate_budget(),
        payload.len(),
        "deflate",
        out,
    )?;
    if !decoder.done || !decoder.rem.is_empty() {
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
