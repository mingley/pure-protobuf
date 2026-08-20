//! Eager-validate, lazy-materialize string/bytes over a shared wire buffer.

use crate::map::MapKey;
use crate::string::{ProtoBytes, ProtoStr, ProtoString};
use std::sync::Arc;

/// Shared immutable wire bytes. Windows are cheap (`Arc` clone + range).
#[derive(Clone, Debug)]
pub struct Wire {
    buf: Arc<[u8]>,
    start: u32,
    end: u32,
}

impl Wire {
    pub fn empty() -> Self {
        static EMPTY: std::sync::OnceLock<Arc<[u8]>> = std::sync::OnceLock::new();
        Self {
            buf: EMPTY
                .get_or_init(|| Arc::<[u8]>::from(&[] as &[u8]))
                .clone(),
            start: 0,
            end: 0,
        }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        if data.is_empty() {
            return Self::empty();
        }
        let buf: Arc<[u8]> = Arc::from(data);
        let end = buf.len() as u32;
        Self { buf, start: 0, end }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.start as usize..self.end as usize]
    }

    /// `rel_start..rel_end` are indices into [`as_slice`].
    pub fn window(&self, rel_start: usize, rel_end: usize) -> Self {
        let start = self.start + rel_start as u32;
        let end = self.start + rel_end as u32;
        debug_assert!(end <= self.end);
        Self {
            buf: Arc::clone(&self.buf),
            start,
            end,
        }
    }
}

/// Singular `string`: slice of the parse buffer, or owned after `set_*`.
#[derive(Clone, Default)]
pub enum LazyStr {
    #[default]
    Empty,
    Wire(Wire),
    Owned(ProtoString),
}

impl LazyStr {
    pub fn owned(s: ProtoString) -> Self {
        if s.is_empty() {
            Self::Empty
        } else {
            Self::Owned(s)
        }
    }

    pub fn from_wire(w: Wire) -> Self {
        Self::from_span(&w, 0, w.as_slice().len())
    }

    #[inline]
    pub fn from_span(wire: &Wire, rel_start: usize, rel_end: usize) -> Self {
        let s = &wire.as_slice()[rel_start..rel_end];
        if s.is_empty() {
            Self::Empty
        } else if s.len() <= 23 {
            Self::Owned(ProtoString::from_bytes(s))
        } else {
            Self::Wire(wire.window(rel_start, rel_end))
        }
    }

    #[inline]
    pub fn from_bytes(s: &[u8]) -> Self {
        if s.is_empty() {
            Self::Empty
        } else {
            Self::Owned(ProtoString::from_bytes(s))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Empty => b"",
            Self::Wire(w) => w.as_slice(),
            Self::Owned(s) => s.as_bytes(),
        }
    }

    pub fn as_view(&self) -> &ProtoStr {
        ProtoStr::from_bytes(self.as_bytes())
    }

    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }
}

impl PartialEq for LazyStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for LazyStr {}
impl PartialOrd for LazyStr {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for LazyStr {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}
impl std::hash::Hash for LazyStr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl std::fmt::Debug for LazyStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_view(), f)
    }
}

impl From<ProtoString> for LazyStr {
    fn from(s: ProtoString) -> Self {
        Self::owned(s)
    }
}
impl From<&str> for LazyStr {
    fn from(s: &str) -> Self {
        Self::owned(ProtoString::from(s))
    }
}
impl From<String> for LazyStr {
    fn from(s: String) -> Self {
        Self::owned(ProtoString::from(s))
    }
}
impl MapKey for LazyStr {}

/// Singular `bytes`. Same cow as [`LazyStr`].
#[derive(Clone, Default)]
pub enum LazyBytes {
    #[default]
    Empty,
    Wire(Wire),
    Owned(ProtoBytes),
}

impl LazyBytes {
    pub fn owned(s: ProtoBytes) -> Self {
        if s.is_empty() {
            Self::Empty
        } else {
            Self::Owned(s)
        }
    }

    pub fn from_wire(w: Wire) -> Self {
        if w.as_slice().is_empty() {
            Self::Empty
        } else {
            Self::Wire(w)
        }
    }

    #[inline]
    pub fn from_bytes(s: &[u8]) -> Self {
        if s.is_empty() {
            Self::Empty
        } else {
            Self::Owned(ProtoBytes::from(s))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Empty => b"",
            Self::Wire(w) => w.as_slice(),
            Self::Owned(s) => s.as_bytes(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }
}

impl PartialEq for LazyBytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for LazyBytes {}

impl std::fmt::Debug for LazyBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.as_bytes(), f)
    }
}

