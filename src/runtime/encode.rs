#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::error::SerializeError;
use crate::internal::{Private, SealedInternal};
use crate::message::Serialize;
use crate::wire::{
    WIRE_EGROUP, WIRE_I32, WIRE_I64, WIRE_SGROUP, WIRE_VARINT, encode_len_field, encode_tag,
    encode_varint,
};

use super::{FieldKind, FieldType, MiniField, MiniTablePtr, MsgData, UpbGetMessagePtr};

fn delimited_len(number: u32, payload: u64) -> u64 {
    crate::wire::tag_len(number, crate::wire::WIRE_LEN) + crate::wire::varint_len(payload) + payload
}

fn packed_item_len(ty: FieldType, slot: FieldKind) -> u64 {
    use crate::wire::{encode_zigzag32, encode_zigzag64, varint_len};
    match ty {
        FieldType::Float | FieldType::Fixed32 | FieldType::SFixed32 => 4,
        FieldType::Double | FieldType::Fixed64 | FieldType::SFixed64 => 8,
        FieldType::SInt32 => varint_len(encode_zigzag32(slot_i32(slot))),
        FieldType::SInt64 => varint_len(encode_zigzag64(slot_i64(slot))),
        FieldType::Int32 | FieldType::Enum => varint_len(slot_i32(slot) as i64 as u64),
        FieldType::UInt32 => varint_len(u64::from(slot_u32(slot))),
        FieldType::Int64 => varint_len(slot_i64(slot) as u64),
        FieldType::UInt64 => varint_len(slot_u64(slot)),
        FieldType::Bool => 1,
        _ => 0,
    }
}

fn map_key_len(ty: FieldType, key: &[u8]) -> u64 {
    use crate::wire::{encode_zigzag32, encode_zigzag64, varint_len};
    let payload = match ty {
        FieldType::String | FieldType::Bytes => return delimited_len(1, key.len() as u64),
        FieldType::Fixed32 | FieldType::SFixed32 => 4,
        FieldType::Fixed64 | FieldType::SFixed64 => 8,
        FieldType::Bool => varint_len(u64::from(key.first().copied().unwrap_or(0))),
        FieldType::SInt32 => varint_len(encode_zigzag32(i32_from_key_bytes(key))),
        FieldType::SInt64 => varint_len(encode_zigzag64(i64_from_key_bytes(key))),
        FieldType::Int64 | FieldType::UInt64 => varint_len(u64_from_key_bytes(key)),
        FieldType::UInt32 => varint_len(u64::from(i32_from_key_bytes(key) as u32)),
        _ => varint_len(i32_from_key_bytes(key) as i64 as u64),
    };
    // Field 1's tag is one byte for every legal wire type.
    1 + payload
}

fn message_len(data: *const MsgData) -> u64 {
    // SAFETY: serialization's caller owns the live message and linked tables.
    let data = unsafe { &*data };
    let Some(table) = (unsafe { data.mt.0.as_ref() }) else {
        return 0;
    };
    let mut length = data.unknown.encoded_len();
    for (index, field) in table.fields.iter().enumerate() {
        if !data.has.get(index).copied().unwrap_or(false) && !field.repeated && !field.is_map {
            continue;
        }
        length += slot_len(
            field,
            data.slots.get(index).copied().unwrap_or(FieldKind::Empty),
        );
    }
    length
}

