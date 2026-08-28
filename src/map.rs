//! Map field storage and views.
use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use crate::string::ProtoString;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

type LastWins<K> = OnceLock<BTreeMap<K, usize>>;

fn last_wins_index<K: MapKey, V>(entries: &[(K, V)]) -> BTreeMap<K, usize> {
    let mut idx = BTreeMap::new();
    for (i, (k, _)) in entries.iter().enumerate() {
        idx.insert(k.clone(), i);
    }
    idx
}

fn scan_unique_len<K: MapKey, V>(v: &[(K, V)]) -> usize {
    let mut n = 0;
    for (i, (k, _)) in v.iter().enumerate() {
        if v.get(i + 1..)
            .is_some_and(|rest| rest.iter().all(|(k2, _)| k2 != k))
        {
            n += 1;
        }
    }
    n
}

fn unique_len<K: MapKey, V>(entries: &[(K, V)], index: Option<&LastWins<K>>) -> usize {
    if entries.is_empty() {
        return 0;
    }
    match index {
        Some(lock) => lock.get_or_init(|| last_wins_index(entries)).len(),
        None => scan_unique_len(entries),
    }
}

fn last_wins_pos<K: MapKey, V>(
    entries: &[(K, V)],
    index: Option<&LastWins<K>>,
    key: &K,
) -> Option<usize> {
    if let Some(lock) = index {
        return lock
            .get_or_init(|| last_wins_index(entries))
            .get(key)
            .copied();
    }
    entries.iter().rposition(|(k, _)| k == key)
}

fn last_wins_get<'a, K: MapKey, V>(
    entries: &'a [(K, V)],
    index: Option<&LastWins<K>>,
    key: &K,
) -> Option<&'a V> {
    let pos = last_wins_pos(entries, index, key)?;
    entries.get(pos).map(|(_, v)| v)
}

/// Types allowed as map keys.
pub trait MapKey: Clone + Ord + Eq + 'static {}
impl MapKey for i32 {}
impl MapKey for i64 {}
impl MapKey for u32 {}
impl MapKey for u64 {}
impl MapKey for bool {}
impl MapKey for ProtoString {}

/// Key argument for `MapView::get` / `MapMut::{get,insert,remove}`.
/// Accepts `K`, `&K`, `View<K>`, and `&str` for string keys.
pub trait MapQuery<K: MapKey> {
    fn eq_key(&self, k: &K) -> bool;
    fn to_owned_key(&self) -> K;
    fn key_bytes(&self) -> Vec<u8>;
}

