use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoProxied, IntoView, MutProxied, Proxied};
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// Empty is an 8-byte null. `Box<Vec<_>>` so unused TAT collections stay off the struct.
#[allow(clippy::box_collection)]
#[derive(Clone)]
pub struct Repeated<T>(Option<Box<Vec<T>>>);

impl<T> Default for Repeated<T> {
    #[inline]
    fn default() -> Self {
        Self(None)
    }
}

impl<T: PartialEq> PartialEq for Repeated<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<T: Eq> Eq for Repeated<T> {}

impl<T> Repeated<T> {
    #[inline]
    pub fn new() -> Self {
        Self(None)
    }

    pub fn from_vec(values: Vec<T>) -> Self {
        if values.is_empty() {
            Self(None)
        } else {
            Self(Some(Box::new(values)))
        }
    }

    #[inline]
    fn ensure(&mut self) -> &mut Vec<T> {
        self.0.get_or_insert_with(|| Box::new(Vec::new()))
    }

    pub fn push(&mut self, value: T) {
        self.ensure().push(value);
    }

    pub fn reserve(&mut self, additional: usize) {
        if additional == 0 {
            return;
        }
        self.ensure().reserve(additional);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |v| v.len())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_ref().is_none_or(|v| v.is_empty())
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.0.as_ref().and_then(|v| v.get(index))
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.0.as_mut().and_then(|v| v.get_mut(index))
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.0.as_deref().map_or(&[], |v| v.as_slice())
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.ensure().as_mut_slice()
    }

    pub fn into_vec(self) -> Vec<T> {
        self.0.map(|b| *b).unwrap_or_default()
    }
}

impl<T> Deref for Repeated<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> DerefMut for Repeated<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: fmt::Debug> fmt::Debug for Repeated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<T> From<Vec<T>> for Repeated<T> {
    fn from(v: Vec<T>) -> Self {
        Self::from_vec(v)
    }
}

impl<T> FromIterator<T> for Repeated<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
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

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.inner.get_mut(index)
    }

    pub fn push_default(&mut self) -> &mut T
    where
        T: Default,
    {
        self.inner.push(T::default());
        self.inner.last_mut().expect("just pushed")
    }

    pub fn set(&mut self, index: usize, value: T) {
        self.inner[index] = value;
    }

    pub fn extend(&mut self, iter: impl IntoIterator<Item = T>) {
        self.inner.extend(iter);
    }

    pub fn copy_from(&mut self, src: RepeatedView<'_, T>)
    where
        T: Clone,
    {
        self.inner.clear();
        self.inner.extend(src.inner.iter().cloned());
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn iter(&self) -> RepeatedIter<'_, T> {
        RepeatedIter {
            inner: self.inner.iter(),
        }
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

impl<T> ExactSizeIterator for RepeatedIter<'_, T> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<T> std::iter::FusedIterator for RepeatedIter<'_, T> {}

impl<'a, T: Copy> IntoIterator for RepeatedView<'a, T> {
    type Item = T;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter().copied()
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
        RepeatedView {
            inner: self.as_slice(),
        }
    }
}
impl<T: 'static> AsMut for Repeated<T> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        RepeatedMut {
            inner: self.ensure(),
        }
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
        Repeated::from_vec(self.inner.to_vec())
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for Vec<T> {
    fn into_proxied(self) -> Repeated<T> {
        Repeated::from_vec(self)
    }
}

/// Used by `proto!` list/map literals so a field mutator can take either a
/// repeated element or a map key/value pair.
pub trait ProtoPut<T> {
    fn proto_put(&mut self, v: T);
}

impl<T> ProtoPut<T> for RepeatedMut<'_, T> {
    fn proto_put(&mut self, v: T) {
        self.push(v);
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

#[allow(dead_code)]
struct _Hold<T>(PhantomData<T>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_eq_and_zeroed() {
        let z: Repeated<i32> = unsafe { std::mem::zeroed() };
        assert!(z.is_empty());
        assert_eq!(z, Repeated::new());
        assert_eq!(z, Repeated::from_vec(Vec::new()));
        drop(z);
    }

    #[test]
    fn push_then_clear() {
        let mut r = Repeated::new();
        r.push(1);
        r.push(2);
        assert_eq!(r.as_slice(), &[1, 2]);
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r, Repeated::new());
    }
}
