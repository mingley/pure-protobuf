//! Eager-validate, lazy-materialize string/bytes over a shared wire buffer.
#![allow(
    clippy::expect_used,
    reason = "OnceLock get_mut after ensure is an internal invariant"
)]

use crate::error::ParseError;
use crate::map::MapKey;
use crate::string::{ProtoBytes, ProtoStr, ProtoString};
use bytes::Bytes;
use std::sync::Arc;

/// Proto3 string UTF-8 check via `simdutf8`.
///
/// `name_4kib` vs `blob_4kib` decode (~80 ns) was `str::from_utf8` on 4 KiB
/// of `'x'`. ASCII is valid UTF-8; `simdutf8` is the fast path for both
/// short tags and 4 KiB payloads.
#[inline(always)]
pub fn require_utf8(b: &[u8]) -> Result<(), ParseError> {
    match simdutf8::basic::from_utf8(b) {
        Ok(_) => Ok(()),
        Err(_) => Err(ParseError::new("invalid utf-8")),
    }
}

/// Shared immutable wire bytes. Windows are cheap (refcount clone + range).
///
/// Two backings (PK-09 variant B): [`Self::from_slice`] keeps the historical
/// private `Arc` copy whose windows never allocate, while [`Self::from_bytes`]
/// shares the caller's [`Bytes`] with zero copies. Empty values normalize to
/// the static empty so they never pin a large backing buffer.
#[derive(Clone, Debug)]
pub struct Wire {
    inner: WireInner,
}

#[derive(Clone, Debug)]
enum WireInner {
    Private {
        buf: Arc<[u8]>,
        start: u32,
        end: u32,
    },
    Shared(Bytes),
}

impl Wire {
    pub fn empty() -> Self {
        Self {
            inner: WireInner::Shared(Bytes::new()),
        }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        if data.is_empty() {
            return Self::empty();
        }
        crate::copy_counts::note_wire(data.len());
        let buf: Arc<[u8]> = Arc::from(data);
        let end = buf.len() as u32;
        Self {
            inner: WireInner::Private { buf, start: 0, end },
        }
    }

    /// Share `data` without copying. No copy counter fires: this is the
    /// zero-copy entry point [`Parse::parse_bytes`](crate::Parse::parse_bytes)
    /// builds on.
    pub fn from_bytes(data: Bytes) -> Self {
        if data.is_empty() {
            return Self::empty();
        }
        Self {
            inner: WireInner::Shared(data),
        }
    }

    /// One-pass copy of `s` into a private buffer, with a high-bit scan.
    ///
    /// ASCII is valid UTF-8. Non-ASCII falls back to `str::from_utf8`.
    /// Used for long parsed strings so we do not pay `from_utf8` and then a
    /// second parent-frame `ensure` (that pair was the `name_4kib` vs
    /// `blob_4kib` decode gap).
    #[inline]
    pub fn from_utf8_payload(s: &[u8]) -> Result<Self, ParseError> {
        // memcpy into a private buffer first so the UTF-8/ASCII scan hits
        // the copy (L1), instead of `str::from_utf8` on the source plus a
        // second parent copy.
        let w = Self::from_slice(s);
        require_utf8(w.as_slice())?;
        Ok(w)
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.inner {
            WireInner::Private { buf, start, end } => &buf[*start as usize..*end as usize],
            WireInner::Shared(b) => b,
        }
    }

    /// This window as a shareable [`Bytes`]: a refcount clone for shared
    /// backing; a copy for private backing (only `parse_bytes` shares).
    pub fn as_bytes(&self) -> Bytes {
        match &self.inner {
            WireInner::Private { buf, start, end } => {
                Bytes::copy_from_slice(&buf[*start as usize..*end as usize])
            }
            WireInner::Shared(b) => b.clone(),
        }
    }

    /// Whether this window shares a caller-owned buffer (`from_bytes`).
    /// Private windows (`from_slice`) only pin memory the message owns.
    pub fn is_shared(&self) -> bool {
        matches!(self.inner, WireInner::Shared(_))
    }

