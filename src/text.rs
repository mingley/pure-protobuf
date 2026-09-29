//! Protocol buffer text format (textproto).
//!
//! Official mapping for [`crate::DynamicMessage`], plus helpers used by
//! generated `to_text` / `from_text` so proto3 messages can encode and decode
//! text without allocating a `DynamicMessage`.

use crate::dynamic::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, FieldValue,
    MapKeyValue, MessageDescriptor, Presence, RECURSION_LIMIT, Value,
};

use crate::error::{ParseError, SerializeError};
use crate::string::{ProtoBytes, ProtoString};
use crate::wire::UnknownField;
use std::fmt::Write as _;
use std::sync::Arc;

pub fn encode(msg: &DynamicMessage) -> Result<String, SerializeError> {
    encode_opts(msg, false)
}

pub fn encode_with_unknown(msg: &DynamicMessage) -> Result<String, SerializeError> {
    encode_opts(msg, true)
}

fn encode_opts(msg: &DynamicMessage, print_unknown: bool) -> Result<String, SerializeError> {
    let mut out = String::new();
    write_msg(msg, &mut out, 0, print_unknown)?;
    Ok(out)
}

pub fn decode(desc: Arc<MessageDescriptor>, text: &str) -> Result<DynamicMessage, ParseError> {
    decode_with_pool(desc, text, None)
}

pub fn decode_with_pool(
    desc: Arc<MessageDescriptor>,
    text: &str,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let mut p = Parser {
        src: text.as_bytes(),
        pos: 0,
        pool,
        depth: 0,
    };
    let msg = p.parse_message(desc)?;
    p.ws();
    if p.pos < p.src.len() {
        return Err(ParseError::new("trailing text"));
    }
    Ok(msg)
}

/// One text-format value. Used by generated field-wise `from_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextValue {
    /// Unquoted token (integers, bools, idents).
    Token(String),
    /// Quoted string or bytes literal (escapes already decoded).
    Bytes(Vec<u8>),
    /// `{ ... }` or `< ... >` message body.
    Message(Vec<(String, TextValue)>),
    /// `[ ... ]` list (repeated fields).
    List(Vec<TextValue>),
}

impl TextValue {
    /// Decode an `int32` token (decimal / hex / octal, same as DynamicMessage).
    pub fn as_i32(&self) -> Result<i32, ParseError> {
        match self {
            Self::Token(t) => parse_i32(t),
            _ => Err(ParseError::new("expected int32")),
        }
    }

    /// Decode an `int64` token (decimal / hex / octal, same as DynamicMessage).
    pub fn as_i64(&self) -> Result<i64, ParseError> {
        match self {
            Self::Token(t) => parse_i64(t),
            _ => Err(ParseError::new("expected int64")),
        }
    }

    /// Decode a `uint32` token (decimal / hex / octal, same as DynamicMessage).
    pub fn as_u32(&self) -> Result<u32, ParseError> {
        match self {
            Self::Token(t) => parse_u32(t),
            _ => Err(ParseError::new("expected uint32")),
        }
    }

    /// Decode a `uint64` token (decimal / hex / octal, same as DynamicMessage).
    pub fn as_u64(&self) -> Result<u64, ParseError> {
        match self {
            Self::Token(t) => parse_u64(t),
            _ => Err(ParseError::new("expected uint64")),
        }
    }

    /// Decode a bool token (`true` / `True` / `t` / `1` and false forms).
    pub fn as_bool(&self) -> Result<bool, ParseError> {
        match self {
            Self::Token(t) => match t.as_str() {
                "true" | "True" | "t" | "T" | "1" => Ok(true),
                "false" | "False" | "f" | "F" | "0" => Ok(false),
                _ => Err(ParseError::owned(format!("bad bool {t}"))),
            },
            _ => Err(ParseError::new("expected bool")),
        }
    }

    /// Decode a `float` token (not hex / octal, same as DynamicMessage).
    pub fn as_f32(&self) -> Result<f32, ParseError> {
        match self {
            Self::Token(t) => {
                if is_hex_or_octal_int(t) {
                    return Err(ParseError::new("hex/octal float not allowed"));
                }
                parse_f32(t)
            }
            _ => Err(ParseError::new("expected float")),
        }
    }

    /// Decode a `double` token (not hex / octal, same as DynamicMessage).
    pub fn as_f64(&self) -> Result<f64, ParseError> {
        match self {
            Self::Token(t) => {
                if is_hex_or_octal_int(t) {
                    return Err(ParseError::new("hex/octal float not allowed"));
                }
                parse_f64(t)
            }
            _ => Err(ParseError::new("expected double")),
        }
    }

    /// Decode a quoted UTF-8 string.
    pub fn as_str(&self) -> Result<&str, ParseError> {
        match self {
            Self::Bytes(b) => std::str::from_utf8(b).map_err(|_| ParseError::new("invalid utf-8")),
            _ => Err(ParseError::new("expected string")),
        }
    }

    /// Decode a quoted bytes literal.
    pub fn as_bytes(&self) -> Result<&[u8], ParseError> {
        match self {
            Self::Bytes(b) => Ok(b.as_slice()),
            _ => Err(ParseError::new("expected bytes")),
        }
    }

    /// Decode a proto3 enum token (ident name or integer).
    pub fn as_enum(&self, lookup: impl Fn(&str) -> Option<i32>) -> Result<i32, ParseError> {
        match self {
            Self::Token(t) => {
                if t.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                    lookup(t).ok_or_else(|| ParseError::owned(format!("unknown enum {t}")))
                } else {
                    parse_i32(t)
                }
            }
            _ => Err(ParseError::new("expected enum")),
        }
    }

    /// Decode a nested message body.
    pub fn as_message(&self) -> Result<&[(String, TextValue)], ParseError> {
        match self {
            Self::Message(fields) => Ok(fields.as_slice()),
            _ => Err(ParseError::new("expected { for message")),
        }
    }

    /// A `[...]` list, if this value is one.
    pub fn as_list(&self) -> Option<&[TextValue]> {
        match self {
            Self::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }
}

/// Parse a text-format message body without allocating a `DynamicMessage`.
pub fn parse(text: &str) -> Result<Vec<(String, TextValue)>, ParseError> {
    let mut r = TextReader::new(text);
    let fields = build_text_fields(&mut r)?;
    r.finish()?;
    Ok(fields)
}

fn build_text_fields(r: &mut TextReader<'_>) -> Result<Vec<(String, TextValue)>, ParseError> {
    let mut fields = Vec::new();
    while let Some(f) = r.next_field()? {
        let v = if r.at_list() {
            r.enter_list()?;
            let mut items = Vec::new();
            while !r.at_list_end() {
                items.push(build_text_value(r)?);
                r.list_item_done()?;
            }
            r.exit_list()?;
            TextValue::List(items)
        } else {
            build_text_value(r)?
        };
        fields.push((f.name().to_owned(), v));
    }
    Ok(fields)
}

