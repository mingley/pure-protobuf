//! Opt-in native singular int32 extension generation.

use super::*;
use crate::dynamic::{Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor};
use crate::wire::{WIRE_LEN, WIRE_VARINT, decode_tag, read_len_bytes, skip_field};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) struct SelectedExtension {
    host: Arc<MessageDescriptor>,
    full_name: String,
    identifier: String,
    number: u32,
    default: i32,
}

fn selection_error(name: &str, reason: &str) -> CodegenError {
    CodegenError::InvalidParameter {
        key: "typed_extension".into(),
        detail: format!("typed extension {name:?}: {reason}"),
    }
}

fn int32_default(text: &str) -> Option<i32> {
    let (negative, magnitude) = if let Some(value) = text.strip_prefix('-') {
        (true, value)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    let (radix, digits) = if let Some(value) = magnitude
        .strip_prefix("0x")
        .or_else(|| magnitude.strip_prefix("0X"))
    {
        (16, value)
    } else if magnitude.len() > 1 && magnitude.starts_with('0') {
        (8, magnitude)
    } else {
        (10, magnitude)
    };
    if digits.starts_with('+') || digits.starts_with('-') {
        return None;
    }
    let value = i64::from_str_radix(digits, radix).ok()?;
    let value = if negative {
        value.checked_neg()?
    } else {
        value
    };
    i32::try_from(value).ok()
}

pub(crate) fn select_typed_extensions(
    pool: &DescriptorPool,
    names: &[String],
    targets: &BTreeSet<String>,
) -> Result<Vec<SelectedExtension>, CodegenError> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    let mut identifiers = BTreeMap::new();
    let mut package_owners = BTreeMap::new();
    for name in names {
        if !seen.insert(name.as_str()) {
            continue;
        }
        let (host, field) = pool
            .get_extension(name)
            .ok_or_else(|| selection_error(name, "extension was not found"))?;
        let file = pool
            .file_for_extension(&host.full_name, field.number)
            .ok_or_else(|| selection_error(name, "declaration file was not found"))?;
        if field.extension_name.as_deref() != Some(name.as_str()) {
            return Err(selection_error(
                name,
                "descriptor field is misbound to another declaration",
            ));
        }
        if pool.file_edition(file) != Some(1001) {
            return Err(selection_error(name, "only Edition 2024 is supported"));
        }
        if field.field_type != FieldType::Int32
            || field.cardinality != Cardinality::Optional
            || field.delimited
            || field.is_map
        {
            return Err(selection_error(name, "only singular int32 is supported"));
        }
        if file != host.file_name {
            return Err(selection_error(
                name,
                "cross-file extendees are not supported",
            ));
        }
        if !file_matches(targets, file) || is_extern_type(&host.full_name) || host.is_map_entry {
            return Err(selection_error(
                name,
                "host must be an owned generated target type",
            ));
        }
        if host.message_set_wire_format {
            return Err(selection_error(name, "MessageSet is not supported"));
        }
        if field.number == 0
            || field.number > crate::wire::MAX_FIELD_NUMBER
            || (19_000..=19_999).contains(&field.number)
        {
            return Err(selection_error(name, "field number is illegal or reserved"));
        }
        if !host
            .extension_ranges
            .iter()
            .any(|(start, end)| (*start..*end).contains(&field.number))
        {
            return Err(selection_error(
                name,
                "field number is outside its host extension ranges",
            ));
        }
        let default = match field.default.as_deref() {
            None => 0,
            Some(value) => int32_default(value)
                .ok_or_else(|| selection_error(name, "int32 default is invalid"))?,
        };
        let descriptor = pool
            .get_file(file)
            .ok_or_else(|| selection_error(name, "declaration file was not found"))?;
        let package = descriptor.package.as_str();
        if let Some(owner) = package_owners.insert(package.to_string(), file.to_string()) {
            if owner != file {
                return Err(selection_error(
                    name,
                    "extensions module has multiple selected owner files in one package",
                ));
            }
        }
        let relative = name
            .strip_prefix(package)
            .and_then(|relative| relative.strip_prefix('.'))
            .unwrap_or(name);
        let identifier = relative
            .split('.')
            .map(|part| to_snake(part).trim_start_matches("r#").to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("_");
        if let Some(previous) = identifiers.insert((file.to_string(), identifier.clone()), name) {
            return Err(selection_error(
                name,
                &format!("identifier {identifier} collides with {previous}"),
            ));
        }
        selected.push(SelectedExtension {
            host,
            full_name: name.clone(),
            identifier,
            number: field.number,
            default,
        });
    }
    selected.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(selected)
}

fn raw_error() -> CodegenError {
    selection_error(
        "<descriptor>",
        "malformed raw descriptor during selected extension validation",
    )
}

#[derive(Default)]
struct RawField<'a> {
    name: &'a str,
    extendee: &'a str,
    number: u32,
}