impl MapQuery<i32> for i32 {
    fn eq_key(&self, k: &i32) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> i32 {
        *self
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
}
impl MapQuery<i32> for &i32 {
    fn eq_key(&self, k: &i32) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> i32 {
        **self
    }
    fn key_bytes(&self) -> Vec<u8> {
        (**self).to_le_bytes().to_vec()
    }
}
impl MapQuery<i64> for i64 {
    fn eq_key(&self, k: &i64) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> i64 {
        *self
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
}
impl MapQuery<i64> for &i64 {
    fn eq_key(&self, k: &i64) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> i64 {
        **self
    }
    fn key_bytes(&self) -> Vec<u8> {
        (**self).to_le_bytes().to_vec()
    }
}
impl MapQuery<u32> for u32 {
    fn eq_key(&self, k: &u32) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> u32 {
        *self
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
}
impl MapQuery<u32> for &u32 {
    fn eq_key(&self, k: &u32) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> u32 {
        **self
    }
    fn key_bytes(&self) -> Vec<u8> {
        (**self).to_le_bytes().to_vec()
    }
}
impl MapQuery<u64> for u64 {
    fn eq_key(&self, k: &u64) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> u64 {
        *self
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
}
impl MapQuery<u64> for &u64 {
    fn eq_key(&self, k: &u64) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> u64 {
        **self
    }
    fn key_bytes(&self) -> Vec<u8> {
        (**self).to_le_bytes().to_vec()
    }
}
impl MapQuery<bool> for bool {
    fn eq_key(&self, k: &bool) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> bool {
        *self
    }
    fn key_bytes(&self) -> Vec<u8> {
        vec![*self as u8]
    }
}
impl MapQuery<bool> for &bool {
    fn eq_key(&self, k: &bool) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> bool {
        **self
    }
    fn key_bytes(&self) -> Vec<u8> {
        vec![**self as u8]
    }
}
impl MapQuery<ProtoString> for ProtoString {
    fn eq_key(&self, k: &ProtoString) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> ProtoString {
        self.clone()
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl MapQuery<ProtoString> for &ProtoString {
    fn eq_key(&self, k: &ProtoString) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> ProtoString {
        (*self).clone()
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl MapQuery<ProtoString> for &str {
    fn eq_key(&self, k: &ProtoString) -> bool {
        k.as_bytes() == self.as_bytes()
    }
    fn to_owned_key(&self) -> ProtoString {
        ProtoString::from(*self)
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl MapQuery<ProtoString> for &crate::string::ProtoStr {
    fn eq_key(&self, k: &ProtoString) -> bool {
        k.as_bytes() == self.as_bytes()
    }
    fn to_owned_key(&self) -> ProtoString {
        ProtoString::from(*self)
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// Types allowed as map values.
pub trait MapValue: Clone + 'static {}
impl<T: Clone + 'static> MapValue for T {}

/// Empty is an 8-byte null. Parse appends to a `Vec` (last-wins). Lookup uses a lazy index.
#[derive(Clone)]
pub struct Map<K: MapKey, V: MapValue>(Option<Box<MapInner<K, V>>>);

struct MapInner<K: MapKey, V: MapValue> {
    entries: Vec<(K, V)>,
    index: LastWins<K>,
}

impl<K: MapKey, V: MapValue> Clone for MapInner<K, V> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            index: OnceLock::new(),
        }
    }
}

impl<K: MapKey, V: MapValue> Default for Map<K, V> {
    #[inline]
    fn default() -> Self {
        Self(None)
    }
}

impl<K: MapKey, V: MapValue + PartialEq> PartialEq for Map<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self.iter().all(|(k, v)| other.get(k) == Some(v))
            && other.iter().all(|(k, _)| self.get(k).is_some())
    }
}
impl<K: MapKey, V: MapValue + Eq> Eq for Map<K, V> {}

impl<K: MapKey, V: MapValue> Map<K, V> {
    #[inline]
    pub fn new() -> Self {
        Self(None)
    }

    pub fn as_view(&self) -> MapView<'_, K, V> {
        match self.0.as_deref() {
            Some(inner) => MapView {
                inner: Some(inner.entries.as_slice()),
                index: Some(&inner.index),
                raw: None,
            },
            None => MapView::empty(),
        }
    }

    pub fn as_mut(&mut self) -> MapMut<'_, K, V> {
        let inner = self.ensure();
        MapMut {
            inner: Some(&mut inner.entries),
            index: Some(&mut inner.index),
            raw: None,
            arena: None,
        }
    }

    #[inline]
    fn ensure(&mut self) -> &mut MapInner<K, V> {
        self.0.get_or_insert_with(|| {
            Box::new(MapInner {
                entries: Vec::new(),
                index: OnceLock::new(),
            })
        })
    }

    /// Append a parsed entry without scanning (protobuf last-wins).
    #[inline]
    pub fn push_entry(&mut self, key: K, value: V) {
        let inner = self.ensure();
        inner.entries.push((key, value));
        inner.index = OnceLock::new();
    }

    /// Returns `true` if the key was newly inserted.
    pub fn insert(&mut self, key: impl Into<K>, value: impl Into<V>) -> bool {
        let key = key.into();
        let value = value.into();
        let inner = self.ensure();
        let existing = last_wins_pos(&inner.entries, Some(&inner.index), &key);
        inner.index = OnceLock::new();
        if let Some(i) = existing {
            if let Some(e) = inner.entries.get_mut(i) {
                e.1 = value;
                return false;
            }
        }
        inner.entries.push((key, value));
        true
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let inner = self.0.as_ref()?;
        last_wins_get(&inner.entries, Some(&inner.index), key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let inner = self.0.as_mut()?;
        let i = last_wins_pos(&inner.entries, Some(&inner.index), key)?;
        let out = inner.entries.swap_remove(i).1;
        inner.index = OnceLock::new();
        if inner.entries.is_empty() {
            self.0 = None;
        }
        Some(out)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }

    #[inline]
    pub fn len(&self) -> usize {
        let Some(inner) = self.0.as_ref() else {
            return 0;
        };
        unique_len(&inner.entries, Some(&inner.index))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|inner| inner.entries.is_empty())
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn iter(&self) -> MapIter<'_, K, V> {
        MapIter {
            items: self.pairs(),
            i: 0,
        }
    }

    #[inline]
    pub fn pairs(&self) -> &[(K, V)] {
        self.0
            .as_deref()
            .map_or(&[], |inner| inner.entries.as_slice())
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for Map<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

pub struct MapIter<'msg, K, V> {
    items: &'msg [(K, V)],
    i: usize,
}

impl<'msg, K: MapKey, V> Iterator for MapIter<'msg, K, V> {
    type Item = (&'msg K, &'msg V);
    fn next(&mut self) -> Option<Self::Item> {
        while self.i < self.items.len() {
            let (k, v) = &self.items[self.i];
            self.i += 1;
            if self.items[self.i..].iter().all(|(k2, _)| k2 != k) {
                return Some((k, v));
            }
        }
        None
    }
}

pub struct MapView<'msg, K: MapKey, V: MapValue> {
    inner: Option<&'msg [(K, V)]>,
    index: Option<&'msg LastWins<K>>,
    raw: Option<crate::runtime::RawMap>,
}

impl<K: MapKey, V: MapValue> Clone for MapView<'_, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K: MapKey, V: MapValue> Copy for MapView<'_, K, V> {}

impl<'msg, K: MapKey, V: MapValue> MapView<'msg, K, V> {
    #[inline]
    pub fn empty() -> Self {
        Self {
            inner: None,
            index: None,
            raw: None,
        }
    }

