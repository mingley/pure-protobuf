//! Packed repeated scalars: eager-validate, lazy-materialize, memcpy encode.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixed-width packed try_into after a length check; OnceLock poison is a bug"
)]

use crate::error::ParseError;
use crate::lazy::Wire;
use crate::repeated::{Repeated, RepeatedMut, RepeatedView};
use crate::wire::{
    bulk_decode_varints, decode_varint, decode_zigzag32, decode_zigzag64, encode_varint,
    encode_zigzag32, encode_zigzag64,
};
use std::sync::OnceLock;

pub trait PackedCodec: Sized {
    type Elem: Copy + Default + PartialEq;
    /// Fixed-width packing has a unique encoding; varints must be recoded canonical.
    const MEMCPY_SAFE: bool = false;
    fn validate(buf: &[u8]) -> Result<(), ParseError>;
    fn decode(buf: &[u8], out: &mut Vec<Self::Elem>) -> Result<(), ParseError>;
    fn encode(elems: &[Self::Elem], out: &mut Vec<u8>);
}

/// Byte length of a fixed-width packed run, fail-closed on overflow
/// (PB07-F2: `usize` multiply wraps on 32-bit targets for multi-GiB
/// vectors; truncating the slice would silently corrupt output).
fn packed_byte_len(elems_len: usize, width: usize) -> usize {
    elems_len
        .checked_mul(width)
        .expect("packed encode length overflow")
}

macro_rules! varint_codec {
    ($name:ident, $elem:ty, $from:expr_2021, $to:expr_2021) => {
        #[derive(Clone, Copy, Debug)]
        pub enum $name {}
        impl PackedCodec for $name {
            type Elem = $elem;
            fn validate(buf: &[u8]) -> Result<(), ParseError> {
                crate::wire::validate_varints(buf)
            }
            fn decode(buf: &[u8], out: &mut Vec<$elem>) -> Result<(), ParseError> {
                // SIMD bulk prefix (PK-04) is out-of-line; the shell stays
                // small so callers inline it exactly as before.
                let mut i = 0;
                if buf.len() >= 16 {
                    i = bulk_decode_varints(buf, 0, out, $from)?;
                }
                while i < buf.len() {
                    out.push($from(decode_varint(buf, &mut i)?));
                }
                Ok(())
            }
            fn encode(elems: &[$elem], out: &mut Vec<u8>) {
                for e in elems {
                    encode_varint(out, $to(*e));
                }
            }
        }
    };
}

varint_codec!(VarintI32, i32, |v| v as i32, |e: i32| e as u64);
varint_codec!(VarintU32, u32, |v| v as u32, |e: u32| e as u64);
varint_codec!(VarintI64, i64, |v| v as i64, |e: i64| e as u64);
varint_codec!(VarintU64, u64, |v| v, |e: u64| e);
varint_codec!(ZigZag32, i32, decode_zigzag32, encode_zigzag32);
varint_codec!(ZigZag64, i64, decode_zigzag64, encode_zigzag64);

#[derive(Clone, Copy, Debug)]
pub enum Bools {}
impl PackedCodec for Bools {
    type Elem = bool;
    fn validate(buf: &[u8]) -> Result<(), ParseError> {
        crate::wire::validate_varints(buf)
    }
    fn decode(buf: &[u8], out: &mut Vec<bool>) -> Result<(), ParseError> {
        let mut i = 0;
        if buf.len() >= 16 {
            i = bulk_decode_varints(buf, 0, out, |v| v != 0)?;
        }
        while i < buf.len() {
            out.push(decode_varint(buf, &mut i)? != 0);
        }
        Ok(())
    }
    fn encode(elems: &[bool], out: &mut Vec<u8>) {
        for e in elems {
            encode_varint(out, u64::from(*e));
        }
    }
}

