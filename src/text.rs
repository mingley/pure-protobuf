//! Protocol buffer text format (textproto).

use crate::dynamic::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, FieldValue,
    MapKeyValue, MessageDescriptor, Presence, Value, RECURSION_LIMIT,
};

use crate::error::{ParseError, SerializeError};
use crate::string::{ProtoBytes, ProtoString};
use crate::wire::UnknownField;
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

fn write_unknown(uf: &UnknownField, out: &mut String, indent: usize) {
    pad(out, indent);
    match uf {
        UnknownField::Varint { number, value } => {
            out.push_str(&format!("{number}: {value}\n"));
        }
        UnknownField::Fixed32 { number, value } => {
            out.push_str(&format!("{number}: 0x{value:08x}\n"));
        }
        UnknownField::Fixed64 { number, value } => {
            out.push_str(&format!("{number}: 0x{value:016x}\n"));
        }
        UnknownField::LengthDelimited { number, value } => {
            out.push_str(&format!("{number}: "));
            write_bytes_lit(value, out);
            out.push('\n');
        }
        UnknownField::Group { number, fields } => {
            out.push_str(&format!("{number} {{\n"));
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
        MapKeyValue::I32(n) => out.push_str(&n.to_string()),
        MapKeyValue::I64(n) => out.push_str(&n.to_string()),
        MapKeyValue::U32(n) => out.push_str(&n.to_string()),
        MapKeyValue::U64(n) => out.push_str(&n.to_string()),
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
        Value::Int32(n) => out.push_str(&n.to_string()),
        Value::Int64(n) => out.push_str(&n.to_string()),
        Value::Uint32(n) => out.push_str(&n.to_string()),
        Value::Uint64(n) => out.push_str(&n.to_string()),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::String(s) => write_bytes_lit(s.as_bytes(), out),
        Value::Bytes(b) => write_bytes_lit(b.as_bytes(), out),
        Value::Enum(n) => {
            if let Some(name) = field.enum_ty.as_ref().and_then(|e| e.values.get(n)) {
                out.push_str(name);
            } else {
                out.push_str(&n.to_string());
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
        out.push_str(&n.to_string());
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
        out.push_str(&n.to_string());
    }
}

fn write_bytes_lit(bytes: &[u8], out: &mut String) {
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
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('"');
}

fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    pool: Option<Arc<DescriptorPool>>,
    depth: u32,
}

impl Parser<'_> {
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
                let key = match entry.get_singular(1) {
                    Some(Value::String(s)) => MapKeyValue::String(s.clone()),
                    Some(Value::Int32(n)) => MapKeyValue::I32(*n),
                    Some(Value::Int64(n)) => MapKeyValue::I64(*n),
                    Some(Value::Uint32(n)) => MapKeyValue::U32(*n),
                    Some(Value::Uint64(n)) => MapKeyValue::U64(*n),
                    Some(Value::Bool(b)) => MapKeyValue::Bool(*b),
                    _ => MapKeyValue::String(ProtoString::new()),
                };
                let val = entry.get_singular(2).cloned().unwrap_or(Value::Int32(0));
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
            if is_hex_or_octal_int(&tok) {
                return Err(ParseError::new("hex/octal float not allowed"));
            }
            if field.field_type == FieldType::Float {
                return Ok(Value::Float(parse_f32(&tok)?));
            }
            return Ok(Value::Double(parse_f64(&tok)?));
        }
        let tok = self.number_or_ident()?;
        match field.field_type {
            FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
                Ok(Value::Int32(parse_i32(&tok)?))
            }
            FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
                Ok(Value::Int64(parse_i64(&tok)?))
            }
            FieldType::Uint32 | FieldType::Fixed32 => Ok(Value::Uint32(parse_u32(&tok)?)),
            FieldType::Uint64 | FieldType::Fixed64 => Ok(Value::Uint64(parse_u64(&tok)?)),
            FieldType::Message | FieldType::Group => Err(ParseError::new("expected { for message")),
            _ => Err(ParseError::new("unexpected scalar")),
        }
    }

    fn parse_bool(&mut self) -> Result<bool, ParseError> {
        let t = self.number_or_ident()?;
        match t.as_str() {
            "true" | "True" | "t" | "T" | "1" => Ok(true),
            "false" | "False" | "f" | "F" | "0" => Ok(false),
            _ => Err(ParseError::owned(format!("bad bool {t}"))),
        }
    }

    fn parse_enum(&mut self, field: &FieldDescriptor) -> Result<i32, ParseError> {
        self.ws();
        if self.peek().is_ascii_alphabetic() || self.peek() == b'_' {
            let name = self.ident()?;
            field
                .enum_ty
                .as_ref()
                .and_then(|e| e.names.get(&name).copied())
                .ok_or_else(|| ParseError::owned(format!("unknown enum {name}")))
        } else {
            let tok = self.number_or_ident()?;
            parse_i32(&tok)
        }
    }

    fn field_token(&mut self) -> Result<String, ParseError> {
        self.ws();
        if self.peek() == b'[' {
            return self.bracket_name();
        }
        self.ident()
    }

    fn bracket_name(&mut self) -> Result<String, ParseError> {
        if self.peek() != b'[' {
            return Err(ParseError::new("expected ["));
        }
        self.pos += 1;
        let mut out = String::from("[");
        loop {
            self.ws();
            if self.pos >= self.src.len() {
                return Err(ParseError::new("unterminated ["));
            }
            let c = self.peek();
            if c == b']' {
                self.pos += 1;
                out.push(']');
                return Ok(out);
            }
            if c == b'#' {
                self.ws();
                continue;
            }
            out.push(c as char);
            self.pos += 1;
        }
    }

    fn ident(&mut self) -> Result<String, ParseError> {
        self.ws();
        let start = self.pos;
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(ParseError::new("expected identifier"));
        }
        Ok(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
    }

    fn number_or_ident(&mut self) -> Result<String, ParseError> {
        self.ws();
        let start = self.pos;
        if self.peek() == b'+' || self.peek() == b'-' {
            self.pos += 1;
        }
        if self.peek().is_ascii_alphabetic() {
            while self.pos < self.src.len() {
                let c = self.src[self.pos];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            return Ok(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned());
        }
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            if c.is_ascii_alphanumeric() || c == b'.' || c == b'+' || c == b'_' || c == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(ParseError::new("expected number"));
        }
        Ok(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
    }

    fn concat_strings(&mut self) -> Result<Vec<u8>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.peek() != b'"' && self.peek() != b'\'' {
                break;
            }
            out.extend(self.string_bytes()?);
            self.ws();
            if self.peek() != b'"' && self.peek() != b'\'' {
                break;
            }
        }
        if out.is_empty() && self.peek() != b'"' && self.peek() != b'\'' {
            return Err(ParseError::new("expected string"));
        }
        Ok(out)
    }

    fn string_bytes(&mut self) -> Result<Vec<u8>, ParseError> {
        self.ws();
        let quote = self.peek();
        if quote != b'"' && quote != b'\'' {
            return Err(ParseError::new("expected string"));
        }
        self.pos += 1;
        let mut out = Vec::new();
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            if c == b'\n' {
                return Err(ParseError::new("string literal includes LF"));
            }
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            if self.pos >= self.src.len() {
                break;
            }
            let e = self.src[self.pos];
            self.pos += 1;
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
                    let (n, used) = self.read_hex(2)?;
                    if used == 0 {
                        return Err(ParseError::new("bad hex escape"));
                    }
                    out.push(n as u8);
                }
                b'u' => {
                    let (cp, used) = self.read_hex(4)?;
                    if used != 4 {
                        return Err(ParseError::new("bad unicode escape"));
                    }
                    push_utf8_cp(&mut out, cp)?;
                }
                b'U' => {
                    let (cp, used) = self.read_hex(8)?;
                    if used != 8 {
                        return Err(ParseError::new("bad unicode escape"));
                    }
                    push_utf8_cp(&mut out, cp)?;
                }
                b'0'..=b'7' => {
                    self.pos -= 1;
                    out.push(self.read_octal());
                }
                _ => out.push(e),
            }
        }
        Err(ParseError::new("unterminated string"))
    }

    fn read_hex(&mut self, max: usize) -> Result<(u32, usize), ParseError> {
        let mut n = 0u32;
        let mut used = 0;
        while used < max && self.pos < self.src.len() {
            let c = self.src[self.pos];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => break,
            };
            n = (n << 4) | u32::from(d);
            self.pos += 1;
            used += 1;
        }
        Ok((n, used))
    }

    fn read_octal(&mut self) -> u8 {
        let mut n = 0u8;
        let mut i = 0;
        while i < 3 && self.pos < self.src.len() {
            let c = self.src[self.pos];
            if !c.is_ascii_digit() || c > b'7' {
                break;
            }
            n = n.wrapping_mul(8).wrapping_add(c - b'0');
            self.pos += 1;
            i += 1;
        }
        n
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
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b' ' | b'\n' | b'\r' | b'\t' => self.pos += 1,
                b'#' => {
                    while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn peek(&self) -> u8 {
        self.src.get(self.pos).copied().unwrap_or(0)
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
