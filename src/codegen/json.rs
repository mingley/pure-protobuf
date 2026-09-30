//! MX-01 split of `super`: json (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, FieldDescriptor, FieldType, MessageDescriptor};

pub(crate) fn rust_str(s: &str) -> String {
    format!("\"{}\"", s.escape_default())
}

pub(crate) fn json_match_keys(f: &FieldDescriptor) -> String {
    let json = rust_str(&f.json_name);
    if f.json_name == f.name {
        json
    } else {
        format!("{} | {}", json, rust_str(&f.name))
    }
}

pub(crate) fn is_real_oneof(desc: &MessageDescriptor, f: &FieldDescriptor) -> bool {
    f.oneof_index
        .and_then(|i| desc.oneofs.get(i as usize))
        .is_some_and(|members| members.len() > 1)
}

pub(crate) fn typed_scalar(ty: FieldType) -> bool {
    matches!(
        ty,
        FieldType::Bool
            | FieldType::Int32
            | FieldType::Int64
            | FieldType::Uint32
            | FieldType::Uint64
            | FieldType::Sint32
            | FieldType::Sint64
            | FieldType::Fixed32
            | FieldType::Fixed64
            | FieldType::Sfixed32
            | FieldType::Sfixed64
            | FieldType::Float
            | FieldType::Double
            | FieldType::String
            | FieldType::Bytes
            | FieldType::Enum
    )
}

pub(crate) fn typed_map_key(ty: FieldType) -> bool {
    matches!(
        ty,
        FieldType::Bool
            | FieldType::Int32
            | FieldType::Int64
            | FieldType::Uint32
            | FieldType::Uint64
            | FieldType::Sint32
            | FieldType::Sint64
            | FieldType::Fixed32
            | FieldType::Fixed64
            | FieldType::Sfixed32
            | FieldType::Sfixed64
            | FieldType::String
    )
}

pub(crate) fn enum_supports_typed(f: &FieldDescriptor) -> bool {
    let Some(en) = f.enum_ty.as_ref() else {
        return false;
    };
    if en.closed {
        return false;
    }
    !en.full_name
        .trim_start_matches('.')
        .starts_with("google.protobuf.")
}

pub(crate) fn message_type_name(f: &FieldDescriptor) -> &str {
    f.type_name
        .as_deref()
        .or_else(|| f.message.as_ref().map(|m| m.full_name.as_str()))
        .unwrap_or("")
        .trim_start_matches('.')
}

pub(crate) fn is_fieldwise_wkt(name: &str) -> bool {
    matches!(
        name,
        "google.protobuf.Timestamp"
            | "google.protobuf.Duration"
            | "google.protobuf.Empty"
            | "google.protobuf.BoolValue"
            | "google.protobuf.Int32Value"
            | "google.protobuf.Int64Value"
            | "google.protobuf.UInt32Value"
            | "google.protobuf.UInt64Value"
            | "google.protobuf.FloatValue"
            | "google.protobuf.DoubleValue"
            | "google.protobuf.StringValue"
            | "google.protobuf.BytesValue"
            | "google.protobuf.FieldMask"
    )
}

/// Official proto3 JSON encode / decode helpers and the `value` getter expr.
pub(crate) fn wrapper_json_spec(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match name {
        "google.protobuf.BoolValue" => ("boolean", "as_bool", "self.value()"),
        "google.protobuf.Int32Value" => ("int32", "as_i32", "self.value()"),
        "google.protobuf.Int64Value" => ("int64", "as_i64", "self.value()"),
        "google.protobuf.UInt32Value" => ("uint32", "as_u32", "self.value()"),
        "google.protobuf.UInt64Value" => ("uint64", "as_u64", "self.value()"),
        "google.protobuf.FloatValue" => ("float", "as_f32", "self.value()"),
        "google.protobuf.DoubleValue" => ("double", "as_f64", "self.value()"),
        "google.protobuf.StringValue" => ("string", "as_str", "self.value().as_bytes()"),
        "google.protobuf.BytesValue" => ("bytes", "as_bytes", "self.value()"),
        _ => return None,
    })
}