macro_rules! fixed_codec {
    ($name:ident, $elem:ty, $width:expr_2021, $read:expr_2021) => {
        #[derive(Clone, Copy, Debug)]
        pub enum $name {}
        impl PackedCodec for $name {
            type Elem = $elem;
            const MEMCPY_SAFE: bool = true;
            fn validate(buf: &[u8]) -> Result<(), ParseError> {
                if buf.len() % $width != 0 {
                    return Err(ParseError::new("truncated packed fixed"));
                }
                Ok(())
            }
            fn decode(buf: &[u8], out: &mut Vec<$elem>) -> Result<(), ParseError> {
                if buf.len() % $width != 0 {
                    return Err(ParseError::new("truncated packed fixed"));
                }
                let n = buf.len() / $width;
                let start = out.len();
                out.reserve(n);
                #[cfg(target_endian = "little")]
                {
                    // SAFETY: $elem is a fixed-width POD; protobuf wire is LE.
                    unsafe {
                        let dest = out.as_mut_ptr().add(start) as *mut u8;
                        std::ptr::copy_nonoverlapping(buf.as_ptr(), dest, buf.len());
                        out.set_len(start + n);
                    }
                }
                #[cfg(target_endian = "big")]
                {
                    for c in buf.chunks_exact($width) {
                        out.push($read(c));
                    }
                }
                Ok(())
            }
            fn encode(elems: &[$elem], out: &mut Vec<u8>) {
                let byte_len = packed_byte_len(elems.len(), $width);
                crate::copy_counts::note_emit(byte_len);
                #[cfg(target_endian = "little")]
                {
                    // SAFETY: the cast to *const u8 needs only 1-byte
                    // alignment, so any element alignment is fine; `elems`
                    // is a live shared slice, so `byte_len` bytes at that
                    // address are valid and initialized (see §4.2).
                    let bytes = unsafe {
                        std::slice::from_raw_parts(elems.as_ptr() as *const u8, byte_len)
                    };
                    out.extend_from_slice(bytes);
                }
                #[cfg(target_endian = "big")]
                {
                    for e in elems {
                        out.extend_from_slice(&e.to_le_bytes());
                    }
                }
            }
        }
    };
}

fixed_codec!(FixedU32, u32, 4, |c: &[u8]| u32::from_le_bytes(
    c.try_into().unwrap()
));
fixed_codec!(FixedI32, i32, 4, |c: &[u8]| i32::from_le_bytes(
    c.try_into().unwrap()
));
fixed_codec!(FixedU64, u64, 8, |c: &[u8]| u64::from_le_bytes(
    c.try_into().unwrap()
));
fixed_codec!(FixedI64, i64, 8, |c: &[u8]| i64::from_le_bytes(
    c.try_into().unwrap()
));
fixed_codec!(Ieee32, f32, 4, |c: &[u8]| f32::from_bits(
    u32::from_le_bytes(c.try_into().unwrap())
));
fixed_codec!(Ieee64, f64, 8, |c: &[u8]| f64::from_bits(
    u64::from_le_bytes(c.try_into().unwrap())
));

pub type PackedI32 = Packed<VarintI32>;
pub type PackedU32 = Packed<VarintU32>;
pub type PackedI64 = Packed<VarintI64>;
pub type PackedU64 = Packed<VarintU64>;
pub type PackedS32 = Packed<ZigZag32>;
pub type PackedS64 = Packed<ZigZag64>;
pub type PackedFx32 = Packed<FixedU32>;
pub type PackedSfx32 = Packed<FixedI32>;
pub type PackedFx64 = Packed<FixedU64>;
pub type PackedSfx64 = Packed<FixedI64>;
pub type PackedF32 = Packed<Ieee32>;
pub type PackedF64 = Packed<Ieee64>;
pub type PackedBool = Packed<Bools>;

struct PackedInner<C: PackedCodec> {
    wire: Option<Wire>,
    decoded: OnceLock<Vec<C::Elem>>,
    encoded: OnceLock<Vec<u8>>,
}

/// Packed repeated scalars. Empty is a null pointer (8 bytes, zero-valid).
pub struct Packed<C: PackedCodec> {
    inner: Option<Box<PackedInner<C>>>,
}

