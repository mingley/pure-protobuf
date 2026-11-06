//! Optional per-field eager message storage.

use super::*;
use crate::ParseError;
use crate::dynamic::{Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor};
use crate::wire::{WIRE_LEN, WIRE_VARINT, decode_tag, decode_varint, read_len_bytes};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Default)]
struct MessageParts<'a> {
    name: &'a str,
    fields: Vec<&'a [u8]>,
    nested: Vec<&'a [u8]>,
    oneofs: Vec<&'a str>,
}

#[derive(Default)]
struct FieldParts<'a> {
    name: &'a str,
    number: u32,
    oneof: Option<usize>,
    proto3_optional: bool,
}

fn descriptor_text(data: &[u8]) -> Result<&str, ParseError> {
    std::str::from_utf8(data).map_err(|_| ParseError::new("invalid UTF-8 descriptor name"))
}

fn descriptor_name(data: &[u8]) -> Result<&str, ParseError> {
    let mut name = "";
    let mut pos = 0;
    while pos < data.len() {
        let (number, wire_type) = decode_tag(data, &mut pos)?;
        if (number, wire_type) == (1, WIRE_LEN) {
            name = descriptor_text(read_len_bytes(data, &mut pos)?)?;
        } else {
            crate::wire::skip_field_with_number_and_depth(data, &mut pos, number, wire_type, 0)?;
        }
    }
    Ok(name)
}

fn message_parts(data: &[u8]) -> Result<MessageParts<'_>, ParseError> {
    let mut parts = MessageParts::default();
    let mut pos = 0;
    while pos < data.len() {
        let (number, wire_type) = decode_tag(data, &mut pos)?;
        match (number, wire_type) {
            (1, WIRE_LEN) => parts.name = descriptor_text(read_len_bytes(data, &mut pos)?)?,
            (2, WIRE_LEN) => parts.fields.push(read_len_bytes(data, &mut pos)?),
            (3, WIRE_LEN) => parts.nested.push(read_len_bytes(data, &mut pos)?),
            (8, WIRE_LEN) => parts
                .oneofs
                .push(descriptor_name(read_len_bytes(data, &mut pos)?)?),
            _ => {
                crate::wire::skip_field_with_number_and_depth(data, &mut pos, number, wire_type, 0)?
            }
        }
    }
    Ok(parts)
}

fn field_parts(data: &[u8]) -> Result<FieldParts<'_>, ParseError> {
    let mut parts = FieldParts::default();
    let mut pos = 0;
    while pos < data.len() {
        let (number, wire_type) = decode_tag(data, &mut pos)?;
        match (number, wire_type) {
            (1, WIRE_LEN) => parts.name = descriptor_text(read_len_bytes(data, &mut pos)?)?,
            (3, WIRE_VARINT) => {
                parts.number = u32::try_from(decode_varint(data, &mut pos)?)
                    .map_err(|_| ParseError::new("descriptor field number overflow"))?;
            }
            (9, WIRE_VARINT) => {
                parts.oneof = Some(
                    usize::try_from(decode_varint(data, &mut pos)?)
                        .map_err(|_| ParseError::new("descriptor oneof index overflow"))?,
                );
            }
            (17, WIRE_VARINT) => parts.proto3_optional = decode_varint(data, &mut pos)? != 0,
            _ => {
                crate::wire::skip_field_with_number_and_depth(data, &mut pos, number, wire_type, 0)?
            }
        }
    }
    Ok(parts)
}

fn collect_boxed_fields(
    files: &[Vec<u8>],
    pool: &DescriptorPool,
    selectors: &[String],
) -> Result<BoxedFieldIndex, ParseError> {
    let mut selected = BTreeMap::<String, BTreeSet<u32>>::new();
    for file in files {
        let mut package = "";
        let mut roots = Vec::new();
        let mut pos = 0;
        while pos < file.len() {
            let (number, wire_type) = decode_tag(file, &mut pos)?;
            match (number, wire_type) {
                (2, WIRE_LEN) => package = descriptor_text(read_len_bytes(file, &mut pos)?)?,
                (4, WIRE_LEN) => roots.push(read_len_bytes(file, &mut pos)?),
                _ => crate::wire::skip_field_with_number_and_depth(
                    file, &mut pos, number, wire_type, 0,
                )?,
            }
        }
        // Iterative traversal keeps descriptor nesting off the Rust stack.
        let mut pending: Vec<_> = roots
            .into_iter()
            .map(|data| (package.to_owned(), data))
            .collect();
        while let Some((parent, data)) = pending.pop() {
            let parts = message_parts(data)?;
            let full_name = if parent.is_empty() {
                parts.name.to_string()
            } else {
                format!("{parent}.{}", parts.name)
            };
            for nested in parts.nested {
                pending.push((full_name.clone(), nested));
            }
            let message = pool
                .get_message(&full_name)
                .ok_or_else(|| ParseError::new("boxed message descriptor was not found"))?;
            if message.is_map_entry {
                continue;
            }
            for data in parts.fields {
                let raw = field_parts(data)?;
                let field = message
                    .field(raw.number)
                    .ok_or_else(|| ParseError::new("boxed field descriptor was not found"))?;
                if field.cardinality == Cardinality::Repeated
                    || field.is_map
                    || !matches!(field.field_type, FieldType::Message | FieldType::Group)
                {
                    continue;
                }
                let path = if let Some(oneof) = raw.oneof.filter(|_| !raw.proto3_optional) {
                    let oneof = parts
                        .oneofs
                        .get(oneof)
                        .ok_or_else(|| ParseError::new("boxed oneof descriptor was not found"))?;
                    format!("{full_name}.{oneof}.{}", raw.name)
                } else {
                    format!("{full_name}.{}", raw.name)
                };
                if selectors
                    .iter()
                    .any(|selector| protobuf_path_matches(selector, &path))
                {
                    selected
                        .entry(full_name.clone())
                        .or_default()
                        .insert(field.number);
                }
            }
        }
    }
    Ok(selected)
}

pub(crate) fn select_boxed_fields(
    files: &[Vec<u8>],
    pool: &DescriptorPool,
    selectors: &[String],
) -> Result<Option<Arc<BoxedFieldIndex>>, CodegenError> {
    if selectors.is_empty() {
        return Ok(None);
    }
    let index = collect_boxed_fields(files, pool, selectors).map_err(|_| {
        CodegenError::MalformedDescriptor {
            detail: "failed to resolve boxed field selectors".to_string(),
            path: None,
        }
    })?;
    Ok((!index.is_empty()).then(|| Arc::new(index)))
}

pub(crate) fn bind_boxed_fields(desc: &MessageDescriptor) {
    let selected = BOXED_FIELDS.with(|fields| {
        fields
            .borrow()
            .as_ref()
            .and_then(|index| index.get(&desc.full_name))
            .cloned()
            .unwrap_or_default()
    });
    HAS_BOXED_FIELDS.with(|has| has.set(!selected.is_empty()));
    BOXED_FIELD_NUMBERS.with(|fields| *fields.borrow_mut() = selected);
}

pub(crate) fn is_boxed_field(field: &FieldDescriptor) -> bool {
    HAS_BOXED_FIELDS.with(|has| has.get())
        && BOXED_FIELD_NUMBERS.with(|fields| fields.borrow().contains(&field.number))
}

pub(crate) fn eager_message_field(field: &FieldDescriptor) -> bool {
    OWNED_MESSAGE_FIELDS.with(|owned| owned.get()) || is_boxed_field(field)
}
