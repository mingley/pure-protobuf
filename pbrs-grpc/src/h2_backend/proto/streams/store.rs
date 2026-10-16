use super::*;

use indexmap::{self, IndexMap};

use std::convert::Infallible;
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::ops;

/// Storage for streams
#[derive(Debug)]
pub(super) struct Store {
    slab: slab::Slab<Stream>,
    ids: IndexMap<StreamId, SlabIndex, StreamIdHasher>,
}

/// Identity hash for stream ids.
///
/// SipHash of a 31-bit id is most of a stream lookup. The id is already a
/// unique key, so the hash is the id. Sixteen bytes match `RandomState`,
/// which keeps this map the same size.
#[derive(Clone, Default)]
struct StreamIdHasher {
    #[allow(
        dead_code,
        reason = "matches RandomState so Store does not change size"
    )]
    pad: [u64; 2],
}

impl BuildHasher for StreamIdHasher {
    type Hasher = IdentityU32;

    fn build_hasher(&self) -> IdentityU32 {
        IdentityU32 { hash: 0 }
    }

    fn hash_one<T: Hash>(&self, x: T) -> u64 {
        let mut hasher = IdentityU32 { hash: 0 };
        x.hash(&mut hasher);
        hasher.hash
    }
}

const _: () = {
    assert!(
        std::mem::size_of::<StreamIdHasher>()
            == std::mem::size_of::<std::collections::hash_map::RandomState>()
    );
    assert!(
        std::mem::align_of::<StreamIdHasher>()
            == std::mem::align_of::<std::collections::hash_map::RandomState>()
    );
};

struct IdentityU32 {
    hash: u64,
}

impl Hasher for IdentityU32 {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Ok(bytes) = <[u8; 4]>::try_from(bytes) {
            self.write_u32(u32::from_le_bytes(bytes));
        }
    }

    fn write_u32(&mut self, i: u32) {
        self.hash = u64::from(i);
    }
}

/// "Pointer" to an entry in the store
pub(super) struct Ptr<'a> {
    key: Key,
    store: &'a mut Store,
}

/// References an entry in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    index: SlabIndex,
    /// Keep the stream ID in the key as an ABA guard, since slab indices
    /// could be re-used with a new stream.
    stream_id: StreamId,
}

// We can never have more than `StreamId::MAX` streams in the store,
// so we can save a smaller index (u32 vs usize).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlabIndex(u32);

pub(super) struct Queue<N> {
    indices: Option<store::Indices>,
    _p: PhantomData<N>,
}

pub(super) trait Next {
    fn next(stream: &Stream) -> Option<Key>;

    fn set_next(stream: &mut Stream, key: Option<Key>);

    fn take_next(stream: &mut Stream) -> Option<Key>;

    fn is_queued(stream: &Stream) -> bool;

    fn set_queued(stream: &mut Stream, val: bool);
}

/// A linked list
#[derive(Debug, Clone, Copy)]
struct Indices {
    pub head: Key,
    pub tail: Key,
}

pub(super) enum Entry<'a> {
    Occupied(OccupiedEntry<'a>),
    Vacant(VacantEntry<'a>),
}

pub(super) struct OccupiedEntry<'a> {
    kind: OccupiedKind<'a>,
}

enum OccupiedKind<'a> {
    Map(indexmap::map::OccupiedEntry<'a, StreamId, SlabIndex>),
    Cached(Key),
}

pub(super) struct VacantEntry<'a> {
    ids: indexmap::map::VacantEntry<'a, StreamId, SlabIndex>,
    slab: &'a mut slab::Slab<Stream>,
}

pub(super) trait Resolve {
    fn resolve(&mut self, key: Key) -> Ptr<'_>;
}

// ===== impl Store =====

impl Store {
    pub fn new() -> Self {
        Store {
            slab: slab::Slab::new(),
            ids: IndexMap::with_hasher(StreamIdHasher::default()),
        }
    }

    /// The only stored stream, without hashing, when `id` is that stream.
    fn sole_index(&self, id: StreamId) -> Option<SlabIndex> {
        if self.ids.len() != 1 {
            return None;
        }
        let (only, index) = self.ids.get_index(0)?;
        if *only == id { Some(*index) } else { None }
    }

    pub fn find_mut(&mut self, id: &StreamId) -> Option<Ptr<'_>> {
        let index = if let Some(index) = self.sole_index(*id) {
            index
        } else {
            match self.ids.get(id) {
                Some(key) => *key,
                None => return None,
            }
        };