impl<C: PackedCodec> Default for Packed<C> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<C: PackedCodec> Packed<C> {
    #[inline]
    pub fn new() -> Self {
        Self { inner: None }
    }

    #[inline]
    pub fn validate_bytes(buf: &[u8]) -> Result<(), ParseError> {
        C::validate(buf)
    }

    pub fn from_repeated(r: Repeated<C::Elem>) -> Self {
        let v = r.into_vec();
        if v.is_empty() {
            return Self::new();
        }
        let d = OnceLock::new();
        let _ = d.set(v);
        Self {
            inner: Some(Box::new(PackedInner {
                wire: None,
                decoded: d,
                encoded: OnceLock::new(),
            })),
        }
    }

    pub fn packed_bytes(&self) -> Option<&[u8]> {
        if self.is_empty() {
            return None;
        }
        let i = self.inner.as_ref()?;
        if C::MEMCPY_SAFE {
            if let Some(w) = i.wire.as_ref() {
                return Some(w.as_slice());
            }
        }
        let elems = i.decoded.get_or_init(|| {
            let mut out = Vec::new();
            if let Some(w) = &i.wire {
                let _ = C::decode(w.as_slice(), &mut out);
            }
            out
        });
        Some(i.encoded.get_or_init(|| {
            let mut out = Vec::new();
            C::encode(elems, &mut out);
            out
        }))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        let Some(i) = self.inner.as_ref() else {
            return true;
        };
        if let Some(v) = i.decoded.get() {
            v.is_empty()
        } else {
            i.wire.as_ref().is_none_or(|w| w.as_slice().is_empty())
        }
    }

    pub fn append_bytes(&mut self, buf: &[u8]) -> Result<(), ParseError> {
        C::validate(buf)?;
        if buf.is_empty() {
            return Ok(());
        }
        if C::MEMCPY_SAFE {
            match self.inner.as_mut() {
                None => {
                    let mut v = Vec::new();
                    C::decode(buf, &mut v)?;
                    let d = OnceLock::new();
                    let _ = d.set(v);
                    self.inner = Some(Box::new(PackedInner {
                        wire: None,
                        decoded: d,
                        encoded: OnceLock::new(),
                    }));
                }
                Some(_) => {
                    let mut items = self.take_vec();
                    C::decode(buf, &mut items)?;
                    let d = OnceLock::new();
                    let _ = d.set(items);
                    self.inner = Some(Box::new(PackedInner {
                        wire: None,
                        decoded: d,
                        encoded: OnceLock::new(),
                    }));
                }
            }
            return Ok(());
        }
        self.append_wire(Wire::from_slice(buf))
    }

    pub fn append_wire(&mut self, w: Wire) -> Result<(), ParseError> {
        C::validate(w.as_slice())?;
        if w.as_slice().is_empty() {
            return Ok(());
        }
        match self.inner.as_mut() {
            None => {
                self.inner = Some(Box::new(PackedInner {
                    wire: Some(w),
                    decoded: OnceLock::new(),
                    encoded: OnceLock::new(),
                }));
            }
            Some(i) if i.decoded.get().is_none() && i.wire.is_none() => {
                i.wire = Some(w);
            }
            Some(_) => {
                let mut items = self.take_vec();
                C::decode(w.as_slice(), &mut items)?;
                let d = OnceLock::new();
                let _ = d.set(items);
                self.inner = Some(Box::new(PackedInner {
                    wire: None,
                    decoded: d,
                    encoded: OnceLock::new(),
                }));
            }
        }
        Ok(())
    }

    pub fn push(&mut self, v: C::Elem) {
        self.force_vec().push(v);
    }

    pub fn reserve(&mut self, additional: usize) {
        if additional == 0 {
            return;
        }
        self.force_vec().reserve(additional);
    }

    pub fn clear(&mut self) {
        self.inner = None;
    }

