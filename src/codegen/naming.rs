//! MX-01 split of `super`: naming (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{
    Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor, Presence,
};
use std::borrow::Cow;
use std::rc::Rc;

pub(crate) fn ident_last(s: &str) -> String {
    let key = s.trim_start_matches('.');
    let last = key.rsplit('.').next().unwrap_or(key);
    rust_ident_from_last(last)
}

pub(crate) fn must_mangle_proxy_suffix(
    full: &str,
    messages: &std::collections::BTreeSet<String>,
) -> bool {
    let key = full.trim_start_matches('.');
    let last = key.rsplit('.').next().unwrap_or(key);
    let Some(without) = last
        .strip_suffix("View")
        .or_else(|| last.strip_suffix("Mut"))
    else {
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
    let base = rust_path.strip_suffix(&suff).or_else(|| {
        let public_ident = type_public_ident(prefix.trim_start_matches('.'));
        if public_ident == last_ident {
            return None;
        }
        rust_path.strip_suffix(&format!("::{public_ident}"))
    });
    if let Some(base) = base {
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
    let all_targets = FileMatcher::for_slice(targets);
    for p in &sorted_pub {
        let in_targets = all_targets.matches(p);
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

// Emitters reuse these names many times per field. Share the immutable
// spelling within its worker instead of allocating a String on every lookup.
pub(crate) fn field_raw(f: &FieldDescriptor) -> Rc<str> {
    FIELD_RAWS.with(|c| {
        c.borrow()
            .get(&f.number)
            .cloned()
            .unwrap_or_else(|| Rc::from(f.name.as_str()))
    })
}

fn try_reserve_type_ident(
    id: &str,
    is_message: bool,
    used: &mut std::collections::BTreeSet<String>,
) -> bool {
    let bare = id.trim_start_matches("r#");
    if used.contains(bare) {
        return false;
    }
    let proxies = if is_message {
        let view = format!("{bare}View");
        if used.contains(&view) {
            return false;
        }
        let mut_ = format!("{bare}Mut");
        if used.contains(&mut_) {
            return false;
        }
        Some((view, mut_))
    } else {
        None
    };
    used.insert(bare.to_string());
    if let Some((view, mut_)) = proxies {
        used.insert(view);
        used.insert(mut_);
    }
    true
}

fn allocate_type_idents<'a>(
    names: impl IntoIterator<Item = (&'a str, String)>,
    messages: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Message candidates reserve companion names too; an enum can still use
    // a bare name that a message rejected because of an occupied companion.
    let mut next_message_suffix = std::collections::BTreeMap::<String, usize>::new();
    let mut next_enum_suffix = std::collections::BTreeMap::<String, usize>::new();
    let mut out = std::collections::BTreeMap::new();
    for (full, last) in names {
        let is_message = messages.contains(full);
        let mut id = last;
        if !try_reserve_type_ident(&id, is_message, &mut used) {
            let parent = full.rsplit_once('.').map(|x| x.0).unwrap_or("X");
            // Raw markers belong to one complete identifier. Embedding one
            // between a parent and a child would produce `Outerr#type`.
            let parent = ident_last(parent);
            let stem = format!(
                "{}{}",
                parent.trim_start_matches("r#"),
                id.trim_start_matches("r#")
            );
            id = rust_ident_from_last(&stem);
            if !try_reserve_type_ident(&id, is_message, &mut used) {
                // Reservations never disappear. Each smaller suffix already
                // rejected or reserved for this stem therefore stays occupied.
                // Resume after it instead of retrying the same growing prefix.
                let next_suffix = if is_message {
                    &mut next_message_suffix
                } else {
                    &mut next_enum_suffix
                };
                let mut suffix = next_suffix.get(&stem).copied().unwrap_or(2);
                loop {
                    id = format!("{stem}_{suffix}");
                    suffix += 1;
                    if try_reserve_type_ident(&id, is_message, &mut used) {
                        break;
                    }
                }
                next_suffix.insert(stem, suffix);
            }
        }
        out.insert(full.to_string(), id);
    }
    out
}

fn preferred_type_ident(full: &str, messages: &std::collections::BTreeSet<String>) -> String {
    let mut id = ident_last(full);
    if must_mangle_proxy_suffix(full, messages) {
        id.push('_');
    }
    id
}

pub(crate) fn scoped_type_idents<'a>(
    full_names: impl IntoIterator<Item = &'a str>,
    messages: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut scopes = std::collections::BTreeMap::<&str, Vec<&str>>::new();
    for full in full_names {
        let parent = full.rsplit_once('.').map(|x| x.0).unwrap_or("");
        scopes.entry(parent).or_default().push(full);
    }
    let mut out = std::collections::BTreeMap::new();
    for siblings in scopes.into_values() {
        out.extend(allocate_type_idents(
            siblings
                .into_iter()
                .map(|full| (full, preferred_type_ident(full, messages))),
            messages,
        ));
    }
    out
}

pub(crate) fn type_public_ident(full: &str) -> String {
    TYPE_FILES.with(|files| {
        files
            .borrow()
            .as_ref()
            .and_then(|files| files.get(full))
            .map(|(_, _, id)| id.clone())
            .unwrap_or_else(|| ident_last(full))
    })
}

pub(crate) fn flat_type_idents(
    full_names: &[String],
    messages: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let nested = |full: &&String| {
        full.rsplit_once('.')
            .is_some_and(|(parent, _)| messages.contains(parent))
    };
    // Public top-level exports retain their scoped names. Flattened nested
    // implementation types can then take a parent prefix where needed.
    let ordered = full_names
        .iter()
        .filter(|full| !nested(full))
        .chain(full_names.iter().filter(nested));
    allocate_type_idents(
        ordered.map(|full| (full.as_str(), type_public_ident(full))),
        messages,
    )
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
    if let Some(path) = TYPE_PATHS.with(|paths| paths.borrow().get(key).cloned()) {
        return path;
    }
    // Resolve inside the table borrow: cloning the (file, package) pair
    // plus the current target on every field-type lookup showed up hot.
    // Only the emitted path is allocated.
    if let Some(path) = TYPE_FILES.with(|c| {
        let map = c.borrow();
        let (type_file, pkg, public_id) = map.as_ref()?.get(key)?;
        let same_file = CURRENT_TARGET.with(|t| {
            let current = t.borrow();
            !current.is_empty() && file_matches_single(&current, type_file)
        });
        if same_file {
            return Some(public_id.clone());
        }
        if !pkg.is_empty() {
            let pkg_path = pkg_mod_path(pkg);
            let rel = key
                .strip_prefix(pkg.as_str())
                .unwrap_or(key)
                .trim_start_matches('.');
            if rel.contains('.') {
                let parts: Vec<&str> = rel.split('.').collect();
                let mod_parts: Vec<String> = parts[..parts.len().saturating_sub(1)]
                    .iter()
                    .map(|p| to_snake(&ident_last(p)))
                    .collect();
                Some(format!("{pkg_path}::{}::{public_id}", mod_parts.join("::")))
            } else {
                Some(format!("{pkg_path}::{public_id}"))
            }
        } else {
            let rel = key.rsplit_once('.').map(|x| x.0).unwrap_or("");
            if rel.is_empty() {
                Some(format!("crate::{public_id}"))
            } else {
                let mods = rel
                    .split('.')
                    .map(|p| to_snake(&ident_last(p)))
                    .collect::<Vec<_>>()
                    .join("::");
                Some(format!("crate::{mods}::{public_id}"))
            }
        }
    }) {
        TYPE_PATHS.with(|paths| {
            paths.borrow_mut().insert(key.to_owned(), path.clone());
        });
        return path;
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
    // Field accessors are inherent methods on the generated message.  Keep
    // legal protobuf names from shadowing trait methods used by the runtime or
    // colliding with helpers emitted into the same inherent impl.  The
    // deterministic `_field` suffix is part of the generated Rust naming
    // convention (the protobuf descriptor name remains unchanged).
    if matches!(
        name,
        "new"
            | "default"
            | "clone"
            | "eq"
            | "hash"
            | "fmt"
            | "parse"
            | "serialize"
            | "serialized_len"
            | "encode"
            | "clear"
            | "clear_and_parse"
            | "clear_and_parse_dont_enforce_required"
            | "merge_from_bytes"
            | "merge_from_bytes_shared"
            | "merge_from_bytes_dont_enforce_required"
            | "merge_from"
            | "copy_from"
            | "take_from"
            | "as_view"
            | "as_mut"
            | "into_view"
            | "into_mut"
            | "check_required"
            | "merge_bytes"
            | "merge_bytes_dont_enforce"
            | "merge_inner"
            | "__pbrs_merge_mode"
            | "__pbrs_merge_prevalidated"
            | "__pbrs_merge_heavy"
            | "write_to"
            | "compute_size"
            | "to_json"
            | "from_json"
            | "from_json_ignore"
            | "to_text"
            | "to_text_with_unknown"
            | "from_text"
            | "cold_mut"
    ) {
        return format!("{name}_field");
    }
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

/// Whether `emit_accessors` adds a borrowed message-view method for this field.
pub(crate) fn has_view_accessor(f: &FieldDescriptor) -> bool {
    !f.is_map
        && f.cardinality != Cardinality::Repeated
        && matches!(f.field_type, FieldType::Message | FieldType::Group)
}

/// The inherent method names emitted for a field, without Rust raw prefixes.
/// Keep this in step with `messages::emit_accessors`: maps and repeated fields
/// return before the singular-field presence and view methods are emitted.
pub(crate) fn field_accessor_names(f: &FieldDescriptor, raw: &str, id: &str) -> Vec<String> {
    let id = id.trim_start_matches("r#");
    let mut names = Vec::with_capacity(7);
    names.push(id.to_string());
    names.push(format!("set_{raw}"));
    if f.is_map || f.cardinality == Cardinality::Repeated {
        names.push(format!("{id}_mut"));
        return names;
    }
    if has_view_accessor(f) {
        names.push(format!("{id}_mut"));
        names.push(format!("{id}_view"));
    }
    if is_option(f) {
        names.push(format!("{id}_opt"));
        names.push(format!("has_{raw}"));
        names.push(format!("clear_{raw}"));
    }
    names
}

fn avoid_view_accessor_collisions(
    desc: &MessageDescriptor,
    ids: &mut std::collections::BTreeMap<u32, Rc<str>>,
    raws: &mut std::collections::BTreeMap<u32, Rc<str>>,
) {
    // Every view method ends in `_view`. Most messages cannot have a conflict,
    // so keep their established allocation path free of method-name tables.
    if !desc.fields.values().any(has_view_accessor)
        || !desc
            .fields
            .values()
            .any(|f| ids[&f.number].ends_with("_view") || raws[&f.number].ends_with("_view"))
    {
        return;
    }

    let mut views = std::collections::BTreeMap::new();
    let mut methods = std::collections::BTreeMap::<String, usize>::new();
    for f in desc.fields.values() {
        let id = &ids[&f.number];
        if has_view_accessor(f) {
            views.insert(format!("{}_view", id.trim_start_matches("r#")), f.number);
        }
        for name in field_accessor_names(f, &raws[&f.number], id) {
            *methods.entry(name).or_default() += 1;
        }
    }

    // Reserve every actual view first. Only schemas with an emitted-method
    // collision change names; e.g. scalar/repeated `child` + `child_view` and
    // renamed `clone` + `clone_view` keep their existing public accessors.
    for f in desc.fields.values() {
        let old_id = &ids[&f.number];
        let old_names = field_accessor_names(f, &raws[&f.number], old_id);
        if !old_names
            .iter()
            .any(|name| views.get(name).is_some_and(|owner| *owner != f.number))
        {
            continue;
        }
        for name in old_names {
            let count = methods
                .get_mut(&name)
                .expect("every old accessor was counted before collision resolution");
            *count -= 1;
            if *count == 0 {
                methods.remove(&name);
            }
        }
        if has_view_accessor(f) {
            views.remove(&format!("{}_view", old_id.trim_start_matches("r#")));
        }
        let base = format!("{}_{}", old_id.trim_start_matches("r#"), f.number);
        let mut candidate = base.clone();
        let mut suffix = 2;
        loop {
            let names = field_accessor_names(f, &candidate, &candidate);
            if names.iter().all(|name| !methods.contains_key(name)) {
                for name in names {
                    *methods.entry(name).or_default() += 1;
                }
                if has_view_accessor(f) {
                    views.insert(format!("{candidate}_view"), f.number);
                }
                let candidate: Rc<str> = Rc::from(candidate);
                ids.insert(f.number, Rc::clone(&candidate));
                raws.insert(f.number, candidate);
                break;
            }
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
    }
}

pub(crate) fn bind_field_idents(desc: &MessageDescriptor) {
    bind_boxed_fields(desc);
    bind_bytes_fields(desc);
    let mut used = std::collections::BTreeSet::new();
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
        // `unknown` is reserved (unknown-fields storage); treating it as
        // used without seeding an allocation matches the old `insert`.
        if id == "unknown" || !used.insert(id.clone()) {
            id = format!("{}_{}", id.trim_start_matches("r#"), f.number);
            used.insert(id.clone());
        }
        let raw: Rc<str> = Rc::from(raw);
        let id = if id == raw.as_ref() {
            Rc::clone(&raw)
        } else {
            Rc::from(id)
        };
        out.insert(f.number, id);
        raws.insert(f.number, raw);
    }
    avoid_view_accessor_collisions(desc, &mut out, &mut raws);
    FIELD_IDENTS.with(|c| *c.borrow_mut() = out);
    FIELD_RAWS.with(|c| *c.borrow_mut() = raws);
    bind_bytes_accessors(desc);
}

pub(crate) fn field_id(f: &FieldDescriptor) -> Rc<str> {
    FIELD_IDENTS.with(|c| {
        c.borrow()
            .get(&f.number)
            .cloned()
            .unwrap_or_else(|| Rc::from(field_ident_base(&f.name)))
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

pub(crate) fn scalar_type(field: &FieldDescriptor) -> Cow<'static, str> {
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
        FieldType::String => runtime_path("::pbrs::rt::LazyStr"),
        FieldType::Bytes if is_selected_bytes(field) => runtime_path("::pbrs::ProtoBytes"),
        FieldType::Bytes => runtime_path("::pbrs::rt::LazyBytes"),
        FieldType::Message | FieldType::Group => rust_type_path(
            field
                .type_name
                .as_deref()
                .or_else(|| field.message.as_ref().map(|m| m.full_name.as_str()))
                .unwrap_or("UnknownMsg"),
        )
        .into(),
    }
}

pub(crate) fn map_kv(field: &FieldDescriptor) -> (Cow<'static, str>, Cow<'static, str>) {
    let entry = field.message.as_ref();
    let k = entry
        .and_then(|e| e.field(1))
        .map(scalar_type)
        .unwrap_or_else(|| runtime_path("::pbrs::ProtoString"));
    let v = entry
        .and_then(|e| e.field(2))
        .map(|value| {
            if value.field_type == FieldType::Bytes {
                if is_selected_bytes(field) {
                    runtime_path("::pbrs::ProtoBytes")
                } else {
                    runtime_path("::pbrs::rt::LazyBytes")
                }
            } else {
                scalar_type(value)
            }
        })
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
        && !eager_message_field(f)
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

fn enum_prefixes(enum_name: &str) -> [String; 3] {
    [
        enum_name.to_string(),
        screaming_snake_to_upper_camel(enum_name),
        camel_to_snake_name(enum_name),
    ]
}

fn strip_enum_prefix_with<'a>(prefixes: &[String; 3], value_name: &'a str) -> &'a str {
    for p in prefixes {
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

fn enum_value_rs_name_with(
    prefixes: &[String; 3],
    value_name: &str,
    retain_prefix: bool,
) -> String {
    let stripped = if retain_prefix {
        value_name
    } else {
        strip_enum_prefix_with(prefixes, value_name)
    };
    let mut name = screaming_snake_to_upper_camel(stripped);
    if name.is_empty() {
        name = screaming_snake_to_upper_camel(value_name);
    }
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        name = format!("_{name}");
    }
    if name == "Self" {
        // Rust forbids `r#Self`, including for an associated enum constant.
        // This spelling also matches Prost's enum-prefix handling.
        "Self_".to_string()
    } else if is_rust_keyword(&name) {
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
    // Prefixes depend only on the enum, not the value: build once.
    let prefixes = enum_prefixes(enum_name);
    let retain_prefix = RETAIN_ENUM_PREFIX.with(|enabled| enabled.get());
    let mut seen_name = std::collections::BTreeSet::new();
    let mut by_number: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    let mut out: Vec<RustEnumValue> = Vec::new();
    for (number, proto_name) in listed {
        let rust_name = enum_value_rs_name_with(&prefixes, proto_name, retain_prefix);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattened_names_reserve_proxies_and_recheck_parent_prefixes() {
        let _guard = CodegenStateGuard::new();
        let names = [
            "pkg.A",
            "pkg.A.X",
            "pkg.B",
            "pkg.B.X",
            "pkg.C",
            "pkg.C.B",
            "pkg.C.B.X",
            "pkg.D",
            "pkg.D.B",
            "pkg.D.B.X",
            "pkg.E",
            "pkg.E.B",
            "pkg.E.B.X",
            "pkg.F",
            "pkg.F.B",
            "pkg.F.B.X",
            "pkg.BX_2",
            "pkg.BX_3View",
            "pkg.Foo",
            "pkg.FooMut",
            "pkg.FooView",
            "pkg.RawA.type",
            "pkg.RawB.type",
        ]
        .map(str::to_string);
        let messages = names
            .iter()
            .filter(|full| full.as_str() != "pkg.F.B.X")
            .cloned()
            .collect();
        let aliases = scoped_type_idents(names.iter().map(String::as_str), &messages);
        let index = aliases
            .into_iter()
            .map(|(full, id)| (full, ("types.proto".to_string(), "pkg".to_string(), id)))
            .collect();
        TYPE_FILES.with(|files| *files.borrow_mut() = Some(std::sync::Arc::new(index)));
        let ids = flat_type_idents(&names, &messages);
        assert_eq!(ids.get("pkg.A.X").map(String::as_str), Some("X"));
        assert_eq!(ids.get("pkg.B.X").map(String::as_str), Some("BX"));
        assert_eq!(ids.get("pkg.C.B.X").map(String::as_str), Some("BX_4"));
        assert_eq!(ids.get("pkg.D.B.X").map(String::as_str), Some("BX_5"));
        assert_eq!(ids.get("pkg.E.B.X").map(String::as_str), Some("BX_6"));
        assert_eq!(ids.get("pkg.F.B.X").map(String::as_str), Some("BX_3"));
        assert_eq!(ids.get("pkg.FooMut").map(String::as_str), Some("FooMut_"));
        assert_eq!(ids.get("pkg.FooView").map(String::as_str), Some("FooView_"));
        assert_eq!(ids.get("pkg.RawA.type").map(String::as_str), Some("r#type"));
        assert_eq!(
            ids.get("pkg.RawB.type").map(String::as_str),
            Some("RawBtype")
        );
        let mut emitted = std::collections::BTreeSet::new();
        for (full, id) in &ids {
            let bare = id.trim_start_matches("r#");
            assert!(emitted.insert(bare.to_string()));
            if messages.contains(full) {
                assert!(emitted.insert(format!("{bare}View")));
                assert!(emitted.insert(format!("{bare}Mut")));
            }
        }
    }

    #[test]
    fn enum_self_values_use_legal_identifiers() {
        let prefixes = enum_prefixes("Keyword");
        for retain_prefix in [false, true] {
            assert_eq!(
                enum_value_rs_name_with(&prefixes, "SELF", retain_prefix),
                "Self_"
            );
        }
        let prefixes = enum_prefixes("Foo");
        assert_eq!(
            enum_value_rs_name_with(&prefixes, "FOO_SELF", false),
            "Self_"
        );
        assert_eq!(
            enum_value_rs_name_with(&prefixes, "FOO_SELF", true),
            "FooSelf"
        );
        assert_eq!(
            enum_value_rs_name_with(&prefixes, "FOO_READY", false),
            "Ready"
        );
    }

    #[test]
    fn imported_aliases_and_renamed_external_parents_keep_their_scope() {
        let _guard = CodegenStateGuard::new();
        let index = TypeFileIndex::from([
            (
                "pkg.FooMut".to_string(),
                (
                    "types.proto".to_string(),
                    "pkg".to_string(),
                    "FooMut_".to_string(),
                ),
            ),
            (
                "pkg.FooView".to_string(),
                (
                    "types.proto".to_string(),
                    "pkg".to_string(),
                    "FooView_".to_string(),
                ),
            ),
            (
                "pkg.FooMut.Child".to_string(),
                (
                    "types.proto".to_string(),
                    "pkg".to_string(),
                    "Child".to_string(),
                ),
            ),
            (
                "Loose.Child".to_string(),
                (
                    "loose.proto".to_string(),
                    String::new(),
                    "Child".to_string(),
                ),
            ),
        ]);
        TYPE_FILES.with(|files| *files.borrow_mut() = Some(std::sync::Arc::new(index)));
        CURRENT_TARGET.with(|target| *target.borrow_mut() = "consumer.proto".to_string());
        EXTERN_PATHS.with(|paths| {
            paths
                .borrow_mut()
                .push((".pkg.FooMut".to_string(), "::provider::FooMut_".to_string()))
        });
        assert_eq!(rust_type_path("pkg.FooMut"), "::provider::FooMut_");
        assert_eq!(
            rust_type_path("pkg.FooMut.Child"),
            "::provider::foo_mut::Child"
        );
        assert_eq!(rust_type_path("pkg.FooView"), "crate::pkg::FooView_");
        assert_eq!(rust_type_path("Loose.Child"), "crate::loose::Child");
        assert_eq!(
            format_extern_rel_path("pkg.FooMut", "::prost_provider::FooMut", "Child"),
            "::prost_provider::foo_mut::Child"
        );
        assert_eq!(
            format_extern_rel_path("pkg", "::custom_namespace", "FooMut"),
            "::custom_namespace::FooMut"
        );
    }

    #[test]
    fn imported_paths_keep_local_identifiers_and_request_options_separate() {
        {
            let _guard = CodegenStateGuard::new();
            let key = "pkg.Parent.Child";
            let index = TypeFileIndex::from([(
                key.to_string(),
                (
                    "dependency.proto".to_string(),
                    "pkg".to_string(),
                    "Child".to_string(),
                ),
            )]);
            TYPE_FILES.with(|files| *files.borrow_mut() = Some(std::sync::Arc::new(index)));
            CURRENT_TARGET.with(|target| *target.borrow_mut() = "consumer.proto".to_string());
            for _ in 0..3 {
                assert_eq!(rust_type_path(key), "crate::pkg::parent::Child");
                assert_eq!(rust_ident(key), "Child");
            }
            IDENTS.with(|ids| {
                ids.borrow_mut()
                    .insert(key.to_string(), "FlatChild".to_string());
            });
            assert_eq!(rust_type_path(key), "FlatChild");
            EXTERN_PATHS.with(|paths| {
                paths
                    .borrow_mut()
                    .push((".pkg".to_string(), "::external".to_string()));
            });
            assert_eq!(rust_type_path(key), "::external::parent::Child");
        }
        let _guard = CodegenStateGuard::new();
        assert_eq!(rust_type_path("pkg.Parent.Child"), "Child");
    }
}
