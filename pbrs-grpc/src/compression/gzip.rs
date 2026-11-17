//! `gzip` coding (RFC 1952) for [`Codec`](super::Codec).
//!
//! Backed by `miniz_oxide` through `flate2`.
//!
//! All members in one payload contribute to the same decoded-message cap.
//! Each member's checksum and size footer are validated; non-member tails
//! are rejected. Use [`decode_limited`] for untrusted input.

use crate::limits::MessageLimits;
use crate::status::Status;
use flate2::{
    Compress, Compression, Crc, Decompress, FlushCompress, FlushDecompress, Status as FlateStatus,
};
use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};

const FHCRC: u8 = 1 << 1;
const FEXTRA: u8 = 1 << 2;
const FNAME: u8 = 1 << 3;
const FCOMMENT: u8 = 1 << 4;
const FRESERVED: u8 = 0b1110_0000;
const MAX_HEADER_FIELD_BYTES: usize = 65_535;
const COMPRESS_SCRATCH_CAP: usize = 16 * 1024;
const MAX_POOLED_DECODERS: usize = 8;

struct CachedRawCompressor {
    level: u32,
    comp: Compress,
    buf: Vec<u8>,
}

impl CachedRawCompressor {
    fn new(level: u32) -> Self {
        Self {
            level,
            comp: Compress::new(Compression::new(level), false),
            buf: Vec::with_capacity(COMPRESS_SCRATCH_CAP),
        }
    }
}

thread_local! {
    static RAW_COMPRESSORS: RefCell<[Option<CachedRawCompressor>; 2]> =
        const { RefCell::new([None, None]) };
    static RAW_DECOMPRESSORS: RefCell<Vec<Decompress>> = const { RefCell::new(Vec::new()) };
}

struct RawCompressorLease {
    inner: Option<CachedRawCompressor>,
}

impl RawCompressorLease {
    fn acquire(level: u32) -> Self {
        let cached = RAW_COMPRESSORS
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
            .unwrap_or_else(|| CachedRawCompressor::new(level));
        Self {
            inner: Some(cached),
        }
    }
}