    /// Build a `Wire` the first time a lazy string/bytes/nested/packed-varint
    /// field needs a stable backing buffer. Scalar-only parses skip this copy.
    #[inline]
    pub fn ensure<'a>(slot: &'a mut Option<Wire>, data: &[u8]) -> &'a Wire {
        slot.get_or_insert_with(|| Wire::from_slice(data))
    }

    /// `rel_start..rel_end` are indices into [`Self::as_slice`].
    pub fn window(&self, rel_start: usize, rel_end: usize) -> Self {
        if rel_start == rel_end {
            return Self::empty();
        }
        match &self.inner {
            WireInner::Private { buf, start, end } => {
                let base = *start as usize;
                debug_assert!(base + rel_end <= *end as usize);
                debug_assert!(rel_start <= rel_end);
                Self {
                    inner: WireInner::Private {
                        buf: Arc::clone(buf),
                        start: base as u32 + rel_start as u32,
                        end: base as u32 + rel_end as u32,
                    },
                }
            }
            WireInner::Shared(b) => {
                debug_assert!(rel_end <= b.len());
                debug_assert!(rel_start <= rel_end);
                Self {
                    inner: WireInner::Shared(b.slice(rel_start..rel_end)),
                }
            }
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
    /// Matches [`ProtoString`] SSO. Inline copies do not keep a [`Wire`].
    const INLINE: usize = 23;

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
        } else if s.len() <= Self::INLINE {
            Self::Owned(ProtoString::from_bytes(s))
        } else {
            Self::Wire(wire.window(rel_start, rel_end))
        }
    }

    /// Parse a string span from the parent message bytes.
    ///
    /// `len <= 23` copies into inline [`ProtoString`] and does **not**
    /// [`Wire::ensure`] the parent frame (hello `"ada"` would otherwise
    /// Arc the 5-byte message and drop it).
    ///
    /// Longer strings that are almost the whole message (`name_4kib`) copy
    /// the payload once while checking UTF-8. Several medium strings in one
    /// message (kernel `strings`) share the parent frame instead of one Arc
    /// each.
    ///
    /// proto3 / `utf8_validation = VERIFY`. proto2 NONE uses
    /// [`Self::from_parse_span_unchecked`].
    #[inline]
    pub fn from_parse_span(
        slot: &mut Option<Wire>,
        data: &[u8],
        rel_start: usize,
        rel_end: usize,
    ) -> Result<Self, ParseError> {
        let s = &data[rel_start..rel_end];
        if s.len() <= Self::INLINE {
            require_utf8(s)?;
            return Ok(Self::from_bytes(s));
        }
        if s.len().saturating_add(8) >= data.len() {
            let _ = slot;
            return Ok(Self::Wire(Wire::from_utf8_payload(s)?));
        }
        require_utf8(s)?;
        Ok(Self::from_span(
            Wire::ensure(slot, data),
            rel_start,
            rel_end,
        ))
    }

    /// Same copy strategy as [`Self::from_parse_span`], no UTF-8 check.
    ///
    /// proto2 `utf8_validation = NONE` (and editions NONE) must Parse
    /// `\x80`. Does not [`Wire::ensure`] the parent frame.
    #[inline]
    pub fn from_parse_span_unchecked(
        slot: &mut Option<Wire>,
        data: &[u8],
        rel_start: usize,
        rel_end: usize,
    ) -> Self {
        let s = &data[rel_start..rel_end];
        if s.len() <= Self::INLINE {
            return Self::from_bytes(s);
        }
        let _ = slot;
        Self::Wire(Wire::from_slice(s))
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
impl crate::proxied::IntoProxied<LazyStr> for &str {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyStr {
        LazyStr::from(self)
    }
}
impl crate::proxied::IntoProxied<LazyStr> for String {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyStr {
        LazyStr::from(self)
    }
}
impl crate::proxied::IntoProxied<LazyStr> for ProtoString {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyStr {
        LazyStr::from(self)
    }
}
impl From<String> for LazyStr {
    fn from(s: String) -> Self {
        Self::owned(ProtoString::from(s))
    }
}
impl MapKey for LazyStr {}

#[derive(Copy, Clone, Debug)]
pub struct LazyStrView<'msg>(pub &'msg ProtoStr);

impl std::ops::Deref for LazyStrView<'_> {
    type Target = ProtoStr;
    fn deref(&self) -> &ProtoStr {
        self.0
    }
}
impl crate::internal::SealedInternal for LazyStr {}
impl crate::internal::SealedInternal for LazyStrView<'_> {}
impl crate::proxied::Proxied for LazyStr {
    type View<'msg> = LazyStrView<'msg>;
}
impl crate::proxied::AsView for LazyStr {
    type Proxied = Self;
    fn as_view(&self) -> LazyStrView<'_> {
        LazyStrView(LazyStr::as_view(self))
    }
}
impl crate::proxied::AsView for LazyStrView<'_> {
    type Proxied = LazyStr;
    fn as_view(&self) -> LazyStrView<'_> {
        *self
    }
}
impl<'msg> crate::proxied::IntoView<'msg> for LazyStrView<'msg> {
    fn into_view<'shorter>(self) -> LazyStrView<'shorter>
    where
        'msg: 'shorter,
    {
        LazyStrView(self.0)
    }
}
impl PartialEq<str> for LazyStrView<'_> {
    fn eq(&self, other: &str) -> bool {
        self.0.as_bytes() == other.as_bytes()
    }
}
impl PartialEq<&str> for LazyStrView<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_bytes() == other.as_bytes()
    }
}

