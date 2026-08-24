//! Proto3 JSON mapping over DynamicMessage.

use crate::dynamic::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, FieldValue,
    MapKeyValue, MessageDescriptor, Presence, Value,
};
use crate::error::{ParseError, SerializeError};
use crate::string::{ProtoBytes, ProtoString};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map as JsonMap, Number, Value as Json};
use std::fmt;
use std::sync::Arc;

pub fn encode(msg: &DynamicMessage) -> Result<String, SerializeError> {
    Ok(encode_value(msg)?.to_string())
}

pub fn decode(
    desc: Arc<MessageDescriptor>,
    json: &str,
    ignore_unknown: bool,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let v = parse_json_no_dup(json)?;
    decode_message(desc, &v, ignore_unknown, pool)
}

fn parse_json_no_dup(s: &str) -> Result<Json, ParseError> {
    let mut de = serde_json::Deserializer::from_str(s);
    let v = JsonNoDup::deserialize(&mut de).map_err(|e| ParseError::owned(e.to_string()))?;
    de.end().map_err(|e| ParseError::owned(e.to_string()))?;
    Ok(v.0)
}

struct JsonNoDup(Json);

impl<'de> Deserialize<'de> for JsonNoDup {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_any(JsonNoDupVisitor)
            .map(JsonNoDup)
    }
}

struct JsonNoDupVisitor;

