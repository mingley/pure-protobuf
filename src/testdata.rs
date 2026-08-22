//! Hand-written typed messages with the v4 accessor shape.

macro_rules! impl_message {
    ($Owned:ident, $View:ident, $Mut:ident) => {
        impl $crate::internal::SealedInternal for $Owned {}
        impl $crate::message::MessageType for $Owned {}
        impl $crate::proxied::Proxied for $Owned {
            type View<'msg> = $View<'msg>;
        }
        impl $crate::proxied::MutProxied for $Owned {
            type Mut<'msg> = $Mut<'msg>;
        }
        impl $crate::proxied::AsView for $Owned {
            type Proxied = Self;
            fn as_view(&self) -> $View<'_> {
                $View(self)
            }
        }
        impl $crate::proxied::AsMut for $Owned {
            type MutProxied = Self;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self)
            }
        }
        impl $crate::internal::SealedInternal for $View<'_> {}
        impl $crate::proxied::AsView for $View<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                *self
            }
        }
        impl<'msg> $crate::proxied::IntoView<'msg> for $View<'msg> {
            fn into_view<'shorter>(self) -> $View<'shorter>
            where
                'msg: 'shorter,
            {
                $View(self.0)
            }
        }
        impl $crate::internal::SealedInternal for $Mut<'_> {}
        impl $crate::proxied::AsView for $Mut<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                $View(self.0)
            }
        }
        impl $crate::proxied::AsMut for $Mut<'_> {
            type MutProxied = $Owned;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self.0)
            }
        }
        impl<'msg> $crate::proxied::IntoView<'msg> for $Mut<'msg> {
            fn into_view<'shorter>(self) -> $View<'shorter>
            where
                'msg: 'shorter,
            {
                $View(self.0)
            }
        }
        impl<'msg> $crate::proxied::IntoMut<'msg> for $Mut<'msg> {
            fn into_mut<'shorter>(self) -> $Mut<'shorter>
            where
                'msg: 'shorter,
            {
                $Mut(self.0)
            }
        }
        impl $crate::message::Serialize for $Owned {
            #[inline]
            fn serialize(&self) -> Result<Vec<u8>, $crate::error::SerializeError> {
                let mut out = Vec::with_capacity(self.compute_size() as usize);
                self.write_to(&mut out);
                $crate::wire::check_size(out.len() as u64)?;
                Ok(out)
            }
            fn serialized_len(&self) -> usize {
                self.compute_size() as usize
            }
            fn encode(
                &self,
                out: &mut impl $crate::wire::WireOut,
            ) -> Result<(), $crate::error::SerializeError> {
                $crate::wire::check_size(self.compute_size())?;
                self.write_to(out);
                Ok(())
            }
        }
        impl $crate::message::Serialize for $View<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::error::SerializeError> {
                self.0.serialize()
            }
            fn serialized_len(&self) -> usize {
                self.0.serialized_len()
            }
            fn encode(
                &self,
                out: &mut impl $crate::wire::WireOut,
            ) -> Result<(), $crate::error::SerializeError> {
                self.0.encode(out)
            }
        }
        impl $crate::message::Serialize for $Mut<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::error::SerializeError> {
                self.0.serialize()
            }
            fn serialized_len(&self) -> usize {
                self.0.serialized_len()
            }
            fn encode(
                &self,
                out: &mut impl $crate::wire::WireOut,
            ) -> Result<(), $crate::error::SerializeError> {
                self.0.encode(out)
            }
        }
        impl $crate::message::Clear for $Owned {
            fn clear(&mut self) {
                *self = Self::default();
            }
        }
        impl $crate::message::Clear for $Mut<'_> {
            fn clear(&mut self) {
                *self.0 = $Owned::default();
            }
        }
        impl $crate::message::ClearAndParse for $Owned {
            const EMPTY_PARSE_OK: bool = true;
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::error::ParseError> {
                $crate::message::Clear::clear(self);
                self.merge_bytes(data)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::error::ParseError> {
                self.clear_and_parse(data)
            }
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::error::ParseError> {
                self.merge_bytes(data)
            }
        }
        impl $crate::message::ClearAndParse for $Mut<'_> {
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::error::ParseError> {
                self.0.clear_and_parse(data)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::error::ParseError> {
                self.0.clear_and_parse_dont_enforce_required(data)
            }
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::error::ParseError> {
                self.0.merge_from_bytes(data)
            }
        }
        impl $crate::message::CopyFrom for $Owned {
            fn copy_from(&mut self, src: impl $crate::proxied::AsView<Proxied = Self>) {
                *self = src.as_view().0.clone();
            }
        }
        impl $crate::message::CopyFrom for $Mut<'_> {
            fn copy_from(&mut self, src: impl $crate::proxied::AsView<Proxied = $Owned>) {
                *self.0 = src.as_view().0.clone();
            }
        }
        impl $crate::message::TakeFrom for $Owned {
            fn take_from(&mut self, mut src: impl $crate::proxied::AsMut<MutProxied = Self>) {
                *self = std::mem::take(src.as_mut().0);
            }
        }
        impl $crate::message::TakeFrom for $Mut<'_> {
            fn take_from(&mut self, mut src: impl $crate::proxied::AsMut<MutProxied = $Owned>) {
                *self.0 = std::mem::take(src.as_mut().0);
            }
        }
        impl $crate::message::MergeFrom for $Owned {
            fn merge_from(&mut self, src: impl $crate::proxied::AsView<Proxied = Self>) {
                let bytes =
                    $crate::message::Serialize::serialize(src.as_view().0).unwrap_or_default();
                let _ = self.merge_bytes(&bytes);
            }
        }
        impl $crate::message::MergeFrom for $Mut<'_> {
            fn merge_from(&mut self, src: impl $crate::proxied::AsView<Proxied = $Owned>) {
                $crate::message::MergeFrom::merge_from(self.0, src);
            }
        }
        impl $crate::message::Message for $Owned {
            type MessageView<'msg> = $View<'msg>;
            type MessageMut<'msg> = $Mut<'msg>;
        }
        impl<'msg> $crate::message::MessageView<'msg> for $View<'msg> {
            type Message = $Owned;
        }
        impl<'msg> $crate::message::MessageMut<'msg> for $Mut<'msg> {
            type Message = $Owned;
        }
        impl std::fmt::Debug for $Mut<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self.0, f)
            }
        }
    };
}