fn slot_len(field: &MiniField, slot: FieldKind) -> u64 {
    match slot {
        FieldKind::Empty => 0,
        FieldKind::Bytes(raw) if !raw.is_null() => {
            // SAFETY: byte slots belong to the live message's arena.
            delimited_len(field.number, unsafe { (*raw).len() } as u64)
        }
        FieldKind::Msg(raw) if !raw.is_null() => {
            let length = message_len(raw);
            if field.ty == FieldType::Group {
                crate::wire::tag_len(field.number, WIRE_SGROUP)
                    + length
                    + crate::wire::tag_len(field.number, WIRE_EGROUP)
            } else {
                delimited_len(field.number, length)
            }
        }
        FieldKind::Repeated(raw) if !raw.is_null() => {
            // SAFETY: array slots belong to the live message's arena.
            let items = unsafe { (*raw).items.borrow() };
            if field.packed {
                let payload = items
                    .iter()
                    .map(|item| packed_item_len(field.ty, *item))
                    .sum();
                if payload == 0 {
                    0
                } else {
                    delimited_len(field.number, payload)
                }
            } else {
                let element = MiniField {
                    repeated: false,
                    packed: false,
                    ..*field
                };
                items.iter().map(|item| slot_len(&element, *item)).sum()
            }
        }
        FieldKind::Map(raw) if !raw.is_null() => {
            let (key_type, value) = map_fields(field);
            let value = MiniField {
                number: 2,
                repeated: false,
                packed: false,
                is_map: false,
                ..value
            };
            // SAFETY: map slots belong to the live message's arena.
            unsafe { (*raw).entries.borrow() }
                .iter()
                .map(|(key, item)| {
                    delimited_len(
                        field.number,
                        map_key_len(key_type, key) + slot_len(&value, *item),
                    )
                })
                .sum()
        }
        _ => {
            let payload = packed_item_len(field.ty, slot);
            if payload == 0 {
                return 0;
            }
            let wire = match field.ty {
                FieldType::Float | FieldType::Fixed32 | FieldType::SFixed32 => WIRE_I32,
                FieldType::Double | FieldType::Fixed64 | FieldType::SFixed64 => WIRE_I64,
                _ => WIRE_VARINT,
            };
            crate::wire::tag_len(field.number, wire) + payload
        }
    }
}

fn encode_map_key(ty: FieldType, k: &[u8], out: &mut Vec<u8>) {
    match ty {
        FieldType::String | FieldType::Bytes => encode_len_field(out, 1, k),
        FieldType::Bool => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, k.first().copied().unwrap_or(0) as u64);
        }
        FieldType::Fixed32 | FieldType::SFixed32 => {
            encode_tag(out, 1, WIRE_I32);
            let mut b = [0u8; 4];
            let n = k.len().min(4);
            b[..n].copy_from_slice(&k[..n]);
            out.extend_from_slice(&b);
        }
        FieldType::Fixed64 | FieldType::SFixed64 => {
            encode_tag(out, 1, WIRE_I64);
            let mut b = [0u8; 8];
            let n = k.len().min(8);
            b[..n].copy_from_slice(&k[..n]);
            out.extend_from_slice(&b);
        }
        FieldType::SInt32 => {
            encode_tag(out, 1, WIRE_VARINT);
            let v = i32_from_key_bytes(k);
            encode_varint(out, crate::wire::encode_zigzag32(v));
        }
        FieldType::SInt64 => {
            encode_tag(out, 1, WIRE_VARINT);
            let v = i64_from_key_bytes(k);
            encode_varint(out, crate::wire::encode_zigzag64(v));
        }
        FieldType::Int64 | FieldType::UInt64 => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, u64_from_key_bytes(k));
        }
        FieldType::UInt32 => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, u64::from(i32_from_key_bytes(k) as u32));
        }
        _ => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, i32_from_key_bytes(k) as i64 as u64);
        }
    }
}

fn i32_from_key_bytes(k: &[u8]) -> i32 {
    let mut b = [0u8; 4];
    let n = k.len().min(4);
    b[..n].copy_from_slice(&k[..n]);
    i32::from_le_bytes(b)
}

fn i64_from_key_bytes(k: &[u8]) -> i64 {
    let mut b = [0u8; 8];
    let n = k.len().min(8);
    b[..n].copy_from_slice(&k[..n]);
    i64::from_le_bytes(b)
}

fn u64_from_key_bytes(k: &[u8]) -> u64 {
    i64_from_key_bytes(k) as u64
}

fn encode_msg(data: *const MsgData, out: &mut Vec<u8>) {
    unsafe {
        let d = &*data;
        if d.mt.0.is_null() {
            return;
        }
        let table = &*d.mt.0;
        for (i, f) in table.fields.iter().enumerate() {
            if !d.has.get(i).copied().unwrap_or(false) && !f.repeated && !f.is_map {
                continue;
            }
            let slot = d.slots.get(i).copied().unwrap_or(FieldKind::Empty);
            encode_slot(f, slot, out);
        }
        d.unknown.encode(out);
    }
}