impl<'de> Visitor<'de> for JsonNoDupVisitor {
    type Value = Json;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("json value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
        Ok(Number::from_f64(v).map(Json::Number).unwrap_or(Json::Null))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Json, E> {
        Ok(Json::String(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Json, E> {
        Ok(Json::String(v))
    }
    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut arr = Vec::new();
        while let Some(JsonNoDup(v)) = seq.next_element()? {
            arr.push(v);
        }
        Ok(Json::Array(arr))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let Some(first) = map.next_key::<String>()? else {
            return Ok(Json::Object(JsonMap::new()));
        };
        if first == "$serde_json::private::Number" {
            let n: String = map.next_value()?;
            let num = n.parse::<Number>().map_err(de::Error::custom)?;
            return Ok(Json::Number(num));
        }
        let mut obj = JsonMap::new();
        let first_v: JsonNoDup = map.next_value()?;
        obj.insert(first, first_v.0);
        while let Some((k, JsonNoDup(v))) = map.next_entry()? {
            if obj.contains_key(&k) {
                return Err(de::Error::custom(format!("duplicate json key {k}")));
            }
            obj.insert(k, v);
        }
        Ok(Json::Object(obj))
    }
}

fn encode_value(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    match msg.descriptor().full_name.as_str() {
        "google.protobuf.Timestamp" => return encode_timestamp(msg),
        "google.protobuf.Duration" => return encode_duration(msg),
        "google.protobuf.BoolValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.Int64Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.UInt64Value"
        | "google.protobuf.FloatValue"
        | "google.protobuf.DoubleValue"
        | "google.protobuf.StringValue"
        | "google.protobuf.BytesValue" => {
            return encode_wrapper(msg);
        }
        "google.protobuf.Empty" => return Ok(Json::Object(JsonMap::new())),
        "google.protobuf.Struct" => return encode_struct(msg),
        "google.protobuf.Value" => return encode_proto_value(msg),
        "google.protobuf.ListValue" => return encode_list_value(msg),
        "google.protobuf.FieldMask" => return encode_field_mask(msg),
        "google.protobuf.Any" => return encode_any(msg),
        _ => {}
    }
    let mut map = JsonMap::new();
    for (num, fv) in msg.raw_fields() {
        let Some(field) = msg.descriptor().field(*num) else {
            continue;
        };
        if let Some(json) = encode_field(field, fv)? {
            let key = field
                .extension_name
                .as_ref()
                .map(|n| format!("[{n}]"))
                .unwrap_or_else(|| field.json_name.clone());
            map.insert(key, json);
        }
    }
    Ok(Json::Object(map))
}

fn encode_field(field: &FieldDescriptor, fv: &FieldValue) -> Result<Option<Json>, SerializeError> {
    Ok(match fv {
        FieldValue::Singular(v) => {
            if field.presence == Presence::Implicit && v.is_implicit_default() {
                None
            } else {
                Some(encode_leaf(field, v)?)
            }
        }
        FieldValue::Repeated(items) => {
            if items.is_empty() {
                None
            } else {
                let arr: Result<Vec<_>, _> = items.iter().map(|v| encode_leaf(field, v)).collect();
                Some(Json::Array(arr?))
            }
        }
        FieldValue::Map(items) => {
            if items.is_empty() {
                None
            } else {
                let mut obj = JsonMap::new();
                for (k, v) in items {
                    obj.insert(map_key_json(k), encode_leaf(field, v)?);
                }
                Some(Json::Object(obj))
            }
        }
    })
}

fn encode_leaf(field: &FieldDescriptor, v: &Value) -> Result<Json, SerializeError> {
    Ok(match v {
        Value::Double(n) => json_f64(*n),
        Value::Float(n) => json_f64(*n as f64),
        Value::Int32(n) => Json::Number((*n).into()),
        Value::Int64(n) => Json::String(n.to_string()),
        Value::Uint32(n) => Json::Number((*n).into()),
        Value::Uint64(n) => Json::String(n.to_string()),
        Value::Bool(b) => Json::Bool(*b),
        Value::String(s) => Json::String(
            s.to_str()
                .map(str::to_string)
                .unwrap_or_else(|_| String::from_utf8_lossy(s.as_bytes()).into_owned()),
        ),
        Value::Bytes(b) => Json::String(b64_encode(b.as_bytes())),
        Value::Enum(n) => {
            if field
                .enum_ty
                .as_ref()
                .map(|e| e.full_name.as_str())
                .or(field.type_name.as_deref())
                .is_some_and(|n| n.trim_start_matches('.') == "google.protobuf.NullValue")
            {
                Json::Null
            } else if let Some(en) = &field.enum_ty {
                if let Some(name) = en.values.get(n) {
                    Json::String(name.clone())
                } else {
                    Json::Number((*n).into())
                }
            } else {
                Json::Number((*n).into())
            }
        }
        Value::Message(m) => encode_value(m)?,
    })
}

fn json_f64(n: f64) -> Json {
    if n.is_nan() {
        Json::String("NaN".into())
    } else if n.is_infinite() {
        Json::String(if n.is_sign_positive() {
            "Infinity".into()
        } else {
            "-Infinity".into()
        })
    } else {
        Number::from_f64(n)
            .map(Json::Number)
            .unwrap_or(Json::String(n.to_string()))
    }
}

fn map_key_json(k: &MapKeyValue) -> String {
    match k {
        MapKeyValue::I32(n) => n.to_string(),
        MapKeyValue::I64(n) => n.to_string(),
        MapKeyValue::U32(n) => n.to_string(),
        MapKeyValue::U64(n) => n.to_string(),
        MapKeyValue::Bool(b) => b.to_string(),
        MapKeyValue::String(s) => s
            .to_str()
            .map(str::to_string)
            .unwrap_or_else(|_| String::from_utf8_lossy(s.as_bytes()).into_owned()),
    }
}

fn decode_message(
    desc: Arc<MessageDescriptor>,
    v: &Json,
    ignore_unknown: bool,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    match desc.full_name.as_str() {
        "google.protobuf.Timestamp" => return decode_timestamp(desc, v),
        "google.protobuf.Duration" => return decode_duration(desc, v),
        "google.protobuf.BoolValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.Int64Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.UInt64Value"
        | "google.protobuf.FloatValue"
        | "google.protobuf.DoubleValue"
        | "google.protobuf.StringValue"
        | "google.protobuf.BytesValue" => return decode_wrapper(desc, v),
        "google.protobuf.Empty" => return Ok(DynamicMessage::new(desc)),
        "google.protobuf.Struct" => return decode_struct(desc, v, pool),
        "google.protobuf.Value" => return decode_proto_value(desc, v, pool),
        "google.protobuf.ListValue" => return decode_list_value(desc, v, pool),
        "google.protobuf.FieldMask" => return decode_field_mask(desc, v),
        "google.protobuf.Any" => return decode_any(desc, v, pool),
        _ => {}
    }
    let obj = v
        .as_object()
        .ok_or_else(|| ParseError::new("json message must be an object"))?;
    let mut msg = DynamicMessage::new(desc.clone());
    if let Some(p) = pool.clone() {
        msg.set_pool(p);
    }
    let mut seen = std::collections::BTreeSet::new();
    for (key, val) in obj {
        let Some(field) = desc.field_by_name(key) else {
            if ignore_unknown {
                continue;
            }
            return Err(ParseError::owned(format!("unknown json field {key}")));
        };
        if !seen.insert(field.number) {
            return Err(ParseError::owned(format!("duplicate json field {key}")));
        }
        decode_into(&mut msg, field, val, ignore_unknown, pool.clone())?;
    }
    Ok(msg)
}

fn decode_into(
    msg: &mut DynamicMessage,
    field: &FieldDescriptor,
    val: &Json,
    ignore_unknown: bool,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<(), ParseError> {
    if val.is_null() {
        if field.field_type == FieldType::Enum
            && field
                .enum_ty
                .as_ref()
                .map(|e| e.full_name.as_str())
                .or(field.type_name.as_deref())
                .is_some_and(|n| n.trim_start_matches('.') == "google.protobuf.NullValue")
        {
            msg.set(field.number, Value::Enum(0));
            return Ok(());
        }
        if field.field_type == FieldType::Message
            && field
                .message
                .as_ref()
                .map(|m| m.full_name.as_str())
                .or(field.type_name.as_deref())
                .is_some_and(|n| n.trim_start_matches('.') == "google.protobuf.Value")
        {
            let mut inner = DynamicMessage::new(
                field
                    .message
                    .clone()
                    .or_else(|| {
                        msg.pool()
                            .and_then(|p| p.get_message("google.protobuf.Value"))
                    })
                    .ok_or_else(|| ParseError::new("unresolved Value"))?,
            );
            inner.set(1, Value::Enum(0));
            msg.set(field.number, Value::Message(inner));
            return Ok(());
        }
        return Ok(());
    }
    if field.cardinality != Cardinality::Repeated && !field.is_map {
        if let Some(idx) = field.oneof_index {
            if let Some(members) = msg.descriptor().oneofs.get(idx as usize) {
                for n in members {
                    if *n != field.number && msg.has(*n) {
                        return Err(ParseError::new("duplicate oneof member"));
                    }
                }
            }
        }
    }
    if field.is_map {
        let obj = val
            .as_object()
            .ok_or_else(|| ParseError::new("json map must be an object"))?;
        let entry_owned;
        let entry = if let Some(e) = field.message.as_ref() {
            e
        } else if let Some(tn) = field.type_name.as_deref() {
            entry_owned = pool
                .as_ref()
                .and_then(|p| p.get_message(tn.trim_start_matches('.')))
                .ok_or_else(|| ParseError::new("map missing entry"))?;
            &entry_owned
        } else {
            return Err(ParseError::new("map missing entry"));
        };
        let kf = entry
            .field(1)
            .ok_or_else(|| ParseError::new("map missing key"))?;
        let vf = entry
            .field(2)
            .ok_or_else(|| ParseError::new("map missing value"))?;
        for (k, v) in obj {
            let key = parse_map_key(kf.field_type, k)?;
            match decode_leaf(vf, v, ignore_unknown, pool.clone())? {
                Some(value) => msg.insert_map(field.number, key, value),
                None => continue,
            }
        }
        return Ok(());
    }
    if field.cardinality == Cardinality::Repeated {
        let arr = val
            .as_array()
            .ok_or_else(|| ParseError::new("json repeated must be an array"))?;
        for item in arr {
            match decode_leaf(field, item, ignore_unknown, pool.clone())? {
                Some(v) => msg.push(field.number, v),
                None => continue,
            }
        }
        return Ok(());
    }
    if let Some(v) = decode_leaf(field, val, ignore_unknown, pool)? {
        msg.set(field.number, v);
    }
    Ok(())
}

fn decode_leaf(
    field: &FieldDescriptor,
    val: &Json,
    ignore_unknown: bool,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<Option<Value>, ParseError> {
    Ok(Some(match field.field_type {
        FieldType::Double => Value::Double(json_as_f64(val)?),
        FieldType::Float => Value::Float(json_as_f32(val)?),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            Value::Int32(json_as_i32(val)?)
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            Value::Int64(json_as_i64(val)?)
        }
        FieldType::Uint32 | FieldType::Fixed32 => Value::Uint32(json_as_u32(val)?),
        FieldType::Uint64 | FieldType::Fixed64 => Value::Uint64(json_as_u64(val)?),
        FieldType::Bool => Value::Bool(
            val.as_bool()
                .ok_or_else(|| ParseError::new("expected bool"))?,
        ),
        FieldType::String => Value::String(ProtoString::from(
            val.as_str()
                .ok_or_else(|| ParseError::new("expected string"))?,
        )),
        FieldType::Bytes => Value::Bytes(ProtoBytes::from(
            b64_decode(
                val.as_str()
                    .ok_or_else(|| ParseError::new("expected base64 string"))?,
            )?
            .as_slice(),
        )),
        FieldType::Enum => {
            let Some(n) = parse_enum(field, val, ignore_unknown, pool.as_ref())? else {
                return Ok(None);
            };
            Value::Enum(n)
        }
        FieldType::Message | FieldType::Group => {
            let desc = field
                .message
                .clone()
                .or_else(|| {
                    field.type_name.as_deref().and_then(|tn| {
                        pool.as_ref()
                            .and_then(|p| p.get_message(tn.trim_start_matches('.')))
                    })
                })
                .ok_or_else(|| ParseError::new("unresolved message"))?;
            Value::Message(decode_message(desc, val, ignore_unknown, pool)?)
        }
    }))
}

fn parse_enum(
    field: &FieldDescriptor,
    val: &Json,
    ignore_unknown: bool,
    pool: Option<&Arc<DescriptorPool>>,
) -> Result<Option<i32>, ParseError> {
    if let Some(s) = val.as_str() {
        let en = field.enum_ty.clone().or_else(|| {
            field
                .type_name
                .as_deref()
                .and_then(|tn| pool.and_then(|p| p.get_enum(tn.trim_start_matches('.'))))
        });
        if let Some(en) = en {
            if let Some(n) = en.names.get(s) {
                return Ok(Some(*n));
            }
            if ignore_unknown {
                return Ok(None);
            }
            return Err(ParseError::owned(format!("unknown enum {s}")));
        }
        if ignore_unknown {
            return Ok(None);
        }
        return Err(ParseError::owned(format!("unknown enum {s}")));
    }
    Ok(Some(json_as_i64(val)? as i32))
}

fn parse_map_key(ty: FieldType, s: &str) -> Result<MapKeyValue, ParseError> {
    Ok(match ty {
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => {
            MapKeyValue::I32(s.parse().map_err(|_| ParseError::new("bad map key"))?)
        }
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => {
            MapKeyValue::I64(s.parse().map_err(|_| ParseError::new("bad map key"))?)
        }
        FieldType::Uint32 | FieldType::Fixed32 => {
            MapKeyValue::U32(s.parse().map_err(|_| ParseError::new("bad map key"))?)
        }
        FieldType::Uint64 | FieldType::Fixed64 => {
            MapKeyValue::U64(s.parse().map_err(|_| ParseError::new("bad map key"))?)
        }
        FieldType::Bool => {
            MapKeyValue::Bool(s.parse().map_err(|_| ParseError::new("bad map key"))?)
        }
        FieldType::String => MapKeyValue::String(ProtoString::from(s)),
        _ => return Err(ParseError::new("invalid map key type")),
    })
}

fn parse_c_f64(s: &str) -> Result<f64, ParseError> {
    let mut buf = Vec::with_capacity(s.len() + 1);
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    let mut end: *mut i8 = std::ptr::null_mut();
    // SAFETY: buf is a NUL-terminated byte string; strtod only reads it.
    let v = unsafe { strtod(buf.as_ptr() as *const i8, &mut end) };
    if end == buf.as_ptr() as *mut i8 || unsafe { *end } != 0 {
        return Err(ParseError::new("bad float"));
    }
    Ok(v)
}

extern "C" {
    fn strtod(nptr: *const i8, endptr: *mut *mut i8) -> f64;
}

fn json_as_f32(v: &Json) -> Result<f32, ParseError> {
    let f = json_as_f64(v)?;
    if f.is_finite() && f.abs() > f32::MAX as f64 {
        return Err(ParseError::new("float out of range"));
    }
    Ok(f as f32)
}

fn json_as_f64(v: &Json) -> Result<f64, ParseError> {
    match v {
        Json::Number(n) => {
            let f = parse_c_f64(&n.to_string())?;
            if !f.is_finite() {
                return Err(ParseError::new("float overflow"));
            }
            Ok(f)
        }
        Json::String(s) => match s.as_str() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            other => parse_c_f64(other),
        },
        _ => Err(ParseError::new("expected number")),
    }
}

fn json_as_i64(v: &Json) -> Result<i64, ParseError> {
    match v {
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(i);
            }
            if let Some(u) = n.as_u64() {
                return i64::try_from(u).map_err(|_| ParseError::new("int out of range"));
            }
            let f = n.as_f64().ok_or_else(|| ParseError::new("bad int"))?;
            if f.is_finite() && f.abs() > ((1u64 << 53) as f64) {
                return Err(ParseError::new("int out of range"));
            }
            float_as_int(f)
        }
        Json::String(s) => {
            if let Ok(i) = s.parse::<i64>() {
                return Ok(i);
            }
            if !(s.contains('e') || s.contains('E') || s.contains('.')) {
                return Err(ParseError::new("bad int string"));
            }
            let f: f64 = s.parse().map_err(|_| ParseError::new("bad int string"))?;
            if f.is_finite() && f.abs() > ((1u64 << 53) as f64) {
                return Err(ParseError::new("int out of range"));
            }
            float_as_int(f)
        }
        _ => Err(ParseError::new("expected int")),
    }
}

fn float_as_int(f: f64) -> Result<i64, ParseError> {
    if !f.is_finite() || f.fract() != 0.0 {
        return Err(ParseError::new("non-integer"));
    }
    if f < i64::MIN as f64 || f > i64::MAX as f64 {
        return Err(ParseError::new("int out of range"));
    }
    Ok(f as i64)
}

fn json_as_i32(v: &Json) -> Result<i32, ParseError> {
    i32::try_from(json_as_i64(v)?).map_err(|_| ParseError::new("int32 out of range"))
}

fn json_as_u64(v: &Json) -> Result<u64, ParseError> {
    match v {
        Json::Number(n) => {
            if let Some(u) = n.as_u64() {
                return Ok(u);
            }
            if let Some(i) = n.as_i64() {
                return u64::try_from(i).map_err(|_| ParseError::new("uint out of range"));
            }
            let f = n.as_f64().ok_or_else(|| ParseError::new("bad uint"))?;
            let i = float_as_int(f)?;
            u64::try_from(i).map_err(|_| ParseError::new("uint out of range"))
        }
        Json::String(s) => {
            if let Ok(u) = s.parse::<u64>() {
                return Ok(u);
            }
            let i = float_as_int(s.parse().map_err(|_| ParseError::new("bad uint string"))?)?;
            u64::try_from(i).map_err(|_| ParseError::new("uint out of range"))
        }
        _ => Err(ParseError::new("expected uint")),
    }
}

fn json_as_u32(v: &Json) -> Result<u32, ParseError> {
    u32::try_from(json_as_u64(v)?).map_err(|_| ParseError::new("uint32 out of range"))
}

fn encode_wrapper(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    match msg.get_singular(1) {
        Some(v) => {
            let dummy = FieldDescriptor::new(
                "value",
                1,
                wrapper_type(msg),
                Cardinality::Optional,
                Presence::Implicit,
            );
            encode_leaf(&dummy, v)
        }
        None => Ok(match msg.descriptor().full_name.as_str() {
            "google.protobuf.BoolValue" => Json::Bool(false),
            "google.protobuf.StringValue" => Json::String(String::new()),
            "google.protobuf.BytesValue" => Json::String(String::new()),
            "google.protobuf.Int64Value" | "google.protobuf.UInt64Value" => {
                Json::String("0".into())
            }
            _ => Json::Number(0.into()),
        }),
    }
}

fn wrapper_type(msg: &DynamicMessage) -> FieldType {
    match msg.descriptor().full_name.as_str() {
        "google.protobuf.BoolValue" => FieldType::Bool,
        "google.protobuf.Int32Value" => FieldType::Int32,
        "google.protobuf.Int64Value" => FieldType::Int64,
        "google.protobuf.UInt32Value" => FieldType::Uint32,
        "google.protobuf.UInt64Value" => FieldType::Uint64,
        "google.protobuf.FloatValue" => FieldType::Float,
        "google.protobuf.DoubleValue" => FieldType::Double,
        "google.protobuf.StringValue" => FieldType::String,
        "google.protobuf.BytesValue" => FieldType::Bytes,
        _ => FieldType::Int32,
    }
}

fn decode_wrapper(desc: Arc<MessageDescriptor>, v: &Json) -> Result<DynamicMessage, ParseError> {
    let mut msg = DynamicMessage::new(desc.clone());
    let ty = match desc.full_name.as_str() {
        "google.protobuf.BoolValue" => FieldType::Bool,
        "google.protobuf.Int32Value" => FieldType::Int32,
        "google.protobuf.Int64Value" => FieldType::Int64,
        "google.protobuf.UInt32Value" => FieldType::Uint32,
        "google.protobuf.UInt64Value" => FieldType::Uint64,
        "google.protobuf.FloatValue" => FieldType::Float,
        "google.protobuf.DoubleValue" => FieldType::Double,
        "google.protobuf.StringValue" => FieldType::String,
        "google.protobuf.BytesValue" => FieldType::Bytes,
        _ => FieldType::Int32,
    };
    let field = FieldDescriptor::new("value", 1, ty, Cardinality::Optional, Presence::Implicit);
    msg.set(
        1,
        decode_leaf(&field, v, false, None)?.ok_or_else(|| ParseError::new("wrapper"))?,
    );
    Ok(msg)
}

fn encode_timestamp(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let seconds = match msg.get_singular(1) {
        Some(Value::Int64(s)) => *s,
        _ => 0,
    };
    let nanos = match msg.get_singular(2) {
        Some(Value::Int32(n)) => *n,
        _ => 0,
    };
    // RFC3339 range: 0001-01-01T00:00:00Z .. 9999-12-31T23:59:59Z
    if !(-62_135_596_800..=253_402_300_799).contains(&seconds) {
        return Err(SerializeError::new("timestamp out of range"));
    }
    if !(0..1_000_000_000).contains(&nanos) {
        return Err(SerializeError::new("timestamp nanos out of range"));
    }
    Ok(Json::String(format_timestamp(seconds, nanos)))
}

fn decode_timestamp(desc: Arc<MessageDescriptor>, v: &Json) -> Result<DynamicMessage, ParseError> {
    let s = v
        .as_str()
        .ok_or_else(|| ParseError::new("timestamp must be string"))?;
    let (seconds, nanos) = parse_timestamp(s)?;
    let mut msg = DynamicMessage::new(desc);
    if seconds != 0 {
        msg.set(1, Value::Int64(seconds));
    }
    if nanos != 0 {
        msg.set(2, Value::Int32(nanos));
    }
    Ok(msg)
}

fn format_timestamp(seconds: i64, nanos: i32) -> String {
    // RFC3339-ish UTC. Good enough for round-trip of whole seconds; nanos appended.
    let days = seconds.div_euclid(86400);
    let rem = seconds.rem_euclid(86400) as u32;
    let (y, m, d) = civil_from_days(days + 719468); // 1970-01-01 = 719468
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;
    if nanos == 0 {
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
    } else {
        format!(
            "{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{}Z",
            frac_digits(nanos.unsigned_abs())
        )
    }
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    // Howard Hinnant civil_from_days
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn parse_timestamp(s: &str) -> Result<(i64, i32), ParseError> {
    let s = s.trim();
    let (core, offset_secs) = if let Some(body) = s.strip_suffix('Z') {
        (body, 0i64)
    } else if let Some(i) = s.rfind(['+', '-']) {
        if i < 10 {
            return Err(ParseError::new("timestamp must be RFC3339"));
        }
        let (body, off) = s.split_at(i);
        let sign = if off.starts_with('-') { -1i64 } else { 1 };
        let off = off.trim_start_matches(['+', '-']);
        let (oh, om) = off
            .split_once(':')
            .ok_or_else(|| ParseError::new("bad timestamp offset"))?;
        let oh: i64 = oh.parse().map_err(|_| ParseError::new("bad offset"))?;
        let om: i64 = om.parse().map_err(|_| ParseError::new("bad offset"))?;
        (body, sign * (oh * 3600 + om * 60))
    } else {
        return Err(ParseError::new("timestamp must be RFC3339"));
    };
    let (main, frac) = match core.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (core, None),
    };
    let parts: Vec<&str> = main.split('T').collect();
    if parts.len() != 2 {
        return Err(ParseError::new("bad timestamp"));
    }
    let date: Vec<&str> = parts[0].split('-').collect();
    let time: Vec<&str> = parts[1].split(':').collect();
    if date.len() != 3 || time.len() != 3 {
        return Err(ParseError::new("bad timestamp"));
    }
    let y: i32 = date[0].parse().map_err(|_| ParseError::new("bad year"))?;
    let m: u32 = date[1].parse().map_err(|_| ParseError::new("bad month"))?;
    let d: u32 = date[2].parse().map_err(|_| ParseError::new("bad day"))?;
    let hh: u32 = time[0].parse().map_err(|_| ParseError::new("bad hour"))?;
    let mm: u32 = time[1].parse().map_err(|_| ParseError::new("bad minute"))?;
    let ss: u32 = time[2].parse().map_err(|_| ParseError::new("bad second"))?;
    let mut nanos = 0i32;
    if let Some(f) = frac {
        let digits: String = f.chars().take_while(|c| c.is_ascii_digit()).collect();
        let mut buf = digits;
        buf.truncate(9);
        while buf.len() < 9 {
            buf.push('0');
        }
        nanos = buf.parse().map_err(|_| ParseError::new("bad nanos"))?;
    }
    let days = days_from_civil(y, m, d) - 719468;
    let seconds = days * 86400 + (hh as i64) * 3600 + (mm as i64) * 60 + ss as i64 - offset_secs;
    if !(-62_135_596_800..=253_402_300_799).contains(&seconds) {
        return Err(ParseError::new("timestamp out of range"));
    }
    if !(0..1_000_000_000).contains(&nanos) {
        return Err(ParseError::new("timestamp nanos out of range"));
    }
    Ok((seconds, nanos))
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64
}

fn encode_duration(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let seconds = match msg.get_singular(1) {
        Some(Value::Int64(s)) => *s,
        _ => 0,
    };
    let nanos = match msg.get_singular(2) {
        Some(Value::Int32(n)) => *n,
        _ => 0,
    };
    if !(-315_576_000_000..=315_576_000_000).contains(&seconds) {
        return Err(SerializeError::new("duration out of range"));
    }
    if !(-999_999_999..=999_999_999).contains(&nanos) {
        return Err(SerializeError::new("duration nanos out of range"));
    }
    if seconds != 0 && nanos != 0 && (seconds < 0) != (nanos < 0) {
        return Err(SerializeError::new("duration nanos sign mismatch"));
    }
    if nanos == 0 {
        Ok(Json::String(format!("{seconds}s")))
    } else if seconds == 0 && nanos < 0 {
        Ok(Json::String(format!(
            "-0.{}s",
            frac_digits(nanos.unsigned_abs())
        )))
    } else {
        Ok(Json::String(format!(
            "{seconds}.{}s",
            frac_digits(nanos.unsigned_abs())
        )))
    }
}

fn frac_digits(nanos: u32) -> String {
    let s = format!("{nanos:09}");
    if s.ends_with("000000") {
        s[..3].to_string()
    } else if s.ends_with("000") {
        s[..6].to_string()
    } else {
        s
    }
}

fn decode_duration(desc: Arc<MessageDescriptor>, v: &Json) -> Result<DynamicMessage, ParseError> {
    let s = v
        .as_str()
        .ok_or_else(|| ParseError::new("duration must be string"))?
        .trim();
    let s = s
        .strip_suffix('s')
        .ok_or_else(|| ParseError::new("duration must end with s"))?;
    let neg = s.starts_with('-');
    let (sec, nanos) = if let Some((a, b)) = s.split_once('.') {
        let sec: i64 = a.parse().map_err(|_| ParseError::new("bad duration"))?;
        let mut frac = b.to_string();
        frac.truncate(9);
        while frac.len() < 9 {
            frac.push('0');
        }
        let mut nanos: i32 = frac.parse().map_err(|_| ParseError::new("bad duration"))?;
        if neg {
            nanos = -nanos;
        }
        (sec, nanos)
    } else {
        (s.parse().map_err(|_| ParseError::new("bad duration"))?, 0)
    };
    if !(-315_576_000_000..=315_576_000_000).contains(&sec)
        || !(-999_999_999..=999_999_999).contains(&nanos)
    {
        return Err(ParseError::new("duration out of range"));
    }
    let mut msg = DynamicMessage::new(desc);
    if sec != 0 {
        msg.set(1, Value::Int64(sec));
    }
    if nanos != 0 {
        msg.set(2, Value::Int32(nanos));
    }
    Ok(msg)
}

fn encode_struct(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let mut obj = JsonMap::new();
    if let Some(map) = msg.get_map(1) {
        for (k, v) in map {
            if let (MapKeyValue::String(name), Value::Message(inner)) = (k, v) {
                obj.insert(
                    name.to_str().map(str::to_string).unwrap_or_default(),
                    encode_value(inner)?,
                );
            }
        }
    }
    Ok(Json::Object(obj))
}

fn decode_struct(
    desc: Arc<MessageDescriptor>,
    v: &Json,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let obj = v
        .as_object()
        .ok_or_else(|| ParseError::new("struct must be object"))?;
    let mut msg = DynamicMessage::new(desc.clone());
    let value_desc = desc
        .field(1)
        .and_then(|f| f.message.as_ref())
        .and_then(|entry| entry.field(2))
        .and_then(|vf| vf.message.clone())
        .or_else(|| {
            pool.as_ref()
                .and_then(|p| p.get_message("google.protobuf.Value"))
        });
    if let Some(vd) = value_desc {
        for (k, val) in obj {
            let inner = decode_message(vd.clone(), val, true, pool.clone())?;
            msg.insert_map(
                1,
                MapKeyValue::String(ProtoString::from(k.as_str())),
                Value::Message(inner),
            );
        }
    }
    Ok(msg)
}

fn encode_proto_value(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    match msg.get_singular(1) {
        Some(Value::Int32(0) | Value::Enum(0)) => return Ok(Json::Null),
        Some(Value::Int32(_)) | Some(Value::Enum(_)) => return Ok(Json::Null),
        _ => {}
    }
    if let Some(Value::Double(n)) = msg.get_singular(2) {
        if !n.is_finite() {
            return Err(SerializeError::new("Value NaN/Inf"));
        }
        return Ok(json_f64(*n));
    }
    if let Some(Value::String(s)) = msg.get_singular(3) {
        return Ok(Json::String(s.to_str().unwrap_or("").to_string()));
    }
    if let Some(Value::Bool(b)) = msg.get_singular(4) {
        return Ok(Json::Bool(*b));
    }
    if let Some(Value::Message(m)) = msg.get_singular(5) {
        return encode_value(m);
    }
    if let Some(Value::Message(m)) = msg.get_singular(6) {
        return encode_value(m);
    }
    Ok(Json::Null)
}

fn decode_proto_value(
    desc: Arc<MessageDescriptor>,
    v: &Json,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let mut msg = DynamicMessage::new(desc);
    match v {
        Json::Null => {
            msg.set(1, Value::Enum(0));
        }
        Json::Number(n) => {
            msg.set(2, Value::Double(n.as_f64().unwrap_or(0.0)));
        }
        Json::String(s) => msg.set(3, Value::String(ProtoString::from(s.as_str()))),
        Json::Bool(b) => msg.set(4, Value::Bool(*b)),
        Json::Object(_) => {
            let d = msg
                .descriptor()
                .field(5)
                .and_then(|f| f.message.clone())
                .or_else(|| {
                    pool.as_ref()
                        .and_then(|p| p.get_message("google.protobuf.Struct"))
                });
            if let Some(d) = d {
                msg.set(5, Value::Message(decode_message(d, v, true, pool.clone())?));
            }
        }
        Json::Array(_) => {
            let d = msg
                .descriptor()
                .field(6)
                .and_then(|f| f.message.clone())
                .or_else(|| {
                    pool.as_ref()
                        .and_then(|p| p.get_message("google.protobuf.ListValue"))
                });
            if let Some(d) = d {
                msg.set(6, Value::Message(decode_message(d, v, true, pool)?));
            }
        }
    }
    Ok(msg)
}

fn encode_list_value(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let mut arr = Vec::new();
    if let Some(items) = msg.get_repeated(1) {
        for v in items {
            if let Value::Message(inner) = v {
                arr.push(encode_value(inner)?);
            }
        }
    }
    Ok(Json::Array(arr))
}

fn decode_list_value(
    desc: Arc<MessageDescriptor>,
    v: &Json,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let arr = v
        .as_array()
        .ok_or_else(|| ParseError::new("list must be array"))?;
    let mut msg = DynamicMessage::new(desc.clone());
    let vd = desc.field(1).and_then(|f| f.message.clone()).or_else(|| {
        pool.as_ref()
            .and_then(|p| p.get_message("google.protobuf.Value"))
    });
    if let Some(vd) = vd {
        for item in arr {
            msg.push(
                1,
                Value::Message(decode_message(vd.clone(), item, true, pool.clone())?),
            );
        }
    }
    Ok(msg)
}

fn encode_field_mask(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let mut paths = Vec::new();
    if let Some(items) = msg.get_repeated(1) {
        for v in items {
            if let Value::String(s) = v {
                paths.push(snake_to_camel_strict(s.to_str().unwrap_or(""))?);
            }
        }
    }
    Ok(Json::String(paths.join(",")))
}

fn decode_field_mask(desc: Arc<MessageDescriptor>, v: &Json) -> Result<DynamicMessage, ParseError> {
    let s = v
        .as_str()
        .ok_or_else(|| ParseError::new("field mask must be string"))?;
    let mut msg = DynamicMessage::new(desc);
    if !s.is_empty() {
        for p in s.split(',') {
            if p.contains('_') {
                return Err(ParseError::new("field mask json path must be camelCase"));
            }
            msg.push(
                1,
                Value::String(ProtoString::from(camel_to_snake(p).as_str())),
            );
        }
    }
    Ok(msg)
}

fn snake_to_camel_strict(s: &str) -> Result<String, SerializeError> {
    if s.chars().any(|c| c.is_ascii_uppercase())
        || s.contains("__")
        || s.starts_with('_')
        || s.ends_with('_')
    {
        return Err(SerializeError::new(
            "field mask path is not round-trippable",
        ));
    }
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'_' && i + 1 < b.len() && !b[i + 1].is_ascii_lowercase() {
            return Err(SerializeError::new(
                "field mask path is not round-trippable",
            ));
        }
    }
    Ok(snake_to_camel(s))
}

