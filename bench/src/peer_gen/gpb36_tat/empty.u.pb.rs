// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Empty_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
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
        super::google_protobuf_empty_proto::google__protobuf__Empty_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_empty_proto::google__protobuf__Empty_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_empty_proto::google__protobuf__Empty_msg_init.0)
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



