// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Struct_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Struct {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Struct>
}

impl ::protobuf::Message for Struct {
  type MessageView<'msg> = StructView<'msg>;
  type MessageMut<'msg> = StructMut<'msg>;
}

impl ::std::default::Default for Struct {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Struct {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Struct` is `Sync` because it does not implement interior mutability.
//    Neither does `StructMut`.
unsafe impl ::std::marker::Sync for Struct {}

// SAFETY:
// - `Struct` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Struct {}

impl ::protobuf::Proxied for Struct {
  type View<'msg> = StructView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Struct {}

impl ::protobuf::MutProxied for Struct {
  type Mut<'msg> = StructMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct StructView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Struct>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for StructView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for StructView<'msg> {
  type Message = Struct;
}

impl ::std::fmt::Debug for StructView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for StructView<'_> {
  fn default() -> StructView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Struct>> for StructView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Struct>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> StructView<'msg> {

  pub fn to_owned(&self) -> Struct {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // fields: repeated message google.protobuf.Struct.FieldsEntry
  pub fn fields(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, super::google_protobuf_struct_proto::Value> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_struct_proto::Value>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

}

// SAFETY:
// - `StructView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for StructView<'_> {}

// SAFETY:
// - `StructView` is `Send` because while its alive a `StructMut` cannot.
// - `StructView` does not use thread-local data.
unsafe impl ::std::marker::Send for StructView<'_> {}

impl<'msg> ::protobuf::AsView for StructView<'msg> {
  type Proxied = Struct;
  fn as_view(&self) -> ::protobuf::View<'msg, Struct> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for StructView<'msg> {
  fn into_view<'shorter>(self) -> StructView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Struct> for StructView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Struct {
    let mut dst = Struct::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Struct> for StructMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Struct {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Struct {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for StructView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for StructMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct StructMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Struct>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for StructMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for StructMut<'msg> {
  type Message = Struct;
}

impl ::std::fmt::Debug for StructMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Struct>> for StructMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Struct>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> StructMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Struct> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Struct {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // fields: repeated message google.protobuf.Struct.FieldsEntry
  pub fn fields(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_struct_proto::Value> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_struct_proto::Value>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn fields_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_struct_proto::Value> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          0, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_fields(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `StructMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for StructMut<'_> {}

// SAFETY:
// - `StructMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for StructMut<'_> {}

impl<'msg> ::protobuf::AsView for StructMut<'msg> {
  type Proxied = Struct;
  fn as_view(&self) -> ::protobuf::View<'_, Struct> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for StructMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Struct>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for StructMut<'msg> {
  type MutProxied = Struct;
  fn as_mut(&mut self) -> StructMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for StructMut<'msg> {
  fn into_mut<'shorter>(self) -> StructMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Struct {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Struct> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> StructView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> StructMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // fields: repeated message google.protobuf.Struct.FieldsEntry
  pub fn fields(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_struct_proto::Value> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(0)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_struct_proto::Value>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn fields_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_struct_proto::Value> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          0, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_fields(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl Struct

impl ::std::ops::Drop for Struct {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Struct {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Struct {
  type Proxied = Self;
  fn as_view(&self) -> StructView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Struct {
  type MutProxied = Self;
  fn as_mut(&mut self) -> StructMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Struct {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        <super::google_protobuf_struct_proto::ListValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table();
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_struct_proto::google__protobuf__Struct_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Struct {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Struct {
  type Msg = Struct;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Struct> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Struct {
  type Msg = Struct;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Struct> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for StructMut<'_> {
  type Msg = Struct;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Struct> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for StructMut<'_> {
  type Msg = Struct;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Struct> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for StructView<'_> {
  type Msg = Struct;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Struct> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for StructMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod r#struct {// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Struct__FieldsEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct FieldsEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for FieldsEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        <super::super::google_protobuf_struct_proto::ListValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table();
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_struct_proto::r#struct::google__protobuf__Struct__FieldsEntry_msg_init.0)
      }).0
    }
  }
}

}  // pub mod r#struct


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__Value_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Value {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Value>
}

impl ::protobuf::Message for Value {
  type MessageView<'msg> = ValueView<'msg>;
  type MessageMut<'msg> = ValueMut<'msg>;
}

impl ::std::default::Default for Value {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Value {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Value` is `Sync` because it does not implement interior mutability.
//    Neither does `ValueMut`.
unsafe impl ::std::marker::Sync for Value {}

// SAFETY:
// - `Value` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Value {}

impl ::protobuf::Proxied for Value {
  type View<'msg> = ValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Value {}

impl ::protobuf::MutProxied for Value {
  type Mut<'msg> = ValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct ValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for ValueView<'msg> {
  type Message = Value;
}

impl ::std::fmt::Debug for ValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for ValueView<'_> {
  fn default() -> ValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Value>> for ValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ValueView<'msg> {

  pub fn to_owned(&self) -> Value {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // null_value: optional enum google.protobuf.NullValue
  pub fn has_null_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn null_value_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_null_value().then(|| self.null_value())
  }
  pub fn null_value(self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }

  // number_value: optional double
  pub fn has_number_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn number_value_opt(self) -> ::std::option::Option<f64> {
    self.has_number_value().then(|| self.number_value())
  }
  pub fn number_value(self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        1, (0f64).into()
      ).try_into().unwrap()
    }
  }

  // string_value: optional string
  pub fn has_string_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn string_value_opt(self) -> ::std::option::Option<&'msg ::protobuf::ProtoStr> {
    self.has_string_value().then(|| self.string_value())
  }
  pub fn string_value(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // bool_value: optional bool
  pub fn has_bool_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(3)
    }
  }
  pub fn bool_value_opt(self) -> ::std::option::Option<bool> {
    self.has_bool_value().then(|| self.bool_value())
  }
  pub fn bool_value(self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        3, (false).into()
      ).try_into().unwrap()
    }
  }

  // struct_value: optional message google.protobuf.Struct
  pub fn has_struct_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn struct_value_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'msg>> {
    self.has_struct_value().then(|| self.struct_value())
  }
  pub fn struct_value(self) -> super::google_protobuf_struct_proto::StructView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }

  // list_value: optional message google.protobuf.ListValue
  pub fn has_list_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn list_value_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::ListValueView<'msg>> {
    self.has_list_value().then(|| self.list_value())
  }
  pub fn list_value(self) -> super::google_protobuf_struct_proto::ListValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ListValueView::default())
  }

  pub fn kind(self) -> super::google_protobuf_struct_proto::value::KindOneof<'msg> {
    match self.kind_case() {
      super::google_protobuf_struct_proto::value::KindCase::NullValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NullValue(self.null_value()),
      super::google_protobuf_struct_proto::value::KindCase::NumberValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NumberValue(self.number_value()),
      super::google_protobuf_struct_proto::value::KindCase::StringValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StringValue(self.string_value()),
      super::google_protobuf_struct_proto::value::KindCase::BoolValue =>
          super::google_protobuf_struct_proto::value::KindOneof::BoolValue(self.bool_value()),
      super::google_protobuf_struct_proto::value::KindCase::StructValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StructValue(self.struct_value()),
      super::google_protobuf_struct_proto::value::KindCase::ListValue =>
          super::google_protobuf_struct_proto::value::KindOneof::ListValue(self.list_value()),
      _ => super::google_protobuf_struct_proto::value::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(self) -> super::google_protobuf_struct_proto::value::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::google_protobuf_struct_proto::value::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `ValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for ValueView<'_> {}

// SAFETY:
// - `ValueView` is `Send` because while its alive a `ValueMut` cannot.
// - `ValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for ValueView<'_> {}

impl<'msg> ::protobuf::AsView for ValueView<'msg> {
  type Proxied = Value;
  fn as_view(&self) -> ::protobuf::View<'msg, Value> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ValueView<'msg> {
  fn into_view<'shorter>(self) -> ValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Value> for ValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Value {
    let mut dst = Value::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Value> for ValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Value {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Value {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct ValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Value>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for ValueMut<'msg> {
  type Message = Value;
}

impl ::std::fmt::Debug for ValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Value>> for ValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Value>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Value> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Value {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // null_value: optional enum google.protobuf.NullValue
  pub fn has_null_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_null_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn null_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_null_value().then(|| self.null_value())
  }
  pub fn null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
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

  // number_value: optional double
  pub fn has_number_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_number_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn number_value_opt(&self) -> ::std::option::Option<f64> {
    self.has_number_value().then(|| self.number_value())
  }
  pub fn number_value(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        1, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_number_value(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        1, val.into()
      )
    }
  }

  // string_value: optional string
  pub fn has_string_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn clear_string_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        2
      );
    }
  }
  pub fn string_value_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_string_value().then(|| self.string_value())
  }
  pub fn string_value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_string_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // bool_value: optional bool
  pub fn has_bool_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(3)
    }
  }
  pub fn clear_bool_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        3
      );
    }
  }
  pub fn bool_value_opt(&self) -> ::std::option::Option<bool> {
    self.has_bool_value().then(|| self.bool_value())
  }
  pub fn bool_value(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        3, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_bool_value(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        3, val.into()
      )
    }
  }

  // struct_value: optional message google.protobuf.Struct
  pub fn has_struct_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn clear_struct_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        4
      );
    }
  }
  pub fn struct_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'_>> {
    self.has_struct_value().then(|| self.struct_value())
  }
  pub fn struct_value(&self) -> super::google_protobuf_struct_proto::StructView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }
  pub fn struct_value_mut(&mut self) -> super::google_protobuf_struct_proto::StructMut<'_> {
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
  pub fn set_struct_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Struct>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        val
      );
    }
  }

  // list_value: optional message google.protobuf.ListValue
  pub fn has_list_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn clear_list_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        5
      );
    }
  }
  pub fn list_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::ListValueView<'_>> {
    self.has_list_value().then(|| self.list_value())
  }
  pub fn list_value(&self) -> super::google_protobuf_struct_proto::ListValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ListValueView::default())
  }
  pub fn list_value_mut(&mut self) -> super::google_protobuf_struct_proto::ListValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         5, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_list_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::ListValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        val
      );
    }
  }

  pub fn kind(&self) -> super::google_protobuf_struct_proto::value::KindOneof<'_> {
    match &self.kind_case() {
      super::google_protobuf_struct_proto::value::KindCase::NullValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NullValue(self.null_value()),
      super::google_protobuf_struct_proto::value::KindCase::NumberValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NumberValue(self.number_value()),
      super::google_protobuf_struct_proto::value::KindCase::StringValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StringValue(self.string_value()),
      super::google_protobuf_struct_proto::value::KindCase::BoolValue =>
          super::google_protobuf_struct_proto::value::KindOneof::BoolValue(self.bool_value()),
      super::google_protobuf_struct_proto::value::KindCase::StructValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StructValue(self.struct_value()),
      super::google_protobuf_struct_proto::value::KindCase::ListValue =>
          super::google_protobuf_struct_proto::value::KindOneof::ListValue(self.list_value()),
      _ => super::google_protobuf_struct_proto::value::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(&self) -> super::google_protobuf_struct_proto::value::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::google_protobuf_struct_proto::value::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for ValueMut<'_> {}

// SAFETY:
// - `ValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for ValueMut<'_> {}

impl<'msg> ::protobuf::AsView for ValueMut<'msg> {
  type Proxied = Value;
  fn as_view(&self) -> ::protobuf::View<'_, Value> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Value>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for ValueMut<'msg> {
  type MutProxied = Value;
  fn as_mut(&mut self) -> ValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for ValueMut<'msg> {
  fn into_mut<'shorter>(self) -> ValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Value {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Value> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> ValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> ValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // null_value: optional enum google.protobuf.NullValue
  pub fn has_null_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(0)
    }
  }
  pub fn clear_null_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        0
      );
    }
  }
  pub fn null_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_null_value().then(|| self.null_value())
  }
  pub fn null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        0, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
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

  // number_value: optional double
  pub fn has_number_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_number_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn number_value_opt(&self) -> ::std::option::Option<f64> {
    self.has_number_value().then(|| self.number_value())
  }
  pub fn number_value(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        1, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_number_value(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        1, val.into()
      )
    }
  }

  // string_value: optional string
  pub fn has_string_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn clear_string_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        2
      );
    }
  }
  pub fn string_value_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_string_value().then(|| self.string_value())
  }
  pub fn string_value(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_string_value(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // bool_value: optional bool
  pub fn has_bool_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(3)
    }
  }
  pub fn clear_bool_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        3
      );
    }
  }
  pub fn bool_value_opt(&self) -> ::std::option::Option<bool> {
    self.has_bool_value().then(|| self.bool_value())
  }
  pub fn bool_value(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        3, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_bool_value(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        3, val.into()
      )
    }
  }

  // struct_value: optional message google.protobuf.Struct
  pub fn has_struct_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(4)
    }
  }
  pub fn clear_struct_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        4
      );
    }
  }
  pub fn struct_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'_>> {
    self.has_struct_value().then(|| self.struct_value())
  }
  pub fn struct_value(&self) -> super::google_protobuf_struct_proto::StructView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(4)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }
  pub fn struct_value_mut(&mut self) -> super::google_protobuf_struct_proto::StructMut<'_> {
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
  pub fn set_struct_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Struct>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        val
      );
    }
  }

  // list_value: optional message google.protobuf.ListValue
  pub fn has_list_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn clear_list_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        5
      );
    }
  }
  pub fn list_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::ListValueView<'_>> {
    self.has_list_value().then(|| self.list_value())
  }
  pub fn list_value(&self) -> super::google_protobuf_struct_proto::ListValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ListValueView::default())
  }
  pub fn list_value_mut(&mut self) -> super::google_protobuf_struct_proto::ListValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         5, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_list_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::ListValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        val
      );
    }
  }

  pub fn kind(&self) -> super::google_protobuf_struct_proto::value::KindOneof<'_> {
    match &self.kind_case() {
      super::google_protobuf_struct_proto::value::KindCase::NullValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NullValue(self.null_value()),
      super::google_protobuf_struct_proto::value::KindCase::NumberValue =>
          super::google_protobuf_struct_proto::value::KindOneof::NumberValue(self.number_value()),
      super::google_protobuf_struct_proto::value::KindCase::StringValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StringValue(self.string_value()),
      super::google_protobuf_struct_proto::value::KindCase::BoolValue =>
          super::google_protobuf_struct_proto::value::KindOneof::BoolValue(self.bool_value()),
      super::google_protobuf_struct_proto::value::KindCase::StructValue =>
          super::google_protobuf_struct_proto::value::KindOneof::StructValue(self.struct_value()),
      super::google_protobuf_struct_proto::value::KindCase::ListValue =>
          super::google_protobuf_struct_proto::value::KindOneof::ListValue(self.list_value()),
      _ => super::google_protobuf_struct_proto::value::KindOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn kind_case(&self) -> super::google_protobuf_struct_proto::value::KindCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(0);
      super::google_protobuf_struct_proto::value::KindCase::try_from(field_num).unwrap_unchecked()
    }
  }
}  // impl Value