pub(crate) fn field_supports_typed_json_message(f: &FieldDescriptor, depth: u32) -> bool {
    let name = message_type_name(f);
    if is_fieldwise_wkt(name) {
        return true;
    }
    if name.starts_with("google.protobuf.") {
        return false;
    }
    let Some(m) = f.message.as_ref() else {
        return false;
    };
    m.fields
        .values()
        .all(|nf| field_supports_typed_json(nf, depth + 1))
}

pub(crate) fn field_supports_typed_json(f: &FieldDescriptor, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    if f.is_map {
        if !typed_map_key(map_key_ty(f)) {
            return false;
        }
        let Some(entry) = f.message.as_ref() else {
            return false;
        };
        let Some(vf) = entry.field(2) else {
            return false;
        };
        return field_supports_typed_json(vf, depth + 1);
    }
    if f.cardinality == Cardinality::Repeated {
        return match f.field_type {
            FieldType::Message => field_supports_typed_json_message(f, depth),
            FieldType::Enum => enum_supports_typed(f),
            other => typed_scalar(other),
        };
    }
    match f.field_type {
        FieldType::Message => field_supports_typed_json_message(f, depth),
        FieldType::Enum => enum_supports_typed(f),
        other => typed_scalar(other),
    }
}

pub(crate) fn enum_name_from_i32(f: &FieldDescriptor, expr: &str) -> String {
    let Some(en) = f.enum_ty.as_ref() else {
        return "None".into();
    };
    let mut arms = String::new();
    for (n, name) in &en.values {
        let _ = write!(arms, "{n} => Some(\"{}\"), ", name.escape_default());
    }
    format!("match {expr} {{ {arms}_ => None }}")
}

pub(crate) fn enum_lookup_closure(f: &FieldDescriptor) -> String {
    let Some(en) = f.enum_ty.as_ref() else {
        return "|_| None".into();
    };
    let mut arms = String::new();
    for (name, n) in &en.names {
        let _ = write!(arms, "\"{}\" => Some({n}), ", name.escape_default());
    }
    format!("|s| match s {{ {arms}_ => None }}")
}

pub(crate) fn json_encode_leaf(f: &FieldDescriptor, expr: &str, view: bool) -> String {
    match f.field_type {
        FieldType::Bool => format!("pbrs::json::boolean({expr})"),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            format!("pbrs::json::int32({expr})")
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            format!("pbrs::json::int64({expr})")
        }
        FieldType::Uint32 | FieldType::Fixed32 => format!("pbrs::json::uint32({expr})"),
        FieldType::Uint64 | FieldType::Fixed64 => format!("pbrs::json::uint64({expr})"),
        FieldType::Float => format!("pbrs::json::float({expr})"),
        FieldType::Double => format!("pbrs::json::double({expr})"),
        FieldType::String => format!("pbrs::json::string({expr}.as_bytes())"),
        FieldType::Bytes if view => format!("pbrs::json::bytes({expr}.as_bytes())"),
        FieldType::Bytes => format!("pbrs::json::bytes({expr})"),
        FieldType::Enum => {
            let n = if view {
                expr.to_string()
            } else {
                format!("i32::from({expr})")
            };
            format!(
                "{{ let n = {n}; pbrs::json::enumeration({}, n) }}",
                enum_name_from_i32(f, "n")
            )
        }
        FieldType::Message | FieldType::Group => format!("{expr}.to_json_value()?"),
    }
}

pub(crate) fn json_decode_leaf(f: &FieldDescriptor, val: &str) -> String {
    match f.field_type {
        FieldType::Bool => format!("pbrs::json::as_bool({val})?"),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            format!("pbrs::json::as_i32({val})?")
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            format!("pbrs::json::as_i64({val})?")
        }
        FieldType::Uint32 | FieldType::Fixed32 => format!("pbrs::json::as_u32({val})?"),
        FieldType::Uint64 | FieldType::Fixed64 => format!("pbrs::json::as_u64({val})?"),
        FieldType::Float => format!("pbrs::json::as_f32({val})?"),
        FieldType::Double => format!("pbrs::json::as_f64({val})?"),
        FieldType::String => format!("pbrs::json::as_str({val})?"),
        FieldType::Bytes => format!("pbrs::json::as_bytes({val})?"),
        FieldType::Enum => format!(
            "pbrs::json::as_enum({val}, ignore, {})?",
            enum_lookup_closure(f)
        ),
        FieldType::Message | FieldType::Group => {
            format!("{}::from_json_value({val}, ignore)?", scalar_type(f))
        }
    }
}

