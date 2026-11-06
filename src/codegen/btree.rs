//! Opt-in unique ordered map storage and eager generated ancestor paths.

use super::*;
use crate::dynamic::{Cardinality, DescriptorPool, FieldDescriptor, FieldType, MessageDescriptor};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct TreeFieldIndex {
    fields: BoxedFieldIndex,
    eager_fields: BoxedFieldIndex,
}

fn target(field: &FieldDescriptor) -> Option<&str> {
    if !matches!(field.field_type, FieldType::Message | FieldType::Group) {
        return None;
    }
    field.type_name.as_deref().or_else(|| {
        field
            .message
            .as_ref()
            .map(|message| message.full_name.as_str())
    })
}

pub(crate) fn select_btree_fields(
    pool: &DescriptorPool,
    selectors: &[String],
) -> Option<Arc<TreeFieldIndex>> {
    if selectors.is_empty() {
        return None;
    }
    let names: Vec<String> = pool.collect_names();
    let mut index = TreeFieldIndex::default();
    let mut parents = BTreeMap::<String, Vec<String>>::new();
    let mut affected = BTreeSet::new();
    for name in &names {
        let Some(message) = pool.get_message(name) else {
            continue;
        };
        for field in message.fields.values() {
            if let Some(child) = target(field) {
                parents
                    .entry(child.trim_start_matches('.').to_owned())
                    .or_default()
                    .push(name.clone());
            }
            if !message.is_map_entry
                && field.is_map
                && selectors.iter().any(|selector| {
                    protobuf_path_matches(
                        selector,
                        &format!(".{}.{}", message.full_name, field.name),
                    )
                })
            {
                index
                    .fields
                    .entry(name.clone())
                    .or_default()
                    .insert(field.number);
                affected.insert(name.clone());
            }
        }
    }
    if index.fields.is_empty() {
        return None;
    }
    // Cached originals contain descendants reached through every edge, even
    // when the immediate field is owned, repeated, a group or a map value.
    let mut pending: VecDeque<_> = affected.iter().cloned().collect();
    while let Some(name) = pending.pop_front() {
        if let Some(ancestors) = parents.get(&name) {
            for parent in ancestors {
                if affected.insert(parent.clone()) {
                    pending.push_back(parent.clone());
                }
            }
        }
    }
    for name in names {
        let Some(message) = pool.get_message(&name) else {
            continue;
        };
        for field in message.fields.values() {
            if field.cardinality != Cardinality::Repeated
                && !field.is_map
                && field.field_type == FieldType::Message
                && !field.delimited
                && target(field).is_some_and(|name| affected.contains(name.trim_start_matches('.')))
            {
                index
                    .eager_fields
                    .entry(name.clone())
                    .or_default()
                    .insert(field.number);
            }
        }
    }
    Some(Arc::new(index))
}

pub(crate) fn add_btree_eager_fields(
    boxed: &mut Option<Arc<BoxedFieldIndex>>,
    tree: Option<&TreeFieldIndex>,
) {
    let Some(tree) = tree.filter(|tree| !tree.eager_fields.is_empty()) else {
        return;
    };
    let boxed = Arc::make_mut(boxed.get_or_insert_with(|| Arc::new(BTreeMap::new())));
    for (message, fields) in &tree.eager_fields {
        boxed.entry(message.clone()).or_default().extend(fields);
    }
}

pub(crate) fn bind_btree_fields(desc: &MessageDescriptor) {
    let selected = BTREE_FIELDS.with(|index| {
        index
            .borrow()
            .as_ref()
            .and_then(|index| index.fields.get(&desc.full_name))
            .cloned()
            .unwrap_or_default()
    });
    HAS_BTREE_FIELDS.with(|has| has.set(!selected.is_empty()));
    BTREE_FIELD_NUMBERS.with(|current| *current.borrow_mut() = selected);
}

pub(crate) fn is_btree_map(field: &FieldDescriptor) -> bool {
    HAS_BTREE_FIELDS.with(|has| has.get())
        && BTREE_FIELD_NUMBERS.with(|fields| fields.borrow().contains(&field.number))
}
