//! MX-01 split of `super`: naming (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{
    Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor, Presence,
};

pub(crate) fn ident_last(s: &str) -> String {
    let key = s.trim_start_matches('.');
    let last = key.rsplit('.').next().unwrap_or(key);
    rust_ident_from_last(last)
}

pub(crate) fn must_mangle_view_suffix(
    full: &str,
    messages: &std::collections::BTreeSet<String>,
) -> bool {
    let key = full.trim_start_matches('.');
    let last = key.rsplit('.').next().unwrap_or(key);
    let Some(without) = last.strip_suffix("View") else {
        return false;
    };
    if without.is_empty() {
        return false;
    }
    let sibling = match key.rsplit_once('.') {
        Some((parent, _)) => format!("{parent}.{without}"),
        None => without.to_string(),
    };
    messages.contains(&sibling)
}

pub(crate) fn file_stem_ident(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .replace('-', "_")
}

pub(crate) fn match_extern_type(s: &str) -> Option<String> {
    let key = s.trim_start_matches('.');
    EXTERN_PATHS.with(|paths| {
        let paths = paths.borrow();
        let mut best_match: Option<(usize, String)> = None;
        for (proto_path, rust_path) in paths.iter() {
            let prefix = proto_path.trim_start_matches('.');
            if key == prefix {
                let len = prefix.len();
                if best_match
                    .as_ref()
                    .is_none_or(|(best_len, _)| len > *best_len)
                {
                    best_match = Some((len, rust_path.clone()));
                }
            } else if key.starts_with(prefix) && key[prefix.len()..].starts_with('.') {
                let len = prefix.len();
                let rel = &key[prefix.len() + 1..];
                let resolved = format_extern_rel_path(prefix, rust_path, rel);
                if best_match
                    .as_ref()
                    .is_none_or(|(best_len, _)| len > *best_len)
                {
                    best_match = Some((len, resolved));
                }
            }
        }
        best_match.map(|(_, r)| r)
    })
}

pub(crate) fn is_extern_type(full_name: &str) -> bool {
    match_extern_type(full_name).is_some()
}

pub(crate) fn format_extern_rel_path(prefix: &str, rust_path: &str, rel: &str) -> String {
    let last_ident = ident_last(prefix);
    let suff = format!("::{last_ident}");
    if let Some(base) = rust_path.strip_suffix(&suff) {
        let mod_name = to_snake(&last_ident);
        let rel_fmt = format_rel_type(rel);
        format!("{base}::{mod_name}::{rel_fmt}")
    } else {
        let rel_fmt = format_rel_type(rel);
        format!("{rust_path}::{rel_fmt}")
    }
}

pub(crate) fn format_rel_type(rel: &str) -> String {
    if rel.contains('.') {
        let parts: Vec<&str> = rel.split('.').collect();
        let mod_parts: Vec<String> = parts[..parts.len().saturating_sub(1)]
            .iter()
            .map(|p| to_snake(&ident_last(p)))
            .collect();
        let last = ident_last(parts.last().unwrap_or(&""));
        format!("{}::{last}", mod_parts.join("::"))
    } else {
        ident_last(rel)
    }
}

pub(crate) fn emit_public_uses(
    src: &mut String,
    pool: &DescriptorPool,
    pub_files: &[String],
    targets: &[String],
) {
    let mut sorted_pub = pub_files.to_vec();
    sorted_pub.sort();
    sorted_pub.dedup();
    for p in &sorted_pub {
        let in_targets = targets
            .iter()
            .any(|t| file_matches(&std::iter::once(t.clone()).collect(), p));
        let pkg = pool
            .get_file(p)
            .map(|f| f.package.clone())
            .unwrap_or_default();
        if !in_targets && match_extern_type(p).is_none() && match_extern_type(&pkg).is_none() {
            continue;
        }
        if !pkg.is_empty() {
            if let Some(extern_rust) = match_extern_type(&pkg) {
                let _ = writeln!(src, "pub use {extern_rust}::*;");
            } else {
                let pkg_path = pkg_mod_path(&pkg);
                let _ = writeln!(src, "pub use {pkg_path}::*;");
            }
        } else {
            let stem = file_stem_ident(p);
            let _ = writeln!(src, "pub use crate::{stem}::*;");
        }
    }
}

