//! Map field storage and views.
use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use crate::string::ProtoString;
use std::fmt;
use std::sync::OnceLock;

/// Entry count at which lookups, unique length and iteration switch from a
/// linear scan to the sorted last-wins index. Below this, reads never
/// allocate an index; at and above it, one build is amortized over all reads.
/// Measured with the PK-17 scratch bench (cold-index `parse+touch`,
/// `get-each`, `iter` at 4..256 entries, exact alloc counts plus wall time):
/// sizes 4-8 are identical-or-better (get/len shed the old node allocations),
/// size 16 matches the scans on time, and 64+ wins strictly on every path.
const INDEX_THRESHOLD: usize = 16;

/// Sorted unique key -> last wire position. One `Vec` (no per-key node
/// allocation); lookups are binary searches. Built lazily on the first
/// large-map read, updated incrementally by `insert` and `remove`, dropped on
/// other mutations. Wire order and duplicates stay in `entries`, so encode order,
/// `pairs()` and last-wins semantics are unchanged.
struct LastWinsIndex<K> {
    pos: Vec<(K, usize)>,
}

type LastWins<K> = OnceLock<LastWinsIndex<K>>;

impl<K: MapKey> LastWinsIndex<K> {
    fn get(&self, key: &K) -> Option<usize> {
        self.pos
            .binary_search_by(|(k, _)| k.cmp(key))
            .ok()
            .map(|i| self.pos[i].1)
    }

    fn is_last(&self, key: &K, i: usize) -> bool {
        self.get(key) == Some(i)
    }

    fn remove_position<V>(&mut self, entries: &[(K, V)], key: &K, removed: usize) -> bool {
        let previous = entries[..removed]
            .iter()
            .rposition(|(stored, _)| stored == key);
        let mut removed_slot = None;
        for (slot, (_, position)) in self.pos.iter_mut().enumerate() {
            if *position == removed {
                removed_slot = Some(slot);
            } else if *position > removed {
                *position -= 1;
            }
        }
        let Some(slot) = removed_slot else {
            return false;
        };
        if let Some(previous) = previous {
            self.pos[slot].1 = previous;
        } else {
            self.pos.remove(slot);
        }
        true
    }
}

fn build_index<K: MapKey, V>(entries: &[(K, V)]) -> LastWinsIndex<K> {
    let mut pos: Vec<(K, usize)> = entries
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i))
        .collect();
    // Key ascending, position descending: `dedup_by` keeps the first of each
    // run, which is the last wire position per key. Single allocation.
    pos.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    pos.dedup_by(|a, b| a.0 == b.0);
    LastWinsIndex { pos }
}

/// Returns the last-wins index when the map is large enough to amortize it.
/// Small maps always scan, so they never pay for an index build.
fn hot_index<'a, K: MapKey, V>(
    entries: &[(K, V)],
    index: Option<&'a LastWins<K>>,
) -> Option<&'a LastWinsIndex<K>> {
    if entries.len() < INDEX_THRESHOLD {
        return None;
    }
    Some(index?.get_or_init(|| build_index(entries)))
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
    match hot_index(entries, index) {
        Some(idx) => idx.pos.len(),
        None => scan_unique_len(entries),
    }
}