impl Drop for RawCompressorLease {
    fn drop(&mut self) {
        let Some(mut cached) = self.inner.take() else {
            return;
        };
        cached.comp.reset();
        cached.buf.clear();
        if cached.buf.capacity() > COMPRESS_SCRATCH_CAP * 4 {
            cached.buf.shrink_to(COMPRESS_SCRATCH_CAP);
        }
        RAW_COMPRESSORS
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

// Returned pool entries are reset before insertion; new engines also start clean.
fn acquire_raw_decompressor() -> Decompress {
    RAW_DECOMPRESSORS
        .try_with(|cell| cell.try_borrow_mut().ok().and_then(|mut pool| pool.pop()))
        .ok()
        .flatten()
        .unwrap_or_else(|| Decompress::new(false))
}

fn release_raw_decompressor(mut decomp: Decompress) {
    RAW_DECOMPRESSORS
        .try_with(|cell| {
            if let Ok(mut pool) = cell.try_borrow_mut() {
                if pool.len() < MAX_POOLED_DECODERS {
                    decomp.reset(false);
                    pool.push(decomp);
                }
            }
        })
        .ok();
}

enum HeaderStep {
    Fixed { pos: usize, buf: [u8; 10] },
    Xlen { pos: usize, buf: [u8; 2] },
    Extra { rem: usize },
    Name { seen: usize },
    Comment { seen: usize },
    Hcrc { pos: usize, buf: [u8; 2] },
    Done,
}

struct HeaderParser {
    flg: u8,
    hcrc: Option<Crc>,
    step: HeaderStep,
}

impl HeaderParser {
    fn new() -> Self {
        Self {
            flg: 0,
            hcrc: None,
            step: HeaderStep::Fixed {
                pos: 0,
                buf: [0; 10],
            },
        }
    }

    fn step_after_fixed(flg: u8) -> HeaderStep {
        if (flg & FEXTRA) != 0 {
            HeaderStep::Xlen {
                pos: 0,
                buf: [0; 2],
            }
        } else {
            Self::step_after_extra(flg)
        }
    }

    fn step_after_extra(flg: u8) -> HeaderStep {
        if (flg & FNAME) != 0 {
            HeaderStep::Name { seen: 0 }
        } else {
            Self::step_after_name(flg)
        }
    }

    fn step_after_name(flg: u8) -> HeaderStep {
        if (flg & FCOMMENT) != 0 {
            HeaderStep::Comment { seen: 0 }
        } else {
            Self::step_after_comment(flg)
        }
    }

    fn step_after_comment(flg: u8) -> HeaderStep {
        if (flg & FHCRC) != 0 {
            HeaderStep::Hcrc {
                pos: 0,
                buf: [0; 2],
            }
        } else {
            HeaderStep::Done
        }
    }

    fn advance<R: BufRead>(&mut self, reader: &mut R) -> io::Result<()> {
        loop {
            match &mut self.step {
                HeaderStep::Fixed { pos, buf } => {
                    fill_exact(reader, buf, pos)?;
                    let [id1, id2, cm, flg, ..] = *buf;
                    if id1 != 0x1f || id2 != 0x8b || cm != 8 || (flg & FRESERVED) != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "corrupt gzip header",
                        ));
                    }
                    self.flg = flg;
                    if (flg & FHCRC) != 0 {
                        let mut crc = Crc::new();
                        crc.update(&*buf);
                        self.hcrc = Some(crc);
                    }
                    self.step = Self::step_after_fixed(flg);
                }
                HeaderStep::Xlen { pos, buf } => {
                    fill_exact(reader, buf, pos)?;
                    let bytes = *buf;
                    if let Some(crc) = &mut self.hcrc {
                        crc.update(&bytes);
                    }
                    let rem = usize::from(u16::from_le_bytes(bytes));
                    self.step = HeaderStep::Extra { rem };
                }
                HeaderStep::Extra { rem } => {
                    while *rem > 0 {
                        let avail = reader.fill_buf()?;
                        if avail.is_empty() {
                            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                        }
                        let take = (*rem).min(avail.len());
                        if let (Some(crc), Some(prefix)) = (&mut self.hcrc, avail.get(..take)) {
                            crc.update(prefix);
                        }
                        reader.consume(take);
                        *rem -= take;
                    }
                    self.step = Self::step_after_extra(self.flg);
                }
                HeaderStep::Name { seen } => {
                    consume_cstring(reader, seen, &mut self.hcrc)?;
                    self.step = Self::step_after_name(self.flg);
                }
                HeaderStep::Comment { seen } => {
                    consume_cstring(reader, seen, &mut self.hcrc)?;
                    self.step = Self::step_after_comment(self.flg);
                }
                HeaderStep::Hcrc { pos, buf } => {
                    fill_exact(reader, buf, pos)?;
                    let expected = u16::from_le_bytes(*buf);
                    let actual = self.hcrc.as_ref().map_or(0, |crc| {
                        let [low, high, _, _] = crc.sum().to_le_bytes();
                        u16::from_le_bytes([low, high])
                    });
                    if expected != actual {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "corrupt gzip header checksum",
                        ));
                    }
                    self.step = HeaderStep::Done;
                }
                HeaderStep::Done => return Ok(()),
            }
        }
    }
}

fn fill_exact<R: BufRead>(reader: &mut R, buf: &mut [u8], pos: &mut usize) -> io::Result<()> {
    while *pos < buf.len() {
        let avail = reader.fill_buf()?;
        if avail.is_empty() {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        let Some(dst) = buf.get_mut(*pos..) else {
            break;
        };
        let take = dst.len().min(avail.len());
        if let (Some(dst_prefix), Some(src_prefix)) = (dst.get_mut(..take), avail.get(..take)) {
            dst_prefix.copy_from_slice(src_prefix);
        }
        reader.consume(take);
        *pos += take;
    }
    Ok(())
}

fn consume_cstring<R: BufRead>(
    reader: &mut R,
    seen: &mut usize,
    hcrc: &mut Option<Crc>,
) -> io::Result<()> {
    loop {
        let avail = reader.fill_buf()?;
        if avail.is_empty() {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        match avail.iter().position(|&byte| byte == 0) {
            Some(nul_idx) => {
                if seen.saturating_add(nul_idx) > MAX_HEADER_FIELD_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "gzip header field too long",
                    ));
                }
                let take = nul_idx + 1;
                if let (Some(crc), Some(prefix)) = (hcrc.as_mut(), avail.get(..take)) {
                    crc.update(prefix);
                }
                reader.consume(take);
                return Ok(());
            }
            None => {
                let take = avail.len();
                *seen = seen.saturating_add(take);
                if *seen > MAX_HEADER_FIELD_BYTES {
                    reader.consume(take);
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "gzip header field too long",
                    ));
                }
                if let Some(crc) = hcrc.as_mut() {
                    crc.update(avail);
                }
                reader.consume(take);
            }
        }
    }
}

enum MemberPhase {
    First,
    Header(HeaderParser),
    Body,
    Footer { pos: usize, buf: [u8; 8] },
    BetweenMembers,
    Done,
}