use crate::error::ParseError;
use crate::map::{Map, MapMut, MapView};
use crate::proxied::IntoProxied;
use crate::repeated::{Repeated, RepeatedMut, RepeatedView};
use crate::string::ProtoString;
use crate::wire::{
    self, decode_tag, encode_len_field, encode_tag, encode_varint, key_len_value_len,
    read_len_bytes, tag_len, varint_len, UnknownFields, WIRE_LEN, WIRE_VARINT,
};
use std::mem::MaybeUninit;
use std::sync::OnceLock;

/// Small repeated/map storage: up to N elements inline, no heap.
struct InlineVec<T, const N: usize> {
    len: u8,
    data: [MaybeUninit<T>; N],
    heap: Vec<T>,
}

impl<T, const N: usize> Default for InlineVec<T, N> {
    fn default() -> Self {
        Self {
            len: 0,
            data: std::array::from_fn(|_| MaybeUninit::uninit()),
            heap: Vec::new(),
        }
    }
}

impl<T, const N: usize> InlineVec<T, N> {
    fn as_slice(&self) -> &[T] {
        if self.heap.is_empty() {
            let n = self.len as usize;
            // SAFETY: push() initializes data[0..len].
            unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const T, n) }
        } else {
            &self.heap
        }
    }

    fn push(&mut self, v: T) {
        if self.heap.is_empty() && (self.len as usize) < N {
            self.data[self.len as usize].write(v);
            self.len += 1;
            return;
        }
        if self.heap.is_empty() {
            for i in 0..self.len as usize {
                // SAFETY: data[0..len] initialized.
                self.heap.push(unsafe { self.data[i].assume_init_read() });
            }
            self.len = 0;
        }
        self.heap.push(v);
    }

    fn as_mut_vec(&mut self) -> &mut Vec<T> {
        if self.heap.is_empty() {
            for i in 0..self.len as usize {
                self.heap.push(unsafe { self.data[i].assume_init_read() });
            }
            self.len = 0;
        }
        &mut self.heap
    }
}