    #[inline]
    pub fn from_slice(items: &'msg [(K, V)]) -> Self {
        if items.is_empty() {
            Self::empty()
        } else {
            Self {
                inner: Some(items),
                index: None,
                raw: None,
            }
        }
    }

    #[doc(hidden)]
    pub unsafe fn from_raw(
        _private: crate::internal::Private,
        raw: crate::runtime::RawMap,
    ) -> Self {
        Self {
            inner: None,
            index: None,
            raw: Some(raw),
        }
    }

    #[doc(hidden)]
    pub unsafe fn from_raw_ptr(raw: crate::runtime::RawMap) -> Self {
        Self {
            inner: None,
            index: None,
            raw: Some(raw),
        }
    }

    pub fn get(self, key: impl MapQuery<K>) -> Option<crate::proxied::View<'msg, V>>
    where
        V: crate::proxied::Proxied + 'static,
    {
        if let Some(raw) = self.raw {
            let kb = key.key_bytes();
            return unsafe { crate::runtime::kernel_map_get_bytes::<V>(raw, &kb) };
        }
        let entries = self.inner?;
        if self.index.is_some() {
            let owned = key.to_owned_key();
            return last_wins_get(entries, self.index, &owned).map(crate::proxied::AsView::as_view);
        }
        entries
            .iter()
            .rev()
            .find(|(k, _)| key.eq_key(k))
            .map(|(_, v)| crate::proxied::AsView::as_view(v))
    }

    pub fn len(self) -> usize {
        if let Some(raw) = self.raw {
            return crate::runtime::kernel_map_len(raw);
        }
        unique_len(self.inner.unwrap_or(&[]), self.index)
    }

    pub fn is_empty(self) -> bool {
        if self.raw.is_some() {
            return self.len() == 0;
        }
        self.inner.is_none_or(|v| v.is_empty())
    }

