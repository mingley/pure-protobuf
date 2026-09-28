//! MX-01 split of `super`: encode (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, FieldDescriptor, FieldType};

pub(crate) fn wire_const(ty: FieldType) -> &'static str {
    match ty {
        FieldType::Fixed64 | FieldType::Sfixed64 | FieldType::Double => "pbrs::rt::WIRE_I64",
        FieldType::Fixed32 | FieldType::Sfixed32 | FieldType::Float => "pbrs::rt::WIRE_I32",
        FieldType::String | FieldType::Bytes | FieldType::Message | FieldType::Group => {
            "pbrs::rt::WIRE_LEN"
        }
        _ => "pbrs::rt::WIRE_VARINT",
    }
}

pub(crate) fn implicit_present(path: &str, ty: FieldType) -> String {
    match ty {
        FieldType::Bool => path.to_string(),
        FieldType::Float | FieldType::Double => format!("{path}.to_bits() != 0"),
        FieldType::String | FieldType::Bytes => format!("!{path}.is_empty()"),
        _ => format!("{path} != 0"),
    }
}

pub(crate) fn map_key_ty(field: &FieldDescriptor) -> FieldType {
    field
        .message
        .as_ref()
        .and_then(|e| e.field(1))
        .map(|f| f.field_type)
        .unwrap_or(FieldType::String)
}

pub(crate) fn map_val_ty(field: &FieldDescriptor) -> FieldType {
    field
        .message
        .as_ref()
        .and_then(|e| e.field(2))
        .map(|f| f.field_type)
        .unwrap_or(FieldType::Int32)
}

pub(crate) fn packed_len_expr(v: &str, ty: FieldType) -> String {
    match ty {
        FieldType::Fixed64 | FieldType::Sfixed64 | FieldType::Double => "8".into(),
        FieldType::Fixed32 | FieldType::Sfixed32 | FieldType::Float => "4".into(),
        FieldType::Sint32 => {
            format!("pbrs::rt::varint_len(pbrs::rt::encode_zigzag32({v}))")
        }
        FieldType::Sint64 => {
            format!("pbrs::rt::varint_len(pbrs::rt::encode_zigzag64({v}))")
        }
        FieldType::Bool => format!("pbrs::rt::varint_len(u64::from({v}))"),
        _ => format!("pbrs::rt::varint_len(({v}) as u64)"),
    }
}

pub(crate) fn write_packed_stmt(out: &str, v: &str, ty: FieldType) -> String {
    match ty {
        FieldType::Double => format!("({out}).extend_from_slice(&({v}).to_bits().to_le_bytes())"),
        FieldType::Float => format!("({out}).extend_from_slice(&({v}).to_bits().to_le_bytes())"),
        FieldType::Fixed64 => format!("({out}).extend_from_slice(&({v}).to_le_bytes())"),
        FieldType::Sfixed64 => format!("({out}).extend_from_slice(&(({v}) as u64).to_le_bytes())"),
        FieldType::Fixed32 => format!("({out}).extend_from_slice(&({v}).to_le_bytes())"),
        FieldType::Sfixed32 => format!("({out}).extend_from_slice(&(({v}) as u32).to_le_bytes())"),
        FieldType::Sint32 => {
            format!("pbrs::rt::encode_varint({out}, pbrs::rt::encode_zigzag32({v}))")
        }
        FieldType::Sint64 => {
            format!("pbrs::rt::encode_varint({out}, pbrs::rt::encode_zigzag64({v}))")
        }
        FieldType::Bool => format!("pbrs::rt::encode_varint({out}, u64::from({v}))"),
        _ => format!("pbrs::rt::encode_varint({out}, {v} as u64)"),
    }
}