impl<T: Clone, const N: usize> Clone for InlineVec<T, N> {
    fn clone(&self) -> Self {
        let mut out = Self::default();
        for v in self.as_slice() {
            out.push(v.clone());
        }
        out
    }
}

impl<T: PartialEq, const N: usize> PartialEq for InlineVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: std::fmt::Debug, const N: usize> std::fmt::Debug for InlineVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<T, const N: usize> Drop for InlineVec<T, N> {
    fn drop(&mut self) {
        if self.heap.is_empty() {
            for i in 0..self.len as usize {
                unsafe { self.data[i].assume_init_drop() };
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Address {
    city: ProtoString,
    unknown: UnknownFields,
    cached_size: crate::rt::CachedSize,
}

impl Default for Address {
    #[inline(always)]
    fn default() -> Self {
        unsafe { crate::rt::zeroed_message() }
    }
}

impl Address {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn city(&self) -> &crate::ProtoStr {
        self.city.as_view()
    }
    pub fn set_city(&mut self, v: impl IntoProxied<ProtoString>) {
        self.cached_size.dirty();
        self.city = v.into_proxied();
    }
    pub fn clear_city(&mut self) {
        self.cached_size.dirty();
        self.city.clear();
    }

    #[inline]
    fn merge_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.cached_size.dirty();
        let mut pos = 0;
        while pos < data.len() {
            let (n, w) = decode_tag(data, &mut pos)?;
            match n {
                1 if w == WIRE_LEN => {
                    self.city = ProtoString::from_bytes(read_len_bytes(data, &mut pos)?);
                }
                _ => self
                    .unknown
                    .fields
                    .push(wire::capture_unknown(data, &mut pos, n, w)?),
            }
        }
        Ok(())
    }

    fn compute_size(&self) -> u64 {
        if let Some(n) = self.cached_size.get() {
            return n;
        }
        let mut n = 0u64;
        if !self.city.is_empty() {
            n += key_len_value_len(1, self.city.as_bytes().len() as u64);
        }
        n += self.unknown.encoded_len();
        self.cached_size.set(n);
        n
    }

    #[inline]
    fn write_to(&self, out: &mut impl crate::wire::WireOut) {
        if !self.city.is_empty() {
            encode_len_field(out, 1, self.city.as_bytes());
        }
        self.unknown.encode(out);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AddressView<'msg>(pub &'msg Address);
pub struct AddressMut<'msg>(pub &'msg mut Address);

impl AddressView<'_> {
    pub fn city(&self) -> &crate::ProtoStr {
        self.0.city()
    }
}

impl_message! { Address, AddressView, AddressMut }

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Person {
    id: i32,
    name: ProtoString,
    email: Option<ProtoString>,
    tags: InlineVec<ProtoString, 4>,
    scores: InlineVec<(ProtoString, i32), 4>,
    address: Option<Address>,
    unknown: UnknownFields,
    cached_size: crate::rt::CachedSize,
}

impl Person {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn set_id(&mut self, v: i32) {
        self.cached_size.dirty();
        self.id = v;
    }

    pub fn name(&self) -> &crate::ProtoStr {
        self.name.as_view()
    }
    pub fn set_name(&mut self, v: impl IntoProxied<ProtoString>) {
        self.cached_size.dirty();
        self.name = v.into_proxied();
    }

    pub fn has_email(&self) -> bool {
        self.email.is_some()
    }
    pub fn email(&self) -> &crate::ProtoStr {
        self.email
            .as_ref()
            .map(ProtoString::as_view)
            .unwrap_or_else(|| crate::ProtoStr::from_bytes(b""))
    }
    pub fn email_opt(&self) -> Option<&crate::ProtoStr> {
        self.email.as_ref().map(ProtoString::as_view)
    }
    pub fn set_email(&mut self, v: impl IntoProxied<ProtoString>) {
        self.cached_size.dirty();
        self.email = Some(v.into_proxied());
    }
    pub fn clear_email(&mut self) {
        self.cached_size.dirty();
        self.email = None;
    }

    pub fn tags(&self) -> RepeatedView<'_, ProtoString> {
        RepeatedView::from_slice(self.tags.as_slice())
    }
    pub fn tags_mut(&mut self) -> RepeatedMut<'_, ProtoString> {
        self.cached_size.dirty();
        RepeatedMut::from_vec(self.tags.as_mut_vec())
    }
    pub fn set_tags(&mut self, v: impl IntoProxied<Repeated<ProtoString>>) {
        self.cached_size.dirty();
        self.tags = InlineVec::default();
        for t in v.into_proxied().into_vec() {
            self.tags.push(t);
        }
    }

