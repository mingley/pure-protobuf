#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::error::ParseError;
use crate::internal::{Private, SealedInternal};
use crate::message::{Clear, ClearAndParse, MergeFrom, Serialize};
use crate::proxied::{AsView, View};
use crate::wire::{
    UnknownField, WIRE_LEN, WIRE_VARINT, decode_tag, decode_varint, decode_zigzag32,
    decode_zigzag64, read_fixed32, read_fixed64, read_len_bytes, skip_field,
};
use std::marker::PhantomData;

use super::{
    Arena, AssociatedMiniTable, FieldKind, FieldType, MessagePtr, MiniField, MiniTablePtr, MsgData,
    StringView, UpbGetArena, UpbGetMessagePtr, UpbGetMessagePtrMut,
};

fn parse_into(
    data: *mut MsgData,
    buf: &[u8],
    arena: &Arena,
    enforce_required: bool,
) -> Result<(), ParseError> {
    let mt = unsafe { (*data).mt };
    if mt.0.is_null() {
        return Ok(());
    }
    let table = unsafe { &*mt.0 };
    let mut pos = 0usize;
    while pos < buf.len() {
        let (num, wire) = decode_tag(buf, &mut pos)?;
        let Some((idx, f)) = table.field_by_number(num) else {
            skip_field(buf, &mut pos, wire)?;
            continue;
        };
        decode_field(data, idx, f, buf, &mut pos, wire, arena)?;
    }
    if enforce_required {
        unsafe {
            let has = &(*data).has;
            for (i, f) in table.fields.iter().enumerate() {
                if f.required && !has.get(i).copied().unwrap_or(false) {
                    return Err(ParseError::new("required"));
                }
            }
        }
    }
    Ok(())
}

fn decode_field(
    data: *mut MsgData,
    idx: usize,
    f: MiniField,
    buf: &[u8],
    pos: &mut usize,
    wire: u32,
    arena: &Arena,
) -> Result<(), ParseError> {
    let ptr = MessagePtr::<()> {
        raw: data,
        _phantom: PhantomData,
    };
    if f.is_map {
        if wire != WIRE_LEN {
            skip_field(buf, pos, wire)?;
            return Ok(());
        }
        let payload = read_len_bytes(buf, pos)?;
        if let Some((k, v)) = decode_map_entry(f.sub, payload, arena)? {
            let map = unsafe { ptr.get_or_create_mutable_map_at_index(idx as u32, arena) }
                .ok_or_else(|| ParseError::new("map alloc"))?;
            unsafe { (*map).entries.borrow_mut().push((k, v)) };
        } else {
            // A rejected closed enum invalidates the entire map entry, even
            // if another value in that entry is valid. Preserve its wire data
            // without replacing an earlier valid entry for the same key.
            ptr.data_mut()
                .unknown
                .fields
                .push(UnknownField::LengthDelimited {
                    number: f.number,
                    value: payload.to_vec(),
                });
        }
        return Ok(());
    }
    if f.repeated && !f.is_map {
        let arr = unsafe { ptr.get_or_create_mutable_array_at_index(idx as u32, arena) }
            .ok_or_else(|| ParseError::new("array alloc"))?;
        if (f.packed || f.ty == FieldType::Enum) && wire == WIRE_LEN {
            let payload = read_len_bytes(buf, pos)?;
            let mut p = 0usize;
            while p < payload.len() {
                let v = if f.ty == FieldType::Enum {
                    let Some(v) = decode_enum(f, payload, &mut p, ptr)? else {
                        continue;
                    };
                    v
                } else {
                    decode_packed_item(f.ty, payload, &mut p)?
                };
                unsafe {
                    (*arr).items.borrow_mut().push(v);
                }
            }
            return Ok(());
        }
        let v = if f.ty == FieldType::Enum {
            if wire != WIRE_VARINT {
                skip_field(buf, pos, wire)?;
                return Ok(());
            }
            let Some(v) = decode_enum(f, buf, pos, ptr)? else {
                return Ok(());
            };
            v
        } else {
            decode_one(f, buf, pos, wire, arena)?
        };
        unsafe {
            (*arr).items.borrow_mut().push(v);
        }
        ptr.set_slot(idx as u32, FieldKind::Repeated(arr), true);
        return Ok(());
    }
    if f.ty == FieldType::Message && !f.repeated {
        let payload = read_len_bytes(buf, pos)?;
        let child = arena.alloc_msg(f.sub);
        parse_into(child, payload, arena, true)?;
        ptr.set_slot(idx as u32, FieldKind::Msg(child), true);
        return Ok(());
    }
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let payload = read_len_bytes(buf, pos)?;
        if f.ty == FieldType::String {
            let enforce = unsafe {
                (*data)
                    .mt
                    .0
                    .as_ref()
                    .map(|t| t.enforce_utf8)
                    .unwrap_or(false)
            };
            if enforce && std::str::from_utf8(payload).is_err() {
                return Err(ParseError::new("utf8"));
            }
        }
        unsafe {
            ptr.set_base_field_string_at_index(idx as u32, StringView::from(payload));
        }
        return Ok(());
    }
    let v = if f.ty == FieldType::Enum {
        if wire != WIRE_VARINT {
            skip_field(buf, pos, wire)?;
            return Ok(());
        }
        let Some(v) = decode_enum(f, buf, pos, ptr)? else {
            return Ok(());
        };
        v
    } else {
        decode_one(f, buf, pos, wire, arena)?
    };
    ptr.set_slot(idx as u32, v, true);
    Ok(())
}

