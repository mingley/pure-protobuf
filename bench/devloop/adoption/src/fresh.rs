//! Direct construction through generated native APIs, from the same recipes.

use crate::native;
use pbrs::Serialize;

fn typed(variant: usize) -> native::TypedValue {
    let mut m = native::TypedValue::new();
    match variant % 5 {
        0 => m.set_boolean(variant.is_multiple_of(2)),
        1 => m.set_integer(-(variant as i64)),
        2 => m.set_real(variant as f64 / 4.0),
        3 => m.set_text(format!("value-{variant}")),
        _ => m.set_binary(vec![variant as u8; 7]),
    }
    m
}

fn predicate(variant: usize) -> native::Predicate {
    let mut m = native::Predicate::new();
    m.set_field(format!("field-{}", variant % 7));
    m.set_comparison((variant % 4) as u32);
    m.set_value(typed(variant));
    m
}

pub fn query_native(depth: usize, variant: usize) -> native::Query {
    assert!((3..=8).contains(&depth) && variant < 8);
    let seed = (depth - 3) * 8 + variant;
    fn chain(depth: usize, seed: usize) -> native::Query {
        let mut m = native::Query::new();
        m.set_operator((1 + seed % 3) as i32);
        if depth > 1 {
            m.set_children([chain(depth - 1, seed + 1)]);
        }
        m.set_predicates([predicate(seed)]);
        m
    }
    let mut m = chain(depth, seed);
    let mut prefix = native::Predicate::new();
    prefix.set_field("path_prefix".to_owned());
    prefix.set_comparison(1);
    let mut value = native::TypedValue::new();
    value.set_text("q".repeat(32 + seed * 6));
    prefix.set_value(value);
    m.predicates_mut().push(prefix);
    let mut subquery = native::Query::new();
    subquery.set_operator(3);
    subquery.set_predicates([predicate(seed + 3)]);
    m.set_subqueries([subquery]);
    let mut traversal = native::EdgeTraversal::new();
    traversal.set_label(format!("edge-{}", seed % 3));
    traversal.set_reverse(seed.is_multiple_of(2));
    traversal.set_target(chain(1, seed + 7));
    m.set_traversals([traversal]);
    m
}

pub fn payload_native(sequence: usize, bytes: usize) -> native::Payload {
    assert!((256..=512).contains(&bytes));
    let mut m = native::Payload::new();
    m.set_sequence(sequence as u64);
    m.set_label(format!("payload-{sequence:04}"));
    m.set_measurements([-(sequence as i64), 0, sequence as i64 + 17]);
    let data_len = bytes - m.serialized_len() - 3;
    assert!((128..16384).contains(&data_len));
    m.set_data(vec![sequence as u8; data_len]);
    assert_eq!(m.serialized_len(), bytes);
    m
}

fn edge(sequence: usize, nested: bool) -> native::Edge {
    let mut m = native::Edge::new();
    m.set_kind(format!("relation-{}", sequence % 3));
    m.targets_mut().push(format!("key-{sequence:06}"));
    m.targets_mut().push(format!("key-{:06}", sequence + 1));
    if nested {
        m.set_nested([edge(sequence + 3, false)]);
    }
    m
}

fn entity(sequence: usize) -> native::Entity {
    let mut m = native::Entity::new();
    m.set_key(format!("key-{sequence:06}"));
    m.set_revision(sequence as u64 + 100);
    let mut any = native::Any::new();
    any.set_type_url("type.googleapis.com/adoption.Payload".to_owned());
    any.set_value(
        payload_native(sequence, 256 + sequence % 5 * 64)
            .serialize()
            .unwrap(),
    );
    m.set_payload(any);
    m.set_edges([edge(sequence, true), edge(sequence + 1, false)]);
    match sequence % 3 {
        0 => {}
        1 => m.set_description(String::new()),
        _ => m.set_description(format!("entity-{sequence}")),
    }
    m
}

pub fn entity_list_native(records: usize) -> native::EntityList {
    assert!([10, 100, 1000].contains(&records));
    let mut m = native::EntityList::new();
    m.set_records((0..records).map(entity));
    m
}

pub fn maps_native(entries: usize) -> native::MapHeavy {
    assert!([8, 64, 512].contains(&entries));
    let mut m = native::MapHeavy::new();
    let mut counts = pbrs::Map::new();
    let mut values = pbrs::Map::new();
    for i in 0..entries {
        counts.insert(format!("count-{i:04}"), -(i as i64));
        values.insert(format!("typed-{i:04}"), typed(i));
    }
    m.set_counts(counts);
    m.set_values(values);
    for i in 0..8 {
        m.entities_mut().insert(i as i64 - 4, entity(i));
    }
    m
}