fn build_text_value(r: &mut TextReader<'_>) -> Result<TextValue, ParseError> {
    if r.at_message() {
        let close = r.enter_message()?;
        let fields = build_text_fields(r)?;
        r.exit_message(close)?;
        return Ok(TextValue::Message(fields));
    }
    if r.at_string() {
        return Ok(TextValue::Bytes(r.read_raw_bytes()?));
    }
    Ok(TextValue::Token(r.read_token_owned()?))
}

/// Skip text-format whitespace and `#` comments.
fn skip_ws(src: &[u8], pos: &mut usize) {
    while *pos < src.len() {
        match src[*pos] {
            b' ' | b'\n' | b'\r' | b'\t' => *pos += 1,
            b'#' => {
                while *pos < src.len() && src[*pos] != b'\n' {
                    *pos += 1;
                }
            }
            _ => break,
        }
    }
}

fn peek_at(src: &[u8], pos: usize) -> u8 {
    src.get(pos).copied().unwrap_or(0)
}

fn scan_ident<'a>(src: &'a [u8], pos: &mut usize) -> Result<&'a str, ParseError> {
    skip_ws(src, pos);
    let start = *pos;
    while *pos < src.len() {
        let c = src[*pos];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
            *pos += 1;
        } else {
            break;
        }
    }
    if start == *pos {
        return Err(ParseError::new("expected identifier"));
    }
    std::str::from_utf8(&src[start..*pos]).map_err(|_| ParseError::new("expected identifier"))
}

fn scan_number<'a>(src: &'a [u8], pos: &mut usize) -> Result<&'a str, ParseError> {
    skip_ws(src, pos);
    let start = *pos;
    if peek_at(src, *pos) == b'+' || peek_at(src, *pos) == b'-' {
        *pos += 1;
    }
    if peek_at(src, *pos).is_ascii_alphabetic() {
        while *pos < src.len() {
            let c = src[*pos];
            if c.is_ascii_alphanumeric() || c == b'_' {
                *pos += 1;
            } else {
                break;
            }
        }
        return std::str::from_utf8(&src[start..*pos])
            .map_err(|_| ParseError::new("expected number"));
    }
    while *pos < src.len() {
        let c = src[*pos];
        if c.is_ascii_alphanumeric() || c == b'.' || c == b'+' || c == b'_' || c == b'-' {
            *pos += 1;
        } else {
            break;
        }
    }
    if start == *pos {
        return Err(ParseError::new("expected number"));
    }
    std::str::from_utf8(&src[start..*pos]).map_err(|_| ParseError::new("expected number"))
}

/// Scan a `[...]` extension / `Any` type-url token, brackets included.
fn scan_bracket(src: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    if peek_at(src, *pos) != b'[' {
        return Err(ParseError::new("expected ["));
    }
    *pos += 1;
    let mut out = String::from("[");
    loop {
        skip_ws(src, pos);
        if *pos >= src.len() {
            return Err(ParseError::new("unterminated ["));
        }
        let c = peek_at(src, *pos);
        if c == b']' {
            *pos += 1;
            out.push(']');
            return Ok(out);
        }
        if c == b'#' {
            skip_ws(src, pos);
            continue;
        }
        out.push(c as char);
        *pos += 1;
    }
}

fn read_hex_at(src: &[u8], pos: &mut usize, max: usize) -> (u32, usize) {
    let mut n = 0u32;
    let mut used = 0;
    while used < max && *pos < src.len() {
        let c = src[*pos];
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => break,
        };
        n = (n << 4) | u32::from(d);
        *pos += 1;
        used += 1;
    }
    (n, used)
}

fn read_octal_at(src: &[u8], pos: &mut usize) -> u8 {
    let mut n = 0u8;
    let mut i = 0;
    while i < 3 && *pos < src.len() {
        let c = src[*pos];
        if !c.is_ascii_digit() || c > b'7' {
            break;
        }
        n = n.wrapping_mul(8).wrapping_add(c - b'0');
        *pos += 1;
        i += 1;
    }
    n
}

/// Decode one quoted string literal (escapes resolved).
fn read_quoted(src: &[u8], pos: &mut usize) -> Result<Vec<u8>, ParseError> {
    skip_ws(src, pos);
    let quote = peek_at(src, *pos);
    if quote != b'"' && quote != b'\'' {
        return Err(ParseError::new("expected string"));
    }
    *pos += 1;
    let mut out = Vec::new();
    while *pos < src.len() {
        let c = src[*pos];
        if c == b'\n' {
            return Err(ParseError::new("string literal includes LF"));
        }
        *pos += 1;
        if c == quote {
            return Ok(out);
        }
        if c != b'\\' {
            out.push(c);
            continue;
        }
        if *pos >= src.len() {
            break;
        }
        let e = src[*pos];
        *pos += 1;
        match e {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'?' => out.push(b'?'),
            b'\\' => out.push(b'\\'),
            b'\'' => out.push(b'\''),
            b'"' => out.push(b'"'),
            b'x' | b'X' => {
                let (n, used) = read_hex_at(src, pos, 2);
                if used == 0 {
                    return Err(ParseError::new("bad hex escape"));
                }
                out.push(n as u8);
            }
            b'u' => {
                let (cp, used) = read_hex_at(src, pos, 4);
                if used != 4 {
                    return Err(ParseError::new("bad unicode escape"));
                }
                push_utf8_cp(&mut out, cp)?;
            }
            b'U' => {
                let (cp, used) = read_hex_at(src, pos, 8);
                if used != 8 {
                    return Err(ParseError::new("bad unicode escape"));
                }
                push_utf8_cp(&mut out, cp)?;
            }
            b'0'..=b'7' => {
                *pos -= 1;
                out.push(read_octal_at(src, pos));
            }
            _ => out.push(e),
        }
    }
    Err(ParseError::new("unterminated string"))
}

/// Decode one or more adjacent quoted literals (official concatenation).
fn concat_quoted(src: &[u8], pos: &mut usize) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::new();
    let mut seen = false;
    loop {
        skip_ws(src, pos);
        if peek_at(src, *pos) != b'"' && peek_at(src, *pos) != b'\'' {
            break;
        }
        seen = true;
        out.extend(read_quoted(src, pos)?);
        skip_ws(src, pos);
        if peek_at(src, *pos) != b'"' && peek_at(src, *pos) != b'\'' {
            break;
        }
    }
    if !seen {
        return Err(ParseError::new("expected string"));
    }
    Ok(out)
}

/// One text-format field header from [`TextReader::next_field`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextField<'a> {
    name: std::borrow::Cow<'a, str>,
}

impl<'a> TextField<'a> {
    /// Field name as written (`[ext/name]` forms keep their brackets).
    ///
    /// Plain identifiers borrow the input; only `[...]` tokens allocate.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Streaming text-format reader over a borrowed buffer.
///
/// Unlike [`parse`], this visits one field at a time without building an
/// intermediate `Vec<(String, TextValue)>`: field names and scalar tokens are
/// borrowed slices, and nested messages are visited inline. Generated
/// `from_text` drives this directly; the accept language matches [`parse`].
pub struct TextReader<'a> {
    src: &'a [u8],
    pos: usize,
    depth: u32,
    fresh: bool,
}