    pub fn keys(self) -> impl Iterator<Item = crate::proxied::View<'msg, K>> + 'msg
    where
        K: crate::proxied::Proxied + 'static,
        V: crate::proxied::Proxied + 'static,
    {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(self) -> impl Iterator<Item = crate::proxied::View<'msg, V>> + 'msg
    where
        K: crate::proxied::Proxied + 'static,
        V: crate::proxied::Proxied + 'static,
    {
        self.iter().map(|(_, v)| v)
    }

    pub fn iter(self) -> MapViewIter<'msg, K, V>
    where
        K: crate::proxied::Proxied + 'static,
        V: crate::proxied::Proxied + 'static,
    {
        MapViewIter::from_view(self)
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for MapView<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MapView").field("len", &self.len()).finish()
    }
}

pub struct MapMut<'msg, K: MapKey, V: MapValue> {
    inner: Option<&'msg mut Vec<(K, V)>>,
    index: Option<&'msg mut LastWins<K>>,
    raw: Option<crate::runtime::RawMap>,
    arena: Option<&'msg crate::runtime::Arena>,
}

impl<'msg, K: MapKey, V: MapValue> MapMut<'msg, K, V> {
    pub fn from_vec(inner: &'msg mut Vec<(K, V)>) -> Self {
        Self {
            inner: Some(inner),
            index: None,
            raw: None,
            arena: None,
        }
    }

    #[doc(hidden)]
    pub unsafe fn from_inner(
        _private: crate::internal::Private,
        inner: crate::runtime::InnerMapMut<'msg>,
    ) -> Self {
        Self {
            inner: None,
            index: None,
            raw: Some(inner.raw),
            arena: Some(inner.arena),
        }
    }

    #[doc(hidden)]
    pub fn from_raw_inner(raw: crate::runtime::RawMap) -> Self {
        Self {
            inner: None,
            index: None,
            raw: Some(raw),
            arena: None,
        }
    }

    fn drop_index(index: &mut Option<&mut LastWins<K>>) {
        if let Some(lock) = index.as_mut() {
            **lock = OnceLock::new();
        }
    }

    /// Returns `true` if the key was newly inserted.
    pub fn insert(
        &mut self,
        key: impl MapQuery<K>,
        value: impl crate::proxied::IntoProxied<V>,
    ) -> bool
    where
        V: 'static,
    {
        let key = key.to_owned_key();
        let value = crate::proxied::IntoProxied::into_proxied(value, crate::internal::Private);
        self.insert_owned(key, value)
    }

    fn insert_owned(&mut self, key: K, value: V) -> bool
    where
        V: 'static,
    {
        if let Some(v) = self.inner.as_mut() {
            let existing = last_wins_pos(v, self.index.as_deref(), &key);
            if let Some(i) = existing {
                if let Some(e) = v.get_mut(i) {
                    e.1 = value;
                    Self::drop_index(&mut self.index);
                    return false;
                }
            }
            v.push((key, value));
            Self::drop_index(&mut self.index);
            true
        } else if let Some(raw) = self.raw {
            crate::runtime::kernel_map_insert(raw, key, value, self.arena)
        } else {
            false
        }
    }

    pub fn get(&self, key: impl MapQuery<K>) -> Option<crate::proxied::View<'_, V>>
    where
        V: crate::proxied::Proxied + 'static,
    {
        self.as_view().get(key)
    }

    pub fn get_mut(&mut self, key: impl MapQuery<K>) -> Option<crate::proxied::Mut<'_, V>>
    where
        V: crate::message::Message + crate::proxied::MutProxied + 'static,
    {
        let owned = key.to_owned_key();
        if let Some(v) = self.inner.as_mut() {
            return last_wins_pos(v, self.index.as_deref(), &owned)
                .and_then(|i| v.get_mut(i))
                .map(|(_, val)| crate::proxied::AsMut::as_mut(val));
        }
        if let (Some(raw), Some(arena)) = (self.raw, self.arena) {
            let kb = key.key_bytes();
            unsafe {
                let entries = (*raw).entries.borrow();
                let fk = entries
                    .iter()
                    .rev()
                    .find(|(k, _)| k == &kb)
                    .map(|(_, v)| *v);
                drop(entries);
                if let Some(crate::runtime::FieldKind::Msg(p)) = fk {
                    return crate::runtime::kernel_msg_ptr_to_mut::<V>(p, arena);
                }
            }
        }
        None
    }

