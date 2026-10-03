//! Binary wire codec. No schema.
#![allow(
    clippy::unwrap_used,
    reason = "fixed32/64 try_into and 16-byte SIMD chunk after a length check"
)]

use crate::error::ParseError;
use crate::internal::MAX_MESSAGE_BYTES;
use bytes::{BufMut, Bytes};
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Byte sink for binary encode. Implemented for every [`BufMut`] (`Vec<u8>`,
/// `BytesMut`, tonic `EncodeBuf`).
///
/// `push` / `extend_from_slice` match generated `write_to` bodies so those
/// methods can target `EncodeBuf` without a per-message `Vec`.
pub trait WireOut {
    fn put_u8(&mut self, b: u8);
    fn put_slice(&mut self, data: &[u8]);

    #[inline]
    fn push(&mut self, b: u8) {
        self.put_u8(b);
    }

    #[inline]
    fn extend_from_slice(&mut self, data: &[u8]) {
        self.put_slice(data);
    }

    /// Hand a large field buffer to the sink without copying. The default
    /// copies, so every existing sink (and old generated code, which never
    /// calls this) behaves exactly as before. Segmented sinks override it to
    /// retain `b` and emit it as its own segment (PK-11).
    #[inline]
    fn put_shared(&mut self, b: &Bytes) {
        self.put_slice(b);
    }

    /// Whether `put_shared` retains without copying. Sinks that override
    /// `put_shared` must override this to `true`; the default `false`
    /// keeps callers from paying for a shareable handle (e.g. a `Bytes`
    /// clone, which can promote a unique buffer) that would only be
    /// copied back out.
    #[inline]
    fn supports_shared(&self) -> bool {
        false
    }
}

impl<T: BufMut + ?Sized> WireOut for T {
    #[inline]
    fn put_u8(&mut self, b: u8) {
        <T as BufMut>::put_u8(self, b);
    }

    #[inline]
    fn put_slice(&mut self, data: &[u8]) {
        crate::copy_counts::note_emit(data.len());
        <T as BufMut>::put_slice(self, data);
    }
}

pub const WIRE_VARINT: u32 = 0;
pub const WIRE_I64: u32 = 1;
pub const WIRE_LEN: u32 = 2;
pub const WIRE_SGROUP: u32 = 3;
pub const WIRE_EGROUP: u32 = 4;
pub const WIRE_I32: u32 = 5;

/// Largest legal protobuf field number (2^29 - 1).
///
/// Shared by tag decode and table validation so the bound lives in one place.
pub const MAX_FIELD_NUMBER: u32 = 536_870_911;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownField {
    Varint { number: u32, value: u64 },
    Fixed64 { number: u32, value: u64 },
    LengthDelimited { number: u32, value: Vec<u8> },
    Group { number: u32, fields: UnknownFields },
    Fixed32 { number: u32, value: u32 },
}

/// Unknown-field list. Empty is an 8-byte null so TAT Default stays small.
/// Named `fields` so generated `self.unknown.fields.push(...)` keeps working.
#[expect(
    clippy::box_collection,
    reason = "unknown fields stay a boxed vec so empty messages stay small"
)]
#[derive(Clone, Debug)]
pub struct FieldList(Option<Box<Vec<UnknownField>>>);

impl Default for FieldList {
    #[inline]
    fn default() -> Self {
        Self(None)
    }
}

impl PartialEq for FieldList {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for FieldList {}

impl FieldList {
    const EMPTY: &'static [UnknownField] = &[];

    #[inline]
    pub fn as_slice(&self) -> &[UnknownField] {
        self.0.as_ref().map_or(Self::EMPTY, |v| v.as_slice())
    }

    pub fn push(&mut self, value: UnknownField) {
        self.0
            .get_or_insert_with(|| Box::new(Vec::new()))
            .push(value);
    }

    pub fn extend<I: IntoIterator<Item = UnknownField>>(&mut self, iter: I) {
        let mut iter = iter.into_iter();
        let Some(first) = iter.next() else {
            return;
        };
        let v = self.0.get_or_insert_with(|| Box::new(Vec::new()));
        v.push(first);
        v.extend(iter);
    }

    pub fn iter(&self) -> std::slice::Iter<'_, UnknownField> {
        self.as_slice().iter()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|v| v.is_empty())
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Retain matching records in their existing order, without allocating.
    pub fn retain(&mut self, mut keep: impl FnMut(&UnknownField) -> bool) {
        if let Some(fields) = self.0.as_mut() {
            fields.retain(|field| keep(field));
        }
    }
}

impl<'a> IntoIterator for &'a FieldList {
    type Item = &'a UnknownField;
    type IntoIter = std::slice::Iter<'a, UnknownField>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnknownFields {
    pub fields: FieldList,
}

impl UnknownFields {
    pub fn clear(&mut self) {
        self.fields.clear();
    }

    pub fn encoded_len(&self) -> u64 {
        self.fields.iter().map(UnknownField::encoded_len).sum()
    }

    // Always inlined: the empty check folds into the caller and the
    // cold loop body stays out-of-line, saving a call on every encode
    // (PK-22).
    #[inline(always)]
    pub fn encode(&self, out: &mut impl WireOut) {
        for f in self.fields.iter() {
            f.encode(out);
        }
    }
}

impl UnknownField {
    fn encoded_len(&self) -> u64 {
        match self {
            Self::Varint { number, value } => tag_len(*number, WIRE_VARINT) + varint_len(*value),
            Self::Fixed64 { number, .. } => tag_len(*number, WIRE_I64) + 8,
            Self::Fixed32 { number, .. } => tag_len(*number, WIRE_I32) + 4,
            Self::LengthDelimited { number, value } => {
                tag_len(*number, WIRE_LEN) + varint_len(value.len() as u64) + value.len() as u64
            }
            Self::Group { number, fields } => {
                tag_len(*number, WIRE_SGROUP) + fields.encoded_len() + tag_len(*number, WIRE_EGROUP)
            }
        }
    }

