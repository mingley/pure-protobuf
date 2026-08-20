//! Packed repeated scalars: eager-validate, lazy-materialize, memcpy encode.

use crate::error::ParseError;
use crate::lazy::Wire;
use crate::repeated::{Repeated, RepeatedMut, RepeatedView};
use crate::wire::{decode_varint, decode_zigzag32, decode_zigzag64};
use std::marker::PhantomData;
use std::sync::OnceLock;

pub trait PackedCodec: Sized {
    type Elem: Copy + Default + PartialEq;
    /// Fixed-width packing has a unique encoding; varints must be recoded canonical.
    const MEMCPY_SAFE: bool = false;
    fn validate(buf: &[u8]) -> Result<(), ParseError>;
    fn decode(buf: &[u8], out: &mut Vec<Self::Elem>) -> Result<(), ParseError>;
}

macro_rules! varint_codec {
    ($name:ident, $elem:ty, $from:expr) => {
        #[derive(Clone, Copy, Debug)]
        pub enum $name {}
        impl PackedCodec for $name {
            type Elem = $elem;
            fn validate(buf: &[u8]) -> Result<(), ParseError> {
                let mut i = 0;
                while i < buf.len() {
                    decode_varint(buf, &mut i)?;
                }
                Ok(())
            }
            fn decode(buf: &[u8], out: &mut Vec<$elem>) -> Result<(), ParseError> {
                let mut i = 0;
                while i < buf.len() {
                    out.push($from(decode_varint(buf, &mut i)?));
                }
                Ok(())
            }
        }
    };
}

varint_codec!(VarintI32, i32, |v| v as i32);
varint_codec!(VarintU32, u32, |v| v as u32);
varint_codec!(VarintI64, i64, |v| v as i64);
varint_codec!(VarintU64, u64, |v| v);
varint_codec!(ZigZag32, i32, decode_zigzag32);
varint_codec!(ZigZag64, i64, decode_zigzag64);

#[derive(Clone, Copy, Debug)]
pub enum Bools {}
impl PackedCodec for Bools {
    type Elem = bool;
    fn validate(buf: &[u8]) -> Result<(), ParseError> {
        let mut i = 0;
        while i < buf.len() {
            decode_varint(buf, &mut i)?;
        }
        Ok(())
    }
    fn decode(buf: &[u8], out: &mut Vec<bool>) -> Result<(), ParseError> {
        let mut i = 0;
        while i < buf.len() {
            out.push(decode_varint(buf, &mut i)? != 0);
        }
        Ok(())
    }
}

macro_rules! fixed_codec {
    ($name:ident, $elem:ty, $width:expr, $read:expr) => {
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
                for c in buf.chunks_exact($width) {
                    out.push($read(c));
                }
                Ok(())
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

pub struct Packed<C: PackedCodec> {
    wire: Option<Wire>,
    decoded: OnceLock<Vec<C::Elem>>,
    _c: PhantomData<C>,
}

impl<C: PackedCodec> Default for Packed<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: PackedCodec> Packed<C> {
    pub fn new() -> Self {
        Self {
            wire: None,
            decoded: OnceLock::new(),
            _c: PhantomData,
        }
    }

    pub fn from_repeated(r: Repeated<C::Elem>) -> Self {
        let d = OnceLock::new();
        let _ = d.set(r.into_vec());
        Self {
            wire: None,
            decoded: d,
            _c: PhantomData,
        }
    }

    pub fn packed_bytes(&self) -> Option<&[u8]> {
        if !C::MEMCPY_SAFE {
            return None;
        }
        self.wire.as_ref().map(|w| w.as_slice())
    }

    pub fn is_empty(&self) -> bool {
        if let Some(v) = self.decoded.get() {
            v.is_empty()
        } else {
            self.wire.as_ref().is_none_or(|w| w.as_slice().is_empty())
        }
    }

    pub fn append_wire(&mut self, w: Wire) -> Result<(), ParseError> {
        C::validate(w.as_slice())?;
        if w.as_slice().is_empty() {
            return Ok(());
        }
        if self.decoded.get().is_none() && self.wire.is_none() {
            self.wire = Some(w);
            return Ok(());
        }
        let mut items = self.take_vec();
        C::decode(w.as_slice(), &mut items)?;
        self.decoded = OnceLock::new();
        let _ = self.decoded.set(items);
        Ok(())
    }

    pub fn push(&mut self, v: C::Elem) {
        self.force_vec().push(v);
    }

    pub fn clear(&mut self) {
        self.wire = None;
        self.decoded = OnceLock::new();
        let _ = self.decoded.set(Vec::new());
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
        self.decoded.get_or_init(|| self.decode_now())
    }

    fn decode_now(&self) -> Vec<C::Elem> {
        let mut out = Vec::new();
        if let Some(w) = &self.wire {
            let _ = C::decode(w.as_slice(), &mut out);
        }
        out
    }

    fn take_vec(&mut self) -> Vec<C::Elem> {
        if let Some(v) = self.decoded.take() {
            self.wire = None;
            v
        } else if let Some(w) = self.wire.take() {
            let mut v = Vec::new();
            let _ = C::decode(w.as_slice(), &mut v);
            v
        } else {
            Vec::new()
        }
    }

    fn force_vec(&mut self) -> &mut Vec<C::Elem> {
        if self.decoded.get().is_none() {
            let v = self.take_vec();
            let _ = self.decoded.set(v);
        }
        self.wire = None;
        self.decoded.get_mut().expect("packed decoded")
    }
}

impl<C: PackedCodec> Clone for Packed<C> {
    fn clone(&self) -> Self {
        let d = OnceLock::new();
        if let Some(v) = self.decoded.get() {
            let _ = d.set(v.clone());
        }
        Self {
            wire: self.wire.clone(),
            decoded: d,
            _c: PhantomData,
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
    fn append_keeps_wire_until_mut() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 1);
        encode_varint(&mut buf, 2);
        let mut p = PackedI32::new();
        p.append_wire(Wire::from_slice(&buf)).unwrap();
        assert!(p.packed_bytes().is_none());
        assert_eq!(p.as_view().len(), 2);
        p.push(3);
        assert_eq!(p.as_view().len(), 3);
        let mut fx = PackedFx32::new();
        fx.append_wire(Wire::from_slice(&1u32.to_le_bytes()))
            .unwrap();
        assert!(fx.packed_bytes().is_some());
        fx.push(2);
        assert!(fx.packed_bytes().is_none());
    }
}
