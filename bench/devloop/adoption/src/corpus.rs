use crate::{google, native, prost_types};
use pbrs::{AsView, Parse, Serialize};
use prost::Message;
use serde_json::{Value, json};

/// Hashes field values, presence, lengths and every byte without allocating.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn word(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn bytes(&mut self, value: &[u8]) {
        self.word(value.len() as u64);
        for byte in value {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn optional(&mut self, value: Option<u64>) {
        self.word(u64::from(value.is_some()));
        if let Some(value) = value {
            self.word(value);
        }
    }
    fn finish(self) -> u64 {
        self.0
    }
}

include!("sparse_walks.rs");

fn typed(variant: usize) -> prost_types::TypedValue {
    use prost_types::typed_value::Kind;
    prost_types::TypedValue {
        kind: Some(match variant % 5 {
            0 => Kind::Boolean(variant.is_multiple_of(2)),
            1 => Kind::Integer(-(variant as i64)),
            2 => Kind::Real(variant as f64 / 4.0),
            3 => Kind::Text(format!("value-{variant}")),
            _ => Kind::Binary(vec![variant as u8; 7]),
        }),
    }
}

fn predicate(variant: usize) -> prost_types::Predicate {
    prost_types::Predicate {
        field: format!("field-{}", variant % 7),
        comparison: (variant % 4) as u32,
        value: Some(typed(variant)),
    }
}

/// One query in the preregistered depth/variant matrix.
pub fn query(depth: usize, variant: usize) -> prost_types::Query {
    assert!((3..=8).contains(&depth) && variant < 8);
    let seed = (depth - 3) * 8 + variant;
    fn chain(depth: usize, seed: usize) -> prost_types::Query {
        prost_types::Query {
            operator: (1 + seed % 3) as i32,
            children: if depth > 1 {
                vec![chain(depth - 1, seed + 1)]
            } else {
                Vec::new()
            },
            predicates: vec![predicate(seed)],
            ..Default::default()
        }
    }
    let mut q = chain(depth, seed);
    q.predicates.push(prost_types::Predicate {
        field: "path_prefix".to_owned(),
        comparison: 1,
        value: Some(prost_types::TypedValue {
            kind: Some(prost_types::typed_value::Kind::Text(
                "q".repeat(32 + seed * 6),
            )),
        }),
    });
    q.subqueries.push(prost_types::Query {
        operator: 3,
        predicates: vec![predicate(seed + 3)],
        ..Default::default()
    });
    q.traversals.push(prost_types::EdgeTraversal {
        label: format!("edge-{}", seed % 3),
        reverse: seed.is_multiple_of(2),
        target: Some(chain(1, seed + 7)),
    });
    q
}

/// An encoded generated payload at an exact, recorded wire-byte size.
pub fn payload(sequence: usize, bytes: usize) -> prost_types::Payload {
    assert!((256..=512).contains(&bytes));
    let mut payload = prost_types::Payload {
        sequence: sequence as u64,
        label: format!("payload-{sequence:04}"),
        data: vec![sequence as u8; bytes],
        measurements: vec![-(sequence as i64), 0, sequence as i64 + 17],
    };
    let overhead = payload.encoded_len() - payload.data.len();
    payload.data.truncate(bytes - overhead);
    assert_eq!(payload.encoded_len(), bytes);
    payload
}

fn edge(sequence: usize, nested: bool) -> prost_types::Edge {
    prost_types::Edge {
        kind: format!("relation-{}", sequence % 3),
        targets: vec![
            format!("key-{sequence:06}"),
            format!("key-{:06}", sequence + 1),
        ],
        nested: if nested {
            vec![edge(sequence + 3, false)]
        } else {
            Vec::new()
        },
    }
}

fn entity(sequence: usize) -> prost_types::Entity {
    prost_types::Entity {
        key: format!("key-{sequence:06}"),
        revision: sequence as u64 + 100,
        payload: Some(google::protobuf::Any {
            type_url: "type.googleapis.com/adoption.Payload".to_owned(),
            value: payload(sequence, 256 + sequence % 5 * 64).encode_to_vec(),
        }),
        edges: vec![edge(sequence, true), edge(sequence + 1, false)],
        // Retain absent, explicitly present empty and present nonempty strings.
        description: match sequence % 3 {
            0 => None,
            1 => Some(String::new()),
            _ => Some(format!("entity-{sequence}")),
        },
    }
}

