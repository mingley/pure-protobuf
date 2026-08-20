//! Runtime helpers for generated code. Not a Google upb kernel.

use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::error::{ParseError, SerializeError};
pub use crate::lazy::{LazyBytes, LazyMsg, LazyStr, Wire};
pub use crate::packed::{
    Bools, FixedI32, FixedI64, FixedU32, FixedU64, Ieee32, Ieee64, Packed, PackedBool, PackedCodec,
    PackedF32, PackedF64, PackedFx32, PackedFx64, PackedI32, PackedI64, PackedS32, PackedS64,
    PackedSfx32, PackedSfx64, PackedU32, PackedU64, VarintI32, VarintI64, VarintU32, VarintU64,
    ZigZag32, ZigZag64,
};
pub use crate::wire::{
    capture_unknown, check_size, decode_tag, decode_varint, decode_zigzag32, decode_zigzag64,
    encode_len_field, encode_len_header, encode_tag, encode_varint, encode_zigzag32,
    encode_zigzag64, key_len_value_len, read_fixed32, read_fixed64, read_len_bytes, read_len_span,
    skip_field, tag_len, varint_len, UnknownField, UnknownFields, WIRE_EGROUP, WIRE_I32, WIRE_I64,
    WIRE_LEN, WIRE_SGROUP, WIRE_VARINT,
};

/// Encoded-size cache (C++ `cached_size_`). Ignored by `PartialEq`.
#[derive(Debug)]
pub struct CachedSize(AtomicU64);

impl CachedSize {
    const DIRTY: u64 = u64::MAX;

    #[inline]
    pub fn get(&self) -> Option<u64> {
        let v = self.0.load(Ordering::Relaxed);
        if v == Self::DIRTY {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
    pub fn set(&self, n: u64) {
        self.0.store(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn dirty(&self) {
        self.0.store(Self::DIRTY, Ordering::Relaxed);
    }
}

impl Default for CachedSize {
    /// 0 is a valid cached size for an empty message.
    #[inline]
    fn default() -> Self {
        Self(AtomicU64::new(0))
    }
}

impl Clone for CachedSize {
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.0.load(Ordering::Relaxed)))
    }
}

impl PartialEq for CachedSize {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for CachedSize {}
