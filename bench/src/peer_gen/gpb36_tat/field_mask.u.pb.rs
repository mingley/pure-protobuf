// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__FieldMask_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct FieldMask {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<FieldMask>
}

impl ::protobuf::Message for FieldMask {
  type MessageView<'msg> = FieldMaskView<'msg>;
  type MessageMut<'msg> = FieldMaskMut<'msg>;
}

impl ::std::default::Default for FieldMask {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for FieldMask {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `FieldMask` is `Sync` because it does not implement interior mutability.
//    Neither does `FieldMaskMut`.
unsafe impl ::std::marker::Sync for FieldMask {}

// SAFETY:
// - `FieldMask` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for FieldMask {}

impl ::protobuf::Proxied for FieldMask {
  type View<'msg> = FieldMaskView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for FieldMask {}

impl ::protobuf::MutProxied for FieldMask {
  type Mut<'msg> = FieldMaskMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct FieldMaskView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, FieldMask>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for FieldMaskView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for FieldMaskView<'msg> {
  type Message = FieldMask;
}

impl ::std::fmt::Debug for FieldMaskView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for FieldMaskView<'_> {
  fn default() -> FieldMaskView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, FieldMask>> for FieldMaskView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, FieldMask>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> FieldMaskView<'msg> {

  pub fn to_owned(&self) -> FieldMask {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // paths: repeated string
  pub fn paths(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
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
// - `FieldMaskView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for FieldMaskView<'_> {}

// SAFETY:
// - `FieldMaskView` is `Send` because while its alive a `FieldMaskMut` cannot.
// - `FieldMaskView` does not use thread-local data.
unsafe impl ::std::marker::Send for FieldMaskView<'_> {}

impl<'msg> ::protobuf::AsView for FieldMaskView<'msg> {
  type Proxied = FieldMask;
  fn as_view(&self) -> ::protobuf::View<'msg, FieldMask> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for FieldMaskView<'msg> {
  fn into_view<'shorter>(self) -> FieldMaskView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<FieldMask> for FieldMaskView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> FieldMask {
    let mut dst = FieldMask::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<FieldMask> for FieldMaskMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> FieldMask {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for FieldMask {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for FieldMaskView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for FieldMaskMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct FieldMaskMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, FieldMask>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for FieldMaskMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for FieldMaskMut<'msg> {
  type Message = FieldMask;
}

impl ::std::fmt::Debug for FieldMaskMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, FieldMask>> for FieldMaskMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, FieldMask>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> FieldMaskMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, FieldMask> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> FieldMask {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // paths: repeated string
  pub fn paths(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
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
  pub fn paths_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
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
  pub fn set_paths(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `FieldMaskMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for FieldMaskMut<'_> {}

// SAFETY:
// - `FieldMaskMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for FieldMaskMut<'_> {}

impl<'msg> ::protobuf::AsView for FieldMaskMut<'msg> {
  type Proxied = FieldMask;
  fn as_view(&self) -> ::protobuf::View<'_, FieldMask> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for FieldMaskMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, FieldMask>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for FieldMaskMut<'msg> {
  type MutProxied = FieldMask;
  fn as_mut(&mut self) -> FieldMaskMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for FieldMaskMut<'msg> {
  fn into_mut<'shorter>(self) -> FieldMaskMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl FieldMask {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, FieldMask> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> FieldMaskView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> FieldMaskMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // paths: repeated string
  pub fn paths(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
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
  pub fn paths_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
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
  pub fn set_paths(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl FieldMask

impl ::std::ops::Drop for FieldMask {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for FieldMask {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for FieldMask {
  type Proxied = Self;
  fn as_view(&self) -> FieldMaskView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for FieldMask {
  type MutProxied = Self;
  fn as_mut(&mut self) -> FieldMaskMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for FieldMask {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_field_mask_proto::google__protobuf__FieldMask_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$ME");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_field_mask_proto::google__protobuf__FieldMask_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_field_mask_proto::google__protobuf__FieldMask_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for FieldMask {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for FieldMask {
  type Msg = FieldMask;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FieldMask> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FieldMask {
  type Msg = FieldMask;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FieldMask> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for FieldMaskMut<'_> {
  type Msg = FieldMask;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FieldMask> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FieldMaskMut<'_> {
  type Msg = FieldMask;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FieldMask> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for FieldMaskView<'_> {
  type Msg = FieldMask;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<FieldMask> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for FieldMaskMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