pub(crate) fn json_map_key_encode(ty: FieldType, k: &str) -> String {
    if ty == FieldType::String {
        format!("String::from_utf8_lossy({k}.as_bytes()).into_owned()")
    } else {
        format!("{k}.to_string()")
    }
}

pub(crate) fn json_map_key_decode(ty: FieldType, k: &str) -> String {
    match ty {
        FieldType::String => format!("{k}.as_str()"),
        FieldType::Bool => format!("pbrs::json::map_key_bool({k})?"),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            format!("pbrs::json::map_key_i32({k})?")
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            format!("pbrs::json::map_key_i64({k})?")
        }
        FieldType::Uint32 | FieldType::Fixed32 => format!("pbrs::json::map_key_u32({k})?"),
        FieldType::Uint64 | FieldType::Fixed64 => format!("pbrs::json::map_key_u64({k})?"),
        _ => format!("{k}.as_str()"),
    }
}

pub(crate) fn typed_present_expr(f: &FieldDescriptor, getter: &str) -> String {
    if is_option(f) {
        return format!("self.has_{}()", field_raw(f));
    }
    match f.field_type {
        FieldType::Bool => getter.to_string(),
        FieldType::Float | FieldType::Double => format!("{getter}.to_bits() != 0"),
        FieldType::String => format!("!{getter}.as_bytes().is_empty()"),
        FieldType::Bytes => format!("!{getter}.is_empty()"),
        FieldType::Enum => format!("i32::from({getter}) != 0"),
        _ => format!("{getter} != 0"),
    }
}

pub(crate) fn map_value_field(f: &FieldDescriptor) -> Option<&FieldDescriptor> {
    f.message.as_ref().and_then(|e| e.field(2))
}

pub(crate) fn can_typed_json(desc: &MessageDescriptor) -> bool {
    desc.fields
        .values()
        .all(|f| field_supports_typed_json(f, 0))
}

/// Reject a second JSON member of the same real oneof (official proto3 JSON).
pub(crate) fn emit_json_oneof_guard(
    src: &mut String,
    desc: &MessageDescriptor,
    f: &FieldDescriptor,
) {
    if !is_real_oneof(desc, f) {
        return;
    }
    let Some(idx) = f.oneof_index else {
        return;
    };
    let Some(members) = desc.oneofs.get(idx as usize) else {
        return;
    };
    let mut sorted_members = members.clone();
    sorted_members.sort();
    for n in &sorted_members {
        if *n == f.number {
            continue;
        }
        let Some(sib) = desc.field(*n) else {
            continue;
        };
        let raw = field_raw(sib);
        let _ = writeln!(
            src,
            "                    if msg.has_{raw}() {{ return Err(ParseError::new(\"duplicate oneof member\")); }}"
        );
    }
}

pub(crate) fn emit_json_text(src: &mut String, desc: &MessageDescriptor) {
    let name = desc.full_name.trim_start_matches('.');
    if emit_json_enabled() {
        match name {
            "google.protobuf.Timestamp" => emit_wkt_string_json(src, "timestamp"),
            "google.protobuf.Duration" => emit_wkt_string_json(src, "duration"),
            "google.protobuf.Empty" => emit_wkt_empty_json(src),
            name if wrapper_json_spec(name).is_some() => {
                let (encode, decode, value_expr) = wrapper_json_spec(name).expect("wrapper");
                emit_wkt_wrapper_json(src, encode, decode, value_expr);
            }
            "google.protobuf.FieldMask" => emit_wkt_field_mask_json(src),
            name if name.starts_with("google.protobuf.") => {
                if emit_reflection_enabled() {
                    // Struct / Value / ListValue / Any keep official
                    // JSON via DynamicMessage. Field-wise object JSON for those
                    // would disagree with the official mapping.
                    emit_dynamic_json(src, &desc.full_name);
                }
            }
            _ if can_typed_json(desc) => emit_typed_json(src, desc),
            _ => {
                if emit_reflection_enabled() {
                    emit_dynamic_json(src, &desc.full_name);
                }
            }
        }
    }
    if emit_text_enabled() {
        match name {
            "google.protobuf.Timestamp" | "google.protobuf.Duration" | "google.protobuf.Empty" => {
                emit_typed_text(src, desc);
            }
            name if wrapper_json_spec(name).is_some() => emit_typed_text(src, desc),
            "google.protobuf.FieldMask" => emit_typed_text(src, desc),
            name if name.starts_with("google.protobuf.") => {
                if emit_reflection_enabled() {
                    emit_dynamic_text(src, &desc.full_name);
                }
            }
            _ if can_typed_json(desc) => emit_typed_text(src, desc),
            _ => {
                if emit_reflection_enabled() {
                    emit_dynamic_text(src, &desc.full_name);
                }
            }
        }
    }
}

