use super::*;

use indexmap::{self, IndexMap};

use std::convert::Infallible;
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops;

/// Storage for streams
#[derive(Debug)]
pub(super) struct Store {
    slab: StreamSlots,
    ids: IndexMap<StreamId, SlabIndex, StreamIdHasher>,
    /// The only stream, when the id map is empty. Opening that stream does
    /// not hash; a second stream moves this entry into `ids` first.
    sole: Option<(StreamId, SlabIndex)>,
}

/// Stream storage with the same free list as `slab`, so a removed stream can
/// be dropped in its slot. `slab::Slab::remove` copies the value out first.
#[derive(Debug)]
struct StreamSlots {
    slots: Vec<Slot>,
    next: u32,
    len: usize,
}

#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "inline stream slots avoid a heap allocation per HTTP/2 stream"
)]
enum Slot {
    Vacant(u32),
    Occupied(ManuallyDrop<Stream>),
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
    ids: VacantIds<'a>,
    slab: &'a mut StreamSlots,
    sole: &'a mut Option<(StreamId, SlabIndex)>,
}

enum VacantIds<'a> {
    /// The map is empty and nothing is cached, so the insert records `sole`.
    Deferred(StreamId),
    Map(indexmap::map::VacantEntry<'a, StreamId, SlabIndex>),
}

pub(super) trait Resolve {
    fn resolve(&mut self, key: Key) -> Ptr<'_>;
}

// ===== impl Store =====

impl Store {
    pub fn new() -> Self {
        Store {
            slab: StreamSlots::new(),
            ids: IndexMap::with_hasher(StreamIdHasher::default()),
            sole: None,
        }
    }

    /// No id is recorded. The next insert does not touch the id map.
    #[inline]
    pub(super) fn has_no_ids(&self) -> bool {
        self.sole.is_none() && self.ids.is_empty()
    }

    /// The only stored stream, without hashing, when `id` is that stream.
    fn sole_index(&self, id: StreamId) -> Option<SlabIndex> {
        if let Some((only, index)) = self.sole {
            return (only == id).then_some(index);
        }
        if self.ids.len() != 1 {
            return None;
        }
        let (only, index) = self.ids.get_index(0)?;
        (*only == id).then_some(*index)
    }

    #[inline]
    fn bind_id(&mut self, id: StreamId, index: SlabIndex) {
        if self.sole.is_none() && self.ids.is_empty() {
            self.sole = Some((id, index));
            return;
        }
        self.bind_id_mapped(id, index);
    }

    /// The id map is only needed once a connection holds more than one stream.
    #[cold]
    #[inline(never)]
    fn bind_id_mapped(&mut self, id: StreamId, index: SlabIndex) {
        self.flush_sole();
        assert!(self.ids.insert(id, index).is_none());
    }

    fn flush_sole(&mut self) {
        if let Some((id, index)) = self.sole.take() {
            assert!(self.ids.insert(id, index).is_none());
        }
    }