/// A record-list fixture including nested edges and generated Any payloads.
pub fn entity_list(records: usize) -> prost_types::EntityList {
    assert!([10, 100, 1000].contains(&records));
    prost_types::EntityList {
        records: (0..records).map(entity).collect(),
    }
}

/// Three map field kinds, including message-valued maps.
pub fn maps(entries: usize) -> prost_types::MapHeavy {
    assert!([8, 64, 512].contains(&entries));
    prost_types::MapHeavy {
        counts: (0..entries)
            .map(|i| (format!("count-{i:04}"), -(i as i64)))
            .collect(),
        values: (0..entries)
            .map(|i| (format!("typed-{i:04}"), typed(i)))
            .collect(),
        entities: (0..8).map(|i| (i as i64 - 4, entity(i))).collect(),
    }
}

/// Read every selected oneof member and retain absent/default distinctions.
pub fn touch_typed_prost(m: &prost_types::TypedValue) -> u64 {
    use prost_types::typed_value::Kind;
    let mut h = Digest::new();
    match &m.kind {
        None => h.word(0),
        Some(Kind::Boolean(v)) => {
            h.word(1);
            h.word(u64::from(*v));
        }
        Some(Kind::Integer(v)) => {
            h.word(2);
            h.word(*v as u64);
        }
        Some(Kind::Real(v)) => {
            h.word(3);
            h.word(v.to_bits());
        }
        Some(Kind::Text(v)) => {
            h.word(4);
            h.bytes(v.as_bytes());
        }
        Some(Kind::Binary(v)) => {
            h.word(5);
            h.bytes(v);
        }
    }
    h.finish()
}

/// Independent generated pbrs oneof walk.
pub fn touch_typed_native(m: &native::TypedValue) -> u64 {
    let mut h = Digest::new();
    if let Some(v) = m.boolean_opt() {
        h.word(1);
        h.word(u64::from(v));
    } else if let Some(v) = m.integer_opt() {
        h.word(2);
        h.word(v as u64);
    } else if let Some(v) = m.real_opt() {
        h.word(3);
        h.word(v.to_bits());
    } else if let Some(v) = m.text_opt() {
        h.word(4);
        h.bytes(v.as_bytes());
    } else if let Some(v) = m.binary_opt() {
        h.word(5);
        h.bytes(v);
    } else {
        h.word(0);
    }
    h.finish()
}

fn touch_predicate_prost(m: &prost_types::Predicate) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.field.as_bytes());
    h.word(u64::from(m.comparison));
    h.optional(m.value.as_ref().map(touch_typed_prost));
    h.finish()
}

fn touch_predicate_native(m: &native::Predicate) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.field().as_bytes());
    h.word(u64::from(m.comparison()));
    h.optional(m.value_opt().map(touch_typed_native));
    h.finish()
}

/// Read every field and node of the query, subqueries and traversals.
pub fn touch_query_prost(m: &prost_types::Query) -> u64 {
    let mut h = Digest::new();
    h.word(m.operator as u64);
    h.word(m.children.len() as u64);
    for child in &m.children {
        h.word(touch_query_prost(child));
    }
    h.word(m.predicates.len() as u64);
    for predicate in &m.predicates {
        h.word(touch_predicate_prost(predicate));
    }
    h.word(m.subqueries.len() as u64);
    for query in &m.subqueries {
        h.word(touch_query_prost(query));
    }
    h.word(m.traversals.len() as u64);
    for traversal in &m.traversals {
        h.bytes(traversal.label.as_bytes());
        h.word(u64::from(traversal.reverse));
        h.optional(traversal.target.as_ref().map(touch_query_prost));
    }
    h.finish()
}

