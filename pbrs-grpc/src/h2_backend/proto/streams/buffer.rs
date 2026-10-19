/// Buffers frames for multiple streams.
#[derive(Debug)]
pub struct Buffer<T> {
    slab: FrameSlab<Slot<T>>,
}

/// A sequence of frames in a `Buffer`
#[derive(Debug)]
pub struct Deque {
    indices: Option<Indices>,
}

/// Tracks the head & tail for a sequence of frames in a `Buffer`.
#[derive(Debug, Default, Copy, Clone)]
struct Indices {
    head: usize,
    tail: usize,
}

#[derive(Debug)]
struct Slot<T> {
    value: T,
    next: Option<usize>,
}

/// Same free list as `slab`, inlined into the send path.
///
/// `slab::insert` is a separate call, so a queued frame is copied onto the
/// stack and then again into the slot. The reuse path writes it once.
#[derive(Debug)]
struct FrameSlab<T> {
    entries: Vec<Entry<T>>,
    next: usize,
    len: usize,
}

#[derive(Debug)]
enum Entry<T> {
    Vacant(usize),
    Occupied(T),
}

impl<T> FrameSlab<T> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    fn insert(&mut self, val: T) -> usize {
        let key = self.next;
        self.len += 1;
        if key == self.entries.len() {
            self.insert_new(key, val);
            return key;
        }
        let next = match self.entries.get(key) {
            Some(Entry::Vacant(next)) => *next,
            _ => broken_frame_slab(),
        };
        self.next = next;
        self.entries[key] = Entry::Occupied(val);
        key
    }

    #[cold]
    #[inline(never)]
    fn insert_new(&mut self, key: usize, val: T) {
        self.entries.push(Entry::Occupied(val));
        self.next = key + 1;
    }

    fn get(&self, key: usize) -> Option<&T> {
        match self.entries.get(key) {
            Some(Entry::Occupied(val)) => Some(val),
            _ => None,
        }
    }
}

impl<T> FrameSlab<Slot<T>> {
    /// Queue a frame into a free slot. The frame is written once; a vacant
    /// entry holds only a link, so it is overwritten in place.
    #[inline(always)]
    #[allow(
        unsafe_code,
        reason = "the frame is written over a vacant link without dropping a live frame"
    )]
    fn insert_frame(&mut self, value: T) -> usize {
        let key = self.next;
        self.len += 1;
        if key == self.entries.len() {
            self.insert_new(key, Slot { value, next: None });
            return key;
        }
        // SAFETY: `key` came from the free list. A non-vacant entry is a
        // corrupt slab. `ptr::write` does not drop the vacant link.
        unsafe {
            let entry = self.entries.as_mut_ptr().add(key);
            let free_next = match &*entry {
                Entry::Vacant(next) => *next,
                Entry::Occupied(_) => broken_frame_slab(),
            };
            self.next = free_next;
            std::ptr::write(entry, Entry::Occupied(Slot { value, next: None }));
        }
        key
    }

    /// Take the queued frame. It is read once; the slot becomes vacant in
    /// place instead of copying the entry out and then the frame again.
    #[inline(always)]
    #[allow(
        unsafe_code,
        reason = "the occupied frame is moved out and the slot is overwritten with vacant"
    )]
    fn remove_frame(&mut self, key: usize) -> (T, Option<usize>) {
        let free = self.next;
        // SAFETY: `key` is a live deque index. A vacant slot is a corrupt
        // queue. `ptr::read` takes the frame; `ptr::write` does not drop it.
        unsafe {
            let entry = self.entries.as_mut_ptr().add(key);
            let (val, next) = match &mut *entry {
                Entry::Occupied(slot) => {
                    let val = std::ptr::read(&slot.value);
                    (val, slot.next)
                }
                Entry::Vacant(_) => broken_frame_slab(),
            };
            std::ptr::write(entry, Entry::Vacant(free));
            self.len -= 1;
            self.next = key;
            (val, next)
        }
    }
}

impl<T> std::ops::Index<usize> for FrameSlab<T> {
    type Output = T;

    fn index(&self, key: usize) -> &T {
        match self.entries.get(key) {
            Some(Entry::Occupied(val)) => val,
            _ => broken_frame_slab(),
        }
    }
}

impl<T> std::ops::IndexMut<usize> for FrameSlab<T> {
    fn index_mut(&mut self, key: usize) -> &mut T {
        match self.entries.get_mut(key) {
            Some(Entry::Occupied(val)) => val,
            _ => broken_frame_slab(),
        }
    }
}

#[cold]
#[inline(never)]
fn broken_frame_slab() -> ! {
    panic!("http2 frame slab is corrupt");
}

impl<T> Buffer<T> {
    pub fn new() -> Self {
        Buffer {
            slab: FrameSlab::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }
}

impl Deque {
    pub fn new() -> Self {
        Deque { indices: None }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_none()
    }

    /// The front value, if the queue is non-empty.
    ///
    /// Callers that only need the kind of the next frame use this so a
    /// rejected front is not removed and inserted again.
    pub fn front<'a, T>(&self, buf: &'a Buffer<T>) -> Option<&'a T> {
        let idxs = self.indices?;
        buf.slab.get(idxs.head).map(|slot| &slot.value)
    }

    #[inline(always)]
    pub fn push_back<T>(&mut self, buf: &mut Buffer<T>, value: T) {
        let key = buf.slab.insert_frame(value);

        match self.indices {
            Some(ref mut idxs) => {
                buf.slab[idxs.tail].next = Some(key);
                idxs.tail = key;
            }
            None => {
                self.indices = Some(Indices {
                    head: key,
                    tail: key,
                });
            }
        }
    }

    pub fn push_front<T>(&mut self, buf: &mut Buffer<T>, value: T) {
        let key = buf.slab.insert(Slot { value, next: None });

        match self.indices {
            Some(ref mut idxs) => {
                buf.slab[key].next = Some(idxs.head);
                idxs.head = key;
            }
            None => {
                self.indices = Some(Indices {
                    head: key,
                    tail: key,
                });
            }
        }
    }

    #[inline]
    pub fn pop_front<T>(&mut self, buf: &mut Buffer<T>) -> Option<T> {
        let Some(mut idxs) = self.indices else {
            return None;
        };
        let (value, next) = buf.slab.remove_frame(idxs.head);

        if idxs.head == idxs.tail {
            if next.is_some() {
                broken_frame_deque();
            }
            self.indices = None;
        } else {
            let Some(head) = next else {
                broken_frame_deque();
            };
            idxs.head = head;
            self.indices = Some(idxs);
        }

        Some(value)
    }
}

#[cold]
#[inline(never)]
fn broken_frame_deque() -> ! {
    panic!("http2 stream frame deque is corrupt");
}
