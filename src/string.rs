use crate::internal::SealedInternal;
use crate::proxied::{AsView, IntoProxied, IntoView, Proxied};
use std::fmt;
use std::ops::Deref;

/// The bytes were not valid UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf8Error;

impl fmt::Display for Utf8Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid utf-8")
    }
}

impl std::error::Error for Utf8Error {}

/// Shared immutable view of a protobuf `string` field. May contain invalid UTF-8.
#[repr(transparent)]
pub struct ProtoStr([u8]);

impl<'msg> From<&'msg str> for &'msg ProtoStr {
    fn from(s: &'msg str) -> &'msg ProtoStr {
        ProtoStr::from_str(s)
    }
}

impl ProtoStr {
    pub fn from_bytes(bytes: &[u8]) -> &Self {
        // SAFETY: transparent over [u8]
        unsafe { &*(bytes as *const [u8] as *const ProtoStr) }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> &Self {
        Self::from_bytes(s.as_bytes())
    }

    /// View over bytes that may not be UTF-8 (C++ interop / proto2).
    pub fn from_utf8_unchecked(bytes: &[u8]) -> &Self {
        Self::from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.0).map_err(|_| Utf8Error)
    }
}

impl fmt::Debug for ProtoStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_str() {
            Ok(s) => fmt::Debug::fmt(s, f),
            Err(_) => f.debug_tuple("ProtoStr").field(&&self.0).finish(),
        }
    }
}

impl PartialEq for ProtoStr {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for ProtoStr {}

impl PartialEq<str> for ProtoStr {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for ProtoStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<ProtoString> for ProtoStr {
    fn eq(&self, other: &ProtoString) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl fmt::Display for ProtoStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&String::from_utf8_lossy(self.as_bytes()), f)
    }
}

const INLINE_CAP: usize = 23;

#[derive(Clone)]
enum Repr {
    Inline { len: u8, data: [u8; INLINE_CAP] },
    Heap(Vec<u8>),
}

/// Owned protobuf `string`. May contain invalid UTF-8 (C++ interop).
#[derive(Clone)]
pub struct ProtoString(Repr);

impl Default for ProtoString {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtoString {
    pub fn new() -> Self {
        Self(Repr::Inline {
            len: 0,
            data: [0; INLINE_CAP],
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() <= INLINE_CAP {
            let mut data = [0u8; INLINE_CAP];
            data[..bytes.len()].copy_from_slice(bytes);
            Self(Repr::Inline {
                len: bytes.len() as u8,
                data,
            })
        } else {
            Self(Repr::Heap(bytes.to_vec()))
        }
    }

    pub fn as_view(&self) -> &ProtoStr {
        ProtoStr::from_bytes(self.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            Repr::Inline { len, data } => &data[..*len as usize],
            Repr::Heap(v) => v,
        }
    }

    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        self.as_view().to_str()
    }

    pub fn clear(&mut self) {
        self.0 = Repr::Inline {
            len: 0,
            data: [0; INLINE_CAP],
        };
    }

    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Repr::Inline { len, .. } => *len == 0,
            Repr::Heap(v) => v.is_empty(),
        }
    }
}

impl PartialEq for ProtoString {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl PartialEq<str> for ProtoString {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl PartialEq<&str> for ProtoString {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl Eq for ProtoString {}
impl PartialOrd for ProtoString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ProtoString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}
impl std::hash::Hash for ProtoString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl fmt::Debug for ProtoString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_view(), f)
    }
}

impl From<&str> for ProtoString {
    fn from(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }
}

impl From<String> for ProtoString {
    fn from(s: String) -> Self {
        if s.len() <= INLINE_CAP {
            Self::from_bytes(s.as_bytes())
        } else {
            Self(Repr::Heap(s.into_bytes()))
        }
    }
}

impl From<&ProtoStr> for ProtoString {
    fn from(s: &ProtoStr) -> Self {
        Self::from_bytes(s.as_bytes())
    }
}