impl<'a> TextReader<'a> {
    /// Borrow `text` for streaming field-wise reads.
    pub fn new(text: &'a str) -> Self {
        Self {
            src: text.as_bytes(),
            pos: 0,
            depth: 0,
            fresh: true,
        }
    }

    /// Next field header, or `None` at end of input / `}` / `>`.
    ///
    /// Consumes the previous field's trailing separator (if any) and this
    /// field's optional `:`, so the reader is positioned at the value.
    pub fn next_field(&mut self) -> Result<Option<TextField<'a>>, ParseError> {
        skip_ws(self.src, &mut self.pos);
        if self.fresh {
            if self.peek() == b',' || self.peek() == b';' {
                return Err(ParseError::new("unexpected separator"));
            }
        } else if self.peek() == b',' || self.peek() == b';' {
            let sep = self.peek();
            self.pos += 1;
            skip_ws(self.src, &mut self.pos);
            if self.peek() == sep {
                return Err(ParseError::new("duplicate field separator"));
            }
        }
        skip_ws(self.src, &mut self.pos);
        let c = self.peek();
        if c == b'}' || c == b'>' || c == 0 {
            return Ok(None);
        }
        let name = if c == b'[' {
            std::borrow::Cow::Owned(scan_bracket(self.src, &mut self.pos)?)
        } else {
            std::borrow::Cow::Borrowed(scan_ident(self.src, &mut self.pos)?)
        };
        skip_ws(self.src, &mut self.pos);
        if self.peek() == b':' {
            self.pos += 1;
            skip_ws(self.src, &mut self.pos);
        }
        self.fresh = false;
        Ok(Some(TextField { name }))
    }

    /// End-of-input check after the field loop (mirrors [`parse`]).
    pub fn finish(&mut self) -> Result<(), ParseError> {
        skip_ws(self.src, &mut self.pos);
        if self.pos < self.src.len() {
            return Err(ParseError::new("trailing text"));
        }
        Ok(())
    }

    /// Whether the value position holds a `[...]` list.
    pub fn at_list(&mut self) -> bool {
        skip_ws(self.src, &mut self.pos);
        self.peek() == b'['
    }

    /// Whether the value position holds a `{...}` / `<...>` message.
    pub fn at_message(&mut self) -> bool {
        skip_ws(self.src, &mut self.pos);
        self.peek() == b'{' || self.peek() == b'<'
    }

    /// Whether the value position holds a quoted string.
    pub fn at_string(&mut self) -> bool {
        skip_ws(self.src, &mut self.pos);
        self.peek() == b'"' || self.peek() == b'\''
    }

    /// Consume `[` to start a repeated-field list.
    pub fn enter_list(&mut self) -> Result<(), ParseError> {
        skip_ws(self.src, &mut self.pos);
        if self.peek() != b'[' {
            return Err(ParseError::new("expected ["));
        }
        self.pos += 1;
        Ok(())
    }

    /// Whether the list is positioned at its closing `]`.
    pub fn at_list_end(&mut self) -> bool {
        skip_ws(self.src, &mut self.pos);
        self.peek() == b']'
    }

    /// Consume the separator after one list item (or validate its end).
    pub fn list_item_done(&mut self) -> Result<(), ParseError> {
        skip_ws(self.src, &mut self.pos);
        if self.peek() == b',' {
            self.pos += 1;
            skip_ws(self.src, &mut self.pos);
            if self.peek() == b',' || self.peek() == b']' {
                return Err(ParseError::new("invalid list separator"));
            }
            return Ok(());
        }
        if self.peek() == b']' {
            return Ok(());
        }
        Err(ParseError::new("expected , or ]"))
    }

    /// Consume the closing `]` of a repeated-field list.
    pub fn exit_list(&mut self) -> Result<(), ParseError> {
        skip_ws(self.src, &mut self.pos);
        if self.peek() != b']' {
            return Err(ParseError::new("expected , or ]"));
        }
        self.pos += 1;
        Ok(())
    }

    /// Consume `{` / `<` and return the matching close delimiter.
    pub fn enter_message(&mut self) -> Result<u8, ParseError> {
        skip_ws(self.src, &mut self.pos);
        let close = match self.peek() {
            b'{' => b'}',
            b'<' => b'>',
            _ => return Err(ParseError::new("expected { or <")),
        };
        self.pos += 1;
        self.depth += 1;
        if self.depth > RECURSION_LIMIT {
            return Err(ParseError::new("recursion limit exceeded"));
        }
        self.fresh = true;
        Ok(close)
    }

    /// Consume the delimiter returned by [`TextReader::enter_message`].
    pub fn exit_message(&mut self, close: u8) -> Result<(), ParseError> {
        self.depth = self.depth.saturating_sub(1);
        skip_ws(self.src, &mut self.pos);
        if self.peek() != close {
            return Err(ParseError::owned(format!("expected {}", close as char)));
        }
        self.pos += 1;
        self.fresh = false;
        Ok(())
    }

    /// Borrowed scalar token (rejects quoted / delimited values).
    fn token(&mut self) -> Result<&'a str, TokenError> {
        skip_ws(self.src, &mut self.pos);
        match self.peek() {
            b'"' | b'\'' | b'{' | b'<' | b'[' => Err(TokenError::WrongKind),
            _ => scan_number(self.src, &mut self.pos).map_err(TokenError::Invalid),
        }
    }

    /// Owned scalar token for the compatibility tree builder.
    fn read_token_owned(&mut self) -> Result<String, ParseError> {
        match self.token() {
            Ok(t) => Ok(t.to_owned()),
            Err(TokenError::WrongKind) => Err(ParseError::new("expected number")),
            Err(TokenError::Invalid(e)) => Err(e),
        }
    }

    /// Raw concatenated string bytes (no UTF-8 check) for the tree builder.
    fn read_raw_bytes(&mut self) -> Result<Vec<u8>, ParseError> {
        concat_quoted(self.src, &mut self.pos)
    }

    /// Read an `int32` value (decimal / hex / octal).
    pub fn read_i32(&mut self) -> Result<i32, ParseError> {
        match self.token() {
            Ok(t) => parse_i32(t),
            Err(TokenError::WrongKind) => Err(ParseError::new("expected int32")),
            Err(TokenError::Invalid(e)) => Err(e),
        }
    }

    /// Read an `int64` value (decimal / hex / octal).
    pub fn read_i64(&mut self) -> Result<i64, ParseError> {
        match self.token() {
            Ok(t) => parse_i64(t),
            Err(TokenError::WrongKind) => Err(ParseError::new("expected int64")),
            Err(TokenError::Invalid(e)) => Err(e),
        }
    }

    /// Read a `uint32` value (decimal / hex / octal).
    pub fn read_u32(&mut self) -> Result<u32, ParseError> {
        match self.token() {
            Ok(t) => parse_u32(t),
            Err(TokenError::WrongKind) => Err(ParseError::new("expected uint32")),
            Err(TokenError::Invalid(e)) => Err(e),
        }
    }

    /// Read a `uint64` value (decimal / hex / octal).
    pub fn read_u64(&mut self) -> Result<u64, ParseError> {
        match self.token() {
            Ok(t) => parse_u64(t),
            Err(TokenError::WrongKind) => Err(ParseError::new("expected uint64")),
            Err(TokenError::Invalid(e)) => Err(e),
        }
    }

    /// Read a bool value (`true` / `True` / `t` / `1` and false forms).
    pub fn read_bool(&mut self) -> Result<bool, ParseError> {
        let t = match self.token() {
            Ok(t) => t,
            Err(TokenError::WrongKind) => return Err(ParseError::new("expected bool")),
            Err(TokenError::Invalid(e)) => return Err(e),
        };
        match t {
            "true" | "True" | "t" | "T" | "1" => Ok(true),
            "false" | "False" | "f" | "F" | "0" => Ok(false),
            _ => Err(ParseError::owned(format!("bad bool {t}"))),
        }
    }

    /// Read a `float` value (hex / octal rejected, like [`parse`]).
    pub fn read_f32(&mut self) -> Result<f32, ParseError> {
        let t = match self.token() {
            Ok(t) => t,
            Err(TokenError::WrongKind) => return Err(ParseError::new("expected float")),
            Err(TokenError::Invalid(e)) => return Err(e),
        };
        if is_hex_or_octal_int(t) {
            return Err(ParseError::new("hex/octal float not allowed"));
        }
        parse_f32(t)
    }

    /// Read a `double` value (hex / octal rejected, like [`parse`]).
    pub fn read_f64(&mut self) -> Result<f64, ParseError> {
        let t = match self.token() {
            Ok(t) => t,
            Err(TokenError::WrongKind) => return Err(ParseError::new("expected double")),
            Err(TokenError::Invalid(e)) => return Err(e),
        };
        if is_hex_or_octal_int(t) {
            return Err(ParseError::new("hex/octal float not allowed"));
        }
        parse_f64(t)
    }

    /// Read a quoted UTF-8 string (adjacent literals concatenate).
    pub fn read_string(&mut self) -> Result<String, ParseError> {
        if !self.at_string() {
            return Err(ParseError::new("expected string"));
        }
        let bytes = concat_quoted(self.src, &mut self.pos)?;
        String::from_utf8(bytes).map_err(|_| ParseError::new("invalid utf-8"))
    }

    /// Read a quoted bytes literal (adjacent literals concatenate).
    pub fn read_bytes(&mut self) -> Result<Vec<u8>, ParseError> {
        if !self.at_string() {
            return Err(ParseError::new("expected bytes"));
        }
        concat_quoted(self.src, &mut self.pos)
    }

    /// Read a proto3 enum (ident name or integer).
    pub fn read_enum(&mut self, lookup: impl Fn(&str) -> Option<i32>) -> Result<i32, ParseError> {
        let t = match self.token() {
            Ok(t) => t,
            Err(TokenError::WrongKind) => return Err(ParseError::new("expected enum")),
            Err(TokenError::Invalid(e)) => return Err(e),
        };
        if t.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            lookup(t).ok_or_else(|| ParseError::owned(format!("unknown enum {t}")))
        } else {
            parse_i32(t)
        }
    }

    fn peek(&self) -> u8 {
        peek_at(self.src, self.pos)
    }
}