impl From<ProtoBytes> for LazyBytes {
    fn from(s: ProtoBytes) -> Self {
        Self::owned(s)
    }
}
impl From<&[u8]> for LazyBytes {
    fn from(s: &[u8]) -> Self {
        Self::owned(ProtoBytes::from(s))
    }
}
impl From<Vec<u8>> for LazyBytes {
    fn from(s: Vec<u8>) -> Self {
        Self::owned(ProtoBytes::from(s.as_slice()))
    }
}

/// Generated messages implement this so [`LazyMsg`] can validate without
/// constructing `T`, then materialize on first getter.
pub trait MergeBytes: Default + Sized {
    fn merge_inner(
        &mut self,
        wire: &Wire,
        pos: &mut usize,
        depth: u32,
        enforce: bool,
        until: Option<u32>,
    ) -> Result<(), crate::error::ParseError>;
    fn validate_inner(
        wire: &Wire,
        pos: &mut usize,
        depth: u32,
    ) -> Result<(), crate::error::ParseError> {
        let mut tmp = Self::default();
        tmp.merge_inner(wire, pos, depth, true, None)
    }
}

struct LazyInner<T> {
    parsed: std::sync::OnceLock<T>,
    wire: Option<Wire>,
}

/// Nested LEN message. Empty is null (8 bytes, zero-valid). Parse stores wire
/// after eager validation; `T` is built on first getter / mutator.
pub struct LazyMsg<T> {
    inner: Option<Box<LazyInner<T>>>,
}

impl<T> Default for LazyMsg<T> {
    #[inline]
    fn default() -> Self {
        Self { inner: None }
    }
}

impl<T> LazyMsg<T> {
    pub fn from_wire(w: Wire) -> Self {
        Self {
            inner: Some(Box::new(LazyInner {
                parsed: std::sync::OnceLock::new(),
                wire: Some(w),
            })),
        }
    }

    pub fn from_parsed(msg: T, w: Wire) -> Self {
        let parsed = std::sync::OnceLock::new();
        let _ = parsed.set(msg);
        Self {
            inner: Some(Box::new(LazyInner {
                parsed,
                wire: Some(w),
            })),
        }
    }

    pub fn from_owned(msg: T) -> Self {
        let parsed = std::sync::OnceLock::new();
        let _ = parsed.set(msg);
        Self {
            inner: Some(Box::new(LazyInner { parsed, wire: None })),
        }
    }

    #[inline]
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    #[inline]
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    pub fn as_deref(&self) -> Option<&T>
    where
        T: MergeBytes,
    {
        let inner = self.inner.as_ref()?;
        Some(inner.parsed.get_or_init(|| {
            let mut m = T::default();
            if let Some(w) = &inner.wire {
                let mut pos = 0;
                let _ = MergeBytes::merge_inner(&mut m, w, &mut pos, 0, true, None);
            }
            m
        }))
    }

    pub fn wire_bytes(&self) -> Option<&[u8]> {
        self.inner
            .as_ref()
            .and_then(|i| i.wire.as_ref())
            .map(|w| w.as_slice())
    }

    pub fn get_or_insert(&mut self) -> &mut T
    where
        T: MergeBytes,
    {
        let inner = self.inner.get_or_insert_with(|| {
            Box::new(LazyInner {
                parsed: std::sync::OnceLock::new(),
                wire: None,
            })
        });
        if inner.parsed.get().is_none() {
            let mut m = T::default();
            if let Some(w) = inner.wire.take() {
                let mut pos = 0;
                let _ = MergeBytes::merge_inner(&mut m, &w, &mut pos, 0, true, None);
            }
            let _ = inner.parsed.set(m);
        } else {
            inner.wire = None;
        }
        inner.parsed.get_mut().expect("lazy parsed")
    }

    pub fn clear(&mut self) {
        self.inner = None;
    }
}

impl<T: Clone> Clone for LazyMsg<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.as_ref().map(|i| {
                let parsed = std::sync::OnceLock::new();
                if let Some(m) = i.parsed.get() {
                    let _ = parsed.set(m.clone());
                }
                Box::new(LazyInner {
                    parsed,
                    wire: i.wire.clone(),
                })
            }),
        }
    }
}

impl<T: MergeBytes + PartialEq> PartialEq for LazyMsg<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_deref() == other.as_deref()
    }
}

impl<T: MergeBytes + std::fmt::Debug> std::fmt::Debug for LazyMsg<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.as_deref(), f)
    }
}
