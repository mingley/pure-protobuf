//! MX-01 split of `super`: text (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, FieldDescriptor, FieldType, MessageDescriptor};

pub(crate) fn text_write_named(f: &FieldDescriptor, name: &str, expr: &str, view: bool) -> String {
    match f.field_type {
        FieldType::Bool => format!("pbrs::text::write_named_bool(out, indent, {name}, {expr})"),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            format!("pbrs::text::write_named_int32(out, indent, {name}, {expr})")
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            format!("pbrs::text::write_named_int64(out, indent, {name}, {expr})")
        }
        FieldType::Uint32 | FieldType::Fixed32 => {
            format!("pbrs::text::write_named_uint32(out, indent, {name}, {expr})")
        }
        FieldType::Uint64 | FieldType::Fixed64 => {
            format!("pbrs::text::write_named_uint64(out, indent, {name}, {expr})")
        }
        FieldType::Float => format!("pbrs::text::write_named_float(out, indent, {name}, {expr})"),
        FieldType::Double => format!("pbrs::text::write_named_double(out, indent, {name}, {expr})"),
        FieldType::String => {
            format!("pbrs::text::write_named_string(out, indent, {name}, {expr}.as_bytes())")
        }
        FieldType::Bytes if view => {
            format!("pbrs::text::write_named_string(out, indent, {name}, {expr}.as_bytes())")
        }
        FieldType::Bytes => {
            format!("pbrs::text::write_named_string(out, indent, {name}, {expr})")
        }
        FieldType::Enum => {
            let n = if view {
                expr.to_string()
            } else {
                format!("i32::from({expr})")
            };
            format!(
                "{{ let n = {n}; pbrs::text::write_named_enum(out, indent, {name}, {}, n); }}",
                enum_name_from_i32(f, "n")
            )
        }
        _ => format!("pbrs::text::write_named_int32(out, indent, {name}, {expr})"),
    }
}

pub(crate) fn text_write_value(f: &FieldDescriptor, expr: &str, view: bool) -> String {
    match f.field_type {
        FieldType::Bool => format!("out.push_str(if {expr} {{ \"true\" }} else {{ \"false\" }})"),
        FieldType::Float => format!("pbrs::text::write_float_lit(out, {expr})"),
        FieldType::Double => format!("pbrs::text::write_double_lit(out, {expr})"),
        FieldType::String => format!("pbrs::text::write_bytes_lit({expr}.as_bytes(), out)"),
        FieldType::Bytes if view => format!("pbrs::text::write_bytes_lit({expr}.as_bytes(), out)"),
        FieldType::Bytes => format!("pbrs::text::write_bytes_lit({expr}, out)"),
        FieldType::Enum => {
            let n = if view {
                expr.to_string()
            } else {
                format!("i32::from({expr})")
            };
            format!(
                "{{ let n = {n}; pbrs::text::write_enum_lit(out, {}, n); }}",
                enum_name_from_i32(f, "n")
            )
        }
        FieldType::Int32
        | FieldType::Sint32
        | FieldType::Sfixed32
        | FieldType::Int64
        | FieldType::Sint64
        | FieldType::Sfixed64
        | FieldType::Uint32
        | FieldType::Fixed32
        | FieldType::Uint64
        | FieldType::Fixed64 => format!("out.push_str(&{expr}.to_string())"),
        _ => format!("out.push_str(&{expr}.to_string())"),
    }
}

pub(crate) fn text_decode_leaf(f: &FieldDescriptor, val: &str) -> String {
    match f.field_type {
        FieldType::Bool => format!("{val}.as_bool()?"),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => format!("{val}.as_i32()?"),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => format!("{val}.as_i64()?"),
        FieldType::Uint32 | FieldType::Fixed32 => format!("{val}.as_u32()?"),
        FieldType::Uint64 | FieldType::Fixed64 => format!("{val}.as_u64()?"),
        FieldType::Float => format!("{val}.as_f32()?"),
        FieldType::Double => format!("{val}.as_f64()?"),
        FieldType::String => format!("{val}.as_str()?"),
        FieldType::Bytes => format!("{val}.as_bytes()?"),
        FieldType::Enum => format!("{val}.as_enum({})?", enum_lookup_closure(f)),
        FieldType::Message | FieldType::Group => {
            format!("{}::from_text_value({val}.as_message()?)?", scalar_type(f))
        }
    }
}