    pub fn scores(&self) -> MapView<'_, ProtoString, i32> {
        MapView::from_slice(self.scores.as_slice())
    }
    pub fn scores_mut(&mut self) -> MapMut<'_, ProtoString, i32> {
        self.cached_size.dirty();
        MapMut::from_vec(self.scores.as_mut_vec())
    }
    pub fn set_scores(&mut self, v: impl IntoProxied<Map<ProtoString, i32>>) {
        self.cached_size.dirty();
        self.scores = InlineVec::default();
        for p in v.into_proxied().pairs() {
            self.scores.push(p.clone());
        }
    }

    pub fn has_address(&self) -> bool {
        self.address.is_some()
    }
    pub fn address(&self) -> AddressView<'_> {
        match &self.address {
            Some(a) => AddressView(a),
            None => AddressView(EMPTY_ADDRESS.get_or_init(Address::default)),
        }
    }
    pub fn address_opt(&self) -> Option<AddressView<'_>> {
        self.address.as_ref().map(AddressView)
    }
    pub fn set_address(&mut self, v: Address) {
        self.cached_size.dirty();
        self.address = Some(v);
    }
    pub fn clear_address(&mut self) {
        self.cached_size.dirty();
        self.address = None;
    }
    pub fn address_mut(&mut self) -> AddressMut<'_> {
        self.cached_size.dirty();
        AddressMut(self.address.get_or_insert_with(Address::default))
    }

    #[inline]
    fn merge_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.cached_size.dirty();
        let mut pos = 0;
        while pos < data.len() {
            let (n, w) = decode_tag(data, &mut pos)?;
            match n {
                1 if w == WIRE_VARINT => {
                    self.id = crate::wire::decode_varint(data, &mut pos)? as i32;
                }
                2 if w == WIRE_LEN => {
                    self.name = ProtoString::from_bytes(read_len_bytes(data, &mut pos)?);
                }
                3 if w == WIRE_LEN => {
                    self.email = Some(ProtoString::from_bytes(read_len_bytes(data, &mut pos)?));
                }
                4 if w == WIRE_LEN => {
                    self.tags
                        .push(ProtoString::from_bytes(read_len_bytes(data, &mut pos)?));
                }
                5 if w == WIRE_LEN => {
                    let payload = read_len_bytes(data, &mut pos)?;
                    let (k, v) = decode_string_i32_entry(payload)?;
                    self.scores.push((k, v));
                }
                6 if w == WIRE_LEN => {
                    let payload = read_len_bytes(data, &mut pos)?;
                    match &mut self.address {
                        Some(existing) => existing.merge_bytes(payload)?,
                        None => {
                            let mut a = Address::default();
                            a.merge_bytes(payload)?;
                            self.address = Some(a);
                        }
                    }
                }
                _ => self
                    .unknown
                    .fields
                    .push(wire::capture_unknown(data, &mut pos, n, w)?),
            }
        }
        Ok(())
    }

    fn compute_size(&self) -> u64 {
        if let Some(n) = self.cached_size.get() {
            return n;
        }
        let mut n = 0u64;
        if self.id != 0 {
            n += tag_len(1, WIRE_VARINT) + varint_len(self.id as u64);
        }
        if !self.name.is_empty() {
            n += key_len_value_len(2, self.name.as_bytes().len() as u64);
        }
        if let Some(email) = &self.email {
            n += key_len_value_len(3, email.as_bytes().len() as u64);
        }
        for t in self.tags.as_slice() {
            n += key_len_value_len(4, t.as_bytes().len() as u64);
        }
        for (k, v) in self.scores.as_slice() {
            let inner = key_len_value_len(1, k.as_bytes().len() as u64)
                + if *v != 0 {
                    tag_len(2, WIRE_VARINT) + varint_len(*v as u64)
                } else {
                    0
                };
            n += key_len_value_len(5, inner);
        }
        if let Some(addr) = &self.address {
            n += key_len_value_len(6, addr.compute_size());
        }
        n += self.unknown.encoded_len();
        self.cached_size.set(n);
        n
    }

    #[inline]
    fn write_to(&self, out: &mut impl crate::wire::WireOut) {
        if self.id != 0 {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, self.id as u64);
        }
        if !self.name.is_empty() {
            encode_len_field(out, 2, self.name.as_bytes());
        }
        if let Some(email) = &self.email {
            encode_len_field(out, 3, email.as_bytes());
        }
        for t in self.tags.as_slice() {
            encode_len_field(out, 4, t.as_bytes());
        }
        for (k, v) in self.scores.as_slice() {
            let inner = key_len_value_len(1, k.as_bytes().len() as u64)
                + if *v != 0 {
                    tag_len(2, WIRE_VARINT) + varint_len(*v as u64)
                } else {
                    0
                };
            encode_tag(out, 5, WIRE_LEN);
            encode_varint(out, inner);
            encode_len_field(out, 1, k.as_bytes());
            if *v != 0 {
                encode_tag(out, 2, WIRE_VARINT);
                encode_varint(out, *v as u64);
            }
        }
        if let Some(addr) = &self.address {
            encode_tag(out, 6, WIRE_LEN);
            encode_varint(out, addr.compute_size());
            addr.write_to(out);
        }
        self.unknown.encode(out);
    }
}

fn decode_string_i32_entry(data: &[u8]) -> Result<(ProtoString, i32), ParseError> {
    let mut key = ProtoString::new();
    let mut val = 0i32;
    let mut pos = 0;
    while pos < data.len() {
        let (n, w) = decode_tag(data, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => {
                key = ProtoString::from_bytes(read_len_bytes(data, &mut pos)?);
            }
            (2, WIRE_VARINT) => val = crate::wire::decode_varint(data, &mut pos)? as i32,
            _ => wire::skip_field(data, &mut pos, w)?,
        }
    }
    Ok((key, val))
}

#[derive(Clone, Copy, Debug)]
pub struct PersonView<'msg>(&'msg Person);
pub struct PersonMut<'msg>(&'msg mut Person);

impl PersonView<'_> {
    pub fn id(&self) -> i32 {
        self.0.id()
    }
    pub fn name(&self) -> &crate::ProtoStr {
        self.0.name()
    }
}

impl_message! { Person, PersonView, PersonMut }

static EMPTY_ADDRESS: OnceLock<Address> = OnceLock::new();