/// Message-local member decoder shared by native and tonic inbound paths.
/// Admission precedes acquisition and each later member's DEFLATE history reset.
pub(crate) struct GzipMembers<R> {
    reader: R,
    decomp: Option<Decompress>,
    body_crc: Crc,
    phase: MemberPhase,
    frames: usize,
    max_frames: Option<usize>,
    failure: Option<Status>,
}

impl<R> Drop for GzipMembers<R> {
    fn drop(&mut self) {
        if let Some(decomp) = self.decomp.take() {
            release_raw_decompressor(decomp);
        }
    }
}

impl<R: BufRead> GzipMembers<R> {
    pub(crate) fn new(reader: R, max_frames: Option<usize>) -> Self {
        Self {
            reader,
            decomp: None,
            body_crc: Crc::new(),
            phase: MemberPhase::First,
            frames: 0,
            max_frames,
            failure: None,
        }
    }

    #[cfg(any(feature = "tonic", test))]
    pub(crate) fn get_ref(&self) -> &R {
        &self.reader
    }

    #[cfg(feature = "tonic")]
    pub(crate) fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    pub(crate) fn take_failure(&mut self) -> Option<Status> {
        self.failure.take()
    }

    fn admit_member(&mut self) -> io::Result<()> {
        if let Some(max) = self.max_frames.filter(|&max| self.frames >= max) {
            self.failure = Some(Status::resource_exhausted(format!(
                "gzip member count exceeds limit {max}"
            )));
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "gzip member limit",
            ));
        }
        self.frames = self.frames.checked_add(1).ok_or_else(|| {
            self.failure = Some(Status::resource_exhausted("gzip member count overflow"));
            io::Error::new(io::ErrorKind::InvalidData, "gzip member count overflow")
        })?;
        Ok(())
    }

    fn ensure_decompressor(&mut self) -> &mut Decompress {
        self.decomp.get_or_insert_with(acquire_raw_decompressor)
    }
}

impl<R: BufRead> Read for GzipMembers<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            match &mut self.phase {
                MemberPhase::First => {
                    self.admit_member()?;
                    self.ensure_decompressor();
                    self.body_crc = Crc::new();
                    self.phase = MemberPhase::Header(HeaderParser::new());
                }
                MemberPhase::BetweenMembers => {
                    // Preserve this state on WouldBlock/Interrupted: retrying must
                    // inspect the next member instead of reading the completed one.
                    if self.reader.fill_buf()?.is_empty() {
                        self.phase = MemberPhase::Done;
                        return Ok(0);
                    }
                    self.admit_member()?;
                    self.ensure_decompressor().reset(false);
                    self.body_crc = Crc::new();
                    self.phase = MemberPhase::Header(HeaderParser::new());
                }
                MemberPhase::Header(parser) => {
                    parser.advance(&mut self.reader)?;
                    self.phase = MemberPhase::Body;
                }
                MemberPhase::Body => {
                    let input = self.reader.fill_buf()?;
                    let eof = input.is_empty();
                    let decomp = self.decomp.get_or_insert_with(acquire_raw_decompressor);
                    let before_in = decomp.total_in();
                    let before_out = decomp.total_out();
                    let flush = if eof {
                        FlushDecompress::Finish
                    } else {
                        FlushDecompress::None
                    };
                    let res = decomp.decompress(input, output, flush);
                    let consumed =
                        usize::try_from(decomp.total_in() - before_in).unwrap_or(input.len());
                    let written =
                        usize::try_from(decomp.total_out() - before_out).unwrap_or(output.len());
                    self.reader.consume(consumed);
                    if let Some(decoded) = output.get(..written) {
                        if !decoded.is_empty() {
                            self.body_crc.update(decoded);
                        }
                    }
                    match res {
                        Ok(FlateStatus::StreamEnd) => {
                            self.phase = MemberPhase::Footer {
                                pos: 0,
                                buf: [0; 8],
                            };
                            if written > 0 {
                                return Ok(written);
                            }
                        }
                        Ok(FlateStatus::Ok | FlateStatus::BufError) => {
                            if written > 0 {
                                return Ok(written);
                            }
                            if eof {
                                self.phase = MemberPhase::Done;
                                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                            }
                            if consumed == 0 {
                                self.phase = MemberPhase::Done;
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "corrupt deflate stream",
                                ));
                            }
                        }
                        Err(_) => {
                            self.phase = MemberPhase::Done;
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "corrupt deflate stream",
                            ));
                        }
                    }
                }
                MemberPhase::Footer { pos, buf } => {
                    fill_exact(&mut self.reader, buf, pos)?;
                    let [c0, c1, c2, c3, l0, l1, l2, l3] = *buf;
                    let expected_crc = u32::from_le_bytes([c0, c1, c2, c3]);
                    let expected_len = u32::from_le_bytes([l0, l1, l2, l3]);
                    if expected_crc != self.body_crc.sum() || expected_len != self.body_crc.amount()
                    {
                        self.phase = MemberPhase::Done;
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "corrupt gzip stream does not have a matching checksum",
                        ));
                    }
                    self.phase = MemberPhase::BetweenMembers;
                }
                MemberPhase::Done => return Ok(0),
            }
        }
    }
}