fn raw_string(bytes: &[u8]) -> Result<&str, CodegenError> {
    std::str::from_utf8(bytes).map_err(|_| raw_error())
}

fn raw_field(bytes: &[u8]) -> Result<RawField<'_>, CodegenError> {
    let mut field = RawField::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (tag, wire) = decode_tag(bytes, &mut pos).map_err(|_| raw_error())?;
        match (tag, wire) {
            (1, WIRE_LEN) => {
                field.name = raw_string(read_len_bytes(bytes, &mut pos).map_err(|_| raw_error())?)?;
            }
            (2, WIRE_LEN) => {
                field.extendee =
                    raw_string(read_len_bytes(bytes, &mut pos).map_err(|_| raw_error())?)?;
            }
            (3, WIRE_VARINT) => {
                let number =
                    crate::wire::decode_varint(bytes, &mut pos).map_err(|_| raw_error())?;
                field.number = u32::try_from(number).map_err(|_| raw_error())?;
            }
            _ => skip_field(bytes, &mut pos, wire).map_err(|_| raw_error())?,
        }
    }
    Ok(field)
}

fn qualified_name(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

struct RawSelections<'a> {
    selected: &'a [SelectedExtension],
    pairs: BTreeMap<(String, u32), (usize, usize)>,
    names: BTreeMap<String, usize>,
}

impl RawSelections<'_> {
    fn extension(&mut self, prefix: &str, bytes: &[u8]) -> Result<(), CodegenError> {
        let field = raw_field(bytes)?;
        let name = qualified_name(prefix, field.name);
        let host = field.extendee.trim_start_matches('.');
        if let Some(count) = self.names.get_mut(&name) {
            *count += 1;
            if !self.selected.iter().any(|extension| {
                extension.full_name == name
                    && extension.host.full_name == host
                    && extension.number == field.number
            }) {
                return Err(selection_error(
                    &name,
                    "raw declaration does not match selected host/tag",
                ));
            }
        }
        if let Some((_, extensions)) = self.pairs.get_mut(&(host.to_string(), field.number)) {
            *extensions += 1;
        }
        Ok(())
    }

    fn ordinary(&mut self, host: &str, bytes: &[u8]) -> Result<(), CodegenError> {
        let field = raw_field(bytes)?;
        if let Some((ordinary, _)) = self.pairs.get_mut(&(host.to_string(), field.number)) {
            *ordinary += 1;
        }
        Ok(())
    }
}

