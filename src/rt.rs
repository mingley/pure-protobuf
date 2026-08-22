//! Runtime helpers for generated code. Not a Google upb kernel.

use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::error::{ParseError, SerializeError};
pub use crate::lazy::{LazyBytes, LazyMsg, LazyStr, MergeBytes, Wire};
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
    skip_field, tag_len, varint_len, UnknownField, UnknownFields, WireOut, WIRE_EGROUP, WIRE_I32,
    WIRE_I64, WIRE_LEN, WIRE_SGROUP, WIRE_VARINT,
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

/// Explicit `optional bool` / oneof bool. 0 = unset so `mem::zeroed` is None
/// (`Option<bool>` lays out `Some(false)` as all-zero).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct OptBool(u8);

impl OptBool {
    pub const NONE: Self = Self(0);

    #[inline]
    pub fn some(v: bool) -> Self {
        Self(1 + u8::from(v))
    }

    #[inline]
    pub fn get(self) -> Option<bool> {
        match self.0 {
            0 => None,
            1 => Some(false),
            _ => Some(true),
        }
    }

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn is_some(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub fn unwrap_or(self, default: bool) -> bool {
        self.get().unwrap_or(default)
    }

    #[inline]
    pub fn map<T>(self, f: impl FnOnce(bool) -> T) -> Option<T> {
        self.get().map(f)
    }
}

/// `T` is a generated message whose every field is zero-valid.
///
/// # Safety
/// No `Option<bool>` / `Option<LazyStr>` fields (those are `OptBool` / `Option<Box<_>>`).
#[inline(always)]
pub unsafe fn zeroed_message<T>() -> T {
    unsafe { std::mem::zeroed() }
}