    pub fn remove(&mut self, key: impl MapQuery<K>) -> bool {
        let key = key.to_owned_key();
        if let Some(v) = self.inner.as_mut() {
            if let Some(i) = last_wins_pos(v, self.index.as_deref(), &key) {
                v.swap_remove(i);
                Self::drop_index(&mut self.index);
                return true;
            }
            false
        } else if let Some(raw) = self.raw {
            unsafe {
                let kb = crate::runtime::kernel_key_bytes(key.clone());
                let mut entries = (*raw).entries.borrow_mut();
                if let Some(i) = entries.iter().rposition(|(k, _)| k == &kb) {
                    entries.swap_remove(i);
                    true
                } else {
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn copy_from<'a>(
        &mut self,
        src: impl IntoIterator<Item = (impl MapQuery<K> + 'a, impl crate::proxied::IntoProxied<V>)>,
    ) where
        V: crate::proxied::Proxied + 'static,
    {
        self.clear();
        for (k, v) in src {
            self.insert(k, v);
        }
    }

    pub fn len(&self) -> usize {
        if let Some(raw) = self.raw {
            return crate::runtime::kernel_map_len(raw);
        }
        unique_len(
            self.inner.as_deref().map_or(&[], |v| v.as_slice()),
            self.index.as_deref(),
        )
    }

    pub fn is_empty(&self) -> bool {
        if self.raw.is_some() {
            return self.len() == 0;
        }
        self.inner.as_ref().is_none_or(|v| v.is_empty())
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner
            .as_ref()
            .map(|v| v.iter())
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner
            .as_ref()
            .map(|v| v.iter())
            .into_iter()
            .flatten()
            .map(|(_, v)| v)
    }

    pub fn clear(&mut self) {
        if let Some(v) = self.inner.as_mut() {
            v.clear();
            Self::drop_index(&mut self.index);
        } else if let Some(raw) = self.raw {
            unsafe {
                (*raw).entries.borrow_mut().clear();
                (*raw).strs.borrow_mut().clear();
            }
        }
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for MapMut<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.as_ref() {
            Some(v) => f.debug_list().entries(v.iter()).finish(),
            None => f.debug_struct("MapMut").field("raw", &self.raw).finish(),
        }
    }
}

impl<K: MapKey, V: MapValue> SealedInternal for Map<K, V> {}
impl<K: MapKey, V: MapValue> Proxied for Map<K, V> {
    type View<'msg> = MapView<'msg, K, V>;
}
impl<K: MapKey, V: MapValue> MutProxied for Map<K, V> {
    type Mut<'msg> = MapMut<'msg, K, V>;
}
impl<K: MapKey, V: MapValue> AsView for Map<K, V> {
    type Proxied = Self;
    fn as_view(&self) -> MapView<'_, K, V> {
        Map::as_view(self)
    }
}
impl<K: MapKey, V: MapValue> AsMut for Map<K, V> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> MapMut<'_, K, V> {
        Map::as_mut(self)
    }
}

impl<K: MapKey, V: MapValue> SealedInternal for MapView<'_, K, V> {}
impl<K: MapKey, V: MapValue> AsView for MapView<'_, K, V> {
    type Proxied = Map<K, V>;
    fn as_view(&self) -> MapView<'_, K, V> {
        *self
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoView<'msg> for MapView<'msg, K, V> {
    fn into_view<'shorter>(self) -> MapView<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        MapView {
            inner: self.inner,
            index: self.index,
            raw: self.raw,
        }
    }
}

impl<K: MapKey, V: MapValue> SealedInternal for MapMut<'_, K, V> {}
impl<K: MapKey, V: MapValue> AsView for MapMut<'_, K, V> {
    type Proxied = Map<K, V>;
    fn as_view(&self) -> MapView<'_, K, V> {
        MapView {
            inner: self.inner.as_ref().map(|v| v.as_slice()),
            index: self.index.as_deref(),
            raw: self.raw,
        }
    }
}
impl<K: MapKey, V: MapValue> AsMut for MapMut<'_, K, V> {
    type MutProxied = Map<K, V>;
    fn as_mut(&mut self) -> MapMut<'_, K, V> {
        MapMut {
            inner: self.inner.as_deref_mut(),
            index: self.index.as_deref_mut(),
            raw: self.raw,
            arena: self.arena,
        }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoView<'msg> for MapMut<'msg, K, V> {
    fn into_view<'shorter>(self) -> MapView<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        MapView {
            inner: self.inner.map(|v| v.as_slice()),
            index: self.index.map(|i| &*i),
            raw: self.raw,
        }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoMut<'msg> for MapMut<'msg, K, V> {
    fn into_mut<'shorter>(self) -> MapMut<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        MapMut {
            inner: self.inner,
            index: self.index,
            raw: self.raw,
            arena: self.arena,
        }
    }
}

