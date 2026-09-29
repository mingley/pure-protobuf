// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Timestamp_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Timestamp {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Timestamp>
}

impl ::protobuf::Message for Timestamp {
  type MessageView<'msg> = TimestampView<'msg>;
  type MessageMut<'msg> = TimestampMut<'msg>;
}

impl ::std::default::Default for Timestamp {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Timestamp {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Timestamp` is `Sync` because it does not implement interior mutability.
//    Neither does `TimestampMut`.
unsafe impl ::std::marker::Sync for Timestamp {}

// SAFETY:
// - `Timestamp` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Timestamp {}

impl ::protobuf::Proxied for Timestamp {
  type View<'msg> = TimestampView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Timestamp {}

impl ::protobuf::MutProxied for Timestamp {
  type Mut<'msg> = TimestampMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct TimestampView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Timestamp>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TimestampView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for TimestampView<'msg> {
  type Message = Timestamp;
}

impl ::std::fmt::Debug for TimestampView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for TimestampView<'_> {
  fn default() -> TimestampView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Timestamp>> for TimestampView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Timestamp>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TimestampView<'msg> {

  pub fn to_owned(&self) -> Timestamp {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // seconds: optional int64
  pub fn seconds(self) -> i64 {
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

  // nanos: optional int32
  pub fn nanos(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        1, (0i32).into()
      ).try_into().unwrap()
    }
  }

}

// SAFETY:
// - `TimestampView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for TimestampView<'_> {}

// SAFETY:
// - `TimestampView` is `Send` because while its alive a `TimestampMut` cannot.
// - `TimestampView` does not use thread-local data.
unsafe impl ::std::marker::Send for TimestampView<'_> {}

impl<'msg> ::protobuf::AsView for TimestampView<'msg> {
  type Proxied = Timestamp;
  fn as_view(&self) -> ::protobuf::View<'msg, Timestamp> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TimestampView<'msg> {
  fn into_view<'shorter>(self) -> TimestampView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Timestamp> for TimestampView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Timestamp {
    let mut dst = Timestamp::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Timestamp> for TimestampMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Timestamp {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Timestamp {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TimestampView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TimestampMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct TimestampMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Timestamp>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TimestampMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for TimestampMut<'msg> {
  type Message = Timestamp;
}

impl ::std::fmt::Debug for TimestampMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Timestamp>> for TimestampMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Timestamp>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TimestampMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Timestamp> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Timestamp {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // seconds: optional int64
  pub fn seconds(&self) -> i64 {
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
  pub fn set_seconds(&mut self, val: i64) {
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

  // nanos: optional int32
  pub fn nanos(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        1, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_nanos(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        1, val.into()
      )
    }
  }

}

// SAFETY:
// - `TimestampMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for TimestampMut<'_> {}

// SAFETY:
// - `TimestampMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for TimestampMut<'_> {}

impl<'msg> ::protobuf::AsView for TimestampMut<'msg> {
  type Proxied = Timestamp;
  fn as_view(&self) -> ::protobuf::View<'_, Timestamp> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TimestampMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Timestamp>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for TimestampMut<'msg> {
  type MutProxied = Timestamp;
  fn as_mut(&mut self) -> TimestampMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for TimestampMut<'msg> {
  fn into_mut<'shorter>(self) -> TimestampMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Timestamp {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Timestamp> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> TimestampView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> TimestampMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // seconds: optional int64
  pub fn seconds(&self) -> i64 {
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
  pub fn set_seconds(&mut self, val: i64) {
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

  // nanos: optional int32
  pub fn nanos(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        1, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_nanos(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        1, val.into()
      )
    }
  }

}  // impl Timestamp

impl ::std::ops::Drop for Timestamp {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Timestamp {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Timestamp {
  type Proxied = Self;
  fn as_view(&self) -> TimestampView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Timestamp {
  type MutProxied = Self;
  fn as_mut(&mut self) -> TimestampMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Timestamp {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_timestamp_proto::google__protobuf__Timestamp_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$+P(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_timestamp_proto::google__protobuf__Timestamp_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_timestamp_proto::google__protobuf__Timestamp_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Timestamp {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Timestamp {
  type Msg = Timestamp;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Timestamp> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Timestamp {
  type Msg = Timestamp;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Timestamp> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for TimestampMut<'_> {
  type Msg = Timestamp;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Timestamp> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TimestampMut<'_> {
  type Msg = Timestamp;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Timestamp> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TimestampView<'_> {
  type Msg = Timestamp;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Timestamp> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for TimestampMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



