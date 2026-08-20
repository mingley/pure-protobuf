//! Binary wire codec. No schema.

use crate::error::ParseError;
use crate::internal::MAX_MESSAGE_BYTES;

pub const WIRE_VARINT: u32 = 0;
pub const WIRE_I64: u32 = 1;
pub const WIRE_LEN: u32 = 2;
pub const WIRE_SGROUP: u32 = 3;
pub const WIRE_EGROUP: u32 = 4;
pub const WIRE_I32: u32 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownField {
    Varint { number: u32, value: u64 },
    Fixed64 { number: u32, value: u64 },
    LengthDelimited { number: u32, value: Vec<u8> },
    Group { number: u32, fields: UnknownFields },
    Fixed32 { number: u32, value: u32 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnknownFields {
    pub fields: Vec<UnknownField>,
}

impl UnknownFields {
    pub fn clear(&mut self) {
        self.fields.clear();
    }

    pub fn encoded_len(&self) -> u64 {
        self.fields.iter().map(UnknownField::encoded_len).sum()
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        for f in &self.fields {
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

    fn encode(&self, out: &mut Vec<u8>) {
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
pub fn encode_varint(out: &mut Vec<u8>, mut value: u64) {
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
pub fn encode_tag(out: &mut Vec<u8>, number: u32, wire: u32) {
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
    if number == 0 || number > 536_870_911 {
        return Err(ParseError::new("illegal field number"));
    }
    Ok((number, wire))
}

#[inline(always)]
pub fn encode_len_header(out: &mut Vec<u8>, number: u32, len: u64) {
    encode_tag(out, number, WIRE_LEN);
    encode_varint(out, len);
}

#[inline(always)]
pub fn encode_len_field(out: &mut Vec<u8>, number: u32, payload: &[u8]) {
    encode_len_header(out, number, payload.len() as u64);
    out.extend_from_slice(payload);
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
            let len = decode_varint(buf, pos)? as usize;
            if *pos + len > buf.len() {
                return Err(ParseError::new("truncated length-delimited"));
            }
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
    let len = decode_varint(buf, pos)? as usize;
    if *pos + len > buf.len() {
        return Err(ParseError::new("truncated length-delimited"));
    }
    let start = *pos;
    *pos += len;
    Ok(&buf[start..*pos])
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