impl<'msg, K: MapKey, V: MapValue> MapMut<'msg, K, V> {
    pub fn default_value(&self) -> V
    where
        V: Default,
    {
        V::default()
    }

    pub fn proto_put<KI: MapQuery<K>, VI: crate::proxied::IntoProxied<V>>(&mut self, pair: (KI, VI))
    where
        V: 'static,
    {
        self.insert(pair.0, pair.1);
    }
}

pub struct MapViewIter<
    'msg,
    K: MapKey + crate::proxied::Proxied,
    V: MapValue + crate::proxied::Proxied,
> {
    items: Vec<Option<(crate::proxied::View<'msg, K>, crate::proxied::View<'msg, V>)>>,
    i: usize,
}

impl<
        'msg,
        K: MapKey + crate::proxied::Proxied + 'static,
        V: MapValue + crate::proxied::Proxied + 'static,
    > MapViewIter<'msg, K, V>
{
    fn from_view(view: MapView<'msg, K, V>) -> Self {
        let mut items = Vec::new();
        if let Some(raw) = view.raw {
            unsafe {
                let entries = (*raw).entries.borrow();
                for (i, (kb, fk)) in entries.iter().enumerate() {
                    if entries[i + 1..].iter().any(|(k2, _)| k2 == kb) {
                        continue;
                    }
                    let leaked: &'static [u8] = Box::leak(kb.clone().into_boxed_slice());
                    let Some(k) = crate::runtime::kernel_bytes_to_view::<K>(leaked) else {
                        continue;
                    };
                    let Some(v) = crate::runtime::kernel_fieldkind_to_view::<'msg, V>(*fk) else {
                        continue;
                    };
                    items.push(Some((k, v)));
                }
            }
        } else if let Some(inner) = view.inner {
            for (i, (k, v)) in inner.iter().enumerate() {
                if inner[i + 1..].iter().any(|(k2, _)| k2 == k) {
                    continue;
                }
                items.push(Some((
                    crate::proxied::AsView::as_view(k),
                    crate::proxied::AsView::as_view(v),
                )));
            }
        }
        Self { items, i: 0 }
    }
}

impl<'msg, K, V> Iterator for MapViewIter<'msg, K, V>
where
    K: MapKey + crate::proxied::Proxied,
    V: MapValue + crate::proxied::Proxied,
{
    type Item = (crate::proxied::View<'msg, K>, crate::proxied::View<'msg, V>);
    fn next(&mut self) -> Option<Self::Item> {
        if self.i >= self.items.len() {
            return None;
        }
        let item = self.items[self.i].take();
        self.i += 1;
        item
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.items.len().saturating_sub(self.i);
        (n, Some(n))
    }
}

impl<
        'msg,
        K: MapKey + crate::proxied::Proxied + 'static,
        V: MapValue + crate::proxied::Proxied + 'static,
    > IntoIterator for MapView<'msg, K, V>
{
    type Item = (crate::proxied::View<'msg, K>, crate::proxied::View<'msg, V>);
    type IntoIter = MapViewIter<'msg, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        MapViewIter::from_view(self)
    }
}

