//! MX-01 split of `super`: parse (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, FieldDescriptor, FieldType, MessageDescriptor};
use std::path::PathBuf;

pub(crate) fn emit_codec(
    src: &mut String,
    desc: &MessageDescriptor,
    edition2024: bool,
    cold: ColdPlacement,
) {
    let required: Vec<_> = desc
        .fields
        .values()
        .filter(|f| f.cardinality == Cardinality::Required)
        .collect();
    let _ = writeln!(
        src,
        "    #[inline(always)] pub fn check_required(&self) -> Result<(), ParseError> {{"
    );
    for f in &required {
        let id = field_id(f);
        if is_option(f) {
            let _ = writeln!(
                src,
                "        if self.{id}.is_none() {{ return Err(ParseError::new(\"missing required field\")); }}"
            );
        }
    }
    let _ = writeln!(src, "        Ok(())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn merge_bytes(&mut self, data: &[u8], depth: u32) -> Result<(), ParseError> {{ if data.is_empty() {{ return self.check_required(); }} let mut pos = 0; let mut wire = None; self.merge_inner(data, &mut wire, &mut pos, depth, true, None) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn merge_bytes_dont_enforce(&mut self, data: &[u8], depth: u32) -> Result<(), ParseError> {{ if data.is_empty() {{ return Ok(()); }} let mut pos = 0; let mut wire = None; self.merge_inner(data, &mut wire, &mut pos, depth, false, None) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn merge_group(&mut self, data: &[u8], wire: &mut Option<pbrs::rt::Wire>, pos: &mut usize, num: u32, depth: u32) -> Result<(), ParseError> {{ self.merge_inner(data, wire, pos, depth, false, Some(num)) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn merge_inner(&mut self, data: &[u8], wire: &mut Option<pbrs::rt::Wire>, pos: &mut usize, depth: u32, enforce: bool, until: Option<u32>) -> Result<(), ParseError> {{"
    );
    let _ = writeln!(
        src,
        "        if depth > pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }} let _ = enforce;"
    );
    let _ = writeln!(src, "        self.cached_size.dirty();");
    let lights: Vec<_> = desc
        .fields
        .values()
        .filter(|f| is_light_merge_field(f))
        .collect();
    let heavies: Vec<_> = desc
        .fields
        .values()
        .filter(|f| !is_light_merge_field(f))
        .collect();
    let split_heavy =
        !desc.message_set_wire_format && (1..=4).contains(&lights.len()) && !heavies.is_empty();
    let _ = writeln!(src, "        while *pos < data.len() {{");
    let _ = writeln!(
        src,
        "            let (n, w) = pbrs::rt::decode_tag(data, pos)?;"
    );
    let _ = writeln!(
        src,
        "            if let Some(g) = until {{ if w == pbrs::rt::WIRE_EGROUP {{ if n != g {{ return Err(ParseError::new(\"mismatched end-group\")); }} return Ok(()); }} }}"
    );
    if split_heavy {
        for f in &lights {
            let _ = writeln!(src, "            if n == {} {{", f.number);
            let _ = writeln!(src, "                match w {{");
            emit_merge_arm(src, desc, f, edition2024, cold);
            let _ = writeln!(
                src,
                "                    _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
            );
            let _ = writeln!(src, "                }}");
            let _ = writeln!(src, "                continue;");
            let _ = writeln!(src, "            }}");
        }
        let _ = writeln!(
            src,
            "            self.merge_heavy(n, w, data, wire, pos, depth)?;"
        );
    } else {
        let _ = writeln!(src, "            match n {{");
        if desc.message_set_wire_format {
            emit_message_set_merge(src, desc);
        }
        for f in desc.fields.values() {
            if desc.message_set_wire_format && f.number == 1 {
                continue;
            }
            let _ = writeln!(src, "            {} => match w {{", f.number);
            emit_merge_arm(src, desc, f, edition2024, cold);
            let _ = writeln!(
                src,
                "                _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
            );
            let _ = writeln!(src, "            }}");
        }
        let _ = writeln!(
            src,
            "                _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
        );
        let _ = writeln!(src, "            }}");
    }
    let _ = writeln!(src, "        }}");
    let _ = writeln!(
        src,
        "        if until.is_some() {{ return Err(ParseError::new(\"truncated group\")); }}"
    );
    let _ = writeln!(src, "        if enforce {{ self.check_required()?; }}");
    let _ = writeln!(src, "        Ok(())");
    let _ = writeln!(src, "    }}");
    if split_heavy {
        let _ = writeln!(src, "    #[inline(never)]");
        let _ = writeln!(
            src,
            "    fn merge_heavy(&mut self, n: u32, w: u32, data: &[u8], wire: &mut Option<pbrs::rt::Wire>, pos: &mut usize, depth: u32) -> Result<(), ParseError> {{"
        );
        let _ = writeln!(src, "        match n {{");
        for f in &heavies {
            let _ = writeln!(src, "            {} => match w {{", f.number);
            emit_merge_arm(src, desc, f, edition2024, cold);
            let _ = writeln!(
                src,
                "                _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
            );
            let _ = writeln!(src, "            }}");
        }
        let _ = writeln!(
            src,
            "            _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "        Ok(())");
        let _ = writeln!(src, "    }}");
    }

    let _ = writeln!(
        src,
        "    pub fn validate_inner(wire: &pbrs::rt::Wire, pos: &mut usize, depth: u32) -> Result<(), ParseError> {{ Self::validate_until(wire, pos, depth, None) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn validate_until(wire: &pbrs::rt::Wire, pos: &mut usize, depth: u32, until: Option<u32>) -> Result<(), ParseError> {{"
    );
    let _ = writeln!(
        src,
        "        if depth > pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }}"
    );
    let _ = writeln!(src, "        let data = wire.as_slice();");
    if !required.is_empty() {
        let _ = writeln!(src, "        let mut seen = 0u64;");
    }
    let _ = writeln!(src, "        while *pos < data.len() {{");
    let _ = writeln!(
        src,
        "            let (n, w) = pbrs::rt::decode_tag(data, pos)?;"
    );
    let _ = writeln!(
        src,
        "            if let Some(g) = until {{ if w == pbrs::rt::WIRE_EGROUP {{ if n != g {{ return Err(ParseError::new(\"mismatched end-group\")); }} return Ok(()); }} }}"
    );
    let _ = writeln!(src, "            match n {{");
    if desc.message_set_wire_format {
        let _ = writeln!(src, "            1 => match w {{");
        let _ = writeln!(
            src,
            "                pbrs::rt::WIRE_LEN => {{ let inner = pbrs::rt::read_len_bytes(data, pos)?; let mut ip = 0; while ip < inner.len() {{ let (_, ww) = pbrs::rt::decode_tag(inner, &mut ip)?; pbrs::rt::skip_field_with_depth(inner, &mut ip, ww, depth + 1)?; }} }}"
        );
        let _ = writeln!(
            src,
            "                _ => pbrs::rt::skip_field_with_depth(data, pos, w, depth)?,"
        );
        let _ = writeln!(src, "            }}");
    }
    for f in desc.fields.values() {
        if desc.message_set_wire_format && f.number == 1 {
            continue;
        }
        let bit = required.iter().position(|r| r.number == f.number);
        let _ = writeln!(src, "            {} => match w {{", f.number);
        emit_validate_arm(src, f, bit);
        let _ = writeln!(
            src,
            "                _ => pbrs::rt::skip_field_with_depth(data, pos, w, depth)?,"
        );
        let _ = writeln!(src, "            }}");
    }
    let _ = writeln!(
        src,
        "                _ => pbrs::rt::skip_field_with_depth(data, pos, w, depth)?,"
    );
    let _ = writeln!(src, "            }}");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(
        src,
        "        if until.is_some() {{ return Err(ParseError::new(\"truncated group\")); }}"
    );
    if !required.is_empty() {
        let all = (1u64 << required.len()) - 1;
        let _ = writeln!(
            src,
            "        if seen != {all} {{ return Err(ParseError::new(\"missing required field\")); }}"
        );
    }
    let _ = writeln!(src, "        Ok(())");
    let _ = writeln!(src, "    }}");

    let _ = writeln!(src, "    pub fn compute_size(&self) -> u64 {{");
    let _ = writeln!(
        src,
        "        if let Some(n) = self.cached_size.get() {{ return n; }}"
    );
    let _ = writeln!(src, "        let mut n = self.unknown.encoded_len();");
    if desc.message_set_wire_format {
        emit_message_set_size(src, desc);
    } else {
        for f in desc.fields.values().filter(|f| cold.stored_hot(f)) {
            emit_size(src, f, "self");
        }
        if cold.uses_cold_storage() {
            let _ = writeln!(src, "        if let Some(c) = self.cold.as_deref() {{");
            for f in desc.fields.values().filter(|f| cold.stored_cold(f)) {
                emit_size(src, f, "c");
            }
            let _ = writeln!(src, "        }}");
        }
    }
    let _ = writeln!(src, "        self.cached_size.set(n);");
    let _ = writeln!(src, "        n");
    let _ = writeln!(src, "    }}");

    let _ = writeln!(
        src,
        "    pub fn write_to(&self, out: &mut impl pbrs::rt::WireOut) {{"
    );
    if desc.message_set_wire_format {
        emit_message_set_write(src, desc);
    } else if cold.uses_cold_storage() {
        // Keep the wire order independent of the storage layout.  Duplicate
        // the hot-field emission in the two arms so encoding still pays for
        // only one cold-storage presence check.
        let _ = writeln!(src, "        if let Some(c) = self.cold.as_deref() {{");
        for f in desc.fields.values() {
            emit_write(src, f, if cold.stored_cold(f) { "c" } else { "self" });
        }
        let _ = writeln!(src, "        }} else {{");
        for f in desc.fields.values().filter(|f| cold.stored_hot(f)) {
            emit_write(src, f, "self");
        }
        let _ = writeln!(src, "        }}");
    } else {
        for f in desc.fields.values() {
            emit_write(src, f, "self");
        }
    }
    let _ = writeln!(src, "        self.unknown.encode(out);");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_message_set_merge(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "            1 => match w {{ pbrs::rt::WIRE_SGROUP | pbrs::rt::WIRE_LEN => {{"
    );
    let _ = writeln!(
        src,
        "                    let mut type_id = 0u32; let mut payload: Vec<u8> = Vec::new();"
    );
    let _ = writeln!(
        src,
        "                    if w == pbrs::rt::WIRE_LEN {{ let inner = pbrs::rt::read_len_bytes(data, pos)?; let mut p = 0; while p < inner.len() {{ let (n, ww) = pbrs::rt::decode_tag(inner, &mut p)?; match (n, ww) {{ (2, pbrs::rt::WIRE_VARINT) => type_id = pbrs::rt::decode_varint(inner, &mut p)? as u32, (3, pbrs::rt::WIRE_LEN) => payload = pbrs::rt::read_len_bytes(inner, &mut p)?.to_vec(), _ => pbrs::rt::skip_field_with_depth(inner, &mut p, ww, depth + 1)?, }} }} }} else {{ loop {{ let (n, ww) = pbrs::rt::decode_tag(data, pos)?; if ww == pbrs::rt::WIRE_EGROUP && n == 1 {{ break; }} match (n, ww) {{ (2, pbrs::rt::WIRE_VARINT) => type_id = pbrs::rt::decode_varint(data, pos)? as u32, (3, pbrs::rt::WIRE_LEN) => payload = pbrs::rt::read_len_bytes(data, pos)?.to_vec(), _ => pbrs::rt::skip_field_with_depth(data, pos, ww, depth + 1)?, }} }} }}"
    );
    let _ = writeln!(src, "                    match type_id {{");
    for f in desc.fields.values() {
        let id = field_id(f);
        let t = scalar_type(f);
        let num = f.number;
        if is_lazy_msg(f) {
            let _ = writeln!(
                src,
                "                        {num} => {{ if self.{id}.is_some() {{ self.{id}.get_or_insert().merge_bytes(&payload, depth + 1)?; }} else {{ let mut inner = {t}::default(); inner.merge_bytes(&payload, depth + 1)?; self.{id} = pbrs::rt::LazyMsg::from_owned(inner); }} }}"
            );
        } else {
            let _ = writeln!(
                src,
                "                        {num} => {{ match &mut self.{id} {{ Some(existing) => existing.merge_bytes(&payload, depth + 1)?, None => {{ let mut inner = {t}::default(); inner.merge_bytes(&payload, depth + 1)?; self.{id} = Some(Box::new(inner)); }} }} }}"
            );
        }
    }
    let _ = writeln!(
        src,
        "                        _ => {{ let mut u = pbrs::UnknownFields::default(); u.fields.push(pbrs::rt::UnknownField::Varint {{ number: 2, value: u64::from(type_id) }}); u.fields.push(pbrs::rt::UnknownField::LengthDelimited {{ number: 3, value: payload }}); self.unknown.fields.push(pbrs::rt::UnknownField::Group {{ number: 1, fields: u }}); }}"
    );
    let _ = writeln!(src, "                    }}");
    let _ = writeln!(src, "                }}");
    let _ = writeln!(
        src,
        "                _ => self.unknown.fields.push(pbrs::rt::capture_unknown_with_depth(data, pos, n, w, depth)?),"
    );
    let _ = writeln!(src, "            }}");
}

pub(crate) fn emit_message_set_size(src: &mut String, desc: &MessageDescriptor) {
    for f in desc.fields.values() {
        let id = field_id(f);
        let num = f.number;
        let _ = writeln!(
            src,
            "        if let Some(m) = self.{id}.as_deref() {{ let inner = m.compute_size(); let item = pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len({num} as u64) + pbrs::rt::key_len_value_len(3, inner); n += pbrs::rt::tag_len(1, pbrs::rt::WIRE_SGROUP) + item + pbrs::rt::tag_len(1, pbrs::rt::WIRE_EGROUP); }}"
        );
    }
}

pub(crate) fn emit_message_set_write(src: &mut String, desc: &MessageDescriptor) {
    for f in desc.fields.values() {
        let id = field_id(f);
        let num = f.number;
        let _ = writeln!(
            src,
            "        if let Some(m) = self.{id}.as_deref() {{ pbrs::rt::encode_tag(out, 1, pbrs::rt::WIRE_SGROUP); pbrs::rt::encode_tag(out, 2, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, {num} as u64); pbrs::rt::encode_len_header(out, 3, m.compute_size()); m.write_to(out); pbrs::rt::encode_tag(out, 1, pbrs::rt::WIRE_EGROUP); }}"
        );
    }
}

pub(crate) fn read_scalar_expr(ty: FieldType, buf: &str, pos: &str) -> String {
    match ty {
        FieldType::Double => format!("f64::from_bits(pbrs::rt::read_fixed64({buf}, {pos})?)"),
        FieldType::Float => format!("f32::from_bits(pbrs::rt::read_fixed32({buf}, {pos})?)"),
        FieldType::Fixed64 => format!("pbrs::rt::read_fixed64({buf}, {pos})?"),
        FieldType::Sfixed64 => format!("pbrs::rt::read_fixed64({buf}, {pos})? as i64"),
        FieldType::Fixed32 => format!("pbrs::rt::read_fixed32({buf}, {pos})?"),
        FieldType::Sfixed32 => format!("pbrs::rt::read_fixed32({buf}, {pos})? as i32"),
        FieldType::Sint32 => {
            format!("pbrs::rt::decode_zigzag32(pbrs::rt::decode_varint({buf}, {pos})?)")
        }
        FieldType::Sint64 => {
            format!("pbrs::rt::decode_zigzag64(pbrs::rt::decode_varint({buf}, {pos})?)")
        }
        FieldType::Bool => format!("pbrs::rt::decode_varint({buf}, {pos})? != 0"),
        FieldType::Int64 => format!("pbrs::rt::decode_varint({buf}, {pos})? as i64"),
        FieldType::Uint64 => format!("pbrs::rt::decode_varint({buf}, {pos})?"),
        FieldType::Uint32 => format!("pbrs::rt::decode_varint({buf}, {pos})? as u32"),
        _ => format!("pbrs::rt::decode_varint({buf}, {pos})? as i32"),
    }
}

pub(crate) fn emit_validate_arm(src: &mut String, f: &FieldDescriptor, req_bit: Option<usize>) {
    let num = f.number;
    let mark = if let Some(b) = req_bit {
        format!(" seen |= 1 << {b};")
    } else {
        String::new()
    };
    if f.is_map {
        let _ = writeln!(src, "                pbrs::rt::WIRE_LEN => {{");
        let _ = writeln!(
            src,
            "                    if depth >= pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }}"
        );
        let _ = writeln!(
            src,
            "                    let entry_depth = depth + 1; let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut ip = 0; let w = wire.window(s, e); let d = w.as_slice();"
        );
        if matches!(map_val_ty(f), FieldType::Message | FieldType::Group) {
            let (_, value_type) = map_kv(f);
            let _ = writeln!(
                src,
                "                    while ip < d.len() {{ let (nn, ww) = pbrs::rt::decode_tag(d, &mut ip)?; match (nn, ww) {{ (2, pbrs::rt::WIRE_LEN) => {{ if entry_depth >= pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }} let (vs, ve) = pbrs::rt::read_len_span(d, &mut ip)?; let mut vp = 0; {value_type}::validate_inner(&w.window(vs, ve), &mut vp, entry_depth + 1)?; }}, _ => pbrs::rt::skip_field_with_depth(d, &mut ip, ww, entry_depth)?, }} }}{mark}"
            );
        } else {
            let _ = writeln!(
                src,
                "                    while ip < d.len() {{ let (_, ww) = pbrs::rt::decode_tag(d, &mut ip)?; pbrs::rt::skip_field_with_depth(d, &mut ip, ww, entry_depth)?; }}{mark}"
            );
        }
        let _ = writeln!(src, "                }}");
        return;
    }
    if f.cardinality == Cardinality::Repeated {
        if f.field_type == FieldType::String {
            let utf = if f.utf8_validate {
                "pbrs::rt::require_utf8(&data[s..e])?;"
            } else {
                ""
            };
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {utf}{mark} }}"
            );
        } else if f.field_type == FieldType::Bytes {
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_LEN => {{ pbrs::rt::read_len_span(data, pos)?;{mark} }}"
            );
        } else if f.field_type == FieldType::Message {
            let t = scalar_type(f);
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut ip = 0; {t}::validate_inner(&wire.window(s, e), &mut ip, depth + 1)?;{mark} }}"
            );
        } else if f.field_type == FieldType::Group || f.delimited {
            let t = scalar_type(f);
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_SGROUP => {{ {t}::validate_until(wire, pos, depth + 1, Some({num}))?;{mark} }}"
            );
        } else if f.field_type.is_packable() {
            let packed_ty = packed_storage_ty(f);
            let unpacked = read_scalar_expr(f.field_type, "data", "pos");
            let packed_wire = wire_const(f.field_type);
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {packed_ty}::validate_bytes(&data[s..e])?;{mark} }}"
            );
            let _ = writeln!(
                src,
                "                {packed_wire} => {{ let _ = {unpacked};{mark} }}"
            );
        } else {
            let unpacked = read_scalar_expr(f.field_type, "data", "pos");
            let w = wire_const(f.field_type);
            let _ = writeln!(
                src,
                "                {w} => {{ let _ = {unpacked};{mark} }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Message {
        let t = scalar_type(f);
        if f.field_type == FieldType::Group || f.delimited {
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_SGROUP => {{ {t}::validate_until(wire, pos, depth + 1, Some({num}))?;{mark} }}"
            );
        } else {
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut ip = 0; {t}::validate_inner(&wire.window(s, e), &mut ip, depth + 1)?;{mark} }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Group || f.delimited {
        let t = scalar_type(f);
        let _ = writeln!(
            src,
            "                pbrs::rt::WIRE_SGROUP => {{ {t}::validate_until(wire, pos, depth + 1, Some({num}))?;{mark} }}"
        );
        return;
    }
    if f.field_type == FieldType::String {
        let utf = if f.utf8_validate {
            "pbrs::rt::require_utf8(&data[s..e])?;"
        } else {
            ""
        };
        let _ = writeln!(
            src,
            "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {utf}{mark} }}"
        );
        return;
    }
    if f.field_type == FieldType::Bytes {
        let _ = writeln!(
            src,
            "                pbrs::rt::WIRE_LEN => {{ pbrs::rt::read_len_span(data, pos)?;{mark} }}"
        );
        return;
    }
    let expr = read_scalar_expr(f.field_type, "data", "pos");
    let w = wire_const(f.field_type);
    let _ = writeln!(src, "                {w} => {{ let _ = {expr};{mark} }}");
}

