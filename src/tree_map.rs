//! Opt-in ordered maps with the same operations as typed map views.
use crate::internal::SealedInternal;
use crate::map::{Map, MapKey, MapQuery, MapValue};
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use std::collections::{BTreeMap, btree_map};
use std::fmt;

/// Ordered storage with a nullable-box empty state for generated zero defaults.
#[derive(Clone)]
#[repr(transparent)]
#[allow(
    clippy::box_collection,
    reason = "generated empty defaults require the guaranteed zero-valid nullable Box representation"
)]
pub struct TreeMap<K: MapKey, V: MapValue>(Option<Box<BTreeMap<K, V>>>);

impl<K: MapKey, V: MapValue> Default for TreeMap<K, V> {
    fn default() -> Self {
        Self(None)
    }
}
impl<K: MapKey, V: MapValue + PartialEq> PartialEq for TreeMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.pairs().eq(other.pairs())
    }
}
impl<K: MapKey, V: MapValue + Eq> Eq for TreeMap<K, V> {}

impl<K: MapKey, V: MapValue> TreeMap<K, V> {
    /// Create an empty ordered map.
    pub fn new() -> Self {
        Self::default()
    }
    fn ensure(&mut self) -> &mut BTreeMap<K, V> {
        self.0.get_or_insert_with(|| Box::new(BTreeMap::new()))
    }

    /// Insert a parsed entry; a later value replaces an earlier one.
    pub fn push_entry(&mut self, key: K, value: V) {
        drop(self.ensure().insert(key, value));
    }
    /// Iterate over unique entries in key order.
    pub fn pairs(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter().flat_map(|map| map.iter())
    }
    /// Whether no entries are present.
    pub fn is_empty(&self) -> bool {
        self.0.as_deref().is_none_or(BTreeMap::is_empty)
    }
    /// Number of distinct keys.
    pub fn len(&self) -> usize {
        self.0.as_deref().map_or(0, BTreeMap::len)
    }
    /// Remove all entries.
    pub fn clear(&mut self) {
        self.0 = None;
    }
    /// Find a borrowed stored value.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.as_deref()?.get(key)
    }
    /// Insert or replace a value, returning whether the key was absent.
    pub fn insert(&mut self, key: impl Into<K>, value: impl Into<V>) -> bool {
        self.ensure().insert(key.into(), value.into()).is_none()
    }
    /// Remove a key and return its only stored value.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.0.as_deref_mut()?.remove(key)
    }
    /// Iterate over unique entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.pairs()
    }
    /// Borrow keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.pairs().map(|(key, _)| key)
    }
    /// Borrow values in key order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.pairs().map(|(_, value)| value)
    }
    /// Borrow the actual ordered backing storage, when allocated.
    pub fn as_btree(&self) -> Option<&BTreeMap<K, V>> {
        self.0.as_deref()
    }
    /// Consume this map, returning its unique ordered entries.
    pub fn into_btree(self) -> BTreeMap<K, V> {
        self.0.map(|tree| *tree).unwrap_or_default()
    }
    /// Borrow the ordered entries.
    pub fn as_view(&self) -> TreeMapView<'_, K, V> {
        TreeMapView {
            inner: self.0.as_deref(),
        }
    }
    /// Mutably borrow the ordered entries.
    pub fn as_mut(&mut self) -> TreeMapMut<'_, K, V> {
        TreeMapMut {
            inner: self.ensure(),
        }
    }
}

impl<K: MapKey, V: MapValue> From<Map<K, V>> for TreeMap<K, V> {
    fn from(map: Map<K, V>) -> Self {
        let mut result = Self::default();
        for (key, value) in map.iter() {
            result.push_entry(key.clone(), value.clone());
        }
        result
    }
}
impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for TreeMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.pairs()).finish()
    }
}