    /// The one-stream connection. `None` means the caller must use [`Self::find_mut`].
    ///
    /// Inlined at the DATA receive so that hit does not call the outlined lookup.
    #[inline(always)]
    pub fn sole_hit(&mut self, id: StreamId) -> Option<Ptr<'_>> {
        match self.sole {
            Some((only, index)) if only == id => Some(Ptr {
                key: Key {
                    index,
                    stream_id: id,
                },
                store: self,
            }),
            _ => None,
        }
    }

    /// Same split as `find_entry`: the steady hit must not pay for the
    /// hash-map lookup's saved registers.
    #[inline(never)]
    pub fn find_mut(&mut self, id: &StreamId) -> Option<Ptr<'_>> {
        if let Some((only, index)) = self.sole {
            if only == *id {
                return Some(Ptr {
                    key: Key {
                        index,
                        stream_id: *id,
                    },
                    store: self,
                });
            }
        }
        self.find_mut_slow(id)
    }

    #[inline(never)]
    fn find_mut_slow(&mut self, id: &StreamId) -> Option<Ptr<'_>> {
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
        let index = SlabIndex(self.slab.insert(val));
        self.bind_id(id, index);

        Ptr {
            key: Key {
                index,
                stream_id: id,
            },
            store: self,
        }
    }

    /// Insert a new stream written directly into a reused slab slot.
    pub fn insert_new(
        &mut self,
        id: StreamId,
        init_send_window: WindowSize,
        init_recv_window: WindowSize,
    ) -> Ptr<'_> {
        let index = SlabIndex(self.slab.insert_new(id, init_send_window, init_recv_window));
        self.bind_id(id, index);

        Ptr {
            key: Key {
                index,
                stream_id: id,
            },
            store: self,
        }
    }

    /// The single-stream hit is the steady path. This stays a small
    /// function so the hit does not pay the hash path's register saves,
    /// and callers are not rewritten around the check.
    #[inline(never)]
    pub fn find_entry(&mut self, id: StreamId) -> Entry<'_> {
        if let Some((only, index)) = self.sole {
            if only == id {
                return Entry::Occupied(OccupiedEntry {
                    kind: OccupiedKind::Cached(Key {
                        index,
                        stream_id: id,
                    }),
                });
            }
        }
        self.find_entry_slow(id)
    }

    #[inline(never)]
    fn find_entry_slow(&mut self, id: StreamId) -> Entry<'_> {
        use self::indexmap::map::Entry::*;

        if let Some(index) = self.sole_index(id) {
            return Entry::Occupied(OccupiedEntry {
                kind: OccupiedKind::Cached(Key {
                    index,
                    stream_id: id,
                }),
            });
        }

        if self.ids.is_empty() && self.sole.is_none() {
            return Entry::Vacant(VacantEntry {
                ids: VacantIds::Deferred(id),
                slab: &mut self.slab,
                sole: &mut self.sole,
            });
        }

        self.flush_sole();

        let Store { slab, ids, sole } = self;
        match ids.entry(id) {
            Occupied(e) => Entry::Occupied(OccupiedEntry {
                kind: OccupiedKind::Map(e),
            }),
            Vacant(e) => Entry::Vacant(VacantEntry {
                ids: VacantIds::Map(e),
                slab,
                sole,
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
        // A cached sole stream is not in the map. Yield it first, and skip
        // it later if this callback moved it into the map.
        let yielded = if let Some((stream_id, index)) = self.sole {
            f(Ptr {
                key: Key { index, stream_id },
                store: self,
            })?;
            Some(stream_id)
        } else {
            None
        };

        let mut len = self.ids.len();
        let mut i = 0;

        while i < len {
            // Get the key by index, this makes the borrow checker happy
            let (stream_id, index) = {
                let entry = self.ids.get_index(i).unwrap();
                (*entry.0, *entry.1)
            };

            if Some(stream_id) == yielded {
                i += 1;
                continue;
            }

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

impl StreamSlots {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            next: 0,
            len: 0,
        }
    }

    fn insert(&mut self, value: Stream) -> u32 {
        let key = self.next;
        self.len += 1;
        if key as usize == self.slots.len() {
            self.slots.push(Slot::Occupied(ManuallyDrop::new(value)));
            self.next = key.wrapping_add(1);
        } else {
            self.next = match self.slots.get(key as usize) {
                Some(Slot::Vacant(next)) => *next,
                _ => unreachable!("stream slab insert hit an occupied slot"),
            };
            self.slots[key as usize] = Slot::Occupied(ManuallyDrop::new(value));
        }
        key
    }

    /// Same free-list as [`Self::insert`], without assuming enum payload layout.
    fn insert_new(
        &mut self,
        id: StreamId,
        init_send_window: WindowSize,
        init_recv_window: WindowSize,
    ) -> u32 {
        self.insert(Stream::new(id, init_send_window, init_recv_window))
    }

    fn get(&self, index: usize) -> Option<&Stream> {
        match self.slots.get(index) {
            Some(Slot::Occupied(stream)) => Some(stream),
            _ => None,
        }
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut Stream> {
        match self.slots.get_mut(index) {
            Some(Slot::Occupied(stream)) => Some(stream),
            _ => None,
        }
    }

    /// Resolve the occupied enum payload without assuming its offset.
    #[inline(always)]
    fn occupied_ptr(&mut self, index: usize) -> *mut Stream {
        match self.slots.get_mut(index) {
            Some(Slot::Occupied(stream)) => &mut **stream,
            _ => panic!("stream pointer names a vacant or missing slab slot"),
        }
    }

    /// Unlink the occupied owner before running user-provided waker drops.
    fn remove_in_place(&mut self, index: u32, stream_id: StreamId) -> StreamId {
        let slot_index = index as usize;
        let slot = match self.slots.get_mut(slot_index) {
            Some(slot @ Slot::Occupied(_)) => slot,
            _ => dangling_store_key(stream_id),
        };
        let id = match slot {
            Slot::Occupied(stream) if stream.id == stream_id => stream.id,
            _ => dangling_store_key(stream_id),
        };
        let old = std::mem::replace(slot, Slot::Vacant(self.next));
        self.next = index;
        self.len -= 1;
        if let Slot::Occupied(stream) = old {
            // A waker destructor may panic. The slab/free-list already publish
            // vacancy, so subsequent teardown cannot destroy this owner twice.
            if !stream.drop_is_nop() {
                drop(ManuallyDrop::into_inner(stream));
            }
        }
        id
    }

    #[cfg(any())]
    fn len(&self) -> usize {
        self.len
    }

    #[cfg(any())]
    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for StreamSlots {
    fn drop(&mut self) {
        // Vec<Slot> cannot drop Occupied's ManuallyDrop fields itself. Move
        // every live owner out first; a panicking waker leaves no stale owner
        // in that slot. Remaining entries can leak on unwind, never double-drop.
        for slot in &mut self.slots {
            if let Slot::Occupied(stream) = std::mem::replace(slot, Slot::Vacant(0)) {
                drop(ManuallyDrop::into_inner(stream));
            }
        }
    }
}

impl Resolve for Store {
    fn resolve(&mut self, key: Key) -> Ptr<'_> {
        Ptr { key, store: self }
    }
}

impl ops::Index<Key> for Store {
    type Output = Stream;

    /// Resolve a live key with its stream-id guard.
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
        self.ids.len() + usize::from(self.sole.is_some())
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

    /// Queue `stream` using a slot this caller already resolved.
    ///
    /// [`Self::push`] looks the stream up to read the queued flag. `queue_frame`
    /// already holds that slot, and a second lookup is a multiply by the stream
    /// stride.
    #[inline(always)]
    #[allow(
        unsafe_code,
        reason = "the queued flag is read through the slot the caller already resolved"
    )]
    pub fn push_resolved(&mut self, stream: &mut store::Ptr, slot: *mut Stream) -> bool {
        // SAFETY: `slot` is `stream.stream_ptr()` and this call does not remove
        // the stream. The tail lookup below borrows the store only after the
        // flag update, so it does not alias `slot`.
        unsafe {
            if N::is_queued(&*slot) {
                return false;
            }
            N::set_queued(&mut *slot, true);
            debug_assert!(N::next(&*slot).is_none());
        }

        let key = stream.key();
        match self.indices {
            Some(ref mut idxs) => {
                N::set_next(&mut stream.resolve(idxs.tail), Some(key));
                idxs.tail = key;
            }
            None => {
                self.indices = Some(store::Indices {
                    head: key,
                    tail: key,
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

    /// [`Self::push_front`] through a slot the caller already resolved.
    #[inline(always)]
    #[allow(
        unsafe_code,
        reason = "the queued flag is written through the slot the caller already resolved"
    )]
    pub fn push_front_resolved(&mut self, stream: &mut store::Ptr, slot: *mut Stream) -> bool {
        // SAFETY: `slot` is this stream. Linking it does not remove the slot.
        // The head lookup borrows the store only after the flag update.
        unsafe {
            if N::is_queued(&*slot) {
                return false;
            }
            N::set_queued(&mut *slot, true);
            debug_assert!(N::next(&*slot).is_none());
        }
        match self.indices {
            Some(ref mut idxs) => {
                let head_key = stream.resolve(idxs.head).key();
                // SAFETY: head resolution changes no slab allocation or occupancy;
                // slot still identifies the exclusively borrowed queued stream.
                unsafe { N::set_next(&mut *slot, Some(head_key)) };
                idxs.head = stream.key();
            }
            None => {
                let key = stream.key();
                self.indices = Some(store::Indices {
                    head: key,
                    tail: key,
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

    /// Pop the head stream and return the slot `pop` would look up again.
    ///
    /// The queued flag, the link, and the slot address share one slab index.
    #[inline(always)]
    #[allow(
        unsafe_code,
        reason = "the popped stream's queued flag is updated through the slot just resolved"
    )]
    pub fn pop_slot<'a>(&mut self, store: &'a mut Store) -> Option<(store::Ptr<'a>, *mut Stream)> {
        let Some(mut idxs) = self.indices else {
            return None;
        };
        let key = idxs.head;
        // One index. Later field reads use `slot` instead of `Ptr`'s lookup.
        let slot = &mut store[key] as *mut Stream;
        // SAFETY: `slot` is the occupied entry `key` just resolved. Updating
        // the link and the queued flag does not remove the slot, and `Ptr`
        // is built only after those writes.
        unsafe {
            if key == idxs.tail {
                if N::next(&*slot).is_some() {
                    broken_stream_queue();
                }
                self.indices = None;
            } else {
                let Some(head) = N::take_next(&mut *slot) else {
                    broken_stream_queue();
                };
                idxs.head = head;
                self.indices = Some(idxs);
            }
            debug_assert!(N::is_queued(&*slot));
            N::set_queued(&mut *slot, false);
        }
        Some((store.resolve(key), slot))
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

    /// Address of this stream's slab slot.
    ///
    /// Indexing the store is a multiply by the stream stride. `pop_frame`
    /// reads the same slot many times; one pointer replaces those indexes.
    /// The stream stays put until [`Self::remove`] or a close transition.
    #[inline(always)]
    pub(super) fn stream_ptr(&mut self) -> *mut Stream {
        let index = self.key.index.0 as usize;
        let ptr = self.store.slab.occupied_ptr(index);
        // The unchecked address has to be the one indexing would return.
        debug_assert_eq!(ptr, &mut self.store[self.key] as *mut Stream);
        ptr
    }

    pub fn store_mut(&mut self) -> &mut Store {
        self.store
    }

    /// Remove the stream from the store
    pub fn remove(self) -> StreamId {
        // The stream must have been unlinked before this point
        debug_assert!(!self.store.ids.contains_key(&self.key.stream_id));

        self.store
            .slab
            .remove_in_place(self.key.index.0, self.key.stream_id)
    }

    /// Remove the StreamId -> stream state association.
    ///
    /// This will effectively remove the stream as far as the H2 protocol is
    /// concerned.
    pub fn unlink(&mut self) {
        let id = self.key.stream_id;
        if self.store.sole.is_some_and(|(only, _)| only == id) {
            self.store.sole = None;
            return;
        }
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
        let stream_id = value.id;
        let index = SlabIndex(self.slab.insert(value));
        self.record(stream_id, index)
    }

    pub fn insert_new(
        self,
        id: StreamId,
        init_send_window: WindowSize,
        init_recv_window: WindowSize,
    ) -> Key {
        let index = SlabIndex(self.slab.insert_new(id, init_send_window, init_recv_window));
        self.record(id, index)
    }

    fn record(self, stream_id: StreamId, index: SlabIndex) -> Key {
        match self.ids {
            VacantIds::Deferred(id) => {
                debug_assert_eq!(id, stream_id);
                *self.sole = Some((stream_id, index));
            }
            VacantIds::Map(entry) => {
                entry.insert(index);
            }
        }
        Key { index, stream_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DropWake {
        drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        panic_once: bool,
    }

    impl std::task::Wake for DropWake {
        fn wake(self: std::sync::Arc<Self>) {}
    }

    impl Drop for DropWake {
        fn drop(&mut self) {
            let previous = self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(!self.panic_once || previous != 0, "waker destructor panic");
        }
    }

    fn drop_waker(
        drops: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        panic_once: bool,
    ) -> std::task::Waker {
        std::task::Waker::from(std::sync::Arc::new(DropWake {
            drops: drops.clone(),
            panic_once,
        }))
    }

    #[test]
    fn live_stream_wakers_are_released_when_the_store_is_dropped() {
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut stream = Stream::new(StreamId::from(1), 0, 0);
        stream.recv_task = Some(drop_waker(&drops, false));
        let mut slots = StreamSlots::new();
        slots.insert(stream);
        drop(slots);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_panicking_waker_drop_leaves_the_removed_slot_vacant_and_reusable() {
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut stream = Stream::new(StreamId::from(1), 0, 0);
        stream.recv_task = Some(drop_waker(&drops, true));
        let mut slots = StreamSlots::new();
        let first = slots.insert(stream);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            slots.remove_in_place(first, StreamId::from(1));
        }));
        assert!(failure.is_err());
        assert!(slots.get(first as usize).is_none());
        assert_eq!(slots.len, 0);
        let reused = slots.insert_new(StreamId::from(3), 64, 128);
        assert_eq!(reused, first);
        assert_eq!(
            slots.get(reused as usize).expect("reused stream").id,
            StreamId::from(3)
        );
        drop(slots);
        assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn reused_slot_initializes_a_complete_stream() {
        let mut slots = StreamSlots::new();
        let first = slots.insert(Stream::new(StreamId::from(1), 0, 0));
        slots.remove_in_place(first, StreamId::from(1));
        let reused = slots.insert_new(StreamId::from(3), 64, 128);
        let Some(got) = slots.get(reused as usize) else {
            panic!("reused slot holds a stream");
        };
        assert_eq!(got.id, StreamId::from(3));
        assert_eq!(got.ref_count, 0);
        assert!(got.is_recv);
    }

    #[test]
    fn one_live_stream_is_not_hashed_until_a_second_arrives() {
        let mut store = Store::new();
        {
            let _ = store.insert_new(StreamId::from(1), 0, 0);
        }
        assert!(store.ids.is_empty());
        assert!(store.sole.is_some());
        let Some(mut found) = store.find_mut(&StreamId::from(1)) else {
            panic!("sole stream is reachable by id");
        };
        found.unlink();
        assert!(store.sole.is_none());
        assert!(store.find_mut(&StreamId::from(1)).is_none());

        {
            let _ = store.insert_new(StreamId::from(3), 0, 0);
        }
        assert!(store.ids.is_empty());
        {
            let _ = store.insert(StreamId::from(5), Stream::new(StreamId::from(5), 0, 0));
        }
        assert!(store.sole.is_none());
        assert_eq!(store.ids.len(), 2);
        assert!(store.find_mut(&StreamId::from(3)).is_some());
        assert!(store.find_mut(&StreamId::from(5)).is_some());
    }
}