pub(crate) fn field_raw(f: &FieldDescriptor) -> String {
    FIELD_RAWS.with(|c| {
        c.borrow()
            .get(&f.number)
            .cloned()
            .unwrap_or_else(|| f.name.clone())
    })
}

pub(crate) fn unique_idents(
    full_names: &[String],
    messages: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = std::collections::BTreeMap::new();
    for full in full_names {
        let mut last = ident_last(full);
        if must_mangle_view_suffix(full, messages) {
            last.push('_');
        }
        let id = if used.contains(&last) {
            let parent = full.rsplit_once('.').map(|x| x.0).unwrap_or("X");
            format!("{}{last}", ident_last(parent))
        } else {
            last
        };
        used.insert(id.clone());
        out.insert(full.clone(), id);
    }
    out
}

pub(crate) fn rust_ident(s: &str) -> String {
    let key = s.trim_start_matches('.');
    if let Some(id) = IDENTS.with(|c| c.borrow().get(key).cloned()) {
        return id;
    }
    ident_last(key)
}

pub(crate) fn rust_type_path(s: &str) -> String {
    let key = s.trim_start_matches('.');
    if let Some(extern_rust) = match_extern_type(key) {
        return extern_rust;
    }
    if let Some(id) = IDENTS.with(|c| c.borrow().get(key).cloned()) {
        return id;
    }
    if let Some((type_file, pkg)) = TYPE_FILES.with(|c| c.borrow().get(key).cloned()) {
        let current_target = CURRENT_TARGET.with(|c| c.borrow().clone());
        if !current_target.is_empty()
            && file_matches(&std::iter::once(current_target).collect(), &type_file)
        {
            return ident_last(key);
        }
        if !pkg.is_empty() {
            let pkg_path = pkg_mod_path(&pkg);
            let rel = key
                .strip_prefix(&pkg)
                .unwrap_or(key)
                .trim_start_matches('.');
            if rel.contains('.') {
                let parts: Vec<&str> = rel.split('.').collect();
                let mod_parts: Vec<String> = parts[..parts.len().saturating_sub(1)]
                    .iter()
                    .map(|p| to_snake(&ident_last(p)))
                    .collect();
                let last = ident_last(parts.last().unwrap_or(&""));
                return format!("{pkg_path}::{}::{last}", mod_parts.join("::"));
            } else {
                let last = ident_last(rel);
                return format!("{pkg_path}::{last}");
            }
        } else {
            let last = ident_last(key);
            return format!("crate::{last}");
        }
    }
    ident_last(key)
}

pub(crate) fn is_rust_keyword(id: &str) -> bool {
    matches!(
        id,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "gen"
            | "unknown"
    )
}

pub(crate) fn field_ident_base(name: &str) -> String {
    let mut id = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '_' {
            id.push(c);
        } else if i > 0 {
            id.push('_');
        }
    }
    if id.is_empty() {
        id.push('_');
    }
    if id.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        id = format!("f_{id}");
    }
    if matches!(
        id.as_str(),
        "crate" | "self" | "Self" | "super" | "_" | "new"
    ) {
        format!("{id}_")
    } else if is_rust_keyword(&id) {
        format!("r#{id}")
    } else {
        id
    }
}

pub(crate) fn field_name_with_collision_avoidance(
    desc: &MessageDescriptor,
    f: &FieldDescriptor,
) -> String {
    let name = f.name.as_str();
    for prefix in ["clear_", "has_", "set_"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if desc.fields.values().any(|o| o.name == rest) {
                return format!("{name}_{}", f.number);
            }
        }
    }
    for suffix in ["_mut", "_opt"] {
        if let Some(rest) = name.strip_suffix(suffix) {
            if desc.fields.values().any(|o| o.name == rest) {
                return format!("{name}_{}", f.number);
            }
        }
    }
    name.to_string()
}