/// Read-only ordered map view.
pub struct TreeMapView<'msg, K: MapKey, V: MapValue> {
    inner: Option<&'msg BTreeMap<K, V>>,
}
impl<K: MapKey, V: MapValue> Copy for TreeMapView<'_, K, V> {}
impl<K: MapKey, V: MapValue> Clone for TreeMapView<'_, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'msg, K: MapKey, V: MapValue> TreeMapView<'msg, K, V> {
    /// Construct an empty view.
    pub fn empty() -> Self {
        Self { inner: None }
    }
    /// Find a value by key.
    ///
    /// Custom queries use their owned-key conversion. String queries may allocate.
    pub fn get(self, key: impl MapQuery<K>) -> Option<crate::View<'msg, V>>
    where
        V: Proxied + 'static,
    {
        self.inner?.get(&key.to_owned_key()).map(AsView::as_view)
    }
    /// Number of distinct keys.
    pub fn len(self) -> usize {
        self.inner.map_or(0, BTreeMap::len)
    }
    /// Whether no entries are present.
    pub fn is_empty(self) -> bool {
        self.inner.is_none_or(BTreeMap::is_empty)
    }
    /// Iterate over projected key/value pairs in key order.
    pub fn iter(self) -> TreeMapViewIter<'msg, K, V>
    where
        K: Proxied + 'static,
        V: Proxied + 'static,
    {
        TreeMapViewIter {
            inner: self.inner.map(BTreeMap::iter),
        }
    }
    /// Iterate over projected keys in key order.
    pub fn keys(self) -> impl Iterator<Item = crate::View<'msg, K>> + 'msg
    where
        K: Proxied + 'static,
        V: Proxied + 'static,
    {
        self.iter().map(|(k, _)| k)
    }
    /// Iterate over projected values in key order.
    pub fn values(self) -> impl Iterator<Item = crate::View<'msg, V>> + 'msg
    where
        K: Proxied + 'static,
        V: Proxied + 'static,
    {
        self.iter().map(|(_, v)| v)
    }
}
impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for TreeMapView<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeMapView")
            .field("len", &self.len())
            .finish()
    }
}

/// Iterator over projected ordered map entries.
pub struct TreeMapViewIter<'msg, K: MapKey + Proxied, V: MapValue + Proxied> {
    inner: Option<btree_map::Iter<'msg, K, V>>,
}
impl<'msg, K: MapKey + Proxied, V: MapValue + Proxied> Iterator for TreeMapViewIter<'msg, K, V> {
    type Item = (crate::View<'msg, K>, crate::View<'msg, V>);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .as_mut()?
            .next()
            .map(|(k, v)| (AsView::as_view(k), AsView::as_view(v)))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner
            .as_ref()
            .map_or((0, Some(0)), Iterator::size_hint)
    }
}
impl<'msg, K: MapKey + Proxied + 'static, V: MapValue + Proxied + 'static> IntoIterator
    for TreeMapView<'msg, K, V>
{
    type Item = (crate::View<'msg, K>, crate::View<'msg, V>);
    type IntoIter = TreeMapViewIter<'msg, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Mutable ordered map view.
pub struct TreeMapMut<'msg, K: MapKey, V: MapValue> {
    inner: &'msg mut BTreeMap<K, V>,
}
impl<'msg, K: MapKey, V: MapValue> TreeMapMut<'msg, K, V> {
    /// Insert a value, returning whether the key was absent.
    pub fn insert(&mut self, key: impl MapQuery<K>, value: impl IntoProxied<V>) -> bool
    where
        V: 'static,
    {
        self.inner
            .insert(
                key.to_owned_key(),
                value.into_proxied(crate::internal::Private),
            )
            .is_none()
    }
    /// Find a value by key.
    pub fn get(&self, key: impl MapQuery<K>) -> Option<crate::View<'_, V>>
    where
        V: Proxied + 'static,
    {
        self.as_view().get(key)
    }
    /// Mutably borrow a message value by key.
    pub fn get_mut(&mut self, key: impl MapQuery<K>) -> Option<crate::Mut<'_, V>>
    where
        V: crate::message::Message + MutProxied + 'static,
    {
        self.inner.get_mut(&key.to_owned_key()).map(AsMut::as_mut)
    }
    /// Remove a key and return whether it was present.
    pub fn remove(&mut self, key: impl MapQuery<K>) -> bool {
        self.inner.remove(&key.to_owned_key()).is_some()
    }
    /// Remove all entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
    /// Number of distinct keys.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    /// Whether no entries are present.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    /// Borrow keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }
    /// Borrow values in key order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }
    /// Replace entries with a sequence of owned or convertible values.
    pub fn copy_from<'a>(
        &mut self,
        src: impl IntoIterator<Item = (impl MapQuery<K> + 'a, impl IntoProxied<V>)>,
    ) where
        V: Proxied + 'static,
    {
        self.clear();
        for (k, v) in src {
            self.insert(k, v);
        }
    }
    /// Clone a typed tree view, retaining its owned shared buffers.
    ///
    /// Unlike iterator conversion, this supports message views directly and
    /// clones `ProtoBytes` owners instead of copying borrowed byte slices.
    pub fn copy_from_view(&mut self, source: TreeMapView<'_, K, V>) {
        if let Some(tree) = source.inner {
            self.inner.clone_from(tree);
        } else {
            self.inner.clear();
        }
    }
    /// Return a default value for protobuf construction.
    pub fn default_value(&self) -> V
    where
        V: Default,
    {
        V::default()
    }
    /// Insert a protobuf key/value pair.
    pub fn proto_put<KI: MapQuery<K>, VI: IntoProxied<V>>(&mut self, pair: (KI, VI))
    where
        V: 'static,
    {
        self.insert(pair.0, pair.1);
    }
}
impl<K: MapKey + fmt::Debug, V: MapValue + fmt::Debug> fmt::Debug for TreeMapMut<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.inner.iter()).finish()
    }
}