pub(crate) fn lazy_str_from_parse(f: &FieldDescriptor) -> &'static str {
    if f.utf8_validate {
        "pbrs::rt::LazyStr::from_parse_span(wire, data, s, e)?"
    } else {
        "pbrs::rt::LazyStr::from_parse_span_unchecked(wire, data, s, e)"
    }
}

/// Same-tag run for repeated length-delimited items (strings/bytes).
/// Reserve from remaining bytes so `tags_32` does not grow the Vec 5×.
pub(crate) fn emit_repeated_len_run(src: &mut String, num: u32, st: &str, push: &str) {
    let _ = writeln!(
        src,
        "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; let rest = data.len().saturating_sub(e); if rest > 2 {{ {st}.reserve(1 + rest / (e - s + 2).max(1)); }} {st}.push({push}); while *pos < data.len() {{ let save = *pos; match pbrs::rt::decode_tag(data, pos) {{ Ok((n2, w2)) if n2 == {num} && w2 == pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {st}.push({push}); }} Ok(_) => {{ *pos = save; break; }} Err(e) => return Err(e), }} }} }}"
    );
}

pub(crate) fn emit_merge_arm(
    src: &mut String,
    desc: &MessageDescriptor,
    f: &FieldDescriptor,
    edition2024: bool,
    cold: ColdPlacement,
) {
    let st = cold.store_mut(f);
    let num = f.number;
    if f.is_map {
        let _ = writeln!(src, "                pbrs::rt::WIRE_LEN => {{");
        let _ = writeln!(
            src,
            "                    if depth >= pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }}"
        );
        let _ = writeln!(
            src,
            "                    let (s, e) = pbrs::rt::read_len_span(data, pos)?;"
        );
        let _ = writeln!(
            src,
            "                    let (kk, vv) = decode_map_entry_{}_{}_{num}(&pbrs::rt::Wire::ensure(wire, data).window(s, e), depth + 1)?;",
            rust_ident(&desc.full_name).replace("r#", ""),
            field_id(f).replace("r#", "")
        );
        let _ = writeln!(src, "                    {st}.push_entry(kk, vv);");
        let _ = writeln!(src, "                }}");
        return;
    }
    if f.cardinality == Cardinality::Repeated {
        if f.field_type == FieldType::String {
            emit_repeated_len_run(src, num, &st, lazy_str_from_parse(f));
        } else if f.field_type == FieldType::Bytes {
            emit_repeated_len_run(
                src,
                num,
                &st,
                "pbrs::rt::LazyBytes::from_parse_span(wire, data, s, e)",
            );
        } else if f.field_type == FieldType::Message || f.field_type == FieldType::Group {
            let t = scalar_type(f);
            if f.field_type == FieldType::Group || f.delimited {
                let _ = writeln!(
                    src,
                    "                pbrs::rt::WIRE_SGROUP => {{ let mut inner = {t}::default(); inner.merge_group(data, wire, pos, {num}, depth + 1)?; {st}.push(inner); }}"
                );
            } else {
                let _ = writeln!(
                    src,
                    "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; let rest = data.len().saturating_sub(e); if rest > 2 {{ {st}.reserve(1 + rest / (e - s + 4).max(1)); }} let mut inner = {t}::default(); let mut ip = 0; let mut sw = None; inner.merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; {st}.push(inner); while *pos < data.len() {{ let save = *pos; match pbrs::rt::decode_tag(data, pos) {{ Ok((n2, w2)) if n2 == {num} && w2 == pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; let mut inner = {t}::default(); let mut ip = 0; let mut sw = None; inner.merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; {st}.push(inner); }} Ok(_) => {{ *pos = save; break; }} Err(e) => return Err(e), }} }} }}"
                );
            }
        } else if f.field_type.is_packable() {
            let unpacked = read_scalar_expr(f.field_type, "data", "pos");
            let packed_wire = wire_const(f.field_type);
            if f.packed {
                if is_memcpy_packed(f) {
                    // Payload-only Arc. Do not Arc the parent message and do
                    // not eager-decode the Vec on parse.
                    let _ = writeln!(
                        src,
                        "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {st}.append_wire(pbrs::rt::Wire::from_slice(&data[s..e]))?; }}"
                    );
                } else {
                    let _ = writeln!(
                        src,
                        "                pbrs::rt::WIRE_LEN => {{ let (s, e) = pbrs::rt::read_len_span(data, pos)?; {st}.append_wire(pbrs::rt::Wire::ensure(wire, data).window(s, e))?; }}"
                    );
                }
            } else {
                let expr = read_scalar_expr(f.field_type, "p", "&mut i");
                let _ = writeln!(
                    src,
                    "                pbrs::rt::WIRE_LEN => {{ let p = pbrs::rt::read_len_bytes(data, pos)?; let mut i = 0; while i < p.len() {{ {st}.push({expr}); }} }}"
                );
            }
            let _ = writeln!(
                src,
                "                {packed_wire} => {{ {st}.push({unpacked}); let rest = data.len().saturating_sub(*pos); if rest > 2 {{ {st}.reserve(rest / 3); }} while *pos < data.len() {{ let save = *pos; match pbrs::rt::decode_tag(data, pos) {{ Ok((n2, w2)) if n2 == {num} && w2 == {packed_wire} => {st}.push({unpacked}), Ok(_) => {{ *pos = save; break; }} Err(e) => return Err(e), }} }} }}"
            );
        } else {
            let unpacked = read_scalar_expr(f.field_type, "data", "pos");
            let w = wire_const(f.field_type);
            let _ = writeln!(src, "                {w} => {st}.push({unpacked}),");
        }
        return;
    }
    if f.field_type == FieldType::Message || f.field_type == FieldType::Group {
        let t = scalar_type(f);
        if f.field_type == FieldType::Group || f.delimited {
            let _ = writeln!(src, "                pbrs::rt::WIRE_SGROUP => {{");
            emit_oneof_clear(src, desc, f, cold);
            let _ = writeln!(
                src,
                "                    match &mut {st} {{ Some(existing) => existing.merge_group(data, wire, pos, {num}, depth + 1)?, None => {{ let mut inner = {t}::default(); inner.merge_group(data, wire, pos, {num}, depth + 1)?; {st} = Some(Box::new(inner)); }} }}"
            );
            let _ = writeln!(src, "                    }}");
        } else {
            let _ = writeln!(src, "                pbrs::rt::WIRE_LEN => {{");
            emit_oneof_clear(src, desc, f, cold);
            let _ = writeln!(
                src,
                "                    let (s, e) = pbrs::rt::read_len_span(data, pos)?;"
            );
            if is_lazy_msg(f) {
                let _ = writeln!(
                    src,
                    "                    if {st}.is_some() {{ let mut ip = 0; let mut sw = None; {st}.get_or_insert().merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; }} else {{ let mut ip = 0; {t}::validate_inner(&pbrs::rt::Wire::ensure(wire, data).window(s, e), &mut ip, depth + 1)?; {st} = pbrs::rt::LazyMsg::from_wire(pbrs::rt::Wire::ensure(wire, data).window(s, e)); }}"
                );
            } else {
                let _ = writeln!(
                    src,
                    "                    match &mut {st} {{ Some(existing) => {{ let mut ip = 0; let mut sw = None; existing.merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; }} None => {{ let mut inner = {t}::default(); let mut ip = 0; let mut sw = None; inner.merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; {st} = Some(Box::new(inner)); }} }}"
                );
            }
            let _ = writeln!(src, "                    }}");
        }
        return;
    }
    if f.field_type == FieldType::String {
        let parse = lazy_str_from_parse(f);
        let assign = if is_option(f) {
            format!("{st} = Some(Box::new({parse}))")
        } else {
            format!("{st} = {parse}")
        };
        let _ = writeln!(src, "                pbrs::rt::WIRE_LEN => {{");
        emit_oneof_clear(src, desc, f, cold);
        let _ = writeln!(
            src,
            "                    let (s, e) = pbrs::rt::read_len_span(data, pos)?; {assign};"
        );
        let _ = writeln!(src, "                    }}");
        return;
    }
    if f.field_type == FieldType::Bytes {
        let assign = if is_option(f) {
            format!("{st} = Some(Box::new(pbrs::rt::LazyBytes::from_parse_span(wire, data, s, e)))")
        } else {
            format!("{st} = pbrs::rt::LazyBytes::from_parse_span(wire, data, s, e)")
        };
        let _ = writeln!(src, "                pbrs::rt::WIRE_LEN => {{");
        emit_oneof_clear(src, desc, f, cold);
        let _ = writeln!(
            src,
            "                    let (s, e) = pbrs::rt::read_len_span(data, pos)?; {assign};"
        );
        let _ = writeln!(src, "                    }}");
        return;
    }
    if edition2024 && f.field_type == FieldType::Enum {
        if let Some(en) = f.enum_ty.as_ref().filter(|en| en.closed) {
            let known = en
                .values
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" | ");
            let is_known = if known.is_empty() {
                "false".to_string()
            } else {
                format!("matches!(value as i32, {known})")
            };
            let assign = if is_option(f) {
                format!("{st} = Some(value as i32)")
            } else {
                format!("{st} = value as i32")
            };
            let _ = writeln!(
                src,
                "                pbrs::rt::WIRE_VARINT => {{ let value = pbrs::rt::decode_varint(data, pos)?; if {is_known} {{"
            );
            emit_oneof_clear(src, desc, f, cold);
            let _ = writeln!(src, "                    {assign};");
            let _ = writeln!(
                src,
                "                }} else {{ self.unknown.fields.push(pbrs::rt::UnknownField::Varint {{ number: {num}, value }}); }} }}"
            );
            return;
        }
    }
    let expr = read_scalar_expr(f.field_type, "data", "pos");
    let w = wire_const(f.field_type);
    let assign = if is_option(f) && f.field_type == FieldType::Bool {
        format!("{st} = pbrs::rt::OptBool::some({expr})")
    } else if is_option(f) {
        format!("{st} = Some({expr})")
    } else {
        format!("{st} = {expr}")
    };
    let _ = writeln!(src, "                {w} => {{");
    emit_oneof_clear(src, desc, f, cold);
    let _ = writeln!(src, "                    {assign};");
    let _ = writeln!(src, "                    }}");
}