pub(crate) fn bind_field_idents(desc: &MessageDescriptor) {
    let mut used = std::collections::BTreeSet::new();
    used.insert("unknown".into());
    let mut raw_used = std::collections::BTreeSet::new();
    let mut out = std::collections::BTreeMap::new();
    let mut raws = std::collections::BTreeMap::new();
    for f in desc.fields.values() {
        let mut raw = field_name_with_collision_avoidance(desc, f);
        if !raw_used.insert(raw.clone()) {
            raw = format!("{}_{}", raw, f.number);
            raw_used.insert(raw.clone());
        }
        let mut id = field_ident_base(&raw);
        if !used.insert(id.clone()) {
            id = format!("{}_{}", id.trim_start_matches("r#"), f.number);
            used.insert(id.clone());
        }
        out.insert(f.number, id);
        raws.insert(f.number, raw);
    }
    FIELD_IDENTS.with(|c| *c.borrow_mut() = out);
    FIELD_RAWS.with(|c| *c.borrow_mut() = raws);
}

pub(crate) fn field_id(f: &FieldDescriptor) -> String {
    FIELD_IDENTS.with(|c| {
        c.borrow()
            .get(&f.number)
            .cloned()
            .unwrap_or_else(|| field_ident_base(&f.name))
    })
}

pub(crate) fn rust_ident_from_last(last: &str) -> String {
    let mut id = String::new();
    for (i, c) in last.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '_' {
            id.push(c);
        } else if i > 0 {
            id.push('_');
        }
    }
    if id.is_empty() {
        id.push('M');
    }
    if matches!(
        id.as_str(),
        "crate" | "self" | "Self" | "super" | "_" | "new"
    ) {
        return format!("{id}_");
    }
    if is_rust_keyword(&id) {
        return format!("r#{id}");
    }
    if matches!(
        id.as_str(),
        "Value" | "Option" | "Result" | "Vec" | "String" | "Box" | "Type"
    ) {
        return format!("Pb{id}");
    }
    id
}

pub(crate) fn scalar_type(field: &FieldDescriptor) -> String {
    match field.field_type {
        FieldType::Double => "f64".into(),
        FieldType::Float => "f32".into(),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => "i64".into(),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 | FieldType::Enum => {
            "i32".into()
        }
        FieldType::Uint64 | FieldType::Fixed64 => "u64".into(),
        FieldType::Uint32 | FieldType::Fixed32 => "u32".into(),
        FieldType::Bool => "bool".into(),
        FieldType::String => "pbrs::rt::LazyStr".into(),
        FieldType::Bytes => "pbrs::rt::LazyBytes".into(),
        FieldType::Message | FieldType::Group => rust_type_path(
            field
                .type_name
                .as_deref()
                .or_else(|| field.message.as_ref().map(|m| m.full_name.as_str()))
                .unwrap_or("UnknownMsg"),
        ),
    }
}

pub(crate) fn map_kv(field: &FieldDescriptor) -> (String, String) {
    let entry = field.message.as_ref();
    let k = entry
        .and_then(|e| e.field(1))
        .map(scalar_type)
        .unwrap_or_else(|| "ProtoString".into());
    let v = entry
        .and_then(|e| e.field(2))
        .map(scalar_type)
        .unwrap_or_else(|| "i32".into());
    (k, v)
}

pub(crate) fn is_option(field: &FieldDescriptor) -> bool {
    !field.is_map
        && field.cardinality != Cardinality::Repeated
        && (field.field_type == FieldType::Message
            || field.field_type == FieldType::Group
            || field.presence == Presence::Explicit)
}

