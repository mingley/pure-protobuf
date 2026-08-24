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

    pub fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView {
            inner: self.as_slice(),
            raw: None,
        }
    }

    pub fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        RepeatedMut {
            inner: Some(self.ensure()),
            raw: None,
            arena: None,
        }
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
    raw: Option<*const crate::runtime::RawArrayInner>,
}

impl<T> Clone for RepeatedView<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for RepeatedView<'_, T> {}

impl<'msg, T> RepeatedView<'msg, T> {
    pub fn from_slice(inner: &'msg [T]) -> Self {
        Self { inner, raw: None }
    }

    #[doc(hidden)]
    pub unsafe fn from_raw_ptr(raw: *const crate::runtime::RawArrayInner) -> Self {
        Self {
            inner: &[],
            raw: Some(raw),
        }
    }

    #[doc(hidden)]
    pub unsafe fn from_raw(
        _private: crate::internal::Private,
        raw: crate::runtime::RawRepeatedField,
    ) -> Self {
        unsafe { Self::from_raw_ptr(raw) }
    }

    pub fn len(self) -> usize {
        if let Some(raw) = self.raw {
            unsafe { (*raw).items.borrow().len() }
        } else {
            self.inner.len()
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: usize) -> Option<crate::proxied::View<'msg, T>>
    where
        T: crate::proxied::Proxied + 'static,
    {
        if let Some(raw) = self.raw {
            return unsafe { crate::runtime::kernel_repeated_get::<T>(raw, index) };
        }
        self.inner.get(index).map(crate::proxied::AsView::as_view)
    }

    pub fn iter(self) -> RepeatedIter<'msg, T> {
        RepeatedIter {
            inner: self.inner.iter(),
            raw: self.raw,
            raw_i: 0,
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
    inner: Option<&'msg mut Vec<T>>,
    raw: Option<*const crate::runtime::RawArrayInner>,
    arena: Option<&'msg crate::runtime::Arena>,
}

impl<'msg, T> RepeatedMut<'msg, T> {
    pub fn from_vec(inner: &'msg mut Vec<T>) -> Self {
        Self {
            inner: Some(inner),
            raw: None,
            arena: None,
        }
    }

    #[doc(hidden)]
    pub fn from_raw_inner(raw: *const crate::runtime::RawArrayInner) -> Self {
        Self {
            inner: None,
            raw: Some(raw),
            arena: None,
        }
    }

    #[doc(hidden)]
    pub unsafe fn from_inner(
        _private: crate::internal::Private,
        inner: crate::runtime::InnerRepeatedMut<'msg>,
    ) -> Self {
        Self {
            inner: None,
            raw: Some(inner.raw),
            arena: Some(inner.arena),
        }
    }

    pub fn push(&mut self, value: impl crate::proxied::IntoProxied<T>)
    where
        T: 'static,
    {
        let value = crate::proxied::IntoProxied::into_proxied(value, crate::internal::Private);
        if let Some(v) = self.inner.as_mut() {
            v.push(value);
        } else if let Some(raw) = self.raw {
            crate::runtime::kernel_array_push(raw, value, self.arena);
        }
    }

    pub fn len(&self) -> usize {
        if let Some(v) = self.inner.as_ref() {
            v.len()
        } else if let Some(raw) = self.raw {
            unsafe { (*raw).items.borrow().len() }
        } else {
            0
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<crate::proxied::View<'_, T>>
    where
        T: crate::proxied::Proxied + 'static,
    {
        self.as_view().get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<crate::proxied::Mut<'_, T>>
    where
        T: crate::proxied::MutProxied + 'static,
    {
        if let Some(v) = self.inner.as_deref_mut() {
            return v.get_mut(index).map(crate::proxied::AsMut::as_mut);
        }
        if let (Some(raw), Some(arena)) = (self.raw, self.arena) {
            return unsafe { crate::runtime::kernel_repeated_get_mut::<T>(raw, index, arena) };
        }
        None
    }

    pub fn push_default(&mut self) -> crate::proxied::Mut<'_, T>
    where
        T: Default + crate::proxied::MutProxied + 'static,
    {
        self.push(T::default());
        let i = self.len() - 1;
        self.get_mut(i).expect("just pushed")
    }

    pub fn set(&mut self, index: usize, value: impl crate::proxied::IntoProxied<T>)
    where
        T: 'static,
    {
        let value = crate::proxied::IntoProxied::into_proxied(value, crate::internal::Private);
        if let Some(v) = self.inner.as_mut() {
            v[index] = value;
        } else if let Some(raw) = self.raw {
            unsafe { crate::runtime::kernel_repeated_set(raw, index, value) };
        }
    }

    pub fn extend(&mut self, iter: impl IntoIterator<Item = T>)
    where
        T: 'static,
    {
        for item in iter {
            self.push(item);
        }
    }

    pub fn copy_from(&mut self, src: RepeatedView<'_, T>)
    where
        T: crate::proxied::Proxied + 'static,
        for<'a> crate::proxied::View<'a, T>: crate::proxied::IntoProxied<T>,
    {
        self.clear();
        for i in 0..src.len() {
            if let Some(v) = src.get(i) {
                self.push(v);
            }
        }
    }

    pub fn clear(&mut self) {
        if let Some(v) = self.inner.as_mut() {
            v.clear();
        } else if let Some(raw) = self.raw {
            unsafe {
                (*raw).items.borrow_mut().clear();
                (*raw).strs.borrow_mut().clear();
            }
        }
    }

    pub fn iter(&self) -> RepeatedIter<'_, T> {
        self.as_view().iter()
    }

    pub fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView {
            inner: self.inner.as_ref().map(|v| v.as_slice()).unwrap_or(&[]),
            raw: self.raw,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for RepeatedMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.as_ref() {
            Some(v) => fmt::Debug::fmt(*v, f),
            None => f
                .debug_struct("RepeatedMut")
                .field("raw", &self.raw)
                .finish(),
        }
    }
}

pub struct RepeatedIter<'msg, T> {
    inner: std::slice::Iter<'msg, T>,
    raw: Option<*const crate::runtime::RawArrayInner>,
    raw_i: usize,
}

impl<'msg, T: crate::proxied::Proxied + 'static> Iterator for RepeatedIter<'msg, T> {
    type Item = crate::proxied::View<'msg, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(raw) = self.raw {
            let v = unsafe { crate::runtime::kernel_repeated_get::<T>(raw, self.raw_i) };
            if v.is_some() {
                self.raw_i += 1;
            }
            return v;
        }
        self.inner.next().map(crate::proxied::AsView::as_view)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        if let Some(raw) = self.raw {
            let n = unsafe { (*raw).items.borrow().len() };
            let rem = n.saturating_sub(self.raw_i);
            (rem, Some(rem))
        } else {
            self.inner.size_hint()
        }
    }
}

impl<T: crate::proxied::Proxied + 'static> ExactSizeIterator for RepeatedIter<'_, T> {
    fn len(&self) -> usize {
        self.size_hint().0
    }
}

impl<T: crate::proxied::Proxied + 'static> std::iter::FusedIterator for RepeatedIter<'_, T> {}

impl<'a, T: crate::proxied::Proxied + 'static> IntoIterator for RepeatedView<'a, T> {
    type Item = crate::proxied::View<'a, T>;
    type IntoIter = RepeatedIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: crate::proxied::Proxied + 'static> IntoIterator for RepeatedMut<'a, T> {
    type Item = crate::proxied::View<'a, T>;
    type IntoIter = RepeatedIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        let inner = if let Some(v) = self.inner {
            v.iter()
        } else {
            let e: &[T] = &[];
            e.iter()
        };
        RepeatedIter {
            inner,
            raw: self.raw,
            raw_i: 0,
        }
    }
}