/// gzip `payload` at the kernel default (deflate level 1, [`Compression::fast`]).
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    encode_level(payload, crate::config::DEFAULT_GZIP_COMPRESSION_LEVEL)
}

/// gzip `payload` at deflate `level` (0 stores, 1 is fast, 9 is best).
///
/// Values above 9 are clamped to 9. Default 1: at gRPC message sizes the extra
/// CPU of higher levels often costs more latency than the saved bytes buy back.
pub fn encode_level(payload: &[u8], level: u32) -> Result<Vec<u8>, Status> {
    let mut out = Vec::with_capacity(payload.len() / 2 + 32);
    encode_into(payload, level, &mut out)?;
    Ok(out)
}

pub(super) fn write_chunk(out: &mut impl Write, bytes: &[u8], coding: &str) -> Result<(), Status> {
    let mut rest = bytes;
    while !rest.is_empty() {
        match out.write(rest) {
            Ok(0) => {
                return Err(Status::internal(format!(
                    "{coding} encode: {}",
                    io::Error::from(io::ErrorKind::WriteZero)
                )));
            }
            Ok(n) => {
                rest = rest
                    .get(n..)
                    .ok_or_else(|| Status::internal(format!("{coding} encode: invalid write")))?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(Status::internal(format!("{coding} encode: {error}"))),
        }
    }
    Ok(())
}

/// gzip `payload` at `level`, appending to `out`.
///
/// The framing layer compresses straight into the frame buffer through
/// this, saving the intermediate `Vec` and its copy.
pub(super) fn encode_into(payload: &[u8], level: u32, out: &mut impl Write) -> Result<(), Status> {
    let clamped = level.min(9);
    let xfl = if clamped >= 9 {
        2
    } else if clamped <= 1 {
        4
    } else {
        0
    };
    let header = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 255];

    let mut lease = RawCompressorLease::acquire(clamped);
    let Some(cached) = lease.inner.as_mut() else {
        return Err(Status::internal("gzip encode: missing compressor"));
    };
    cached.buf.clear();
    cached.buf.extend_from_slice(&header);

    let mut rem = payload;
    loop {
        if cached.buf.capacity().saturating_sub(cached.buf.len()) < 4096 {
            write_chunk(out, &cached.buf, "gzip")?;
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
            .map_err(|error| Status::internal(format!("gzip encode: {error}")))?;
        let consumed = usize::try_from(cached.comp.total_in() - before_in).unwrap_or(rem.len());
        let produced = cached.buf.len().saturating_sub(before_len);
        rem = rem
            .get(consumed..)
            .ok_or_else(|| Status::internal("gzip encode: invalid input progress"))?;
        match status {
            FlateStatus::StreamEnd => break,
            FlateStatus::Ok | FlateStatus::BufError => {
                if consumed == 0 && produced == 0 && cached.buf.is_empty() {
                    return Err(Status::internal("gzip encode: compressor stalled"));
                }
                if !cached.buf.is_empty() {
                    write_chunk(out, &cached.buf, "gzip")?;
                    cached.buf.clear();
                }
            }
        }
    }

    let mut crc = Crc::new();
    crc.update(payload);
    let [c0, c1, c2, c3] = crc.sum().to_le_bytes();
    let [l0, l1, l2, l3] = crc.amount().to_le_bytes();
    let footer = [c0, c1, c2, c3, l0, l1, l2, l3];

    if cached.buf.capacity().saturating_sub(cached.buf.len()) >= footer.len() {
        cached.buf.extend_from_slice(&footer);
        write_chunk(out, &cached.buf, "gzip")?;
    } else {
        if !cached.buf.is_empty() {
            write_chunk(out, &cached.buf, "gzip")?;
        }
        write_chunk(out, &footer, "gzip")?;
    }
    cached.buf.clear();
    Ok(())
}

/// Inflate `payload` with no cap.
///
/// Prefer [`decode_limited`]. This is only safe against a trusted peer.
pub fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    decode_limited(payload, MessageLimits::unlimited())
}

