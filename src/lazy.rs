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
    pub fn from_slice(data: &[u8]) -> Self {
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
        if w.as_slice().is_empty() {
            Self::Empty
        } else {
            Self::Wire(w)
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