fn slot_u32(slot: FieldKind) -> u32 {
    match slot {
        FieldKind::U32(v) => v,
        FieldKind::I32(v) => v as u32,
        FieldKind::F32(v) => v.to_bits(),
        _ => 0,
    }
}

fn slot_u64(slot: FieldKind) -> u64 {
    match slot {
        FieldKind::U64(v) => v,
        FieldKind::I64(v) => v as u64,
        FieldKind::F64(v) => v.to_bits(),
        FieldKind::U32(v) => v as u64,
        FieldKind::I32(v) => v as u64,
        _ => 0,
    }
}

fn slot_i32(slot: FieldKind) -> i32 {
    match slot {
        FieldKind::I32(v) => v,
        FieldKind::U32(v) => v as i32,
        FieldKind::F32(v) => v.to_bits() as i32,
        _ => 0,
    }
}

fn slot_i64(slot: FieldKind) -> i64 {
    match slot {
        FieldKind::I64(v) => v,
        FieldKind::U64(v) => v as i64,
        FieldKind::I32(v) => v as i64,
        FieldKind::U32(v) => v as i64,
        FieldKind::F64(v) => v.to_bits() as i64,
        _ => 0,
    }
}

fn encode_i32_bits(out: &mut Vec<u8>, number: u32, bits: u32) {
    encode_tag(out, number, WIRE_I32);
    out.extend_from_slice(&bits.to_le_bytes());
}

fn encode_i64_bits(out: &mut Vec<u8>, number: u32, bits: u64) {
    encode_tag(out, number, WIRE_I64);
    out.extend_from_slice(&bits.to_le_bytes());
}

fn encode_packed_item(ty: FieldType, slot: FieldKind, out: &mut Vec<u8>) {
    match ty {
        FieldType::Float | FieldType::Fixed32 | FieldType::SFixed32 => {
            out.extend_from_slice(&slot_u32(slot).to_le_bytes());
        }
        FieldType::Double | FieldType::Fixed64 | FieldType::SFixed64 => {
            out.extend_from_slice(&slot_u64(slot).to_le_bytes());
        }
        FieldType::SInt32 => {
            encode_varint(out, crate::wire::encode_zigzag32(slot_i32(slot)));
        }
        FieldType::SInt64 => {
            encode_varint(out, crate::wire::encode_zigzag64(slot_i64(slot)));
        }
        FieldType::Bool => {
            let v = matches!(slot, FieldKind::Bool(true));
            encode_varint(out, v as u64);
        }
        FieldType::Int32 | FieldType::Enum => {
            encode_varint(out, slot_i32(slot) as i64 as u64);
        }
        FieldType::UInt32 => encode_varint(out, slot_u32(slot) as u64),
        FieldType::Int64 => encode_varint(out, slot_i64(slot) as u64),
        FieldType::UInt64 => encode_varint(out, slot_u64(slot)),
        _ => {}
    }
}

fn encode_scalar_slot(f: &MiniField, slot: FieldKind, out: &mut Vec<u8>) {
    match f.ty {
        FieldType::SInt32 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, crate::wire::encode_zigzag32(slot_i32(slot)));
        }
        FieldType::SInt64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, crate::wire::encode_zigzag64(slot_i64(slot)));
        }
        FieldType::Fixed32 | FieldType::SFixed32 | FieldType::Float => {
            encode_i32_bits(out, f.number, slot_u32(slot));
        }
        FieldType::Fixed64 | FieldType::SFixed64 | FieldType::Double => {
            encode_i64_bits(out, f.number, slot_u64(slot));
        }
        FieldType::Int32 | FieldType::Enum => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_i32(slot) as i64 as u64);
        }
        FieldType::UInt32 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_u32(slot) as u64);
        }
        FieldType::Int64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_i64(slot) as u64);
        }
        FieldType::UInt64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_u64(slot));
        }
        FieldType::Bool => {
            encode_tag(out, f.number, WIRE_VARINT);
            let v = matches!(slot, FieldKind::Bool(true)) || slot_u32(slot) != 0;
            encode_varint(out, v as u64);
        }
        _ => {}
    }
}