    pub fn as_view(&self) -> RepeatedView<'_, C::Elem> {
        RepeatedView::from_slice(self.as_slice())
    }

    pub fn as_mut(&mut self) -> RepeatedMut<'_, C::Elem> {
        RepeatedMut::from_vec(self.force_vec())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, C::Elem> {
        self.as_slice().iter()
    }

    fn as_slice(&self) -> &[C::Elem] {
        let Some(i) = self.inner.as_ref() else {
            return &[];
        };
        i.decoded.get_or_init(|| {
            let mut out = Vec::new();
            if let Some(w) = &i.wire {
                let _ = C::decode(w.as_slice(), &mut out);
            }
            out
        })
    }

    fn take_vec(&mut self) -> Vec<C::Elem> {
        let Some(i) = self.inner.as_mut() else {
            return Vec::new();
        };
        if let Some(v) = i.decoded.take() {
            i.wire = None;
            v
        } else if let Some(w) = i.wire.take() {
            let mut v = Vec::new();
            let _ = C::decode(w.as_slice(), &mut v);
            v
        } else {
            Vec::new()
        }
    }

    fn force_vec(&mut self) -> &mut Vec<C::Elem> {
        if self.inner.is_none() {
            let d = OnceLock::new();
            let _ = d.set(Vec::new());
            self.inner = Some(Box::new(PackedInner {
                wire: None,
                decoded: d,
                encoded: OnceLock::new(),
            }));
        }
        let i = self.inner.as_mut().expect("packed inner");
        i.encoded = OnceLock::new();
        if i.decoded.get().is_none() {
            let mut v = Vec::new();
            if let Some(w) = i.wire.take() {
                let _ = C::decode(w.as_slice(), &mut v);
            }
            let _ = i.decoded.set(v);
        } else {
            i.wire = None;
        }
        i.decoded.get_mut().expect("packed decoded")
    }
}

impl<C: PackedCodec> Clone for Packed<C> {
    fn clone(&self) -> Self {
        let Some(i) = self.inner.as_ref() else {
            return Self::new();
        };
        let d = OnceLock::new();
        if let Some(v) = i.decoded.get() {
            let _ = d.set(v.clone());
        }
        Self {
            inner: Some(Box::new(PackedInner {
                wire: i.wire.clone(),
                decoded: d,
                encoded: OnceLock::new(),
            })),
        }
    }
}