/// The descriptor pool flattens fields by tag. Validate selected raw records
/// before emitting constants so collisions cannot hide behind map overwrite.
pub(crate) fn validate_raw_typed_extensions(
    selected: &[SelectedExtension],
    files: &[Vec<u8>],
) -> Result<(), CodegenError> {
    let mut facts = RawSelections {
        selected,
        pairs: selected
            .iter()
            .map(|extension| ((extension.host.full_name.clone(), extension.number), (0, 0)))
            .collect(),
        names: selected
            .iter()
            .map(|extension| (extension.full_name.clone(), 0))
            .collect(),
    };
    for file in files {
        let mut package = "";
        let mut messages = Vec::new();
        let mut extensions = Vec::new();
        let mut pos = 0;
        while pos < file.len() {
            let (tag, wire) = decode_tag(file, &mut pos).map_err(|_| raw_error())?;
            match (tag, wire) {
                (2, WIRE_LEN) => {
                    package = raw_string(read_len_bytes(file, &mut pos).map_err(|_| raw_error())?)?
                }
                (4, WIRE_LEN) => {
                    messages.push(read_len_bytes(file, &mut pos).map_err(|_| raw_error())?)
                }
                (7, WIRE_LEN) => {
                    extensions.push(read_len_bytes(file, &mut pos).map_err(|_| raw_error())?)
                }
                _ => skip_field(file, &mut pos, wire).map_err(|_| raw_error())?,
            }
        }
        for extension in extensions {
            facts.extension(package, extension)?;
        }
        let mut pending: Vec<_> = messages
            .into_iter()
            .map(|message| (package.to_string(), message, 0_u32))
            .collect();
        while let Some((prefix, message, depth)) = pending.pop() {
            if depth >= crate::RECURSION_LIMIT {
                return Err(selection_error(
                    "<descriptor>",
                    "nested descriptor depth exceeds the existing recursion limit",
                ));
            }
            let mut name = "";
            let mut fields = Vec::new();
            let mut nested = Vec::new();
            let mut extensions = Vec::new();
            let mut pos = 0;
            while pos < message.len() {
                let (tag, wire) = decode_tag(message, &mut pos).map_err(|_| raw_error())?;
                match (tag, wire) {
                    (1, WIRE_LEN) => {
                        name =
                            raw_string(read_len_bytes(message, &mut pos).map_err(|_| raw_error())?)?
                    }
                    (2, WIRE_LEN) => {
                        fields.push(read_len_bytes(message, &mut pos).map_err(|_| raw_error())?)
                    }
                    (3, WIRE_LEN) => {
                        nested.push(read_len_bytes(message, &mut pos).map_err(|_| raw_error())?)
                    }
                    (6, WIRE_LEN) => {
                        extensions.push(read_len_bytes(message, &mut pos).map_err(|_| raw_error())?)
                    }
                    _ => skip_field(message, &mut pos, wire).map_err(|_| raw_error())?,
                }
            }
            let host = qualified_name(&prefix, name);
            for field in fields {
                facts.ordinary(&host, field)?;
            }
            for extension in extensions {
                facts.extension(&host, extension)?;
            }
            for nested in nested {
                pending.push((host.clone(), nested, depth + 1));
            }
        }
    }
    for extension in selected {
        if facts.names.get(&extension.full_name) != Some(&1) {
            return Err(selection_error(
                &extension.full_name,
                "selected declaration is missing or duplicated",
            ));
        }
        if facts
            .pairs
            .get(&(extension.host.full_name.clone(), extension.number))
            != Some(&(0, 1))
        {
            return Err(selection_error(
                &extension.full_name,
                "selected tag collides with a regular field or another extension",
            ));
        }
    }
    Ok(())
}

fn method_collision(field: &FieldDescriptor) -> bool {
    let id = field_id(field);
    let raw = field_raw(field);
    [
        id,
        format!("set_{raw}"),
        format!("has_{raw}"),
        format!("clear_{raw}"),
    ]
    .iter()
    .any(|method| {
        matches!(
            method.as_str(),
            "get_extension" | "has_extension" | "set_extension" | "clear_extension"
        )
    })
}