fn map_fields(f: &MiniField) -> (FieldType, MiniField) {
    // SAFETY: linked MiniTables outlive their arena messages.
    unsafe {
        f.sub
            .0
            .as_ref()
            .map(|t| {
                let kf = t
                    .fields
                    .iter()
                    .find(|x| x.number == 1)
                    .map(|x| x.ty)
                    .unwrap_or(FieldType::String);
                let vf = t
                    .fields
                    .iter()
                    .copied()
                    .find(|x| x.number == 2)
                    .unwrap_or(MiniField {
                        number: 2,
                        ty: FieldType::Int32,
                        repeated: false,
                        packed: false,
                        proto3_singular: false,
                        required: false,
                        is_map: false,
                        sub: MiniTablePtr::dangling(),
                        closed_enum: None,
                        oneof_group: 0,
                    });
                (kf, vf)
            })
            .unwrap_or((
                FieldType::String,
                MiniField {
                    number: 2,
                    ty: FieldType::Int32,
                    repeated: false,
                    packed: false,
                    proto3_singular: false,
                    required: false,
                    is_map: false,
                    sub: MiniTablePtr::dangling(),
                    closed_enum: None,
                    oneof_group: 0,
                },
            ))
    }
}

pub(crate) fn encode_slot(f: &MiniField, slot: FieldKind, out: &mut Vec<u8>) {
    match slot {
        FieldKind::Empty => {}
        FieldKind::Bytes(p) if !p.is_null() => unsafe {
            encode_len_field(out, f.number, &*p);
        },
        FieldKind::Msg(p) if !p.is_null() => {
            if f.ty == FieldType::Group {
                encode_tag(out, f.number, WIRE_SGROUP);
                encode_msg(p, out);
                encode_tag(out, f.number, WIRE_EGROUP);
            } else {
                let mut tmp = Vec::new();
                encode_msg(p, &mut tmp);
                encode_len_field(out, f.number, &tmp);
            }
        }
        FieldKind::Repeated(p) if !p.is_null() => unsafe {
            if f.packed {
                let mut payload = Vec::new();
                for item in (*p).items.borrow().iter() {
                    encode_packed_item(f.ty, *item, &mut payload);
                }
                if !payload.is_empty() {
                    encode_len_field(out, f.number, &payload);
                }
            } else {
                for item in (*p).items.borrow().iter() {
                    encode_slot(
                        &MiniField {
                            repeated: false,
                            packed: false,
                            ..*f
                        },
                        *item,
                        out,
                    );
                }
            }
        },
        FieldKind::Map(p) if !p.is_null() => unsafe {
            let (key_ty, val_f) = map_fields(f);
            for (k, v) in (*p).entries.borrow().iter() {
                let mut tmp = Vec::new();
                encode_map_key(key_ty, k, &mut tmp);
                encode_slot(
                    &MiniField {
                        number: 2,
                        repeated: false,
                        packed: false,
                        is_map: false,
                        ..val_f
                    },
                    *v,
                    &mut tmp,
                );
                encode_len_field(out, f.number, &tmp);
            }
        },
        _ => encode_scalar_slot(f, slot, out),
    }
}

impl<T> Serialize for T
where
    Self: SealedInternal + UpbGetMessagePtr,
{
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut out = Vec::new();
        encode_msg(self.get_ptr(Private).raw, &mut out);
        Ok(out)
    }
    fn serialized_len(&self) -> usize {
        usize::try_from(message_len(self.get_ptr(Private).raw)).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "fixtures own exact slot and presence layouts"
)]
mod length_tests {
    use super::*;
    use crate::runtime::{Arena, MiniTable};

