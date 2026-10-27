//! Runtime helpers for generated code. Not a Google upb kernel.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub use crate::error::{ParseError, SerializeError};
pub use crate::lazy::{LazyBytes, LazyMsg, LazyStr, MergeBytes, Wire, require_utf8};
pub use crate::packed::{
    Bools, FixedI32, FixedI64, FixedU32, FixedU64, Ieee32, Ieee64, Packed, PackedBool, PackedCodec,
    PackedF32, PackedF64, PackedFx32, PackedFx64, PackedI32, PackedI64, PackedS32, PackedS64,
    PackedSfx32, PackedSfx64, PackedU32, PackedU64, VarintI32, VarintI64, VarintU32, VarintU64,
    ZigZag32, ZigZag64,
};
pub use crate::wire::{
    SHARED_SEND_THRESHOLD, UnknownField, UnknownFields, WIRE_EGROUP, WIRE_I32, WIRE_I64, WIRE_LEN,
    WIRE_SGROUP, WIRE_VARINT, WireOut, capture_unknown, capture_unknown_with_depth, check_size,
    decode_tag, decode_varint, decode_zigzag32, decode_zigzag64, encode_len_field,
    encode_len_field_shared, encode_len_header, encode_tag, encode_varint, encode_zigzag32,
    encode_zigzag64, key_len_value_len, read_fixed32, read_fixed64, read_len_bytes, read_len_span,
    skip_field, skip_field_with_depth, skip_field_with_number_and_depth, tag_len, varint_len,
};
pub use bytes::Bytes;

/// Encoded-size cache (C++ `cached_size_`). Ignored by `PartialEq`.
#[derive(Debug)]
pub struct CachedSize(AtomicU64);

impl CachedSize {
    const DIRTY: u64 = u64::MAX;

    #[inline]
    pub fn get(&self) -> Option<u64> {
        let v = self.0.load(Ordering::Relaxed);
        if v == Self::DIRTY { None } else { Some(v) }
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

/// Compact encoded-size cache for generated messages.
///
/// Sizes below `u32::MAX` are cached exactly. Larger sizes remain uncached;
/// they are still computed and returned as `u64` by the message. This does
/// not impose a message-size limit. Cache state is ignored by `PartialEq`.
#[derive(Debug)]
#[repr(transparent)]
pub struct CachedSize32(AtomicU32);

impl CachedSize32 {
    const DIRTY: u32 = u32::MAX;

    #[inline]
    pub fn get(&self) -> Option<u64> {
        let value = self.0.load(Ordering::Relaxed);
        if value == Self::DIRTY {
            None
        } else {
            Some(u64::from(value))
        }
    }

    #[inline]
    pub fn set(&self, size: u64) {
        let cached = u32::try_from(size).unwrap_or(Self::DIRTY);
        self.0.store(cached, Ordering::Relaxed);
    }

    #[inline]
    pub fn dirty(&self) {
        self.0.store(Self::DIRTY, Ordering::Relaxed);
    }
}

impl Default for CachedSize32 {
    /// Zero remains a valid cached size for an empty message.
    #[inline]
    fn default() -> Self {
        Self(AtomicU32::new(0))
    }
}

impl Clone for CachedSize32 {
    fn clone(&self) -> Self {
        Self(AtomicU32::new(self.0.load(Ordering::Relaxed)))
    }
}

impl PartialEq for CachedSize32 {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for CachedSize32 {}

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

#[cfg(test)]
mod cache_tests {
    use super::{CachedSize, CachedSize32};

    #[test]
    fn compact_cache_keeps_exact_sizes_and_does_not_truncate_large_sizes() {
        let cache = CachedSize32::default();
        for size in [
            0,
            1,
            u64::from(u32::MAX) - 1,
            u64::from(u32::MAX),
            u64::from(u32::MAX) + 1,
            u64::MAX,
        ] {
            cache.set(size);
            let expected = (size < u64::from(u32::MAX)).then_some(size);
            assert_eq!(cache.get(), expected, "size {size}");
            assert_eq!(cache.clone().get(), expected, "cloned size {size}");
        }
        // A large value is a miss, rather than a sticky overflow state.
        cache.set(123);
        assert_eq!(cache.get(), Some(123));
    }

    #[test]
    fn compact_cache_preserves_zero_dirty_clone_and_equality_contracts() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<CachedSize32>();
        let cache = CachedSize32::default();
        assert_eq!(cache.get(), Some(0));
        assert_eq!(std::mem::size_of::<CachedSize32>(), 4);
        cache.set(9);
        let copy = cache.clone();
        cache.dirty();
        assert_eq!(cache.get(), None);
        assert_eq!(cache.clone().get(), None);
        assert_eq!(copy.get(), Some(9));
        assert_eq!(cache, copy, "cache state is not message state");

        // AtomicU32's zero representation is the valid empty-message cache.
        let zero = CachedSize32(std::sync::atomic::AtomicU32::new(0));
        assert_eq!(zero.get(), Some(0));

        // Existing generated code retains the full-width cache's behavior.
        let legacy = CachedSize::default();
        legacy.set(u64::from(u32::MAX) + 1);
        assert_eq!(legacy.get(), Some(u64::from(u32::MAX) + 1));
    }
}
