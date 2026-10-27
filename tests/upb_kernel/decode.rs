//! Retained MiniTable trees obey protobuf merge and initialization rules.
#![allow(
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the fixture owns exact table/slot layouts and asserts wire semantics"
)]

use pbrs::__internal::{Private, SealedInternal};
use pbrs::runtime::{
    Arena, FieldKind, FieldType, MessagePtr, MiniField, MiniTable, MiniTablePtr, MsgData,
    UpbGetArena, UpbGetMessagePtr, UpbGetMessagePtrMut,
};
use pbrs::{ClearAndParse, Parse};
use std::ptr::NonNull;

fn field(number: u32, ty: FieldType) -> MiniField {
    MiniField {
        number,
        ty,
        repeated: false,
        packed: false,
        proto3_singular: false,
        required: false,
        is_map: false,
        sub: MiniTablePtr::dangling(),
        closed_enum: None,
        oneof_group: 0,
    }
}
fn table(fields: Vec<MiniField>, is_map: bool) -> Box<MiniTable> {
    Box::new(MiniTable {
        fields,
        is_map,
        enforce_utf8: true,
    })
}
// Drop order keeps tables alive until root and arena pointers are destroyed.
struct Fixture {
    root: Box<MsgData>,
    arena: Arena,
    #[expect(
        clippy::vec_box,
        reason = "linked MiniTable pointers need stable Box addresses"
    )]
    _tables: Vec<Box<MiniTable>>,
}
impl Default for Fixture {
    fn default() -> Self {
        let child = table(
            vec![
                MiniField {
                    required: true,
                    ..field(1, FieldType::Int32)
                },
                MiniField {
                    required: true,
                    ..field(2, FieldType::Int32)
                },
            ],
            false,
        );
        let child_ptr = MiniTablePtr(child.as_ref());
        let entry = table(
            vec![
                field(1, FieldType::Int32),
                MiniField {
                    sub: child_ptr,
                    ..field(2, FieldType::Message)
                },
            ],
            true,
        );
        let entry_ptr = MiniTablePtr(entry.as_ref());
        let mut parent = table(
            vec![
                MiniField {
                    sub: child_ptr,
                    ..field(1, FieldType::Message)
                },
                MiniField {
                    sub: child_ptr,
                    repeated: true,
                    ..field(2, FieldType::Message)
                },
                MiniField {
                    sub: child_ptr,
                    oneof_group: 1,
                    ..field(3, FieldType::Message)
                },
                MiniField {
                    oneof_group: 1,
                    ..field(4, FieldType::Int32)
                },
                MiniField {
                    sub: entry_ptr,
                    repeated: true,
                    is_map: true,
                    ..field(5, FieldType::Message)
                },
            ],
            false,
        );
        let parent_ptr = MiniTablePtr(parent.as_ref());
        parent.fields.push(MiniField {
            sub: parent_ptr,
            ..field(6, FieldType::Message)
        });
        parent.fields.push(MiniField {
            repeated: true,
            ..field(7, FieldType::Int32)
        });
        parent.fields.push(MiniField {
            repeated: true,
            packed: true,
            ..field(8, FieldType::Int32)
        });
        parent.fields.push(MiniField {
            repeated: true,
            packed: true,
            ..field(9, FieldType::Float)
        });
        parent.fields.push(MiniField {
            repeated: true,
            ..field(10, FieldType::Double)
        });
        Self {
            root: Box::new(MsgData {
                slots: vec![FieldKind::Empty; parent.fields.len()],
                has: vec![false; parent.fields.len()],
                strs: Vec::new(),
                unknown: pbrs::rt::UnknownFields::default(),
                mt: parent_ptr,
            }),
            arena: Arena::new(),
            _tables: vec![child, entry, parent],
        }
    }
}
impl SealedInternal for Fixture {}
// SAFETY: root has a stable Box address and linked tables owned by self.
unsafe impl UpbGetMessagePtr for Fixture {
    type Msg = Self;
    fn get_ptr(&self, _: Private) -> MessagePtr<Self> {
        unsafe { MessagePtr::wrap(NonNull::from(self.root.as_ref())) }
    }
}
// SAFETY: the mutable self borrow is exclusive and root's Box remains live.
unsafe impl UpbGetMessagePtrMut for Fixture {
    type Msg = Self;
    fn get_ptr_mut(&mut self, _: Private) -> MessagePtr<Self> {
        unsafe { MessagePtr::wrap(NonNull::from(self.root.as_mut())) }
    }
}
// SAFETY: self owns root and the arena holding every parsed descendant.
unsafe impl UpbGetArena for Fixture {
    fn get_arena(&mut self, _: Private) -> &Arena {
        &self.arena
    }
}
fn len_field(number: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    pbrs::rt::encode_len_field(&mut bytes, number, body);
    bytes
}
fn child(data: &MsgData, index: usize) -> &MsgData {
    let FieldKind::Msg(raw) = data.slots[index] else {
        panic!("expected child")
    };
    // SAFETY: this fixture's live arena owns every decoded child.
    unsafe { &*raw }
}
fn integer(data: &MsgData, index: usize) -> i32 {
    let FieldKind::I32(value) = data.slots[index] else {
        panic!("expected integer")
    };
    value
}
fn map_value(data: &MsgData) -> &MsgData {
    let FieldKind::Map(raw) = data.slots[4] else {
        panic!("expected map")
    };
    // SAFETY: this fixture's live arena owns the map and its value.
    let entries = unsafe { (*raw).entries.borrow() };
    assert_eq!(entries.len(), 1);
    let FieldKind::Msg(raw) = entries[0].1 else {
        panic!("expected map message")
    };
    unsafe { &*raw }
}
fn entry(parts: &[&[u8]]) -> Vec<u8> {
    let mut value = vec![8, 7];
    for part in parts {
        value.extend(len_field(2, part));
    }
    len_field(5, &value)
}
#[test]
fn singular_fragments_merge_before_required_initialization() {
    let message = Fixture::parse(
        &[
            len_field(1, &[8, 11]),
            len_field(1, &[16, 22]),
            len_field(1, &[]),
        ]
        .concat(),
    )
    .unwrap();
    assert_eq!(integer(child(&message.root, 0), 0), 11);
    assert_eq!(integer(child(&message.root, 0), 1), 22);
}
#[test]
fn merge_calls_and_empty_enforcing_merge_validate_retained_tree() {
    let mut message = Fixture::default();
    message
        .merge_from_bytes_dont_enforce_required(&len_field(1, &[8, 11]))
        .unwrap();
    assert!(message.merge_from_bytes(&[]).is_err());
    message.merge_from_bytes(&len_field(1, &[16, 22])).unwrap();
    assert_eq!(integer(child(&message.root, 0), 0), 11);
    assert_eq!(integer(child(&message.root, 0), 1), 22);
    message.merge_from_bytes(&[]).unwrap();
}