/// Inflate every RFC 1952 member in `payload` under one decoded-message cap.
/// Non-member bytes and malformed later members are rejected.
///
/// The output buffer capacity stays within the cap. A fixed 8 KiB stack
/// chunk probes one extra decoded byte to detect overflow.
/// Exceeding it is [`Code::ResourceExhausted`](crate::Code::ResourceExhausted),
/// reported against the cap rather than the true inflated size, because
/// learning the true size is exactly the work being refused.
pub fn decode_limited(payload: &[u8], limits: MessageLimits) -> Result<Vec<u8>, Status> {
    decode_limited_into(payload, limits, Vec::new())
}

pub(super) fn decode_limited_into(
    payload: &[u8],
    limits: MessageLimits,
    out: Vec<u8>,
) -> Result<Vec<u8>, Status> {
    // Frame bytes already implement BufRead; no extra compressed-input buffer.
    let mut decoder = GzipMembers::new(payload, limits.max_compression_frames());
    let result = super::read_bounded_into(
        &mut decoder,
        limits.inflate_budget(),
        payload.len(),
        "gzip",
        out,
    );
    if let Some(status) = decoder.take_failure() {
        return Err(status);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{GzipMembers, decode, decode_limited, encode, encode_into, encode_level};
    use crate::limits::MessageLimits;
    use crate::status::Code;
    use std::io::{self, Read, Write};

    fn assert_decoder_pool_reset() {
        super::RAW_DECOMPRESSORS.with(|cell| {
            let pool = cell.borrow();
            assert!(pool.len() <= super::MAX_POOLED_DECODERS);
            assert!(pool.iter().all(|d| d.total_in() == 0 && d.total_out() == 0));
        });
    }

    #[test]
    fn decoder_pool_recovers_after_errors_and_output_limits() {
        super::RAW_DECOMPRESSORS.with(|cell| cell.borrow_mut().clear());
        let payload = b"independent decoder history".repeat(40);
        let encoded = encode(&payload).expect("encode");
        let next = encode(b"next message").expect("encode next");

        for end in 0..encoded.len() {
            assert!(decode(&encoded[..end]).is_err(), "truncated at {end}");
            assert_decoder_pool_reset();
            assert_eq!(
                decode(&next).expect("decode after truncation"),
                b"next message"
            );
        }
        let mut bad_crc = encoded.clone();
        let crc_pos = bad_crc.len() - 8;
        bad_crc[crc_pos] ^= 1;
        assert!(decode(&bad_crc).is_err());
        assert_decoder_pool_reset();
        assert_eq!(
            decode_limited(&encoded, MessageLimits::new().with_max_decoding(3))
                .expect_err("decoded cap")
                .code(),
            Code::ResourceExhausted
        );
        assert_decoder_pool_reset();
        let joined = [encoded.as_slice(), next.as_slice()].concat();
        assert_eq!(
            decode(&joined).expect("independent concatenated members"),
            [payload.as_slice(), b"next message"].concat()
        );
        assert_decoder_pool_reset();
        assert_eq!(decode(&next).expect("fresh reuse"), b"next message");
        assert!(
            decode(&encode(b"").expect("empty frame"))
                .expect("empty")
                .is_empty()
        );
    }

    #[test]
    fn decoder_pool_recovers_from_early_drop_and_unwinding_and_stays_bounded() {
        super::RAW_DECOMPRESSORS.with(|cell| cell.borrow_mut().clear());
        let encoded = encode(&vec![b'a'; 4096]).expect("encode");
        let mut active = Vec::new();
        for _ in 0..super::MAX_POOLED_DECODERS + 2 {
            let mut decoder = GzipMembers::new(encoded.as_slice(), None);
            let mut byte = [0];
            decoder.read_exact(&mut byte).expect("partial decode");
            assert_eq!(byte, [b'a']);
            active.push(decoder);
        }
        drop(active);
        assert_decoder_pool_reset();
        super::RAW_DECOMPRESSORS.with(|cell| {
            assert_eq!(cell.borrow().len(), super::MAX_POOLED_DECODERS);
        });
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut decoder = GzipMembers::new(encoded.as_slice(), None);
            let mut byte = [0];
            decoder.read_exact(&mut byte).expect("partial decode");
            panic!("unwind while the decoder is leased");
        }));
        assert!(caught.is_err());
        assert_decoder_pool_reset();
        assert_eq!(
            decode(&encoded).expect("decode after unwind"),
            vec![b'a'; 4096]
        );
    }

    #[test]
    fn decoder_pool_falls_back_while_borrowed() {
        let payload = b"a separate engine while the pool is borrowed";
        let encoded = encode(payload).expect("encode");
        super::RAW_DECOMPRESSORS.with(|cell| {
            let mut pool = cell.borrow_mut();
            pool.clear();
            assert_eq!(decode(&encoded).expect("decode without the pool"), payload);
            assert!(pool.is_empty(), "the borrowed pool cannot accept a return");
        });
        assert_eq!(decode(&encoded).expect("decode with the pool"), payload);
        assert_decoder_pool_reset();
    }

    #[test]
    fn pooled_decoders_keep_independent_histories_across_sizes_and_codecs() {
        for length in [0, 1, 1024, 65_537, 1024 * 1024, 15, 8192, 0] {
            let payload = noise(length);
            let gzip = encode(&payload).expect("gzip encode");
            let zlib = crate::compression::deflate::encode(&payload).expect("deflate encode");
            assert_eq!(decode(&gzip).expect("gzip history"), payload);
            assert_eq!(
                crate::compression::deflate::decode(&zlib).expect("deflate history"),
                payload
            );
            assert_eq!(decode(&gzip).expect("gzip reuse"), payload);
            assert_decoder_pool_reset();
        }
    }

    fn noise(length: usize) -> Vec<u8> {
        let mut state = 0x7f4a_7c15_9e37_79b9u64;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()[0]
            })
            .collect()
    }

    fn reference_decode_single_member(encoded: &[u8]) -> Vec<u8> {
        let mut decoder = flate2::bufread::GzDecoder::new(encoded);
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .expect("reference decoder validates DEFLATE, CRC and ISIZE");
        assert!(decoder.get_ref().is_empty(), "exactly one complete member");
        decoded
    }

    #[test]
    fn encoded_messages_have_complete_independent_members_across_sizes_and_levels() {
        for length in [
            0,
            1,
            15,
            1024,
            8191,
            8192,
            8193,
            32767,
            32768,
            32769,
            65535,
            65536,
            65537,
            1024 * 1024,
        ] {
            let compressible = vec![0; length];
            let incompressible = noise(length);
            for payload in [&compressible, &incompressible] {
                for level in [0, 1, 6, 9, 99] {
                    let encoded = encode_level(payload, level).expect("encode complete member");
                    assert_eq!(
                        reference_decode_single_member(&encoded),
                        *payload,
                        "length {length}, level {level}"
                    );
                }
                assert_eq!(
                    encode_level(payload, 99).expect("clamped level"),
                    encode_level(payload, 9).expect("maximum level"),
                    "clamping must not select a different compression level"
                );
            }
        }
    }

    #[test]
    fn encoding_appends_without_sharing_message_history() {
        let first = noise(32769);
        let second = vec![7; 8193];
        let prefix = b"an existing frame prefix";
        let mut output = prefix.to_vec();
        encode_into(&first, 6, &mut output).expect("first message");
        let boundary = output.len();
        encode_into(&second, 6, &mut output).expect("second message");
        assert_eq!(&output[..prefix.len()], prefix);
        assert_eq!(
            reference_decode_single_member(&output[prefix.len()..boundary]),
            first
        );
        assert_eq!(reference_decode_single_member(&output[boundary..]), second);
    }

    #[test]
    fn encoding_retries_interrupted_and_short_writes_without_flushing_the_sink() {
        #[derive(Default)]
        struct ShortWriter {
            bytes: Vec<u8>,
            interrupt: bool,
            interrupted: usize,
            flushes: usize,
        }

        impl Write for ShortWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.interrupt = !self.interrupt;
                if self.interrupt {
                    self.interrupted += 1;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let n = bytes.len().min(7);
                self.bytes.extend_from_slice(&bytes[..n]);
                Ok(n)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }

        let payload = noise(32769);
        let mut writer = ShortWriter::default();
        encode_into(&payload, 6, &mut writer).expect("short/interrupted sink");
        assert!(writer.interrupted > 1);
        assert_eq!(writer.flushes, 0, "framing owns sink flush policy");
        assert_eq!(reference_decode_single_member(&writer.bytes), payload);
    }

    #[test]
    fn encoding_returns_the_first_sink_error_without_later_writes() {
        struct FailingWriter {
            bytes: Vec<u8>,
            fail_after: usize,
            zero: bool,
            failed: bool,
            writes_after_failure: usize,
            flushes: usize,
        }

        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.failed {
                    self.writes_after_failure += 1;
                    return Err(io::Error::other("write after failure"));
                }
                if self.bytes.len() == self.fail_after {
                    self.failed = true;
                    return if self.zero {
                        Ok(0)
                    } else {
                        Err(io::Error::other("injected encode failure"))
                    };
                }
                let n = bytes.len().min(self.fail_after - self.bytes.len());
                self.bytes.extend_from_slice(&bytes[..n]);
                Ok(n)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }

        let payload = noise(32769);
        let complete = encode_level(&payload, 6).expect("complete member");
        // Reject within the header, compressed body, CRC and ISIZE.
        for fail_after in [
            0,
            1,
            9,
            10,
            complete.len() / 2,
            complete.len() - 8,
            complete.len() - 1,
        ] {
            for zero in [false, true] {
                let mut writer = FailingWriter {
                    bytes: Vec::new(),
                    fail_after,
                    zero,
                    failed: false,
                    writes_after_failure: 0,
                    flushes: 0,
                };
                let error = encode_into(&payload, 6, &mut writer).expect_err("sink failure");
                assert_eq!(error.code(), Code::Internal);
                if !zero {
                    assert_eq!(error.message(), "gzip encode: injected encode failure");
                }
                assert!(writer.failed);
                assert_eq!(writer.bytes, complete[..fail_after]);
                assert_eq!(writer.writes_after_failure, 0);
                assert_eq!(writer.flushes, 0);
            }
        }
    }

    // Independently specified RFC 1952 member with one stored DEFLATE block.
    // CRC32("hello") = 0x3610a686, ISIZE = 5.
    const HELLO_MEMBER: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x01, 0x05, 0x00, 0xfa, 0xff,
        b'h', b'e', b'l', b'l', b'o', 0x86, 0xa6, 0x10, 0x36, 0x05, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn member_count_admission_is_independent_of_output_bytes() {
        let empty = encode(b"").expect("empty member");
        let limits = MessageLimits::new().with_max_decoding(0);
        assert!(
            decode_limited(&empty.repeat(1024), limits)
                .expect("default exact count")
                .is_empty()
        );
        assert_eq!(
            decode_limited(&empty.repeat(1025), limits)
                .expect_err("default count limit")
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
        let two = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let exact = MessageLimits::new().with_max_compression_frames(2);
        assert_eq!(
            decode_limited(&two, exact).expect("exact count"),
            b"hellohello"
        );
        assert_eq!(
            decode_limited(&two, exact.with_max_compression_frames(1))
                .expect_err("one extra member")
                .code(),
            Code::ResourceExhausted
        );
        assert_eq!(
            decode_limited(HELLO_MEMBER, exact.with_max_compression_frames(0))
                .expect_err("no member admitted")
                .code(),
            Code::ResourceExhausted
        );
        let malformed = [HELLO_MEMBER, b"invalid"].concat();
        assert_eq!(
            decode_limited(&malformed, exact)
                .expect_err("admitted malformed later member")
                .code(),
            Code::Internal
        );
        assert_eq!(
            decode_limited(&malformed, exact.with_max_compression_frames(1))
                .expect_err("refused before next member reset")
                .code(),
            Code::ResourceExhausted
        );
    }

    #[derive(Default)]
    struct BoundaryYield<'a> {
        remaining: &'a [u8],
        consumed: usize,
        yielded: bool,
    }

    impl std::io::BufRead for BoundaryYield<'_> {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.consumed == HELLO_MEMBER.len() && !self.yielded {
                self.yielded = true;
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            Ok(self.remaining)
        }

        fn consume(&mut self, amount: usize) {
            self.remaining = self.remaining.get(amount..).expect("valid consumption");
            self.consumed += amount;
        }
    }

    impl std::io::Read for BoundaryYield<'_> {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            use std::io::BufRead;
            let input = self.fill_buf()?;
            let n = input.len().min(target.len());
            target
                .get_mut(..n)
                .expect("output")
                .copy_from_slice(input.get(..n).expect("input"));
            self.consume(n);
            Ok(n)
        }
    }

    #[test]
    fn member_end_survives_would_block_before_the_next_reset() {
        use std::io::Read;
        let joined = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let input = BoundaryYield {
            remaining: &joined,
            ..BoundaryYield::default()
        };
        let mut decoder = GzipMembers::new(input, Some(2));
        let mut output = Vec::new();
        loop {
            let mut chunk = [0; 16];
            match decoder.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(chunk.get(..n).expect("chunk")),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => panic!("unexpected decode failure: {error}"),
            }
        }
        assert!(decoder.get_ref().yielded);
        assert!(decoder.take_failure().is_none());
        assert_eq!(output, b"hellohello");
        assert_eq!(decoder.frames, 2);
    }

    #[test]
    fn every_member_contributes_to_the_same_decoded_message_limit() {
        let joined = [HELLO_MEMBER, HELLO_MEMBER].concat();
        let exact = MessageLimits::new().with_max_decoding(10);
        assert_eq!(
            decode_limited(&joined, exact).expect("both members"),
            b"hellohello"
        );
        assert_eq!(
            decode_limited(&joined, exact.with_max_decoding(9))
                .expect_err("aggregate cap")
                .code(),
            Code::ResourceExhausted
        );
        let empty = encode(b"").expect("empty member");
        let joined = [empty.as_slice(), HELLO_MEMBER, empty.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, exact.with_max_decoding(5)).expect("empty members"),
            b"hello"
        );
    }

    #[test]
    fn later_member_crc_and_isize_are_validated() {
        for offset_from_end in [8, 4] {
            let mut later = HELLO_MEMBER.to_vec();
            let index = later.len() - offset_from_end;
            *later.get_mut(index).expect("footer byte") ^= 1;
            let joined = [HELLO_MEMBER, later.as_slice()].concat();
            assert_eq!(
                decode(&joined).expect_err("later footer").code(),
                Code::Internal
            );
        }
        // No extra output is available at this cap. The next empty member's
        // checksum must still be checked while probing for end-of-message.
        let mut empty = encode(b"").expect("empty member");
        let footer = empty.len() - 8;
        *empty.get_mut(footer).expect("empty CRC") ^= 1;
        let joined = [HELLO_MEMBER, empty.as_slice()].concat();
        assert_eq!(
            decode_limited(&joined, MessageLimits::new().with_max_decoding(5))
                .expect_err("empty later CRC at exact cap")
                .code(),
            Code::Internal
        );
    }

    #[test]
    fn every_truncated_later_member_and_nonmember_tail_is_rejected() {
        for end in 1..HELLO_MEMBER.len() {
            let tail = HELLO_MEMBER.get(..end).expect("member prefix");
            let joined = [HELLO_MEMBER, tail].concat();
            assert_eq!(
                decode(&joined).expect_err("truncated later member").code(),
                Code::Internal,
                "tail length {end}"
            );
        }
        for tail in [b"junk".as_slice(), &[0], &[0, 0, 0, 0]] {
            let joined = [HELLO_MEMBER, tail].concat();
            assert_eq!(
                decode(&joined).expect_err("nonmember bytes").code(),
                Code::Internal
            );
        }
    }

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
        assert_eq!(decode(&store).expect("decode store"), payload);
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let gz = encode(&vec![3u8; 4096]).expect("encode");
        let err = decode(&gz[..gz.len() / 2]).expect_err("truncated");
        assert_eq!(err.code(), Code::Internal);
    }

    #[test]
    fn optional_rfc1952_header_fields_and_hcrc_are_supported() {
        let payload = b"optional header fields payload";
        let mut base = Vec::new();
        {
            let mut enc = flate2::GzBuilder::new()
                .extra(b"kv=12345")
                .filename("hello.proto")
                .comment("test comment")
                .write(&mut base, flate2::Compression::fast());
            enc.write_all(payload).expect("write");
            enc.finish().expect("finish");
        }
        assert_eq!(decode(&base).expect("decode rfc1952 fields"), payload);

        // Add FHCRC (0x02) over the full header (10 fixed + 2+8 extra + 12 fname + 13 fcomment = 45 bytes).
        let hdr_len =
            10 + 2 + b"kv=12345".len() + (b"hello.proto".len() + 1) + (b"test comment".len() + 1);
        let mut with_hcrc = base.get(..hdr_len).expect("hdr").to_vec();
        *with_hcrc.get_mut(3).expect("flg") |= super::FHCRC;
        let mut hcrc = super::Crc::new();
        hcrc.update(&with_hcrc);
        let crc32 = hcrc.sum().to_le_bytes();
        with_hcrc.extend_from_slice(&[crc32[0], crc32[1]]);
        with_hcrc.extend_from_slice(base.get(hdr_len..).expect("body+footer"));
        assert_eq!(
            decode(&with_hcrc).expect("decode full rfc1952 + hcrc"),
            payload
        );

        // Flip a byte inside the header and ensure FHCRC catches it.
        let mut corrupted_hcrc = with_hcrc.clone();
        *corrupted_hcrc.get_mut(12).expect("extra byte") ^= 0x01;
        assert_eq!(
            decode(&corrupted_hcrc)
                .expect_err("corrupt header crc")
                .code(),
            Code::Internal
        );

        // Set a reserved flag bit (bit 5 = 0x20) and ensure header validation rejects it.
        let mut reserved_flag = encode(payload).expect("encode");
        *reserved_flag.get_mut(3).expect("flg") |= 0x20;
        assert_eq!(
            decode(&reserved_flag).expect_err("reserved flag").code(),
            Code::Internal
        );
    }
}