impl Deref for ProtoString {
    type Target = ProtoStr;
    fn deref(&self) -> &ProtoStr {
        self.as_view()
    }
}

impl SealedInternal for ProtoString {}
impl Proxied for ProtoString {
    type View<'msg> = &'msg ProtoStr;
}
impl AsView for ProtoString {
    type Proxied = Self;
    fn as_view(&self) -> &ProtoStr {
        ProtoString::as_view(self)
    }
}
impl<'msg> IntoView<'msg> for &'msg ProtoStr {
    fn into_view<'shorter>(self) -> &'shorter ProtoStr
    where
        'msg: 'shorter,
    {
        self
    }
}
impl SealedInternal for &ProtoStr {}
impl AsView for &ProtoStr {
    type Proxied = ProtoString;
    fn as_view(&self) -> &ProtoStr {
        self
    }
}

impl IntoProxied<ProtoString> for &str {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self)
    }
}
impl IntoProxied<ProtoString> for String {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self)
    }
}
impl IntoProxied<ProtoString> for &String {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.as_str())
    }
}
impl IntoProxied<ProtoString> for &ProtoStr {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self)
    }
}
impl IntoProxied<ProtoString> for std::borrow::Cow<'_, str> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.as_ref())
    }
}
impl IntoProxied<ProtoString> for Box<str> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.as_ref())
    }
}
impl IntoProxied<ProtoString> for std::rc::Rc<str> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.as_ref())
    }
}
impl IntoProxied<ProtoString> for std::sync::Arc<str> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.as_ref())
    }
}
impl IntoProxied<ProtoString> for std::ffi::OsString {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.to_string_lossy().as_ref())
    }
}
impl IntoProxied<ProtoString> for &std::ffi::OsStr {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoString {
        ProtoString::from(self.to_string_lossy().as_ref())
    }
}

/// Owned protobuf `bytes`.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct ProtoBytes(Vec<u8>);

impl ProtoBytes {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn as_view(&self) -> &[u8] {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl fmt::Debug for ProtoBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProtoBytes").field(&self.0).finish()
    }
}

impl From<&[u8]> for ProtoBytes {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec())
    }
}
impl From<Vec<u8>> for ProtoBytes {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}
impl<const N: usize> From<&[u8; N]> for ProtoBytes {
    fn from(v: &[u8; N]) -> Self {
        Self(v.to_vec())
    }
}

impl SealedInternal for ProtoBytes {}
impl Proxied for ProtoBytes {
    type View<'msg> = &'msg [u8];
}
impl AsView for ProtoBytes {
    type Proxied = Self;
    fn as_view(&self) -> &[u8] {
        &self.0
    }
}
impl IntoProxied<ProtoBytes> for &[u8] {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self)
    }
}
impl IntoProxied<ProtoBytes> for Vec<u8> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self)
    }
}
impl IntoProxied<ProtoBytes> for &Vec<u8> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_slice())
    }
}
impl<const N: usize> IntoProxied<ProtoBytes> for &[u8; N] {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_slice())
    }
}
impl IntoProxied<ProtoBytes> for std::borrow::Cow<'_, [u8]> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_ref())
    }
}
impl IntoProxied<ProtoBytes> for Box<[u8]> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_ref())
    }
}
impl IntoProxied<ProtoBytes> for std::rc::Rc<[u8]> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_ref())
    }
}
impl IntoProxied<ProtoBytes> for std::sync::Arc<[u8]> {
    fn into_proxied(self, _private: crate::internal::Private) -> ProtoBytes {
        ProtoBytes::from(self.as_ref())
    }
}

impl SealedInternal for &[u8] {}
impl AsView for &[u8] {
    type Proxied = ProtoBytes;
    fn as_view(&self) -> &[u8] {
        self
    }
}
impl<'msg> IntoView<'msg> for &'msg [u8] {
    fn into_view<'shorter>(self) -> &'shorter [u8]
    where
        'msg: 'shorter,
    {
        self
    }
}