        Some(Ptr {
            key: Key {
                index,
                stream_id: *id,
            },
            store: self,
        })
    }

    pub fn insert(&mut self, id: StreamId, val: Stream) -> Ptr<'_> {
        let index = SlabIndex(self.slab.insert(val) as u32);
        assert!(self.ids.insert(id, index).is_none());

        Ptr {
            key: Key {
                index,
                stream_id: id,
            },
            store: self,
        }
    }

    pub fn find_entry(&mut self, id: StreamId) -> Entry<'_> {
        use self::indexmap::map::Entry::*;

        if let Some(index) = self.sole_index(id) {
            return Entry::Occupied(OccupiedEntry {
                kind: OccupiedKind::Cached(Key {
                    index,
                    stream_id: id,
                }),
            });
        }

        match self.ids.entry(id) {
            Occupied(e) => Entry::Occupied(OccupiedEntry {
                kind: OccupiedKind::Map(e),
            }),
            Vacant(e) => Entry::Vacant(VacantEntry {
                ids: e,
                slab: &mut self.slab,
            }),
        }
    }

    #[allow(clippy::blocks_in_conditions)]
    pub(crate) fn for_each<F>(&mut self, mut f: F)
    where
        F: FnMut(Ptr),
    {
        match self.try_for_each(|ptr| {
            f(ptr);
            Ok::<_, Infallible>(())
        }) {
            Ok(()) => (),
            #[allow(unused)]
            Err(infallible) => match infallible {},
        }
    }

    pub fn try_for_each<F, E>(&mut self, mut f: F) -> Result<(), E>
    where
        F: FnMut(Ptr) -> Result<(), E>,
    {
        let mut len = self.ids.len();
        let mut i = 0;

        while i < len {
            // Get the key by index, this makes the borrow checker happy
            let (stream_id, index) = {
                let entry = self.ids.get_index(i).unwrap();
                (*entry.0, *entry.1)
            };

            f(Ptr {
                key: Key { index, stream_id },
                store: self,
            })?;

            // TODO: This logic probably could be better...
            let new_len = self.ids.len();

            if new_len < len {
                debug_assert!(new_len == len - 1);
                len -= 1;
            } else {
                i += 1;
            }
        }

        Ok(())
    }
}

impl Resolve for Store {
    fn resolve(&mut self, key: Key) -> Ptr<'_> {
        Ptr { key, store: self }
    }
}

impl ops::Index<Key> for Store {
    type Output = Stream;

    #[inline]
    fn index(&self, key: Key) -> &Self::Output {
        match self.slab.get(key.index.0 as usize) {
            Some(stream) if stream.id == key.stream_id => stream,
            _ => dangling_store_key(key.stream_id),
        }
    }
}

impl ops::IndexMut<Key> for Store {
    #[inline]
    fn index_mut(&mut self, key: Key) -> &mut Self::Output {
        match self.slab.get_mut(key.index.0 as usize) {
            Some(stream) if stream.id == key.stream_id => stream,
            _ => dangling_store_key(key.stream_id),
        }
    }
}

/// Kept cold so a live stream lookup does not carry the panic formatting.
#[cold]
#[inline(never)]
fn dangling_store_key(stream_id: StreamId) -> ! {
    panic!("dangling store key for stream_id={stream_id:?}");
}

impl Store {
    #[cfg(any())]
    pub fn num_active_streams(&self) -> usize {
        self.ids.len()
    }

    #[cfg(any())]
    pub fn num_wired_streams(&self) -> usize {
        self.slab.len()
    }
}

// While running h2 unit/integration tests, enable this debug assertion.
//
// In practice, we don't need to ensure this. But the integration tests
// help to make sure we've cleaned up in cases where we could (like, the
// runtime isn't suddenly dropping the task for unknown reasons).
#[cfg(any())]
impl Drop for Store {
    fn drop(&mut self) {
        use std::thread;

        if !thread::panicking() {
            debug_assert!(self.slab.is_empty());
        }
    }
}

// ===== impl Queue =====