pub(crate) fn emit_size(src: &mut String, f: &FieldDescriptor, p: &str) {
    let id = field_id(f);
    let fld = format!("{p}.{id}");
    let num = f.number;
    if f.is_map {
        let _ = writeln!(src, "        if !{fld}.is_empty() {{");
        let _ = writeln!(src, "        for (k, v) in {fld}.pairs() {{");
        let key_sz = map_key_size(map_key_ty(f), "k");
        let val_sz = map_val_size(map_val_ty(f), "v");
        let _ = writeln!(src, "            let inner = {key_sz} + {val_sz};");
        let _ = writeln!(
            src,
            "            n += pbrs::rt::key_len_value_len({num}, inner);"
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "        }}");
        return;
    }
    if f.cardinality == Cardinality::Repeated {
        if f.field_type == FieldType::String || f.field_type == FieldType::Bytes {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ n += pbrs::rt::key_len_value_len({num}, t.as_bytes().len() as u64); }}"
            );
        } else if f.field_type == FieldType::Group || f.delimited {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ n += pbrs::rt::tag_len({num}, pbrs::rt::WIRE_SGROUP) + t.compute_size() + pbrs::rt::tag_len({num}, pbrs::rt::WIRE_EGROUP); }}"
            );
        } else if f.field_type == FieldType::Message {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ n += pbrs::rt::key_len_value_len({num}, t.compute_size()); }}"
            );
        } else if f.packed && f.field_type.is_packable() {
            let plen = packed_len_expr("*t", f.field_type);
            let _ = writeln!(
                src,
                "        if let Some(p) = {fld}.packed_bytes() {{ n += pbrs::rt::key_len_value_len({num}, p.len() as u64); }} else if !{fld}.is_empty() {{ let mut payload = 0u64; for t in {fld}.iter() {{ payload += {plen}; }} n += pbrs::rt::key_len_value_len({num}, payload); }}"
            );
        } else {
            let plen = packed_len_expr("*t", f.field_type);
            let w = wire_const(f.field_type);
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ n += pbrs::rt::tag_len({num}, {w}) + {plen}; }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Group || f.delimited {
        let _ = writeln!(
            src,
            "        if let Some(m) = &{fld} {{ n += pbrs::rt::tag_len({num}, pbrs::rt::WIRE_SGROUP) + m.compute_size() + pbrs::rt::tag_len({num}, pbrs::rt::WIRE_EGROUP); }}"
        );
        return;
    }
    if f.field_type == FieldType::Message {
        if is_lazy_msg(f) {
            let _ = writeln!(
                src,
                "        if let Some(p) = {fld}.wire_bytes() {{ n += pbrs::rt::key_len_value_len({num}, p.len() as u64); }} else if let Some(m) = {fld}.as_deref() {{ n += pbrs::rt::key_len_value_len({num}, m.compute_size()); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "        if let Some(m) = &{fld} {{ n += pbrs::rt::key_len_value_len({num}, m.compute_size()); }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::String || f.field_type == FieldType::Bytes {
        if is_option(f) {
            let _ = writeln!(
                src,
                "        if let Some(s) = &{fld} {{ n += pbrs::rt::key_len_value_len({num}, s.as_bytes().len() as u64); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "        if !{fld}.is_empty() {{ n += pbrs::rt::key_len_value_len({num}, {fld}.as_bytes().len() as u64); }}"
            );
        }
        return;
    }
    let sz = packed_len_expr("v", f.field_type);
    let w = wire_const(f.field_type);
    if is_option(f) && f.field_type == FieldType::Bool {
        let _ = writeln!(
            src,
            "        if let Some(v) = {fld}.get() {{ n += pbrs::rt::tag_len({num}, {w}) + {sz}; }}"
        );
    } else if is_option(f) {
        let _ = writeln!(
            src,
            "        if let Some(v) = {fld} {{ n += pbrs::rt::tag_len({num}, {w}) + {sz}; }}"
        );
    } else {
        let pred = implicit_present(&fld, f.field_type);
        let sz2 = packed_len_expr(&fld, f.field_type);
        let _ = writeln!(
            src,
            "        if {pred} {{ n += pbrs::rt::tag_len({num}, {w}) + {sz2}; }}"
        );
    }
}

pub(crate) fn map_key_size(ty: FieldType, var: &str) -> String {
    match ty {
        FieldType::String => {
            format!("pbrs::rt::key_len_value_len(1, {var}.as_bytes().len() as u64)")
        }
        FieldType::Bool => format!(
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(u64::from(*{var}))"
        ),
        FieldType::Fixed32 | FieldType::Sfixed32 => {
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_I32) + 4".into()
        }
        FieldType::Fixed64 | FieldType::Sfixed64 => {
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_I64) + 8".into()
        }
        FieldType::Sint32 => format!(
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(pbrs::rt::encode_zigzag32(*{var}))"
        ),
        FieldType::Sint64 => format!(
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(pbrs::rt::encode_zigzag64(*{var}))"
        ),
        FieldType::Uint32 => format!(
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(u64::from(*{var}))"
        ),
        FieldType::Uint64 => {
            format!("pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(*{var})")
        }
        _ => format!(
            "pbrs::rt::tag_len(1, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len((*{var}) as u64)"
        ),
    }
}

pub(crate) fn map_val_size(ty: FieldType, var: &str) -> String {
    match ty {
        FieldType::String | FieldType::Bytes => {
            format!("pbrs::rt::key_len_value_len(2, {var}.as_bytes().len() as u64)")
        }
        FieldType::Message | FieldType::Group => {
            format!("pbrs::rt::key_len_value_len(2, {var}.compute_size())")
        }
        FieldType::Bool => {
            format!(
                "pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(u64::from(*{var}))"
            )
        }
        FieldType::Float => "pbrs::rt::tag_len(2, pbrs::rt::WIRE_I32) + 4".into(),
        FieldType::Double => "pbrs::rt::tag_len(2, pbrs::rt::WIRE_I64) + 8".into(),
        FieldType::Fixed32 | FieldType::Sfixed32 => {
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_I32) + 4".into()
        }
        FieldType::Fixed64 | FieldType::Sfixed64 => {
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_I64) + 8".into()
        }
        FieldType::Sint32 => format!(
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(pbrs::rt::encode_zigzag32(*{var}))"
        ),
        FieldType::Sint64 => format!(
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(pbrs::rt::encode_zigzag64(*{var}))"
        ),
        FieldType::Uint32 => format!(
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(u64::from(*{var}))"
        ),
        FieldType::Uint64 => {
            format!("pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len(*{var})")
        }
        _ => format!(
            "pbrs::rt::tag_len(2, pbrs::rt::WIRE_VARINT) + pbrs::rt::varint_len((*{var}) as u64)"
        ),
    }
}

