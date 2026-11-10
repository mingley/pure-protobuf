//! Scratch-only behavior contracts for Query cold-placement experiments.
//! Both the unchanged generator and an isolated candidate run these tests.

use pbrs::{Parse, Serialize};
use pbrs_adoption_corpus::{native, prost_types, query, touch_query_native, touch_query_prost};
use prost::Message as _;

#[derive(Clone, Copy, Debug)]
enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

fn read_varint(wire: &[u8], cursor: &mut usize) -> u64 {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *wire.get(*cursor).expect("complete varint");
        *cursor += 1;
        assert!(shift != 63 || byte <= 1, "varint overflow");
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated varint")
}

fn fields(wire: &[u8]) -> Vec<(u32, Field<'_>)> {
    let mut cursor = 0usize;
    let mut out = Vec::new();
    while cursor < wire.len() {
        let key = read_varint(wire, &mut cursor);
        let number = u32::try_from(key >> 3).expect("field number");
        assert_ne!(number, 0);
        let value = match key & 7 {
            0 => Field::Varint(read_varint(wire, &mut cursor)),
            2 => {
                let len = usize::try_from(read_varint(wire, &mut cursor)).expect("field length");
                let end = cursor.checked_add(len).expect("field length overflow");
                let bytes = wire
                    .get(cursor..end)
                    .expect("complete length-delimited field");
                cursor = end;
                Field::Bytes(bytes)
            }
            other => panic!("unexpected fixture wire type {other}"),
        };
        out.push((number, value));
    }
    out
}

fn scalar_values(wire: &[u8], number: u32) -> Vec<u64> {
    fields(wire)
        .into_iter()
        .filter_map(|(n, value)| match (n == number, value) {
            (true, Field::Varint(v)) => Some(v),
            (true, Field::Bytes(_)) => panic!("expected varint field {number}"),
            _ => None,
        })
        .collect()
}

fn messages(wire: &[u8], number: u32) -> Vec<&[u8]> {
    fields(wire)
        .into_iter()
        .filter_map(|(n, value)| match (n == number, value) {
            (true, Field::Bytes(v)) => Some(v),
            (true, Field::Varint(_)) => panic!("expected message field {number}"),
            _ => None,
        })
        .collect()
}

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let low = u8::try_from(value & 0x7f).expect("seven bits");
        value >>= 7;
        if value == 0 {
            out.push(low);
            return out;
        }
        out.push(low | 0x80);
    }
}

fn scalar(number: u32, value: u64) -> Vec<u8> {
    let mut out = varint(u64::from(number) << 3);
    out.extend(varint(value));
    out
}

fn nested(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = varint((u64::from(number) << 3) | 2);
    out.extend(varint(
        u64::try_from(payload.len()).expect("fixture length"),
    ));
    out.extend_from_slice(payload);
    out
}

// Two target occurrences must merge, including the unknown tags in each.
// Unknown root, child, traversal and target tags have distinct numbers.
fn unknown_fixture() -> Vec<u8> {
    let child = [scalar(1, 2), scalar(101, 23)].concat();
    let predicate = [nested(1, b"merged"), scalar(2, 7)].concat();
    let first_target = [scalar(1, 1), scalar(102, 53)].concat();
    let second_target = [nested(3, &predicate), scalar(103, 59)].concat();
    let traversal = [
        nested(1, b"edge"),
        nested(3, &first_target),
        nested(3, &second_target),
        scalar(104, 61),
    ]
    .concat();
    [
        scalar(1, 1),
        nested(2, &child),
        nested(5, &traversal),
        scalar(100, 17),
    ]
    .concat()
}

fn parse_query(wire: &[u8], shared: bool) -> (native::Query, Option<pbrs::rt::Bytes>) {
    if shared {
        let keeper = pbrs::rt::Bytes::copy_from_slice(wire);
        let message = native::Query::parse_bytes(keeper.clone()).expect("shared parse");
        (message, Some(keeper))
    } else {
        (native::Query::parse(wire).expect("borrowed parse"), None)
    }
}