impl<N> Queue<N>
where
    N: Next,
{
    pub fn new() -> Self {
        Queue {
            indices: None,
            _p: PhantomData,
        }
    }

    pub fn take(&mut self) -> Self {
        Queue {
            indices: self.indices.take(),
            _p: PhantomData,
        }
    }

    /// Queue the stream.
    ///
    /// If the stream is already contained by the list, return `false`.
    pub fn push(&mut self, stream: &mut store::Ptr) -> bool {
        if N::is_queued(stream) {
            return false;
        }

        N::set_queued(stream, true);

        // The next pointer shouldn't be set
        debug_assert!(N::next(stream).is_none());

        // Queue the stream
        match self.indices {
            Some(ref mut idxs) => {
                // Update the current tail node to point to `stream`
                let key = stream.key();
                N::set_next(&mut stream.resolve(idxs.tail), Some(key));

                // Update the tail pointer
                idxs.tail = stream.key();
            }
            None => {
                self.indices = Some(store::Indices {
                    head: stream.key(),
                    tail: stream.key(),
                });
            }
        }

        true
    }

    /// Queue the stream
    ///
    /// If the stream is already contained by the list, return `false`.
    pub fn push_front(&mut self, stream: &mut store::Ptr) -> bool {
        if N::is_queued(stream) {
            return false;
        }

        N::set_queued(stream, true);

        // The next pointer shouldn't be set
        debug_assert!(N::next(stream).is_none());

        // Queue the stream
        match self.indices {
            Some(ref mut idxs) => {
                // Update the provided stream to point to the head node
                let head_key = stream.resolve(idxs.head).key();
                N::set_next(stream, Some(head_key));

                // Update the head pointer
                idxs.head = stream.key();
            }
            None => {
                self.indices = Some(store::Indices {
                    head: stream.key(),
                    tail: stream.key(),
                });
            }
        }

        true
    }

    #[inline]
    pub fn pop<'a, R>(&mut self, store: &'a mut R) -> Option<store::Ptr<'a>>
    where
        R: Resolve,
    {
        let Some(mut idxs) = self.indices else {
            return None;
        };
        let mut stream = store.resolve(idxs.head);

        if idxs.head == idxs.tail {
            if N::next(&stream).is_some() {
                broken_stream_queue();
            }
            self.indices = None;
        } else {
            let Some(head) = N::take_next(&mut stream) else {
                broken_stream_queue();
            };
            idxs.head = head;
            self.indices = Some(idxs);
        }

        debug_assert!(N::is_queued(&stream));
        N::set_queued(&mut stream, false);

        Some(stream)
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_none()
    }

    pub fn pop_if<'a, R, F>(&mut self, store: &'a mut R, f: F) -> Option<store::Ptr<'a>>
    where
        R: Resolve,
        F: Fn(&Stream) -> bool,
    {
        if let Some(idxs) = self.indices {
            let should_pop = f(&store.resolve(idxs.head));
            if should_pop {
                return self.pop(store);
            }
        }

        None
    }
}

#[cold]
#[inline(never)]
fn broken_stream_queue() -> ! {
    panic!("http2 stream queue is corrupt");
}

impl<N> fmt::Debug for Queue<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Queue")
            .field("indices", &self.indices)
            // skip phantom data
            .finish()
    }
}

// ===== impl Ptr =====

impl<'a> Ptr<'a> {
    /// Returns the Key associated with the stream
    pub fn key(&self) -> Key {
        self.key
    }

    pub fn store_mut(&mut self) -> &mut Store {
        self.store
    }

    /// Remove the stream from the store
    pub fn remove(self) -> StreamId {
        // The stream must have been unlinked before this point
        debug_assert!(!self.store.ids.contains_key(&self.key.stream_id));

        // Read the id before removal. Binding `slab.remove`'s return value
        // copies all 328 bytes of the stream out of the slab just to drop
        // them. Leaving that value unused lets the compiler drop it in place.
        let id = self.store[self.key].id;
        self.store.slab.remove(self.key.index.0 as usize);
        id
    }

    /// Remove the StreamId -> stream state association.
    ///
    /// This will effectively remove the stream as far as the H2 protocol is
    /// concerned.
    pub fn unlink(&mut self) {
        let id = self.key.stream_id;
        let ids = &mut self.store.ids;
        // One live stream is the steady case. Clearing it skips the hash
        // lookup and the index fixup that `swap_remove` does for a larger map.
        if ids.len() == 1 && ids.get_index(0).is_some_and(|(only, _)| *only == id) {
            ids.clear();
            return;
        }
        ids.swap_remove(&id);
    }
}

impl<'a> Resolve for Ptr<'a> {
    fn resolve(&mut self, key: Key) -> Ptr<'_> {
        Ptr {
            key,
            store: &mut *self.store,
        }
    }
}

impl<'a> ops::Deref for Ptr<'a> {
    type Target = Stream;

    #[inline]
    fn deref(&self) -> &Stream {
        &self.store[self.key]
    }
}

impl<'a> ops::DerefMut for Ptr<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Stream {
        &mut self.store[self.key]
    }
}

impl<'a> fmt::Debug for Ptr<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(fmt)
    }
}

// ===== impl OccupiedEntry =====

impl<'a> OccupiedEntry<'a> {
    pub fn key(&self) -> Key {
        match &self.kind {
            OccupiedKind::Map(ids) => Key {
                index: *ids.get(),
                stream_id: *ids.key(),
            },
            OccupiedKind::Cached(key) => *key,
        }
    }
}

// ===== impl VacantEntry =====

impl<'a> VacantEntry<'a> {
    pub fn insert(self, value: Stream) -> Key {
        // Insert the value in the slab
        let stream_id = value.id;
        let index = SlabIndex(self.slab.insert(value) as u32);

        // Insert the handle in the ID map
        self.ids.insert(index);

        Key { index, stream_id }
    }
}
