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

/// An owned map field. `BTreeMap` so encode order is deterministic.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Map<K: MapKey, V: MapValue>(BTreeMap<K, V>);

impl<K: MapKey, V: MapValue> Map<K, V> {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Returns `true` if the key was newly inserted.
    pub fn insert(&mut self, key: impl Into<K>, value: impl Into<V>) -> bool {
        self.0.insert(key.into(), value.into()).is_none()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.0.remove(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.values()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn iter(&self) -> MapIter<'_, K, V> {
        MapIter {
            inner: self.0.iter(),
        }
    }

    pub fn inner(&self) -> &BTreeMap<K, V> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut BTreeMap<K, V> {
        &mut self.0
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for Map<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

pub struct MapIter<'msg, K, V> {
    inner: std::collections::btree_map::Iter<'msg, K, V>,
}

impl<'msg, K, V> Iterator for MapIter<'msg, K, V> {
    type Item = (&'msg K, &'msg V);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct MapView<'msg, K: MapKey, V: MapValue> {
    inner: &'msg BTreeMap<K, V>,
}

impl<K: MapKey, V: MapValue> Clone for MapView<'_, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K: MapKey, V: MapValue> Copy for MapView<'_, K, V> {}

impl<'msg, K: MapKey, V: MapValue> MapView<'msg, K, V> {
    pub fn from_map(inner: &'msg BTreeMap<K, V>) -> Self {
        Self { inner }
    }

    pub fn get(self, key: &K) -> Option<&'msg V> {
        self.inner.get(key)
    }

    pub fn len(self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(self) -> bool {
        self.inner.is_empty()
    }

    pub fn keys(self) -> impl Iterator<Item = &'msg K> {
        self.inner.keys()
    }

    pub fn values(self) -> impl Iterator<Item = &'msg V> {
        self.inner.values()
    }

    pub fn iter(self) -> MapIter<'msg, K, V> {
        MapIter {
            inner: self.inner.iter(),
        }
    }
}

impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for MapView<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
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
        MapView { inner: &self.0 }
    }
}
impl<K: MapKey, V: MapValue> AsMut for Map<K, V> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> MapMut<'_, K, V> {
        MapMut { inner: &mut self.0 }
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
        MapView { inner: self.inner }
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
        MapView { inner: self.inner }
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
        Map(self.inner.clone())
    }
}