// Check unknown fields by wire location. Prost is intentionally not the
// oracle here because it discards unknown fields.
fn check_unknowns(wire: &[u8]) {
    assert_eq!(scalar_values(wire, 100), [17], "root unknown");
    let children = messages(wire, 2);
    assert_eq!(children.len(), 1);
    assert_eq!(scalar_values(children[0], 101), [23], "child unknown");
    let traversals = messages(wire, 5);
    assert_eq!(traversals.len(), 1);
    assert_eq!(scalar_values(traversals[0], 104), [61], "traversal unknown");
    let targets = messages(traversals[0], 3);
    assert!(!targets.is_empty());
    // Forwarding may retain duplicate occurrences, while a materialized
    // message may serialize their semantic merge as one occurrence.
    let joined = targets.concat();
    assert_eq!(scalar_values(&joined, 102), [53], "first target unknown");
    assert_eq!(scalar_values(&joined, 103), [59], "second target unknown");
}

fn prost_value(wire: &[u8]) -> prost_types::Query {
    prost_types::Query::decode(wire).expect("prost decode")
}

fn check_known_values(message: &native::Query, expected: &prost_types::Query) {
    assert_eq!(touch_query_native(message), touch_query_prost(expected));
    let encoded = message.serialize().expect("native serialization");
    assert_eq!(&prost_value(&encoded), expected, "known fields");
}

fn target_data(message: &native::Query) -> (i32, usize, Vec<u8>) {
    let traversals = message.traversals();
    let traversal = traversals.get(0).expect("traversal");
    let target = traversal.target_opt().expect("merged target");
    let predicates = target.predicates();
    let predicate = predicates.get(0).expect("predicate");
    (
        i32::from(target.operator()),
        predicates.len(),
        predicate.field().as_bytes().to_vec(),
    )
}

fn mutate_nested(message: &mut native::Query, child_operator: i32, target_operator: i32) {
    {
        let mut children = message.children_mut();
        let mut child = children.get_mut(0).expect("child");
        child.set_operator(child_operator);
    }
    {
        let mut traversals = message.traversals_mut();
        let mut traversal = traversals.get_mut(0).expect("traversal");
        traversal.target_mut().set_operator(target_operator);
    }
}

#[test]
fn default_sparse_and_corpus_match_prost() {
    let empty = native::Query::new().serialize().expect("empty");
    assert!(empty.is_empty());
    for shared in [false, true] {
        let (empty_message, _keeper) = parse_query(&[], shared);
        assert_eq!(empty_message.serialize().expect("empty round trip"), empty);
        let sparse = scalar(1, 2);
        let (sparse_message, _keeper) = parse_query(&sparse, shared);
        check_known_values(&sparse_message, &prost_value(&sparse));
        for (depth, variant) in [(3, 0), (4, 6), (8, 7)] {
            let expected = query(depth, variant);
            let wire = expected.encode_to_vec();
            let (actual, _keeper) = parse_query(&wire, shared);
            check_known_values(&actual, &expected);
        }
    }
}

#[test]
fn duplicate_target_fields_merge_and_keep_nested_unknowns() {
    let wire = unknown_fixture();
    let independent = prost_value(&wire);
    let expected_target = independent
        .traversals
        .first()
        .and_then(|t| t.target.as_ref())
        .expect("prost target");
    assert_eq!(expected_target.operator, 1);
    assert_eq!(expected_target.predicates.len(), 1);
    assert_eq!(expected_target.predicates[0].field, "merged");
    assert_eq!(expected_target.predicates[0].comparison, 7);
    for shared in [false, true] {
        let (actual, _keeper) = parse_query(&wire, shared);
        let (operator, count, field) = target_data(&actual);
        assert_eq!(operator, 1);
        assert_eq!(count, 1);
        assert_eq!(field.as_slice(), b"merged");
        check_known_values(&actual, &independent);
        check_unknowns(&actual.serialize().expect("unknown fields"));
    }
}

#[test]
fn forwarding_keeps_unknowns_before_and_after_lazy_target_read() {
    let wire = unknown_fixture();
    let expected = prost_value(&wire);
    for shared in [false, true] {
        let (actual, keeper) = parse_query(&wire, shared);
        drop(keeper);
        let before = actual.serialize().expect("forward before read");
        check_unknowns(&before);
        assert_eq!(prost_value(&before), expected);
        assert_eq!(target_data(&actual).0, 1);
        let after = actual.serialize().expect("forward after read");
        check_unknowns(&after);
        assert_eq!(prost_value(&after), expected);
    }
}

