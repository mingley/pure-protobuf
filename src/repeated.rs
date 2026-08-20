use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// A `repeated` field of `T`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Repeated<T>(Vec<T>);

impl<T> Repeated<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn from_vec(values: Vec<T>) -> Self {
        Self(values)
    }

    pub fn push(&mut self, value: T) {
        self.0.push(value);
    }

    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.0.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.0.get_mut(index)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<T> Deref for Repeated<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> DerefMut for Repeated<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Repeated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<T> From<Vec<T>> for Repeated<T> {
    fn from(v: Vec<T>) -> Self {
        Self(v)
    }
}

impl<T> FromIterator<T> for Repeated<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// View of a repeated field.
pub struct RepeatedView<'msg, T> {
    inner: &'msg [T],
}

impl<T> Clone for RepeatedView<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for RepeatedView<'_, T> {}

impl<'msg, T> RepeatedView<'msg, T> {
    pub fn from_slice(inner: &'msg [T]) -> Self {
        Self { inner }
    }

    pub fn len(self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(self) -> bool {
        self.inner.is_empty()
    }

    pub fn get(self, index: usize) -> Option<&'msg T> {
        self.inner.get(index)
    }

    pub fn iter(self) -> RepeatedIter<'msg, T> {
        RepeatedIter {
            inner: self.inner.iter(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for RepeatedView<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
    }
}

/// Mutable proxy of a repeated field.
pub struct RepeatedMut<'msg, T> {
    inner: &'msg mut Vec<T>,
}

impl<'msg, T> RepeatedMut<'msg, T> {
    pub fn from_vec(inner: &'msg mut Vec<T>) -> Self {
        Self { inner }
    }

    pub fn push(&mut self, value: T) {
        self.inner.push(value);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.inner.get(index)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView {
            inner: self.inner.as_slice(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for RepeatedMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
    }
}

pub struct RepeatedIter<'msg, T> {
    inner: std::slice::Iter<'msg, T>,
}

impl<'msg, T> Iterator for RepeatedIter<'msg, T> {
    type Item = &'msg T;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T: 'static> SealedInternal for Repeated<T> {}
impl<T: 'static> Proxied for Repeated<T> {
    type View<'msg> = RepeatedView<'msg, T>;
}
impl<T: 'static> MutProxied for Repeated<T> {
    type Mut<'msg> = RepeatedMut<'msg, T>;
}
impl<T: 'static> AsView for Repeated<T> {
    type Proxied = Self;
    fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView { inner: &self.0 }
    }
}
impl<T: 'static> AsMut for Repeated<T> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        RepeatedMut { inner: &mut self.0 }
    }
}

impl<T> SealedInternal for RepeatedView<'_, T> {}
impl<T: 'static> AsView for RepeatedView<'_, T> {
    type Proxied = Repeated<T>;
    fn as_view(&self) -> RepeatedView<'_, T> {
        *self
    }
}
impl<'msg, T: 'static> IntoView<'msg> for RepeatedView<'msg, T> {
    fn into_view<'shorter>(self) -> RepeatedView<'shorter, T>
    where
        'msg: 'shorter,
    {
        RepeatedView { inner: self.inner }
    }
}

impl<T> SealedInternal for RepeatedMut<'_, T> {}
impl<T: 'static> AsView for RepeatedMut<'_, T> {
    type Proxied = Repeated<T>;
    fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView { inner: self.inner }
    }
}
impl<T: 'static> AsMut for RepeatedMut<'_, T> {
    type MutProxied = Repeated<T>;
    fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        RepeatedMut { inner: self.inner }
    }
}
impl<'msg, T: 'static> IntoView<'msg> for RepeatedMut<'msg, T> {
    fn into_view<'shorter>(self) -> RepeatedView<'shorter, T>
    where
        'msg: 'shorter,
    {
        RepeatedView { inner: self.inner }
    }
}
impl<'msg, T: 'static> IntoMut<'msg> for RepeatedMut<'msg, T> {
    fn into_mut<'shorter>(self) -> RepeatedMut<'shorter, T>
    where
        'msg: 'shorter,
    {
        RepeatedMut { inner: self.inner }
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for RepeatedView<'_, T>
where
    T: Clone,
{
    fn into_proxied(self) -> Repeated<T> {
        Repeated(self.inner.to_vec())
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for Vec<T> {
    fn into_proxied(self) -> Repeated<T> {
        Repeated(self)
    }
}

/// Types allowed as a simple field, repeated element, or map value.
pub trait Singular: Proxied + SealedInternal {}

impl Singular for i32 {}
impl Singular for i64 {}
impl Singular for u32 {}
impl Singular for u64 {}
impl Singular for f32 {}
impl Singular for f64 {}
impl Singular for bool {}
impl Singular for crate::string::ProtoString {}
impl Singular for crate::string::ProtoBytes {}

// silence unused PhantomData if we add later
#[allow(dead_code)]
struct _Hold<T>(PhantomData<T>);
