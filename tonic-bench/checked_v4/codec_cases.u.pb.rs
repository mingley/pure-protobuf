const _: () = ::protobuf::__internal::assert_compatible_gencode_version("4.35.1-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Empty_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Empty {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Empty>
}

impl ::protobuf::Message for Empty {
  type MessageView<'msg> = EmptyView<'msg>;
  type MessageMut<'msg> = EmptyMut<'msg>;
}

impl ::std::default::Default for Empty {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Empty {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Empty` is `Sync` because it does not implement interior mutability.
//    Neither does `EmptyMut`.
unsafe impl ::std::marker::Sync for Empty {}

// SAFETY:
// - `Empty` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Empty {}

impl ::protobuf::Proxied for Empty {
  type View<'msg> = EmptyView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Empty {}

impl ::protobuf::MutProxied for Empty {
  type Mut<'msg> = EmptyMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct EmptyView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Empty>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EmptyView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for EmptyView<'msg> {
  type Message = Empty;
}

impl ::std::fmt::Debug for EmptyView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for EmptyView<'_> {
  fn default() -> EmptyView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Empty>> for EmptyView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Empty>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EmptyView<'msg> {

  pub fn to_owned(&self) -> Empty {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

}

// SAFETY:
// - `EmptyView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for EmptyView<'_> {}

// SAFETY:
// - `EmptyView` is `Send` because while its alive a `EmptyMut` cannot.
// - `EmptyView` does not use thread-local data.
unsafe impl ::std::marker::Send for EmptyView<'_> {}

impl<'msg> ::protobuf::AsView for EmptyView<'msg> {
  type Proxied = Empty;
  fn as_view(&self) -> ::protobuf::View<'msg, Empty> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EmptyView<'msg> {
  fn into_view<'shorter>(self) -> EmptyView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Empty> for EmptyView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Empty {
    let mut dst = Empty::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Empty> for EmptyMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Empty {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Empty {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EmptyView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EmptyMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct EmptyMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Empty>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EmptyMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for EmptyMut<'msg> {
  type Message = Empty;
}

impl ::std::fmt::Debug for EmptyMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Empty>> for EmptyMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Empty>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EmptyMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Empty> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Empty {
    ::protobuf::AsView::as_view(self).to_owned()
  }

}

// SAFETY:
// - `EmptyMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for EmptyMut<'_> {}

// SAFETY:
// - `EmptyMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for EmptyMut<'_> {}

impl<'msg> ::protobuf::AsView for EmptyMut<'msg> {
  type Proxied = Empty;
  fn as_view(&self) -> ::protobuf::View<'_, Empty> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EmptyMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Empty>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for EmptyMut<'msg> {
  type MutProxied = Empty;
  fn as_mut(&mut self) -> EmptyMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for EmptyMut<'msg> {
  fn into_mut<'shorter>(self) -> EmptyMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Empty {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Empty> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> EmptyView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> EmptyMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

}  // impl Empty

impl ::std::ops::Drop for Empty {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Empty {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Empty {
  type Proxied = Self;
  fn as_view(&self) -> EmptyView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Empty {
  type MutProxied = Self;
  fn as_mut(&mut self) -> EmptyMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Empty {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Empty_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Empty_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Empty_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Empty {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Empty {
  type Msg = Empty;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Empty> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Empty {
  type Msg = Empty;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Empty> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for EmptyMut<'_> {
  type Msg = Empty;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Empty> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EmptyMut<'_> {
  type Msg = Empty;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Empty> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EmptyView<'_> {
  type Msg = Empty;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Empty> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for EmptyMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Id_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Id {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Id>
}

impl ::protobuf::Message for Id {
  type MessageView<'msg> = IdView<'msg>;
  type MessageMut<'msg> = IdMut<'msg>;
}

impl ::std::default::Default for Id {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Id {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Id` is `Sync` because it does not implement interior mutability.
//    Neither does `IdMut`.
unsafe impl ::std::marker::Sync for Id {}

// SAFETY:
// - `Id` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Id {}

impl ::protobuf::Proxied for Id {
  type View<'msg> = IdView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Id {}

impl ::protobuf::MutProxied for Id {
  type Mut<'msg> = IdMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct IdView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Id>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for IdView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for IdView<'msg> {
  type Message = Id;
}

impl ::std::fmt::Debug for IdView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for IdView<'_> {
  fn default() -> IdView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Id>> for IdView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Id>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> IdView<'msg> {

  pub fn to_owned(&self) -> Id {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // id: optional int64
  pub fn id(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `IdView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for IdView<'_> {}

// SAFETY:
// - `IdView` is `Send` because while its alive a `IdMut` cannot.
// - `IdView` does not use thread-local data.
unsafe impl ::std::marker::Send for IdView<'_> {}

impl<'msg> ::protobuf::AsView for IdView<'msg> {
  type Proxied = Id;
  fn as_view(&self) -> ::protobuf::View<'msg, Id> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for IdView<'msg> {
  fn into_view<'shorter>(self) -> IdView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Id> for IdView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Id {
    let mut dst = Id::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Id> for IdMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Id {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Id {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for IdView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for IdMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct IdMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Id>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for IdMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for IdMut<'msg> {
  type Message = Id;
}

impl ::std::fmt::Debug for IdMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Id>> for IdMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Id>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> IdMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Id> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Id {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `IdMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for IdMut<'_> {}

// SAFETY:
// - `IdMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for IdMut<'_> {}

impl<'msg> ::protobuf::AsView for IdMut<'msg> {
  type Proxied = Id;
  fn as_view(&self) -> ::protobuf::View<'_, Id> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for IdMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Id>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for IdMut<'msg> {
  type MutProxied = Id;
  fn as_mut(&mut self) -> IdMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for IdMut<'msg> {
  fn into_mut<'shorter>(self) -> IdMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Id {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Id> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> IdView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> IdMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

}  // impl Id

impl ::std::ops::Drop for Id {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Id {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Id {
  type Proxied = Self;
  fn as_view(&self) -> IdView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Id {
  type MutProxied = Self;
  fn as_mut(&mut self) -> IdMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Id {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Id_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$+P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Id_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Id_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Id {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Id {
  type Msg = Id;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Id> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Id {
  type Msg = Id;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Id> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for IdMut<'_> {
  type Msg = Id;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Id> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for IdMut<'_> {
  type Msg = Id;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Id> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for IdView<'_> {
  type Msg = Id;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Id> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for IdMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Scalars_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Scalars {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Scalars>
}

impl ::protobuf::Message for Scalars {
  type MessageView<'msg> = ScalarsView<'msg>;
  type MessageMut<'msg> = ScalarsMut<'msg>;
}

impl ::std::default::Default for Scalars {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Scalars {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Scalars` is `Sync` because it does not implement interior mutability.
//    Neither does `ScalarsMut`.
unsafe impl ::std::marker::Sync for Scalars {}

// SAFETY:
// - `Scalars` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Scalars {}

impl ::protobuf::Proxied for Scalars {
  type View<'msg> = ScalarsView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Scalars {}

impl ::protobuf::MutProxied for Scalars {
  type Mut<'msg> = ScalarsMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct ScalarsView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Scalars>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ScalarsView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for ScalarsView<'msg> {
  type Message = Scalars;
}

impl ::std::fmt::Debug for ScalarsView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for ScalarsView<'_> {
  fn default() -> ScalarsView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Scalars>> for ScalarsView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Scalars>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ScalarsView<'msg> {

  pub fn to_owned(&self) -> Scalars {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // id: optional int64
  pub fn id(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // seq: optional uint32
  pub fn seq(self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        1, (0u32).into()
      ).try_into().unwrap()
    }
  }

  // ok: optional bool
  pub fn ok(self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        2, (false).into()
      ).try_into().unwrap()
    }
  }

  // status: optional enum cases.Status
  pub fn status(self) -> super::Status {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        3, (super::Status::Unspecified).into()
      ).try_into().unwrap()
    }
  }

  // ts: optional int64
  pub fn ts(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        4, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // lat: optional double
  pub fn lat(self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        5, (0f64).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `ScalarsView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for ScalarsView<'_> {}

// SAFETY:
// - `ScalarsView` is `Send` because while its alive a `ScalarsMut` cannot.
// - `ScalarsView` does not use thread-local data.
unsafe impl ::std::marker::Send for ScalarsView<'_> {}

impl<'msg> ::protobuf::AsView for ScalarsView<'msg> {
  type Proxied = Scalars;
  fn as_view(&self) -> ::protobuf::View<'msg, Scalars> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ScalarsView<'msg> {
  fn into_view<'shorter>(self) -> ScalarsView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Scalars> for ScalarsView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Scalars {
    let mut dst = Scalars::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Scalars> for ScalarsMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Scalars {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Scalars {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ScalarsView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ScalarsMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct ScalarsMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Scalars>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ScalarsMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for ScalarsMut<'msg> {
  type Message = Scalars;
}

impl ::std::fmt::Debug for ScalarsMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Scalars>> for ScalarsMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Scalars>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ScalarsMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Scalars> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Scalars {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // seq: optional uint32
  pub fn seq(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        1, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_seq(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        1, val.into()
      )
    }
  }

  // ok: optional bool
  pub fn ok(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        2, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ok(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        2, val.into()
      )
    }
  }

  // status: optional enum cases.Status
  pub fn status(&self) -> super::Status {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        3, (super::Status::Unspecified).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_status(&mut self, val: super::Status) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        3, val.into()
      )
    }
  }

  // ts: optional int64
  pub fn ts(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        4, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ts(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        4, val.into()
      )
    }
  }

  // lat: optional double
  pub fn lat(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        5, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_lat(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        5, val.into()
      )
    }
  }

}

// SAFETY:
// - `ScalarsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for ScalarsMut<'_> {}

// SAFETY:
// - `ScalarsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for ScalarsMut<'_> {}

impl<'msg> ::protobuf::AsView for ScalarsMut<'msg> {
  type Proxied = Scalars;
  fn as_view(&self) -> ::protobuf::View<'_, Scalars> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ScalarsMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Scalars>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for ScalarsMut<'msg> {
  type MutProxied = Scalars;
  fn as_mut(&mut self) -> ScalarsMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for ScalarsMut<'msg> {
  fn into_mut<'shorter>(self) -> ScalarsMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Scalars {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Scalars> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> ScalarsView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> ScalarsMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // seq: optional uint32
  pub fn seq(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        1, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_seq(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        1, val.into()
      )
    }
  }

  // ok: optional bool
  pub fn ok(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        2, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ok(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        2, val.into()
      )
    }
  }

  // status: optional enum cases.Status
  pub fn status(&self) -> super::Status {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        3, (super::Status::Unspecified).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_status(&mut self, val: super::Status) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        3, val.into()
      )
    }
  }

  // ts: optional int64
  pub fn ts(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        4, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ts(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        4, val.into()
      )
    }
  }

  // lat: optional double
  pub fn lat(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        5, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_lat(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        5, val.into()
      )
    }
  }

}  // impl Scalars

impl ::std::ops::Drop for Scalars {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Scalars {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Scalars {
  type Proxied = Self;
  fn as_view(&self) -> ScalarsView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Scalars {
  type MutProxied = Self;
  fn as_mut(&mut self) -> ScalarsMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Scalars {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Scalars_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$+P)P/P.P+P P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Scalars_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Scalars_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Scalars {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Scalars {
  type Msg = Scalars;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Scalars> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Scalars {
  type Msg = Scalars;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Scalars> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ScalarsMut<'_> {
  type Msg = Scalars;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Scalars> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ScalarsMut<'_> {
  type Msg = Scalars;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Scalars> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ScalarsView<'_> {
  type Msg = Scalars;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Scalars> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ScalarsMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Name_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Name {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Name>
}

impl ::protobuf::Message for Name {
  type MessageView<'msg> = NameView<'msg>;
  type MessageMut<'msg> = NameMut<'msg>;
}

impl ::std::default::Default for Name {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Name {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Name` is `Sync` because it does not implement interior mutability.
//    Neither does `NameMut`.
unsafe impl ::std::marker::Sync for Name {}

// SAFETY:
// - `Name` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Name {}

impl ::protobuf::Proxied for Name {
  type View<'msg> = NameView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Name {}

impl ::protobuf::MutProxied for Name {
  type Mut<'msg> = NameMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct NameView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Name>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NameView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for NameView<'msg> {
  type Message = Name;
}

impl ::std::fmt::Debug for NameView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for NameView<'_> {
  fn default() -> NameView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Name>> for NameView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Name>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NameView<'msg> {

  pub fn to_owned(&self) -> Name {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // name: optional string
  pub fn name(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

}

// SAFETY:
// - `NameView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for NameView<'_> {}

// SAFETY:
// - `NameView` is `Send` because while its alive a `NameMut` cannot.
// - `NameView` does not use thread-local data.
unsafe impl ::std::marker::Send for NameView<'_> {}

impl<'msg> ::protobuf::AsView for NameView<'msg> {
  type Proxied = Name;
  fn as_view(&self) -> ::protobuf::View<'msg, Name> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NameView<'msg> {
  fn into_view<'shorter>(self) -> NameView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Name> for NameView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Name {
    let mut dst = Name::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Name> for NameMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Name {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Name {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NameView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NameMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct NameMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Name>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NameMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for NameMut<'msg> {
  type Message = Name;
}

impl ::std::fmt::Debug for NameMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Name>> for NameMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Name>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NameMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Name> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Name {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // name: optional string
  pub fn name(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_name(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}

// SAFETY:
// - `NameMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for NameMut<'_> {}

// SAFETY:
// - `NameMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for NameMut<'_> {}

impl<'msg> ::protobuf::AsView for NameMut<'msg> {
  type Proxied = Name;
  fn as_view(&self) -> ::protobuf::View<'_, Name> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NameMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Name>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for NameMut<'msg> {
  type MutProxied = Name;
  fn as_mut(&mut self) -> NameMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for NameMut<'msg> {
  fn into_mut<'shorter>(self) -> NameMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Name {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Name> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> NameView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> NameMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // name: optional string
  pub fn name(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_name(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}  // impl Name

impl ::std::ops::Drop for Name {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Name {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Name {
  type Proxied = Self;
  fn as_view(&self) -> NameView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Name {
  type MutProxied = Self;
  fn as_mut(&mut self) -> NameMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Name {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Name_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$M1P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Name_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Name_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Name {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Name {
  type Msg = Name;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Name> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Name {
  type Msg = Name;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Name> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NameMut<'_> {
  type Msg = Name;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Name> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NameMut<'_> {
  type Msg = Name;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Name> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NameView<'_> {
  type Msg = Name;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Name> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NameMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Blob_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Blob {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Blob>
}

impl ::protobuf::Message for Blob {
  type MessageView<'msg> = BlobView<'msg>;
  type MessageMut<'msg> = BlobMut<'msg>;
}

impl ::std::default::Default for Blob {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Blob {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Blob` is `Sync` because it does not implement interior mutability.
//    Neither does `BlobMut`.
unsafe impl ::std::marker::Sync for Blob {}

// SAFETY:
// - `Blob` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Blob {}

impl ::protobuf::Proxied for Blob {
  type View<'msg> = BlobView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Blob {}

impl ::protobuf::MutProxied for Blob {
  type Mut<'msg> = BlobMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct BlobView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Blob>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BlobView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for BlobView<'msg> {
  type Message = Blob;
}

impl ::std::fmt::Debug for BlobView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for BlobView<'_> {
  fn default() -> BlobView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Blob>> for BlobView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Blob>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BlobView<'msg> {

  pub fn to_owned(&self) -> Blob {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // payload: optional bytes
  pub fn payload(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

}

// SAFETY:
// - `BlobView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for BlobView<'_> {}

// SAFETY:
// - `BlobView` is `Send` because while its alive a `BlobMut` cannot.
// - `BlobView` does not use thread-local data.
unsafe impl ::std::marker::Send for BlobView<'_> {}

impl<'msg> ::protobuf::AsView for BlobView<'msg> {
  type Proxied = Blob;
  fn as_view(&self) -> ::protobuf::View<'msg, Blob> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BlobView<'msg> {
  fn into_view<'shorter>(self) -> BlobView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Blob> for BlobView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Blob {
    let mut dst = Blob::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Blob> for BlobMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Blob {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Blob {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BlobView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BlobMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct BlobMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Blob>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BlobMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for BlobMut<'msg> {
  type Message = Blob;
}

impl ::std::fmt::Debug for BlobMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Blob>> for BlobMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Blob>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BlobMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Blob> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Blob {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // payload: optional bytes
  pub fn payload(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_payload(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}

// SAFETY:
// - `BlobMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for BlobMut<'_> {}

// SAFETY:
// - `BlobMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for BlobMut<'_> {}

impl<'msg> ::protobuf::AsView for BlobMut<'msg> {
  type Proxied = Blob;
  fn as_view(&self) -> ::protobuf::View<'_, Blob> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BlobMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Blob>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for BlobMut<'msg> {
  type MutProxied = Blob;
  fn as_mut(&mut self) -> BlobMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for BlobMut<'msg> {
  fn into_mut<'shorter>(self) -> BlobMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Blob {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Blob> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> BlobView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> BlobMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // payload: optional bytes
  pub fn payload(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_payload(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}  // impl Blob

impl ::std::ops::Drop for Blob {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Blob {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Blob {
  type Proxied = Self;
  fn as_view(&self) -> BlobView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Blob {
  type MutProxied = Self;
  fn as_mut(&mut self) -> BlobMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Blob {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Blob_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$0P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Blob_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Blob_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Blob {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Blob {
  type Msg = Blob;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Blob> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Blob {
  type Msg = Blob;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Blob> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for BlobMut<'_> {
  type Msg = Blob;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Blob> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BlobMut<'_> {
  type Msg = Blob;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Blob> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BlobView<'_> {
  type Msg = Blob;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Blob> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for BlobMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Meta_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Meta {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Meta>
}

impl ::protobuf::Message for Meta {
  type MessageView<'msg> = MetaView<'msg>;
  type MessageMut<'msg> = MetaMut<'msg>;
}

impl ::std::default::Default for Meta {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Meta {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Meta` is `Sync` because it does not implement interior mutability.
//    Neither does `MetaMut`.
unsafe impl ::std::marker::Sync for Meta {}

// SAFETY:
// - `Meta` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Meta {}

impl ::protobuf::Proxied for Meta {
  type View<'msg> = MetaView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Meta {}

impl ::protobuf::MutProxied for Meta {
  type Mut<'msg> = MetaMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct MetaView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Meta>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for MetaView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for MetaView<'msg> {
  type Message = Meta;
}

impl ::std::fmt::Debug for MetaView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for MetaView<'_> {
  fn default() -> MetaView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Meta>> for MetaView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Meta>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> MetaView<'msg> {

  pub fn to_owned(&self) -> Meta {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // id: optional int64
  pub fn id(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // ts: optional int64
  pub fn ts(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        1, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // trace: optional string
  pub fn trace(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

}

// SAFETY:
// - `MetaView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for MetaView<'_> {}

// SAFETY:
// - `MetaView` is `Send` because while its alive a `MetaMut` cannot.
// - `MetaView` does not use thread-local data.
unsafe impl ::std::marker::Send for MetaView<'_> {}

impl<'msg> ::protobuf::AsView for MetaView<'msg> {
  type Proxied = Meta;
  fn as_view(&self) -> ::protobuf::View<'msg, Meta> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for MetaView<'msg> {
  fn into_view<'shorter>(self) -> MetaView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Meta> for MetaView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Meta {
    let mut dst = Meta::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Meta> for MetaMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Meta {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Meta {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for MetaView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for MetaMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct MetaMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Meta>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for MetaMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for MetaMut<'msg> {
  type Message = Meta;
}

impl ::std::fmt::Debug for MetaMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Meta>> for MetaMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Meta>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> MetaMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Meta> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Meta {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // ts: optional int64
  pub fn ts(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        1, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ts(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        1, val.into()
      )
    }
  }

  // trace: optional string
  pub fn trace(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_trace(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

}

// SAFETY:
// - `MetaMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for MetaMut<'_> {}

// SAFETY:
// - `MetaMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for MetaMut<'_> {}

impl<'msg> ::protobuf::AsView for MetaMut<'msg> {
  type Proxied = Meta;
  fn as_view(&self) -> ::protobuf::View<'_, Meta> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for MetaMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Meta>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for MetaMut<'msg> {
  type MutProxied = Meta;
  fn as_mut(&mut self) -> MetaMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for MetaMut<'msg> {
  fn into_mut<'shorter>(self) -> MetaMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Meta {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Meta> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> MetaView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> MetaMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // ts: optional int64
  pub fn ts(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        1, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_ts(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        1, val.into()
      )
    }
  }

  // trace: optional string
  pub fn trace(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_trace(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

}  // impl Meta

impl ::std::ops::Drop for Meta {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Meta {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Meta {
  type Proxied = Self;
  fn as_view(&self) -> MetaView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Meta {
  type MutProxied = Self;
  fn as_mut(&mut self) -> MetaMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Meta {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Meta_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$+P+P1X");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Meta_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Meta_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Meta {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Meta {
  type Msg = Meta;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Meta> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Meta {
  type Msg = Meta;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Meta> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for MetaMut<'_> {
  type Msg = Meta;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Meta> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for MetaMut<'_> {
  type Msg = Meta;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Meta> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for MetaView<'_> {
  type Msg = Meta;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Meta> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for MetaMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Envelope_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Envelope {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Envelope>
}

impl ::protobuf::Message for Envelope {
  type MessageView<'msg> = EnvelopeView<'msg>;
  type MessageMut<'msg> = EnvelopeMut<'msg>;
}

impl ::std::default::Default for Envelope {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Envelope {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Envelope` is `Sync` because it does not implement interior mutability.
//    Neither does `EnvelopeMut`.
unsafe impl ::std::marker::Sync for Envelope {}

// SAFETY:
// - `Envelope` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Envelope {}

impl ::protobuf::Proxied for Envelope {
  type View<'msg> = EnvelopeView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Envelope {}

impl ::protobuf::MutProxied for Envelope {
  type Mut<'msg> = EnvelopeMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct EnvelopeView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Envelope>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EnvelopeView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for EnvelopeView<'msg> {
  type Message = Envelope;
}

impl ::std::fmt::Debug for EnvelopeView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for EnvelopeView<'_> {
  fn default() -> EnvelopeView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Envelope>> for EnvelopeView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Envelope>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EnvelopeView<'msg> {

  pub fn to_owned(&self) -> Envelope {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // meta: optional message cases.Meta
  pub fn has_meta(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn meta_opt(self) -> ::std::option::Option<super::MetaView<'msg>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(self) -> super::MetaView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(0)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }

  // body: optional string
  pub fn body(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

}

// SAFETY:
// - `EnvelopeView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for EnvelopeView<'_> {}

// SAFETY:
// - `EnvelopeView` is `Send` because while its alive a `EnvelopeMut` cannot.
// - `EnvelopeView` does not use thread-local data.
unsafe impl ::std::marker::Send for EnvelopeView<'_> {}

impl<'msg> ::protobuf::AsView for EnvelopeView<'msg> {
  type Proxied = Envelope;
  fn as_view(&self) -> ::protobuf::View<'msg, Envelope> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EnvelopeView<'msg> {
  fn into_view<'shorter>(self) -> EnvelopeView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Envelope> for EnvelopeView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Envelope {
    let mut dst = Envelope::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Envelope> for EnvelopeMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Envelope {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Envelope {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EnvelopeView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EnvelopeMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct EnvelopeMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Envelope>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EnvelopeMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for EnvelopeMut<'msg> {
  type Message = Envelope;
}

impl ::std::fmt::Debug for EnvelopeMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Envelope>> for EnvelopeMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Envelope>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EnvelopeMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Envelope> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Envelope {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // meta: optional message cases.Meta
  pub fn has_meta(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_meta(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn meta_opt(&self) -> ::std::option::Option<super::MetaView<'_>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(&self) -> super::MetaView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(0)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }
  pub fn meta_mut(&mut self) -> super::MetaMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         0, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_meta(&mut self,
    val: impl ::protobuf::IntoProxied<super::Meta>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val
      );
    }
  }

  // body: optional string
  pub fn body(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_body(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

}

// SAFETY:
// - `EnvelopeMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for EnvelopeMut<'_> {}

// SAFETY:
// - `EnvelopeMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for EnvelopeMut<'_> {}

impl<'msg> ::protobuf::AsView for EnvelopeMut<'msg> {
  type Proxied = Envelope;
  fn as_view(&self) -> ::protobuf::View<'_, Envelope> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EnvelopeMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Envelope>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for EnvelopeMut<'msg> {
  type MutProxied = Envelope;
  fn as_mut(&mut self) -> EnvelopeMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for EnvelopeMut<'msg> {
  fn into_mut<'shorter>(self) -> EnvelopeMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Envelope {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Envelope> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> EnvelopeView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> EnvelopeMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // meta: optional message cases.Meta
  pub fn has_meta(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_meta(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn meta_opt(&self) -> ::std::option::Option<super::MetaView<'_>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(&self) -> super::MetaView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(0)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }
  pub fn meta_mut(&mut self) -> super::MetaMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         0, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_meta(&mut self,
    val: impl ::protobuf::IntoProxied<super::Meta>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val
      );
    }
  }

  // body: optional string
  pub fn body(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_body(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

}  // impl Envelope

impl ::std::ops::Drop for Envelope {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Envelope {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Envelope {
  type Proxied = Self;
  fn as_view(&self) -> EnvelopeView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Envelope {
  type MutProxied = Self;
  fn as_mut(&mut self) -> EnvelopeMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Envelope {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Envelope_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$31X");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Envelope_msg_init.0, &[<super::Meta as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Envelope_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Envelope {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Envelope {
  type Msg = Envelope;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Envelope> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Envelope {
  type Msg = Envelope;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Envelope> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for EnvelopeMut<'_> {
  type Msg = Envelope;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Envelope> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EnvelopeMut<'_> {
  type Msg = Envelope;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Envelope> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EnvelopeView<'_> {
  type Msg = Envelope;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Envelope> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for EnvelopeMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Node_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Node {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Node>
}

impl ::protobuf::Message for Node {
  type MessageView<'msg> = NodeView<'msg>;
  type MessageMut<'msg> = NodeMut<'msg>;
}

impl ::std::default::Default for Node {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Node {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Node` is `Sync` because it does not implement interior mutability.
//    Neither does `NodeMut`.
unsafe impl ::std::marker::Sync for Node {}

// SAFETY:
// - `Node` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Node {}

impl ::protobuf::Proxied for Node {
  type View<'msg> = NodeView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Node {}

impl ::protobuf::MutProxied for Node {
  type Mut<'msg> = NodeMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct NodeView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Node>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NodeView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for NodeView<'msg> {
  type Message = Node;
}

impl ::std::fmt::Debug for NodeView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for NodeView<'_> {
  fn default() -> NodeView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Node>> for NodeView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Node>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NodeView<'msg> {

  pub fn to_owned(&self) -> Node {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // n: optional int32
  pub fn n(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // child: optional message cases.Node
  pub fn has_child(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn child_opt(self) -> ::std::option::Option<super::NodeView<'msg>> {
    self.has_child().then(|| self.child())
  }
  pub fn child(self) -> super::NodeView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::NodeView::default())
  }

}

// SAFETY:
// - `NodeView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for NodeView<'_> {}

// SAFETY:
// - `NodeView` is `Send` because while its alive a `NodeMut` cannot.
// - `NodeView` does not use thread-local data.
unsafe impl ::std::marker::Send for NodeView<'_> {}

impl<'msg> ::protobuf::AsView for NodeView<'msg> {
  type Proxied = Node;
  fn as_view(&self) -> ::protobuf::View<'msg, Node> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NodeView<'msg> {
  fn into_view<'shorter>(self) -> NodeView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Node> for NodeView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Node {
    let mut dst = Node::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Node> for NodeMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Node {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Node {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NodeView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NodeMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct NodeMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Node>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NodeMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for NodeMut<'msg> {
  type Message = Node;
}

impl ::std::fmt::Debug for NodeMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Node>> for NodeMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Node>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NodeMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Node> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Node {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // n: optional int32
  pub fn n(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_n(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        0, val.into()
      )
    }
  }

  // child: optional message cases.Node
  pub fn has_child(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_child(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn child_opt(&self) -> ::std::option::Option<super::NodeView<'_>> {
    self.has_child().then(|| self.child())
  }
  pub fn child(&self) -> super::NodeView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::NodeView::default())
  }
  pub fn child_mut(&mut self) -> super::NodeMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         1, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_child(&mut self,
    val: impl ::protobuf::IntoProxied<super::Node>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val
      );
    }
  }

}

// SAFETY:
// - `NodeMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for NodeMut<'_> {}

// SAFETY:
// - `NodeMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for NodeMut<'_> {}

impl<'msg> ::protobuf::AsView for NodeMut<'msg> {
  type Proxied = Node;
  fn as_view(&self) -> ::protobuf::View<'_, Node> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NodeMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Node>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for NodeMut<'msg> {
  type MutProxied = Node;
  fn as_mut(&mut self) -> NodeMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for NodeMut<'msg> {
  fn into_mut<'shorter>(self) -> NodeMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Node {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Node> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> NodeView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> NodeMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // n: optional int32
  pub fn n(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_n(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        0, val.into()
      )
    }
  }

  // child: optional message cases.Node
  pub fn has_child(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_child(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn child_opt(&self) -> ::std::option::Option<super::NodeView<'_>> {
    self.has_child().then(|| self.child())
  }
  pub fn child(&self) -> super::NodeView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::NodeView::default())
  }
  pub fn child_mut(&mut self) -> super::NodeMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         1, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_child(&mut self,
    val: impl ::protobuf::IntoProxied<super::Node>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val
      );
    }
  }

}  // impl Node

impl ::std::ops::Drop for Node {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Node {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Node {
  type Proxied = Self;
  fn as_view(&self) -> NodeView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Node {
  type MutProxied = Self;
  fn as_mut(&mut self) -> NodeMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Node {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Node_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$(P3");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Node_msg_init.0, &[super::cases__Node_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Node_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Node {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Node {
  type Msg = Node;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Node> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Node {
  type Msg = Node;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Node> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NodeMut<'_> {
  type Msg = Node;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Node> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NodeMut<'_> {
  type Msg = Node;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Node> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NodeView<'_> {
  type Msg = Node;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Node> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NodeMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Ids_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Ids {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Ids>
}

impl ::protobuf::Message for Ids {
  type MessageView<'msg> = IdsView<'msg>;
  type MessageMut<'msg> = IdsMut<'msg>;
}

impl ::std::default::Default for Ids {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Ids {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Ids` is `Sync` because it does not implement interior mutability.
//    Neither does `IdsMut`.
unsafe impl ::std::marker::Sync for Ids {}

// SAFETY:
// - `Ids` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Ids {}

impl ::protobuf::Proxied for Ids {
  type View<'msg> = IdsView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Ids {}

impl ::protobuf::MutProxied for Ids {
  type Mut<'msg> = IdsMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct IdsView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Ids>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for IdsView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for IdsView<'msg> {
  type Message = Ids;
}

impl ::std::fmt::Debug for IdsView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for IdsView<'_> {
  fn default() -> IdsView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Ids>> for IdsView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Ids>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> IdsView<'msg> {

  pub fn to_owned(&self) -> Ids {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // ids: repeated int64
  pub fn ids(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

}

// SAFETY:
// - `IdsView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for IdsView<'_> {}

// SAFETY:
// - `IdsView` is `Send` because while its alive a `IdsMut` cannot.
// - `IdsView` does not use thread-local data.
unsafe impl ::std::marker::Send for IdsView<'_> {}

impl<'msg> ::protobuf::AsView for IdsView<'msg> {
  type Proxied = Ids;
  fn as_view(&self) -> ::protobuf::View<'msg, Ids> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for IdsView<'msg> {
  fn into_view<'shorter>(self) -> IdsView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Ids> for IdsView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Ids {
    let mut dst = Ids::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Ids> for IdsMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Ids {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Ids {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for IdsView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for IdsMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct IdsMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Ids>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for IdsMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for IdsMut<'msg> {
  type Message = Ids;
}

impl ::std::fmt::Debug for IdsMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Ids>> for IdsMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Ids>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> IdsMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Ids> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Ids {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // ids: repeated int64
  pub fn ids(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn ids_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        0,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_ids(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `IdsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for IdsMut<'_> {}

// SAFETY:
// - `IdsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for IdsMut<'_> {}

impl<'msg> ::protobuf::AsView for IdsMut<'msg> {
  type Proxied = Ids;
  fn as_view(&self) -> ::protobuf::View<'_, Ids> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for IdsMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Ids>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for IdsMut<'msg> {
  type MutProxied = Ids;
  fn as_mut(&mut self) -> IdsMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for IdsMut<'msg> {
  fn into_mut<'shorter>(self) -> IdsMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Ids {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Ids> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> IdsView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> IdsMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // ids: repeated int64
  pub fn ids(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn ids_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        0,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_ids(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl Ids

impl ::std::ops::Drop for Ids {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Ids {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Ids {
  type Proxied = Self;
  fn as_view(&self) -> IdsView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Ids {
  type MutProxied = Self;
  fn as_mut(&mut self) -> IdsMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Ids {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Ids_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$N?");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Ids_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Ids_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Ids {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Ids {
  type Msg = Ids;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Ids> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Ids {
  type Msg = Ids;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Ids> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for IdsMut<'_> {
  type Msg = Ids;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Ids> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for IdsMut<'_> {
  type Msg = Ids;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Ids> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for IdsView<'_> {
  type Msg = Ids;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Ids> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for IdsMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Tags_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Tags {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Tags>
}

impl ::protobuf::Message for Tags {
  type MessageView<'msg> = TagsView<'msg>;
  type MessageMut<'msg> = TagsMut<'msg>;
}

impl ::std::default::Default for Tags {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Tags {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Tags` is `Sync` because it does not implement interior mutability.
//    Neither does `TagsMut`.
unsafe impl ::std::marker::Sync for Tags {}

// SAFETY:
// - `Tags` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Tags {}

impl ::protobuf::Proxied for Tags {
  type View<'msg> = TagsView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Tags {}

impl ::protobuf::MutProxied for Tags {
  type Mut<'msg> = TagsMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct TagsView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Tags>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TagsView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for TagsView<'msg> {
  type Message = Tags;
}

impl ::std::fmt::Debug for TagsView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for TagsView<'_> {
  fn default() -> TagsView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Tags>> for TagsView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Tags>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TagsView<'msg> {

  pub fn to_owned(&self) -> Tags {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // tags: repeated string
  pub fn tags(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

}

// SAFETY:
// - `TagsView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for TagsView<'_> {}

// SAFETY:
// - `TagsView` is `Send` because while its alive a `TagsMut` cannot.
// - `TagsView` does not use thread-local data.
unsafe impl ::std::marker::Send for TagsView<'_> {}

impl<'msg> ::protobuf::AsView for TagsView<'msg> {
  type Proxied = Tags;
  fn as_view(&self) -> ::protobuf::View<'msg, Tags> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TagsView<'msg> {
  fn into_view<'shorter>(self) -> TagsView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Tags> for TagsView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Tags {
    let mut dst = Tags::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Tags> for TagsMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Tags {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Tags {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TagsView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TagsMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct TagsMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Tags>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TagsMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for TagsMut<'msg> {
  type Message = Tags;
}

impl ::std::fmt::Debug for TagsMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Tags>> for TagsMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Tags>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TagsMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Tags> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Tags {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn tags_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        0,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_tags(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `TagsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for TagsMut<'_> {}

// SAFETY:
// - `TagsMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for TagsMut<'_> {}

impl<'msg> ::protobuf::AsView for TagsMut<'msg> {
  type Proxied = Tags;
  fn as_view(&self) -> ::protobuf::View<'_, Tags> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TagsMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Tags>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for TagsMut<'msg> {
  type MutProxied = Tags;
  fn as_mut(&mut self) -> TagsMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for TagsMut<'msg> {
  fn into_mut<'shorter>(self) -> TagsMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Tags {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Tags> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> TagsView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> TagsMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn tags_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        0,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_tags(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl Tags

impl ::std::ops::Drop for Tags {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Tags {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Tags {
  type Proxied = Self;
  fn as_view(&self) -> TagsView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Tags {
  type MutProxied = Self;
  fn as_mut(&mut self) -> TagsMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Tags {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Tags_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$ME");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Tags_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Tags_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Tags {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Tags {
  type Msg = Tags;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Tags> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Tags {
  type Msg = Tags;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Tags> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for TagsMut<'_> {
  type Msg = Tags;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Tags> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TagsMut<'_> {
  type Msg = Tags;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Tags> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TagsView<'_> {
  type Msg = Tags;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Tags> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for TagsMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Headers_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Headers {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Headers>
}

impl ::protobuf::Message for Headers {
  type MessageView<'msg> = HeadersView<'msg>;
  type MessageMut<'msg> = HeadersMut<'msg>;
}

impl ::std::default::Default for Headers {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Headers {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Headers` is `Sync` because it does not implement interior mutability.
//    Neither does `HeadersMut`.
unsafe impl ::std::marker::Sync for Headers {}

// SAFETY:
// - `Headers` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Headers {}

impl ::protobuf::Proxied for Headers {
  type View<'msg> = HeadersView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Headers {}

impl ::protobuf::MutProxied for Headers {
  type Mut<'msg> = HeadersMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct HeadersView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Headers>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for HeadersView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for HeadersView<'msg> {
  type Message = Headers;
}

impl ::std::fmt::Debug for HeadersView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for HeadersView<'_> {
  fn default() -> HeadersView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Headers>> for HeadersView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Headers>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> HeadersView<'msg> {

  pub fn to_owned(&self) -> Headers {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // h: repeated message cases.Headers.HEntry
  pub fn h(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

}

// SAFETY:
// - `HeadersView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for HeadersView<'_> {}

// SAFETY:
// - `HeadersView` is `Send` because while its alive a `HeadersMut` cannot.
// - `HeadersView` does not use thread-local data.
unsafe impl ::std::marker::Send for HeadersView<'_> {}

impl<'msg> ::protobuf::AsView for HeadersView<'msg> {
  type Proxied = Headers;
  fn as_view(&self) -> ::protobuf::View<'msg, Headers> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for HeadersView<'msg> {
  fn into_view<'shorter>(self) -> HeadersView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Headers> for HeadersView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Headers {
    let mut dst = Headers::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Headers> for HeadersMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Headers {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Headers {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for HeadersView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for HeadersMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct HeadersMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Headers>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for HeadersMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for HeadersMut<'msg> {
  type Message = Headers;
}

impl ::std::fmt::Debug for HeadersMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Headers>> for HeadersMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Headers>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> HeadersMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Headers> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Headers {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // h: repeated message cases.Headers.HEntry
  pub fn h(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn h_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          0, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_h(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `HeadersMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for HeadersMut<'_> {}

// SAFETY:
// - `HeadersMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for HeadersMut<'_> {}

impl<'msg> ::protobuf::AsView for HeadersMut<'msg> {
  type Proxied = Headers;
  fn as_view(&self) -> ::protobuf::View<'_, Headers> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for HeadersMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Headers>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for HeadersMut<'msg> {
  type MutProxied = Headers;
  fn as_mut(&mut self) -> HeadersMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for HeadersMut<'msg> {
  fn into_mut<'shorter>(self) -> HeadersMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Headers {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Headers> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> HeadersView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> HeadersMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // h: repeated message cases.Headers.HEntry
  pub fn h(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn h_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          0, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_h(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl Headers

impl ::std::ops::Drop for Headers {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Headers {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Headers {
  type Proxied = Self;
  fn as_view(&self) -> HeadersView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Headers {
  type MutProxied = Self;
  fn as_mut(&mut self) -> HeadersMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Headers {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Headers_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$G");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Headers_msg_init.0, &[<super::headers::HEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Headers_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Headers {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Headers {
  type Msg = Headers;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Headers> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Headers {
  type Msg = Headers;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Headers> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for HeadersMut<'_> {
  type Msg = Headers;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Headers> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for HeadersMut<'_> {
  type Msg = Headers;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Headers> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for HeadersView<'_> {
  type Msg = Headers;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Headers> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for HeadersMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod headers {// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Headers__HEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct HEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for HEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::headers::cases__Headers__HEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X1X");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::headers::cases__Headers__HEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::headers::cases__Headers__HEntry_msg_init.0)
      }).0
    }
  }
}

}  // pub mod headers


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Result_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Result {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Result>
}

impl ::protobuf::Message for Result {
  type MessageView<'msg> = ResultView<'msg>;
  type MessageMut<'msg> = ResultMut<'msg>;
}

impl ::std::default::Default for Result {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Result {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Result` is `Sync` because it does not implement interior mutability.
//    Neither does `ResultMut`.
unsafe impl ::std::marker::Sync for Result {}

// SAFETY:
// - `Result` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Result {}

impl ::protobuf::Proxied for Result {
  type View<'msg> = ResultView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Result {}

impl ::protobuf::MutProxied for Result {
  type Mut<'msg> = ResultMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct ResultView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Result>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ResultView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for ResultView<'msg> {
  type Message = Result;
}

impl ::std::fmt::Debug for ResultView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for ResultView<'_> {
  fn default() -> ResultView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Result>> for ResultView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Result>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ResultView<'msg> {

  pub fn to_owned(&self) -> Result {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // ok: optional string
  pub fn has_ok(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn ok_opt(self) -> ::std::option::Option<&'msg ::protobuf::ProtoStr> {
    self.has_ok().then(|| self.ok())
  }
  pub fn ok(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // err: optional string
  pub fn has_err(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn err_opt(self) -> ::std::option::Option<&'msg ::protobuf::ProtoStr> {
    self.has_err().then(|| self.err())
  }
  pub fn err(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  pub fn kind(self) -> super::result::KindOneof<'msg> {
    match self.kind_case() {
      super::result::KindCase::Ok =>
          super::result::KindOneof::Ok(self.ok()),
      super::result::KindCase::Err =>
          super::result::KindOneof::Err(self.err()),
      _ => super::result::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(self) -> super::result::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::result::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `ResultView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for ResultView<'_> {}

// SAFETY:
// - `ResultView` is `Send` because while its alive a `ResultMut` cannot.
// - `ResultView` does not use thread-local data.
unsafe impl ::std::marker::Send for ResultView<'_> {}

impl<'msg> ::protobuf::AsView for ResultView<'msg> {
  type Proxied = Result;
  fn as_view(&self) -> ::protobuf::View<'msg, Result> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ResultView<'msg> {
  fn into_view<'shorter>(self) -> ResultView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Result> for ResultView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Result {
    let mut dst = Result::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Result> for ResultMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Result {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Result {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ResultView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ResultMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct ResultMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Result>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ResultMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for ResultMut<'msg> {
  type Message = Result;
}

impl ::std::fmt::Debug for ResultMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Result>> for ResultMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Result>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ResultMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Result> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Result {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // ok: optional string
  pub fn has_ok(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_ok(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn ok_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_ok().then(|| self.ok())
  }
  pub fn ok(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_ok(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

  // err: optional string
  pub fn has_err(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_err(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn err_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_err().then(|| self.err())
  }
  pub fn err(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_err(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  pub fn kind(&self) -> super::result::KindOneof<'_> {
    match &self.kind_case() {
      super::result::KindCase::Ok =>
          super::result::KindOneof::Ok(self.ok()),
      super::result::KindCase::Err =>
          super::result::KindOneof::Err(self.err()),
      _ => super::result::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(&self) -> super::result::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::result::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `ResultMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for ResultMut<'_> {}

// SAFETY:
// - `ResultMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for ResultMut<'_> {}

impl<'msg> ::protobuf::AsView for ResultMut<'msg> {
  type Proxied = Result;
  fn as_view(&self) -> ::protobuf::View<'_, Result> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ResultMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Result>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for ResultMut<'msg> {
  type MutProxied = Result;
  fn as_mut(&mut self) -> ResultMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for ResultMut<'msg> {
  fn into_mut<'shorter>(self) -> ResultMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Result {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Result> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> ResultView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> ResultMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // ok: optional string
  pub fn has_ok(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_ok(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn ok_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_ok().then(|| self.ok())
  }
  pub fn ok(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_ok(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

  // err: optional string
  pub fn has_err(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_err(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn err_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_err().then(|| self.err())
  }
  pub fn err(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_err(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  pub fn kind(&self) -> super::result::KindOneof<'_> {
    match &self.kind_case() {
      super::result::KindCase::Ok =>
          super::result::KindOneof::Ok(self.ok()),
      super::result::KindCase::Err =>
          super::result::KindOneof::Err(self.err()),
      _ => super::result::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(&self) -> super::result::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::result::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}  // impl Result

impl ::std::ops::Drop for Result {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Result {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Result {
  type Proxied = Self;
  fn as_view(&self) -> ResultView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Result {
  type MutProxied = Self;
  fn as_mut(&mut self) -> ResultMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Result {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Result_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$M11^!|#");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Result_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Result_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Result {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Result {
  type Msg = Result;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Result> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Result {
  type Msg = Result;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Result> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ResultMut<'_> {
  type Msg = Result;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Result> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ResultMut<'_> {
  type Msg = Result;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Result> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ResultView<'_> {
  type Msg = Result;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Result> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ResultMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod result {

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
#[repr(u32)]
pub enum KindOneof<'msg> {
  Ok(&'msg ::protobuf::ProtoStr) = 1,
  Err(&'msg ::protobuf::ProtoStr) = 2,

  not_set(std::marker::PhantomData<&'msg ()>) = 0
}
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(dead_code)]
pub enum KindCase {
  Ok = 1,
  Err = 2,

  not_set = 0
}

impl KindCase {
  #[allow(dead_code)]
  pub(crate) fn try_from(v: u32) -> ::std::option::Option<KindCase> {
    match v {
      0 => Some(KindCase::not_set),
      1 => Some(KindCase::Ok),
      2 => Some(KindCase::Err),
      _ => None
    }
  }
}
}  // pub mod result


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Rpc_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Rpc {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Rpc>
}

impl ::protobuf::Message for Rpc {
  type MessageView<'msg> = RpcView<'msg>;
  type MessageMut<'msg> = RpcMut<'msg>;
}

impl ::std::default::Default for Rpc {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Rpc {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Rpc` is `Sync` because it does not implement interior mutability.
//    Neither does `RpcMut`.
unsafe impl ::std::marker::Sync for Rpc {}

// SAFETY:
// - `Rpc` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Rpc {}

impl ::protobuf::Proxied for Rpc {
  type View<'msg> = RpcView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Rpc {}

impl ::protobuf::MutProxied for Rpc {
  type Mut<'msg> = RpcMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct RpcView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Rpc>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for RpcView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for RpcView<'msg> {
  type Message = Rpc;
}

impl ::std::fmt::Debug for RpcView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for RpcView<'_> {
  fn default() -> RpcView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Rpc>> for RpcView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Rpc>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> RpcView<'msg> {

  pub fn to_owned(&self) -> Rpc {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // id: optional int64
  pub fn id(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // method: optional string
  pub fn method(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // path: optional string
  pub fn path(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // user: optional string
  pub fn user(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        3, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // meta: optional message cases.Meta
  pub fn has_meta(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn meta_opt(self) -> ::std::option::Option<super::MetaView<'msg>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(self) -> super::MetaView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }

  // ids: repeated int64
  pub fn ids(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        5
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // tags: repeated string
  pub fn tags(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        6
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // headers: repeated message cases.Rpc.HeadersEntry
  pub fn headers(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(7)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // extra: optional bytes
  pub fn extra(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        8, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

}

// SAFETY:
// - `RpcView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for RpcView<'_> {}

// SAFETY:
// - `RpcView` is `Send` because while its alive a `RpcMut` cannot.
// - `RpcView` does not use thread-local data.
unsafe impl ::std::marker::Send for RpcView<'_> {}

impl<'msg> ::protobuf::AsView for RpcView<'msg> {
  type Proxied = Rpc;
  fn as_view(&self) -> ::protobuf::View<'msg, Rpc> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for RpcView<'msg> {
  fn into_view<'shorter>(self) -> RpcView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Rpc> for RpcView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Rpc {
    let mut dst = Rpc::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Rpc> for RpcMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Rpc {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Rpc {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for RpcView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for RpcMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct RpcMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Rpc>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for RpcMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for RpcMut<'msg> {
  type Message = Rpc;
}

impl ::std::fmt::Debug for RpcMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Rpc>> for RpcMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Rpc>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> RpcMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Rpc> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Rpc {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // method: optional string
  pub fn method(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_method(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  // path: optional string
  pub fn path(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_path(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // user: optional string
  pub fn user(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        3, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_user(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        3,
        val);
    }
  }

  // meta: optional message cases.Meta
  pub fn has_meta(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn clear_meta(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        4
      );
    }
  }
  pub fn meta_opt(&self) -> ::std::option::Option<super::MetaView<'_>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(&self) -> super::MetaView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }
  pub fn meta_mut(&mut self) -> super::MetaMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         4, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_meta(&mut self,
    val: impl ::protobuf::IntoProxied<super::Meta>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        val
      );
    }
  }

  // ids: repeated int64
  pub fn ids(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        5
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn ids_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        5,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_ids(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        src);
    }
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        6
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn tags_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        6,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_tags(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        6,
        src);
    }
  }

  // headers: repeated message cases.Rpc.HeadersEntry
  pub fn headers(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(7)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn headers_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          7, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_headers(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        7,
        src);
    }
  }

  // extra: optional bytes
  pub fn extra(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        8, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_extra(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        8,
        val);
    }
  }

}

// SAFETY:
// - `RpcMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for RpcMut<'_> {}

// SAFETY:
// - `RpcMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for RpcMut<'_> {}

impl<'msg> ::protobuf::AsView for RpcMut<'msg> {
  type Proxied = Rpc;
  fn as_view(&self) -> ::protobuf::View<'_, Rpc> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for RpcMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Rpc>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for RpcMut<'msg> {
  type MutProxied = Rpc;
  fn as_mut(&mut self) -> RpcMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for RpcMut<'msg> {
  fn into_mut<'shorter>(self) -> RpcMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Rpc {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Rpc> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> RpcView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> RpcMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // id: optional int64
  pub fn id(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        0, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_id(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        0, val.into()
      )
    }
  }

  // method: optional string
  pub fn method(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_method(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  // path: optional string
  pub fn path(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_path(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // user: optional string
  pub fn user(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        3, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_user(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        3,
        val);
    }
  }

  // meta: optional message cases.Meta
  pub fn has_meta(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn clear_meta(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        4
      );
    }
  }
  pub fn meta_opt(&self) -> ::std::option::Option<super::MetaView<'_>> {
    self.has_meta().then(|| self.meta())
  }
  pub fn meta(&self) -> super::MetaView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::MetaView::default())
  }
  pub fn meta_mut(&mut self) -> super::MetaMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         4, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_meta(&mut self,
    val: impl ::protobuf::IntoProxied<super::Meta>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        val
      );
    }
  }

  // ids: repeated int64
  pub fn ids(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        5
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn ids_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        5,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_ids(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        src);
    }
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        6
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn tags_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        6,
        self.inner.arena()
      ).expect("alloc should not fail");
      ::protobuf::RepeatedMut::from_inner(
        ::protobuf::__internal::Private,
        ::protobuf::__internal::runtime::InnerRepeatedMut::new(
          raw_array, self.inner.arena(),
        ),
      )
    }
  }
  pub fn set_tags(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        6,
        src);
    }
  }

  // headers: repeated message cases.Rpc.HeadersEntry
  pub fn headers(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(7)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn headers_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          7, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_headers(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        7,
        src);
    }
  }

  // extra: optional bytes
  pub fn extra(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        8, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_extra(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        8,
        val);
    }
  }

}  // impl Rpc

impl ::std::ops::Drop for Rpc {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Rpc {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Rpc {
  type Proxied = Self;
  fn as_view(&self) -> RpcView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Rpc {
  type MutProxied = Self;
  fn as_mut(&mut self) -> RpcMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Rpc {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::cases__Rpc_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$N+P1X1X1X3?ETG0P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::cases__Rpc_msg_init.0, &[<super::Meta as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::rpc::HeadersEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::cases__Rpc_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Rpc {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Rpc {
  type Msg = Rpc;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Rpc> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Rpc {
  type Msg = Rpc;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Rpc> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for RpcMut<'_> {
  type Msg = Rpc;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Rpc> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for RpcMut<'_> {
  type Msg = Rpc;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Rpc> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for RpcView<'_> {
  type Msg = Rpc;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Rpc> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for RpcMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod rpc {// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut cases__Rpc__HeadersEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct HeadersEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for HeadersEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::rpc::cases__Rpc__HeadersEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X1X");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::rpc::cases__Rpc__HeadersEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::rpc::cases__Rpc__HeadersEntry_msg_init.0)
      }).0
    }
  }
}

}  // pub mod rpc


#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Status(i32);

#[allow(non_upper_case_globals)]
impl Status {
  pub const Unspecified: Status = Status(0);
  pub const Ok: Status = Status(1);
  pub const Err: Status = Status(2);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "Unspecified",
      1 => "Ok",
      2 => "Err",
      _ => return None
    })
  }
}

impl ::std::convert::From<Status> for i32 {
  fn from(val: Status) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for Status {
  fn from(val: i32) -> Status {
    Self(val)
  }
}

impl ::std::default::Default for Status {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for Status {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "Status::{}", constant_name)
    } else {
      write!(f, "Status::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for Status {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for Status {}

impl ::protobuf::Proxied for Status {
  type View<'a> = Status;
}

impl ::protobuf::AsView for Status {
  type Proxied = Status;

  fn as_view(&self) -> Status {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Status {
  fn into_view<'shorter>(self) -> Status where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for Status {
  const NAME: &'static str = "Status";

  fn is_known(value: i32) -> bool {
    matches!(value, 0|1|2)
  }
}

impl ::protobuf::__internal::EntityType for Status {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}