pub(crate) fn text_map_key_write(ty: FieldType, k: &str) -> String {
    match ty {
        FieldType::String => format!("pbrs::text::write_bytes_lit({k}.as_bytes(), out)"),
        FieldType::Bool => format!("out.push_str(if {k} {{ \"true\" }} else {{ \"false\" }})"),
        _ => format!("out.push_str(&{k}.to_string())"),
    }
}

pub(crate) fn text_map_key_default(ty: FieldType) -> &'static str {
    match ty {
        FieldType::String => "\"\"",
        FieldType::Bool => "false",
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => "0i64",
        FieldType::Uint32 | FieldType::Fixed32 => "0u32",
        FieldType::Uint64 | FieldType::Fixed64 => "0u64",
        _ => "0i32",
    }
}

pub(crate) fn text_map_val_default(f: &FieldDescriptor) -> String {
    match f.field_type {
        FieldType::String => "\"\"".into(),
        FieldType::Bytes => "Vec::<u8>::new()".into(),
        FieldType::Bool => "false".into(),
        FieldType::Float => "0.0f32".into(),
        FieldType::Double => "0.0f64".into(),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => "0i64".into(),
        FieldType::Uint32 | FieldType::Fixed32 => "0u32".into(),
        FieldType::Uint64 | FieldType::Fixed64 => "0u64".into(),
        FieldType::Message | FieldType::Group => format!("{}::new()", scalar_type(f)),
        _ => "0i32".into(),
    }
}