enum TokenError {
    /// Quoted string or delimited value where a scalar token belongs.
    WrongKind,
    /// Malformed scalar token (scan error preserved).
    Invalid(ParseError),
}

/// Write indent spaces.
pub fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

/// Write a quoted text-format string / bytes literal.
pub fn write_bytes_lit(bytes: &[u8], out: &mut String) {
    out.push('"');
    for &b in bytes {
        match b {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\'' => out.push_str("\\'"),
            0x07 => out.push_str("\\a"),
            0x08 => out.push_str("\\b"),
            0x0c => out.push_str("\\f"),
            0x0b => out.push_str("\\v"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                // Octal escape without a `format!` temporary per byte.
                out.push('\\');
                out.push((b'0' + ((b >> 6) & 7)) as char);
                out.push((b'0' + ((b >> 3) & 7)) as char);
                out.push((b'0' + (b & 7)) as char);
            }
        }
    }
    out.push('"');
}

/// Write an integer literal with no intermediate `String`.
///
/// Generated map-value and repeated-scalar writers use this instead of
/// `out.push_str(&v.to_string())`, which allocates one temporary per value.
pub fn write_int_lit<T: std::fmt::Display>(out: &mut String, v: T) {
    let _ = write!(out, "{v}");
}

/// Write `name: <token>` at `indent`.
pub fn write_named_token(out: &mut String, indent: usize, name: &str, token: &str) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    out.push_str(token);
    out.push('\n');
}

/// Write `name: <int>` at `indent` with no intermediate `String`.
fn write_named_int<T: std::fmt::Display>(out: &mut String, indent: usize, name: &str, n: T) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    write_int_lit(out, n);
    out.push('\n');
}

/// Write `name: <int32>` at `indent`.
pub fn write_named_int32(out: &mut String, indent: usize, name: &str, n: i32) {
    write_named_int(out, indent, name, n);
}

/// Write `name: <int64>` at `indent`.
pub fn write_named_int64(out: &mut String, indent: usize, name: &str, n: i64) {
    write_named_int(out, indent, name, n);
}

/// Write `name: <uint32>` at `indent`.
pub fn write_named_uint32(out: &mut String, indent: usize, name: &str, n: u32) {
    write_named_int(out, indent, name, n);
}

/// Write `name: <uint64>` at `indent`.
pub fn write_named_uint64(out: &mut String, indent: usize, name: &str, n: u64) {
    write_named_int(out, indent, name, n);
}

/// Write `name: true` / `name: false` at `indent`.
pub fn write_named_bool(out: &mut String, indent: usize, name: &str, b: bool) {
    write_named_token(out, indent, name, if b { "true" } else { "false" });
}

/// Write `name: <float>` at `indent` (nan / inf / -0, same as DynamicMessage).
pub fn write_named_float(out: &mut String, indent: usize, name: &str, n: f32) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    write_float32(n, out);
    out.push('\n');
}

/// Write `name: <double>` at `indent` (nan / inf / -0, same as DynamicMessage).
pub fn write_named_double(out: &mut String, indent: usize, name: &str, n: f64) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    write_float64(n, out);
    out.push('\n');
}

/// Write a float literal (nan / inf / -0, same as DynamicMessage).
pub fn write_float_lit(out: &mut String, n: f32) {
    write_float32(n, out);
}

/// Write a double literal (nan / inf / -0, same as DynamicMessage).
pub fn write_double_lit(out: &mut String, n: f64) {
    write_float64(n, out);
}

/// Write an enum name, or the numeric value if unknown.
pub fn write_enum_lit(out: &mut String, enum_name: Option<&str>, n: i32) {
    if let Some(name) = enum_name {
        out.push_str(name);
    } else {
        write_int_lit(out, n);
    }
}