pub(crate) fn emit_map_decoders(
    src: &mut String,
    desc: &MessageDescriptor,
    edition2024: bool,
) -> Result<(), CodegenError> {
    bind_field_idents(desc);
    let msg = rust_ident(&desc.full_name).replace("r#", "");
    for f in desc.fields.values() {
        if !f.is_map {
            continue;
        }
        let (key_utf8, val_utf8) = if edition2024 {
            f.message
                .as_ref()
                .and_then(|entry| entry.field(1).zip(entry.field(2)))
                .map(|(key, val)| (key.utf8_validate, val.utf8_validate))
                .ok_or_else(|| CodegenError::MalformedDescriptor {
                    detail: format!(
                        "Edition 2024 map {}.{} has no resolved key/value entry",
                        desc.full_name, f.name
                    ),
                    path: Some(PathBuf::from(&desc.file_name)),
                })?
        } else {
            (f.utf8_validate, f.utf8_validate)
        };
        let id = field_id(f).replace("r#", "");
        let num = f.number;
        let (k, v) = map_kv(f);
        let kty = map_key_ty(f);
        let vty = map_val_ty(f);
        let message_value = matches!(vty, FieldType::Message | FieldType::Group);
        let _ = writeln!(
            src,
            "fn decode_map_entry_{msg}_{id}_{num}(wire: &pbrs::rt::Wire, depth: u32) -> Result<({k}, {v}), ParseError> {{"
        );
        let _ = writeln!(
            src,
            "    if depth > pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }}"
        );
        let _ = writeln!(
            src,
            "    let data = wire.as_slice(); let mut key = {k}::default();"
        );
        if message_value {
            // A present value has its own wire frame. Defer construction until
            // its guard passes; an omitted value still receives the old default.
            let _ = writeln!(src, "    let mut val: Option<{v}> = None;");
        } else {
            let _ = writeln!(src, "    let mut val = {v}::default();");
        }
        let _ = writeln!(src, "    let mut pos = 0;");
        let _ = writeln!(
            src,
            "    while pos < data.len() {{ let (n, w) = pbrs::rt::decode_tag(data, &mut pos)?; match (n, w) {{"
        );
        emit_map_scalar_decode(src, 1, "key", kty, key_utf8);
        if message_value {
            let _ = writeln!(
                src,
                "        (2, pbrs::rt::WIRE_LEN) => {{ if depth >= pbrs::RECURSION_LIMIT {{ return Err(ParseError::new(\"recursion limit exceeded\")); }} let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; let mut ip = 0; let mut sw = None; val.get_or_insert_with({v}::default).merge_inner(&data[s..e], &mut sw, &mut ip, depth + 1, true, None)?; }},"
            );
        } else {
            emit_map_scalar_decode(src, 2, "val", vty, val_utf8);
        }
        let _ = writeln!(
            src,
            "        _ => pbrs::rt::skip_field_with_depth(data, &mut pos, w, depth)?,"
        );
        let _ = writeln!(src, "    }} }}");
        if message_value {
            let _ = writeln!(src, "    Ok((key, val.unwrap_or_default()))");
        } else {
            let _ = writeln!(src, "    Ok((key, val))");
        }
        let _ = writeln!(src, "}}");
    }
    Ok(())
}