#[test]
fn clearing_parse_modes_keep_required_checks_independent() {
    let complete = len_field(1, &[8, 11, 16, 22]);
    let mut message = Fixture::parse(&complete).unwrap();
    message
        .clear_and_parse_dont_enforce_required(&len_field(1, &[8, 33]))
        .unwrap();
    assert!(!child(&message.root, 0).has[1]);
    assert!(message.clear_and_parse(&len_field(1, &[16, 44])).is_err());
    assert!(!child(&message.root, 0).has[0]);
    message.clear_and_parse(&complete).unwrap();
    assert_eq!(integer(child(&message.root, 0), 0), 11);
    assert_eq!(integer(child(&message.root, 0), 1), 22);
}
#[test]
fn same_case_oneof_messages_merge_and_replaced_cases_disappear() {
    let message =
        Fixture::parse(&[len_field(3, &[8, 11]), len_field(3, &[16, 22])].concat()).unwrap();
    assert_eq!(integer(child(&message.root, 2), 0), 11);
    assert_eq!(integer(child(&message.root, 2), 1), 22);
    Fixture::parse(&[len_field(3, &[8, 11]), vec![32, 9]].concat()).unwrap();
    assert!(
        Fixture::parse(
            &[
                len_field(3, &[8, 11, 16, 22]),
                vec![32, 9],
                len_field(3, &[16, 33])
            ]
            .concat()
        )
        .is_err()
    );
}
#[test]
fn repeated_elements_remain_independent_and_are_initialized() {
    let bytes = [len_field(2, &[8, 11]), len_field(2, &[16, 22])].concat();
    assert!(
        Fixture::parse(&bytes).is_err(),
        "repeated fragments cannot merge into each other"
    );
    let message = Fixture::parse_dont_enforce_required(&bytes).unwrap();
    let FieldKind::Repeated(raw) = message.root.slots[1] else {
        panic!("expected array")
    };
    assert_eq!(unsafe { (*raw).items.borrow().len() }, 2);
    Fixture::parse(
        &[
            len_field(2, &[8, 11, 16, 22]),
            len_field(2, &[8, 33, 16, 44]),
        ]
        .concat(),
    )
    .unwrap();
}
#[test]
fn map_messages_merge_within_an_entry_and_last_entry_wins() {
    let message = Fixture::parse(&entry(&[&[8, 11], &[16, 22]])).unwrap();
    assert_eq!(integer(map_value(&message.root), 0), 11);
    assert_eq!(integer(map_value(&message.root), 1), 22);
    let message =
        Fixture::parse(&[entry(&[&[8, 99]]), entry(&[&[8, 11, 16, 22]])].concat()).unwrap();
    assert_eq!(integer(map_value(&message.root), 0), 11);
    assert!(Fixture::parse(&[entry(&[&[8, 11, 16, 22]]), entry(&[&[16, 33]])].concat()).is_err());
    assert!(
        Fixture::parse(&entry(&[])).is_err(),
        "omitted map value is an uninitialized default"
    );
    Fixture::parse_dont_enforce_required(&entry(&[])).unwrap();
}