/// Write `name: <enum>` at `indent`.
pub fn write_named_enum(
    out: &mut String,
    indent: usize,
    name: &str,
    enum_name: Option<&str>,
    n: i32,
) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    write_enum_lit(out, enum_name, n);
    out.push('\n');
}

/// Write `name: "<bytes>"` at `indent`.
pub fn write_named_string(out: &mut String, indent: usize, name: &str, bytes: &[u8]) {
    pad(out, indent);
    out.push_str(name);
    out.push_str(": ");
    write_bytes_lit(bytes, out);
    out.push('\n');
}

/// Write a map entry in DynamicMessage text shape (`key` / `value` body).
pub fn write_map_entry(
    out: &mut String,
    indent: usize,
    name: &str,
    write_key: impl FnOnce(&mut String),
    write_value: impl FnOnce(&mut String) -> Result<(), SerializeError>,
    value_is_message: bool,
) -> Result<(), SerializeError> {
    pad(out, indent);
    out.push_str(name);
    out.push_str(" {\n");
    pad(out, indent + 2);
    out.push_str("key: ");
    write_key(out);
    out.push('\n');
    pad(out, indent + 2);
    if value_is_message {
        out.push_str("value ");
        write_value(out)?;
    } else {
        out.push_str("value: ");
        write_value(out)?;
        out.push('\n');
    }
    pad(out, indent);
    out.push_str("}\n");
    Ok(())
}

/// Write a `map<string, int32>` entry in DynamicMessage text order.
pub fn write_map_string_i32(out: &mut String, indent: usize, name: &str, key: &[u8], value: i32) {
    let _ = write_map_entry(
        out,
        indent,
        name,
        |out| write_bytes_lit(key, out),
        |out| {
            write_int_lit(out, value);
            Ok(())
        },
        false,
    );
}

/// Write unknown fields (same printer as [`DynamicMessage::to_text_with_unknown`]).
pub fn write_unknown_fields(unknown: &crate::wire::UnknownFields, out: &mut String, indent: usize) {
    for uf in &unknown.fields {
        write_unknown(uf, out, indent);
    }
}

fn write_msg(
    msg: &DynamicMessage,
    out: &mut String,
    indent: usize,
    print_unknown: bool,
) -> Result<(), SerializeError> {
    for (num, fv) in msg.raw_fields() {
        let Some(field) = msg.descriptor().field(*num) else {
            continue;
        };
        match fv {
            FieldValue::Singular(v) => {
                if field.presence == Presence::Implicit && v.is_implicit_default() {
                    continue;
                }
                write_field(field, v, out, indent)?;
            }
            FieldValue::Repeated(items) => {
                for v in items {
                    write_field(field, v, out, indent)?;
                }
            }
            FieldValue::Map(items) => {
                for (k, v) in items {
                    pad(out, indent);
                    out.push_str(&field.name);
                    out.push_str(" {\n");
                    pad(out, indent + 2);
                    out.push_str("key: ");
                    write_map_key(k, out);
                    out.push('\n');
                    pad(out, indent + 2);
                    if matches!(v, Value::Message(_)) {
                        out.push_str("value ");
                    } else {
                        out.push_str("value: ");
                    }
                    write_leaf(v, field, out, indent + 2)?;
                    if !matches!(v, Value::Message(_)) {
                        out.push('\n');
                    }
                    pad(out, indent);
                    out.push_str("}\n");
                }
            }
        }
    }
    if print_unknown {
        for uf in &msg.unknown_fields().fields {
            write_unknown(uf, out, indent);
        }
    }
    Ok(())
}

fn parse_unknown_message(data: &[u8]) -> Option<crate::wire::UnknownFields> {
    if data.is_empty() {
        return None;
    }
    let mut pos = 0;
    let mut fields = crate::wire::UnknownFields::default();
    while pos < data.len() {
        let (n, w) = crate::wire::decode_tag(data, &mut pos).ok()?;
        fields
            .fields
            .push(crate::wire::capture_unknown(data, &mut pos, n, w).ok()?);
    }
    Some(fields)
}

fn write_unknown(uf: &UnknownField, out: &mut String, indent: usize) {
    pad(out, indent);
    match uf {
        UnknownField::Varint { number, value } => {
            let _ = writeln!(out, "{number}: {value}");
        }
        UnknownField::Fixed32 { number, value } => {
            let _ = writeln!(out, "{number}: 0x{value:08x}");
        }
        UnknownField::Fixed64 { number, value } => {
            let _ = writeln!(out, "{number}: 0x{value:016x}");
        }
        UnknownField::LengthDelimited { number, value } => {
            if let Some(inner) = parse_unknown_message(value) {
                let _ = writeln!(out, "{number} {{");
                for uf in &inner.fields {
                    write_unknown(uf, out, indent + 2);
                }
                pad(out, indent);
                out.push_str("}\n");
            } else {
                let _ = write!(out, "{number}: ");
                write_bytes_lit(value, out);
                out.push('\n');
            }
        }
        UnknownField::Group { number, fields } => {
            let _ = writeln!(out, "{number} {{");
            for inner in &fields.fields {
                write_unknown(inner, out, indent + 2);
            }
            pad(out, indent);
            out.push_str("}\n");
        }
    }
}

fn write_map_key(k: &MapKeyValue, out: &mut String) {
    match k {
        MapKeyValue::I32(n) => write_int_lit(out, *n),
        MapKeyValue::I64(n) => write_int_lit(out, *n),
        MapKeyValue::U32(n) => write_int_lit(out, *n),
        MapKeyValue::U64(n) => write_int_lit(out, *n),
        MapKeyValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        MapKeyValue::String(s) => write_bytes_lit(s.as_bytes(), out),
    }
}

fn write_field(
    field: &FieldDescriptor,
    v: &Value,
    out: &mut String,
    indent: usize,
) -> Result<(), SerializeError> {
    pad(out, indent);
    if let Some(ext_name) = &field.extension_name {
        out.push('[');
        out.push_str(ext_name);
        out.push(']');
    } else if field.extendee.is_some() {
        out.push('[');
        out.push_str(&field.name);
        out.push(']');
    } else {
        out.push_str(&field.name);
    }
    match v {
        Value::Message(_) => {
            out.push(' ');
            write_leaf(v, field, out, indent)?;
        }
        other => {
            out.push_str(": ");
            write_leaf(other, field, out, indent)?;
            out.push('\n');
        }
    }
    Ok(())
}

fn write_leaf(
    v: &Value,
    field: &FieldDescriptor,
    out: &mut String,
    indent: usize,
) -> Result<(), SerializeError> {
    match v {
        Value::Double(n) => write_float64(*n, out),
        Value::Float(n) => write_float32(*n, out),
        Value::Int32(n) => write_int_lit(out, *n),
        Value::Int64(n) => write_int_lit(out, *n),
        Value::Uint32(n) => write_int_lit(out, *n),
        Value::Uint64(n) => write_int_lit(out, *n),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::String(s) => write_bytes_lit(s.as_bytes(), out),
        Value::Bytes(b) => write_bytes_lit(b.as_bytes(), out),
        Value::Enum(n) => {
            if let Some(name) = field.enum_ty.as_ref().and_then(|e| e.values.get(n)) {
                out.push_str(name);
            } else {
                write_int_lit(out, *n);
            }
        }
        Value::Message(m) => {
            out.push_str("{\n");
            write_msg(m, out, indent + 2, false)?;
            pad(out, indent);
            out.push_str("}\n");
        }
    }
    Ok(())
}