pub(crate) fn is_packed_scalar(f: &FieldDescriptor) -> bool {
    !f.is_map && f.cardinality == Cardinality::Repeated && f.packed && f.field_type.is_packable()
}

pub(crate) fn is_lazy_msg(f: &FieldDescriptor) -> bool {
    !f.is_map
        && f.cardinality != Cardinality::Repeated
        && f.field_type == FieldType::Message
        && !f.delimited
}

pub(crate) fn is_wkt_msg(f: &FieldDescriptor) -> bool {
    let name = f
        .type_name
        .as_deref()
        .or_else(|| f.message.as_ref().map(|m| m.full_name.as_str()))
        .unwrap_or("")
        .trim_start_matches('.');
    name.starts_with("google.protobuf.")
}

pub(crate) fn is_memcpy_packed(f: &FieldDescriptor) -> bool {
    f.packed
        && matches!(
            f.field_type,
            FieldType::Fixed32
                | FieldType::Fixed64
                | FieldType::Sfixed32
                | FieldType::Sfixed64
                | FieldType::Float
                | FieldType::Double
        )
}

/// packed_fixed32/64 and packed_float stay on the hot struct. Putting every
/// memcpy-packed TAT slot on hot grew `size_of` to 824 and lost strings vs v4.
pub(crate) fn is_hot_packed_memcpy(f: &FieldDescriptor) -> bool {
    is_memcpy_packed(f)
        && f.name.starts_with("packed_")
        && matches!(
            f.field_type,
            FieldType::Fixed32 | FieldType::Fixed64 | FieldType::Float
        )
}

pub(crate) fn is_hot_repeated_nested(f: &FieldDescriptor) -> bool {
    !f.is_map
        && f.cardinality == Cardinality::Repeated
        && f.field_type == FieldType::Message
        && !f.delimited
        && f.name == "repeated_nested_message"
}

pub(crate) fn is_light_merge_field(f: &FieldDescriptor) -> bool {
    if f.is_map || f.cardinality == Cardinality::Repeated {
        return false;
    }
    !matches!(
        f.field_type,
        FieldType::String | FieldType::Bytes | FieldType::Message | FieldType::Group
    )
}

pub(crate) fn is_cold_field(f: &FieldDescriptor) -> bool {
    // Maps and repeated string/bytes stay hot (map_64 / strings).
    // packed_fixed32/64, packed_float, and repeated_nested_message stay hot.
    if f.is_map || is_hot_packed_memcpy(f) || is_hot_repeated_nested(f) {
        return false;
    }
    if f.cardinality == Cardinality::Repeated {
        return !matches!(f.field_type, FieldType::String | FieldType::Bytes);
    }
    is_lazy_msg(f) && is_wkt_msg(f)
}

/// Per-message cold-placement answers, computed once.
///
/// The placement predicates scan every field; calling them per field is
/// quadratic in field count. Emitters build this once per message and
/// query it per field.
#[derive(Clone, Copy)]
pub(crate) struct ColdPlacement {
    sparse: bool,
    cold_storage: bool,
}

impl ColdPlacement {
    pub(crate) fn for_message(desc: &MessageDescriptor) -> Self {
        let mut light = 0usize;
        let mut cold = 0usize;
        let mut total = 0usize;
        for f in desc.fields.values() {
            total += 1;
            if is_light_merge_field(f) {
                light += 1;
            }
            if is_cold_field(f) {
                cold += 1;
            }
        }
        let sparse =
            !desc.message_set_wire_format && (1..=4).contains(&light) && total - light >= 3;
        Self {
            sparse,
            cold_storage: sparse || cold >= 6,
        }
    }

    pub(crate) fn uses_cold_storage(&self) -> bool {
        self.cold_storage
    }

    pub(crate) fn stored_cold(&self, f: &FieldDescriptor) -> bool {
        if self.sparse {
            !is_light_merge_field(f)
        } else {
            self.cold_storage && is_cold_field(f)
        }
    }

