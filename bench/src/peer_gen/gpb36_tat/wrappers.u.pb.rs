// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__DoubleValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct DoubleValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<DoubleValue>
}

impl ::protobuf::Message for DoubleValue {
  type MessageView<'msg> = DoubleValueView<'msg>;
  type MessageMut<'msg> = DoubleValueMut<'msg>;
}

impl ::std::default::Default for DoubleValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for DoubleValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `DoubleValue` is `Sync` because it does not implement interior mutability.
//    Neither does `DoubleValueMut`.
unsafe impl ::std::marker::Sync for DoubleValue {}

// SAFETY:
// - `DoubleValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for DoubleValue {}

impl ::protobuf::Proxied for DoubleValue {
  type View<'msg> = DoubleValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for DoubleValue {}

impl ::protobuf::MutProxied for DoubleValue {
  type Mut<'msg> = DoubleValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct DoubleValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, DoubleValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for DoubleValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for DoubleValueView<'msg> {
  type Message = DoubleValue;
}

impl ::std::fmt::Debug for DoubleValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for DoubleValueView<'_> {
  fn default() -> DoubleValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, DoubleValue>> for DoubleValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, DoubleValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> DoubleValueView<'msg> {

  pub fn to_owned(&self) -> DoubleValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional double
  pub fn value(self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        0, (0f64).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `DoubleValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for DoubleValueView<'_> {}

// SAFETY:
// - `DoubleValueView` is `Send` because while its alive a `DoubleValueMut` cannot.
// - `DoubleValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for DoubleValueView<'_> {}

impl<'msg> ::protobuf::AsView for DoubleValueView<'msg> {
  type Proxied = DoubleValue;
  fn as_view(&self) -> ::protobuf::View<'msg, DoubleValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for DoubleValueView<'msg> {
  fn into_view<'shorter>(self) -> DoubleValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<DoubleValue> for DoubleValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> DoubleValue {
    let mut dst = DoubleValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<DoubleValue> for DoubleValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> DoubleValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for DoubleValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for DoubleValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for DoubleValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct DoubleValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, DoubleValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for DoubleValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for DoubleValueMut<'msg> {
  type Message = DoubleValue;
}

impl ::std::fmt::Debug for DoubleValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, DoubleValue>> for DoubleValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, DoubleValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> DoubleValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, DoubleValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> DoubleValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional double
  pub fn value(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        0, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `DoubleValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for DoubleValueMut<'_> {}

// SAFETY:
// - `DoubleValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for DoubleValueMut<'_> {}

impl<'msg> ::protobuf::AsView for DoubleValueMut<'msg> {
  type Proxied = DoubleValue;
  fn as_view(&self) -> ::protobuf::View<'_, DoubleValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for DoubleValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, DoubleValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for DoubleValueMut<'msg> {
  type MutProxied = DoubleValue;
  fn as_mut(&mut self) -> DoubleValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for DoubleValueMut<'msg> {
  fn into_mut<'shorter>(self) -> DoubleValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl DoubleValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, DoubleValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> DoubleValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> DoubleValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional double
  pub fn value(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        0, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        0, val.into()
      )
    }
  }

}  // impl DoubleValue

impl ::std::ops::Drop for DoubleValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for DoubleValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for DoubleValue {
  type Proxied = Self;
  fn as_view(&self) -> DoubleValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for DoubleValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> DoubleValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for DoubleValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__DoubleValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$ P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__DoubleValue_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__DoubleValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for DoubleValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for DoubleValue {
  type Msg = DoubleValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<DoubleValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for DoubleValue {
  type Msg = DoubleValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<DoubleValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for DoubleValueMut<'_> {
  type Msg = DoubleValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<DoubleValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for DoubleValueMut<'_> {
  type Msg = DoubleValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<DoubleValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for DoubleValueView<'_> {
  type Msg = DoubleValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<DoubleValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for DoubleValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__FloatValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct FloatValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<FloatValue>
}

impl ::protobuf::Message for FloatValue {
  type MessageView<'msg> = FloatValueView<'msg>;
  type MessageMut<'msg> = FloatValueMut<'msg>;
}

impl ::std::default::Default for FloatValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for FloatValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `FloatValue` is `Sync` because it does not implement interior mutability.
//    Neither does `FloatValueMut`.
unsafe impl ::std::marker::Sync for FloatValue {}

// SAFETY:
// - `FloatValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for FloatValue {}

impl ::protobuf::Proxied for FloatValue {
  type View<'msg> = FloatValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for FloatValue {}

impl ::protobuf::MutProxied for FloatValue {
  type Mut<'msg> = FloatValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct FloatValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, FloatValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for FloatValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for FloatValueView<'msg> {
  type Message = FloatValue;
}

impl ::std::fmt::Debug for FloatValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for FloatValueView<'_> {
  fn default() -> FloatValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, FloatValue>> for FloatValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, FloatValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> FloatValueView<'msg> {

  pub fn to_owned(&self) -> FloatValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional float
  pub fn value(self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        0, (0f32).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `FloatValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for FloatValueView<'_> {}

// SAFETY:
// - `FloatValueView` is `Send` because while its alive a `FloatValueMut` cannot.
// - `FloatValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for FloatValueView<'_> {}

impl<'msg> ::protobuf::AsView for FloatValueView<'msg> {
  type Proxied = FloatValue;
  fn as_view(&self) -> ::protobuf::View<'msg, FloatValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for FloatValueView<'msg> {
  fn into_view<'shorter>(self) -> FloatValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<FloatValue> for FloatValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> FloatValue {
    let mut dst = FloatValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<FloatValue> for FloatValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> FloatValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for FloatValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for FloatValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for FloatValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct FloatValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, FloatValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for FloatValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for FloatValueMut<'msg> {
  type Message = FloatValue;
}

impl ::std::fmt::Debug for FloatValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, FloatValue>> for FloatValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, FloatValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> FloatValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, FloatValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> FloatValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional float
  pub fn value(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        0, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `FloatValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for FloatValueMut<'_> {}

// SAFETY:
// - `FloatValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for FloatValueMut<'_> {}

impl<'msg> ::protobuf::AsView for FloatValueMut<'msg> {
  type Proxied = FloatValue;
  fn as_view(&self) -> ::protobuf::View<'_, FloatValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for FloatValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, FloatValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for FloatValueMut<'msg> {
  type MutProxied = FloatValue;
  fn as_mut(&mut self) -> FloatValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for FloatValueMut<'msg> {
  fn into_mut<'shorter>(self) -> FloatValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl FloatValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, FloatValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> FloatValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> FloatValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional float
  pub fn value(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        0, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        0, val.into()
      )
    }
  }

}  // impl FloatValue

impl ::std::ops::Drop for FloatValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for FloatValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for FloatValue {
  type Proxied = Self;
  fn as_view(&self) -> FloatValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for FloatValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> FloatValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for FloatValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__FloatValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$!P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__FloatValue_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__FloatValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for FloatValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for FloatValue {
  type Msg = FloatValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FloatValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FloatValue {
  type Msg = FloatValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FloatValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for FloatValueMut<'_> {
  type Msg = FloatValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FloatValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FloatValueMut<'_> {
  type Msg = FloatValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FloatValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FloatValueView<'_> {
  type Msg = FloatValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FloatValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for FloatValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Int64Value_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Int64Value {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Int64Value>
}

impl ::protobuf::Message for Int64Value {
  type MessageView<'msg> = Int64ValueView<'msg>;
  type MessageMut<'msg> = Int64ValueMut<'msg>;
}

impl ::std::default::Default for Int64Value {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Int64Value {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Int64Value` is `Sync` because it does not implement interior mutability.
//    Neither does `Int64ValueMut`.
unsafe impl ::std::marker::Sync for Int64Value {}

// SAFETY:
// - `Int64Value` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Int64Value {}

impl ::protobuf::Proxied for Int64Value {
  type View<'msg> = Int64ValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Int64Value {}

impl ::protobuf::MutProxied for Int64Value {
  type Mut<'msg> = Int64ValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct Int64ValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Int64Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for Int64ValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for Int64ValueView<'msg> {
  type Message = Int64Value;
}

impl ::std::fmt::Debug for Int64ValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for Int64ValueView<'_> {
  fn default() -> Int64ValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Int64Value>> for Int64ValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Int64Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> Int64ValueView<'msg> {

  pub fn to_owned(&self) -> Int64Value {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional int64
  pub fn value(self) -> i64 {
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
// - `Int64ValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for Int64ValueView<'_> {}

// SAFETY:
// - `Int64ValueView` is `Send` because while its alive a `Int64ValueMut` cannot.
// - `Int64ValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for Int64ValueView<'_> {}

impl<'msg> ::protobuf::AsView for Int64ValueView<'msg> {
  type Proxied = Int64Value;
  fn as_view(&self) -> ::protobuf::View<'msg, Int64Value> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Int64ValueView<'msg> {
  fn into_view<'shorter>(self) -> Int64ValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Int64Value> for Int64ValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Int64Value {
    let mut dst = Int64Value::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Int64Value> for Int64ValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Int64Value {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Int64Value {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for Int64ValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for Int64ValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct Int64ValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Int64Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for Int64ValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for Int64ValueMut<'msg> {
  type Message = Int64Value;
}

impl ::std::fmt::Debug for Int64ValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Int64Value>> for Int64ValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Int64Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> Int64ValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Int64Value> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Int64Value {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional int64
  pub fn value(&self) -> i64 {
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
  pub fn set_value(&mut self, val: i64) {
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
// - `Int64ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for Int64ValueMut<'_> {}

// SAFETY:
// - `Int64ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for Int64ValueMut<'_> {}

impl<'msg> ::protobuf::AsView for Int64ValueMut<'msg> {
  type Proxied = Int64Value;
  fn as_view(&self) -> ::protobuf::View<'_, Int64Value> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Int64ValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Int64Value>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for Int64ValueMut<'msg> {
  type MutProxied = Int64Value;
  fn as_mut(&mut self) -> Int64ValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for Int64ValueMut<'msg> {
  fn into_mut<'shorter>(self) -> Int64ValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Int64Value {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Int64Value> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> Int64ValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> Int64ValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional int64
  pub fn value(&self) -> i64 {
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
  pub fn set_value(&mut self, val: i64) {
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

}  // impl Int64Value

impl ::std::ops::Drop for Int64Value {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Int64Value {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Int64Value {
  type Proxied = Self;
  fn as_view(&self) -> Int64ValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Int64Value {
  type MutProxied = Self;
  fn as_mut(&mut self) -> Int64ValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Int64Value {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__Int64Value_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$+P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__Int64Value_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__Int64Value_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Int64Value {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Int64Value {
  type Msg = Int64Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int64Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int64Value {
  type Msg = Int64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int64Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Int64ValueMut<'_> {
  type Msg = Int64Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int64Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int64ValueMut<'_> {
  type Msg = Int64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int64Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int64ValueView<'_> {
  type Msg = Int64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int64Value> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Int64ValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__UInt64Value_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct UInt64Value {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<UInt64Value>
}

impl ::protobuf::Message for UInt64Value {
  type MessageView<'msg> = UInt64ValueView<'msg>;
  type MessageMut<'msg> = UInt64ValueMut<'msg>;
}

impl ::std::default::Default for UInt64Value {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for UInt64Value {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `UInt64Value` is `Sync` because it does not implement interior mutability.
//    Neither does `UInt64ValueMut`.
unsafe impl ::std::marker::Sync for UInt64Value {}

// SAFETY:
// - `UInt64Value` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for UInt64Value {}

impl ::protobuf::Proxied for UInt64Value {
  type View<'msg> = UInt64ValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for UInt64Value {}

impl ::protobuf::MutProxied for UInt64Value {
  type Mut<'msg> = UInt64ValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct UInt64ValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, UInt64Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for UInt64ValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for UInt64ValueView<'msg> {
  type Message = UInt64Value;
}

impl ::std::fmt::Debug for UInt64ValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for UInt64ValueView<'_> {
  fn default() -> UInt64ValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, UInt64Value>> for UInt64ValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, UInt64Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> UInt64ValueView<'msg> {

  pub fn to_owned(&self) -> UInt64Value {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional uint64
  pub fn value(self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        0, (0u64).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `UInt64ValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for UInt64ValueView<'_> {}

// SAFETY:
// - `UInt64ValueView` is `Send` because while its alive a `UInt64ValueMut` cannot.
// - `UInt64ValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for UInt64ValueView<'_> {}

impl<'msg> ::protobuf::AsView for UInt64ValueView<'msg> {
  type Proxied = UInt64Value;
  fn as_view(&self) -> ::protobuf::View<'msg, UInt64Value> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for UInt64ValueView<'msg> {
  fn into_view<'shorter>(self) -> UInt64ValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<UInt64Value> for UInt64ValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> UInt64Value {
    let mut dst = UInt64Value::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<UInt64Value> for UInt64ValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> UInt64Value {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for UInt64Value {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for UInt64ValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for UInt64ValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct UInt64ValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt64Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for UInt64ValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for UInt64ValueMut<'msg> {
  type Message = UInt64Value;
}

impl ::std::fmt::Debug for UInt64ValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, UInt64Value>> for UInt64ValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt64Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> UInt64ValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt64Value> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> UInt64Value {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional uint64
  pub fn value(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        0, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `UInt64ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for UInt64ValueMut<'_> {}

// SAFETY:
// - `UInt64ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for UInt64ValueMut<'_> {}

impl<'msg> ::protobuf::AsView for UInt64ValueMut<'msg> {
  type Proxied = UInt64Value;
  fn as_view(&self) -> ::protobuf::View<'_, UInt64Value> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for UInt64ValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, UInt64Value>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for UInt64ValueMut<'msg> {
  type MutProxied = UInt64Value;
  fn as_mut(&mut self) -> UInt64ValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for UInt64ValueMut<'msg> {
  fn into_mut<'shorter>(self) -> UInt64ValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl UInt64Value {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, UInt64Value> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> UInt64ValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> UInt64ValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional uint64
  pub fn value(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        0, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        0, val.into()
      )
    }
  }

}  // impl UInt64Value

impl ::std::ops::Drop for UInt64Value {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for UInt64Value {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for UInt64Value {
  type Proxied = Self;
  fn as_view(&self) -> UInt64ValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for UInt64Value {
  type MutProxied = Self;
  fn as_mut(&mut self) -> UInt64ValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for UInt64Value {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__UInt64Value_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$,P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__UInt64Value_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__UInt64Value_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for UInt64Value {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for UInt64Value {
  type Msg = UInt64Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt64Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt64Value {
  type Msg = UInt64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt64Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for UInt64ValueMut<'_> {
  type Msg = UInt64Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt64Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt64ValueMut<'_> {
  type Msg = UInt64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt64Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt64ValueView<'_> {
  type Msg = UInt64Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt64Value> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for UInt64ValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Int32Value_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Int32Value {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Int32Value>
}

impl ::protobuf::Message for Int32Value {
  type MessageView<'msg> = Int32ValueView<'msg>;
  type MessageMut<'msg> = Int32ValueMut<'msg>;
}

impl ::std::default::Default for Int32Value {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Int32Value {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Int32Value` is `Sync` because it does not implement interior mutability.
//    Neither does `Int32ValueMut`.
unsafe impl ::std::marker::Sync for Int32Value {}

// SAFETY:
// - `Int32Value` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Int32Value {}

impl ::protobuf::Proxied for Int32Value {
  type View<'msg> = Int32ValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Int32Value {}

impl ::protobuf::MutProxied for Int32Value {
  type Mut<'msg> = Int32ValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct Int32ValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Int32Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for Int32ValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for Int32ValueView<'msg> {
  type Message = Int32Value;
}

impl ::std::fmt::Debug for Int32ValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for Int32ValueView<'_> {
  fn default() -> Int32ValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Int32Value>> for Int32ValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Int32Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> Int32ValueView<'msg> {

  pub fn to_owned(&self) -> Int32Value {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional int32
  pub fn value(self) -> i32 {
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

}

// SAFETY:
// - `Int32ValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for Int32ValueView<'_> {}

// SAFETY:
// - `Int32ValueView` is `Send` because while its alive a `Int32ValueMut` cannot.
// - `Int32ValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for Int32ValueView<'_> {}

impl<'msg> ::protobuf::AsView for Int32ValueView<'msg> {
  type Proxied = Int32Value;
  fn as_view(&self) -> ::protobuf::View<'msg, Int32Value> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Int32ValueView<'msg> {
  fn into_view<'shorter>(self) -> Int32ValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Int32Value> for Int32ValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Int32Value {
    let mut dst = Int32Value::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Int32Value> for Int32ValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Int32Value {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Int32Value {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for Int32ValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for Int32ValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct Int32ValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Int32Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for Int32ValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for Int32ValueMut<'msg> {
  type Message = Int32Value;
}

impl ::std::fmt::Debug for Int32ValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Int32Value>> for Int32ValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Int32Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> Int32ValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Int32Value> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Int32Value {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional int32
  pub fn value(&self) -> i32 {
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
  pub fn set_value(&mut self, val: i32) {
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

}

// SAFETY:
// - `Int32ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for Int32ValueMut<'_> {}

// SAFETY:
// - `Int32ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for Int32ValueMut<'_> {}

impl<'msg> ::protobuf::AsView for Int32ValueMut<'msg> {
  type Proxied = Int32Value;
  fn as_view(&self) -> ::protobuf::View<'_, Int32Value> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Int32ValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Int32Value>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for Int32ValueMut<'msg> {
  type MutProxied = Int32Value;
  fn as_mut(&mut self) -> Int32ValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for Int32ValueMut<'msg> {
  fn into_mut<'shorter>(self) -> Int32ValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Int32Value {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Int32Value> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> Int32ValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> Int32ValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional int32
  pub fn value(&self) -> i32 {
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
  pub fn set_value(&mut self, val: i32) {
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

}  // impl Int32Value

impl ::std::ops::Drop for Int32Value {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Int32Value {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Int32Value {
  type Proxied = Self;
  fn as_view(&self) -> Int32ValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Int32Value {
  type MutProxied = Self;
  fn as_mut(&mut self) -> Int32ValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Int32Value {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__Int32Value_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__Int32Value_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__Int32Value_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Int32Value {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Int32Value {
  type Msg = Int32Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int32Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int32Value {
  type Msg = Int32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int32Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Int32ValueMut<'_> {
  type Msg = Int32Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int32Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int32ValueMut<'_> {
  type Msg = Int32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int32Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Int32ValueView<'_> {
  type Msg = Int32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Int32Value> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Int32ValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__UInt32Value_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct UInt32Value {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<UInt32Value>
}

impl ::protobuf::Message for UInt32Value {
  type MessageView<'msg> = UInt32ValueView<'msg>;
  type MessageMut<'msg> = UInt32ValueMut<'msg>;
}

impl ::std::default::Default for UInt32Value {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for UInt32Value {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `UInt32Value` is `Sync` because it does not implement interior mutability.
//    Neither does `UInt32ValueMut`.
unsafe impl ::std::marker::Sync for UInt32Value {}

// SAFETY:
// - `UInt32Value` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for UInt32Value {}

impl ::protobuf::Proxied for UInt32Value {
  type View<'msg> = UInt32ValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for UInt32Value {}

impl ::protobuf::MutProxied for UInt32Value {
  type Mut<'msg> = UInt32ValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct UInt32ValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, UInt32Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for UInt32ValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for UInt32ValueView<'msg> {
  type Message = UInt32Value;
}

impl ::std::fmt::Debug for UInt32ValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for UInt32ValueView<'_> {
  fn default() -> UInt32ValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, UInt32Value>> for UInt32ValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, UInt32Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> UInt32ValueView<'msg> {

  pub fn to_owned(&self) -> UInt32Value {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional uint32
  pub fn value(self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        0, (0u32).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `UInt32ValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for UInt32ValueView<'_> {}

// SAFETY:
// - `UInt32ValueView` is `Send` because while its alive a `UInt32ValueMut` cannot.
// - `UInt32ValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for UInt32ValueView<'_> {}

impl<'msg> ::protobuf::AsView for UInt32ValueView<'msg> {
  type Proxied = UInt32Value;
  fn as_view(&self) -> ::protobuf::View<'msg, UInt32Value> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for UInt32ValueView<'msg> {
  fn into_view<'shorter>(self) -> UInt32ValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<UInt32Value> for UInt32ValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> UInt32Value {
    let mut dst = UInt32Value::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<UInt32Value> for UInt32ValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> UInt32Value {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for UInt32Value {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for UInt32ValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for UInt32ValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct UInt32ValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt32Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for UInt32ValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for UInt32ValueMut<'msg> {
  type Message = UInt32Value;
}

impl ::std::fmt::Debug for UInt32ValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, UInt32Value>> for UInt32ValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt32Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> UInt32ValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, UInt32Value> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> UInt32Value {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional uint32
  pub fn value(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        0, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `UInt32ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for UInt32ValueMut<'_> {}

// SAFETY:
// - `UInt32ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for UInt32ValueMut<'_> {}

impl<'msg> ::protobuf::AsView for UInt32ValueMut<'msg> {
  type Proxied = UInt32Value;
  fn as_view(&self) -> ::protobuf::View<'_, UInt32Value> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for UInt32ValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, UInt32Value>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for UInt32ValueMut<'msg> {
  type MutProxied = UInt32Value;
  fn as_mut(&mut self) -> UInt32ValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for UInt32ValueMut<'msg> {
  fn into_mut<'shorter>(self) -> UInt32ValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl UInt32Value {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, UInt32Value> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> UInt32ValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> UInt32ValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional uint32
  pub fn value(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        0, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        0, val.into()
      )
    }
  }

}  // impl UInt32Value

impl ::std::ops::Drop for UInt32Value {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for UInt32Value {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for UInt32Value {
  type Proxied = Self;
  fn as_view(&self) -> UInt32ValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for UInt32Value {
  type MutProxied = Self;
  fn as_mut(&mut self) -> UInt32ValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for UInt32Value {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__UInt32Value_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$)P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__UInt32Value_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__UInt32Value_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for UInt32Value {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for UInt32Value {
  type Msg = UInt32Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt32Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt32Value {
  type Msg = UInt32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt32Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for UInt32ValueMut<'_> {
  type Msg = UInt32Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt32Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt32ValueMut<'_> {
  type Msg = UInt32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt32Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for UInt32ValueView<'_> {
  type Msg = UInt32Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<UInt32Value> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for UInt32ValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__BoolValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct BoolValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<BoolValue>
}

impl ::protobuf::Message for BoolValue {
  type MessageView<'msg> = BoolValueView<'msg>;
  type MessageMut<'msg> = BoolValueMut<'msg>;
}

impl ::std::default::Default for BoolValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for BoolValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `BoolValue` is `Sync` because it does not implement interior mutability.
//    Neither does `BoolValueMut`.
unsafe impl ::std::marker::Sync for BoolValue {}

// SAFETY:
// - `BoolValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for BoolValue {}

impl ::protobuf::Proxied for BoolValue {
  type View<'msg> = BoolValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for BoolValue {}

impl ::protobuf::MutProxied for BoolValue {
  type Mut<'msg> = BoolValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct BoolValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, BoolValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BoolValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for BoolValueView<'msg> {
  type Message = BoolValue;
}

impl ::std::fmt::Debug for BoolValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for BoolValueView<'_> {
  fn default() -> BoolValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, BoolValue>> for BoolValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, BoolValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BoolValueView<'msg> {

  pub fn to_owned(&self) -> BoolValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional bool
  pub fn value(self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        0, (false).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `BoolValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for BoolValueView<'_> {}

// SAFETY:
// - `BoolValueView` is `Send` because while its alive a `BoolValueMut` cannot.
// - `BoolValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for BoolValueView<'_> {}

impl<'msg> ::protobuf::AsView for BoolValueView<'msg> {
  type Proxied = BoolValue;
  fn as_view(&self) -> ::protobuf::View<'msg, BoolValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BoolValueView<'msg> {
  fn into_view<'shorter>(self) -> BoolValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<BoolValue> for BoolValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> BoolValue {
    let mut dst = BoolValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<BoolValue> for BoolValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> BoolValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for BoolValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BoolValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BoolValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct BoolValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, BoolValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BoolValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for BoolValueMut<'msg> {
  type Message = BoolValue;
}

impl ::std::fmt::Debug for BoolValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, BoolValue>> for BoolValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, BoolValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BoolValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, BoolValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> BoolValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional bool
  pub fn value(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        0, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        0, val.into()
      )
    }
  }

}

// SAFETY:
// - `BoolValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for BoolValueMut<'_> {}

// SAFETY:
// - `BoolValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for BoolValueMut<'_> {}

impl<'msg> ::protobuf::AsView for BoolValueMut<'msg> {
  type Proxied = BoolValue;
  fn as_view(&self) -> ::protobuf::View<'_, BoolValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BoolValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, BoolValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for BoolValueMut<'msg> {
  type MutProxied = BoolValue;
  fn as_mut(&mut self) -> BoolValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for BoolValueMut<'msg> {
  fn into_mut<'shorter>(self) -> BoolValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl BoolValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, BoolValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> BoolValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> BoolValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional bool
  pub fn value(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        0, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_value(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        0, val.into()
      )
    }
  }

}  // impl BoolValue

impl ::std::ops::Drop for BoolValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for BoolValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for BoolValue {
  type Proxied = Self;
  fn as_view(&self) -> BoolValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for BoolValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> BoolValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for BoolValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__BoolValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$/P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__BoolValue_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__BoolValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for BoolValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for BoolValue {
  type Msg = BoolValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BoolValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BoolValue {
  type Msg = BoolValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BoolValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for BoolValueMut<'_> {
  type Msg = BoolValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BoolValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BoolValueMut<'_> {
  type Msg = BoolValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BoolValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BoolValueView<'_> {
  type Msg = BoolValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BoolValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for BoolValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__StringValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct StringValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<StringValue>
}

impl ::protobuf::Message for StringValue {
  type MessageView<'msg> = StringValueView<'msg>;
  type MessageMut<'msg> = StringValueMut<'msg>;
}

impl ::std::default::Default for StringValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for StringValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `StringValue` is `Sync` because it does not implement interior mutability.
//    Neither does `StringValueMut`.
unsafe impl ::std::marker::Sync for StringValue {}

// SAFETY:
// - `StringValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for StringValue {}

impl ::protobuf::Proxied for StringValue {
  type View<'msg> = StringValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for StringValue {}

impl ::protobuf::MutProxied for StringValue {
  type Mut<'msg> = StringValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct StringValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, StringValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for StringValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for StringValueView<'msg> {
  type Message = StringValue;
}

impl ::std::fmt::Debug for StringValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for StringValueView<'_> {
  fn default() -> StringValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, StringValue>> for StringValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, StringValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> StringValueView<'msg> {

  pub fn to_owned(&self) -> StringValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional string
  pub fn value(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

}

// SAFETY:
// - `StringValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for StringValueView<'_> {}

// SAFETY:
// - `StringValueView` is `Send` because while its alive a `StringValueMut` cannot.
// - `StringValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for StringValueView<'_> {}

impl<'msg> ::protobuf::AsView for StringValueView<'msg> {
  type Proxied = StringValue;
  fn as_view(&self) -> ::protobuf::View<'msg, StringValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for StringValueView<'msg> {
  fn into_view<'shorter>(self) -> StringValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<StringValue> for StringValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> StringValue {
    let mut dst = StringValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<StringValue> for StringValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> StringValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for StringValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for StringValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for StringValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct StringValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, StringValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for StringValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for StringValueMut<'msg> {
  type Message = StringValue;
}

impl ::std::fmt::Debug for StringValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, StringValue>> for StringValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, StringValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> StringValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, StringValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> StringValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional string
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}

// SAFETY:
// - `StringValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for StringValueMut<'_> {}

// SAFETY:
// - `StringValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for StringValueMut<'_> {}

impl<'msg> ::protobuf::AsView for StringValueMut<'msg> {
  type Proxied = StringValue;
  fn as_view(&self) -> ::protobuf::View<'_, StringValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for StringValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, StringValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for StringValueMut<'msg> {
  type MutProxied = StringValue;
  fn as_mut(&mut self) -> StringValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for StringValueMut<'msg> {
  fn into_mut<'shorter>(self) -> StringValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl StringValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, StringValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> StringValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> StringValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional string
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}  // impl StringValue

impl ::std::ops::Drop for StringValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for StringValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for StringValue {
  type Proxied = Self;
  fn as_view(&self) -> StringValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for StringValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> StringValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for StringValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__StringValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$M1P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__StringValue_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__StringValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for StringValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for StringValue {
  type Msg = StringValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<StringValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for StringValue {
  type Msg = StringValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<StringValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for StringValueMut<'_> {
  type Msg = StringValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<StringValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for StringValueMut<'_> {
  type Msg = StringValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<StringValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for StringValueView<'_> {
  type Msg = StringValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<StringValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for StringValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__BytesValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct BytesValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<BytesValue>
}

impl ::protobuf::Message for BytesValue {
  type MessageView<'msg> = BytesValueView<'msg>;
  type MessageMut<'msg> = BytesValueMut<'msg>;
}

impl ::std::default::Default for BytesValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for BytesValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `BytesValue` is `Sync` because it does not implement interior mutability.
//    Neither does `BytesValueMut`.
unsafe impl ::std::marker::Sync for BytesValue {}

// SAFETY:
// - `BytesValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for BytesValue {}

impl ::protobuf::Proxied for BytesValue {
  type View<'msg> = BytesValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for BytesValue {}

impl ::protobuf::MutProxied for BytesValue {
  type Mut<'msg> = BytesValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct BytesValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, BytesValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BytesValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for BytesValueView<'msg> {
  type Message = BytesValue;
}

impl ::std::fmt::Debug for BytesValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for BytesValueView<'_> {
  fn default() -> BytesValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, BytesValue>> for BytesValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, BytesValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BytesValueView<'msg> {

  pub fn to_owned(&self) -> BytesValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // value: optional bytes
  pub fn value(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

}

// SAFETY:
// - `BytesValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for BytesValueView<'_> {}

// SAFETY:
// - `BytesValueView` is `Send` because while its alive a `BytesValueMut` cannot.
// - `BytesValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for BytesValueView<'_> {}

impl<'msg> ::protobuf::AsView for BytesValueView<'msg> {
  type Proxied = BytesValue;
  fn as_view(&self) -> ::protobuf::View<'msg, BytesValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BytesValueView<'msg> {
  fn into_view<'shorter>(self) -> BytesValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<BytesValue> for BytesValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> BytesValue {
    let mut dst = BytesValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<BytesValue> for BytesValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> BytesValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for BytesValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BytesValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for BytesValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct BytesValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, BytesValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for BytesValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for BytesValueMut<'msg> {
  type Message = BytesValue;
}

impl ::std::fmt::Debug for BytesValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, BytesValue>> for BytesValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, BytesValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> BytesValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, BytesValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> BytesValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // value: optional bytes
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}

// SAFETY:
// - `BytesValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for BytesValueMut<'_> {}

// SAFETY:
// - `BytesValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for BytesValueMut<'_> {}

impl<'msg> ::protobuf::AsView for BytesValueMut<'msg> {
  type Proxied = BytesValue;
  fn as_view(&self) -> ::protobuf::View<'_, BytesValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for BytesValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, BytesValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for BytesValueMut<'msg> {
  type MutProxied = BytesValue;
  fn as_mut(&mut self) -> BytesValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for BytesValueMut<'msg> {
  fn into_mut<'shorter>(self) -> BytesValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl BytesValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, BytesValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> BytesValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> BytesValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // value: optional bytes
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}  // impl BytesValue

impl ::std::ops::Drop for BytesValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for BytesValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for BytesValue {
  type Proxied = Self;
  fn as_view(&self) -> BytesValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for BytesValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> BytesValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for BytesValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_wrappers_proto::google__protobuf__BytesValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$0P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_wrappers_proto::google__protobuf__BytesValue_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_wrappers_proto::google__protobuf__BytesValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for BytesValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for BytesValue {
  type Msg = BytesValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BytesValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BytesValue {
  type Msg = BytesValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BytesValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for BytesValueMut<'_> {
  type Msg = BytesValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BytesValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BytesValueMut<'_> {
  type Msg = BytesValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BytesValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for BytesValueView<'_> {
  type Msg = BytesValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<BytesValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for BytesValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