pub(crate) fn emit_typed_text(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn to_text(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        let mut out = String::new();");
    let _ = writeln!(src, "        self.write_text(&mut out, 0)?;");
    let _ = writeln!(src, "        Ok(out)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn to_text_with_unknown(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        let mut out = String::new();");
    let _ = writeln!(src, "        self.write_text(&mut out, 0)?;");
    let _ = writeln!(
        src,
        "        pbrs::text::write_unknown_fields(&self.unknown, &mut out, 0);"
    );
    let _ = writeln!(src, "        Ok(out)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_text(text: &str) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(
        src,
        "        Self::from_text_value(&pbrs::text::parse(text)?)"
    );
    let _ = writeln!(src, "    }}");
    emit_write_text(src, desc);
    emit_from_text_value(src, desc);
}

pub(crate) fn emit_write_text(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn write_text(&self, out: &mut String, indent: usize) -> Result<(), SerializeError> {{"
    );
    for f in desc.fields.values() {
        let id = field_id(f);
        let m = field_raw(f);
        let name = rust_str(&f.name);
        let getter = format!("self.{id}()");
        if f.is_map {
            let kt = map_key_ty(f);
            let vf = map_value_field(f);
            let value_is_msg = vf.is_some_and(|v| v.field_type == FieldType::Message);
            let sort = if kt == FieldType::String {
                "items.sort_by(|(a, _), (b, _)| a.as_bytes().cmp(b.as_bytes()))"
            } else {
                "items.sort_by(|(a, _), (b, _)| a.cmp(b))"
            };
            let _ = writeln!(src, "        if !self.{id}().is_empty() {{");
            let _ = writeln!(
                src,
                "            let mut items: Vec<_> = self.{id}().iter().collect();"
            );
            let _ = writeln!(src, "            {sort};");
            let _ = writeln!(src, "            for (k, v) in items {{");
            let key_write = text_map_key_write(kt, "k");
            if value_is_msg {
                let _ = writeln!(
                    src,
                    "                pbrs::text::write_map_entry(out, indent, {name}, |out| {{ {key_write}; }}, |out| {{"
                );
                let _ = writeln!(src, "                    out.push_str(\"{{\\n\");");
                let _ = writeln!(src, "                    v.write_text(out, indent + 4)?;");
                let _ = writeln!(src, "                    pbrs::text::pad(out, indent + 2);");
                let _ = writeln!(src, "                    out.push_str(\"}}\\n\");");
                let _ = writeln!(src, "                    Ok(())");
                let _ = writeln!(src, "                }}, true)?;");
            } else {
                let val_write = vf
                    .map(|v| text_write_value(v, "v", true))
                    .unwrap_or_else(|| "out.push_str(&v.to_string())".into());
                let _ = writeln!(
                    src,
                    "                pbrs::text::write_map_entry(out, indent, {name}, |out| {{ {key_write}; }}, |out| {{ {val_write}; Ok(()) }}, false)?;"
                );
            }
            let _ = writeln!(src, "            }}");
            let _ = writeln!(src, "        }}");
        } else if f.cardinality == Cardinality::Repeated {
            if f.field_type == FieldType::Message {
                let _ = writeln!(src, "        for item in self.{id}() {{");
                let _ = writeln!(src, "            pbrs::text::pad(out, indent);");
                let _ = writeln!(src, "            out.push_str({name});");
                let _ = writeln!(src, "            out.push_str(\" {{\\n\");");
                let _ = writeln!(src, "            item.write_text(out, indent + 2)?;");
                let _ = writeln!(src, "            pbrs::text::pad(out, indent);");
                let _ = writeln!(src, "            out.push_str(\"}}\\n\");");
                let _ = writeln!(src, "        }}");
            } else {
                let _ = writeln!(src, "        for item in self.{id}() {{");
                let _ = writeln!(
                    src,
                    "            {};",
                    text_write_named(f, &name, "item", true)
                );
                let _ = writeln!(src, "        }}");
            }
        } else if f.field_type == FieldType::Message {
            let _ = writeln!(src, "        if self.has_{m}() {{");
            let _ = writeln!(src, "            pbrs::text::pad(out, indent);");
            let _ = writeln!(src, "            out.push_str({name});");
            let _ = writeln!(src, "            out.push_str(\" {{\\n\");");
            let _ = writeln!(src, "            self.{id}().write_text(out, indent + 2)?;");
            let _ = writeln!(src, "            pbrs::text::pad(out, indent);");
            let _ = writeln!(src, "            out.push_str(\"}}\\n\");");
            let _ = writeln!(src, "        }}");
        } else {
            let _ = writeln!(src, "        if {} {{", typed_present_expr(f, &getter));
            let _ = writeln!(
                src,
                "            {};",
                text_write_named(f, &name, &getter, false)
            );
            let _ = writeln!(src, "        }}");
        }
    }
    let _ = writeln!(src, "        Ok(())");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_from_text_value(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn from_text_value(fields: &[(String, pbrs::text::TextValue)]) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let mut msg = Self::new();");
    let _ = writeln!(src, "        for (key, val) in fields {{");
    let _ = writeln!(src, "            match key.as_str() {{");
    for f in desc.fields.values() {
        let id = field_id(f);
        let m = field_raw(f);
        let name = rust_str(&f.name);
        let _ = writeln!(src, "                {name} => {{");
        if f.is_map {
            let kt = map_key_ty(f);
            let vf = map_value_field(f);
            let key_decode = match kt {
                FieldType::String => "ev.as_str()?".to_string(),
                FieldType::Bool => "ev.as_bool()?".to_string(),
                FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
                    "ev.as_i64()?".to_string()
                }
                FieldType::Uint32 | FieldType::Fixed32 => "ev.as_u32()?".to_string(),
                FieldType::Uint64 | FieldType::Fixed64 => "ev.as_u64()?".to_string(),
                _ => "ev.as_i32()?".to_string(),
            };
            let val_decode = match vf {
                Some(v) if v.field_type == FieldType::Bytes => "ev.as_bytes()?.to_vec()".into(),
                Some(v) => text_decode_leaf(v, "ev"),
                None => "ev.as_i32()?".into(),
            };
            let _ = writeln!(src, "                    let entry = val.as_message()?;");
            let _ = writeln!(
                src,
                "                    let mut k = {};",
                text_map_key_default(kt)
            );
            let _ = writeln!(
                src,
                "                    let mut v = {};",
                vf.map(text_map_val_default)
                    .unwrap_or_else(|| "0i32".into())
            );
            let _ = writeln!(src, "                    for (ek, ev) in entry {{");
            let _ = writeln!(src, "                        match ek.as_str() {{");
            let _ = writeln!(
                src,
                "                            \"key\" => k = {key_decode},"
            );
            let _ = writeln!(
                src,
                "                            \"value\" => v = {val_decode},"
            );
            let _ = writeln!(
                src,
                "                            _ => return Err(ParseError::owned(format!(\"unknown field {{ek}}\"))),"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }}");
            let _ = writeln!(src, "                    msg.{id}_mut().insert(k, v);");
        } else if f.cardinality == Cardinality::Repeated {
            let item = text_decode_leaf(f, "item");
            let one = text_decode_leaf(f, "val");
            let _ = writeln!(
                src,
                "                    if let Some(items) = val.as_list() {{"
            );
            let _ = writeln!(src, "                        for item in items {{");
            let _ = writeln!(
                src,
                "                            msg.{id}_mut().push({item});"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }} else {{");
            let _ = writeln!(src, "                        msg.{id}_mut().push({one});");
            let _ = writeln!(src, "                    }}");
        } else if f.field_type == FieldType::Message {
            let ty = scalar_type(f);
            let _ = writeln!(
                src,
                "                    msg.{id}_mut().merge_from({ty}::from_text_value(val.as_message()?)?);"
            );
        } else {
            let _ = writeln!(
                src,
                "                    msg.set_{m}({});",
                text_decode_leaf(f, "val")
            );
        }
        let _ = writeln!(src, "                }}");
    }
    let _ = writeln!(
        src,
        "                _ => return Err(ParseError::owned(format!(\"unknown field {{key}}\"))),"
    );
    let _ = writeln!(src, "            }}");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "        Ok(msg)");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_dynamic_text(src: &mut String, full_name: &str) {
    let _ = writeln!(
        src,
        "    pub fn to_text(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(
        src,
        "        let b = pbrs::Serialize::serialize(self).map_err(|_| SerializeError::new(\"text\"))?;"
    );
    let _ = writeln!(src, "        let pool = generated_pool();");
    let _ = writeln!(
        src,
        "        let desc = pool.get_message(\"{full_name}\").ok_or_else(|| SerializeError::new(\"missing desc\"))?;"
    );
    let _ = writeln!(
        src,
        "        pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), &b).map_err(|_| SerializeError::new(\"text\"))?.to_text()"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn to_text_with_unknown(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(
        src,
        "        let b = pbrs::Serialize::serialize(self).map_err(|_| SerializeError::new(\"text\"))?;"
    );
    let _ = writeln!(src, "        let pool = generated_pool();");
    let _ = writeln!(
        src,
        "        let desc = pool.get_message(\"{full_name}\").ok_or_else(|| SerializeError::new(\"missing desc\"))?;"
    );
    let _ = writeln!(
        src,
        "        pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), &b).map_err(|_| SerializeError::new(\"text\"))?.to_text_with_unknown()"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_text(text: &str) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let pool = generated_pool();");
    let _ = writeln!(
        src,
        "        let desc = pool.get_message(\"{full_name}\").ok_or_else(|| ParseError::owned(\"missing desc\".into()))?;"
    );
    let _ = writeln!(
        src,
        "        let d = pbrs::DynamicMessage::from_text_with_pool(desc, Some(pool), text)?;"
    );
    let _ = writeln!(
        src,
        "        let b = pbrs::Serialize::serialize(&d).map_err(|e| ParseError::owned(e.to_string()))?;"
    );
    let _ = writeln!(src, "        <Self as pbrs::Parse>::parse(&b)");
    let _ = writeln!(src, "    }}");
}