impl<K: MapKey, V: MapValue> SealedInternal for TreeMap<K, V> {}
impl<K: MapKey, V: MapValue> Proxied for TreeMap<K, V> {
    type View<'msg> = TreeMapView<'msg, K, V>;
}
impl<K: MapKey, V: MapValue> MutProxied for TreeMap<K, V> {
    type Mut<'msg> = TreeMapMut<'msg, K, V>;
}
impl<K: MapKey, V: MapValue> AsView for TreeMap<K, V> {
    type Proxied = Self;
    fn as_view(&self) -> TreeMapView<'_, K, V> {
        TreeMap::as_view(self)
    }
}
impl<K: MapKey, V: MapValue> AsMut for TreeMap<K, V> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> TreeMapMut<'_, K, V> {
        TreeMap::as_mut(self)
    }
}
impl<K: MapKey, V: MapValue> SealedInternal for TreeMapView<'_, K, V> {}
impl<K: MapKey, V: MapValue> AsView for TreeMapView<'_, K, V> {
    type Proxied = TreeMap<K, V>;
    fn as_view(&self) -> TreeMapView<'_, K, V> {
        *self
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoView<'msg> for TreeMapView<'msg, K, V> {
    fn into_view<'shorter>(self) -> TreeMapView<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        TreeMapView { inner: self.inner }
    }
}
impl<K: MapKey, V: MapValue> SealedInternal for TreeMapMut<'_, K, V> {}
impl<K: MapKey, V: MapValue> AsView for TreeMapMut<'_, K, V> {
    type Proxied = TreeMap<K, V>;
    fn as_view(&self) -> TreeMapView<'_, K, V> {
        TreeMapView {
            inner: Some(self.inner),
        }
    }
}
impl<K: MapKey, V: MapValue> AsMut for TreeMapMut<'_, K, V> {
    type MutProxied = TreeMap<K, V>;
    fn as_mut(&mut self) -> TreeMapMut<'_, K, V> {
        TreeMapMut { inner: self.inner }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoView<'msg> for TreeMapMut<'msg, K, V> {
    fn into_view<'shorter>(self) -> TreeMapView<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        TreeMapView {
            inner: Some(self.inner),
        }
    }
}
impl<'msg, K: MapKey, V: MapValue> IntoMut<'msg> for TreeMapMut<'msg, K, V> {
    fn into_mut<'shorter>(self) -> TreeMapMut<'shorter, K, V>
    where
        'msg: 'shorter,
    {
        TreeMapMut { inner: self.inner }
    }
}

impl<K: MapKey, V: MapValue> From<TreeMapView<'_, K, V>> for TreeMap<K, V> {
    fn from(view: TreeMapView<'_, K, V>) -> Self {
        Self(view.inner.map(|tree| Box::new(tree.clone())))
    }
}
impl<K: MapKey, V: MapValue> IntoProxied<TreeMap<K, V>> for TreeMapView<'_, K, V> {
    fn into_proxied(self, _private: crate::internal::Private) -> TreeMap<K, V> {
        self.into()
    }
}
impl<K: MapKey, V: MapValue> IntoProxied<TreeMap<K, V>> for TreeMapMut<'_, K, V> {
    fn into_proxied(self, private: crate::internal::Private) -> TreeMap<K, V> {
        self.into_view().into_proxied(private)
    }
}

impl<K: MapKey, V: MapValue> From<BTreeMap<K, V>> for TreeMap<K, V> {
    fn from(map: BTreeMap<K, V>) -> Self {
        if map.is_empty() {
            Self::new()
        } else {
            Self(Some(Box::new(map)))
        }
    }
}
impl<K: MapKey, V: MapValue> FromIterator<(K, V)> for TreeMap<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(items: T) -> Self {
        let mut output = Self::new();
        for (key, value) in items {
            output.push_entry(key, value);
        }
        output
    }
}
impl<K: MapKey, V: MapValue> From<TreeMap<K, V>> for Map<K, V> {
    fn from(tree: TreeMap<K, V>) -> Self {
        let mut output = Self::new();
        for (key, value) in tree.into_btree() {
            output.push_entry(key, value);
        }
        output
    }
}
impl<K: MapKey> TreeMap<K, crate::ProtoBytes> {
    /// Retain the selected byte value's shared backing without copying its payload.
    pub fn get_shared(&self, key: impl MapQuery<K>) -> Option<crate::ProtoBytes> {
        self.get(&key.to_owned_key()).cloned()
    }
}
impl<K: MapKey + Proxied, V: MapValue + Proxied> ExactSizeIterator for TreeMapViewIter<'_, K, V> {}
impl<K: MapKey + Proxied, V: MapValue + Proxied> std::iter::FusedIterator
    for TreeMapViewIter<'_, K, V>
{
}