    pub(crate) fn stored_hot(&self, f: &FieldDescriptor) -> bool {
        !self.stored_cold(f)
    }

    pub(crate) fn store_mut(&self, f: &FieldDescriptor) -> String {
        let id = field_id(f);
        if self.stored_cold(f) {
            format!("self.cold_mut().{id}")
        } else {
            format!("self.{id}")
        }
    }
}

pub(crate) fn camel_to_snake_name(s: &str) -> String {
    to_snake(s)
}

pub(crate) fn screaming_snake_to_upper_camel(s: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            if cap {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            cap = false;
        } else if c.is_ascii_digit() {
            out.push(c);
            cap = true;
        } else {
            cap = true;
        }
    }
    out
}

pub(crate) fn starts_with_ignore_ascii(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

pub(crate) fn strip_enum_prefix<'a>(enum_name: &str, value_name: &'a str) -> &'a str {
    let prefixes = [
        enum_name.to_string(),
        screaming_snake_to_upper_camel(enum_name),
        camel_to_snake_name(enum_name),
    ];
    for p in &prefixes {
        if starts_with_ignore_ascii(value_name, p) {
            let mut rest = &value_name[p.len()..];
            rest = rest.strip_prefix('_').unwrap_or(rest);
            if !rest.is_empty() {
                return rest;
            }
            break;
        }
    }
    value_name
}

pub(crate) fn enum_value_rs_name(enum_name: &str, value_name: &str) -> String {
    let stripped = strip_enum_prefix(enum_name, value_name);
    let mut name = screaming_snake_to_upper_camel(stripped);
    if name.is_empty() {
        name = screaming_snake_to_upper_camel(value_name);
    }
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        name = format!("_{name}");
    }
    if is_rust_keyword(&name) {
        format!("r#{name}")
    } else {
        name
    }
}

pub(crate) struct RustEnumValue {
    pub(crate) name: String,
    pub(crate) number: i32,
    pub(crate) aliases: Vec<String>,
}

pub(crate) fn rust_enum_values(enum_name: &str, listed: &[(i32, String)]) -> Vec<RustEnumValue> {
    let mut seen_name = std::collections::BTreeSet::new();
    let mut by_number: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    let mut out: Vec<RustEnumValue> = Vec::new();
    for (number, proto_name) in listed {
        let rust_name = enum_value_rs_name(enum_name, proto_name);
        if !seen_name.insert(rust_name.clone()) {
            continue;
        }
        if let Some(&idx) = by_number.get(number) {
            out[idx].aliases.push(rust_name);
        } else {
            by_number.insert(*number, out.len());
            out.push(RustEnumValue {
                name: rust_name,
                number: *number,
                aliases: Vec::new(),
            });
        }
    }
    out
}

/// Rust module name for one proto package segment. Keywords escape with
/// `r#`; segments that cannot be raw identifiers (`crate`, `self`, ...)
/// take a trailing underscore. Every package-module emission and every
/// `crate::pkg::...` path must use this so definition and use agree.
pub(crate) fn mod_ident(segment: &str) -> String {
    if matches!(
        segment,
        "crate" | "self" | "Self" | "super" | "true" | "false" | "_"
    ) {
        format!("{segment}_")
    } else if segment != "unknown" && is_rust_keyword(segment) {
        format!("r#{segment}")
    } else {
        segment.to_string()
    }
}

/// `pkg.segments` rendered as a `crate::`-rooted module path.
pub(crate) fn pkg_mod_path(pkg: &str) -> String {
    format!(
        "crate::{}",
        pkg.split('.').map(mod_ident).collect::<Vec<_>>().join("::")
    )
}

pub(crate) fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    if matches!(out.as_str(), "crate" | "self" | "Self" | "super" | "_") {
        format!("{out}_")
    } else if is_rust_keyword(&out) {
        format!("r#{out}")
    } else {
        out
    }
}