/// Official proto3 JSON string mapping for Timestamp / Duration.
pub(crate) fn emit_wkt_string_json(src: &mut String, helper: &str) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(self.to_json_value()?.to_string())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let v = pbrs::json::parse(json)?;");
    let _ = writeln!(src, "        Self::from_json_value(&v, ignore)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn to_json_value(&self) -> Result<pbrs::json::Json, SerializeError> {{"
    );
    let _ = writeln!(
        src,
        "        pbrs::json::{helper}(self.seconds(), self.nanos())"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json_value(v: &pbrs::json::Json, _ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(
        src,
        "        let (seconds, nanos) = pbrs::json::as_{helper}(v)?;"
    );
    let _ = writeln!(src, "        let mut msg = Self::new();");
    let _ = writeln!(src, "        msg.set_seconds(seconds);");
    let _ = writeln!(src, "        msg.set_nanos(nanos);");
    let _ = writeln!(src, "        Ok(msg)");
    let _ = writeln!(src, "    }}");
}

/// Official proto3 JSON for Empty (`{}`).
pub(crate) fn emit_wkt_empty_json(src: &mut String) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(self.to_json_value()?.to_string())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let v = pbrs::json::parse(json)?;");
    let _ = writeln!(src, "        Self::from_json_value(&v, ignore)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn to_json_value(&self) -> Result<pbrs::json::Json, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(pbrs::json::empty())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json_value(v: &pbrs::json::Json, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        pbrs::json::as_empty(v, ignore)?;");
    let _ = writeln!(src, "        Ok(Self::new())");
    let _ = writeln!(src, "    }}");
}

/// Official proto3 JSON for a wrapper: the wrapped value, not an object.
pub(crate) fn emit_wkt_wrapper_json(
    src: &mut String,
    encode: &str,
    decode: &str,
    value_expr: &str,
) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(self.to_json_value()?.to_string())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let v = pbrs::json::parse(json)?;");
    let _ = writeln!(src, "        Self::from_json_value(&v, ignore)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn to_json_value(&self) -> Result<pbrs::json::Json, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(pbrs::json::{encode}({value_expr}))");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json_value(v: &pbrs::json::Json, _ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let mut msg = Self::new();");
    let _ = writeln!(src, "        msg.set_value(pbrs::json::{decode}(v)?);");
    let _ = writeln!(src, "        Ok(msg)");
    let _ = writeln!(src, "    }}");
}

