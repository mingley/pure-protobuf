// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Any_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Any {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Any>
}

impl ::protobuf::Message for Any {
  type MessageView<'msg> = AnyView<'msg>;
  type MessageMut<'msg> = AnyMut<'msg>;
}

impl ::std::default::Default for Any {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Any {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Any` is `Sync` because it does not implement interior mutability.
//    Neither does `AnyMut`.
unsafe impl ::std::marker::Sync for Any {}

// SAFETY:
// - `Any` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Any {}

impl ::protobuf::Proxied for Any {
  type View<'msg> = AnyView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Any {}

impl ::protobuf::MutProxied for Any {
  type Mut<'msg> = AnyMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct AnyView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Any>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for AnyView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for AnyView<'msg> {
  type Message = Any;
}

impl ::std::fmt::Debug for AnyView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for AnyView<'_> {
  fn default() -> AnyView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Any>> for AnyView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Any>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> AnyView<'msg> {

  pub fn to_owned(&self) -> Any {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // type_url: optional string
  pub fn type_url(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // value: optional bytes
  pub fn value(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

}

// SAFETY:
// - `AnyView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for AnyView<'_> {}

// SAFETY:
// - `AnyView` is `Send` because while its alive a `AnyMut` cannot.
// - `AnyView` does not use thread-local data.
unsafe impl ::std::marker::Send for AnyView<'_> {}

impl<'msg> ::protobuf::AsView for AnyView<'msg> {
  type Proxied = Any;
  fn as_view(&self) -> ::protobuf::View<'msg, Any> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for AnyView<'msg> {
  fn into_view<'shorter>(self) -> AnyView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Any> for AnyView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Any {
    let mut dst = Any::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Any> for AnyMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Any {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Any {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for AnyView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for AnyMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct AnyMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Any>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for AnyMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for AnyMut<'msg> {
  type Message = Any;
}

impl ::std::fmt::Debug for AnyMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Any>> for AnyMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Any>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> AnyMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Any> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Any {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // type_url: optional string
  pub fn type_url(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_type_url(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

  // value: optional bytes
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

}

// SAFETY:
// - `AnyMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for AnyMut<'_> {}

// SAFETY:
// - `AnyMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for AnyMut<'_> {}

impl<'msg> ::protobuf::AsView for AnyMut<'msg> {
  type Proxied = Any;
  fn as_view(&self) -> ::protobuf::View<'_, Any> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for AnyMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Any>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for AnyMut<'msg> {
  type MutProxied = Any;
  fn as_mut(&mut self) -> AnyMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for AnyMut<'msg> {
  fn into_mut<'shorter>(self) -> AnyMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Any {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Any> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> AnyView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> AnyMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // type_url: optional string
  pub fn type_url(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_type_url(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

  // value: optional bytes
  pub fn value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

}  // impl Any

impl ::std::ops::Drop for Any {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Any {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Any {
  type Proxied = Self;
  fn as_view(&self) -> AnyView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Any {
  type MutProxied = Self;
  fn as_mut(&mut self) -> AnyMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Any {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_any_proto::google__protobuf__Any_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$1X0P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_any_proto::google__protobuf__Any_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_any_proto::google__protobuf__Any_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Any {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Any {
  type Msg = Any;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Any> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Any {
  type Msg = Any;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Any> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for AnyMut<'_> {
  type Msg = Any;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Any> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for AnyMut<'_> {
  type Msg = Any;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Any> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for AnyView<'_> {
  type Msg = Any;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Any> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for AnyMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