fn decode_enum(
    field: MiniField,
    buf: &[u8],
    pos: &mut usize,
    message: MessagePtr<()>,
) -> Result<Option<FieldKind>, ParseError> {
    let number = decode_varint(buf, pos)?;
    if field.accepts_enum(number as i32) {
        Ok(Some(FieldKind::I32(number as i32)))
    } else {
        // Preserve the original varint, not a truncated/reinterpreted enum.
        message
            .data_mut()
            .unknown
            .fields
            .push(UnknownField::Varint {
                number: field.number,
                value: number,
            });
        Ok(None)
    }
}

fn decode_map_entry(
    sub: MiniTablePtr,
    buf: &[u8],
    arena: &Arena,
) -> Result<Option<(Vec<u8>, FieldKind)>, ParseError> {
    let table = unsafe { sub.0.as_ref() };
    let mut key = Vec::new();
    // Enum-valued maps require zero as the enum's first value. An omitted
    // value therefore still has a valid typed enum value, not an empty slot.
    let mut val = if table.is_some_and(|t| {
        t.fields
            .iter()
            .any(|f| f.number == 2 && f.ty == FieldType::Enum)
    }) {
        FieldKind::I32(0)
    } else {
        FieldKind::Empty
    };
    let mut rejected_enum = false;
    let mut pos = 0usize;
    while pos < buf.len() {
        let (num, wire) = decode_tag(buf, &mut pos)?;
        let field = table.and_then(|t| t.fields.iter().copied().find(|f| f.number == num));
        match num {
            1 => match field.map(|f| f.ty) {
                Some(FieldType::String) | Some(FieldType::Bytes) | None => {
                    if wire == WIRE_LEN {
                        key = read_len_bytes(buf, &mut pos)?.to_vec();
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
                Some(FieldType::Bool) => {
                    if wire == WIRE_VARINT {
                        key = vec![if decode_varint(buf, &mut pos)? != 0 {
                            1
                        } else {
                            0
                        }];
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
                Some(FieldType::Fixed32 | FieldType::SFixed32) => {
                    key = read_fixed32(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(FieldType::Fixed64 | FieldType::SFixed64) => {
                    key = read_fixed64(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(FieldType::SInt32) => {
                    let v = decode_zigzag32(decode_varint(buf, &mut pos)?);
                    key = v.to_le_bytes().to_vec();
                }
                Some(FieldType::SInt64) => {
                    let v = decode_zigzag64(decode_varint(buf, &mut pos)?);
                    key = v.to_le_bytes().to_vec();
                }
                Some(FieldType::Int64 | FieldType::UInt64) => {
                    key = decode_varint(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(_) => {
                    if wire == WIRE_VARINT {
                        key = (decode_varint(buf, &mut pos)? as i32)
                            .to_le_bytes()
                            .to_vec();
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
            },
            2 => {
                if let Some(f) = field {
                    if f.ty == FieldType::Enum && wire != WIRE_VARINT {
                        skip_field(buf, &mut pos, wire)?;
                        rejected_enum = true;
                        continue;
                    }
                    val = decode_one(f, buf, &mut pos, wire, arena)?;
                    if let FieldKind::I32(number) = val {
                        rejected_enum |= !f.accepts_enum(number);
                    }
                } else if wire == WIRE_VARINT {
                    val = FieldKind::I32(decode_varint(buf, &mut pos)? as i32);
                } else {
                    skip_field(buf, &mut pos, wire)?;
                }
            }
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    Ok((!rejected_enum).then_some((key, val)))
}

fn decode_packed_item(ty: FieldType, buf: &[u8], pos: &mut usize) -> Result<FieldKind, ParseError> {
    match ty {
        FieldType::Int32 | FieldType::Enum => Ok(FieldKind::I32(decode_varint(buf, pos)? as i32)),
        FieldType::Int64 => Ok(FieldKind::I64(decode_varint(buf, pos)? as i64)),
        FieldType::UInt32 => Ok(FieldKind::U32(decode_varint(buf, pos)? as u32)),
        FieldType::UInt64 => Ok(FieldKind::U64(decode_varint(buf, pos)?)),
        FieldType::SInt32 => Ok(FieldKind::I32(decode_zigzag32(decode_varint(buf, pos)?))),
        FieldType::SInt64 => Ok(FieldKind::I64(decode_zigzag64(decode_varint(buf, pos)?))),
        FieldType::Bool => Ok(FieldKind::Bool(decode_varint(buf, pos)? != 0)),
        FieldType::Fixed32 | FieldType::SFixed32 | FieldType::Float => {
            Ok(FieldKind::U32(read_fixed32(buf, pos)?))
        }
        FieldType::Fixed64 | FieldType::SFixed64 | FieldType::Double => {
            Ok(FieldKind::U64(read_fixed64(buf, pos)?))
        }
        _ => Err(ParseError::new("bad packed type")),
    }
}

fn decode_one(
    f: MiniField,
    buf: &[u8],
    pos: &mut usize,
    wire: u32,
    arena: &Arena,
) -> Result<FieldKind, ParseError> {
    match f.ty {
        FieldType::Int32 | FieldType::Enum => {
            if wire != WIRE_VARINT {
                skip_field(buf, pos, wire)?;
                return Ok(FieldKind::Empty);
            }
            Ok(FieldKind::I32(decode_varint(buf, pos)? as i32))
        }
        FieldType::Int64 => Ok(FieldKind::I64(decode_varint(buf, pos)? as i64)),
        FieldType::UInt32 => Ok(FieldKind::U32(decode_varint(buf, pos)? as u32)),
        FieldType::UInt64 => Ok(FieldKind::U64(decode_varint(buf, pos)?)),
        FieldType::SInt32 => Ok(FieldKind::I32(decode_zigzag32(decode_varint(buf, pos)?))),
        FieldType::SInt64 => Ok(FieldKind::I64(decode_zigzag64(decode_varint(buf, pos)?))),
        FieldType::Bool => Ok(FieldKind::Bool(decode_varint(buf, pos)? != 0)),
        FieldType::Fixed32 | FieldType::SFixed32 => Ok(FieldKind::U32(read_fixed32(buf, pos)?)),
        FieldType::Fixed64 | FieldType::SFixed64 => Ok(FieldKind::U64(read_fixed64(buf, pos)?)),
        FieldType::Float => Ok(FieldKind::F32(f32::from_bits(read_fixed32(buf, pos)?))),
        FieldType::Double => Ok(FieldKind::F64(f64::from_bits(read_fixed64(buf, pos)?))),
        FieldType::String | FieldType::Bytes => {
            let p = read_len_bytes(buf, pos)?;
            Ok(FieldKind::Bytes(arena.alloc_bytes(p.to_vec())))
        }
        FieldType::Message => {
            if wire != WIRE_LEN {
                skip_field(buf, pos, wire)?;
                return Ok(FieldKind::Empty);
            }
            let payload = read_len_bytes(buf, pos)?;
            let child = arena.alloc_msg(f.sub);
            parse_into(child, payload, arena, false)?;
            Ok(FieldKind::Msg(child))
        }
        _ => {
            skip_field(buf, pos, wire)?;
            Ok(FieldKind::Empty)
        }
    }
}

impl<T> ClearAndParse for T
where
    Self: SealedInternal + UpbGetMessagePtrMut + UpbGetArena,
{
    fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), ParseError> {
        Clear::clear(self);
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            true,
        )
    }
    fn clear_and_parse_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        Clear::clear(self);
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            false,
        )
    }
    fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            true,
        )
    }
    fn merge_from_bytes_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            false,
        )
    }
}

impl<T> MergeFrom for T
where
    Self: SealedInternal + AsView + UpbGetArena + UpbGetMessagePtr,
    Self::Proxied: AssociatedMiniTable,
    for<'a> View<'a, Self::Proxied>: UpbGetMessagePtr,
{
    fn merge_from(&mut self, src: impl AsView<Proxied = Self::Proxied>) {
        if let Ok(bytes) = Serialize::serialize(&src.as_view()) {
            let _ = parse_into(
                self.get_ptr(Private).raw,
                &bytes,
                self.get_arena(Private),
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::encode::encode_slot;
    use crate::runtime::{Arena, FieldType, MiniField, MiniTablePtr};
    use crate::wire::WIRE_LEN;

    fn string_field(repeated: bool) -> MiniField {
        MiniField {
            number: 1,
            ty: FieldType::String,
            repeated,
            packed: false,
            proto3_singular: false,
            required: false,
            is_map: false,
            sub: MiniTablePtr::dangling(),
            closed_enum: None,
            oneof_group: 0,
        }
    }

    #[test]
    fn parsed_string_storage_is_owned_by_the_arena() {
        let arena = Arena::new();
        let mut pos = 0;
        let field = string_field(false);
        let kind = decode_one(field, &[1, b'a'], &mut pos, WIRE_LEN, &arena).expect("decode");
        let mut encoded = Vec::new();
        encode_slot(&field, kind, &mut encoded);
        assert_eq!(encoded, [0x0a, 1, b'a']);
    }

    #[test]
    fn closed_enum_map_unknown_entries_do_not_replace_valid_keys() {
        use crate::runtime::{build_enum_mini_table, build_mini_table, link_mini_table};
        // Real linked mini descriptors: enum {0,1}, map<int32,closed enum>,
        // and a message containing that map at field 1. No generated output
        // is patched to provide metadata that the actual generator omits.
        let enumeration = unsafe { build_enum_mini_table("!$") };
        let entry = unsafe { build_mini_table("%(4") };
        let parent = unsafe { build_mini_table("$G") };
        unsafe {
            link_mini_table(entry, &[], &[enumeration]);
            link_mini_table(parent, &[entry], &[]);
        }
        {
            let arena = Arena::new();
            let data = arena.alloc_msg(parent);
            let known = [0x0a, 4, 8, 5, 16, 1];
            let rejected_same_key = [0x0a, 4, 8, 5, 16, 42];
            let rejected_then_valid = [0x0a, 6, 8, 7, 16, 42, 16, 1];
            let rejected_wrong_wire = [0x0a, 4, 8, 9, 18, 0];
            let missing_value = [0x0a, 2, 8, 8];
            let input = [
                known.as_slice(),
                &rejected_same_key,
                &rejected_then_valid,
                &rejected_wrong_wire,
                &missing_value,
            ]
            .concat();
            parse_into(data, &input, &arena, true).unwrap();
            let FieldKind::Map(raw) = (unsafe { (&(*data).slots)[0] }) else {
                panic!("missing map");
            };
            {
                // SAFETY: this live arena contains validated I32 wire values;
                // primitive views let this test inspect the stored numbers.
                let view = unsafe { crate::MapView::<i32, i32>::from_raw_ptr(raw) };
                assert_eq!(view.len(), 2);
                assert_eq!(view.get(5), Some(1));
                assert_eq!(view.get(7), None);
                assert_eq!(view.get(8), Some(0));
                assert_eq!(view.iter().collect::<Vec<_>>(), [(5, 1), (8, 0)]);
            }
            let mut unknown = Vec::new();
            unsafe { (*data).unknown.encode(&mut unknown) };
            assert_eq!(
                unknown,
                [
                    &rejected_same_key[..],
                    &rejected_then_valid[..],
                    &rejected_wrong_wire[..]
                ]
                .concat()
            );
            let mut encoded = Vec::new();
            // Serialize this single-field message exactly as encode_msg does.
            unsafe { encode_slot(&(&(*parent.0).fields)[0], (&(*data).slots)[0], &mut encoded) };
            encoded.extend_from_slice(&unknown);
            let decoded = arena.alloc_msg(parent);
            parse_into(decoded, &encoded, &arena, true).unwrap();
            let mut roundtrip_unknown = Vec::new();
            unsafe { (*decoded).unknown.encode(&mut roundtrip_unknown) };
            assert_eq!(roundtrip_unknown, unknown);
            let FieldKind::Map(roundtrip) = (unsafe { (&(*decoded).slots)[0] }) else {
                panic!("missing roundtrip map");
            };
            let view = unsafe { crate::MapView::<i32, i32>::from_raw_ptr(roundtrip) };
            assert_eq!(view.iter().collect::<Vec<_>>(), [(5, 1), (8, 0)]);
        }
        // SAFETY: arenas and all views have dropped. Each builder allocated a
        // fresh Box; none was installed in a global/generated OnceLock.
        unsafe {
            drop(Box::from_raw(parent.0.cast_mut()));
            drop(Box::from_raw(entry.0.cast_mut()));
            drop(Box::from_raw(enumeration.cast_mut()));
        }
    }
}
