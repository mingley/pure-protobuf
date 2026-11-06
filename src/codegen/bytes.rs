//! Selected eager Bytes-backed field storage.

use super::*;
use crate::dynamic::{Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct BytesFieldIndex {
    fields: BTreeMap<String, BTreeSet<u32>>,
    shared_types: BTreeSet<String>,
}

#[derive(Clone)]
pub(crate) struct BytesAccessors {
    pub(crate) shared: Rc<str>,
    pub(crate) shared_opt: Option<Rc<str>>,
}

fn message_target(field: &FieldDescriptor) -> Option<&str> {
    matches!(field.field_type, FieldType::Message | FieldType::Group).then(|| {
        field.type_name.as_deref().or_else(|| {
            field
                .message
                .as_ref()
                .map(|message| message.full_name.as_str())
        })
    })?
}

pub(crate) fn select_bytes_fields(
    pool: &DescriptorPool,
    selectors: &[String],
) -> Option<Arc<BytesFieldIndex>> {
    if selectors.is_empty() {
        return None;
    }
    let names: Vec<String> = pool.collect_names();
    let mut index = BytesFieldIndex::default();
    let mut parents = BTreeMap::<String, Vec<String>>::new();
    for name in &names {
        let Some(message) = pool.get_message(name) else {
            continue;
        };
        for field in message.fields.values() {
            if let Some(target) = message_target(field) {
                parents
                    .entry(target.trim_start_matches('.').to_owned())
                    .or_default()
                    .push(name.clone());
            }
            let bytes_name = if field.is_map {
                field
                    .message
                    .as_ref()
                    .and_then(|entry| entry.field(2))
                    .filter(|value| value.field_type == FieldType::Bytes)
                    .map(|_| "value")
            } else {
                (field.field_type == FieldType::Bytes).then_some(field.name.as_str())
            };
            let Some(bytes_name) = bytes_name else {
                continue;
            };
            let path = format!(".{}.{bytes_name}", message.full_name);
            if selectors
                .iter()
                .any(|selector| protobuf_path_matches(selector, &path))
            {
                index
                    .fields
                    .entry(name.clone())
                    .or_default()
                    .insert(field.number);
                index.shared_types.insert(name.clone());
            }
        }
    }
    if index.fields.is_empty() {
        return None;
    }
    let mut pending: VecDeque<_> = index.shared_types.iter().cloned().collect();
    while let Some(name) = pending.pop_front() {
        if let Some(ancestors) = parents.get(&name) {
            for parent in ancestors {
                if index.shared_types.insert(parent.clone()) {
                    pending.push_back(parent.clone());
                }
            }
        }
    }
    Some(Arc::new(index))
}

pub(crate) fn bind_bytes_fields(desc: &MessageDescriptor) {
    let (fields, children) = BYTES_FIELDS.with(|index| {
        let index = index.borrow();
        let Some(index) = index.as_ref() else {
            return (BTreeSet::new(), BTreeSet::new());
        };
        let fields = index
            .fields
            .get(&desc.full_name)
            .cloned()
            .unwrap_or_default();
        let children = desc
            .fields
            .values()
            .filter(|field| {
                message_target(field).is_some_and(|target| {
                    index.shared_types.contains(target.trim_start_matches('.'))
                })
            })
            .map(|field| field.number)
            .collect();
        (fields, children)
    });
    HAS_BYTES_FIELDS.with(|has| has.set(!fields.is_empty()));
    BYTES_FIELD_NUMBERS.with(|current| *current.borrow_mut() = fields);
    BYTES_SHARED_CHILDREN.with(|current| *current.borrow_mut() = children);
}

pub(crate) fn is_selected_bytes(field: &FieldDescriptor) -> bool {
    HAS_BYTES_FIELDS.with(|has| has.get())
        && BYTES_FIELD_NUMBERS.with(|fields| fields.borrow().contains(&field.number))
}

pub(crate) fn bytes_child_shares_wire(field: &FieldDescriptor) -> bool {
    BYTES_SHARED_CHILDREN.with(|fields| fields.borrow().contains(&field.number))
}

pub(crate) fn direct_selected_bytes(field: &FieldDescriptor) -> bool {
    field.field_type == FieldType::Bytes
        && field.cardinality != Cardinality::Repeated
        && !is_option(field)
        && is_selected_bytes(field)
}

pub(crate) fn bind_bytes_accessors(desc: &MessageDescriptor) {
    let mut names = BTreeMap::new();
    if HAS_BYTES_FIELDS.with(|has| has.get()) {
        let mut occupied = BTreeSet::from(["merge_from_bytes_shared".to_owned()]);
        for field in desc.fields.values() {
            let id = field_id(field);
            let raw = field_raw(field);
            occupied.extend(field_accessor_names(field, &raw, &id));
        }
        let mut allocate = |preferred: String| {
            let mut result = preferred.clone();
            let mut suffix = 2usize;
            while !occupied.insert(result.clone()) {
                result = format!("{preferred}_{suffix}");
                suffix += 1;
            }
            Rc::<str>::from(result)
        };
        for field in desc
            .fields
            .values()
            .filter(|field| is_selected_bytes(field))
        {
            let id = field_id(field);
            let base = id.trim_start_matches("r#");
            let shared = allocate(format!("{base}_shared"));
            let shared_opt =
                (!field.is_map && field.cardinality != Cardinality::Repeated && is_option(field))
                    .then(|| allocate(format!("{base}_shared_opt")));
            names.insert(field.number, BytesAccessors { shared, shared_opt });
        }
    }
    BYTES_ACCESSORS.with(|current| *current.borrow_mut() = names);
}

pub(crate) fn bytes_accessors(field: &FieldDescriptor) -> BytesAccessors {
    BYTES_ACCESSORS
        .with(|names| names.borrow().get(&field.number).cloned())
        .expect("selected bytes accessor was bound")
}

pub(crate) fn emit_bytes_shared_accessor(
    src: &mut String,
    field: &FieldDescriptor,
    cold: ColdPlacement,
) {
    let runtime = runtime_crate_path();
    let id = field_id(field);
    let names = bytes_accessors(field);
    let shared = names.shared;
    let read = if cold.stored_cold(field) {
        format!("self.cold.as_ref().map(|cold| &cold.{id})")
    } else {
        format!("::core::option::Option::Some(&self.{id})")
    };
    let _ = writeln!(
        src,
        "    /// Retains this selected field's shared buffer without copying its payload."
    );
    if field.is_map {
        let (key, _) = map_kv(field);
        let _ = writeln!(
            src,
            "    pub fn {shared}(&self, key: impl {runtime}::MapQuery<{key}>) -> ::core::option::Option<{runtime}::ProtoBytes> {{ {read}.and_then(|map| map.get_shared(key)) }}"
        );
    } else if field.cardinality == Cardinality::Repeated {
        let _ = writeln!(
            src,
            "    pub fn {shared}(&self, index: usize) -> ::core::option::Option<{runtime}::ProtoBytes> {{ {read}.and_then(|items| items.get(index)).cloned() }}"
        );
    } else {
        let read = if is_option(field) {
            format!("{read}.and_then(|field| field.as_deref())")
        } else {
            read
        };
        let fallback = bytes_default_lit(field);
        let _ = writeln!(
            src,
            "    pub fn {shared}(&self) -> {runtime}::ProtoBytes {{ {read}.cloned().unwrap_or_else(|| {runtime}::ProtoBytes::from({runtime}::rt::Bytes::from_static({fallback}))) }}"
        );
        if let Some(shared_opt) = names.shared_opt {
            let _ = writeln!(
                src,
                "    /// Retains a present buffer, distinguishing absence from an empty payload."
            );
            let _ = writeln!(
                src,
                "    pub fn {shared_opt}(&self) -> ::core::option::Option<{runtime}::ProtoBytes> {{ {read}.cloned() }}"
            );
        }
    }
}

pub(crate) fn emit_selected_bytes_accessors(
    src: &mut String,
    desc: &MessageDescriptor,
    field: &FieldDescriptor,
    cold: ColdPlacement,
) {
    let boxed = runtime_path("::std::boxed::Box");

    let runtime = runtime_crate_path();
    let id = field_id(field);
    let raw = field_raw(field);
    let store = cold.store_mut(field);
    let fallback = bytes_default_lit(field);
    let read = if cold.stored_cold(field) {
        format!("self.cold.as_ref().map(|cold| &cold.{id})")
    } else {
        format!("::core::option::Option::Some(&self.{id})")
    };
    if is_option(field) {
        let read = format!("{read}.and_then(|field| field.as_deref())");
        emit_has_doc(src, field);
        let _ = writeln!(
            src,
            "    pub fn has_{raw}(&self) -> bool {{ {read}.is_some() }}"
        );
        emit_field_getter_doc(src, desc, field);
        let _ = writeln!(
            src,
            "    pub fn {id}(&self) -> &[u8] {{ {read}.map({runtime}::ProtoBytes::as_bytes).unwrap_or({fallback}) }}"
        );
        emit_opt_doc(src, field);
        let _ = writeln!(
            src,
            "    pub fn {id}_opt(&self) -> ::core::option::Option<&[u8]> {{ {read}.map({runtime}::ProtoBytes::as_bytes) }}"
        );
        emit_set_doc(src, desc, field);
        let _ = writeln!(
            src,
            "    pub fn set_{raw}(&mut self, value: impl {runtime}::IntoProxied<{runtime}::ProtoBytes>) {{"
        );
        let _ = writeln!(src, "        self.cached_size.dirty();");
        emit_oneof_clear(src, desc, field, cold);
        let _ = writeln!(
            src,
            "        {store} = ::core::option::Option::Some({boxed}::new(value.into_proxied({runtime}::__internal::Private)));"
        );
        let _ = writeln!(src, "    }}");
        emit_clear_doc(src, field);
        let _ = writeln!(
            src,
            "    pub fn clear_{raw}(&mut self) {{ self.cached_size.dirty(); {store} = ::core::option::Option::None; }}"
        );
    } else {
        emit_field_getter_doc(src, desc, field);
        let _ = writeln!(
            src,
            "    pub fn {id}(&self) -> &[u8] {{ {read}.map({runtime}::ProtoBytes::as_bytes).unwrap_or({fallback}) }}"
        );
        emit_set_doc(src, desc, field);
        let _ = writeln!(
            src,
            "    pub fn set_{raw}(&mut self, value: impl {runtime}::IntoProxied<{runtime}::ProtoBytes>) {{ self.cached_size.dirty(); {store} = value.into_proxied({runtime}::__internal::Private); }}"
        );
    }
    emit_bytes_shared_accessor(src, field, cold);
}
