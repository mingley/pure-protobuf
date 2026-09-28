#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use std::fmt::Debug;

pub type MiniTableEnumPtr = *const ();

#[derive(Clone, Copy, Debug)]
pub struct MiniTableEnumInitPtr(pub MiniTableEnumPtr);
unsafe impl Send for MiniTableEnumInitPtr {}
unsafe impl Sync for MiniTableEnumInitPtr {}

impl MiniTableEnumInitPtr {
    pub const fn dangling() -> Self {
        Self(std::ptr::null())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MiniTableInitPtr(pub MiniTablePtr);
unsafe impl Send for MiniTableInitPtr {}
unsafe impl Sync for MiniTableInitPtr {}

#[derive(Clone, Copy, Debug)]
pub struct MiniTablePtr(pub *const MiniTable);
unsafe impl Send for MiniTablePtr {}
unsafe impl Sync for MiniTablePtr {}

impl MiniTablePtr {
    pub const fn dangling() -> Self {
        Self(std::ptr::null())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Double,
    Float,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Bool,
    String,
    Bytes,
    Message,
    Group,
    Enum,
}

#[derive(Clone, Copy, Debug)]
pub struct MiniField {
    pub number: u32,
    pub ty: FieldType,
    pub repeated: bool,
    pub packed: bool,
    pub proto3_singular: bool,
    pub required: bool,
    pub is_map: bool,
    pub sub: MiniTablePtr,
    pub oneof_group: u32,
}

#[derive(Debug)]
pub struct MiniTable {
    pub fields: Vec<MiniField>,
    pub is_map: bool,
    pub enforce_utf8: bool,
}

pub mod __unstable {
    pub struct DescriptorInfo {
        pub descriptor: &'static [u8],
        pub deps: &'static [&'static DescriptorInfo],
    }
}

const FROM92: [i8; 95] = [
    0, 1, -1, 2, 3, 4, 5, -1, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, -1, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70,
    71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91,
];

fn from92(ch: u8) -> i8 {
    if !(b' '..=b'~').contains(&ch) {
        return -1;
    }
    FROM92[(ch - b' ') as usize]
}

fn encoded_type(ty: i8) -> FieldType {
    match ty {
        0 => FieldType::Double,
        1 => FieldType::Float,
        2 => FieldType::Fixed32,
        3 => FieldType::Fixed64,
        4 => FieldType::SFixed32,
        5 => FieldType::SFixed64,
        6 => FieldType::Int32,
        7 => FieldType::UInt32,
        8 => FieldType::SInt32,
        9 => FieldType::Int64,
        10 => FieldType::UInt64,
        11 => FieldType::SInt64,
        12 => FieldType::Enum,
        13 => FieldType::Bool,
        14 => FieldType::Bytes,
        15 => FieldType::String,
        16 => FieldType::Group,
        17 => FieldType::Message,
        18 => FieldType::Enum,
        _ => FieldType::Int32,
    }
}

fn decode_base92_varint(s: &[u8], i: &mut usize, first: u8, min: u8, max: u8) -> u32 {
    let mut val = 0u32;
    let mut shift = 0u32;
    let bits = {
        let span = (from92(max) - from92(min)) as u32;
        32 - span.leading_zeros()
    };
    let mut ch = first;
    loop {
        let bits_val = (from92(ch) - from92(min)) as u32;
        val |= bits_val << shift;
        if *i >= s.len() || s[*i] < min || s[*i] > max {
            return val;
        }
        ch = s[*i];
        *i += 1;
        shift += bits;
        if shift >= 32 {
            return val;
        }
    }
}

pub unsafe fn build_mini_table(mini_descriptor: &'static str) -> MiniTablePtr {
    let mt = decode_mini_table(mini_descriptor.as_bytes());
    MiniTablePtr(Box::into_raw(Box::new(mt)))
}

pub unsafe fn build_enum_mini_table(_mini_descriptor: &'static str) -> MiniTableEnumPtr {
    std::ptr::null()
}

pub unsafe fn link_mini_table(
    mini_table: MiniTablePtr,
    submessages: &[MiniTablePtr],
    _subenums: &[MiniTableEnumPtr],
) {
    if mini_table.0.is_null() {
        return;
    }
    let mt = unsafe { &mut *mini_table.0.cast_mut() };
    let mut si = 0usize;
    for f in &mut mt.fields {
        if (f.ty == FieldType::Message || f.ty == FieldType::Group || f.is_map)
            && si < submessages.len()
        {
            f.sub = submessages[si];
            si += 1;
        }
    }
    for f in &mut mt.fields {
        if !f.sub.0.is_null() {
            if let Some(sub) = unsafe { f.sub.0.as_ref() } {
                if sub.is_map {
                    f.is_map = true;
                }
            }
        }
    }
}

fn decode_mini_table(bytes: &[u8]) -> MiniTable {
    if bytes.is_empty() {
        return MiniTable {
            fields: Vec::new(),
            is_map: false,
            enforce_utf8: false,
        };
    }
    let mut i = 0usize;
    let ver = bytes[i];
    i += 1;
    let is_map = ver == b'%';
    let mut fields = Vec::new();
    let mut last_num = 0u32;
    let mut enforce_utf8 = false;
    while i < bytes.len() {
        let ch = bytes[i];
        i += 1;
        if ch <= b'I' {
            last_num += 1;
            let mut tyv = from92(ch);
            let mut repeated = false;
            if tyv >= 20 {
                tyv -= 20;
                repeated = true;
            }
            let ty = encoded_type(tyv);
            let packable = !matches!(
                ty,
                FieldType::String | FieldType::Bytes | FieldType::Message | FieldType::Group
            );
            fields.push(MiniField {
                number: last_num,
                ty,
                repeated,
                packed: repeated && packable,
                proto3_singular: false,
                required: false,
                is_map: false,
                sub: MiniTablePtr::dangling(),
                oneof_group: 0,
            });
        } else if (b'L'..=b'[').contains(&ch) {
            let modv = decode_base92_varint(bytes, &mut i, ch, b'L', b'[');
            if let Some(f) = fields.last_mut() {
                if modv & 1 != 0 {
                    f.packed = !f.packed;
                }
                if modv & 2 != 0 {
                    f.required = true;
                }
                if modv & 4 != 0 {
                    f.proto3_singular = true;
                }
                if modv & 8 != 0 {
                    f.proto3_singular = !f.proto3_singular;
                }
            } else if modv & 1 != 0 {
                enforce_utf8 = true;
            }
        } else if (b'_'..=b'~').contains(&ch) && ch != b'~' {
            let skip = decode_base92_varint(bytes, &mut i, ch, b'_', b'~');
            last_num = last_num.saturating_add(skip).saturating_sub(1);
        } else if ch == b'^' {
            let mut group = 1u32;
            while i < bytes.len() {
                let och = bytes[i];
                i += 1;
                if och == b'~' {
                    group += 1;
                    continue;
                }
                if och == b'|' {
                    continue;
                }
                let num = decode_base92_varint(bytes, &mut i, och, b' ', b'b');
                if let Some(f) = fields.iter_mut().find(|f| f.number == num) {
                    f.oneof_group = group;
                }
            }
            break;
        }
    }
    if is_map {
        for f in &mut fields {
            f.is_map = false;
        }
    }
    MiniTable {
        fields,
        is_map,
        enforce_utf8,
    }
}

impl MiniTable {
    pub(crate) fn field_by_number(&self, n: u32) -> Option<(usize, MiniField)> {
        self.fields
            .iter()
            .copied()
            .enumerate()
            .find(|(_, f)| f.number == n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_mini_table_one_string() {
        let mt = decode_mini_table(b"$M1P");
        assert_eq!(mt.fields.len(), 1);
        assert_eq!(mt.fields[0].number, 1);
        assert_eq!(mt.fields[0].ty, FieldType::String);
    }
}
