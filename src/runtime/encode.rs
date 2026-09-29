#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::error::SerializeError;
use crate::internal::{Private, SealedInternal};
use crate::message::Serialize;
use crate::wire::{WIRE_I32, WIRE_I64, WIRE_VARINT, encode_len_field, encode_tag, encode_varint};

use super::{FieldKind, FieldType, MiniField, MiniTablePtr, MsgData, UpbGetMessagePtr};

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

pub(crate) fn encode_slot(f: &MiniField, slot: FieldKind, out: &mut Vec<u8>) {
    match slot {
        FieldKind::Empty => {}
        FieldKind::Bytes(p) if !p.is_null() => unsafe {
            encode_len_field(out, f.number, &*p);
        },
        FieldKind::Msg(p) if !p.is_null() => {
            let mut tmp = Vec::new();
            encode_msg(p, &mut tmp);
            encode_len_field(out, f.number, &tmp);
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
            let (key_ty, val_f) = f
                .sub
                .0
                .as_ref()
                .map(|t| {
                    let kf = t
                        .fields
                        .iter()
                        .find(|x| x.number == 1)
                        .map(|x| x.ty)
                        .unwrap_or(FieldType::String);
                    let vf =
                        t.fields
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
                ));
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
        self.serialize().map(|v| v.len()).unwrap_or(0)
    }
}