impl<K: MapKey + crate::proxied::Proxied, V: MapValue + crate::proxied::Proxied>
    IntoProxied<Map<K, V>> for MapView<'_, K, V>
where
    for<'a> crate::proxied::View<'a, K>: Into<K>,
    for<'a> crate::proxied::View<'a, V>: crate::proxied::IntoProxied<V>,
{
    fn into_proxied(self, private: crate::internal::Private) -> Map<K, V> {
        let mut m = Map::new();
        for (k, v) in self {
            m.insert(
                k.into(),
                crate::proxied::IntoProxied::into_proxied(v, private),
            );
        }
        m
    }
}

impl<K: MapKey + crate::proxied::Proxied, V: MapValue + crate::proxied::Proxied>
    IntoProxied<Map<K, V>> for MapMut<'_, K, V>
where
    for<'a> crate::proxied::View<'a, K>: Into<K>,
    for<'a> crate::proxied::View<'a, V>: crate::proxied::IntoProxied<V>,
{
    fn into_proxied(self, private: crate::internal::Private) -> Map<K, V> {
        self.into_view().into_proxied(private)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_eq_and_zeroed() {
        let z: Map<i32, i32> = unsafe { std::mem::zeroed() };
        assert!(z.is_empty());
        assert_eq!(z, Map::new());
        assert_eq!(
            std::mem::size_of::<Map<i32, i32>>(),
            std::mem::size_of::<usize>()
        );
        drop(z);
    }

    #[test]
    fn insert_remove() {
        let mut m = Map::new();
        assert!(m.insert(1, 2));
        assert!(!m.insert(1, 3));
        assert_eq!(m.get(&1), Some(&3));
        assert_eq!(m.remove(&1), Some(3)); // Map::remove still returns Option
        assert!(m.is_empty());
    }

    #[test]
    fn push_entry_last_wins_and_pairs_keep_wire_order() {
        let mut m = Map::new();
        m.push_entry(1, 10);
        m.push_entry(2, 20);
        m.push_entry(1, 11);
        assert_eq!(m.pairs().len(), 3);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&1), Some(&11));
        assert_eq!(m.get(&2), Some(&20));
        assert_eq!(m.as_view().get(1), Some(11));
        assert_eq!(m.as_view().len(), 2);
        let mut it = m.iter();
        assert_eq!(it.next(), Some((&2, &20)));
        assert_eq!(it.next(), Some((&1, &11)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn index_invalidates_on_insert_remove_clear_push() {
        let mut m = Map::new();
        m.push_entry(1, 1);
        m.push_entry(2, 2);
        assert_eq!(m.get(&1), Some(&1));
        m.push_entry(1, 9);
        assert_eq!(m.get(&1), Some(&9));
        assert_eq!(m.len(), 2);
        assert!(!m.insert(2, 8));
        assert_eq!(m.get(&2), Some(&8));
        // remove drops the last-wins entry only; an earlier duplicate remains.
        assert_eq!(m.remove(&1), Some(9));
        assert_eq!(m.get(&1), Some(&1));
        assert_eq!(m.len(), 2);
        assert_eq!(m.remove(&1), Some(1));
        assert_eq!(m.get(&1), None);
        assert_eq!(m.len(), 1);
        m.clear();
        assert!(m.is_empty());
        assert_eq!(m.get(&2), None);
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn map_mut_uses_index_and_from_slice_still_scans() {
        let mut m = Map::new();
        m.push_entry(1, 1);
        m.push_entry(1, 2);
        {
            let mut mm = m.as_mut();
            assert_eq!(mm.len(), 1);
            assert_eq!(mm.get(1), Some(2));
            assert!(mm.remove(1));
            assert_eq!(mm.get(1), Some(1));
            assert!(mm.remove(1));
            assert!(mm.is_empty());
        }
        let pairs = [(3, 30), (3, 31)];
        let view = MapView::from_slice(&pairs);
        assert_eq!(view.get(3), Some(31));
        assert_eq!(view.len(), 1);
    }
}