impl crate::map::MapQuery<LazyStr> for LazyStr {
    fn eq_key(&self, k: &LazyStr) -> bool {
        self == k
    }
    fn to_owned_key(&self) -> LazyStr {
        self.clone()
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl crate::map::MapQuery<LazyStr> for &LazyStr {
    fn eq_key(&self, k: &LazyStr) -> bool {
        *self == k
    }
    fn to_owned_key(&self) -> LazyStr {
        (*self).clone()
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl crate::map::MapQuery<LazyStr> for &str {
    fn eq_key(&self, k: &LazyStr) -> bool {
        k.as_bytes() == self.as_bytes()
    }
    fn to_owned_key(&self) -> LazyStr {
        LazyStr::from(*self)
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}
impl crate::map::MapQuery<LazyStr> for crate::string::ProtoString {
    fn eq_key(&self, k: &LazyStr) -> bool {
        k.as_bytes() == self.as_bytes()
    }
    fn to_owned_key(&self) -> LazyStr {
        LazyStr::from(self.clone())
    }
    fn key_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// Singular `bytes`. Same cow as [`LazyStr`].
#[derive(Clone, Default)]
pub enum LazyBytes {
    #[default]
    Empty,
    Wire(Wire),
    Owned(ProtoBytes),
}

impl LazyBytes {
    /// Sharing threshold (PK-09): a parsed `bytes` field shorter than this
    /// is copied into owned storage when the parent frame is shared
    /// (`parse_bytes`); at or above it, the field windows the parent. Small
    /// fields therefore never pin a large shared frame, while the copying
    /// (`parse`) path keeps winding its private parent exactly as before.
    pub const SHARE_THRESHOLD: usize = 4096;

    pub fn owned(s: ProtoBytes) -> Self {
        if s.is_empty() {
            Self::Empty
        } else {
            Self::Owned(s)
        }
    }

    /// Parse a bytes span from the parent message bytes.
    ///
    /// Below [`Self::SHARE_THRESHOLD`] the span is copied when the parent
    /// frame is shared, so a small field never pins a large frame; at or
    /// above it, the span windows the parent. A private parent (the copying
    /// `parse` path) is always windowed: it pins nothing foreign.
    /// Generated `merge_inner` calls this with the message's wire slot.
    #[inline]
    pub fn from_parse_span(
        slot: &mut Option<Wire>,
        data: &[u8],
        rel_start: usize,
        rel_end: usize,
    ) -> Self {
        let shared = matches!(slot, Some(w) if w.is_shared());
        if !shared || rel_end.saturating_sub(rel_start) >= Self::SHARE_THRESHOLD {
            return Self::from_wire(Wire::ensure(slot, data).window(rel_start, rel_end));
        }
        Self::from_bytes(&data[rel_start..rel_end])
    }

    /// Same threshold policy as [`Self::from_parse_span`] for callers that
    /// already hold the parent [`Wire`] (map entry decode).
    #[inline]
    pub fn from_wire_span(wire: &Wire, rel_start: usize, rel_end: usize) -> Self {
        if !wire.is_shared() || rel_end.saturating_sub(rel_start) >= Self::SHARE_THRESHOLD {
            return Self::from_wire(wire.window(rel_start, rel_end));
        }
        Self::from_bytes(&wire.as_slice()[rel_start..rel_end])
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

    /// This field as a shareable buffer: a refcount clone for shared or
    /// owned backing, a copy for a privately-parsed wire window (PK-09; the
    /// one runtime method Phase 2 sends share).
    pub fn as_shared(&self) -> Bytes {
        match self {
            Self::Empty => Bytes::new(),
            Self::Wire(w) => w.as_bytes(),
            Self::Owned(s) => s.as_shared(),
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
impl crate::proxied::IntoProxied<LazyBytes> for Vec<u8> {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyBytes {
        LazyBytes::from(self)
    }
}
impl crate::proxied::IntoProxied<LazyBytes> for &[u8] {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyBytes {
        LazyBytes::from(self)
    }
}
impl crate::proxied::IntoProxied<LazyBytes> for Bytes {
    fn into_proxied(self, _private: crate::internal::Private) -> LazyBytes {
        LazyBytes::owned(ProtoBytes::from(self))
    }
}

/// View of [`LazyBytes`]. Newtype so `&[u8]` can stay [`ProtoBytes`]'s View.
#[derive(Copy, Clone, Debug)]
pub struct LazyBytesView<'msg>(pub &'msg [u8]);

impl std::ops::Deref for LazyBytesView<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.0
    }
}
impl LazyBytesView<'_> {
    pub fn as_bytes(&self) -> &[u8] {
        self.0
    }
}
impl crate::internal::SealedInternal for LazyBytes {}
impl crate::internal::SealedInternal for LazyBytesView<'_> {}
impl crate::proxied::Proxied for LazyBytes {
    type View<'msg> = LazyBytesView<'msg>;
}
impl crate::proxied::AsView for LazyBytes {
    type Proxied = Self;
    fn as_view(&self) -> LazyBytesView<'_> {
        LazyBytesView(self.as_bytes())
    }
}
impl crate::proxied::AsView for LazyBytesView<'_> {
    type Proxied = LazyBytes;
    fn as_view(&self) -> LazyBytesView<'_> {
        *self
    }
}
impl<'msg> crate::proxied::IntoView<'msg> for LazyBytesView<'msg> {
    fn into_view<'shorter>(self) -> LazyBytesView<'shorter>
    where
        'msg: 'shorter,
    {
        LazyBytesView(self.0)
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

#[cfg(test)]
mod tests {
    use super::{LazyStr, Wire};

    #[test]
    fn from_parse_span_skips_parent_arc_when_inline() {
        // hello name "ada" (3 ≤ 23): copy into ProtoString, no parent Arc.
        let data = b"ada";
        let mut slot = None;
        let s = LazyStr::from_parse_span(&mut slot, data, 0, data.len()).unwrap();
        assert_eq!(s.as_view(), "ada");
        assert!(
            slot.is_none(),
            "inline string must not Wire::ensure the parent frame"
        );
        assert!(matches!(s, LazyStr::Owned(_)));
    }

    #[test]
    fn from_parse_span_long_copies_payload_not_parent() {
        let data = [b'x'; 24];
        let mut slot = None;
        let s = LazyStr::from_parse_span(&mut slot, &data, 0, data.len()).unwrap();
        assert_eq!(s.as_bytes(), &data);
        assert!(
            slot.is_none(),
            "len > 23 copies the payload once; does not Wire::ensure the parent"
        );
        assert!(matches!(s, LazyStr::Wire(_)));
    }

    #[test]
    fn from_parse_span_medium_shares_parent_frame() {
        let mut data = vec![0u8; 163];
        data[10..34].fill(b'x');
        let mut slot = None;
        let s = LazyStr::from_parse_span(&mut slot, &data, 10, 34).unwrap();
        assert_eq!(s.as_bytes(), &[b'x'; 24]);
        assert!(
            slot.is_some(),
            "medium string in a larger message shares the parent Wire"
        );
        assert!(matches!(s, LazyStr::Wire(_)));
    }

    #[test]
    fn from_parse_span_empty_skips_parent_arc() {
        let data = b"";
        let mut slot = None;
        let s = LazyStr::from_parse_span(&mut slot, data, 0, 0).unwrap();
        assert!(s.is_empty());
        assert!(slot.is_none());
        assert!(matches!(s, LazyStr::Empty));
    }

    #[test]
    fn require_utf8_accepts_ascii_and_multibyte_rejects_invalid() {
        super::require_utf8(b"ada").unwrap();
        super::require_utf8(&[b'x'; 4096]).unwrap();
        super::require_utf8("é".as_bytes()).unwrap();
        assert!(super::require_utf8(&[0xff, 0xfe, 0xfd]).is_err());
    }

    #[test]
    fn from_parse_span_rejects_invalid_utf8() {
        let data = [0xff, 0xfe, 0xfd];
        let mut slot = None;
        assert!(LazyStr::from_parse_span(&mut slot, &data, 0, 3).is_err());
        let long = vec![0xff; 24];
        let mut slot = None;
        assert!(LazyStr::from_parse_span(&mut slot, &long, 0, 24).is_err());
    }

    #[test]
    fn from_parse_span_unchecked_keeps_non_utf8() {
        let data = [0x80];
        let mut slot = None;
        let s = LazyStr::from_parse_span_unchecked(&mut slot, &data, 0, 1);
        assert_eq!(s.as_bytes(), &[0x80]);
        assert!(slot.is_none());
        let long = vec![0x80; 24];
        let mut slot = None;
        let s = LazyStr::from_parse_span_unchecked(&mut slot, &long, 0, 24);
        assert_eq!(s.as_bytes(), &long);
        assert!(slot.is_none());
        assert!(matches!(s, LazyStr::Wire(_)));
    }

    #[test]
    fn from_span_still_inlines_after_ensure() {
        let data = b"ada";
        let w = Wire::from_slice(data);
        let s = LazyStr::from_span(&w, 0, 3);
        assert!(matches!(s, LazyStr::Owned(_)));
        assert_eq!(s.as_view(), "ada");
    }
}