impl From<crate::lazy::LazyStrView<'_>> for crate::lazy::LazyStr {
    fn from(view: crate::lazy::LazyStrView<'_>) -> Self {
        Self::owned(crate::ProtoString::from(view.0))
    }
}
impl IntoProxied<crate::lazy::LazyStr> for crate::lazy::LazyStrView<'_> {
    fn into_proxied(self, _private: crate::internal::Private) -> crate::lazy::LazyStr {
        self.into()
    }
}
impl MapQuery<crate::lazy::LazyStr> for crate::lazy::LazyStrView<'_> {
    fn eq_key(&self, key: &crate::lazy::LazyStr) -> bool {
        self.0.as_bytes() == key.as_bytes()
    }
    fn to_owned_key(&self) -> crate::lazy::LazyStr {
        (*self).into()
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_unique_storage_replaces_and_erases_duplicates() {
        let mut tree: TreeMap<i32, i32> =
            [(3, 30), (1, 10), (3, 31), (2, 20)].into_iter().collect();
        assert_eq!(tree.len(), 3);
        assert_eq!(tree.as_btree().map(BTreeMap::len), Some(3));
        assert_eq!(
            tree.pairs().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
            [(1, 10), (2, 20), (3, 31)]
        );
        assert_eq!(tree.remove(&3), Some(31));
        assert_eq!(tree.get(&3), None);
        assert_eq!(tree.as_view().keys().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(tree.as_view().iter().len(), 2);
        assert_eq!(tree.clone(), tree);
        let legacy: Map<i32, i32> = tree.clone().into();
        assert_eq!(TreeMap::from(legacy), tree);
        let mut view = tree.as_mut();
        assert!(!view.insert(1, 11));
        assert!(view.insert(4, 40));
        assert!(view.remove(1));
        assert!(!view.remove(1));
        assert_eq!(view.get(4), Some(40));
        assert_eq!(view.keys().copied().collect::<Vec<_>>(), [2, 4]);
        view.copy_from([(8, 80), (7, 70), (8, 81)]);
        assert_eq!(
            view.as_view().iter().collect::<Vec<_>>(),
            [(7, 70), (8, 81)]
        );
        tree.clear();
        assert!(tree.is_empty());
        assert!(tree.as_btree().is_none());
    }

    #[test]
    fn empty_default_needs_no_key_or_value_default() {
        #[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
        struct Key(u8);
        impl MapKey for Key {}
        #[derive(Clone)]
        struct NoDefault;
        let tree = TreeMap::<Key, NoDefault>::default();
        assert_eq!(std::mem::size_of_val(&tree), std::mem::size_of::<usize>());
        assert_eq!(tree.len(), 0);
        assert_eq!(tree.as_view().len(), 0);
    }

    #[test]
    fn string_view_copy_preserves_invalid_utf8_bytes() {
        use crate::lazy::LazyStr;
        let mut source = TreeMap::new();
        source.push_entry(
            LazyStr::from_bytes(b"\xff\0"),
            LazyStr::from_bytes(b"\xfe\0"),
        );
        source.push_entry(LazyStr::from_bytes(b"\x80"), LazyStr::from_bytes(b"\x81"));
        let mut copy = TreeMap::new();
        copy.as_mut().copy_from(source.as_view());
        assert_eq!(copy, source);
        assert_eq!(
            copy.keys().map(LazyStr::as_bytes).collect::<Vec<_>>(),
            [b"\x80".as_slice(), b"\xff\0".as_slice()]
        );
        assert_eq!(TreeMap::from(source.as_view()), source);
    }

    #[test]
    fn borrowed_tree_conversion_retains_shared_bytes() {
        let input = bytes::Bytes::from(vec![7; 1024]);
        let mut source = TreeMap::new();
        source.push_entry(1i32, crate::ProtoBytes::from(input.clone()));
        let copied = TreeMap::from(source.as_view());
        let copied_mut: TreeMap<i32, crate::ProtoBytes> =
            source.as_mut().into_proxied(crate::internal::Private);
        let retained = copied.get_shared(1).unwrap();
        assert_eq!(retained.as_bytes().as_ptr(), input.as_ptr());
        assert_eq!(
            copied_mut.get_shared(1).unwrap().as_bytes().as_ptr(),
            input.as_ptr()
        );
        drop(input);
        drop(source);
        assert_eq!(retained.as_bytes(), vec![7; 1024]);
    }
}