fn write_float32(n: f32, out: &mut String) {
    if n.is_nan() {
        out.push_str("nan");
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            out.push_str("-inf");
        } else {
            out.push_str("inf");
        }
    } else if n == 0.0 && n.is_sign_negative() {
        out.push_str("-0");
    } else {
        write_int_lit(out, n);
    }
}

fn write_float64(n: f64, out: &mut String) {
    if n.is_nan() {
        out.push_str("nan");
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            out.push_str("-inf");
        } else {
            out.push_str("inf");
        }
    } else if n == 0.0 && n.is_sign_negative() {
        out.push_str("-0");
    } else {
        write_int_lit(out, n);
    }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    pool: Option<Arc<DescriptorPool>>,
    depth: u32,
}

impl<'a> Parser<'a> {
    fn parse_message(
        &mut self,
        desc: Arc<MessageDescriptor>,
    ) -> Result<DynamicMessage, ParseError> {
        if self.depth > RECURSION_LIMIT {
            return Err(ParseError::new("recursion limit exceeded"));
        }
        let mut msg = DynamicMessage::new(desc.clone());
        if let Some(p) = &self.pool {
            msg.set_pool(p.clone());
        }
        self.ws();
        while self.pos < self.src.len() {
            let c = self.peek();
            if c == b'}' || c == b'>' {
                break;
            }
            if c == 0 {
                break;
            }
            if self.try_consume_sep_only()? {
                continue;
            }
            let field_tok = self.field_token()?;
            self.ws();
            if desc.is_reserved_name(&field_tok) {
                if self.peek() == b':' {
                    self.pos += 1;
                    self.ws();
                }
                self.skip_value()?;
                self.optional_separator()?;
                continue;
            }
            if desc.full_name == "google.protobuf.Any" && field_tok.starts_with('[') {
                self.parse_any_type_url(&mut msg, &field_tok)?;
                self.optional_separator()?;
                continue;
            }
            let field = self.resolve_field(&desc, &field_tok)?;
            if self.peek() == b':' {
                self.pos += 1;
                self.ws();
            }
            if self.peek() == b'['
                && (field.cardinality == Cardinality::Repeated || field.is_map)
                && !field.is_map
            {
                self.parse_list(&mut msg, &field)?;
            } else {
                let v = self.parse_value(&field)?;
                self.apply_value(&mut msg, &field, v)?;
            }
            self.optional_separator()?;
        }
        Ok(msg)
    }

