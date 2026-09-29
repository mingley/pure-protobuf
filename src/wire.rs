//! Binary wire codec. No schema.
#![allow(
    clippy::unwrap_used,
    reason = "fixed32/64 try_into after a length check"
)]

use crate::error::ParseError;
use crate::internal::MAX_MESSAGE_BYTES;
use bytes::{BufMut, Bytes};

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

/// Packed-varint well-formedness without materializing values.
#[inline]
pub fn validate_varints(buf: &[u8]) -> Result<(), ParseError> {
    let mut i = 0;
    let n = buf.len();
    while i < n {
        let b = buf[i];
        i += 1;
        if b < 0x80 {
            continue;
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
                break;
            }
            if cnt >= 10 {
                return Err(ParseError::new("varint overflow"));
            }
        }
    }
    Ok(())
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
pub fn encode_varint(out: &mut impl WireOut, mut value: u64) {
    if value < 0x80 {
        out.push(value as u8);
        return;
    }
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
        WIRE_SGROUP => loop {
            let (_, inner) = decode_tag(buf, pos)?;
            if inner == WIRE_EGROUP {
                break;
            }
            skip_field(buf, pos, inner)?;
        },
        WIRE_EGROUP => return Err(ParseError::new("unexpected end-group")),
        _ => return Err(ParseError::new("unknown wire type")),
    }
    Ok(())
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
        WIRE_SGROUP => {
            let mut fields = UnknownFields::default();
            loop {
                let (n, w) = decode_tag(buf, pos)?;
                if w == WIRE_EGROUP {
                    if n != number {
                        return Err(ParseError::new("mismatched end-group"));
                    }
                    break;
                }
                fields.fields.push(capture_unknown(buf, pos, n, w)?);
            }
            Ok(UnknownField::Group { number, fields })
        }
        _ => Err(ParseError::new("unknown wire type")),
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
}