pub(crate) fn emit_write(src: &mut String, f: &FieldDescriptor, p: &str) {
    let id = field_id(f);
    let fld = format!("{p}.{id}");
    let num = f.number;
    if f.is_map {
        let kty = map_key_ty(f);
        let vty = map_val_ty(f);
        let _ = writeln!(src, "        if !{fld}.is_empty() {{");
        let _ = writeln!(src, "        for (k, v) in {fld}.pairs() {{");
        let _ = writeln!(
            src,
            "            let inner = {} + {};",
            map_key_size(kty, "k"),
            map_val_size(vty, "v")
        );
        let _ = writeln!(
            src,
            "            pbrs::rt::encode_tag(out, {num}, pbrs::rt::WIRE_LEN); pbrs::rt::encode_varint(out, inner);"
        );
        emit_map_key_write(src, kty);
        emit_map_val_write(src, vty);
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "        }}");
        return;
    }
    if f.cardinality == Cardinality::Repeated {
        if f.field_type == FieldType::Bytes {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ pbrs::rt::encode_len_field_shared(out, {num}, t); }}"
            );
        } else if f.field_type == FieldType::String {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ pbrs::rt::encode_len_field(out, {num}, t.as_bytes()); }}"
            );
        } else if f.field_type == FieldType::Group || f.delimited {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ pbrs::rt::encode_tag(out, {num}, pbrs::rt::WIRE_SGROUP); t.write_to(out); pbrs::rt::encode_tag(out, {num}, pbrs::rt::WIRE_EGROUP); }}"
            );
        } else if f.field_type == FieldType::Message {
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ pbrs::rt::encode_len_header(out, {num}, t.compute_size()); t.write_to(out); }}"
            );
        } else if f.packed && f.field_type.is_packable() {
            let plen = packed_len_expr("*t", f.field_type);
            let stmt = write_packed_stmt("out", "*t", f.field_type);
            let _ = writeln!(
                src,
                "        if let Some(p) = {fld}.packed_bytes() {{ pbrs::rt::encode_len_header(out, {num}, p.len() as u64); out.extend_from_slice(p); }} else if !{fld}.is_empty() {{ let mut payload = 0u64; for t in {fld}.iter() {{ payload += {plen}; }} pbrs::rt::encode_len_header(out, {num}, payload); for t in {fld}.iter() {{ {stmt}; }} }}"
            );
        } else {
            let w = wire_const(f.field_type);
            let stmt = write_packed_stmt("out", "*t", f.field_type);
            let _ = writeln!(
                src,
                "        for t in {fld}.iter() {{ pbrs::rt::encode_tag(out, {num}, {w}); {stmt}; }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Group || f.delimited {
        let _ = writeln!(
            src,
            "        if let Some(m) = &{fld} {{ pbrs::rt::encode_tag(out, {num}, pbrs::rt::WIRE_SGROUP); m.write_to(out); pbrs::rt::encode_tag(out, {num}, pbrs::rt::WIRE_EGROUP); }}"
        );
        return;
    }
    if f.field_type == FieldType::Message {
        if is_lazy_msg(f) {
            let _ = writeln!(
                src,
                "        if let Some(p) = {fld}.wire_bytes() {{ pbrs::rt::encode_len_header(out, {num}, p.len() as u64); out.extend_from_slice(p); }} else if let Some(m) = {fld}.as_deref() {{ pbrs::rt::encode_len_header(out, {num}, m.compute_size()); m.write_to(out); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "        if let Some(m) = &{fld} {{ pbrs::rt::encode_len_header(out, {num}, m.compute_size()); m.write_to(out); }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::Bytes {
        // PK-11: large bytes fields share their buffer with segmented sinks.
        if is_option(f) {
            let _ = writeln!(
                src,
                "        if let Some(s) = &{fld} {{ pbrs::rt::encode_len_field_shared(out, {num}, s); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "        if !{fld}.is_empty() {{ pbrs::rt::encode_len_field_shared(out, {num}, &{fld}); }}"
            );
        }
        return;
    }
    if f.field_type == FieldType::String {
        if is_option(f) {
            let _ = writeln!(
                src,
                "        if let Some(s) = &{fld} {{ pbrs::rt::encode_len_field(out, {num}, s.as_bytes()); }}"
            );
        } else {
            let _ = writeln!(
                src,
                "        if !{fld}.is_empty() {{ pbrs::rt::encode_len_field(out, {num}, {fld}.as_bytes()); }}"
            );
        }
        return;
    }
    let w = wire_const(f.field_type);
    if is_option(f) && f.field_type == FieldType::Bool {
        let stmt = write_packed_stmt("out", "v", f.field_type);
        let _ = writeln!(
            src,
            "        if let Some(v) = {fld}.get() {{ pbrs::rt::encode_tag(out, {num}, {w}); {stmt}; }}"
        );
    } else if is_option(f) {
        let stmt = write_packed_stmt("out", "v", f.field_type);
        let _ = writeln!(
            src,
            "        if let Some(v) = {fld} {{ pbrs::rt::encode_tag(out, {num}, {w}); {stmt}; }}"
        );
    } else {
        let pred = implicit_present(&fld, f.field_type);
        let stmt = write_packed_stmt("out", &fld, f.field_type);
        let _ = writeln!(
            src,
            "        if {pred} {{ pbrs::rt::encode_tag(out, {num}, {w}); {stmt}; }}"
        );
    }
}

pub(crate) fn emit_map_key_write(src: &mut String, ty: FieldType) {
    emit_map_scalar_write(src, 1, "k", ty);
}

pub(crate) fn emit_map_val_write(src: &mut String, ty: FieldType) {
    emit_map_scalar_write(src, 2, "v", ty);
}

pub(crate) fn emit_map_scalar_write(src: &mut String, n: u32, var: &str, ty: FieldType) {
    match ty {
        FieldType::String | FieldType::Bytes => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_len_field(out, {n}, {var}.as_bytes());"
            );
        }
        FieldType::Message | FieldType::Group => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_len_header(out, {n}, {var}.compute_size()); {var}.write_to(out);"
            );
        }
        FieldType::Float => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I32); out.extend_from_slice(&{var}.to_bits().to_le_bytes());"
            );
        }
        FieldType::Double => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I64); out.extend_from_slice(&{var}.to_bits().to_le_bytes());"
            );
        }
        FieldType::Fixed32 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I32); out.extend_from_slice(&{var}.to_le_bytes());"
            );
        }
        FieldType::Sfixed32 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I32); out.extend_from_slice(&(*{var} as u32).to_le_bytes());"
            );
        }
        FieldType::Fixed64 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I64); out.extend_from_slice(&{var}.to_le_bytes());"
            );
        }
        FieldType::Sfixed64 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_I64); out.extend_from_slice(&(*{var} as u64).to_le_bytes());"
            );
        }
        FieldType::Sint32 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, pbrs::rt::encode_zigzag32(*{var}));"
            );
        }
        FieldType::Sint64 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, pbrs::rt::encode_zigzag64(*{var}));"
            );
        }
        FieldType::Bool => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, u64::from(*{var}));"
            );
        }
        FieldType::Uint64 => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, *{var});"
            );
        }
        _ => {
            let _ = writeln!(
                src,
                "            pbrs::rt::encode_tag(out, {n}, pbrs::rt::WIRE_VARINT); pbrs::rt::encode_varint(out, *{var} as u64);"
            );
        }
    }
}
