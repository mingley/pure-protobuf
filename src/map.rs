use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use crate::string::ProtoString;
use std::collections::BTreeMap;
use std::fmt;

/// Types allowed as map keys.
pub trait MapKey: Clone + Ord + Eq + 'static {}
impl MapKey for i32 {}
impl MapKey for i64 {}
impl MapKey for u32 {}
impl MapKey for u64 {}
impl MapKey for bool {}
impl MapKey for ProtoString {}

/// Types allowed as map values.
pub trait MapValue: Clone + 'static {}
impl<T: Clone + 'static> MapValue for T {}

/// An owned map field. Empty `None` is zero-valid (`BTreeMap` layout is not guaranteed).
/// `BTreeMap` so encode order is deterministic.
#[derive(Clone)]
pub struct Map<K: MapKey, V: MapValue>(Option<BTreeMap<K, V>>);

impl<K: MapKey, V: MapValue> Default for Map<K, V> {
    #[inline]
    fn default() -> Self {
        Self(None)
    }
}

impl<K: MapKey, V: MapValue + PartialEq> PartialEq for Map<K, V> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            (None, Some(b)) => b.is_empty(),
            (Some(a), None) => a.is_empty(),
        }
    }
}
impl<K: MapKey, V: MapValue + Eq> Eq for Map<K, V> {}

impl<K: MapKey, V: MapValue> Map<K, V> {
    #[inline]
    pub fn new() -> Self {
        Self(None)
    }

    #[inline]
    fn ensure(&mut self) -> &mut BTreeMap<K, V> {
        self.0.get_or_insert_with(BTreeMap::new)
    }

    /// Returns `true` if the key was newly inserted.
    pub fn insert(&mut self, key: impl Into<K>, value: impl Into<V>) -> bool {
        self.ensure().insert(key.into(), value.into()).is_none()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.as_ref().and_then(|m| m.get(key))
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let m = self.0.as_mut()?;
        let v = m.remove(key);
        if m.is_empty() {
            self.0 = None;
        }
        v
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.iter().flat_map(|m| m.keys())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().flat_map(|m| m.values())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |m| m.len())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|m| m.is_empty())
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn iter(&self) -> MapIter<'_, K, V> {
        MapIter {
            inner: self.0.as_ref().map(|m| m.iter()),
        }
    }

    pub fn inner(&self) -> Option<&BTreeMap<K, V>> {
        self.0.as_ref()
    }

    pub fn inner_mut(&mut self) -> &mut BTreeMap<K, V> {
        self.ensure()
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for Map<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(m) => fmt::Debug::fmt(m, f),
            None => f.write_str("{}"),
        }
    }
}

pub struct MapIter<'msg, K, V> {
    inner: Option<std::collections::btree_map::Iter<'msg, K, V>>,
}

impl<'msg, K, V> Iterator for MapIter<'msg, K, V> {
    type Item = (&'msg K, &'msg V);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.as_mut()?.next()
    }
}

pub struct MapView<'msg, K: MapKey, V: MapValue> {
    inner: Option<&'msg BTreeMap<K, V>>,
}

impl<K: MapKey, V: MapValue> Clone for MapView<'_, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K: MapKey, V: MapValue> Copy for MapView<'_, K, V> {}

impl<'msg, K: MapKey, V: MapValue> MapView<'msg, K, V> {
    pub fn from_map(inner: &'msg BTreeMap<K, V>) -> Self {
        Self { inner: Some(inner) }
    }

    pub fn get(self, key: &K) -> Option<&'msg V> {
        self.inner.and_then(|m| m.get(key))
    }

    pub fn len(self) -> usize {
        self.inner.map_or(0, |m| m.len())
    }

    pub fn is_empty(self) -> bool {
        self.inner.is_none_or(|m| m.is_empty())
    }

    pub fn keys(self) -> impl Iterator<Item = &'msg K> {
        self.inner.into_iter().flat_map(|m| m.keys())
    }

    pub fn values(self) -> impl Iterator<Item = &'msg V> {
        self.inner.into_iter().flat_map(|m| m.values())
    }

    pub fn iter(self) -> MapIter<'msg, K, V> {
        MapIter {
            inner: self.inner.map(|m| m.iter()),
        }
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for MapView<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner {
            Some(m) => fmt::Debug::fmt(m, f),
            None => f.write_str("{}"),
        }
    }
}

pub struct MapMut<'msg, K: MapKey, V: MapValue> {
    inner: &'msg mut BTreeMap<K, V>,
}

impl<'msg, K: MapKey, V: MapValue> MapMut<'msg, K, V> {
    pub fn from_map(inner: &'msg mut BTreeMap<K, V>) -> Self {
        Self { inner }
    }

    /// Returns `true` if the key was newly inserted.
    pub fn insert(&mut self, key: impl Into<K>, value: impl Into<V>) -> bool {
        self.inner.insert(key.into(), value.into()).is_none()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for MapMut<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
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
        MapView {
            inner: self.0.as_ref(),
        }
    }
}
impl<K: MapKey, V: MapValue> AsMut for Map<K, V> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> MapMut<'_, K, V> {
        MapMut {
            inner: self.ensure(),
        }
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
        MapView { inner: self.inner }
    }
}

impl<K: MapKey, V: MapValue> SealedInternal for MapMut<'_, K, V> {}
impl<K: MapKey, V: MapValue> AsView for MapMut<'_, K, V> {
    type Proxied = Map<K, V>;
    fn as_view(&self) -> MapView<'_, K, V> {
        MapView {
            inner: Some(self.inner),
        }
    }
}
impl<K: MapKey, V: MapValue> AsMut for MapMut<'_, K, V> {
    type MutProxied = Map<K, V>;
    fn as_mut(&mut self) -> MapMut<'_, K, V> {
        MapMut { inner: self.inner }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoView<'msg> for MapMut<'msg, K, V> {
    fn into_view<'shorter>(self) -> MapView<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        MapView {
            inner: Some(self.inner),
        }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoMut<'msg> for MapMut<'msg, K, V> {
    fn into_mut<'shorter>(self) -> MapMut<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        MapMut { inner: self.inner }
    }
}

impl<K, V, KI, VI> crate::repeated::ProtoPut<(KI, VI)> for MapMut<'_, K, V>
where
    K: MapKey,
    V: MapValue,
    KI: Into<K>,
    VI: Into<V>,
{
    fn proto_put(&mut self, (k, v): (KI, VI)) {
        self.insert(k, v);
    }
}

impl<K: MapKey, V: MapValue> IntoProxied<Map<K, V>> for MapView<'_, K, V> {
    fn into_proxied(self) -> Map<K, V> {
        match self.inner {
            Some(m) if !m.is_empty() => Map(Some(m.clone())),
            _ => Map(None),
        }
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
        drop(z);
    }

    #[test]
    fn insert_remove() {
        let mut m = Map::new();
        assert!(m.insert(1, 2));
        assert!(!m.insert(1, 3));
        assert_eq!(m.get(&1), Some(&3));
        assert_eq!(m.remove(&1), Some(3));
        assert!(m.is_empty());
    }
}