    fn field(ty: FieldType) -> MiniField {
        MiniField {
            number: 16384,
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

    fn check(field: MiniField, slot: FieldKind) {
        let mut encoded = Vec::new();
        encode_slot(&field, slot, &mut encoded);
        assert_eq!(
            slot_len(&field, slot),
            encoded.len() as u64,
            "{field:?} {slot:?}"
        );
    }

    #[test]
    fn uint32_map_keys_use_unsigned_varints() {
        let mut encoded = Vec::new();
        encode_map_key(FieldType::UInt32, &u32::MAX.to_le_bytes(), &mut encoded);
        assert_eq!(encoded, [8, 255, 255, 255, 255, 15]);
        assert_eq!(map_key_len(FieldType::UInt32, &u32::MAX.to_le_bytes()), 6);
    }

    #[test]
    fn scalar_and_packed_lengths_cover_varint_boundaries_and_bit_patterns() {
        let arena = Arena::new();
        let types = [
            FieldType::Double,
            FieldType::Float,
            FieldType::Int32,
            FieldType::Int64,
            FieldType::UInt32,
            FieldType::UInt64,
            FieldType::SInt32,
            FieldType::SInt64,
            FieldType::Fixed32,
            FieldType::Fixed64,
            FieldType::SFixed32,
            FieldType::SFixed64,
            FieldType::Bool,
            FieldType::Enum,
        ];
        let values = [
            FieldKind::I32(-1),
            FieldKind::I32(i32::MIN),
            FieldKind::I64(i64::MIN),
            FieldKind::U64(u64::MAX),
            FieldKind::U32(127),
            FieldKind::U32(128),
            FieldKind::F32(f32::NAN),
            FieldKind::F64(f64::NEG_INFINITY),
            FieldKind::Bool(true),
        ];
        let array = arena.alloc_array();
        // SAFETY: the arena owns this live array and no other borrow exists.
        unsafe { (*array).items.borrow_mut().extend(values) };
        for ty in types {
            for value in values {
                check(field(ty), value);
            }
            for packed in [false, true] {
                check(
                    MiniField {
                        repeated: true,
                        packed,
                        ..field(ty)
                    },
                    FieldKind::Repeated(array),
                );
            }
        }
    }

    #[test]
    fn nested_group_map_bytes_and_unknown_lengths_match_wire_output() {
        let arena = Arena::new();
        let table = MiniTable {
            fields: vec![field(FieldType::Int32)],
            is_map: false,
            enforce_utf8: true,
        };
        let child = arena.alloc_msg(MiniTablePtr(&table));
        // SAFETY: the child and its table live for this entire test.
        unsafe {
            let data = &mut *child;
            data.slots[0] = FieldKind::I32(-1);
            data.has[0] = true;
            data.unknown
                .fields
                .push(crate::wire::UnknownField::LengthDelimited {
                    number: 20,
                    value: vec![1; 128],
                });
        }
        for ty in [FieldType::Group, FieldType::Message] {
            check(field(ty), FieldKind::Msg(child));
        }
        let bytes = arena.alloc_bytes(vec![1; 128]);
        check(field(FieldType::Bytes), FieldKind::Bytes(bytes));
        let map = arena.alloc_map();
        // SAFETY: both map values refer to live arena-owned storage.
        unsafe {
            (*map).entries.borrow_mut().extend([
                (vec![0xff; 8], FieldKind::Msg(child)),
                (vec![], FieldKind::Msg(child)),
            ])
        };
        for ty in [
            FieldType::String,
            FieldType::Bool,
            FieldType::SInt32,
            FieldType::SInt64,
            FieldType::Int32,
            FieldType::UInt32,
            FieldType::Int64,
            FieldType::UInt64,
            FieldType::Fixed32,
            FieldType::Fixed64,
        ] {
            let entry = MiniTable {
                fields: vec![
                    MiniField {
                        number: 1,
                        ..field(ty)
                    },
                    MiniField {
                        number: 2,
                        ..field(FieldType::Message)
                    },
                ],
                is_map: true,
                enforce_utf8: true,
            };
            check(
                MiniField {
                    is_map: true,
                    sub: MiniTablePtr(&entry),
                    ..field(FieldType::Message)
                },
                FieldKind::Map(map),
            );
        }
        let mut encoded = Vec::new();
        encode_msg(child, &mut encoded);
        assert_eq!(message_len(child), encoded.len() as u64);
    }
}