impl<C: PackedCodec> PartialEq for Packed<C> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<C: PackedCodec> std::fmt::Debug for Packed<C>
where
    C::Elem: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_slice(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::encode_varint;

    #[test]
    fn truncated_varint_fails() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 300);
        buf.pop();
        let last = buf.len() - 1;
        buf[last] |= 0x80;
        assert!(VarintI32::validate(&buf).is_err());
    }

    #[test]
    fn truncated_fixed_fails() {
        assert!(FixedU32::validate(&[1, 2, 3]).is_err());
        assert!(FixedU32::validate(&[1, 2, 3, 4]).is_ok());
    }

    #[test]
    fn append_bytes_fixed_decodes() {
        let mut fx = PackedFx32::new();
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        fx.append_bytes(&buf).unwrap();
        assert_eq!(fx.as_view().len(), 2);
        assert_eq!(fx.as_view().get(0).unwrap(), 1);
        assert_eq!(fx.as_view().get(1).unwrap(), 2);
    }

    #[test]
    fn append_keeps_wire_until_mut() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 1);
        encode_varint(&mut buf, 2);
        let mut p = PackedI32::new();
        p.append_wire(Wire::from_slice(&buf)).unwrap();
        assert_eq!(p.packed_bytes(), Some(buf.as_slice()));
        assert_eq!(p.as_view().len(), 2);
        p.push(3);
        assert_eq!(p.as_view().len(), 3);
        let mut fx = PackedFx32::new();
        fx.append_wire(Wire::from_slice(&1u32.to_le_bytes()))
            .unwrap();
        assert!(fx.packed_bytes().is_some());
        fx.push(2);
        assert_eq!(fx.packed_bytes().map(|p| p.len()), Some(8));
    }

    #[test]
    fn empty_eq_and_zeroed() {
        let z: PackedI32 = unsafe { std::mem::zeroed() };
        assert!(z.is_empty());
        assert_eq!(z, PackedI32::new());
        drop(z);
    }

    #[test]
    fn packed_byte_len_passthrough() {
        assert_eq!(super::packed_byte_len(0, 8), 0);
        assert_eq!(super::packed_byte_len(3, 4), 12);
        assert_eq!(
            super::packed_byte_len(usize::MAX / 8, 8),
            usize::MAX / 8 * 8
        );
    }

    #[test]
    #[should_panic(expected = "packed encode length overflow")]
    fn packed_byte_len_overflow_fails_closed() {
        let _ = super::packed_byte_len(usize::MAX / 8 + 1, 8);
    }

    /// Scalar one-varint-at-a-time decode (reference for the SIMD loops).
    fn scalar_decode_u64(buf: &[u8]) -> Result<Vec<u64>, ParseError> {
        let mut i = 0;
        let mut out = Vec::new();
        while i < buf.len() {
            out.push(decode_varint(buf, &mut i)?);
        }
        Ok(out)
    }

    #[test]
    fn simd_bulk_decode_matches_scalar() {
        // Shapes hitting every bulk path: all-singles, all-pairs, mixed,
        // and pair runs split across 16-byte chunk boundaries.
        let mut bufs: Vec<Vec<u8>> = Vec::new();
        bufs.push((0..64u8).collect());
        let mut pairs = Vec::new();
        for i in 0..32u8 {
            pairs.push(0x80 | (i & 0x7f));
            pairs.push(i & 0x7f);
        }
        bufs.push(pairs);
        let mut mixed = Vec::new();
        for i in 0..24u8 {
            mixed.push(i);
            mixed.push(0x80 | i);
            mixed.push(0x01);
        }
        bufs.push(mixed);
        for prefix in [7usize, 15, 17, 31] {
            let mut v = vec![0x11u8; prefix];
            for i in 0..16u8 {
                v.push(0x80 | i);
                v.push(0x02);
            }
            bufs.push(v);
        }
        // Canonical encodings of 0..256 (packed_256 shape) plus an
        // overlong pair and a truncated tail variant.
        let mut p256 = Vec::new();
        for i in 0..256u64 {
            encode_varint(&mut p256, i);
        }
        bufs.push(p256.clone());
        let mut overlong = p256.clone();
        overlong.extend_from_slice(&[0x80, 0x00]);
        bufs.push(overlong);
        let mut trunc = p256;
        trunc.push(0x80);
        bufs.push(trunc);

        for buf in &bufs {
            let expect = scalar_decode_u64(buf);
            let mut i32_out = Vec::new();
            let i32_res = VarintI32::decode(buf, &mut i32_out);
            let mut u64_out = Vec::new();
            let u64_res = VarintU64::decode(buf, &mut u64_out);
            let mut zz_out = Vec::new();
            let zz_res = ZigZag32::decode(buf, &mut zz_out);
            let mut b_out = Vec::new();
            let b_res = Bools::decode(buf, &mut b_out);
            match &expect {
                Ok(values) => {
                    assert!(i32_res.is_ok());
                    assert!(u64_res.is_ok());
                    assert!(zz_res.is_ok());
                    assert!(b_res.is_ok());
                    assert_eq!(u64_out, *values);
                    assert_eq!(
                        i32_out,
                        values.iter().map(|v| *v as i32).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        zz_out,
                        values
                            .iter()
                            .map(|v| decode_zigzag32(*v))
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(b_out, values.iter().map(|v| *v != 0).collect::<Vec<_>>());
                }
                Err(_) => {
                    assert!(i32_res.is_err());
                    assert!(u64_res.is_err());
                    assert!(zz_res.is_err());
                    assert!(b_res.is_err());
                }
            }
        }
    }
}