impl ::std::ops::Drop for Value {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Value {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Value {
  type Proxied = Self;
  fn as_view(&self) -> ValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Value {
  type MutProxied = Self;
  fn as_mut(&mut self) -> ValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Value {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        <super::google_protobuf_struct_proto::ListValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table();
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_struct_proto::google__protobuf__Value_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Value {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Value {
  type Msg = Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Value {
  type Msg = Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ValueMut<'_> {
  type Msg = Value;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Value> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ValueMut<'_> {
  type Msg = Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Value> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ValueView<'_> {
  type Msg = Value;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Value> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod value {

#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
#[repr(u32)]
pub enum KindOneof<'msg> {
  NullValue(::protobuf::View<'msg, super::super::google_protobuf_struct_proto::NullValue>) = 1,
  NumberValue(f64) = 2,
  StringValue(&'msg ::protobuf::ProtoStr) = 3,
  BoolValue(bool) = 4,
  StructValue(::protobuf::View<'msg, super::super::google_protobuf_struct_proto::Struct>) = 5,
  ListValue(::protobuf::View<'msg, super::super::google_protobuf_struct_proto::ListValue>) = 6,

  not_set(std::marker::PhantomData<&'msg ()>) = 0
}
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(dead_code)]
pub enum KindCase {
  NullValue = 1,
  NumberValue = 2,
  StringValue = 3,
  BoolValue = 4,
  StructValue = 5,
  ListValue = 6,

  not_set = 0
}

impl KindCase {
  #[allow(dead_code)]
  pub(crate) fn try_from(v: u32) -> ::std::option::Option<KindCase> {
    match v {
      0 => Some(KindCase::not_set),
      1 => Some(KindCase::NullValue),
      2 => Some(KindCase::NumberValue),
      3 => Some(KindCase::StringValue),
      4 => Some(KindCase::BoolValue),
      5 => Some(KindCase::StructValue),
      6 => Some(KindCase::ListValue),
      _ => None
    }
  }
}
}  // pub mod value


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut google__protobuf__ListValue_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct ListValue {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<ListValue>
}

impl ::protobuf::Message for ListValue {
  type MessageView<'msg> = ListValueView<'msg>;
  type MessageMut<'msg> = ListValueMut<'msg>;
}

impl ::std::default::Default for ListValue {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for ListValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `ListValue` is `Sync` because it does not implement interior mutability.
//    Neither does `ListValueMut`.
unsafe impl ::std::marker::Sync for ListValue {}

// SAFETY:
// - `ListValue` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for ListValue {}

impl ::protobuf::Proxied for ListValue {
  type View<'msg> = ListValueView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for ListValue {}

impl ::protobuf::MutProxied for ListValue {
  type Mut<'msg> = ListValueMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct ListValueView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, ListValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ListValueView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for ListValueView<'msg> {
  type Message = ListValue;
}

impl ::std::fmt::Debug for ListValueView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for ListValueView<'_> {
  fn default() -> ListValueView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, ListValue>> for ListValueView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, ListValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ListValueView<'msg> {

  pub fn to_owned(&self) -> ListValue {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // values: repeated message google.protobuf.Value
  pub fn values(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

}

// SAFETY:
// - `ListValueView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for ListValueView<'_> {}

// SAFETY:
// - `ListValueView` is `Send` because while its alive a `ListValueMut` cannot.
// - `ListValueView` does not use thread-local data.
unsafe impl ::std::marker::Send for ListValueView<'_> {}

impl<'msg> ::protobuf::AsView for ListValueView<'msg> {
  type Proxied = ListValue;
  fn as_view(&self) -> ::protobuf::View<'msg, ListValue> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ListValueView<'msg> {
  fn into_view<'shorter>(self) -> ListValueView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<ListValue> for ListValueView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> ListValue {
    let mut dst = ListValue::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<ListValue> for ListValueMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> ListValue {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for ListValue {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ListValueView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ListValueMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct ListValueMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, ListValue>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ListValueMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for ListValueMut<'msg> {
  type Message = ListValue;
}

impl ::std::fmt::Debug for ListValueMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, ListValue>> for ListValueMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, ListValue>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ListValueMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, ListValue> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> ListValue {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // values: repeated message google.protobuf.Value
  pub fn values(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn values_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Value> {
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
  pub fn set_values(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}

// SAFETY:
// - `ListValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for ListValueMut<'_> {}

// SAFETY:
// - `ListValueMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for ListValueMut<'_> {}

impl<'msg> ::protobuf::AsView for ListValueMut<'msg> {
  type Proxied = ListValue;
  fn as_view(&self) -> ::protobuf::View<'_, ListValue> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ListValueMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, ListValue>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for ListValueMut<'msg> {
  type MutProxied = ListValue;
  fn as_mut(&mut self) -> ListValueMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for ListValueMut<'msg> {
  fn into_mut<'shorter>(self) -> ListValueMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl ListValue {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, ListValue> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> ListValueView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> ListValueMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // values: repeated message google.protobuf.Value
  pub fn values(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        0
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn values_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Value> {
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
  pub fn set_values(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        src);
    }
  }

}  // impl ListValue

impl ::std::ops::Drop for ListValue {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for ListValue {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for ListValue {
  type Proxied = Self;
  fn as_view(&self) -> ListValueView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for ListValue {
  type MutProxied = Self;
  fn as_mut(&mut self) -> ListValueMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for ListValue {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_struct_proto::google__protobuf__ListValue_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$G");
        super::google_protobuf_struct_proto::google__protobuf__Struct_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$G");
        super::google_protobuf_struct_proto::r#struct::google__protobuf__Struct__FieldsEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X3");
        super::google_protobuf_struct_proto::google__protobuf__Value_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$. 1T/33^!|#|$|%|&|(");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_struct_proto::google__protobuf__ListValue_msg_init.0, &[super::google_protobuf_struct_proto::google__protobuf__Value_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_struct_proto::google__protobuf__Struct_msg_init.0, &[super::google_protobuf_struct_proto::r#struct::google__protobuf__Struct__FieldsEntry_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_struct_proto::r#struct::google__protobuf__Struct__FieldsEntry_msg_init.0, &[super::google_protobuf_struct_proto::google__protobuf__Value_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_struct_proto::google__protobuf__Value_msg_init.0, &[super::google_protobuf_struct_proto::google__protobuf__Struct_msg_init.0,
            super::google_protobuf_struct_proto::google__protobuf__ListValue_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_struct_proto::google__protobuf__ListValue_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ListValue {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ListValue {
  type Msg = ListValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ListValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ListValue {
  type Msg = ListValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ListValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ListValueMut<'_> {
  type Msg = ListValue;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ListValue> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ListValueMut<'_> {
  type Msg = ListValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ListValue> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ListValueView<'_> {
  type Msg = ListValue;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ListValue> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ListValueMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NullValue(i32);

#[allow(non_upper_case_globals)]
impl NullValue {
  pub const NullValue: NullValue = NullValue(0);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "NullValue",
      _ => return None
    })
  }
}

impl ::std::convert::From<NullValue> for i32 {
  fn from(val: NullValue) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for NullValue {
  fn from(val: i32) -> NullValue {
    Self(val)
  }
}

impl ::std::default::Default for NullValue {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for NullValue {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "NullValue::{}", constant_name)
    } else {
      write!(f, "NullValue::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for NullValue {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for NullValue {}

impl ::protobuf::Proxied for NullValue {
  type View<'a> = NullValue;
}

impl ::protobuf::AsView for NullValue {
  type Proxied = NullValue;

  fn as_view(&self) -> NullValue {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NullValue {
  fn into_view<'shorter>(self) -> NullValue where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for NullValue {
  const NAME: &'static str = "NullValue";

  fn is_known(value: i32) -> bool {
    matches!(value, 0)
  }
}

impl ::protobuf::__internal::EntityType for NullValue {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}