/// Emit the allocation-light official FieldMask JSON mapping.
pub(crate) fn emit_wkt_field_mask_json(src: &mut String) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{ Ok(self.to_json_value()?.to_string()) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{ let v = pbrs::json::parse(json)?; Self::from_json_value(&v, ignore) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn to_json_value(&self) -> Result<pbrs::json::Json, SerializeError> {{ pbrs::json::field_mask(self.paths().iter().map(|p| p.0.as_bytes())) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_value(v: &pbrs::json::Json, _ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let mut msg = Self::new();");
    let _ = writeln!(
        src,
        "        for path in pbrs::json::as_field_mask(v)? {{ msg.paths_mut().push(path); }}"
    );
    let _ = writeln!(src, "        Ok(msg)");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_typed_json(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(src, "        Ok(self.to_json_value()?.to_string())");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let v = pbrs::json::parse(json)?;");
    let _ = writeln!(src, "        Self::from_json_value(&v, ignore)");
    let _ = writeln!(src, "    }}");
    emit_to_json_value(src, desc);
    emit_from_json_value(src, desc);
}

pub(crate) fn emit_to_json_value(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn to_json_value(&self) -> Result<pbrs::json::Json, SerializeError> {{"
    );
    let _ = writeln!(src, "        let mut map = pbrs::json::JsonMap::new();");
    for f in desc.fields.values() {
        let id = field_id(f);
        let m = field_raw(f);
        let key = rust_str(&f.json_name);
        let getter = format!("self.{id}()");
        if f.is_map {
            let kt = map_key_ty(f);
            let vf = map_value_field(f);
            let val = vf
                .map(|v| json_encode_leaf(v, "v", true))
                .unwrap_or_else(|| json_encode_leaf(f, "v", true));
            let _ = writeln!(src, "        if !self.{id}().is_empty() {{");
            let _ = writeln!(src, "            let mut obj = pbrs::json::JsonMap::new();");
            let _ = writeln!(src, "            for (k, v) in self.{id}() {{");
            let _ = writeln!(
                src,
                "                let _ = obj.insert({}, {val});",
                json_map_key_encode(kt, "k")
            );
            let _ = writeln!(src, "            }}");
            let _ = writeln!(
                src,
                "            let _ = map.insert({key}.into(), pbrs::json::Json::Object(obj));"
            );
            let _ = writeln!(src, "        }}");
        } else if f.cardinality == Cardinality::Repeated {
            let item = json_encode_leaf(f, "item", true);
            let _ = writeln!(src, "        if !self.{id}().is_empty() {{");
            let _ = writeln!(src, "            let mut arr = Vec::new();");
            let _ = writeln!(src, "            for item in self.{id}() {{");
            let _ = writeln!(src, "                arr.push({item});");
            let _ = writeln!(src, "            }}");
            let _ = writeln!(
                src,
                "            let _ = map.insert({key}.into(), pbrs::json::Json::Array(arr));"
            );
            let _ = writeln!(src, "        }}");
        } else if f.field_type == FieldType::Message {
            let _ = writeln!(src, "        if self.has_{m}() {{");
            let _ = writeln!(
                src,
                "            let _ = map.insert({key}.into(), {});",
                json_encode_leaf(f, &getter, false)
            );
            let _ = writeln!(src, "        }}");
        } else {
            let _ = writeln!(src, "        if {} {{", typed_present_expr(f, &getter));
            let _ = writeln!(
                src,
                "            let _ = map.insert({key}.into(), {});",
                json_encode_leaf(f, &getter, false)
            );
            let _ = writeln!(src, "        }}");
        }
    }
    let _ = writeln!(src, "        Ok(pbrs::json::Json::Object(map))");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_from_json_value(src: &mut String, desc: &MessageDescriptor) {
    let _ = writeln!(
        src,
        "    pub fn from_json_value(v: &pbrs::json::Json, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(
        src,
        "        let obj = v.as_object().ok_or_else(|| ParseError::new(\"json message must be an object\"))?;"
    );
    let _ = writeln!(src, "        let mut msg = Self::new();");
    let _ = writeln!(
        src,
        "        let mut seen = std::collections::BTreeSet::<u32>::new();"
    );
    let _ = writeln!(src, "        for (key, val) in obj {{");
    let _ = writeln!(src, "            match key.as_str() {{");
    for f in desc.fields.values() {
        let id = field_id(f);
        let m = field_raw(f);
        let keys = json_match_keys(f);
        let num = f.number;
        let _ = writeln!(src, "                {keys} => {{");
        let _ = writeln!(
            src,
            "                    if !seen.insert({num}u32) {{ return Err(ParseError::owned(format!(\"duplicate json field {{key}}\"))); }}"
        );
        let _ = writeln!(src, "                    if val.is_null() {{ continue; }}");
        emit_json_oneof_guard(src, desc, f);
        if f.is_map {
            let kt = map_key_ty(f);
            let vf = map_value_field(f);
            let val_ty = vf.map(|v| v.field_type);
            let _ = writeln!(
                src,
                "                    let obj = val.as_object().ok_or_else(|| ParseError::new(\"json map must be an object\"))?;"
            );
            let _ = writeln!(src, "                    for (k, v) in obj {{");
            if val_ty == Some(FieldType::Enum) {
                let Some(vf) = vf else { continue };
                let _ = writeln!(
                    src,
                    "                        if let Some(val) = {} {{",
                    json_decode_leaf(vf, "v")
                );
                let _ = writeln!(
                    src,
                    "                            msg.{id}_mut().insert({}, val);",
                    json_map_key_decode(kt, "k")
                );
                let _ = writeln!(src, "                        }}");
            } else {
                let decode = vf
                    .map(|v| json_decode_leaf(v, "v"))
                    .unwrap_or_else(|| "pbrs::json::as_i32(v)?".into());
                let _ = writeln!(
                    src,
                    "                        msg.{id}_mut().insert({}, {decode});",
                    json_map_key_decode(kt, "k")
                );
            }
            let _ = writeln!(src, "                    }}");
        } else if f.cardinality == Cardinality::Repeated {
            let _ = writeln!(
                src,
                "                    let arr = val.as_array().ok_or_else(|| ParseError::new(\"json repeated must be an array\"))?;"
            );
            let _ = writeln!(src, "                    for item in arr {{");
            if f.field_type == FieldType::Enum {
                let _ = writeln!(
                    src,
                    "                        if let Some(item) = {} {{",
                    json_decode_leaf(f, "item")
                );
                let _ = writeln!(
                    src,
                    "                            msg.{id}_mut().push(item);"
                );
                let _ = writeln!(src, "                        }}");
            } else {
                let _ = writeln!(
                    src,
                    "                        msg.{id}_mut().push({});",
                    json_decode_leaf(f, "item")
                );
            }
            let _ = writeln!(src, "                    }}");
        } else if f.field_type == FieldType::Enum {
            let _ = writeln!(
                src,
                "                    if let Some(n) = {} {{",
                json_decode_leaf(f, "val")
            );
            let _ = writeln!(src, "                        msg.set_{m}(n);");
            let _ = writeln!(src, "                    }}");
        } else {
            let _ = writeln!(
                src,
                "                    msg.set_{m}({});",
                json_decode_leaf(f, "val")
            );
        }
        let _ = writeln!(src, "                }}");
    }
    let _ = writeln!(src, "                _ => {{");
    let _ = writeln!(
        src,
        "                    if !ignore {{ return Err(ParseError::owned(format!(\"unknown json field {{key}}\"))); }}"
    );
    let _ = writeln!(src, "                }}");
    let _ = writeln!(src, "            }}");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "        Ok(msg)");
    let _ = writeln!(src, "    }}");
}

pub(crate) fn emit_dynamic_json(src: &mut String, full_name: &str) {
    let _ = writeln!(
        src,
        "    pub fn to_json(&self) -> Result<String, SerializeError> {{"
    );
    let _ = writeln!(
        src,
        "        let b = pbrs::Serialize::serialize(self).map_err(|_| SerializeError::new(\"json\"))?;"
    );
    let _ = writeln!(src, "        let pool = generated_pool();");
    let _ = writeln!(
        src,
        "        let desc = pool.get_message(\"{full_name}\").ok_or_else(|| SerializeError::new(\"missing desc\"))?;"
    );
    let _ = writeln!(
        src,
        "        pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), &b).map_err(|_| SerializeError::new(\"json\"))?.to_json()"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn from_json(json: &str) -> Result<Self, ParseError> {{ Self::from_json_ignore(json, false) }}"
    );
    let _ = writeln!(
        src,
        "    pub fn from_json_ignore(json: &str, ignore: bool) -> Result<Self, ParseError> {{"
    );
    let _ = writeln!(src, "        let pool = generated_pool();");
    let _ = writeln!(
        src,
        "        let desc = pool.get_message(\"{full_name}\").ok_or_else(|| ParseError::owned(\"missing desc\".into()))?;"
    );
    let _ = writeln!(
        src,
        "        let d = pbrs::DynamicMessage::from_json_with_pool(desc, Some(pool), json, ignore)?;"
    );
    let _ = writeln!(
        src,
        "        let b = pbrs::Serialize::serialize(&d).map_err(|e| ParseError::owned(e.to_string()))?;"
    );
    let _ = writeln!(src, "        <Self as pbrs::Parse>::parse(&b)");
    let _ = writeln!(src, "    }}");
}