    fn encode(&self, out: &mut impl WireOut) {
        match self {
            Self::Varint { number, value } => {
                encode_tag(out, *number, WIRE_VARINT);
                encode_varint(out, *value);
            }
            Self::Fixed64 { number, value } => {
                encode_tag(out, *number, WIRE_I64);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::Fixed32 { number, value } => {
                encode_tag(out, *number, WIRE_I32);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::LengthDelimited { number, value } => {
                encode_len_field(out, *number, value);
            }
            Self::Group { number, fields } => {
                encode_tag(out, *number, WIRE_SGROUP);
                fields.encode(out);
                encode_tag(out, *number, WIRE_EGROUP);
            }
        }
    }
}

#[inline(always)]
pub fn varint_len(mut value: u64) -> u64 {
    let mut n = 1;
    while value >= 0x80 {
        value >>= 7;
        n += 1;
    }
    n
}

/// Continuation-bit mask of a 16-byte varint run (PK-04).
///
/// Bit `i` is set iff `chunk[i]` has the varint continuation bit
/// (`byte >= 0x80`). NEON (`aarch64`) and SSE2 (`x86_64`) builds compute it
/// with one unaligned vector load plus a SIMD reduction; every other target
/// uses a scalar byte loop with identical results. SSE2 is part of the
/// x86-64 baseline and ASIMD of aarch64, so no runtime feature check is
/// needed. Shared by the two bulk loops below, which in turn serve the
/// generated inline path and the table engine through `validate_varints`
/// and `PackedCodec::decode`.
#[inline(always)]
fn cont_mask16(chunk: &[u8; 16]) -> u16 {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: `vld1q_u8` reads exactly the 16 bytes behind the shared
        // reference (no alignment requirement, no over-read); the remaining
        // intrinsics operate on register values only, so aliasing,
        // out-of-bounds access, and use-after-free are impossible. The
        // weights vector is a compile-time constant transmuted to a register
        // (same size and alignment; every bit pattern is a valid vector).
        // See docs/unsafe-invariants.md §11.
        unsafe {
            let v = vld1q_u8(chunk.as_ptr());
            // 0xFF per lane whose byte is >= 0x80 (negative as i8).
            let hi = vcltq_s8(vreinterpretq_s8_u8(v), vdupq_n_s8(0));
            let weights: uint8x16_t =
                std::mem::transmute([1u8, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128]);
            let m = vandq_u8(hi, weights);
            u16::from(vaddv_u8(vget_low_u8(m))) | (u16::from(vaddv_u8(vget_high_u8(m))) << 8)
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: `_mm_loadu_si128` reads exactly the 16 bytes behind the
        // shared reference (explicitly unaligned-safe, no over-read);
        // `_mm_movemask_epi8` only packs register sign bits, so aliasing,
        // out-of-bounds access, and use-after-free are impossible.
        // See docs/unsafe-invariants.md §11.
        unsafe {
            let v = _mm_loadu_si128(chunk.as_ptr() as *const __m128i);
            _mm_movemask_epi8(v) as u16
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let mut mask = 0u16;
        for (i, b) in chunk.iter().enumerate() {
            if *b >= 0x80 {
                mask |= 1 << i;
            }
        }
        mask
    }
}

/// `cont_mask16` pattern of eight consecutive 2-byte varints: bytes
/// 0,2,...,14 carry a continuation bit and bytes 1,3,...,15 terminate.
const VARINT_PAIRS_MASK16: u16 = 0x5555;

/// Packed-varint well-formedness without materializing values.
///
/// Buffers of 16+ bytes run the outlined SIMD classifier (PK-04) first;
/// short buffers and chunk tails use the scalar loop. The shell stays
/// small so callers inline it exactly as before the SIMD work.
#[inline]
pub fn validate_varints(buf: &[u8]) -> Result<(), ParseError> {
    let mut i = 0;
    let n = buf.len();
    if n >= 16 {
        i = validate_bulk_16(buf, 0)?;
    }
    while i < n {
        i = validate_one_varint(buf, i, n)?;
    }
    Ok(())
}

/// SIMD bulk validation over the 16-byte-chunk portion of `buf` starting
/// at `i`, returning the offset where fewer than 16 bytes remain. Each
/// chunk is sixteen 1-byte varints, eight 2-byte varints (both
/// unconditionally valid — overflow needs 10 bytes), a leading run of
/// 1-byte varints, or one scalar varint. Skipped bytes are provably valid,
/// so the first error reported is identical to the scalar loop.
/// Out-of-line by design: one call per field keeps the `validate_varints`
/// shell (and its inlining) unchanged for short buffers.
#[inline(never)]
fn validate_bulk_16(buf: &[u8], mut i: usize) -> Result<usize, ParseError> {
    let n = buf.len();
    while n - i >= 16 {
        let chunk = buf[i..].first_chunk::<16>().unwrap();
        let mask = cont_mask16(chunk);
        if mask == 0 {
            i += 16;
            continue;
        }
        if mask == VARINT_PAIRS_MASK16 {
            i += 16;
            continue;
        }
        // Mixed chunk: skip the leading 1-byte run (`trailing_zeros` < 16
        // because the mask is nonzero), else decode one scalar varint below.
        let singles = mask.trailing_zeros() as usize;
        if singles > 0 {
            i += singles;
            continue;
        }
        i = validate_one_varint(buf, i, n)?;
    }
    Ok(i)
}

/// SIMD bulk decode of the 16-byte-chunk portion of a packed varint
/// payload, appending through `conv` and returning the offset where fewer
/// than 16 bytes remain. Values use the same arithmetic as the scalar 1-
/// and 2-byte fast paths, and the first invalid varint is always handled
/// by `decode_varint` at the identical offset, so status, values, and the
/// partial prefix match the scalar loop exactly. Shared by every packed
/// varint/bool codec (generated inline path and table engine alike).
/// Out-of-line by design: one call per field keeps the `decode` shells
/// (and their inlining) unchanged for short buffers.
#[inline(never)]
pub(crate) fn bulk_decode_varints<T, F>(
    buf: &[u8],
    mut i: usize,
    out: &mut Vec<T>,
    conv: F,
) -> Result<usize, ParseError>
where
    F: Fn(u64) -> T,
{
    let n = buf.len();
    while n - i >= 16 {
        let chunk = buf[i..].first_chunk::<16>().unwrap();
        let mask = cont_mask16(chunk);
        if mask == 0 {
            out.extend(chunk.iter().map(|b| conv(u64::from(*b))));
            i += 16;
            continue;
        }
        if mask == VARINT_PAIRS_MASK16 {
            out.extend(
                chunk
                    .chunks_exact(2)
                    .map(|p| conv(u64::from(p[0] & 0x7f) | (u64::from(p[1]) << 7))),
            );
            i += 16;
            continue;
        }
        let singles = mask.trailing_zeros() as usize;
        if singles > 0 {
            out.extend(chunk[..singles].iter().map(|b| conv(u64::from(*b))));
            i += singles;
            continue;
        }
        out.push(conv(decode_varint(buf, &mut i)?));
    }
    Ok(i)
}

/// Validate the single varint starting at `i` (`i < n`), returning the
/// offset past it. Scalar step shared by the SIMD loop and the tail.
#[inline(always)]
fn validate_one_varint(buf: &[u8], mut i: usize, n: usize) -> Result<usize, ParseError> {
    let b = buf[i];
    i += 1;
    if b < 0x80 {
        return Ok(i);
    }
    let mut cnt = 1u32;
    loop {
        if i >= n {
            return Err(ParseError::new("truncated varint"));
        }
        let c = buf[i];
        i += 1;
        cnt += 1;
        if c < 0x80 {
            if cnt == 10 && c > 1 {
                return Err(ParseError::new("varint overflow"));
            }
            return Ok(i);
        }
        if cnt >= 10 {
            return Err(ParseError::new("varint overflow"));
        }
    }
}

pub fn decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, ParseError> {
    let start = *pos;
    let rest = &buf[start..];
    if let Some(&b0) = rest.first() {
        if b0 < 0x80 {
            *pos = start + 1;
            return Ok(u64::from(b0));
        }
        if rest.len() >= 2 && rest[1] < 0x80 {
            *pos = start + 2;
            return Ok(u64::from(b0 & 0x7f) | (u64::from(rest[1]) << 7));
        }
    }
    let mut result = 0u64;
    let mut shift = 0;
    for i in 0..10 {
        if *pos >= buf.len() {
            return Err(ParseError::new("truncated varint"));
        }
        let byte = buf[*pos];
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err(ParseError::new("varint overflow"));
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(ParseError::new("varint overflow"))
}

#[inline(always)]
pub fn encode_varint(out: &mut impl WireOut, value: u64) {
    // One- and two-byte fast paths stay inline: tags, small values, and
    // small lengths dominate encode traffic. The two-byte form emits via
    // a single `put_slice` (fixed size, nothing to unroll). The general
    // loop would unroll into hundreds of bytes at each call site and
    // bloat small `write_to` bodies, so lengths 3+ live out-of-line
    // (PK-22).
    if value < 0x80 {
        out.push(value as u8);
    } else if value < 0x4000 {
        out.extend_from_slice(&[(value as u8) | 0x80, (value >> 7) as u8]);
    } else {
        encode_varint_slow(out, value);
    }
}

/// Varint encode for lengths 3+. Cold and never inlined: keeps the hot
/// one- and two-byte paths (and their callers) small. Byte-identical to
/// the inlined loop, including the single `put_slice` emission.
#[cold]
#[inline(never)]
fn encode_varint_slow(out: &mut impl WireOut, mut value: u64) {
    debug_assert!(value >= 0x4000);
    let mut buf = [0u8; 10];
    let mut i = 0;
    while value >= 0x80 {
        buf[i] = (value as u8) | 0x80;
        value >>= 7;
        i += 1;
    }
    buf[i] = value as u8;
    out.extend_from_slice(&buf[..=i]);
}

#[inline(always)]
pub fn encode_tag(out: &mut impl WireOut, number: u32, wire: u32) {
    encode_varint(out, u64::from((number << 3) | wire));
}

pub fn tag_len(number: u32, wire: u32) -> u64 {
    varint_len(u64::from((number << 3) | wire))
}

#[inline(always)]
pub fn decode_tag(buf: &[u8], pos: &mut usize) -> Result<(u32, u32), ParseError> {
    if let Some(&b) = buf.get(*pos) {
        if b < 0x80 {
            *pos += 1;
            let wire = u32::from(b & 7);
            let number = u32::from(b >> 3);
            if number == 0 {
                return Err(ParseError::new("illegal field number"));
            }
            return Ok((number, wire));
        }
    }
    let start = *pos;
    let tag = decode_varint(buf, pos)?;
    if *pos - start != varint_len(tag) as usize {
        return Err(ParseError::new("overlong tag varint"));
    }
    if tag > u64::from(u32::MAX) {
        return Err(ParseError::new("tag overflow"));
    }
    let wire = (tag & 7) as u32;
    let number = (tag >> 3) as u32;
    if number == 0 || number > MAX_FIELD_NUMBER {
        return Err(ParseError::new("illegal field number"));
    }
    Ok((number, wire))
}

#[inline(always)]
pub fn encode_len_header(out: &mut impl WireOut, number: u32, len: u64) {
    encode_tag(out, number, WIRE_LEN);
    encode_varint(out, len);
}

#[inline(always)]
pub fn encode_len_field(out: &mut impl WireOut, number: u32, payload: &[u8]) {
    encode_len_header(out, number, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// Minimum bytes-field size eligible for zero-copy send (PK-11 `T_send`).
///
/// Below this a field is copied inline: a segment would cost an extra h2
/// DATA frame and bookkeeping while saving only a small `memcpy`. At or
/// above it, generated bytes-field encode offers the field's buffer to the
/// sink via [`WireOut::put_shared`].
pub const SHARED_SEND_THRESHOLD: usize = 32 * 1024;

/// Encode a bytes field, sharing its buffer with a segmented sink when free.
///
/// Writes the tag and length prefix, then: if the field holds at least
/// [`SHARED_SEND_THRESHOLD`] bytes obtainable without a copy, hands the
/// buffer to `out.put_shared` (a segmented sink retains it; every other
/// sink copies through the default). Otherwise copies inline as before.
/// Byte-identical output either way.
#[inline(always)]
pub fn encode_len_field_shared(
    out: &mut impl WireOut,
    number: u32,
    field: &crate::lazy::LazyBytes,
) {
    let len = field.len();
    encode_len_header(out, number, len as u64);
    if len >= SHARED_SEND_THRESHOLD && out.supports_shared() {
        if let Some(shared) = field.shared_bytes() {
            out.put_shared(&shared);
            return;
        }
    }
    out.extend_from_slice(field.as_bytes());
}

pub fn encode_zigzag32(n: i32) -> u64 {
    ((n << 1) ^ (n >> 31)) as u32 as u64
}

pub fn encode_zigzag64(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

pub fn decode_zigzag32(n: u64) -> i32 {
    let n = n as u32;
    ((n >> 1) as i32) ^ -((n & 1) as i32)
}

pub fn decode_zigzag64(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

pub fn skip_field(buf: &[u8], pos: &mut usize, wire: u32) -> Result<(), ParseError> {
    match wire {
        WIRE_VARINT => {
            decode_varint(buf, pos)?;
        }
        WIRE_I64 => {
            if *pos + 8 > buf.len() {
                return Err(ParseError::new("truncated fixed64"));
            }
            *pos += 8;
        }
        WIRE_I32 => {
            if *pos + 4 > buf.len() {
                return Err(ParseError::new("truncated fixed32"));
            }
            *pos += 4;
        }
        WIRE_LEN => {
            let len = decode_len(buf, pos)?;
            *pos += len;
        }
        WIRE_SGROUP => skip_group_with_depth(buf, pos, 0)?,
        WIRE_EGROUP => return Err(ParseError::new("unexpected end-group")),
        _ => return Err(ParseError::new("unknown wire type")),
    }
    Ok(())
}

/// Skip one field, counting unknown groups with its enclosing parse depth.
///
/// Root depth is zero. Each group consumes one level of [`crate::RECURSION_LIMIT`];
/// non-group fields retain [`skip_field`]'s behavior without a depth check.
/// Like `skip_field`, this helper has no expected outer field number and does
/// not validate matching end-group numbers.
pub fn skip_field_with_depth(
    buf: &[u8],
    pos: &mut usize,
    wire: u32,
    depth: u32,
) -> Result<(), ParseError> {
    if wire == WIRE_SGROUP {
        skip_group_with_depth(buf, pos, depth)
    } else {
        skip_field(buf, pos, wire)
    }
}

#[cold]
fn skip_group_with_depth(buf: &[u8], pos: &mut usize, depth: u32) -> Result<(), ParseError> {
    if depth >= crate::RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    let group_depth = depth + 1;
    loop {
        let (_, inner) = decode_tag(buf, pos)?;
        if inner == WIRE_EGROUP {
            return Ok(());
        }
        skip_field_with_depth(buf, pos, inner, group_depth)?;
    }
}

pub fn read_len_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], ParseError> {
    let (start, end) = read_len_span(buf, pos)?;
    Ok(&buf[start..end])
}

/// Length-delimited prefix decoded and bounds-checked against the remaining
/// buffer. Lengths that exceed the remaining bytes — including varints that
/// would overflow `usize` arithmetic — are errors, never panics.
fn decode_len(buf: &[u8], pos: &mut usize) -> Result<usize, ParseError> {
    let raw = decode_varint(buf, pos)?;
    let Ok(len) = usize::try_from(raw) else {
        return Err(ParseError::new("length exceeds addressable memory"));
    };
    if len > buf.len().saturating_sub(*pos) {
        return Err(ParseError::new("truncated length-delimited"));
    }
    Ok(len)
}

/// Length-delimited payload as `start..end` indices into `buf` (after the length varint).
pub fn read_len_span(buf: &[u8], pos: &mut usize) -> Result<(usize, usize), ParseError> {
    let len = decode_len(buf, pos)?;
    let start = *pos;
    *pos += len;
    Ok((start, *pos))
}

pub fn read_fixed32(buf: &[u8], pos: &mut usize) -> Result<u32, ParseError> {
    if *pos + 4 > buf.len() {
        return Err(ParseError::new("truncated fixed32"));
    }
    let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

pub fn read_fixed64(buf: &[u8], pos: &mut usize) -> Result<u64, ParseError> {
    if *pos + 8 > buf.len() {
        return Err(ParseError::new("truncated fixed64"));
    }
    let v = u64::from_le_bytes(buf[*pos..*pos + 8].try_into().unwrap());
    *pos += 8;
    Ok(v)
}

pub fn capture_unknown(
    buf: &[u8],
    pos: &mut usize,
    number: u32,
    wire: u32,
) -> Result<UnknownField, ParseError> {
    match wire {
        WIRE_VARINT => Ok(UnknownField::Varint {
            number,
            value: decode_varint(buf, pos)?,
        }),
        WIRE_I64 => Ok(UnknownField::Fixed64 {
            number,
            value: read_fixed64(buf, pos)?,
        }),
        WIRE_I32 => Ok(UnknownField::Fixed32 {
            number,
            value: read_fixed32(buf, pos)?,
        }),
        WIRE_LEN => Ok(UnknownField::LengthDelimited {
            number,
            value: read_len_bytes(buf, pos)?.to_vec(),
        }),
        WIRE_SGROUP => capture_group_with_depth(buf, pos, number, 0),
        _ => Err(ParseError::new("unknown wire type")),
    }
}

/// Capture an unknown field, counting groups with its enclosing parse depth.
///
/// Root depth is zero. Each group consumes one level of [`crate::RECURSION_LIMIT`];
/// excessive depth is rejected before group storage or an increment is created.
/// Non-group fields retain [`capture_unknown`]'s behavior without a depth check.
/// Matching end-group numbers continue to be checked.
pub fn capture_unknown_with_depth(
    buf: &[u8],
    pos: &mut usize,
    number: u32,
    wire: u32,
    depth: u32,
) -> Result<UnknownField, ParseError> {
    if wire == WIRE_SGROUP {
        capture_group_with_depth(buf, pos, number, depth)
    } else {
        capture_unknown(buf, pos, number, wire)
    }
}

#[cold]
fn capture_group_with_depth(
    buf: &[u8],
    pos: &mut usize,
    number: u32,
    depth: u32,
) -> Result<UnknownField, ParseError> {
    if depth >= crate::RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    let group_depth = depth + 1;
    let mut fields = UnknownFields::default();
    loop {
        let (n, w) = decode_tag(buf, pos)?;
        if w == WIRE_EGROUP {
            if n != number {
                return Err(ParseError::new("mismatched end-group"));
            }
            return Ok(UnknownField::Group { number, fields });
        }
        fields
            .fields
            .push(capture_unknown_with_depth(buf, pos, n, w, group_depth)?);
    }
}

pub fn check_size(len: u64) -> Result<u32, crate::error::SerializeError> {
    if len > MAX_MESSAGE_BYTES {
        return Err(crate::error::SerializeError::new(
            "encoded message exceeds 2 GiB",
        ));
    }
    Ok(len as u32)
}

pub fn key_len_value_len(number: u32, payload_len: u64) -> u64 {
    tag_len(number, WIRE_LEN) + varint_len(payload_len) + payload_len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimized fuzzer crash: the length varint exceeds the remaining
    /// buffer by far (`fuzz/corpus/wire/len_overflow_min.bin`).
    const LEN_OVERFLOW: &[u8] = &[0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];

    #[test]
    fn max_field_number_matches_spec_bound() {
        assert_eq!(MAX_FIELD_NUMBER, (1u32 << 29) - 1);
        let mut buf = Vec::new();
        encode_tag(&mut buf, MAX_FIELD_NUMBER, WIRE_VARINT);
        buf.push(0);
        let mut pos = 0;
        assert_eq!(
            decode_tag(&buf, &mut pos),
            Ok((MAX_FIELD_NUMBER, WIRE_VARINT))
        );
        let mut over = Vec::new();
        encode_varint(&mut over, u64::from(MAX_FIELD_NUMBER + 1) << 3);
        let mut pos = 0;
        assert!(decode_tag(&over, &mut pos).is_err());
    }

    #[test]
    fn huge_length_span_is_error_not_panic() {
        let mut pos = 1usize;
        assert!(read_len_span(LEN_OVERFLOW, &mut pos).is_err());
    }

    #[test]
    fn huge_length_skip_is_error_not_panic() {
        let mut pos = 1usize;
        assert!(skip_field(LEN_OVERFLOW, &mut pos, WIRE_LEN).is_err());
    }

    #[test]
    fn max_u64_length_is_error_not_panic() {
        let mut buf = vec![0x0au8];
        buf.extend_from_slice(&[0xff; 9]);
        buf.push(0x01);
        let mut pos = 1usize;
        assert!(read_len_span(&buf, &mut pos).is_err());
        let mut pos = 1usize;
        assert!(skip_field(&buf, &mut pos, WIRE_LEN).is_err());
    }

    fn validate_varints_reference(buf: &[u8]) -> Result<(), ParseError> {
        let mut i = 0;
        while i < buf.len() {
            let mut cnt = 0u32;
            loop {
                if i >= buf.len() {
                    return Err(ParseError::new("truncated varint"));
                }
                let byte = buf[i];
                i += 1;
                cnt += 1;
                if byte < 0x80 {
                    if cnt == 10 && byte > 1 {
                        return Err(ParseError::new("varint overflow"));
                    }
                    break;
                }
                if cnt >= 10 {
                    return Err(ParseError::new("varint overflow"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn validate_varints_matches_scalar_reference() {
        let cases: &[&[u8]] = &[
            b"",
            &[0],
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9],
            &[0x80, 0x01],
            &[0xff, 0x01, 0x00, 0x7f],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            &[0x80],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
            ],
        ];
        for case in cases {
            assert_eq!(
                validate_varints(case).is_ok(),
                validate_varints_reference(case).is_ok(),
                "case {case:?}"
            );
        }
        let mut many_single = Vec::new();
        for i in 0..255 {
            encode_varint(&mut many_single, i);
        }
        assert_eq!(
            validate_varints(&many_single).is_ok(),
            validate_varints_reference(&many_single).is_ok()
        );
    }

    /// Recording sink: counts `put_shared` vs inline writes.
    struct Recorder {
        bytes: Vec<u8>,
        shared_calls: u32,
        shared_bytes: usize,
    }

    impl WireOut for Recorder {
        fn put_u8(&mut self, b: u8) {
            self.bytes.push(b);
        }

        fn put_slice(&mut self, data: &[u8]) {
            self.bytes.extend_from_slice(data);
        }

        fn put_shared(&mut self, b: &Bytes) {
            self.shared_calls += 1;
            self.shared_bytes += b.len();
            self.bytes.extend_from_slice(b);
        }

        fn supports_shared(&self) -> bool {
            true
        }
    }

    #[test]
    fn put_shared_default_copies() {
        // The blanket `BufMut` impl keeps the default: shared == inline.
        let mut buf = Vec::new();
        let b = Bytes::from_static(b"hello");
        buf.put_shared(&b);
        assert_eq!(buf, b"hello");
    }

    #[test]
    fn shared_field_routes_by_threshold_and_backing() {
        use crate::lazy::LazyBytes;
        use crate::string::ProtoBytes;

        // Big owned field: shared, no inline copy of the payload.
        let big = LazyBytes::owned(ProtoBytes::from(vec![7u8; SHARED_SEND_THRESHOLD + 1]));
        let mut rec = Recorder {
            bytes: Vec::new(),
            shared_calls: 0,
            shared_bytes: 0,
        };
        encode_len_field_shared(&mut rec, 2, &big);
        assert_eq!(rec.shared_calls, 1);
        assert_eq!(rec.shared_bytes, SHARED_SEND_THRESHOLD + 1);
        let mut expect = Vec::new();
        encode_len_field(&mut expect, 2, &vec![7u8; SHARED_SEND_THRESHOLD + 1]);
        assert_eq!(rec.bytes, expect);

        // Small field: inline, never shared.
        let small = LazyBytes::owned(ProtoBytes::from(vec![7u8; 16]));
        let mut rec = Recorder {
            bytes: Vec::new(),
            shared_calls: 0,
            shared_bytes: 0,
        };
        encode_len_field_shared(&mut rec, 2, &small);
        assert_eq!(rec.shared_calls, 0);
        let mut expect = Vec::new();
        encode_len_field(&mut expect, 2, &[7u8; 16]);
        assert_eq!(rec.bytes, expect);

        // Big privately-parsed field: sharing would copy anyway, so inline.
        let mut slot = None;
        let wire_data = vec![9u8; SHARED_SEND_THRESHOLD + 8];
        let private = LazyBytes::from_parse_span(&mut slot, &wire_data, 0, wire_data.len());
        assert!(private.shared_bytes().is_none());
        let mut rec = Recorder {
            bytes: Vec::new(),
            shared_calls: 0,
            shared_bytes: 0,
        };
        encode_len_field_shared(&mut rec, 2, &private);
        assert_eq!(rec.shared_calls, 0);
        let mut expect = Vec::new();
        encode_len_field(&mut expect, 2, &wire_data);
        assert_eq!(rec.bytes, expect);

        // Big shared-parse field: shared.
        let frame = Bytes::from(vec![3u8; SHARED_SEND_THRESHOLD + 8]);
        let mut slot = Some(crate::lazy::Wire::from_bytes(frame.clone()));
        let shared = LazyBytes::from_parse_span(&mut slot, &frame, 0, SHARED_SEND_THRESHOLD + 8);
        assert!(shared.shared_bytes().is_some());
        let mut rec = Recorder {
            bytes: Vec::new(),
            shared_calls: 0,
            shared_bytes: 0,
        };
        encode_len_field_shared(&mut rec, 2, &shared);
        assert_eq!(rec.shared_calls, 1);
    }

    // --- PK-04 SIMD varint differential tests ---

    /// Scalar reference for `cont_mask16` (the fallback-target algorithm).
    fn scalar_cont_mask16(chunk: &[u8; 16]) -> u16 {
        let mut mask = 0u16;
        for (i, b) in chunk.iter().enumerate() {
            if *b >= 0x80 {
                mask |= 1 << i;
            }
        }
        mask
    }

    #[test]
    fn simd_cont_mask_matches_scalar() {
        assert_eq!(cont_mask16(&[0u8; 16]), 0);
        assert_eq!(cont_mask16(&[0xffu8; 16]), 0xffff);
        assert_eq!(
            cont_mask16(&[0x80, 0, 0x81, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
            0x0005
        );
        let mut pairs = [0u8; 16];
        for i in 0..8 {
            pairs[2 * i] = 0x80 | i as u8;
            pairs[2 * i + 1] = i as u8;
        }
        assert_eq!(cont_mask16(&pairs), VARINT_PAIRS_MASK16);
        // Deterministic sweep against the scalar loop (short under Miri:
        // each intrinsic step is interpreted).
        let mut rng = CampaignRng(0x9e37_79b9_7f4a_7c15);
        let iters = if cfg!(miri) { 500 } else { 200_000 };
        for _ in 0..iters {
            let mut chunk = [0u8; 16];
            for b in chunk.iter_mut() {
                *b = rng.next_byte();
            }
            assert_eq!(cont_mask16(&chunk), scalar_cont_mask16(&chunk));
        }
    }

    /// Independent scalar varint reference (never calls crate decoders).
    fn ref_decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, &'static str> {
        let mut result = 0u64;
        let mut shift = 0;
        for i in 0..10 {
            if *pos >= buf.len() {
                return Err("truncated");
            }
            let byte = buf[*pos];
            *pos += 1;
            if i == 9 && byte > 1 {
                return Err("overflow");
            }
            result |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(result);
            }
            shift += 7;
        }
        Err("overflow")
    }

    fn ref_varint_len(mut value: u64) -> usize {
        let mut n = 1;
        while value >= 0x80 {
            value >>= 7;
            n += 1;
        }
        n
    }

    fn ref_decode_tag(buf: &[u8], pos: &mut usize) -> Result<(u32, u32), &'static str> {
        let start = *pos;
        let tag = ref_decode_varint(buf, pos)?;
        if *pos - start != ref_varint_len(tag) {
            return Err("overlong tag");
        }
        if tag > u64::from(u32::MAX) {
            return Err("tag overflow");
        }
        let wire = (tag & 7) as u32;
        let number = (tag >> 3) as u32;
        if number == 0 || number > MAX_FIELD_NUMBER {
            return Err("illegal field number");
        }
        Ok((number, wire))
    }

    /// Reference packed-payload decode: values so far plus final status.
    fn ref_decode_all(buf: &[u8]) -> (Vec<u64>, Result<(), &'static str>) {
        let mut pos = 0;
        let mut out = Vec::new();
        while pos < buf.len() {
            match ref_decode_varint(buf, &mut pos) {
                Ok(v) => out.push(v),
                Err(e) => return (out, Err(e)),
            }
        }
        (out, Ok(()))
    }

    fn ref_zigzag32(n: u64) -> i32 {
        let n = n as u32;
        ((n >> 1) as i32) ^ -((n & 1) as i32)
    }

    fn ref_zigzag64(n: u64) -> i64 {
        ((n >> 1) as i64) ^ -((n & 1) as i64)
    }

    fn check_codec<C, F>(data: &[u8], values: &[u64], ok: bool, conv: F)
    where
        C: crate::packed::PackedCodec,
        C::Elem: std::fmt::Debug,
        F: Fn(u64) -> C::Elem,
    {
        let mut out = Vec::new();
        let status = C::decode(data, &mut out);
        assert_eq!(
            status.is_ok(),
            ok,
            "{} status {data:?}",
            std::any::type_name::<C>()
        );
        let expect: Vec<C::Elem> = values.iter().map(|x| conv(*x)).collect();
        assert_eq!(
            out,
            expect,
            "{} values {data:?}",
            std::any::type_name::<C>()
        );
    }

    /// Full differential check of one input: single varint/tag (status,
    /// position, value), packed validation (status), and every varint
    /// codec decode (status plus the full value vector, including the
    /// partial prefix when decoding fails partway).
    fn check_one_varint_input(data: &[u8]) {
        use crate::packed::{
            Bools, PackedCodec, VarintI32, VarintI64, VarintU32, VarintU64, ZigZag32, ZigZag64,
        };

        let mut fast_pos = 0;
        let mut ref_pos = 0;
        let fast = decode_varint(data, &mut fast_pos).map_err(|_| ());
        let reference = ref_decode_varint(data, &mut ref_pos).map_err(|_| ());
        assert_eq!(
            fast.is_ok(),
            reference.is_ok(),
            "decode_varint status {data:?}"
        );
        assert_eq!(fast_pos, ref_pos, "decode_varint position {data:?}");
        if let (Ok(fast), Ok(reference)) = (fast, reference) {
            assert_eq!(fast, reference, "decode_varint value {data:?}");
        }

        let mut fast_pos = 0;
        let mut ref_pos = 0;
        let fast = decode_tag(data, &mut fast_pos).map_err(|_| ());
        let reference = ref_decode_tag(data, &mut ref_pos).map_err(|_| ());
        assert_eq!(
            fast.is_ok(),
            reference.is_ok(),
            "decode_tag status {data:?}"
        );
        assert_eq!(fast_pos, ref_pos, "decode_tag position {data:?}");
        if let (Ok(fast), Ok(reference)) = (fast, reference) {
            assert_eq!(fast, reference, "decode_tag value {data:?}");
        }

        let (values, status) = ref_decode_all(data);
        let ok = status.is_ok();
        assert_eq!(
            validate_varints(data).is_ok(),
            ok,
            "validate_varints {data:?}"
        );
        assert_eq!(
            <VarintU64 as PackedCodec>::validate(data).is_ok(),
            ok,
            "packed validate {data:?}"
        );

        check_codec::<VarintU64, _>(data, &values, ok, |v| v);
        check_codec::<VarintU32, _>(data, &values, ok, |v| v as u32);
        check_codec::<VarintI32, _>(data, &values, ok, |v| v as i32);
        check_codec::<VarintI64, _>(data, &values, ok, |v| v as i64);
        check_codec::<ZigZag32, _>(data, &values, ok, ref_zigzag32);
        check_codec::<ZigZag64, _>(data, &values, ok, ref_zigzag64);
        check_codec::<Bools, _>(data, &values, ok, |v| v != 0);
    }

    #[test]
    fn simd_chunk_edges_match_reference() {
        let singles = |n: usize| vec![0x7fu8; n];
        let pairs = |n: usize| {
            let mut v = Vec::new();
            for i in 0..n {
                v.push(0x80 | (i as u8 & 0x7f));
                v.push(i as u8 & 0x7f);
            }
            v
        };
        let overlong_pairs = |n: usize| {
            let mut v = Vec::new();
            for _ in 0..n {
                v.push(0x80);
                v.push(0x00);
            }
            v
        };
        let max10: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
        let over10: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];
        let over11: &[u8] = &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
        ];
        let trunc1: &[u8] = &[0x80];
        let trunc2: &[u8] = &[0x80, 0x80];

        let mut cases: Vec<Vec<u8>> = Vec::new();
        for n in [15usize, 16, 17, 31, 32, 33, 48, 64] {
            cases.push(singles(n));
        }
        cases.push(pairs(8));
        cases.push(pairs(16));
        cases.push(pairs(20));
        // Pair split across the 16-byte chunk boundary.
        let mut split = singles(15);
        split.extend_from_slice(&pairs(2));
        cases.push(split);
        let mut split2 = singles(7);
        split2.extend_from_slice(&pairs(6));
        cases.push(split2);
        cases.push(overlong_pairs(16));
        // Error tails after full SIMD chunks, plus an embedded error.
        for prefix in [singles(16), pairs(8), singles(32)] {
            for tail in [max10, over10, over11, trunc1, trunc2] {
                let mut v = prefix.clone();
                v.extend_from_slice(tail);
                cases.push(v);
            }
            let mut v = prefix.clone();
            v.extend_from_slice(over10);
            v.extend_from_slice(&singles(20));
            cases.push(v);
        }
        // Alternating single/double: every chunk mixed.
        let mut alt = Vec::new();
        for i in 0..20u8 {
            alt.push(i);
            alt.push(0x80 | i);
            alt.push(i);
        }
        cases.push(alt);
        // Packed-256-shaped payload: 128 singles then 128 doubles.
        let mut p256 = singles(128);
        p256.extend_from_slice(&pairs(128));
        cases.push(p256);

        for c in &cases {
            check_one_varint_input(c);
        }
        assert!(!cases.is_empty());
    }

    /// Deterministic xorshift64 (fixed seed: the campaign is reproducible).
    struct CampaignRng(u64);

    impl CampaignRng {
        fn next_u64(&mut self) -> u64 {
            let mut s = self.0;
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            self.0 = s;
            s
        }

        fn next_byte(&mut self) -> u8 {
            (self.next_u64() >> 11) as u8
        }

        fn next_below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }

        /// Lengths biased at SIMD chunk boundaries (15/16/17 + 16k).
        fn next_len(&mut self) -> usize {
            match self.next_below(8) {
                0..=3 => self.next_below(96),
                4 => 15 + 16 * self.next_below(4),
                5 => 16 + 16 * self.next_below(4),
                6 => 17 + 16 * self.next_below(4),
                _ => self.next_below(8),
            }
        }

        /// Fill `out` with one shape: uniform bytes, single-byte runs,
        /// 2-byte pair runs, concatenated canonical encodings (maybe
        /// truncated), or a structured edge fragment in random context.
        fn fill(&mut self, out: &mut Vec<u8>) {
            out.clear();
            let len = self.next_len();
            out.reserve(len);
            match self.next_below(5) {
                0 => {
                    for _ in 0..len {
                        out.push(self.next_byte());
                    }
                }
                1 => {
                    for _ in 0..len {
                        let b = self.next_byte();
                        out.push(if self.next_below(8) == 0 { b } else { b & 0x7f });
                    }
                }
                2 => {
                    let mut i = 0;
                    while i < len {
                        if self.next_below(8) == 0 || i + 1 >= len {
                            out.push(self.next_byte());
                            i += 1;
                        } else {
                            out.push(self.next_byte() | 0x80);
                            out.push(self.next_byte() & 0x7f);
                            i += 2;
                        }
                    }
                }
                3 => {
                    while out.len() < len {
                        let v = self.next_u64() >> self.next_below(64);
                        let mut tmp = Vec::new();
                        encode_varint(&mut tmp, v);
                        out.extend_from_slice(&tmp);
                    }
                    out.truncate(len);
                }
                _ => {
                    const FRAGS: &[&[u8]] = &[
                        &[0x80, 0x00],
                        &[0xff, 0xff, 0xff, 0xff, 0x0f],
                        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
                        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
                        &[0x80],
                        &[
                            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
                        ],
                        &[
                            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01,
                        ],
                    ];
                    let frag = FRAGS[self.next_below(FRAGS.len())];
                    let at = if len == 0 {
                        0
                    } else {
                        self.next_below(len + 1)
                    };
                    for _ in 0..at.min(len) {
                        out.push(self.next_byte());
                    }
                    for b in frag.iter().take(len.saturating_sub(out.len())) {
                        out.push(*b);
                    }
                    while out.len() < len {
                        out.push(self.next_byte());
                    }
                }
            }
        }
    }

    /// Bounded deterministic differential campaign: every input runs the
    /// full [`check_one_varint_input`]. `PBRS_VARINT_BULK_N` sets the PRNG
    /// input count (default 20k, fast for `cargo test`); large campaigns
    /// add the exhaustive 3-byte sweep. Prints the exact input count; a
    /// pass means zero divergence over all of them.
    #[test]
    fn bulk_varint_differential_campaign() {
        let prng_n: u64 = std::env::var("PBRS_VARINT_BULK_N")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20_000);
        // Short under Miri: the interpreter runs every intrinsic step.
        let (depth, prng_n) = if cfg!(miri) {
            (1, prng_n.min(200))
        } else {
            (2, prng_n)
        };
        let mut count = 0u64;
        let mut check = |data: &[u8]| {
            check_one_varint_input(data);
            count += 1;
        };
        check(b"");
        for a in 0..=255u16 {
            check(&[a as u8]);
            if depth >= 2 {
                for b in 0..=255u16 {
                    check(&[a as u8, b as u8]);
                }
            }
        }
        if !cfg!(miri) && prng_n >= 1_000_000 {
            for a in 0..=255u16 {
                for b in 0..=255u16 {
                    for c in 0..=255u16 {
                        check(&[a as u8, b as u8, c as u8]);
                    }
                }
            }
        }
        let mut rng = CampaignRng(0x243f_6a88_85a3_08d3);
        let mut buf = Vec::new();
        for _ in 0..prng_n {
            rng.fill(&mut buf);
            check(&buf);
        }
        println!("bulk varint differential: {count} inputs, no divergence");
    }
}