impl<'a, 'b, T: crate::proxied::Proxied + 'static> IntoIterator for &'b RepeatedMut<'a, T>
where
    'a: 'b,
{
    type Item = crate::proxied::View<'b, T>;
    type IntoIter = RepeatedIter<'b, T>;
    fn into_iter(self) -> Self::IntoIter {
        RepeatedView {
            inner: self.inner.as_ref().map(|v| v.as_slice()).unwrap_or(&[]),
            raw: self.raw,
        }
        .iter()
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
        Repeated::as_view(self)
    }
}
impl<T: 'static> AsMut for Repeated<T> {
    type MutProxied = Self;
    fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        Repeated::as_mut(self)
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
        RepeatedView {
            inner: self.inner,
            raw: self.raw,
        }
    }
}

impl<T> SealedInternal for RepeatedMut<'_, T> {}
impl<T: 'static> AsView for RepeatedMut<'_, T> {
    type Proxied = Repeated<T>;
    fn as_view(&self) -> RepeatedView<'_, T> {
        RepeatedView {
            inner: self.inner.as_ref().map(|v| v.as_slice()).unwrap_or(&[]),
            raw: self.raw,
        }
    }
}
impl<T: 'static> AsMut for RepeatedMut<'_, T> {
    type MutProxied = Repeated<T>;
    fn as_mut(&mut self) -> RepeatedMut<'_, T> {
        RepeatedMut {
            inner: self.inner.as_deref_mut(),
            raw: self.raw,
            arena: self.arena,
        }
    }
}
impl<'msg, T: 'static> IntoView<'msg> for RepeatedMut<'msg, T> {
    fn into_view<'shorter>(self) -> RepeatedView<'shorter, T>
    where
        'msg: 'shorter,
    {
        RepeatedView {
            inner: self.inner.map(|v| v.as_slice()).unwrap_or(&[]),
            raw: self.raw,
        }
    }
}
impl<'msg, T: 'static> IntoMut<'msg> for RepeatedMut<'msg, T> {
    fn into_mut<'shorter>(self) -> RepeatedMut<'shorter, T>
    where
        'msg: 'shorter,
    {
        RepeatedMut {
            inner: self.inner,
            raw: self.raw,
            arena: self.arena,
        }
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for RepeatedView<'_, T>
where
    T: crate::proxied::Proxied + Clone,
    for<'a> crate::proxied::View<'a, T>: crate::proxied::IntoProxied<T>,
{
    fn into_proxied(self, private: crate::internal::Private) -> Repeated<T> {
        if self.raw.is_none() {
            return Repeated::from_vec(self.inner.to_vec());
        }
        Repeated::from_vec(
            self.iter()
                .map(|v| crate::proxied::IntoProxied::into_proxied(v, private))
                .collect(),
        )
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for RepeatedMut<'_, T>
where
    T: crate::proxied::Proxied + Clone,
    for<'a> crate::proxied::View<'a, T>: crate::proxied::IntoProxied<T>,
{
    fn into_proxied(self, private: crate::internal::Private) -> Repeated<T> {
        self.as_view().into_proxied(private)
    }
}

impl<T: 'static> IntoProxied<Repeated<T>> for Vec<T> {
    fn into_proxied(self, _private: crate::internal::Private) -> Repeated<T> {
        Repeated::from_vec(self)
    }
}

impl<T: 'static, U: IntoProxied<T>, const N: usize> IntoProxied<Repeated<T>>
    for std::array::IntoIter<U, N>
{
    fn into_proxied(self, private: crate::internal::Private) -> Repeated<T> {
        Repeated::from_vec(self.map(|u| u.into_proxied(private)).collect())
    }
}

impl<T: 'static, U: IntoProxied<T>> IntoProxied<Repeated<T>> for std::vec::IntoIter<U> {
    fn into_proxied(self, private: crate::internal::Private) -> Repeated<T> {
        Repeated::from_vec(self.map(|u| u.into_proxied(private)).collect())
    }
}

/// Used by `proto!` list/map literals so a field mutator can take either a
/// repeated element or a map key/value pair.
pub trait ProtoPut<T> {
    fn proto_put(&mut self, v: T);
}

impl<'msg, T> RepeatedMut<'msg, T> {
    pub fn proto_put(&mut self, v: T)
    where
        T: 'static,
    {
        self.push(v);
    }
}

impl<T: 'static> ProtoPut<T> for RepeatedMut<'_, T> {
    fn proto_put(&mut self, v: T) {
        RepeatedMut::proto_put(self, v);
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