/// Independent generated pbrs query walk.
pub fn touch_query_native(m: &native::Query) -> u64 {
    let mut h = Digest::new();
    h.word(i32::from(m.operator()) as u64);
    h.word(m.children().len() as u64);
    for child in m.children().iter() {
        h.word(touch_query_native(&child));
    }
    h.word(m.predicates().len() as u64);
    for predicate in m.predicates().iter() {
        h.word(touch_predicate_native(&predicate));
    }
    h.word(m.subqueries().len() as u64);
    for query in m.subqueries().iter() {
        h.word(touch_query_native(&query));
    }
    h.word(m.traversals().len() as u64);
    for traversal in m.traversals().iter() {
        h.bytes(traversal.label().as_bytes());
        h.word(u64::from(traversal.reverse()));
        h.optional(traversal.target_opt().map(touch_query_native));
    }
    h.finish()
}

pub fn touch_payload_prost(m: &prost_types::Payload) -> u64 {
    let mut h = Digest::new();
    h.word(m.sequence);
    h.bytes(m.label.as_bytes());
    h.bytes(&m.data);
    h.word(m.measurements.len() as u64);
    for value in &m.measurements {
        h.word(*value as u64);
    }
    h.finish()
}

pub fn touch_payload_native(m: &native::Payload) -> u64 {
    let mut h = Digest::new();
    h.word(m.sequence());
    h.bytes(m.label().as_bytes());
    h.bytes(m.data());
    h.word(m.measurements().len() as u64);
    for value in m.measurements().iter() {
        h.word(value as u64);
    }
    h.finish()
}

fn touch_edge_prost(m: &prost_types::Edge) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.kind.as_bytes());
    h.word(m.targets.len() as u64);
    for target in &m.targets {
        h.bytes(target.as_bytes());
    }
    h.word(m.nested.len() as u64);
    for edge in &m.nested {
        h.word(touch_edge_prost(edge));
    }
    h.finish()
}

fn touch_edge_native(m: &native::Edge) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.kind().as_bytes());
    h.word(m.targets().len() as u64);
    for target in m.targets().iter() {
        h.bytes(target.as_view().as_bytes());
    }
    h.word(m.nested().len() as u64);
    for edge in m.nested().iter() {
        h.word(touch_edge_native(&edge));
    }
    h.finish()
}

fn touch_entity_prost(m: &prost_types::Entity) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.key.as_bytes());
    h.word(m.revision);
    h.optional(m.payload.as_ref().map(|any| {
        let mut h = Digest::new();
        h.bytes(any.type_url.as_bytes());
        h.bytes(&any.value);
        h.word(touch_payload_prost(
            &prost_types::Payload::decode(any.value.as_slice()).unwrap(),
        ));
        h.finish()
    }));
    h.word(m.edges.len() as u64);
    for edge in &m.edges {
        h.word(touch_edge_prost(edge));
    }
    h.optional(m.description.as_ref().map(|description| {
        let mut h = Digest::new();
        h.bytes(description.as_bytes());
        h.finish()
    }));
    h.finish()
}

fn touch_entity_native(m: &native::Entity) -> u64 {
    let mut h = Digest::new();
    h.bytes(m.key().as_bytes());
    h.word(m.revision());
    h.optional(m.payload_opt().map(|any| {
        let mut h = Digest::new();
        h.bytes(any.type_url().as_bytes());
        h.bytes(any.value());
        h.word(touch_payload_native(
            &native::Payload::parse(any.value()).unwrap(),
        ));
        h.finish()
    }));
    h.word(m.edges().len() as u64);
    for edge in m.edges().iter() {
        h.word(touch_edge_native(&edge));
    }
    h.optional(m.description_opt().map(|description| {
        let mut h = Digest::new();
        h.bytes(description.as_bytes());
        h.finish()
    }));
    h.finish()
}

pub fn touch_entity_list_prost(m: &prost_types::EntityList) -> u64 {
    let mut h = Digest::new();
    h.word(m.records.len() as u64);
    for record in &m.records {
        h.word(touch_entity_prost(record));
    }
    h.finish()
}

pub fn touch_entity_list_native(m: &native::EntityList) -> u64 {
    let mut h = Digest::new();
    h.word(m.records().len() as u64);
    for record in m.records().iter() {
        h.word(touch_entity_native(&record));
    }
    h.finish()
}