/// Check the actual emitted type/method namespaces before returning output.
pub(crate) fn validate_typed_extension_emission(
    selected: &[SelectedExtension],
    messages: &[String],
    enums: &[String],
    pool: &DescriptorPool,
    target: &str,
    public_files: &[String],
) -> Result<(), CodegenError> {
    if !selected
        .iter()
        .any(|extension| file_matches_single(target, &extension.host.file_name))
    {
        return Ok(());
    }
    let public_set: BTreeSet<_> = public_files.iter().cloned().collect();
    let mut visible_messages = messages.to_vec();
    let mut visible_enums = enums.to_vec();
    for name in pool.collect_names() {
        if pool
            .get_message(&name)
            .is_some_and(|message| file_matches(&public_set, &message.file_name))
        {
            visible_messages.push(name);
        }
    }
    for name in pool.collect_enum_names() {
        if pool
            .get_enum(&name)
            .is_some_and(|enumeration| file_matches(&public_set, &enumeration.file_name))
        {
            visible_enums.push(name);
        }
    }
    let message_set: BTreeSet<_> = visible_messages.iter().map(String::as_str).collect();
    for name in visible_messages.iter().chain(&visible_enums) {
        if rust_ident(name) == "extensions" {
            return Err(selection_error(
                name,
                "type collides with extensions module",
            ));
        }
        let mut outer = name.as_str();
        while let Some((parent, _)) = outer.rsplit_once('.') {
            if !message_set.contains(parent) {
                break;
            }
            if to_snake(&ident_last(parent)) == "extensions" {
                return Err(selection_error(
                    name,
                    "nested module collides with extensions module",
                ));
            }
            outer = parent;
        }
    }
    let mut hosts = BTreeSet::new();
    let mut identifiers = BTreeSet::new();
    for extension in selected {
        if !file_matches_single(target, &extension.host.file_name) {
            continue;
        }
        if !messages.contains(&extension.host.full_name) {
            return Err(selection_error(
                &extension.full_name,
                "host was not emitted as an owned type",
            ));
        }
        if !identifiers.insert(&extension.identifier) {
            return Err(selection_error(
                &extension.full_name,
                "identifier collides in the emitted extensions module",
            ));
        }
        if !hosts.insert(&extension.host.full_name) {
            continue;
        }
        let mut ordinary = pool
            .get_message(&extension.host.full_name)
            .ok_or_else(|| selection_error(&extension.full_name, "host was not found"))?
            .as_ref()
            .clone();
        ordinary
            .fields
            .retain(|_, field| field.extension_name.is_none());
        bind_field_idents(&ordinary);
        if ordinary.fields.values().any(method_collision) {
            return Err(selection_error(
                &extension.full_name,
                "field accessor collides with typed extension methods",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_typed_extension_services(
    selected: &[SelectedExtension],
    target: &str,
    services: &[Arc<crate::dynamic::ServiceDescriptor>],
    config: &ResolvedConfig,
) -> Result<(), CodegenError> {
    if config.build_server
        && config.stubs != StubStyle::None
        && selected
            .iter()
            .any(|extension| file_matches_single(target, &extension.host.file_name))
    {
        for service in services {
            if rust_ident(&service.full_name) == "extensions" {
                return Err(selection_error(
                    &service.full_name,
                    "emitted service trait collides with extensions module",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn emit_typed_extensions(
    src: &mut String,
    selected: &[SelectedExtension],
    target: &str,
) {
    let selected: Vec<_> = selected
        .iter()
        .filter(|extension| file_matches_single(target, &extension.host.file_name))
        .collect();
    if selected.is_empty() {
        return;
    }
    let mut hosts = BTreeSet::new();
    for extension in &selected {
        if !hosts.insert(&extension.host.full_name) {
            continue;
        }
        let host = rust_ident(&extension.host.full_name);
        let _ = writeln!(src, "impl pbrs::ExtensionHost for {host} {{");
        let _ = writeln!(
            src,
            "    fn __extension_fields(&self) -> &UnknownFields {{ &self.unknown }}"
        );
        let _ = writeln!(
            src,
            "    fn __extension_fields_mut(&mut self) -> &mut UnknownFields {{ self.cached_size.dirty(); &mut self.unknown }}"
        );
        let _ = writeln!(src, "}}");
        let _ = writeln!(src, "impl {host} {{");
        let _ = writeln!(
            src,
            "    /// Read a selected scalar extension or its declared default."
        );
        let _ = writeln!(
            src,
            "    pub fn get_extension<V: pbrs::ExtensionValue>(&self, e: &pbrs::Extension<Self, V>) -> V {{ e.get(self) }}"
        );
        let _ = writeln!(
            src,
            "    /// Whether a selected scalar extension is explicitly present."
        );
        let _ = writeln!(
            src,
            "    pub fn has_extension<V: pbrs::ExtensionValue>(&self, e: &pbrs::Extension<Self, V>) -> bool {{ e.has(self) }}"
        );
        let _ = writeln!(
            src,
            "    /// Set a selected scalar extension, preserving other unknown records."
        );
        let _ = writeln!(
            src,
            "    pub fn set_extension<V: pbrs::ExtensionValue>(&mut self, e: &pbrs::Extension<Self, V>, value: V) {{ e.set(self, value); }}"
        );
        let _ = writeln!(
            src,
            "    /// Clear matching scalar records, preserving other wire types."
        );
        let _ = writeln!(
            src,
            "    pub fn clear_extension<V: pbrs::ExtensionValue>(&mut self, e: &pbrs::Extension<Self, V>) {{ e.clear(self); }}"
        );
        let _ = writeln!(src, "}}");
    }
    let _ = writeln!(
        src,
        "/// Explicitly selected native scalar extension identifiers."
    );
    let _ = writeln!(src, "pub mod extensions {{");
    for extension in selected {
        let host = rust_ident(&extension.host.full_name);
        let _ = writeln!(
            src,
            "    /// The `{}` singular int32 extension.",
            extension.full_name
        );
        let _ = writeln!(
            src,
            "    pub const {}: pbrs::Extension<super::{host}, i32> = pbrs::Extension::__new({}, {:?}, {}).expect(\"validated extension number\");",
            extension.identifier, extension.number, extension.full_name, extension.default
        );
    }
    let _ = writeln!(src, "}}");
}