fn snake_to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_uppercase() {
            out.push('_');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn encode_any(msg: &DynamicMessage) -> Result<Json, SerializeError> {
    let type_url = match msg.get_singular(1) {
        Some(Value::String(s)) => s.to_str().unwrap_or("").to_string(),
        _ => String::new(),
    };
    let value_bytes = match msg.get_singular(2) {
        Some(Value::Bytes(b)) => b.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    let type_name = type_url.rsplit('/').next().unwrap_or(type_url.as_str());
    if type_url.is_empty() {
        return Ok(Json::Object(JsonMap::new()));
    }
    if let Some(pool) = msg.pool() {
        if let Some(desc) = pool.get_message(type_name) {
            if let Ok(inner) =
                DynamicMessage::parse_with_pool(desc, Some(pool.clone()), &value_bytes)
            {
                let encoded = encode_value(&inner)?;
                if is_wkt(type_name) {
                    let mut obj = JsonMap::new();
                    obj.insert("@type".into(), Json::String(type_url));
                    obj.insert("value".into(), encoded);
                    return Ok(Json::Object(obj));
                }
                if let Json::Object(mut obj) = encoded {
                    obj.insert("@type".into(), Json::String(type_url));
                    return Ok(Json::Object(obj));
                }
            }
        }
    }
    let mut obj = JsonMap::new();
    obj.insert("@type".into(), Json::String(type_url));
    Ok(Json::Object(obj))
}

fn is_wkt(name: &str) -> bool {
    matches!(
        name,
        "google.protobuf.Timestamp"
            | "google.protobuf.Duration"
            | "google.protobuf.FieldMask"
            | "google.protobuf.Struct"
            | "google.protobuf.Value"
            | "google.protobuf.ListValue"
            | "google.protobuf.DoubleValue"
            | "google.protobuf.FloatValue"
            | "google.protobuf.Int64Value"
            | "google.protobuf.UInt64Value"
            | "google.protobuf.Int32Value"
            | "google.protobuf.UInt32Value"
            | "google.protobuf.BoolValue"
            | "google.protobuf.StringValue"
            | "google.protobuf.BytesValue"
            | "google.protobuf.Any"
    )
}

fn decode_any(
    desc: Arc<MessageDescriptor>,
    v: &Json,
    pool: Option<Arc<DescriptorPool>>,
) -> Result<DynamicMessage, ParseError> {
    let mut msg = DynamicMessage::new(desc);
    if let Some(p) = pool.clone() {
        msg.set_pool(p);
    }
    let Some(obj) = v.as_object() else {
        return Err(ParseError::new("any must be object"));
    };
    let type_url = match obj.get("@type") {
        Some(Json::String(t)) => t.clone(),
        _ => String::new(),
    };
    if type_url.is_empty() {
        if obj.keys().any(|k| k != "@type") {
            return Err(ParseError::new("any missing @type"));
        }
        return Ok(msg);
    }
    msg.set(1, Value::String(ProtoString::from(type_url.as_str())));
    let type_name = type_url.rsplit('/').next().unwrap_or(type_url.as_str());
    if type_name.is_empty() {
        return Err(ParseError::new("any empty type"));
    }
    if let Some(pool) = pool {
        if let Some(inner_desc) = pool.get_message(type_name) {
            let inner_json = if is_wkt(type_name) {
                obj.get("value").cloned().unwrap_or(Json::Null)
            } else {
                let mut rest = obj.clone();
                rest.remove("@type");
                Json::Object(rest)
            };
            let inner = decode_message(inner_desc, &inner_json, true, Some(pool))?;
            let bytes = crate::message::Serialize::serialize(&inner)
                .map_err(|e| ParseError::owned(e.to_string()))?;
            if !bytes.is_empty() {
                msg.set(2, Value::Bytes(ProtoBytes::from(bytes.as_slice())));
            }
        } else {
            return Err(ParseError::owned(format!("unknown any type {type_name}")));
        }
    }
    Ok(msg)
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let c = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (a << 16) | (b << 8) | c;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6 & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>, ParseError> {
    fn val(c: u8) -> Result<u8, ParseError> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' | b'-' => Ok(62),
            b'/' | b'_' => Ok(63),
            _ => Err(ParseError::new("bad base64")),
        }
    }
    let mut bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    while !bytes.is_empty() && bytes.len() % 4 != 0 {
        bytes.push(b'=');
    }
    if bytes.len() % 4 != 0 {
        return Err(ParseError::new("bad base64 length"));
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = ((val(chunk[0])? as u32) << 18)
            | ((val(chunk[1])? as u32) << 12)
            | ((if chunk[2] == b'=' { 0 } else { val(chunk[2])? } as u32) << 6)
            | (if chunk[3] == b'=' { 0 } else { val(chunk[3])? } as u32);
        out.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(n as u8);
        }
    }
    Ok(out)
}