#[test]
fn omitted_map_keys_replace_explicit_default_keys_in_both_directions() {
    let omitted_complete = len_field(5, &len_field(2, &[8, 11, 16, 22]));
    let explicit_partial = len_field(5, &[vec![8, 0], len_field(2, &[8, 33])].concat());
    let message =
        Fixture::parse(&[explicit_partial.clone(), omitted_complete.clone()].concat()).unwrap();
    assert_eq!(integer(map_value(&message.root), 0), 11);
    assert!(Fixture::parse(&[omitted_complete.clone(), explicit_partial].concat()).is_err());

    let omitted_partial = len_field(5, &len_field(2, &[8, 33]));
    let explicit_complete = len_field(5, &[vec![8, 0], len_field(2, &[8, 11, 16, 22])].concat());
    Fixture::parse(&[omitted_partial, explicit_complete].concat()).unwrap();
}

fn keyed_entry(key: u32, payload: &[u8]) -> Vec<u8> {
    let mut body = vec![8];
    pbrs::rt::encode_varint(&mut body, u64::from(key));
    body.extend(len_field(2, payload));
    len_field(5, &body)
}

#[test]
fn many_map_keys_and_duplicate_keys_keep_only_the_final_values() {
    const KEYS: u32 = 4096;
    let mut first = Vec::new();
    let mut last = Vec::new();
    for key in 0..KEYS {
        first.extend(keyed_entry(key, &[8, 33]));
        last.extend(keyed_entry(key, &[8, 11, 16, 22]));
    }
    // Both wire fragments enter the same singular child. Root finalization
    // must share map state across the recursive merges.
    let message = Fixture::parse(&[len_field(6, &first), len_field(6, &last)].concat()).unwrap();
    let FieldKind::Map(raw) = child(&message.root, 5).slots[4] else {
        panic!("expected map")
    };
    let entries = unsafe { (*raw).entries.borrow() };
    assert_eq!(entries.len(), KEYS as usize);
    for (_, value) in entries.iter() {
        let FieldKind::Msg(raw) = *value else {
            panic!("expected map message")
        };
        assert_eq!(integer(unsafe { &*raw }, 0), 11);
        assert_eq!(integer(unsafe { &*raw }, 1), 22);
    }

    let mut duplicates = Vec::new();
    for _ in 0..KEYS {
        duplicates.extend(keyed_entry(7, &[8, 33]));
    }
    duplicates.extend(keyed_entry(7, &[8, 11, 16, 22]));
    let message = Fixture::parse(&duplicates).unwrap();
    assert_eq!(integer(map_value(&message.root), 0), 11);
}

#[test]
fn map_normalization_also_finishes_after_wire_errors() {
    let mut bytes = [entry(&[&[8, 33]]), entry(&[&[8, 11, 16, 22]])].concat();
    bytes.push(0);
    let mut message = Fixture::default();
    assert!(
        message
            .merge_from_bytes_dont_enforce_required(&bytes)
            .is_err()
    );
    assert_eq!(integer(map_value(&message.root), 0), 11);
    message.merge_from_bytes(&[]).unwrap();
}
#[test]
fn ancestor_fragments_can_complete_required_descendants() {
    let bytes = [
        len_field(6, &len_field(1, &[8, 11])),
        len_field(6, &len_field(1, &[16, 22])),
    ]
    .concat();
    let message = Fixture::parse(&bytes).unwrap();
    assert_eq!(integer(child(child(&message.root, 5), 0), 0), 11);
    assert_eq!(integer(child(child(&message.root, 5), 0), 1), 22);
}
#[test]
fn wrong_wire_messages_do_not_replace_valid_children() {
    let message = Fixture::parse(&[len_field(1, &[8, 11, 16, 22]), vec![8, 0]].concat()).unwrap();
    assert_eq!(integer(child(&message.root, 0), 0), 11);
    assert_eq!(integer(child(&message.root, 0), 1), 22);
}

