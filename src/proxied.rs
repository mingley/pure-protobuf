use crate::internal::SealedInternal;

/// A type that can be accessed through a reference-like proxy.
pub trait Proxied: SealedInternal + AsView<Proxied = Self> + Sized + 'static {
    type View<'msg>: AsView<Proxied = Self> + IntoView<'msg>;
}

/// A type that can be accessed through a mutable proxy.
pub trait MutProxied: SealedInternal + Proxied + AsMut<MutProxied = Self> + 'static {
    type Mut<'msg>: AsMut<MutProxied = Self> + IntoMut<'msg> + IntoView<'msg>;
}

pub type View<'msg, T> = <T as Proxied>::View<'msg>;
pub type Mut<'msg, T> = <T as MutProxied>::Mut<'msg>;

pub trait AsView: SealedInternal {
    type Proxied: Proxied;
    fn as_view(&self) -> View<'_, Self::Proxied>;
}

pub trait IntoView<'msg>: SealedInternal + AsView {
    fn into_view<'shorter>(self) -> View<'shorter, Self::Proxied>
    where
        'msg: 'shorter;
}

pub trait AsMut: SealedInternal {
    type MutProxied: MutProxied;
    fn as_mut(&mut self) -> Mut<'_, Self::MutProxied>;
}

pub trait IntoMut<'msg>: SealedInternal + AsMut {
    fn into_mut<'shorter>(self) -> Mut<'shorter, Self::MutProxied>
    where
        'msg: 'shorter;
}

/// A value-to-`Proxied` conversion that consumes the input.
pub trait IntoProxied<T> {
    fn into_proxied(self) -> T;
}

impl<T> IntoProxied<T> for T {
    fn into_proxied(self) -> T {
        self
    }
}

macro_rules! impl_copy_proxied {
    ($($t:ty),* $(,)?) => {
        $(
            impl SealedInternal for $t {}
            impl Proxied for $t {
                type View<'msg> = $t;
            }
            impl AsView for $t {
                type Proxied = Self;
                fn as_view(&self) -> $t {
                    *self
                }
            }
            impl<'msg> IntoView<'msg> for $t {
                fn into_view<'shorter>(self) -> $t
                where
                    'msg: 'shorter,
                {
                    self
                }
            }
        )*
    };
}

impl_copy_proxied!(i32, i64, u32, u64, f32, f64, bool);