    fn resolve_field(
        &self,
        desc: &MessageDescriptor,
        tok: &str,
    ) -> Result<FieldDescriptor, ParseError> {
        if let Some(inner) = tok.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Some(pool) = &self.pool {
                if let Some((_host, field)) = pool.get_extension(inner) {
                    return Ok(field);
                }
            }
            if let Some(&n) = desc.fields_by_name.get(inner) {
                if let Some(f) = desc.field(n) {
                    if f.extension_name.is_some() {
                        return Ok(f.clone());
                    }
                }
            }
            return Err(ParseError::owned(format!("unknown extension {inner}")));
        }
        desc.field_by_name(tok)
            .cloned()
            .ok_or_else(|| ParseError::owned(format!("unknown field {tok}")))
    }

    fn apply_value(
        &self,
        msg: &mut DynamicMessage,
        field: &FieldDescriptor,
        v: Value,
    ) -> Result<(), ParseError> {
        if field.is_map {
            if let Value::Message(entry) = v {
                // Missing key/value default per the entry field types, matching
                // the wire path (`decode_map_entry`): an untyped Int32(0) default
                // breaks text re-parsing for string/bool/message maps.
                let key = match entry.get_singular(1) {
                    Some(Value::String(s)) => MapKeyValue::String(s.clone()),
                    Some(Value::Int32(n)) => MapKeyValue::I32(*n),
                    Some(Value::Int64(n)) => MapKeyValue::I64(*n),
                    Some(Value::Uint32(n)) => MapKeyValue::U32(*n),
                    Some(Value::Uint64(n)) => MapKeyValue::U64(*n),
                    Some(Value::Bool(b)) => MapKeyValue::Bool(*b),
                    Some(_) => return Err(ParseError::new("invalid map key type")),
                    None => {
                        let kf = entry
                            .descriptor()
                            .field(1)
                            .ok_or_else(|| ParseError::new("map entry missing key"))?;
                        crate::dynamic::default_map_key(kf.field_type)?
                    }
                };
                let val = match entry.get_singular(2) {
                    Some(v) => v.clone(),
                    None => {
                        let vf = entry
                            .descriptor()
                            .field(2)
                            .ok_or_else(|| ParseError::new("map entry missing value"))?;
                        crate::dynamic::default_value(vf, self.pool.as_ref())?
                    }
                };
                msg.insert_map(field.number, key, val);
                return Ok(());
            }
            return Err(ParseError::new("map entry expected"));
        }
        if field.cardinality == Cardinality::Repeated {
            if let Value::Enum(n) = v {
                if field
                    .enum_ty
                    .as_ref()
                    .is_some_and(|e| e.closed && !e.values.contains_key(&n))
                {
                    return Err(ParseError::new("unknown closed enum"));
                }
            }
            msg.push(field.number, v);
            return Ok(());
        }
        if let Value::Enum(n) = &v {
            if field
                .enum_ty
                .as_ref()
                .is_some_and(|e| e.closed && !e.values.contains_key(n))
            {
                return Err(ParseError::new("unknown closed enum"));
            }
        }
        if let Value::Message(incoming) = v {
            if let Some(Value::Message(existing)) = msg.get_singular(field.number).cloned() {
                let mut merged = existing;
                merged.merge_from_dyn(&incoming);
                msg.set(field.number, Value::Message(merged));
            } else {
                msg.set(field.number, Value::Message(incoming));
            }
            return Ok(());
        }
        msg.set(field.number, v);
        Ok(())
    }

    fn parse_list(
        &mut self,
        msg: &mut DynamicMessage,
        field: &FieldDescriptor,
    ) -> Result<(), ParseError> {
        if self.peek() != b'[' {
            return Err(ParseError::new("expected ["));
        }
        self.pos += 1;
        self.ws();
        if self.peek() == b']' {
            self.pos += 1;
            return Ok(());
        }
        loop {
            let v = self.parse_value(field)?;
            self.apply_value(msg, field, v)?;
            self.ws();
            if self.peek() == b',' {
                self.pos += 1;
                self.ws();
                if self.peek() == b',' || self.peek() == b']' {
                    return Err(ParseError::new("invalid list separator"));
                }
                continue;
            }
            if self.peek() == b']' {
                self.pos += 1;
                return Ok(());
            }
            if field.field_type == FieldType::String || field.field_type == FieldType::Bytes {
                continue;
            }
            return Err(ParseError::new("expected , or ]"));
        }
    }

    fn parse_any_type_url(
        &mut self,
        msg: &mut DynamicMessage,
        tok: &str,
    ) -> Result<(), ParseError> {
        let inner = tok
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .ok_or_else(|| ParseError::new("bad any type url"))?;
        let url = normalize_type_url(inner)?;
        let type_name = url
            .rsplit('/')
            .next()
            .ok_or_else(|| ParseError::new("any type url missing name"))?;
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| ParseError::new("any requires descriptor pool"))?;
        let desc = pool
            .get_message(type_name)
            .ok_or_else(|| ParseError::new("unknown any type"))?;
        if self.peek() == b':' {
            self.pos += 1;
            self.ws();
        }
        self.depth += 1;
        let inner_msg = self.parse_delimited(desc)?;
        self.depth -= 1;
        let bytes = crate::message::Serialize::serialize(&inner_msg)
            .map_err(|e| ParseError::owned(e.to_string()))?;
        msg.set(1, Value::String(ProtoString::from(url.as_str())));
        msg.set(2, Value::Bytes(ProtoBytes::from(bytes.as_slice())));
        Ok(())
    }

    fn parse_delimited(
        &mut self,
        desc: Arc<MessageDescriptor>,
    ) -> Result<DynamicMessage, ParseError> {
        self.ws();
        let (open, close) = match self.peek() {
            b'{' => (b'{', b'}'),
            b'<' => (b'<', b'>'),
            _ => return Err(ParseError::new("expected { or <")),
        };
        self.pos += 1;
        let inner = self.parse_message(desc)?;
        self.ws();
        if self.peek() != close {
            return Err(ParseError::owned(format!("expected {}", close as char)));
        }
        self.pos += 1;
        let _ = open;
        Ok(inner)
    }

    fn parse_value(&mut self, field: &FieldDescriptor) -> Result<Value, ParseError> {
        self.ws();
        if self.peek() == b'{' || self.peek() == b'<' {
            if field.field_type != FieldType::Message
                && field.field_type != FieldType::Group
                && !field.is_map
            {
                return Err(ParseError::new("unexpected message value"));
            }
            let desc = field
                .message
                .clone()
                .or_else(|| {
                    let tn = field.type_name.as_deref()?;
                    self.pool.as_ref()?.get_message(tn.trim_start_matches('.'))
                })
                .ok_or_else(|| ParseError::new("text message missing descriptor"))?;
            self.depth += 1;
            let inner = self.parse_delimited(desc)?;
            self.depth -= 1;
            return Ok(Value::Message(inner));
        }
        if field.field_type == FieldType::String || field.field_type == FieldType::Bytes {
            let bytes = self.concat_strings()?;
            if field.field_type == FieldType::String {
                std::str::from_utf8(&bytes).map_err(|_| ParseError::new("invalid utf-8"))?;
                return Ok(Value::String(ProtoString::from_bytes(&bytes)));
            }
            return Ok(Value::Bytes(ProtoBytes::from(bytes.as_slice())));
        }
        if field.field_type == FieldType::Bool {
            return Ok(Value::Bool(self.parse_bool()?));
        }
        if field.field_type == FieldType::Enum {
            return Ok(Value::Enum(self.parse_enum(field)?));
        }
        if matches!(field.field_type, FieldType::Float | FieldType::Double) {
            let tok = self.number_or_ident()?;
            if is_hex_or_octal_int(tok) {
                return Err(ParseError::new("hex/octal float not allowed"));
            }
            if field.field_type == FieldType::Float {
                return Ok(Value::Float(parse_f32(tok)?));
            }
            return Ok(Value::Double(parse_f64(tok)?));
        }
        let tok = self.number_or_ident()?;
        match field.field_type {
            FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
                Ok(Value::Int32(parse_i32(tok)?))
            }
            FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
                Ok(Value::Int64(parse_i64(tok)?))
            }
            FieldType::Uint32 | FieldType::Fixed32 => Ok(Value::Uint32(parse_u32(tok)?)),
            FieldType::Uint64 | FieldType::Fixed64 => Ok(Value::Uint64(parse_u64(tok)?)),
            FieldType::Message | FieldType::Group => Err(ParseError::new("expected { for message")),
            _ => Err(ParseError::new("unexpected scalar")),
        }
    }

    fn parse_bool(&mut self) -> Result<bool, ParseError> {
        let t = self.number_or_ident()?;
        match t {
            "true" | "True" | "t" | "T" | "1" => Ok(true),
            "false" | "False" | "f" | "F" | "0" => Ok(false),
            _ => Err(ParseError::owned(format!("bad bool {t}"))),
        }
    }

    fn parse_enum(&mut self, field: &FieldDescriptor) -> Result<i32, ParseError> {
        self.ws();
        if self.peek().is_ascii_alphabetic() || self.peek() == b'_' {
            let name = self.ident()?;
            if let Some(n) = field
                .enum_ty
                .as_ref()
                .and_then(|e| e.names.get(name).copied())
            {
                return Ok(n);
            }
            // Pool-linked nested descriptors can lack enum linkage; resolve
            // the name through the pool instead of failing outright.
            if let (Some(pool), Some(tn)) = (&self.pool, field.type_name.as_deref()) {
                if let Some(n) = pool
                    .get_enum(tn.trim_start_matches('.'))
                    .and_then(|e| e.names.get(name).copied())
                {
                    return Ok(n);
                }
            }
            Err(ParseError::owned(format!("unknown enum {name}")))
        } else {
            let tok = self.number_or_ident()?;
            parse_i32(tok)
        }
    }

    fn field_token(&mut self) -> Result<std::borrow::Cow<'a, str>, ParseError> {
        self.ws();
        if self.peek() == b'[' {
            return Ok(std::borrow::Cow::Owned(self.bracket_name()?));
        }
        Ok(std::borrow::Cow::Borrowed(self.ident()?))
    }

    fn bracket_name(&mut self) -> Result<String, ParseError> {
        scan_bracket(self.src, &mut self.pos)
    }

    fn ident(&mut self) -> Result<&'a str, ParseError> {
        scan_ident(self.src, &mut self.pos)
    }

    fn number_or_ident(&mut self) -> Result<&'a str, ParseError> {
        scan_number(self.src, &mut self.pos)
    }

    fn concat_strings(&mut self) -> Result<Vec<u8>, ParseError> {
        concat_quoted(self.src, &mut self.pos)
    }

    fn string_bytes(&mut self) -> Result<Vec<u8>, ParseError> {
        read_quoted(self.src, &mut self.pos)
    }

    fn skip_value(&mut self) -> Result<(), ParseError> {
        self.ws();
        match self.peek() {
            b'{' => self.skip_block(b'{', b'}'),
            b'<' => self.skip_block(b'<', b'>'),
            b'[' => self.skip_block(b'[', b']'),
            b'"' | b'\'' => {
                let _ = self.concat_strings()?;
                Ok(())
            }
            _ => {
                let _ = self.number_or_ident()?;
                Ok(())
            }
        }
    }

    fn skip_block(&mut self, open: u8, close: u8) -> Result<(), ParseError> {
        if self.peek() != open {
            return Err(ParseError::new("expected block"));
        }
        self.pos += 1;
        let mut depth = 1;
        while self.pos < self.src.len() && depth > 0 {
            let c = self.peek();
            if c == b'"' || c == b'\'' {
                let _ = self.string_bytes()?;
                continue;
            }
            if c == b'#' {
                self.ws();
                continue;
            }
            self.pos += 1;
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
            }
        }
        if depth != 0 {
            return Err(ParseError::new("unterminated block"));
        }
        Ok(())
    }

    fn optional_separator(&mut self) -> Result<(), ParseError> {
        self.ws();
        if self.peek() == b',' || self.peek() == b';' {
            let sep = self.peek();
            self.pos += 1;
            self.ws();
            if self.peek() == sep {
                return Err(ParseError::new("duplicate field separator"));
            }
        }
        Ok(())
    }

    fn try_consume_sep_only(&mut self) -> Result<bool, ParseError> {
        self.ws();
        if self.peek() == b',' || self.peek() == b';' {
            return Err(ParseError::new("unexpected separator"));
        }
        Ok(false)
    }

    fn ws(&mut self) {
        skip_ws(self.src, &mut self.pos);
    }

    fn peek(&self) -> u8 {
        peek_at(self.src, self.pos)
    }
}