#[test]
fn incompatible_scalar_wire_tags_preserve_oneof_case_presence_and_value() {
    for wrong in [vec![34, 0], vec![37, 0, 0, 0, 0], vec![35, 36]] {
        let message =
            Fixture::parse(&[len_field(3, &[8, 11, 16, 22]), wrong.clone()].concat()).unwrap();
        assert!(message.root.has[2]);
        assert!(!message.root.has[3]);
        assert_eq!(integer(child(&message.root, 2), 0), 11);
        assert_eq!(integer(child(&message.root, 2), 1), 22);
        assert!(
            Fixture::parse(&[len_field(3, &[8, 11]), wrong].concat()).is_err(),
            "wrong-wire scalar must not hide missing required child fields"
        );
    }
    assert!(
        Fixture::parse(&[len_field(3, &[8, 11, 16, 22]), vec![35, 44]].concat()).is_err(),
        "malformed skipped group still fails"
    );
}

#[test]
fn repeated_scalars_accept_packed_and_unpacked_independent_of_the_option() {
    let bytes = [
        vec![56, 11],
        len_field(7, &[22, 33]),
        len_field(8, &[44, 55]),
        vec![64, 66],
    ]
    .concat();
    let message = Fixture::parse(&bytes).unwrap();
    for (index, expected) in [(6, vec![11, 22, 33]), (7, vec![44, 55, 66])] {
        let FieldKind::Repeated(raw) = message.root.slots[index] else {
            panic!("expected array")
        };
        let actual = unsafe { (*raw).items.borrow() }
            .iter()
            .map(|value| match value {
                FieldKind::I32(n) => *n,
                _ => panic!("expected repeated integer"),
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    assert!(
        Fixture::parse_dont_enforce_required(&len_field(7, &[0x80])).is_err(),
        "truncated packed varint still fails"
    );
}

#[test]
fn packed_floats_keep_the_same_typed_values_as_unpacked_floats() {
    let bytes = [
        len_field(9, &1.25f32.to_le_bytes()),
        [vec![77], (-2.5f32).to_le_bytes().to_vec()].concat(),
        len_field(10, &3.5f64.to_le_bytes()),
        [vec![81], (-4.75f64).to_le_bytes().to_vec()].concat(),
    ]
    .concat();
    let message = Fixture::parse(&bytes).unwrap();
    let FieldKind::Repeated(raw) = message.root.slots[8] else {
        panic!("expected float array")
    };
    let values = unsafe { (*raw).items.borrow() };
    assert!(
        matches!(values.as_slice(), [FieldKind::F32(a), FieldKind::F32(b)] if *a == 1.25 && *b == -2.5)
    );
    let FieldKind::Repeated(raw) = message.root.slots[9] else {
        panic!("expected double array")
    };
    let values = unsafe { (*raw).items.borrow() };
    assert!(
        matches!(values.as_slice(), [FieldKind::F64(a), FieldKind::F64(b)] if *a == 3.5 && *b == -4.75)
    );
    assert!(Fixture::parse_dont_enforce_required(&len_field(9, &[0, 0, 0])).is_err());
    assert!(Fixture::parse_dont_enforce_required(&len_field(10, &[0; 7])).is_err());
}
#[test]
fn dont_enforce_rejects_bad_wire_and_excessive_depth() {
    for malformed in [vec![10, 1, 8], vec![10, 5], vec![0x1b, 0x24]] {
        assert!(Fixture::parse_dont_enforce_required(&malformed).is_err());
    }
    let mut bytes = Vec::new();
    for _ in 0..pbrs::RECURSION_LIMIT {
        bytes = len_field(6, &bytes);
    }
    Fixture::parse(&bytes).unwrap();
    assert!(Fixture::parse_dont_enforce_required(&len_field(6, &bytes)).is_err());
    Fixture::parse(&[len_field(1, &[8, 11, 16, 22]), len_field(99, &[0x1b, 0x24])].concat())
        .unwrap();
}