pub(crate) fn emit_map_scalar_decode(
    src: &mut String,
    n: u32,
    var: &str,
    ty: FieldType,
    utf8: bool,
) {
    match ty {
        FieldType::String => {
            if utf8 {
                let _ = writeln!(
                    src,
                    "        ({n}, pbrs::rt::WIRE_LEN) => {{ let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; pbrs::rt::require_utf8(&data[s..e])?; {var} = pbrs::rt::LazyStr::from_span(wire, s, e); }},"
                );
            } else {
                let _ = writeln!(
                    src,
                    "        ({n}, pbrs::rt::WIRE_LEN) => {{ let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; {var} = pbrs::rt::LazyStr::from_span(wire, s, e); }},"
                );
            }
        }
        FieldType::Bytes => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_LEN) => {{ let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; {var} = pbrs::rt::LazyBytes::from_wire_span(wire, s, e); }},"
            );
        }
        FieldType::Message | FieldType::Group => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_LEN) => {{ let (s, e) = pbrs::rt::read_len_span(data, &mut pos)?; let mut ip = 0; let mut sw = None; {var}.merge_inner(&data[s..e], &mut sw, &mut ip, depth, true, None)?; }},"
            );
        }
        FieldType::Float => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I32) => {var} = f32::from_bits(pbrs::rt::read_fixed32(data, &mut pos)?),"
            );
        }
        FieldType::Double => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I64) => {var} = f64::from_bits(pbrs::rt::read_fixed64(data, &mut pos)?),"
            );
        }
        FieldType::Fixed32 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I32) => {var} = pbrs::rt::read_fixed32(data, &mut pos)?,"
            );
        }
        FieldType::Sfixed32 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I32) => {var} = pbrs::rt::read_fixed32(data, &mut pos)? as i32,"
            );
        }
        FieldType::Fixed64 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I64) => {var} = pbrs::rt::read_fixed64(data, &mut pos)?,"
            );
        }
        FieldType::Sfixed64 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_I64) => {var} = pbrs::rt::read_fixed64(data, &mut pos)? as i64,"
            );
        }
        FieldType::Sint32 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_zigzag32(pbrs::rt::decode_varint(data, &mut pos)?),"
            );
        }
        FieldType::Sint64 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_zigzag64(pbrs::rt::decode_varint(data, &mut pos)?),"
            );
        }
        FieldType::Bool => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_varint(data, &mut pos)? != 0,"
            );
        }
        FieldType::Uint64 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_varint(data, &mut pos)?,"
            );
        }
        FieldType::Int64 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_varint(data, &mut pos)? as i64,"
            );
        }
        FieldType::Uint32 => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_varint(data, &mut pos)? as u32,"
            );
        }
        _ => {
            let _ = writeln!(
                src,
                "        ({n}, pbrs::rt::WIRE_VARINT) => {var} = pbrs::rt::decode_varint(data, &mut pos)? as i32,"
            );
        }
    }
}