fn push_utf8_cp(out: &mut Vec<u8>, cp: u32) -> Result<(), ParseError> {
    if (0xd800..=0xdfff).contains(&cp) {
        return Err(ParseError::new("unicode surrogate"));
    }
    if cp > 0x10ffff {
        return Err(ParseError::new("unicode too large"));
    }
    let Some(ch) = char::from_u32(cp) else {
        return Err(ParseError::new("invalid unicode"));
    };
    let mut buf = [0u8; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    Ok(())
}

fn normalize_type_url(raw: &str) -> Result<String, ParseError> {
    let mut cleaned = String::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\n' | b'\r' | b'\t' => i += 1,
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {
                cleaned.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    let mut i = 0;
    let b = cleaned.as_bytes();
    let mut out = String::new();
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(ParseError::new("bad percent escape"));
            }
            let h = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if u8::from_str_radix(h, 16).is_err() {
                return Err(ParseError::new("bad percent escape"));
            }
            out.push('%');
            out.push(b[i + 1] as char);
            out.push(b[i + 2] as char);
            i += 3;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    if !out.contains('/') {
        return Err(ParseError::new("any type url missing slash"));
    }
    Ok(out)
}

fn is_hex_or_octal_int(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    if t.starts_with("0x") || t.starts_with("0X") {
        return true;
    }
    t.len() > 1
        && t.starts_with('0')
        && t.bytes().all(|c| c.is_ascii_digit())
        && !t.contains('.')
        && !t.contains('e')
        && !t.contains('E')
}

fn split_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        (false, s)
    }
}

fn parse_mag(s: &str) -> Result<u128, ParseError> {
    if s.is_empty() {
        return Err(ParseError::new("empty number"));
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u128::from_str_radix(hex, 16).map_err(|_| ParseError::new("bad hex"));
    }
    if s.len() > 1 && s.starts_with('0') && s.bytes().all(|c| c.is_ascii_digit()) {
        return u128::from_str_radix(s, 8).map_err(|_| ParseError::new("bad octal"));
    }
    s.parse::<u128>()
        .map_err(|_| ParseError::new("bad integer"))
}

fn parse_i32(s: &str) -> Result<i32, ParseError> {
    let (neg, rest) = split_sign(s);
    let mag = parse_mag(rest)?;
    if !neg {
        i32::try_from(mag).map_err(|_| ParseError::new("int32 overflow"))
    } else if mag == 1 << 31 {
        Ok(i32::MIN)
    } else {
        i32::try_from(mag)
            .map(|n| -n)
            .map_err(|_| ParseError::new("int32 overflow"))
    }
}

fn parse_i64(s: &str) -> Result<i64, ParseError> {
    let (neg, rest) = split_sign(s);
    let mag = parse_mag(rest)?;
    if !neg {
        i64::try_from(mag).map_err(|_| ParseError::new("int64 overflow"))
    } else if mag == 1u128 << 63 {
        Ok(i64::MIN)
    } else {
        i64::try_from(mag)
            .map(|n| -n)
            .map_err(|_| ParseError::new("int64 overflow"))
    }
}

fn parse_u32(s: &str) -> Result<u32, ParseError> {
    let (neg, rest) = split_sign(s);
    if neg {
        return Err(ParseError::new("negative uint"));
    }
    u32::try_from(parse_mag(rest)?).map_err(|_| ParseError::new("uint32 overflow"))
}

fn parse_u64(s: &str) -> Result<u64, ParseError> {
    let (neg, rest) = split_sign(s);
    if neg {
        return Err(ParseError::new("negative uint"));
    }
    u64::try_from(parse_mag(rest)?).map_err(|_| ParseError::new("uint64 overflow"))
}

fn strip_float_suffix(s: &str) -> &str {
    s.strip_suffix(['f', 'F']).unwrap_or(s)
}

fn parse_special_float(s: &str) -> Option<f64> {
    let (neg, rest) = split_sign(s);
    let r = rest.to_ascii_lowercase();
    if r == "nan" {
        return Some(f64::NAN);
    }
    if r == "inf" || r == "infinity" {
        return Some(if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    None
}

fn parse_f64(s: &str) -> Result<f64, ParseError> {
    if let Some(v) = parse_special_float(s) {
        return Ok(v);
    }
    let t = strip_float_suffix(s);
    if let Some(v) = parse_special_float(t) {
        return Ok(v);
    }
    match t.parse::<f64>() {
        Ok(v) => Ok(v),
        Err(_) => huge_float(t),
    }
}

fn parse_f32(s: &str) -> Result<f32, ParseError> {
    if let Some(v) = parse_special_float(s) {
        return Ok(v as f32);
    }
    let t = strip_float_suffix(s);
    if let Some(v) = parse_special_float(t) {
        return Ok(v as f32);
    }
    match t.parse::<f32>() {
        Ok(v) => Ok(v),
        Err(_) => Ok(huge_float(t)? as f32),
    }
}

fn huge_float(s: &str) -> Result<f64, ParseError> {
    let (neg, rest) = split_sign(s);
    let lower = rest.to_ascii_lowercase();
    if let Some(idx) = lower.find('e') {
        let exp = &lower[idx + 1..];
        let exp_neg = exp.starts_with('-');
        if exp_neg {
            return Ok(if neg { -0.0 } else { 0.0 });
        }
        return Ok(if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    Err(ParseError::owned(format!("bad float {s}")))
}