/// Map walks consume every key/value; entry hash sums do not depend on order.
pub fn touch_maps_prost(m: &prost_types::MapHeavy) -> u64 {
    let mut h = Digest::new();
    h.word(m.counts.len() as u64);
    h.word(
        m.counts
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.bytes(key.as_bytes());
                h.word(*value as u64);
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.word(m.values.len() as u64);
    h.word(
        m.values
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.bytes(key.as_bytes());
                h.word(touch_typed_prost(value));
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.word(m.entities.len() as u64);
    h.word(
        m.entities
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.word(*key as u64);
                h.word(touch_entity_prost(value));
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.finish()
}

pub fn touch_maps_native(m: &native::MapHeavy) -> u64 {
    let mut h = Digest::new();
    h.word(m.counts().len() as u64);
    h.word(
        m.counts()
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.bytes(key.as_view().as_bytes());
                h.word(value as u64);
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.word(m.values().len() as u64);
    h.word(
        m.values()
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.bytes(key.as_view().as_bytes());
                h.word(touch_typed_native(&value));
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.word(m.entities().len() as u64);
    h.word(
        m.entities()
            .iter()
            .map(|(key, value)| {
                let mut h = Digest::new();
                h.word(key as u64);
                h.word(touch_entity_native(&value));
                h.finish()
            })
            .fold(0u64, u64::wrapping_add),
    );
    h.finish()
}

/// Full-message equality is proved independently of the read checksum.
pub fn verify_pair<N, P>(
    id: &str,
    specimen: &P,
    read_native: fn(&N) -> u64,
    read_prost: fn(&P) -> u64,
) -> Value
where
    N: Parse + Serialize,
    P: Message + Default + PartialEq + std::fmt::Debug,
{
    let prost_wire = specimen.encode_to_vec();
    let native = N::parse(&prost_wire).unwrap();
    let native_wire = native.serialize().unwrap();
    assert_eq!(
        &P::decode(native_wire.as_slice()).unwrap(),
        specimen,
        "{id}: full semantic equality"
    );
    let checksum = read_prost(specimen);
    assert_eq!(read_native(&native), checksum, "{id}: read-all checksum");
    assert_eq!(
        read_native(&N::parse(&native_wire).unwrap()),
        checksum,
        "{id}: native wire read-all"
    );
    json!({"id":id,"prost_bytes":prost_wire.len(),"pbrs_bytes":native_wire.len(),"read_checksum":checksum,"semantic_equality":"passed","read_all":"passed"})
}

/// Qualify every fixture before exposing it to a timing harness.
pub fn inventory() -> Value {
    let mut cases = Vec::new();
    for depth in 3..=8 {
        for variant in 0..8 {
            let specimen = query(depth, variant);
            assert!(
                (100..=800).contains(&specimen.encoded_len()),
                "query size: depth {depth}, variant {variant}, {} bytes",
                specimen.encoded_len()
            );
            cases.push(verify_pair(
                &format!("query.d{depth}.v{variant}"),
                &specimen,
                touch_query_native,
                touch_query_prost,
            ));
        }
    }
    for count in [10, 100, 1000] {
        cases.push(verify_pair(
            &format!("entities.{count}"),
            &entity_list(count),
            touch_entity_list_native,
            touch_entity_list_prost,
        ));
    }
    for variant in 0..10 {
        let native = sparse_native(variant);
        let specimen = sparse_prost(variant);
        assert_eq!(
            prost_types::Sparse::decode(native.serialize().unwrap().as_slice()).unwrap(),
            specimen
        );
        cases.push(verify_pair(
            &format!("sparse.v{variant}"),
            &specimen,
            touch_sparse_native,
            touch_sparse_prost,
        ));
    }
    for entries in [8, 64, 512] {
        cases.push(verify_pair(
            &format!("maps.{entries}"),
            &maps(entries),
            touch_maps_native,
            touch_maps_prost,
        ));
    }
    json!({"schema":"adoption-corpus/1","tier":"fixture-qualification","case_count":cases.len(),"query_variants":48,"query_depths":[3,4,5,6,7,8],"entity_counts":[10,100,1000],"any_payload_bytes":[256,320,384,448,512],"sparse_field_count":128,"sparse_presence_patterns":10,"map_entries":[8,64,512],"option_files":20,"cases":cases,"qualification_status":"passed","measurement_status":"not_run"})
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("option_tests.rs");

    #[test]
    fn every_sparse_field_preserves_and_reads_present_defaults() {
        check_sparse_field_coverage();
    }

    #[test]
    fn typed_oneofs_read_present_defaults_and_absence() {
        use prost_types::typed_value::Kind;
        let absent = touch_typed_prost(&prost_types::TypedValue::default());
        for kind in [
            Kind::Boolean(false),
            Kind::Integer(0),
            Kind::Real(0.0),
            Kind::Text(String::new()),
            Kind::Binary(Vec::new()),
        ] {
            let m = prost_types::TypedValue { kind: Some(kind) };
            assert_ne!(touch_typed_prost(&m), absent);
            verify_pair(
                "oneof-present-default",
                &m,
                touch_typed_native,
                touch_typed_prost,
            );
        }
        verify_pair(
            "oneof-absent",
            &prost_types::TypedValue::default(),
            touch_typed_native,
            touch_typed_prost,
        );
    }

    #[test]
    fn every_corpus_cross_decodes_and_reads_equally() {
        assert_eq!(inventory()["case_count"], 64);
    }

    fn depth(q: &prost_types::Query) -> usize {
        1 + q
            .children
            .iter()
            .chain(q.subqueries.iter())
            .chain(q.traversals.iter().filter_map(|edge| edge.target.as_ref()))
            .map(depth)
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn recursive_query_matrix_keeps_depth_and_all_typed_variants() {
        let mut variants = [false; 5];
        fn walk(q: &prost_types::Query, variants: &mut [bool; 5]) {
            use prost_types::typed_value::Kind;
            for p in &q.predicates {
                let index = match p.value.as_ref().unwrap().kind.as_ref().unwrap() {
                    Kind::Boolean(_) => 0,
                    Kind::Integer(_) => 1,
                    Kind::Real(_) => 2,
                    Kind::Text(_) => 3,
                    Kind::Binary(_) => 4,
                };
                variants[index] = true;
            }
            for child in &q.children {
                walk(child, variants);
            }
            for child in &q.subqueries {
                walk(child, variants);
            }
            for edge in &q.traversals {
                walk(edge.target.as_ref().unwrap(), variants);
            }
        }
        for requested in 3..=8 {
            for variant in 0..8 {
                let q = query(requested, variant);
                assert_eq!(depth(&q), requested);
                assert!(!q.subqueries.is_empty() && !q.traversals.is_empty());
                walk(&q, &mut variants);
            }
        }
        assert!(variants.into_iter().all(|seen| seen));
    }

    #[test]
    fn any_sizes_and_nested_payload_fields_are_observed() {
        for count in [10, 100, 1000] {
            let mut list = entity_list(count);
            let before = touch_entity_list_prost(&list);
            for (i, record) in list.records.iter().enumerate() {
                let any = record.payload.as_ref().unwrap();
                assert_eq!(any.value.len(), 256 + i % 5 * 64);
                assert!(!record.edges[0].nested.is_empty());
            }
            let any = list.records.last_mut().unwrap().payload.as_mut().unwrap();
            let mut value = prost_types::Payload::decode(any.value.as_slice()).unwrap();
            *value.data.last_mut().unwrap() ^= 1;
            any.value = value.encode_to_vec();
            assert_ne!(touch_entity_list_prost(&list), before);
            verify_pair(
                "mutated-last-record-last-byte",
                &list,
                touch_entity_list_native,
                touch_entity_list_prost,
            );
        }
    }

    #[test]
    fn map_read_walk_is_independent_of_entry_order() {
        let mut message = maps(64);
        let before = touch_maps_prost(&message);
        message.counts = message
            .counts
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        message.values = message
            .values
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        message.entities = message
            .entities
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(touch_maps_prost(&message), before);
        verify_pair(
            "reordered-maps",
            &message,
            touch_maps_native,
            touch_maps_prost,
        );
        *message.counts.get_mut("count-0063").unwrap() += 1;
        assert_ne!(touch_maps_prost(&message), before);
        verify_pair(
            "mutated-map-value",
            &message,
            touch_maps_native,
            touch_maps_prost,
        );
    }
}