#[test]
fn unread_and_read_clones_isolate_nested_mutation() {
    let wire = unknown_fixture();
    for shared in [false, true] {
        for read_before_clone in [false, true] {
            let (mut original, _keeper) = parse_query(&wire, shared);
            if read_before_clone {
                assert_eq!(target_data(&original).0, 1);
                assert_eq!(target_data(&original).1, 1);
            }
            let mut clone = original.clone();
            mutate_nested(&mut clone, 3, 2);
            assert_eq!(
                i32::from(original.children().get(0).expect("child").operator()),
                2
            );
            assert_eq!(target_data(&original).0, 1);
            assert_eq!(
                i32::from(clone.children().get(0).expect("clone child").operator()),
                3
            );
            assert_eq!(target_data(&clone).0, 2);
            assert_eq!(target_data(&clone).1, 1);
            let original_wire = original.serialize().expect("original after clone mutation");
            let clone_wire = clone.serialize().expect("clone after mutation");
            check_unknowns(&original_wire);
            check_unknowns(&clone_wire);
            assert_eq!(prost_value(&original_wire), prost_value(&wire));
            let mut expected_clone = prost_value(&wire);
            expected_clone.children[0].operator = 3;
            expected_clone.traversals[0]
                .target
                .as_mut()
                .expect("prost target")
                .operator = 2;
            assert_eq!(prost_value(&clone_wire), expected_clone);

            mutate_nested(&mut original, 1, 3);
            assert_eq!(target_data(&clone).0, 2);
            assert_eq!(
                i32::from(clone.children().get(0).expect("clone child").operator()),
                3
            );
            check_unknowns(&original.serialize().expect("original after own mutation"));
            check_unknowns(&clone.serialize().expect("clone after original mutation"));
        }
    }
}

#[test]
fn cloned_owned_targets_are_independent_without_input_wire() {
    let mut predicate = native::Predicate::new();
    predicate.set_field("owned");
    predicate.set_comparison(7);
    let mut nested = native::Query::new();
    nested.set_operator(1);
    nested.set_predicates([predicate]);
    let mut traversal = native::EdgeTraversal::new();
    traversal.set_label("owned-edge");
    traversal.set_target(nested);
    let mut original = native::Query::new();
    original.set_traversals([traversal]);

    let mut clone = original.clone();
    {
        let mut traversals = clone.traversals_mut();
        let mut traversal = traversals.get_mut(0).expect("clone traversal");
        traversal.target_mut().set_operator(2);
    }
    assert_eq!(target_data(&original).0, 1);
    assert_eq!(target_data(&clone).0, 2);
    assert_eq!(target_data(&clone).2.as_slice(), b"owned");
    {
        let mut traversals = original.traversals_mut();
        let mut traversal = traversals.get_mut(0).expect("original traversal");
        traversal.target_mut().set_operator(3);
    }
    assert_eq!(target_data(&original).0, 3);
    assert_eq!(target_data(&clone).0, 2);
    let original_wire = original.serialize().expect("owned original");
    let clone_wire = clone.serialize().expect("owned clone");
    assert_ne!(original_wire, clone_wire);
    assert_eq!(
        prost_value(&original_wire).traversals[0]
            .target
            .as_ref()
            .expect("original target")
            .operator,
        3
    );
    assert_eq!(
        prost_value(&clone_wire).traversals[0]
            .target
            .as_ref()
            .expect("clone target")
            .operator,
        2
    );
}

#[test]
fn empty_and_singleton_mutation_keep_unknown_and_default_equality() {
    let mut empty = native::Query::new();
    assert_eq!(empty, native::Query::new());
    assert_eq!(empty.children_mut().len(), 0);
    assert_eq!(empty, native::Query::new(), "empty cold storage compares equal");
    assert!(empty.serialize().expect("empty encode").is_empty());
    let mut child = native::Query::new();
    child.set_operator(2);
    empty.set_children([child]);
    let mut clone = empty.clone();
    clone.children_mut().clear();
    assert_eq!(clone, native::Query::new());
    assert_eq!(empty.children().len(), 1);
    assert_eq!(i32::from(empty.children().get(0).expect("child").operator()), 2);

    let unknown_wire = [0x98, 0x06, 0x2a];
    let mut value = native::Query::parse(&unknown_wire).expect("unknown parse");
    value.children_mut().clear();
    assert_eq!(value.serialize().expect("unknown encode"), unknown_wire);
}

#[test]
fn report_query_layout() {
    let (outer, cold) = native::Query::scratch_cold_layout();
    println!("QUERY_INLINE_LAYOUT outer={outer} cold={cold}");
    assert_eq!(outer, core::mem::size_of::<native::Query>());
}