fn last_wins_pos<K: MapKey, V>(
    entries: &[(K, V)],
    index: Option<&LastWins<K>>,
    key: &K,
) -> Option<usize> {
    if let Some(idx) = hot_index(entries, index) {
        return idx.get(key);
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

fn remove_entry_at<K: MapKey, V>(
    entries: &mut Vec<(K, V)>,
    mut index: Option<&mut LastWins<K>>,
    position: usize,
) -> V {
    // Keep the cache absent until repair completes. A user key's Eq or Drop
    // may panic after entries change; a caught panic must not expose stale
    // positions. No index is built on a mutation path.
    let cached = index.as_deref_mut().and_then(OnceLock::take);
    // Stable removal preserves the winner of every unrelated duplicate key.
    // Keep the removed tuple alive through repair, including unwinding.
    let removed = entries.remove(position);
    if let Some(mut cached) = cached {
        if entries.len() >= INDEX_THRESHOLD && cached.remove_position(entries, &removed.0, position)
        {
            if let Some(index) = index {
                *index = OnceLock::from(cached);
            }
        }
    }
    // Drop the key before moving the value into the return place: if key
    // destruction panics, the local value must still be dropped on unwind.
    let (key, value) = removed;
    drop(key);
    value
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

// Keep the indexed comparison out of the small-map scan's generated code.
// Both indexes already exist after equality compares the unique lengths.
#[inline(never)]
fn indexed_entries_eq<K: MapKey, V: MapValue + PartialEq>(
    left: &MapInner<K, V>,
    right: &MapInner<K, V>,
    left_index: &LastWinsIndex<K>,
    right_index: &LastWinsIndex<K>,
) -> bool {
    left_index.pos.iter().zip(&right_index.pos).all(
        |((left_key, left_pos), (right_key, right_pos))| {
            if left_key != right_key {
                return false;
            }
            let Some((_, left_value)) = left.entries.get(*left_pos) else {
                return false;
            };
            let Some((_, right_value)) = right.entries.get(*right_pos) else {
                return false;
            };
            // Keep the original value comparison direction.
            right_value == left_value
        },
    )
}

impl<K: MapKey, V: MapValue + PartialEq> PartialEq for Map<K, V> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        if let (Some(left), Some(right)) = (self.0.as_deref(), other.0.as_deref()) {
            if left.entries.len() >= INDEX_THRESHOLD && right.entries.len() >= INDEX_THRESHOLD {
                if let (Some(left_index), Some(right_index)) = (left.index.get(), right.index.get())
                {
                    // Unique sorted positions pair corresponding last-wins
                    // values without a binary search for every wire entry.
                    return indexed_entries_eq(left, right, left_index, right_index);
                }
            }
        }
        // Iteration yields each last-wins key once. Equal unique lengths and
        // matching every left entry also prove that the right has no extras.
        self.iter().all(|(k, v)| other.get(k) == Some(v))
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
                enum_codec: None,
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
            enum_codec: None,
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
        // Reuse a built index, but never build one on a mutation path: a
        // build here would be discarded by the mutation itself.
        let existing = match inner.index.get() {
            Some(idx) => idx.get(&key),
            None => inner.entries.iter().rposition(|(k, _)| k == &key),
        };
        if let Some(i) = existing {
            if let Some(e) = inner.entries.get_mut(i) {
                e.1 = value;
                return false;
            }
        }
        let pos = inner.entries.len();
        if let Some(idx) = inner.index.get_mut() {
            debug_assert!(idx.get(&key).is_none(), "hot index disagrees with lookup");
            let at = idx.pos.partition_point(|(k, _)| k < &key);
            idx.pos.insert(at, (key.clone(), pos));
        }
        inner.entries.push((key, value));
        true
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let inner = self.0.as_ref()?;
        last_wins_get(&inner.entries, Some(&inner.index), key)
    }

    /// Remove the last stored entry with this key and return its value.
    ///
    /// An earlier parsed duplicate can become visible. Other entries keep
    /// their relative order. Repeated removals erase every stored version.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let inner = self.0.as_mut()?;
        // Remove the last occurrence while preserving unrelated wire order.
        // Reuse an existing index without building one on mutation.
        let i = match inner.index.get() {
            Some(idx) => idx.get(key),
            None => inner.entries.iter().rposition(|(k, _)| k == key),
        }?;
        let out = remove_entry_at(&mut inner.entries, Some(&mut inner.index), i);
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
        match self.0.as_deref() {
            Some(inner) => {
                let items = inner.entries.as_slice();
                MapIter {
                    items,
                    index: hot_index(items, Some(&inner.index)),
                    i: 0,
                }
            }
            None => MapIter {
                items: &[],
                index: None,
                i: 0,
            },
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
    index: Option<&'msg LastWinsIndex<K>>,
    i: usize,
}

impl<'msg, K: MapKey, V> Iterator for MapIter<'msg, K, V> {
    type Item = (&'msg K, &'msg V);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Indexed path yields the same last-occurrence order as the scan:
        // position `i` is yielded exactly when no later duplicate exists.
        if let Some(idx) = self.index {
            // The index has one entry per unique key. Equal cardinalities
            // prove that no wire entry was shadowed, so iteration is just a
            // borrowed slice walk, without a key search for every element.
            if idx.pos.len() == self.items.len() {
                let (k, v) = self.items.get(self.i)?;
                self.i += 1;
                return Some((k, v));
            }
            while self.i < self.items.len() {
                let (k, v) = &self.items[self.i];
                let i = self.i;
                self.i += 1;
                if idx.is_last(k, i) {
                    return Some((k, v));
                }
            }
            return None;
        }
        while self.i < self.items.len() {
            let (k, v) = &self.items[self.i];
            self.i += 1;
            if self.items[self.i..].iter().all(|(k2, _)| k2 != k) {
                return Some((k, v));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.items.len().saturating_sub(self.i);
        // Cardinality is already available in the shared index. Do not scan
        // keys just to reserve output: unindexed or duplicate-containing maps
        // retain conservative bounds.
        let unique = self.items.len() <= 1
            || self
                .index
                .is_some_and(|index| index.pos.len() == self.items.len());
        (if unique { remaining } else { 0 }, Some(remaining))
    }
}

pub struct MapView<'msg, K: MapKey, V: MapValue> {
    inner: Option<&'msg [(K, V)]>,
    index: Option<&'msg LastWins<K>>,
    raw: Option<crate::runtime::RawMap>,
    enum_codec: Option<crate::runtime::KernelEnumCodec<V>>,
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
            enum_codec: None,
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
                enum_codec: None,
            }
        }
    }

    #[doc(hidden)]
    /// # Safety
    /// `raw` must point to a live map with keys `K` and values `V`, owned by an
    /// arena that outlives `'msg`. No mutation may invalidate its borrowed data
    /// while this view or any view derived from it is live.
    pub unsafe fn from_raw(_private: crate::internal::Private, raw: crate::runtime::RawMap) -> Self
    where
        V: crate::internal::EntityType,
        V::Tag: crate::runtime::KernelCollectionValue<V>,
    {
        Self {
            inner: None,
            index: None,
            raw: Some(raw),
            enum_codec: <V::Tag as crate::runtime::KernelCollectionValue<V>>::enum_codec(),
        }
    }

    #[doc(hidden)]
    /// # Safety
    /// The same typed storage and lifetime requirements as [`Self::from_raw`]
    /// apply.
    pub unsafe fn from_raw_ptr(raw: crate::runtime::RawMap) -> Self
    where
        V: crate::internal::EntityType,
        V::Tag: crate::runtime::KernelCollectionValue<V>,
    {
        // SAFETY: the caller supplies the same typed map/lifetime contract as
        // from_raw; this alias only supplies its internal marker.
        unsafe { Self::from_raw(crate::internal::Private, raw) }
    }

    pub fn get(self, key: impl MapQuery<K>) -> Option<crate::proxied::View<'msg, V>>
    where
        V: crate::proxied::Proxied + 'static,
    {
        if let Some(raw) = self.raw {
            let kb = key.key_bytes();
            return unsafe { crate::runtime::kernel_map_get_bytes::<V>(raw, &kb, self.enum_codec) };
        }
        let entries = self.inner?;
        if let Some(index) = hot_index(entries, self.index) {
            let owned = key.to_owned_key();
            return index
                .get(&owned)
                .and_then(|pos| entries.get(pos))
                .map(|(_, value)| crate::proxied::AsView::as_view(value));
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

    #[inline]
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
    enum_codec: Option<crate::runtime::KernelEnumCodec<V>>,
    arena: Option<&'msg crate::runtime::Arena>,
}

impl<'msg, K: MapKey, V: MapValue> MapMut<'msg, K, V> {
    pub fn from_vec(inner: &'msg mut Vec<(K, V)>) -> Self {
        Self {
            inner: Some(inner),
            index: None,
            raw: None,
            enum_codec: None,
            arena: None,
        }
    }

    #[doc(hidden)]
    /// # Safety
    /// `inner.raw` must be a live map with keys `K` and values `V`, owned by
    /// `inner.arena` for `'msg` and exclusively borrowed by this mutator.
    pub unsafe fn from_inner(
        _private: crate::internal::Private,
        inner: crate::runtime::InnerMapMut<'msg>,
    ) -> Self
    where
        V: crate::internal::EntityType,
        V::Tag: crate::runtime::KernelCollectionValue<V>,
    {
        Self {
            inner: None,
            index: None,
            raw: Some(inner.raw),
            enum_codec: <V::Tag as crate::runtime::KernelCollectionValue<V>>::enum_codec(),
            arena: Some(inner.arena),
        }
    }

    #[doc(hidden)]
    /// # Safety
    /// `raw` must be a live arena-owned map with keys `K` and values `V`.
    /// Its arena must outlive `'msg`; the map must be exclusively borrowed by
    /// this mutator, just as for [`Self::from_inner`].
    pub unsafe fn from_raw_inner(raw: crate::runtime::RawMap) -> Self
    where
        V: crate::internal::EntityType,
        V::Tag: crate::runtime::KernelCollectionValue<V>,
    {
        Self {
            inner: None,
            index: None,
            raw: Some(raw),
            enum_codec: <V::Tag as crate::runtime::KernelCollectionValue<V>>::enum_codec(),
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
            // Reuse a built index, but never build one on a mutation path.
            let existing = match self.index.as_deref().and_then(|l| l.get()) {
                Some(idx) => idx.get(&key),
                None => v.iter().rposition(|(k, _)| k == &key),
            };
            if let Some(i) = existing {
                if let Some(e) = v.get_mut(i) {
                    e.1 = value;
                    return false;
                }
            }
            let pos = v.len();
            if let Some(idx) = self.index.as_deref_mut().and_then(|l| l.get_mut()) {
                debug_assert!(idx.get(&key).is_none(), "hot index disagrees with lookup");
                let at = idx.pos.partition_point(|(k, _)| k < &key);
                idx.pos.insert(at, (key.clone(), pos));
            }
            v.push((key, value));
            true
        } else if let Some(raw) = self.raw {
            crate::runtime::kernel_map_insert(raw, key, value, self.arena, self.enum_codec)
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

    /// Remove the last stored entry with this key, returning whether it existed.
    ///
    /// An earlier parsed duplicate can become visible. Other entries keep
    /// their relative order. Repeated removals erase every stored version.
    pub fn remove(&mut self, key: impl MapQuery<K>) -> bool {
        let key = key.to_owned_key();
        if let Some(v) = self.inner.as_mut() {
            // Preserve every unrelated duplicate's order and reuse a built
            // index without creating one on a mutation path.
            let found = match self.index.as_deref().and_then(|l| l.get()) {
                Some(idx) => idx.get(&key),
                None => v.iter().rposition(|(k, _)| k == &key),
            };
            if let Some(i) = found {
                remove_entry_at(v, self.index.as_deref_mut(), i);
                return true;
            }
            false
        } else if let Some(raw) = self.raw {
            unsafe {
                let kb = crate::runtime::kernel_key_bytes(key.clone());
                let mut entries = (*raw).entries.borrow_mut();
                if let Some(i) = entries.iter().rposition(|(k, _)| k == &kb) {
                    let (_, old) = entries.remove(i);
                    drop(entries);
                    crate::runtime::kernel_map_release_value(raw, old);
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
            enum_codec: self.enum_codec,
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
            enum_codec: self.enum_codec,
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
            enum_codec: self.enum_codec,
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
            enum_codec: self.enum_codec,
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
            enum_codec: self.enum_codec,
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

/// Iterator over map views in last-occurrence wire order.
///
/// Typed entries are borrowed and projected when yielded. Their size hint is
/// exact when the shared index proves every key unique, and conservative when
/// duplicate keys may be filtered during iteration.
pub struct MapViewIter<
    'msg,
    K: MapKey + crate::proxied::Proxied,
    V: MapValue + crate::proxied::Proxied,
> {
    storage: MapViewIterStorage<'msg, K, V>,
}

enum MapViewIterStorage<
    'msg,
    K: MapKey + crate::proxied::Proxied,
    V: MapValue + crate::proxied::Proxied,
> {
    // Typed views already borrow stable entries. Project values only when
    // yielded, without allocating a second collection of those same views.
    Typed(MapIter<'msg, K, V>),
    // Arena storage keeps its established snapshot and lifetime handling.
    Raw {
        items: Vec<Option<(crate::proxied::View<'msg, K>, crate::proxied::View<'msg, V>)>>,
        i: usize,
    },
}

impl<
    'msg,
    K: MapKey + crate::proxied::Proxied + 'static,
    V: MapValue + crate::proxied::Proxied + 'static,
> MapViewIter<'msg, K, V>
{
    #[inline]
    fn from_view(view: MapView<'msg, K, V>) -> Self {
        if let Some(raw) = view.raw {
            return Self::from_raw_view(view, raw);
        }
        let items = view.inner.unwrap_or(&[]);
        Self {
            storage: MapViewIterStorage::Typed(MapIter {
                items,
                index: if items.is_empty() {
                    None
                } else {
                    hot_index(items, view.index)
                },
                i: 0,
            }),
        }
    }

    // Keep the raw projection loop out of the small borrowed constructor.
    #[inline(never)]
    fn from_raw_view(view: MapView<'msg, K, V>, raw: crate::runtime::RawMap) -> Self {
        let mut items = Vec::new();
        unsafe {
            let entries = (*raw).entries.borrow();
            for (i, (kb, fk)) in entries.iter().enumerate() {
                if entries[i + 1..].iter().any(|(k2, _)| k2 == kb) {
                    continue;
                }
                // SAFETY: raw map views borrow their arena-owned keys for 'msg;
                // safe callers cannot remove a key while a yielded view is live.
                let key_bytes: &'msg [u8] = std::slice::from_raw_parts(kb.as_ptr(), kb.len());
                let Some(k) = crate::runtime::kernel_bytes_to_view::<K>(key_bytes) else {
                    continue;
                };
                let Some(v) = crate::runtime::kernel_collection_value_to_view::<'msg, V>(
                    *fk,
                    view.enum_codec,
                ) else {
                    continue;
                };
                items.push(Some((k, v)));
            }
        }
        Self {
            storage: MapViewIterStorage::Raw { items, i: 0 },
        }
    }
}

impl<'msg, K, V> Iterator for MapViewIter<'msg, K, V>
where
    K: MapKey + crate::proxied::Proxied,
    V: MapValue + crate::proxied::Proxied,
{
    type Item = (crate::proxied::View<'msg, K>, crate::proxied::View<'msg, V>);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.storage {
            MapViewIterStorage::Typed(iter) => iter.next().map(|(k, v)| {
                (
                    crate::proxied::AsView::as_view(k),
                    crate::proxied::AsView::as_view(v),
                )
            }),
            MapViewIterStorage::Raw { items, i } => {
                let item = items.get_mut(*i)?.take();
                *i += 1;
                item
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.storage {
            MapViewIterStorage::Typed(iter) => iter.size_hint(),
            MapViewIterStorage::Raw { items, i } => {
                let remaining = items.len().saturating_sub(*i);
                (remaining, Some(remaining))
            }
        }
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
    #[inline]
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

    fn unrelated_duplicate_fixture(n: usize) -> Vec<(i32, i32)> {
        let mut entries: Vec<_> = (0..n - 4).map(|i| (100 + i as i32, i as i32)).collect();
        entries.extend([(2, 20), (1, 10), (2, 21), (2, 22)]);
        entries
    }

    #[test]
    fn owned_remove_keeps_unrelated_duplicate_winner() {
        let mut failures = Vec::new();
        for n in [4, 15, 16, 17, 64] {
            for warm in [false, true] {
                let mut map = Map::new();
                for (key, value) in unrelated_duplicate_fixture(n) {
                    map.push_entry(key, value);
                }
                assert_eq!(MapView::from_slice(map.pairs()).get(2), Some(22));
                if warm {
                    assert_eq!(map.get(&2), Some(&22));
                }
                assert_eq!(map.remove(&1), Some(10));
                let actual = map.get(&2).copied();
                if actual != Some(22) {
                    failures.push((n, warm, actual));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "unrelated winners changed: {failures:?}"
        );
    }

    #[test]
    fn typed_mut_remove_keeps_unrelated_duplicate_winner() {
        let mut failures = Vec::new();
        for n in [4, 15, 16, 17, 64] {
            for warm in [false, true] {
                let mut map = Map::new();
                for (key, value) in unrelated_duplicate_fixture(n) {
                    map.push_entry(key, value);
                }
                assert_eq!(MapView::from_slice(map.pairs()).get(2), Some(22));
                if warm {
                    assert_eq!(map.get(&2), Some(&22));
                }
                assert!(map.as_mut().remove(1));
                let actual = map.as_view().get(2);
                if actual != Some(22) {
                    failures.push((n, warm, actual));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "unrelated winners changed: {failures:?}"
        );
    }

    #[test]
    fn raw_mut_remove_keeps_unrelated_duplicate_winner() {
        let mut failures = Vec::new();
        for n in [4, 15, 16, 17, 64] {
            for warm in [false, true] {
                let arena = crate::runtime::Arena::new();
                let raw = arena.alloc_map();
                // SAFETY: the arena owns the map; these entries have i32 key/value types.
                unsafe {
                    (*raw).entries.borrow_mut().extend(
                        unrelated_duplicate_fixture(n)
                            .into_iter()
                            .map(|(key, value)| {
                                (
                                    key.to_le_bytes().to_vec(),
                                    crate::runtime::FieldKind::I32(value),
                                )
                            }),
                    );
                }
                // SAFETY: the arena outlives this exclusive mutator with matching types.
                let mut map: MapMut<'_, i32, i32> = unsafe {
                    MapMut::from_inner(
                        crate::internal::Private,
                        crate::runtime::InnerMapMut::new(raw, &arena),
                    )
                };
                if warm {
                    assert_eq!(map.get(2), Some(22));
                }
                assert!(map.remove(1));
                let actual = map.get(2);
                if actual != Some(22) {
                    failures.push((n, warm, actual));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "unrelated winners changed: {failures:?}"
        );
    }

    #[test]
    fn removing_target_keeps_historical_pop_duplicate_behavior() {
        let mut map = Map::new();
        for (key, value) in [(1, 10), (1, 11), (2, 20)] {
            map.push_entry(key, value);
        }
        assert_eq!(map.remove(&1), Some(11));
        assert_eq!(map.get(&1), Some(&10));
        assert!(map.as_mut().remove(1));
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&20));
    }

    #[test]
    fn removal_matches_stable_pop_model_across_backends() {
        fn model_get(entries: &[(i32, i32)], key: i32) -> Option<i32> {
            entries
                .iter()
                .rev()
                .find(|(stored, _)| *stored == key)
                .map(|(_, value)| *value)
        }
        fn logical(entries: &[(i32, i32)]) -> Vec<(i32, i32)> {
            entries
                .iter()
                .enumerate()
                .filter(|(i, (key, _))| entries[i + 1..].iter().all(|(later, _)| later != key))
                .map(|(_, entry)| *entry)
                .collect()
        }
        for seed in 0..12 {
            let mut state = seed + 1u64;
            let mut model = Vec::new();
            let mut owned = Map::new();
            let mut typed = Map::new();
            let arena = crate::runtime::Arena::new();
            let raw = arena.alloc_map();
            for step in 0..256 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let key = ((state >> 8) % 19) as i32 - 9;
                let value = step + seed as i32 * 1000;
                match state % 16 {
                    0..=5 => {
                        model.push((key, value));
                        owned.push_entry(key, value);
                        typed.push_entry(key, value);
                        // SAFETY: no view or mutator is live; arena owns matching i32 entries.
                        unsafe {
                            (*raw).entries.borrow_mut().push((
                                key.to_le_bytes().to_vec(),
                                crate::runtime::FieldKind::I32(value),
                            ));
                        }
                    }
                    6..=10 => {
                        let before = model.iter().rposition(|(stored, _)| *stored == key);
                        let expected = before.map(|position| model.remove(position).1);
                        assert_eq!(owned.remove(&key), expected, "seed={seed} step={step}");
                        assert_eq!(typed.as_mut().remove(key), expected.is_some());
                        // SAFETY: exclusive matching map mutator is scoped to this operation.
                        let mut raw_mut: MapMut<'_, i32, i32> = unsafe {
                            MapMut::from_inner(
                                crate::internal::Private,
                                crate::runtime::InnerMapMut::new(raw, &arena),
                            )
                        };
                        assert_eq!(raw_mut.remove(key), expected.is_some());
                    }
                    11..=14 => {
                        let fresh = if let Some(position) =
                            model.iter().rposition(|(stored, _)| *stored == key)
                        {
                            model[position].1 = value;
                            false
                        } else {
                            model.push((key, value));
                            true
                        };
                        assert_eq!(owned.insert(key, value), fresh);
                        assert_eq!(typed.as_mut().insert(key, value), fresh);
                        // SAFETY: exclusive matching map mutator is scoped to this operation.
                        let mut raw_mut: MapMut<'_, i32, i32> = unsafe {
                            MapMut::from_inner(
                                crate::internal::Private,
                                crate::runtime::InnerMapMut::new(raw, &arena),
                            )
                        };
                        assert_eq!(raw_mut.insert(key, value), fresh);
                    }
                    _ => {
                        model.clear();
                        owned.clear();
                        typed.as_mut().clear();
                        // SAFETY: exclusive matching map mutator is scoped to this operation.
                        let mut raw_mut: MapMut<'_, i32, i32> = unsafe {
                            MapMut::from_inner(
                                crate::internal::Private,
                                crate::runtime::InnerMapMut::new(raw, &arena),
                            )
                        };
                        raw_mut.clear();
                    }
                }
                let expected = logical(&model);
                assert_eq!(owned.pairs(), model.as_slice(), "seed={seed} step={step}");
                assert_eq!(typed.pairs(), model.as_slice(), "seed={seed} step={step}");
                assert_eq!(
                    owned.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(typed.as_view().iter().collect::<Vec<_>>(), expected);
                // SAFETY: this immutable view is dropped before the next mutation.
                let raw_view: MapView<'_, i32, i32> = unsafe { MapView::from_raw_ptr(raw) };
                assert_eq!(raw_view.iter().collect::<Vec<_>>(), expected);
                assert_eq!(owned.len(), expected.len());
                assert_eq!(typed.len(), expected.len());
                assert_eq!(raw_view.len(), expected.len());
                for query in -10..=10 {
                    let expected = model_get(&model, query);
                    assert_eq!(owned.get(&query).copied(), expected);
                    assert_eq!(typed.as_view().get(query), expected);
                    assert_eq!(raw_view.get(query), expected);
                }
            }
        }
    }

    #[test]
    fn removal_preserves_cache_threshold_and_capacity_policy() {
        for n in [1, 4, 15, 16, 17, 64] {
            for warm in [false, true] {
                for through_mut in [false, true] {
                    let mut map = Map::new();
                    for i in 0..n {
                        map.push_entry(i, i);
                    }
                    if warm {
                        assert_eq!(map.get(&0), Some(&0));
                    }
                    let capacity = map.0.as_ref().map(|inner| inner.entries.capacity());
                    if through_mut {
                        assert!(map.as_mut().remove(0));
                    } else {
                        assert_eq!(map.remove(&0), Some(0));
                    }
                    if n == 1 && !through_mut {
                        assert!(map.0.is_none());
                    } else {
                        let inner = map.0.as_ref();
                        assert_eq!(inner.map(|inner| inner.entries.capacity()), capacity);
                        assert_eq!(
                            inner.is_some_and(|inner| inner.index.get().is_some()),
                            warm && n > INDEX_THRESHOLD as i32
                        );
                    }
                    // Clear keeps typed entry capacity and drops its index; owned clear frees it.
                    if through_mut {
                        map.as_mut().clear();
                        assert_eq!(
                            map.0.as_ref().map(|inner| inner.entries.capacity()),
                            capacity
                        );
                        assert!(
                            map.0
                                .as_ref()
                                .is_some_and(|inner| inner.index.get().is_none())
                        );
                    } else {
                        map.clear();
                        assert!(map.0.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn raw_removal_releases_only_removed_bytes_and_keeps_survivors_live() {
        let arena = crate::runtime::Arena::new();
        let raw = arena.alloc_map();
        // SAFETY: the arena owns both matching raw key/value storage and each retained value.
        unsafe {
            for (key, value) in [(2i32, 20u8), (1, 10), (2, 21), (2, 22)] {
                let pointer = crate::runtime::retain_bytes(&(*raw).strs, vec![value; 96]);
                (*raw).entries.borrow_mut().push((
                    key.to_le_bytes().to_vec(),
                    crate::runtime::FieldKind::Bytes(pointer),
                ));
            }
        }
        // SAFETY: this exclusive mutator has matching i32/ProtoBytes entries in a live arena.
        let mut map: MapMut<'_, i32, crate::string::ProtoBytes> = unsafe {
            MapMut::from_inner(
                crate::internal::Private,
                crate::runtime::InnerMapMut::new(raw, &arena),
            )
        };
        assert!(map.remove(1));
        assert_eq!(map.get(2).map(|value| value.to_vec()), Some(vec![22; 96]));
        // SAFETY: storage is inspected without mutation while the arena and mutator are live.
        assert_eq!(unsafe { (*raw).strs.borrow().len() }, 3);
        assert!(map.remove(2));
        assert_eq!(map.get(2).map(|value| value.to_vec()), Some(vec![21; 96]));
        // SAFETY: same live owner, after the preceding field view was dropped.
        assert_eq!(unsafe { (*raw).strs.borrow().len() }, 2);
        assert!(map.remove(2));
        assert_eq!(map.get(2).map(|value| value.to_vec()), Some(vec![20; 96]));
        assert!(map.remove(2));
        assert_eq!(map.get(2), None);
        assert!(!map.remove(2));
        // SAFETY: no retained entry remains; each removed allocation was released once.
        assert!(unsafe { (*raw).strs.borrow().is_empty() });
        assert!(map.insert(3, crate::string::ProtoBytes::from(vec![7; 128])));
        assert_eq!(map.get(3).map(<[u8]>::len), Some(128));
        map.clear();
        // SAFETY: clear has released all raw bytes storage while the arena remains live.
        assert!(unsafe { (*raw).strs.borrow().is_empty() });
    }

    #[derive(Debug, Default)]
    struct KeyFaults {
        ord: std::cell::Cell<bool>,
        eq: std::cell::Cell<bool>,
        drop_key: std::cell::Cell<Option<i32>>,
        clones: std::cell::Cell<usize>,
    }
    #[derive(Debug)]
    struct FaultKey(i32, std::rc::Rc<KeyFaults>);
    impl Clone for FaultKey {
        fn clone(&self) -> Self {
            self.1.clones.set(self.1.clones.get() + 1);
            Self(self.0, self.1.clone())
        }
    }
    impl PartialEq for FaultKey {
        fn eq(&self, other: &Self) -> bool {
            assert!(!self.1.eq.replace(false), "armed Eq panic");
            self.0 == other.0
        }
    }
    impl Eq for FaultKey {}
    impl PartialOrd for FaultKey {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for FaultKey {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            assert!(!self.1.ord.replace(false), "armed Ord panic");
            self.0.cmp(&other.0)
        }
    }
    impl Drop for FaultKey {
        fn drop(&mut self) {
            let armed = self.1.drop_key.get() == Some(self.0);
            if armed {
                self.1.drop_key.set(None);
            }
            assert!(!armed, "armed key Drop panic");
        }
    }
    impl MapKey for FaultKey {}
    impl MapQuery<FaultKey> for &FaultKey {
        fn eq_key(&self, key: &FaultKey) -> bool {
            *self == key
        }
        fn to_owned_key(&self) -> FaultKey {
            (*self).clone()
        }
        fn key_bytes(&self) -> Vec<u8> {
            self.0.to_le_bytes().to_vec()
        }
    }
    #[derive(Debug, Default)]
    struct ValueFaults {
        drops: std::cell::RefCell<Vec<i32>>,
        drop_value: std::cell::Cell<Option<i32>>,
    }
    #[derive(Clone, Debug)]
    struct FaultValue(i32, std::rc::Rc<ValueFaults>);
    impl Drop for FaultValue {
        fn drop(&mut self) {
            self.1.drops.borrow_mut().push(self.0);
            let armed = self.1.drop_value.get() == Some(self.0);
            if armed {
                self.1.drop_value.set(None);
            }
            assert!(!armed, "armed value Drop panic");
        }
    }
    fn fault_map(
        duplicate: bool,
    ) -> (
        Map<FaultKey, FaultValue>,
        std::rc::Rc<KeyFaults>,
        std::rc::Rc<ValueFaults>,
    ) {
        let keys = std::rc::Rc::new(KeyFaults::default());
        let values = std::rc::Rc::new(ValueFaults::default());
        let mut map = Map::new();
        for id in 0..20 {
            map.push_entry(FaultKey(id, keys.clone()), FaultValue(id, values.clone()));
        }
        if duplicate {
            map.push_entry(FaultKey(1, keys.clone()), FaultValue(20, values.clone()));
        }
        assert_eq!(map.len(), 20); // Warm the index with all fault flags disabled.
        (map, keys, values)
    }
    fn assert_value_drops_once(values: &ValueFaults, n: i32) {
        let mut drops = values.drops.borrow().clone();
        drops.sort_unstable();
        assert_eq!(drops, (0..n).collect::<Vec<_>>());
    }

    #[test]
    fn removal_panic_guards_prevent_stale_visible_indexes() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        // Ord runs during lookup, before mutation; the existing cache stays valid.
        let (mut map, keys, values) = fault_map(false);
        let query = FaultKey(1, keys.clone());
        keys.ord.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| map.remove(&query))).is_err());
        assert_eq!(map.pairs().len(), 20);
        assert!(
            map.0
                .as_ref()
                .is_some_and(|inner| inner.index.get().is_some())
        );
        assert!(values.drops.borrow().is_empty());
        assert_eq!(map.get(&query).map(|value| value.0), Some(1));
        drop(map);
        assert_value_drops_once(&values, 20);

        // Eq can panic during repair after stable removal; cache must be absent.
        let (mut map, keys, values) = fault_map(false);
        let query = FaultKey(1, keys.clone());
        keys.eq.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| map.remove(&query))).is_err());
        assert_eq!(map.pairs().len(), 19);
        assert!(
            map.0
                .as_ref()
                .is_some_and(|inner| inner.index.get().is_none())
        );
        assert_eq!(*values.drops.borrow(), [1]);
        assert_eq!(map.get(&query).map(|value| value.0), None);
        assert_eq!(
            map.get(&FaultKey(2, keys.clone())).map(|value| value.0),
            Some(2)
        );
        drop(map);
        assert_value_drops_once(&values, 20);

        for duplicate in [false, true] {
            let (mut map, keys, values) = fault_map(duplicate);
            let query = FaultKey(1, keys.clone());
            keys.drop_key.set(Some(1));
            assert!(catch_unwind(AssertUnwindSafe(|| map.remove(&query))).is_err());
            assert_eq!(map.pairs().len(), if duplicate { 20 } else { 19 });
            // Removing a unique index key can panic before restore. With a previous
            // version, the removed entry key drops only after complete restoration.
            assert_eq!(
                map.0
                    .as_ref()
                    .is_some_and(|inner| inner.index.get().is_some()),
                duplicate
            );
            assert_eq!(*values.drops.borrow(), [if duplicate { 20 } else { 1 }]);
            assert_eq!(map.get(&query).map(|value| value.0), duplicate.then_some(1));
            assert_eq!(
                map.get(&FaultKey(2, keys.clone())).map(|value| value.0),
                Some(2)
            );
            drop(map);
            assert_value_drops_once(&values, if duplicate { 21 } else { 20 });
        }

        // A typed mutator drops its removed value after cache repair.
        let (mut map, keys, values) = fault_map(false);
        let query = FaultKey(1, keys.clone());
        values.drop_value.set(Some(1));
        assert!(catch_unwind(AssertUnwindSafe(|| map.as_mut().remove(&query))).is_err());
        assert_eq!(map.pairs().len(), 19);
        assert!(
            map.0
                .as_ref()
                .is_some_and(|inner| inner.index.get().is_some())
        );
        assert_eq!(*values.drops.borrow(), [1]);
        assert_eq!(map.get(&query).map(|value| value.0), None);
        drop(map);
        assert_value_drops_once(&values, 20);
    }

    #[test]
    fn warm_removal_reuses_index_without_key_clones() {
        let (mut map, keys, values) = fault_map(false);
        keys.clones.set(0);
        let before = map
            .0
            .as_ref()
            .and_then(|inner| inner.index.get())
            .map(|index| index.pos.as_ptr());
        let removed = map.remove(&FaultKey(7, keys.clone()));
        assert_eq!(removed.as_ref().map(|value| value.0), Some(7));
        assert_eq!(
            map.0
                .as_ref()
                .and_then(|inner| inner.index.get())
                .map(|index| index.pos.as_ptr()),
            before
        );
        for key in 0..20 {
            assert_eq!(
                map.get(&FaultKey(key, keys.clone())).map(|value| value.0),
                (key != 7).then_some(key)
            );
        }
        assert_eq!(keys.clones.get(), 0);
        drop(removed);
        drop(map);
        assert_value_drops_once(&values, 20);
    }

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
    fn equality_matches_hash_map_last_wins() {
        use std::collections::HashMap;
        // Include both sides of the lazy-index boundary and maps with the
        // same logical entries but different wire order and shadowed values.
        for n in [0, 1, 2, 8, 15, 16, 17, 64, 256] {
            for distinct in [1, 3, n.max(1)] {
                let mut left = Map::new();
                let mut expected = HashMap::new();
                for i in 0..n {
                    let key = (i * 17 + 3) % distinct;
                    left.push_entry(key, i);
                    expected.insert(key, i);
                }
                let mut right = Map::new();
                for (&key, &value) in &expected {
                    right.push_entry(key, -1);
                    right.push_entry(key, value);
                }
                assert_eq!(left, right, "n={n}, distinct={distinct}");
                assert_eq!(right, left, "n={n}, distinct={distinct}");
                for changed_key in [0, 3, 17, 257] {
                    for changed_value in [-2, 0, 1, 17, 257] {
                        let mut changed = right.clone();
                        let mut expected_changed = expected.clone();
                        changed.insert(changed_key, changed_value);
                        expected_changed.insert(changed_key, changed_value);
                        assert_eq!(
                            left == changed,
                            expected == expected_changed,
                            "n={n}, distinct={distinct}, key={changed_key}, value={changed_value}"
                        );
                        assert_eq!(changed == left, expected_changed == expected);
                    }
                }
                for missing in expected.keys() {
                    let mut changed = Map::new();
                    let mut expected_changed = expected.clone();
                    expected_changed.remove(missing);
                    // Keep cardinality unchanged while replacing one key.
                    expected_changed.insert(1000, -1);
                    for (&key, &value) in &expected_changed {
                        changed.push_entry(key, value);
                    }
                    assert_eq!(left == changed, expected == expected_changed);
                    assert_eq!(changed == left, expected_changed == expected);
                }
            }
        }
    }

    #[test]
    fn equality_retains_non_reflexive_value_semantics() {
        let mut left = Map::new();
        left.push_entry(1, f64::NAN);
        let right = left.clone();
        assert_ne!(left, right);
        left.insert(1, -0.0);
        let mut right = Map::new();
        right.push_entry(1, 0.0);
        assert_eq!(left, right);
    }

    #[test]
    fn equality_compares_string_keys_as_raw_bytes() {
        for n in [2, 16, 32] {
            let mut left = Map::new();
            let mut right = Map::new();
            let mut expected_left = std::collections::HashMap::new();
            let mut expected_right = std::collections::HashMap::new();
            for i in 0..n {
                let key = vec![0xff, i, 0x80];
                left.push_entry(ProtoString::from_bytes(&key), i);
                expected_left.insert(key, i);
            }
            for i in (0..n).rev() {
                let key = vec![0xff, i, 0x80];
                right.push_entry(ProtoString::from_bytes(&key), 0);
                right.push_entry(ProtoString::from_bytes(&key), i);
                expected_right.insert(key, i);
            }
            assert_eq!(left == right, expected_left == expected_right);
            assert_eq!(right, left);
            let replacement = vec![0xff, 0, 0x81];
            right.remove(&ProtoString::from_bytes(&[0xff, 0, 0x80]));
            // Remove every old duplicate before replacing its logical key.
            right.remove(&ProtoString::from_bytes(&[0xff, 0, 0x80]));
            right.push_entry(ProtoString::from_bytes(&replacement), 0);
            expected_right.remove(&vec![0xff, 0, 0x80]);
            expected_right.insert(replacement, 0);
            assert_eq!(left == right, expected_left == expected_right);
            assert_ne!(right, left);
        }
    }

    #[test]
    fn equality_preserves_value_comparison_direction() {
        // Even a user type with an asymmetric comparison is called in the
        // existing direction: the looked-up right value compares to the left.
        #[derive(Clone, Debug)]
        struct Directional(u8);
        impl PartialEq for Directional {
            fn eq(&self, other: &Self) -> bool {
                self.0 <= other.0
            }
        }
        let mut left = Map::new();
        let mut right = Map::new();
        for n in [1, 16, 32] {
            left.clear();
            right.clear();
            for key in 0..n {
                left.push_entry(key, Directional(1));
                right.push_entry(key, Directional(2));
            }
            assert_ne!(left, right);
            assert_eq!(right, left);
        }
    }

    #[test]
    fn equality_observes_warm_index_mutations() {
        let mut left = Map::new();
        let mut right = Map::new();
        for key in 0..32 {
            left.push_entry(key, key * 10);
        }
        for key in (0..32).rev() {
            right.push_entry(key, key * 10);
        }
        assert_eq!(left, right); // Builds both indexes.
        assert!(!left.insert(17, -1));
        assert_ne!(left, right);
        assert!(!right.as_mut().insert(17, -1));
        assert_eq!(left, right);
        assert!(left.insert(33, 330));
        assert_ne!(left, right);
        assert!(right.as_mut().insert(33, 330));
        assert_eq!(left, right);
        left.push_entry(17, 170); // Drops one index, keeps a shadowed entry.
        assert_ne!(left, right);
        assert!(!right.insert(17, 170));
        assert_eq!(left, right);
        assert_eq!(left.remove(&17), Some(170));
        assert_ne!(left, right); // The older -1 value resurfaces.
        assert!(!right.insert(17, -1));
        assert_eq!(left, right);
        assert!(left.as_mut().remove(33));
        assert_ne!(left, right);
        assert_eq!(right.remove(&33), Some(330));
        assert_eq!(left, right);
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

    #[test]
    fn arena_backed_map_iteration_does_not_leak_key_bytes() {
        let arena = crate::runtime::Arena::new();
        let raw = arena.alloc_map();
        // SAFETY: `raw` points to a map owned by `arena`; no view exists while inserting.
        unsafe {
            (*raw)
                .entries
                .borrow_mut()
                .push((b"alpha".to_vec(), crate::runtime::FieldKind::I32(7)));
        }
        // SAFETY: `arena` owns the map throughout the lifetime of this immutable view.
        let view: MapView<'_, ProtoString, i32> = unsafe { MapView::from_raw_ptr(raw) };
        let pairs: Vec<_> = view
            .iter()
            .map(|(key, value)| (key.as_bytes().to_vec(), value))
            .collect();
        assert_eq!(pairs, vec![(b"alpha".to_vec(), 7)]);
    }

    fn oracle_last_wins(wire: &[(i32, i32)]) -> (usize, Vec<(i32, i32)>) {
        use std::collections::HashMap;
        let mut last: HashMap<i32, i32> = HashMap::new();
        for (k, v) in wire {
            last.insert(*k, *v);
        }
        let mut order = Vec::new();
        for (i, (k, _)) in wire.iter().enumerate() {
            if wire[i + 1..].iter().all(|(k2, _)| k2 != k) {
                order.push((*k, last[k]));
            }
        }
        (last.len(), order)
    }

    #[test]
    fn hybrid_matches_scan_across_threshold() {
        // White-box boundary: below INDEX_THRESHOLD reads scan, at and above
        // they use the sorted index. Semantics must be identical.
        for n in [
            0,
            1,
            2,
            INDEX_THRESHOLD - 1,
            INDEX_THRESHOLD,
            INDEX_THRESHOLD + 1,
            64,
            100,
        ] {
            let wire: Vec<(i32, i32)> = (0..n as i32)
                .map(|i| ((i * 7 + 3) % ((n / 2 + 1) as i32).max(1), i))
                .collect();
            let mut m = Map::new();
            for (k, v) in &wire {
                m.push_entry(*k, *v);
            }
            let (unique, order) = oracle_last_wins(&wire);
            assert_eq!(m.pairs(), wire.as_slice(), "pairs keep wire order (n={n})");
            assert_eq!(m.len(), unique, "len (n={n})");
            assert_eq!(m.as_view().len(), unique, "view len (n={n})");
            for (k, v) in &order {
                assert_eq!(m.get(k), Some(v), "get {k} (n={n})");
                assert_eq!(m.as_view().get(*k), Some(*v), "view get {k} (n={n})");
            }
            assert_eq!(m.get(&i32::MIN), None, "missing key (n={n})");
            let got: Vec<(i32, i32)> = m.iter().map(|(k, v)| (*k, *v)).collect();
            assert_eq!(got, order, "iter order (n={n})");
            let vgot: Vec<(i32, i32)> = m.as_view().iter().collect();
            assert_eq!(vgot, order, "view iter order (n={n})");
        }
    }

    #[test]
    fn hybrid_hot_index_survives_insert_and_rebuilds_after_remove() {
        let n = INDEX_THRESHOLD + 8;
        let mut m = Map::new();
        for i in 0..n as i32 {
            m.push_entry(i, i * 10);
        }
        // Build the index, then mutate through both owned and view APIs.
        assert_eq!(m.get(&0), Some(&0));
        assert!(!m.insert(0, 1));
        assert!(m.insert(n as i32, -1));
        assert_eq!(m.get(&0), Some(&1));
        assert_eq!(m.get(&(n as i32)), Some(&-1));
        assert_eq!(m.len(), n + 1);
        {
            let mut mm = m.as_mut();
            assert!(!mm.insert(1, 11));
            assert!(mm.insert(n as i32 + 1, -2));
            assert_eq!(mm.get(1), Some(11));
            assert_eq!(mm.len(), n + 2);
            assert!(mm.remove(1));
            assert_eq!(mm.get(1), None);
            assert_eq!(mm.len(), n + 1);
        }
        // Duplicate-key remove drops only the last entry; the earlier one
        // resurfaces even after the index was dropped and rebuilt.
        m.push_entry(1000, 50);
        m.push_entry(1000, 51);
        assert_eq!(m.get(&1000), Some(&51));
        assert_eq!(m.remove(&1000), Some(51));
        assert_eq!(m.get(&1000), Some(&50));
        assert_eq!(m.remove(&1000), Some(50));
        assert_eq!(m.get(&1000), None);
        let (unique, order) = oracle_last_wins(m.pairs());
        assert_eq!(m.len(), unique);
        let got: Vec<(i32, i32)> = m.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(got, order);
    }

    #[test]
    fn arena_backed_map_values_are_reclaimed() {
        let arena = crate::runtime::Arena::new();
        let raw = arena.alloc_map();
        // SAFETY: raw is owned by arena and exclusively borrowed by map until
        // map is dropped before its backing arena.
        let mut map: MapMut<'_, ProtoString, ProtoString> = unsafe { MapMut::from_raw_inner(raw) };
        assert!(map.insert("alpha", "one"));
        assert!(!map.insert("alpha", "two"));
        assert_eq!(
            map.get("alpha").map(|v| v.as_bytes()),
            Some(b"two".as_slice())
        );
        assert!(map.remove("alpha"));
        assert!(map.is_empty());
    }
}
