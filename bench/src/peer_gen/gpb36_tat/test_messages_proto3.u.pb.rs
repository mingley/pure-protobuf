// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: protoc 36.2 --rust_out (experimental-codegen=enabled, kernel=upb)
// source: third_party/protobuf/src/google/protobuf/test_messages_proto3.proto [protobuf v35.1 @ 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03] (+ google/protobuf WKTs) (sha256 c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873)
// rewrite: none
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
const _: () = ::protobuf::__internal::assert_compatible_gencode_version("0.36.2-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct TestAllTypesProto3 {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<TestAllTypesProto3>
}

impl ::protobuf::Message for TestAllTypesProto3 {
  type MessageView<'msg> = TestAllTypesProto3View<'msg>;
  type MessageMut<'msg> = TestAllTypesProto3Mut<'msg>;
}

impl ::std::default::Default for TestAllTypesProto3 {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for TestAllTypesProto3 {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `TestAllTypesProto3` is `Sync` because it does not implement interior mutability.
//    Neither does `TestAllTypesProto3Mut`.
unsafe impl ::std::marker::Sync for TestAllTypesProto3 {}

// SAFETY:
// - `TestAllTypesProto3` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for TestAllTypesProto3 {}

impl ::protobuf::Proxied for TestAllTypesProto3 {
  type View<'msg> = TestAllTypesProto3View<'msg>;
}

impl ::protobuf::__internal::SealedInternal for TestAllTypesProto3 {}

impl ::protobuf::MutProxied for TestAllTypesProto3 {
  type Mut<'msg> = TestAllTypesProto3Mut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct TestAllTypesProto3View<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, TestAllTypesProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TestAllTypesProto3View<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for TestAllTypesProto3View<'msg> {
  type Message = TestAllTypesProto3;
}

impl ::std::fmt::Debug for TestAllTypesProto3View<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for TestAllTypesProto3View<'_> {
  fn default() -> TestAllTypesProto3View<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, TestAllTypesProto3>> for TestAllTypesProto3View<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, TestAllTypesProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TestAllTypesProto3View<'msg> {

  pub fn to_owned(&self) -> TestAllTypesProto3 {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // optional_int32: optional int32
  pub fn optional_int32(self) -> i32 {
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

  // optional_int64: optional int64
  pub fn optional_int64(self) -> i64 {
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

  // optional_uint32: optional uint32
  pub fn optional_uint32(self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        2, (0u32).into()
      ).try_into().unwrap()
    }
  }

  // optional_uint64: optional uint64
  pub fn optional_uint64(self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        3, (0u64).into()
      ).try_into().unwrap()
    }
  }

  // optional_sint32: optional sint32
  pub fn optional_sint32(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        4, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // optional_sint64: optional sint64
  pub fn optional_sint64(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        5, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // optional_fixed32: optional fixed32
  pub fn optional_fixed32(self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        6, (0u32).into()
      ).try_into().unwrap()
    }
  }

  // optional_fixed64: optional fixed64
  pub fn optional_fixed64(self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        7, (0u64).into()
      ).try_into().unwrap()
    }
  }

  // optional_sfixed32: optional sfixed32
  pub fn optional_sfixed32(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        8, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // optional_sfixed64: optional sfixed64
  pub fn optional_sfixed64(self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        9, (0i64).into()
      ).try_into().unwrap()
    }
  }

  // optional_float: optional float
  pub fn optional_float(self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        10, (0f32).into()
      ).try_into().unwrap()
    }
  }

  // optional_double: optional double
  pub fn optional_double(self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        11, (0f64).into()
      ).try_into().unwrap()
    }
  }

  // optional_bool: optional bool
  pub fn optional_bool(self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        12, (false).into()
      ).try_into().unwrap()
    }
  }

  // optional_string: optional string
  pub fn optional_string(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        13, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // optional_bytes: optional bytes
  pub fn optional_bytes(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        14, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

  // optional_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_optional_nested_message(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(15)
    }
  }
  pub fn optional_nested_message_opt(self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'msg>> {
    self.has_optional_nested_message().then(|| self.optional_nested_message())
  }
  pub fn optional_nested_message(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(15)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }

  // optional_foreign_message: optional message protobuf_test_messages.proto3.ForeignMessage
  pub fn has_optional_foreign_message(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(16)
    }
  }
  pub fn optional_foreign_message_opt(self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'msg>> {
    self.has_optional_foreign_message().then(|| self.optional_foreign_message())
  }
  pub fn optional_foreign_message(self) -> super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(16)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::ForeignMessageView::default())
  }

  // optional_nested_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn optional_nested_enum(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        17, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }

  // optional_foreign_enum: optional enum protobuf_test_messages.proto3.ForeignEnum
  pub fn optional_foreign_enum(self) -> super::google_protobuf_test_messages_proto3_proto::ForeignEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        18, (super::google_protobuf_test_messages_proto3_proto::ForeignEnum::ForeignFoo).into()
      ).try_into().unwrap()
    }
  }

  // optional_aliased_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.AliasedEnum
  pub fn optional_aliased_enum(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        19, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum::AliasFoo).into()
      ).try_into().unwrap()
    }
  }

  // optional_string_piece: optional string
  pub fn optional_string_piece(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        20, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // optional_cord: optional string
  pub fn optional_cord(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        21, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // recursive_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_recursive_message(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(22)
    }
  }
  pub fn recursive_message_opt(self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'msg>> {
    self.has_recursive_message().then(|| self.recursive_message())
  }
  pub fn recursive_message(self) -> super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(22)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }

  // repeated_int32: repeated int32
  pub fn repeated_int32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        23
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_int64: repeated int64
  pub fn repeated_int64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        24
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_uint32: repeated uint32
  pub fn repeated_uint32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        25
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_uint64: repeated uint64
  pub fn repeated_uint64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        26
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_sint32: repeated sint32
  pub fn repeated_sint32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        27
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_sint64: repeated sint64
  pub fn repeated_sint64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        28
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_fixed32: repeated fixed32
  pub fn repeated_fixed32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        29
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_fixed64: repeated fixed64
  pub fn repeated_fixed64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        30
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_sfixed32: repeated sfixed32
  pub fn repeated_sfixed32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        31
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_sfixed64: repeated sfixed64
  pub fn repeated_sfixed64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        32
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_float: repeated float
  pub fn repeated_float(self) -> ::protobuf::RepeatedView<'msg, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        33
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_double: repeated double
  pub fn repeated_double(self) -> ::protobuf::RepeatedView<'msg, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        34
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_bool: repeated bool
  pub fn repeated_bool(self) -> ::protobuf::RepeatedView<'msg, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        35
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_string: repeated string
  pub fn repeated_string(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        36
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_bytes: repeated bytes
  pub fn repeated_bytes(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoBytes> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        37
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoBytes>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn repeated_nested_message(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        38
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_foreign_message: repeated message protobuf_test_messages.proto3.ForeignMessage
  pub fn repeated_foreign_message(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        39
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn repeated_nested_enum(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        40
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_foreign_enum: repeated enum protobuf_test_messages.proto3.ForeignEnum
  pub fn repeated_foreign_enum(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        41
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_string_piece: repeated string
  pub fn repeated_string_piece(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        42
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_cord: repeated string
  pub fn repeated_cord(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        43
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_int32: repeated int32
  pub fn packed_int32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        63
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_int64: repeated int64
  pub fn packed_int64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        64
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_uint32: repeated uint32
  pub fn packed_uint32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        65
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_uint64: repeated uint64
  pub fn packed_uint64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        66
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_sint32: repeated sint32
  pub fn packed_sint32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        67
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_sint64: repeated sint64
  pub fn packed_sint64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        68
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_fixed32: repeated fixed32
  pub fn packed_fixed32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        69
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_fixed64: repeated fixed64
  pub fn packed_fixed64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        70
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_sfixed32: repeated sfixed32
  pub fn packed_sfixed32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        71
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_sfixed64: repeated sfixed64
  pub fn packed_sfixed64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        72
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_float: repeated float
  pub fn packed_float(self) -> ::protobuf::RepeatedView<'msg, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        73
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_double: repeated double
  pub fn packed_double(self) -> ::protobuf::RepeatedView<'msg, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        74
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_bool: repeated bool
  pub fn packed_bool(self) -> ::protobuf::RepeatedView<'msg, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        75
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // packed_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn packed_nested_enum(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        76
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_int32: repeated int32
  pub fn unpacked_int32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        77
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_int64: repeated int64
  pub fn unpacked_int64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        78
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_uint32: repeated uint32
  pub fn unpacked_uint32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        79
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_uint64: repeated uint64
  pub fn unpacked_uint64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        80
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_sint32: repeated sint32
  pub fn unpacked_sint32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        81
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_sint64: repeated sint64
  pub fn unpacked_sint64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        82
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_fixed32: repeated fixed32
  pub fn unpacked_fixed32(self) -> ::protobuf::RepeatedView<'msg, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        83
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_fixed64: repeated fixed64
  pub fn unpacked_fixed64(self) -> ::protobuf::RepeatedView<'msg, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        84
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_sfixed32: repeated sfixed32
  pub fn unpacked_sfixed32(self) -> ::protobuf::RepeatedView<'msg, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        85
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_sfixed64: repeated sfixed64
  pub fn unpacked_sfixed64(self) -> ::protobuf::RepeatedView<'msg, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        86
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_float: repeated float
  pub fn unpacked_float(self) -> ::protobuf::RepeatedView<'msg, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        87
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_double: repeated double
  pub fn unpacked_double(self) -> ::protobuf::RepeatedView<'msg, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        88
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_bool: repeated bool
  pub fn unpacked_bool(self) -> ::protobuf::RepeatedView<'msg, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        89
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // unpacked_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn unpacked_nested_enum(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        90
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // map_int32_int32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32Int32Entry
  pub fn map_int32_int32(self)
    -> ::protobuf::MapView<'msg, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(44)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_int64_int64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt64Int64Entry
  pub fn map_int64_int64(self)
    -> ::protobuf::MapView<'msg, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(45)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_uint32_uint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint32Uint32Entry
  pub fn map_uint32_uint32(self)
    -> ::protobuf::MapView<'msg, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(46)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_uint64_uint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint64Uint64Entry
  pub fn map_uint64_uint64(self)
    -> ::protobuf::MapView<'msg, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(47)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_sint32_sint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint32Sint32Entry
  pub fn map_sint32_sint32(self)
    -> ::protobuf::MapView<'msg, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(48)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_sint64_sint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint64Sint64Entry
  pub fn map_sint64_sint64(self)
    -> ::protobuf::MapView<'msg, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(49)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_fixed32_fixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed32Fixed32Entry
  pub fn map_fixed32_fixed32(self)
    -> ::protobuf::MapView<'msg, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(50)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_fixed64_fixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed64Fixed64Entry
  pub fn map_fixed64_fixed64(self)
    -> ::protobuf::MapView<'msg, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(51)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_sfixed32_sfixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed32Sfixed32Entry
  pub fn map_sfixed32_sfixed32(self)
    -> ::protobuf::MapView<'msg, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(52)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_sfixed64_sfixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed64Sfixed64Entry
  pub fn map_sfixed64_sfixed64(self)
    -> ::protobuf::MapView<'msg, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(53)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_int32_float: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32FloatEntry
  pub fn map_int32_float(self)
    -> ::protobuf::MapView<'msg, i32, f32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(54)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_int32_double: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32DoubleEntry
  pub fn map_int32_double(self)
    -> ::protobuf::MapView<'msg, i32, f64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(55)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_bool_bool: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapBoolBoolEntry
  pub fn map_bool_bool(self)
    -> ::protobuf::MapView<'msg, bool, bool> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(56)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<bool, bool>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_string: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringStringEntry
  pub fn map_string_string(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(57)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_bytes: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringBytesEntry
  pub fn map_string_bytes(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, ::protobuf::ProtoBytes> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(58)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoBytes>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedMessageEntry
  pub fn map_string_nested_message(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(59)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_foreign_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignMessageEntry
  pub fn map_string_foreign_message(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(60)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_nested_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedEnumEntry
  pub fn map_string_nested_enum(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(61)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // map_string_foreign_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignEnumEntry
  pub fn map_string_foreign_enum(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(62)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // oneof_uint32: optional uint32
  pub fn has_oneof_uint32(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(91)
    }
  }
  pub fn oneof_uint32_opt(self) -> ::std::option::Option<u32> {
    self.has_oneof_uint32().then(|| self.oneof_uint32())
  }
  pub fn oneof_uint32(self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        91, (0u32).into()
      ).try_into().unwrap()
    }
  }

  // oneof_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_oneof_nested_message(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(92)
    }
  }
  pub fn oneof_nested_message_opt(self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'msg>> {
    self.has_oneof_nested_message().then(|| self.oneof_nested_message())
  }
  pub fn oneof_nested_message(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(92)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }

  // oneof_string: optional string
  pub fn has_oneof_string(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(93)
    }
  }
  pub fn oneof_string_opt(self) -> ::std::option::Option<&'msg ::protobuf::ProtoStr> {
    self.has_oneof_string().then(|| self.oneof_string())
  }
  pub fn oneof_string(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        93, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // oneof_bytes: optional bytes
  pub fn has_oneof_bytes(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(94)
    }
  }
  pub fn oneof_bytes_opt(self) -> ::std::option::Option<&'msg [u8]> {
    self.has_oneof_bytes().then(|| self.oneof_bytes())
  }
  pub fn oneof_bytes(self) -> ::protobuf::View<'msg, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        94, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }

  // oneof_bool: optional bool
  pub fn has_oneof_bool(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(95)
    }
  }
  pub fn oneof_bool_opt(self) -> ::std::option::Option<bool> {
    self.has_oneof_bool().then(|| self.oneof_bool())
  }
  pub fn oneof_bool(self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        95, (false).into()
      ).try_into().unwrap()
    }
  }

  // oneof_uint64: optional uint64
  pub fn has_oneof_uint64(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(96)
    }
  }
  pub fn oneof_uint64_opt(self) -> ::std::option::Option<u64> {
    self.has_oneof_uint64().then(|| self.oneof_uint64())
  }
  pub fn oneof_uint64(self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        96, (0u64).into()
      ).try_into().unwrap()
    }
  }

  // oneof_float: optional float
  pub fn has_oneof_float(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(97)
    }
  }
  pub fn oneof_float_opt(self) -> ::std::option::Option<f32> {
    self.has_oneof_float().then(|| self.oneof_float())
  }
  pub fn oneof_float(self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        97, (0f32).into()
      ).try_into().unwrap()
    }
  }

  // oneof_double: optional double
  pub fn has_oneof_double(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(98)
    }
  }
  pub fn oneof_double_opt(self) -> ::std::option::Option<f64> {
    self.has_oneof_double().then(|| self.oneof_double())
  }
  pub fn oneof_double(self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        98, (0f64).into()
      ).try_into().unwrap()
    }
  }

  // oneof_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn has_oneof_enum(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(99)
    }
  }
  pub fn oneof_enum_opt(self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    self.has_oneof_enum().then(|| self.oneof_enum())
  }
  pub fn oneof_enum(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        99, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }

  // oneof_null_value: optional enum google.protobuf.NullValue
  pub fn has_oneof_null_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(100)
    }
  }
  pub fn oneof_null_value_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_oneof_null_value().then(|| self.oneof_null_value())
  }
  pub fn oneof_null_value(self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        100, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }

  // optional_bool_wrapper: optional message google.protobuf.BoolValue
  pub fn has_optional_bool_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(101)
    }
  }
  pub fn optional_bool_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BoolValueView<'msg>> {
    self.has_optional_bool_wrapper().then(|| self.optional_bool_wrapper())
  }
  pub fn optional_bool_wrapper(self) -> super::google_protobuf_wrappers_proto::BoolValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(101)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BoolValueView::default())
  }

  // optional_int32_wrapper: optional message google.protobuf.Int32Value
  pub fn has_optional_int32_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(102)
    }
  }
  pub fn optional_int32_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int32ValueView<'msg>> {
    self.has_optional_int32_wrapper().then(|| self.optional_int32_wrapper())
  }
  pub fn optional_int32_wrapper(self) -> super::google_protobuf_wrappers_proto::Int32ValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(102)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int32ValueView::default())
  }

  // optional_int64_wrapper: optional message google.protobuf.Int64Value
  pub fn has_optional_int64_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(103)
    }
  }
  pub fn optional_int64_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int64ValueView<'msg>> {
    self.has_optional_int64_wrapper().then(|| self.optional_int64_wrapper())
  }
  pub fn optional_int64_wrapper(self) -> super::google_protobuf_wrappers_proto::Int64ValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(103)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int64ValueView::default())
  }

  // optional_uint32_wrapper: optional message google.protobuf.UInt32Value
  pub fn has_optional_uint32_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(104)
    }
  }
  pub fn optional_uint32_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt32ValueView<'msg>> {
    self.has_optional_uint32_wrapper().then(|| self.optional_uint32_wrapper())
  }
  pub fn optional_uint32_wrapper(self) -> super::google_protobuf_wrappers_proto::UInt32ValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(104)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt32ValueView::default())
  }

  // optional_uint64_wrapper: optional message google.protobuf.UInt64Value
  pub fn has_optional_uint64_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(105)
    }
  }
  pub fn optional_uint64_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt64ValueView<'msg>> {
    self.has_optional_uint64_wrapper().then(|| self.optional_uint64_wrapper())
  }
  pub fn optional_uint64_wrapper(self) -> super::google_protobuf_wrappers_proto::UInt64ValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(105)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt64ValueView::default())
  }

  // optional_float_wrapper: optional message google.protobuf.FloatValue
  pub fn has_optional_float_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(106)
    }
  }
  pub fn optional_float_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::FloatValueView<'msg>> {
    self.has_optional_float_wrapper().then(|| self.optional_float_wrapper())
  }
  pub fn optional_float_wrapper(self) -> super::google_protobuf_wrappers_proto::FloatValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(106)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::FloatValueView::default())
  }

  // optional_double_wrapper: optional message google.protobuf.DoubleValue
  pub fn has_optional_double_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(107)
    }
  }
  pub fn optional_double_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::DoubleValueView<'msg>> {
    self.has_optional_double_wrapper().then(|| self.optional_double_wrapper())
  }
  pub fn optional_double_wrapper(self) -> super::google_protobuf_wrappers_proto::DoubleValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(107)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::DoubleValueView::default())
  }

  // optional_string_wrapper: optional message google.protobuf.StringValue
  pub fn has_optional_string_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(108)
    }
  }
  pub fn optional_string_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::StringValueView<'msg>> {
    self.has_optional_string_wrapper().then(|| self.optional_string_wrapper())
  }
  pub fn optional_string_wrapper(self) -> super::google_protobuf_wrappers_proto::StringValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(108)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::StringValueView::default())
  }

  // optional_bytes_wrapper: optional message google.protobuf.BytesValue
  pub fn has_optional_bytes_wrapper(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(109)
    }
  }
  pub fn optional_bytes_wrapper_opt(self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BytesValueView<'msg>> {
    self.has_optional_bytes_wrapper().then(|| self.optional_bytes_wrapper())
  }
  pub fn optional_bytes_wrapper(self) -> super::google_protobuf_wrappers_proto::BytesValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(109)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BytesValueView::default())
  }

  // repeated_bool_wrapper: repeated message google.protobuf.BoolValue
  pub fn repeated_bool_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::BoolValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        110
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BoolValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_int32_wrapper: repeated message google.protobuf.Int32Value
  pub fn repeated_int32_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::Int32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        111
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_int64_wrapper: repeated message google.protobuf.Int64Value
  pub fn repeated_int64_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::Int64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        112
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_uint32_wrapper: repeated message google.protobuf.UInt32Value
  pub fn repeated_uint32_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::UInt32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        113
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_uint64_wrapper: repeated message google.protobuf.UInt64Value
  pub fn repeated_uint64_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::UInt64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        114
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_float_wrapper: repeated message google.protobuf.FloatValue
  pub fn repeated_float_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::FloatValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        115
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::FloatValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_double_wrapper: repeated message google.protobuf.DoubleValue
  pub fn repeated_double_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::DoubleValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        116
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::DoubleValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_string_wrapper: repeated message google.protobuf.StringValue
  pub fn repeated_string_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::StringValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        117
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::StringValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_bytes_wrapper: repeated message google.protobuf.BytesValue
  pub fn repeated_bytes_wrapper(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_wrappers_proto::BytesValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        118
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BytesValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // optional_duration: optional message google.protobuf.Duration
  pub fn has_optional_duration(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(119)
    }
  }
  pub fn optional_duration_opt(self) -> ::std::option::Option<super::google_protobuf_duration_proto::DurationView<'msg>> {
    self.has_optional_duration().then(|| self.optional_duration())
  }
  pub fn optional_duration(self) -> super::google_protobuf_duration_proto::DurationView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(119)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_duration_proto::DurationView::default())
  }

  // optional_timestamp: optional message google.protobuf.Timestamp
  pub fn has_optional_timestamp(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(120)
    }
  }
  pub fn optional_timestamp_opt(self) -> ::std::option::Option<super::google_protobuf_timestamp_proto::TimestampView<'msg>> {
    self.has_optional_timestamp().then(|| self.optional_timestamp())
  }
  pub fn optional_timestamp(self) -> super::google_protobuf_timestamp_proto::TimestampView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(120)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_timestamp_proto::TimestampView::default())
  }

  // optional_field_mask: optional message google.protobuf.FieldMask
  pub fn has_optional_field_mask(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(121)
    }
  }
  pub fn optional_field_mask_opt(self) -> ::std::option::Option<super::google_protobuf_field_mask_proto::FieldMaskView<'msg>> {
    self.has_optional_field_mask().then(|| self.optional_field_mask())
  }
  pub fn optional_field_mask(self) -> super::google_protobuf_field_mask_proto::FieldMaskView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(121)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_field_mask_proto::FieldMaskView::default())
  }

  // optional_struct: optional message google.protobuf.Struct
  pub fn has_optional_struct(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(122)
    }
  }
  pub fn optional_struct_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'msg>> {
    self.has_optional_struct().then(|| self.optional_struct())
  }
  pub fn optional_struct(self) -> super::google_protobuf_struct_proto::StructView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(122)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }

  // optional_any: optional message google.protobuf.Any
  pub fn has_optional_any(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(123)
    }
  }
  pub fn optional_any_opt(self) -> ::std::option::Option<super::google_protobuf_any_proto::AnyView<'msg>> {
    self.has_optional_any().then(|| self.optional_any())
  }
  pub fn optional_any(self) -> super::google_protobuf_any_proto::AnyView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(123)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_any_proto::AnyView::default())
  }

  // optional_value: optional message google.protobuf.Value
  pub fn has_optional_value(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(124)
    }
  }
  pub fn optional_value_opt(self) -> ::std::option::Option<super::google_protobuf_struct_proto::ValueView<'msg>> {
    self.has_optional_value().then(|| self.optional_value())
  }
  pub fn optional_value(self) -> super::google_protobuf_struct_proto::ValueView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(124)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ValueView::default())
  }

  // optional_null_value: optional enum google.protobuf.NullValue
  pub fn optional_null_value(self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        125, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }

  // optional_empty: optional message google.protobuf.Empty
  pub fn has_optional_empty(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(126)
    }
  }
  pub fn optional_empty_opt(self) -> ::std::option::Option<super::google_protobuf_empty_proto::EmptyView<'msg>> {
    self.has_optional_empty().then(|| self.optional_empty())
  }
  pub fn optional_empty(self) -> super::google_protobuf_empty_proto::EmptyView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(126)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_empty_proto::EmptyView::default())
  }

  // repeated_duration: repeated message google.protobuf.Duration
  pub fn repeated_duration(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_duration_proto::Duration> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        127
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_duration_proto::Duration>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_timestamp: repeated message google.protobuf.Timestamp
  pub fn repeated_timestamp(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_timestamp_proto::Timestamp> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        128
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_timestamp_proto::Timestamp>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_fieldmask: repeated message google.protobuf.FieldMask
  pub fn repeated_fieldmask(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_field_mask_proto::FieldMask> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        129
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_field_mask_proto::FieldMask>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_struct: repeated message google.protobuf.Struct
  pub fn repeated_struct(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_struct_proto::Struct> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        134
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Struct>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_any: repeated message google.protobuf.Any
  pub fn repeated_any(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_any_proto::Any> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        130
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_any_proto::Any>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_value: repeated message google.protobuf.Value
  pub fn repeated_value(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        131
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_list_value: repeated message google.protobuf.ListValue
  pub fn repeated_list_value(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_struct_proto::ListValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        132
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::ListValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // repeated_empty: repeated message google.protobuf.Empty
  pub fn repeated_empty(self) -> ::protobuf::RepeatedView<'msg, super::google_protobuf_empty_proto::Empty> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        133
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_empty_proto::Empty>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // fieldname1: optional int32
  pub fn fieldname1(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        135, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field_name2: optional int32
  pub fn field_name2(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        136, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // _field_name3: optional int32
  pub fn _field_name3(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        137, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field__name4_: optional int32
  pub fn field__name4_(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        138, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field0name5: optional int32
  pub fn field0name5(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        139, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field_0_name6: optional int32
  pub fn field_0_name6(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        140, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // fieldName7: optional int32
  pub fn fieldName7(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        141, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // FieldName8: optional int32
  pub fn FieldName8(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        142, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field_Name9: optional int32
  pub fn field_Name9(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        143, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // Field_Name10: optional int32
  pub fn Field_Name10(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        144, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // FIELD_NAME11: optional int32
  pub fn FIELD_NAME11(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        145, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // FIELD_name12: optional int32
  pub fn FIELD_name12(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        146, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // __field_name13: optional int32
  pub fn __field_name13(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        147, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // __Field_name14: optional int32
  pub fn __Field_name14(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        148, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field__name15: optional int32
  pub fn field__name15(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        149, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field__Name16: optional int32
  pub fn field__Name16(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        150, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // field_name17__: optional int32
  pub fn field_name17__(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        151, (0i32).into()
      ).try_into().unwrap()
    }
  }

  // Field_name18__: optional int32
  pub fn Field_name18__(self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        152, (0i32).into()
      ).try_into().unwrap()
    }
  }

  pub fn oneof_field(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof<'msg> {
    match self.oneof_field_case() {
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint32 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint32(self.oneof_uint32()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNestedMessage =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNestedMessage(self.oneof_nested_message()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofString =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofString(self.oneof_string()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBytes =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBytes(self.oneof_bytes()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBool =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBool(self.oneof_bool()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint64 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint64(self.oneof_uint64()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofFloat =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofFloat(self.oneof_float()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofDouble =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofDouble(self.oneof_double()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofEnum =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofEnum(self.oneof_enum()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNullValue =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNullValue(self.oneof_null_value()),
      _ => super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn oneof_field_case(self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(91);
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `TestAllTypesProto3View` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for TestAllTypesProto3View<'_> {}

// SAFETY:
// - `TestAllTypesProto3View` is `Send` because while its alive a `TestAllTypesProto3Mut` cannot.
// - `TestAllTypesProto3View` does not use thread-local data.
unsafe impl ::std::marker::Send for TestAllTypesProto3View<'_> {}

impl<'msg> ::protobuf::AsView for TestAllTypesProto3View<'msg> {
  type Proxied = TestAllTypesProto3;
  fn as_view(&self) -> ::protobuf::View<'msg, TestAllTypesProto3> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TestAllTypesProto3View<'msg> {
  fn into_view<'shorter>(self) -> TestAllTypesProto3View<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<TestAllTypesProto3> for TestAllTypesProto3View<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> TestAllTypesProto3 {
    let mut dst = TestAllTypesProto3::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<TestAllTypesProto3> for TestAllTypesProto3Mut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> TestAllTypesProto3 {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for TestAllTypesProto3 {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TestAllTypesProto3View<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for TestAllTypesProto3Mut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct TestAllTypesProto3Mut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, TestAllTypesProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for TestAllTypesProto3Mut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for TestAllTypesProto3Mut<'msg> {
  type Message = TestAllTypesProto3;
}

impl ::std::fmt::Debug for TestAllTypesProto3Mut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, TestAllTypesProto3>> for TestAllTypesProto3Mut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, TestAllTypesProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> TestAllTypesProto3Mut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, TestAllTypesProto3> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> TestAllTypesProto3 {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // optional_int32: optional int32
  pub fn optional_int32(&self) -> i32 {
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
  pub fn set_optional_int32(&mut self, val: i32) {
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

  // optional_int64: optional int64
  pub fn optional_int64(&self) -> i64 {
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
  pub fn set_optional_int64(&mut self, val: i64) {
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

  // optional_uint32: optional uint32
  pub fn optional_uint32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        2, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_uint32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        2, val.into()
      )
    }
  }

  // optional_uint64: optional uint64
  pub fn optional_uint64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        3, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_uint64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        3, val.into()
      )
    }
  }

  // optional_sint32: optional sint32
  pub fn optional_sint32(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        4, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sint32(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        4, val.into()
      )
    }
  }

  // optional_sint64: optional sint64
  pub fn optional_sint64(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        5, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sint64(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        5, val.into()
      )
    }
  }

  // optional_fixed32: optional fixed32
  pub fn optional_fixed32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        6, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_fixed32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        6, val.into()
      )
    }
  }

  // optional_fixed64: optional fixed64
  pub fn optional_fixed64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        7, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_fixed64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        7, val.into()
      )
    }
  }

  // optional_sfixed32: optional sfixed32
  pub fn optional_sfixed32(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        8, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sfixed32(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        8, val.into()
      )
    }
  }

  // optional_sfixed64: optional sfixed64
  pub fn optional_sfixed64(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        9, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sfixed64(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        9, val.into()
      )
    }
  }

  // optional_float: optional float
  pub fn optional_float(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        10, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_float(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        10, val.into()
      )
    }
  }

  // optional_double: optional double
  pub fn optional_double(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        11, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_double(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        11, val.into()
      )
    }
  }

  // optional_bool: optional bool
  pub fn optional_bool(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        12, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_bool(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        12, val.into()
      )
    }
  }

  // optional_string: optional string
  pub fn optional_string(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        13, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_string(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        13,
        val);
    }
  }

  // optional_bytes: optional bytes
  pub fn optional_bytes(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        14, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_optional_bytes(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        14,
        val);
    }
  }

  // optional_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_optional_nested_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(15)
    }
  }
  pub fn clear_optional_nested_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        15
      );
    }
  }
  pub fn optional_nested_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_>> {
    self.has_optional_nested_message().then(|| self.optional_nested_message())
  }
  pub fn optional_nested_message(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(15)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }
  pub fn optional_nested_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         15, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_nested_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        15,
        val
      );
    }
  }

  // optional_foreign_message: optional message protobuf_test_messages.proto3.ForeignMessage
  pub fn has_optional_foreign_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(16)
    }
  }
  pub fn clear_optional_foreign_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        16
      );
    }
  }
  pub fn optional_foreign_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'_>> {
    self.has_optional_foreign_message().then(|| self.optional_foreign_message())
  }
  pub fn optional_foreign_message(&self) -> super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(16)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::ForeignMessageView::default())
  }
  pub fn optional_foreign_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::ForeignMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         16, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_foreign_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        16,
        val
      );
    }
  }

  // optional_nested_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn optional_nested_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        17, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_nested_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        17, val.into()
      )
    }
  }

  // optional_foreign_enum: optional enum protobuf_test_messages.proto3.ForeignEnum
  pub fn optional_foreign_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::ForeignEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        18, (super::google_protobuf_test_messages_proto3_proto::ForeignEnum::ForeignFoo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_foreign_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::ForeignEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        18, val.into()
      )
    }
  }

  // optional_aliased_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.AliasedEnum
  pub fn optional_aliased_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        19, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum::AliasFoo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_aliased_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        19, val.into()
      )
    }
  }

  // optional_string_piece: optional string
  pub fn optional_string_piece(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        20, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_string_piece(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        20,
        val);
    }
  }

  // optional_cord: optional string
  pub fn optional_cord(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        21, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_cord(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        21,
        val);
    }
  }

  // recursive_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_recursive_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(22)
    }
  }
  pub fn clear_recursive_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        22
      );
    }
  }
  pub fn recursive_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_>> {
    self.has_recursive_message().then(|| self.recursive_message())
  }
  pub fn recursive_message(&self) -> super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(22)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }
  pub fn recursive_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3Mut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         22, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_recursive_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        22,
        val
      );
    }
  }

  // repeated_int32: repeated int32
  pub fn repeated_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        23
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        23,
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
  pub fn set_repeated_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        23,
        src);
    }
  }

  // repeated_int64: repeated int64
  pub fn repeated_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        24
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        24,
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
  pub fn set_repeated_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        24,
        src);
    }
  }

  // repeated_uint32: repeated uint32
  pub fn repeated_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        25
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        25,
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
  pub fn set_repeated_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        25,
        src);
    }
  }

  // repeated_uint64: repeated uint64
  pub fn repeated_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        26
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        26,
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
  pub fn set_repeated_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        26,
        src);
    }
  }

  // repeated_sint32: repeated sint32
  pub fn repeated_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        27
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        27,
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
  pub fn set_repeated_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        27,
        src);
    }
  }

  // repeated_sint64: repeated sint64
  pub fn repeated_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        28
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        28,
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
  pub fn set_repeated_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        28,
        src);
    }
  }

  // repeated_fixed32: repeated fixed32
  pub fn repeated_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        29
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        29,
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
  pub fn set_repeated_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        29,
        src);
    }
  }

  // repeated_fixed64: repeated fixed64
  pub fn repeated_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        30
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        30,
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
  pub fn set_repeated_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        30,
        src);
    }
  }

  // repeated_sfixed32: repeated sfixed32
  pub fn repeated_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        31
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        31,
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
  pub fn set_repeated_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        31,
        src);
    }
  }

  // repeated_sfixed64: repeated sfixed64
  pub fn repeated_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        32
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        32,
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
  pub fn set_repeated_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        32,
        src);
    }
  }

  // repeated_float: repeated float
  pub fn repeated_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        33
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        33,
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
  pub fn set_repeated_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        33,
        src);
    }
  }

  // repeated_double: repeated double
  pub fn repeated_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        34
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        34,
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
  pub fn set_repeated_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        34,
        src);
    }
  }

  // repeated_bool: repeated bool
  pub fn repeated_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        35
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        35,
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
  pub fn set_repeated_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        35,
        src);
    }
  }

  // repeated_string: repeated string
  pub fn repeated_string(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        36
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        36,
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
  pub fn set_repeated_string(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        36,
        src);
    }
  }

  // repeated_bytes: repeated bytes
  pub fn repeated_bytes(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoBytes> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        37
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoBytes>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bytes_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoBytes> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        37,
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
  pub fn set_repeated_bytes(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoBytes>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        37,
        src);
    }
  }

  // repeated_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn repeated_nested_message(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        38
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_nested_message_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        38,
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
  pub fn set_repeated_nested_message(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        38,
        src);
    }
  }

  // repeated_foreign_message: repeated message protobuf_test_messages.proto3.ForeignMessage
  pub fn repeated_foreign_message(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        39
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_foreign_message_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        39,
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
  pub fn set_repeated_foreign_message(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        39,
        src);
    }
  }

  // repeated_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn repeated_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        40
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        40,
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
  pub fn set_repeated_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        40,
        src);
    }
  }

  // repeated_foreign_enum: repeated enum protobuf_test_messages.proto3.ForeignEnum
  pub fn repeated_foreign_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        41
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_foreign_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        41,
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
  pub fn set_repeated_foreign_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::ForeignEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        41,
        src);
    }
  }

  // repeated_string_piece: repeated string
  pub fn repeated_string_piece(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        42
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_piece_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        42,
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
  pub fn set_repeated_string_piece(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        42,
        src);
    }
  }

  // repeated_cord: repeated string
  pub fn repeated_cord(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        43
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_cord_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        43,
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
  pub fn set_repeated_cord(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        43,
        src);
    }
  }

  // packed_int32: repeated int32
  pub fn packed_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        63
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        63,
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
  pub fn set_packed_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        63,
        src);
    }
  }

  // packed_int64: repeated int64
  pub fn packed_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        64
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        64,
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
  pub fn set_packed_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        64,
        src);
    }
  }

  // packed_uint32: repeated uint32
  pub fn packed_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        65
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        65,
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
  pub fn set_packed_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        65,
        src);
    }
  }

  // packed_uint64: repeated uint64
  pub fn packed_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        66
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        66,
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
  pub fn set_packed_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        66,
        src);
    }
  }

  // packed_sint32: repeated sint32
  pub fn packed_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        67
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        67,
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
  pub fn set_packed_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        67,
        src);
    }
  }

  // packed_sint64: repeated sint64
  pub fn packed_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        68
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        68,
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
  pub fn set_packed_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        68,
        src);
    }
  }

  // packed_fixed32: repeated fixed32
  pub fn packed_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        69
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        69,
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
  pub fn set_packed_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        69,
        src);
    }
  }

  // packed_fixed64: repeated fixed64
  pub fn packed_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        70
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        70,
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
  pub fn set_packed_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        70,
        src);
    }
  }

  // packed_sfixed32: repeated sfixed32
  pub fn packed_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        71
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        71,
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
  pub fn set_packed_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        71,
        src);
    }
  }

  // packed_sfixed64: repeated sfixed64
  pub fn packed_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        72
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        72,
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
  pub fn set_packed_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        72,
        src);
    }
  }

  // packed_float: repeated float
  pub fn packed_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        73
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        73,
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
  pub fn set_packed_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        73,
        src);
    }
  }

  // packed_double: repeated double
  pub fn packed_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        74
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        74,
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
  pub fn set_packed_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        74,
        src);
    }
  }

  // packed_bool: repeated bool
  pub fn packed_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        75
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        75,
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
  pub fn set_packed_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        75,
        src);
    }
  }

  // packed_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn packed_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        76
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        76,
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
  pub fn set_packed_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        76,
        src);
    }
  }

  // unpacked_int32: repeated int32
  pub fn unpacked_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        77
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        77,
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
  pub fn set_unpacked_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        77,
        src);
    }
  }

  // unpacked_int64: repeated int64
  pub fn unpacked_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        78
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        78,
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
  pub fn set_unpacked_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        78,
        src);
    }
  }

  // unpacked_uint32: repeated uint32
  pub fn unpacked_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        79
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        79,
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
  pub fn set_unpacked_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        79,
        src);
    }
  }

  // unpacked_uint64: repeated uint64
  pub fn unpacked_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        80
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        80,
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
  pub fn set_unpacked_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        80,
        src);
    }
  }

  // unpacked_sint32: repeated sint32
  pub fn unpacked_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        81
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        81,
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
  pub fn set_unpacked_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        81,
        src);
    }
  }

  // unpacked_sint64: repeated sint64
  pub fn unpacked_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        82
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        82,
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
  pub fn set_unpacked_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        82,
        src);
    }
  }

  // unpacked_fixed32: repeated fixed32
  pub fn unpacked_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        83
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        83,
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
  pub fn set_unpacked_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        83,
        src);
    }
  }

  // unpacked_fixed64: repeated fixed64
  pub fn unpacked_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        84
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        84,
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
  pub fn set_unpacked_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        84,
        src);
    }
  }

  // unpacked_sfixed32: repeated sfixed32
  pub fn unpacked_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        85
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        85,
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
  pub fn set_unpacked_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        85,
        src);
    }
  }

  // unpacked_sfixed64: repeated sfixed64
  pub fn unpacked_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        86
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        86,
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
  pub fn set_unpacked_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        86,
        src);
    }
  }

  // unpacked_float: repeated float
  pub fn unpacked_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        87
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        87,
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
  pub fn set_unpacked_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        87,
        src);
    }
  }

  // unpacked_double: repeated double
  pub fn unpacked_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        88
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        88,
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
  pub fn set_unpacked_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        88,
        src);
    }
  }

  // unpacked_bool: repeated bool
  pub fn unpacked_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        89
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        89,
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
  pub fn set_unpacked_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        89,
        src);
    }
  }

  // unpacked_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn unpacked_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        90
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        90,
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
  pub fn set_unpacked_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        90,
        src);
    }
  }

  // map_int32_int32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32Int32Entry
  pub fn map_int32_int32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(44)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_int32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          44, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_int32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        44,
        src);
    }
  }

  // map_int64_int64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt64Int64Entry
  pub fn map_int64_int64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(45)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int64_int64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          45, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int64_int64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        45,
        src);
    }
  }

  // map_uint32_uint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint32Uint32Entry
  pub fn map_uint32_uint32(&self)
    -> ::protobuf::MapView<'_, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(46)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_uint32_uint32_mut(&mut self)
    -> ::protobuf::MapMut<'_, u32, u32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          46, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_uint32_uint32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u32, u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        46,
        src);
    }
  }

  // map_uint64_uint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint64Uint64Entry
  pub fn map_uint64_uint64(&self)
    -> ::protobuf::MapView<'_, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(47)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_uint64_uint64_mut(&mut self)
    -> ::protobuf::MapMut<'_, u64, u64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          47, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_uint64_uint64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u64, u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        47,
        src);
    }
  }

  // map_sint32_sint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint32Sint32Entry
  pub fn map_sint32_sint32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(48)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sint32_sint32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          48, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sint32_sint32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        48,
        src);
    }
  }

  // map_sint64_sint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint64Sint64Entry
  pub fn map_sint64_sint64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(49)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sint64_sint64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          49, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sint64_sint64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        49,
        src);
    }
  }

  // map_fixed32_fixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed32Fixed32Entry
  pub fn map_fixed32_fixed32(&self)
    -> ::protobuf::MapView<'_, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(50)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_fixed32_fixed32_mut(&mut self)
    -> ::protobuf::MapMut<'_, u32, u32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          50, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_fixed32_fixed32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u32, u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        50,
        src);
    }
  }

  // map_fixed64_fixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed64Fixed64Entry
  pub fn map_fixed64_fixed64(&self)
    -> ::protobuf::MapView<'_, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(51)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_fixed64_fixed64_mut(&mut self)
    -> ::protobuf::MapMut<'_, u64, u64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          51, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_fixed64_fixed64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u64, u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        51,
        src);
    }
  }

  // map_sfixed32_sfixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed32Sfixed32Entry
  pub fn map_sfixed32_sfixed32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(52)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sfixed32_sfixed32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          52, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sfixed32_sfixed32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        52,
        src);
    }
  }

  // map_sfixed64_sfixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed64Sfixed64Entry
  pub fn map_sfixed64_sfixed64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(53)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sfixed64_sfixed64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          53, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sfixed64_sfixed64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        53,
        src);
    }
  }

  // map_int32_float: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32FloatEntry
  pub fn map_int32_float(&self)
    -> ::protobuf::MapView<'_, i32, f32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(54)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_float_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, f32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          54, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_float(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        54,
        src);
    }
  }

  // map_int32_double: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32DoubleEntry
  pub fn map_int32_double(&self)
    -> ::protobuf::MapView<'_, i32, f64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(55)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_double_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, f64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          55, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_double(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        55,
        src);
    }
  }

  // map_bool_bool: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapBoolBoolEntry
  pub fn map_bool_bool(&self)
    -> ::protobuf::MapView<'_, bool, bool> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(56)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<bool, bool>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_bool_bool_mut(&mut self)
    -> ::protobuf::MapMut<'_, bool, bool> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          56, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_bool_bool(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<bool, bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        56,
        src);
    }
  }

  // map_string_string: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringStringEntry
  pub fn map_string_string(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(57)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_string_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          57, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_string(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        57,
        src);
    }
  }

  // map_string_bytes: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringBytesEntry
  pub fn map_string_bytes(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoBytes> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(58)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoBytes>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_bytes_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoBytes> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          58, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_bytes(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoBytes>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        58,
        src);
    }
  }

  // map_string_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedMessageEntry
  pub fn map_string_nested_message(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(59)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_nested_message_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          59, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_nested_message(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        59,
        src);
    }
  }

  // map_string_foreign_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignMessageEntry
  pub fn map_string_foreign_message(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(60)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_foreign_message_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          60, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_foreign_message(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        60,
        src);
    }
  }

  // map_string_nested_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedEnumEntry
  pub fn map_string_nested_enum(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(61)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_nested_enum_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          61, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_nested_enum(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        61,
        src);
    }
  }

  // map_string_foreign_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignEnumEntry
  pub fn map_string_foreign_enum(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(62)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_foreign_enum_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          62, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_foreign_enum(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        62,
        src);
    }
  }

  // oneof_uint32: optional uint32
  pub fn has_oneof_uint32(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(91)
    }
  }
  pub fn clear_oneof_uint32(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        91
      );
    }
  }
  pub fn oneof_uint32_opt(&self) -> ::std::option::Option<u32> {
    self.has_oneof_uint32().then(|| self.oneof_uint32())
  }
  pub fn oneof_uint32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        91, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_uint32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        91, val.into()
      )
    }
  }

  // oneof_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_oneof_nested_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(92)
    }
  }
  pub fn clear_oneof_nested_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        92
      );
    }
  }
  pub fn oneof_nested_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_>> {
    self.has_oneof_nested_message().then(|| self.oneof_nested_message())
  }
  pub fn oneof_nested_message(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(92)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }
  pub fn oneof_nested_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         92, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_oneof_nested_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        92,
        val
      );
    }
  }

  // oneof_string: optional string
  pub fn has_oneof_string(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(93)
    }
  }
  pub fn clear_oneof_string(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        93
      );
    }
  }
  pub fn oneof_string_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_oneof_string().then(|| self.oneof_string())
  }
  pub fn oneof_string(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        93, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_oneof_string(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        93,
        val);
    }
  }

  // oneof_bytes: optional bytes
  pub fn has_oneof_bytes(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(94)
    }
  }
  pub fn clear_oneof_bytes(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        94
      );
    }
  }
  pub fn oneof_bytes_opt(&self) -> ::std::option::Option<&'_ [u8]> {
    self.has_oneof_bytes().then(|| self.oneof_bytes())
  }
  pub fn oneof_bytes(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        94, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_oneof_bytes(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        94,
        val);
    }
  }

  // oneof_bool: optional bool
  pub fn has_oneof_bool(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(95)
    }
  }
  pub fn clear_oneof_bool(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        95
      );
    }
  }
  pub fn oneof_bool_opt(&self) -> ::std::option::Option<bool> {
    self.has_oneof_bool().then(|| self.oneof_bool())
  }
  pub fn oneof_bool(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        95, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_bool(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        95, val.into()
      )
    }
  }

  // oneof_uint64: optional uint64
  pub fn has_oneof_uint64(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(96)
    }
  }
  pub fn clear_oneof_uint64(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        96
      );
    }
  }
  pub fn oneof_uint64_opt(&self) -> ::std::option::Option<u64> {
    self.has_oneof_uint64().then(|| self.oneof_uint64())
  }
  pub fn oneof_uint64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        96, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_uint64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        96, val.into()
      )
    }
  }

  // oneof_float: optional float
  pub fn has_oneof_float(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(97)
    }
  }
  pub fn clear_oneof_float(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        97
      );
    }
  }
  pub fn oneof_float_opt(&self) -> ::std::option::Option<f32> {
    self.has_oneof_float().then(|| self.oneof_float())
  }
  pub fn oneof_float(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        97, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_float(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        97, val.into()
      )
    }
  }

  // oneof_double: optional double
  pub fn has_oneof_double(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(98)
    }
  }
  pub fn clear_oneof_double(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        98
      );
    }
  }
  pub fn oneof_double_opt(&self) -> ::std::option::Option<f64> {
    self.has_oneof_double().then(|| self.oneof_double())
  }
  pub fn oneof_double(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        98, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_double(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        98, val.into()
      )
    }
  }

  // oneof_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn has_oneof_enum(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(99)
    }
  }
  pub fn clear_oneof_enum(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        99
      );
    }
  }
  pub fn oneof_enum_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    self.has_oneof_enum().then(|| self.oneof_enum())
  }
  pub fn oneof_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        99, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        99, val.into()
      )
    }
  }

  // oneof_null_value: optional enum google.protobuf.NullValue
  pub fn has_oneof_null_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(100)
    }
  }
  pub fn clear_oneof_null_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        100
      );
    }
  }
  pub fn oneof_null_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_oneof_null_value().then(|| self.oneof_null_value())
  }
  pub fn oneof_null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        100, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        100, val.into()
      )
    }
  }

  // optional_bool_wrapper: optional message google.protobuf.BoolValue
  pub fn has_optional_bool_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(101)
    }
  }
  pub fn clear_optional_bool_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        101
      );
    }
  }
  pub fn optional_bool_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BoolValueView<'_>> {
    self.has_optional_bool_wrapper().then(|| self.optional_bool_wrapper())
  }
  pub fn optional_bool_wrapper(&self) -> super::google_protobuf_wrappers_proto::BoolValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(101)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BoolValueView::default())
  }
  pub fn optional_bool_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::BoolValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         101, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_bool_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::BoolValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        101,
        val
      );
    }
  }

  // optional_int32_wrapper: optional message google.protobuf.Int32Value
  pub fn has_optional_int32_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(102)
    }
  }
  pub fn clear_optional_int32_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        102
      );
    }
  }
  pub fn optional_int32_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int32ValueView<'_>> {
    self.has_optional_int32_wrapper().then(|| self.optional_int32_wrapper())
  }
  pub fn optional_int32_wrapper(&self) -> super::google_protobuf_wrappers_proto::Int32ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(102)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int32ValueView::default())
  }
  pub fn optional_int32_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::Int32ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         102, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_int32_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::Int32Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        102,
        val
      );
    }
  }

  // optional_int64_wrapper: optional message google.protobuf.Int64Value
  pub fn has_optional_int64_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(103)
    }
  }
  pub fn clear_optional_int64_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        103
      );
    }
  }
  pub fn optional_int64_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int64ValueView<'_>> {
    self.has_optional_int64_wrapper().then(|| self.optional_int64_wrapper())
  }
  pub fn optional_int64_wrapper(&self) -> super::google_protobuf_wrappers_proto::Int64ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(103)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int64ValueView::default())
  }
  pub fn optional_int64_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::Int64ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         103, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_int64_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::Int64Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        103,
        val
      );
    }
  }

  // optional_uint32_wrapper: optional message google.protobuf.UInt32Value
  pub fn has_optional_uint32_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(104)
    }
  }
  pub fn clear_optional_uint32_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        104
      );
    }
  }
  pub fn optional_uint32_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt32ValueView<'_>> {
    self.has_optional_uint32_wrapper().then(|| self.optional_uint32_wrapper())
  }
  pub fn optional_uint32_wrapper(&self) -> super::google_protobuf_wrappers_proto::UInt32ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(104)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt32ValueView::default())
  }
  pub fn optional_uint32_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::UInt32ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         104, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_uint32_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::UInt32Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        104,
        val
      );
    }
  }

  // optional_uint64_wrapper: optional message google.protobuf.UInt64Value
  pub fn has_optional_uint64_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(105)
    }
  }
  pub fn clear_optional_uint64_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        105
      );
    }
  }
  pub fn optional_uint64_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt64ValueView<'_>> {
    self.has_optional_uint64_wrapper().then(|| self.optional_uint64_wrapper())
  }
  pub fn optional_uint64_wrapper(&self) -> super::google_protobuf_wrappers_proto::UInt64ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(105)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt64ValueView::default())
  }
  pub fn optional_uint64_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::UInt64ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         105, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_uint64_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::UInt64Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        105,
        val
      );
    }
  }

  // optional_float_wrapper: optional message google.protobuf.FloatValue
  pub fn has_optional_float_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(106)
    }
  }
  pub fn clear_optional_float_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        106
      );
    }
  }
  pub fn optional_float_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::FloatValueView<'_>> {
    self.has_optional_float_wrapper().then(|| self.optional_float_wrapper())
  }
  pub fn optional_float_wrapper(&self) -> super::google_protobuf_wrappers_proto::FloatValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(106)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::FloatValueView::default())
  }
  pub fn optional_float_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::FloatValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         106, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_float_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::FloatValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        106,
        val
      );
    }
  }

  // optional_double_wrapper: optional message google.protobuf.DoubleValue
  pub fn has_optional_double_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(107)
    }
  }
  pub fn clear_optional_double_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        107
      );
    }
  }
  pub fn optional_double_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::DoubleValueView<'_>> {
    self.has_optional_double_wrapper().then(|| self.optional_double_wrapper())
  }
  pub fn optional_double_wrapper(&self) -> super::google_protobuf_wrappers_proto::DoubleValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(107)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::DoubleValueView::default())
  }
  pub fn optional_double_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::DoubleValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         107, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_double_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::DoubleValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        107,
        val
      );
    }
  }

  // optional_string_wrapper: optional message google.protobuf.StringValue
  pub fn has_optional_string_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(108)
    }
  }
  pub fn clear_optional_string_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        108
      );
    }
  }
  pub fn optional_string_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::StringValueView<'_>> {
    self.has_optional_string_wrapper().then(|| self.optional_string_wrapper())
  }
  pub fn optional_string_wrapper(&self) -> super::google_protobuf_wrappers_proto::StringValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(108)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::StringValueView::default())
  }
  pub fn optional_string_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::StringValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         108, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_string_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::StringValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        108,
        val
      );
    }
  }

  // optional_bytes_wrapper: optional message google.protobuf.BytesValue
  pub fn has_optional_bytes_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(109)
    }
  }
  pub fn clear_optional_bytes_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        109
      );
    }
  }
  pub fn optional_bytes_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BytesValueView<'_>> {
    self.has_optional_bytes_wrapper().then(|| self.optional_bytes_wrapper())
  }
  pub fn optional_bytes_wrapper(&self) -> super::google_protobuf_wrappers_proto::BytesValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(109)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BytesValueView::default())
  }
  pub fn optional_bytes_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::BytesValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         109, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_bytes_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::BytesValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        109,
        val
      );
    }
  }

  // repeated_bool_wrapper: repeated message google.protobuf.BoolValue
  pub fn repeated_bool_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::BoolValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        110
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BoolValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bool_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::BoolValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        110,
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
  pub fn set_repeated_bool_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::BoolValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        110,
        src);
    }
  }

  // repeated_int32_wrapper: repeated message google.protobuf.Int32Value
  pub fn repeated_int32_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::Int32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        111
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int32_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::Int32Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        111,
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
  pub fn set_repeated_int32_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::Int32Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        111,
        src);
    }
  }

  // repeated_int64_wrapper: repeated message google.protobuf.Int64Value
  pub fn repeated_int64_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::Int64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        112
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int64_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::Int64Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        112,
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
  pub fn set_repeated_int64_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::Int64Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        112,
        src);
    }
  }

  // repeated_uint32_wrapper: repeated message google.protobuf.UInt32Value
  pub fn repeated_uint32_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::UInt32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        113
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint32_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::UInt32Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        113,
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
  pub fn set_repeated_uint32_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::UInt32Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        113,
        src);
    }
  }

  // repeated_uint64_wrapper: repeated message google.protobuf.UInt64Value
  pub fn repeated_uint64_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::UInt64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        114
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint64_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::UInt64Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        114,
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
  pub fn set_repeated_uint64_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::UInt64Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        114,
        src);
    }
  }

  // repeated_float_wrapper: repeated message google.protobuf.FloatValue
  pub fn repeated_float_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::FloatValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        115
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::FloatValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_float_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::FloatValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        115,
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
  pub fn set_repeated_float_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::FloatValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        115,
        src);
    }
  }

  // repeated_double_wrapper: repeated message google.protobuf.DoubleValue
  pub fn repeated_double_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::DoubleValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        116
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::DoubleValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_double_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::DoubleValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        116,
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
  pub fn set_repeated_double_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::DoubleValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        116,
        src);
    }
  }

  // repeated_string_wrapper: repeated message google.protobuf.StringValue
  pub fn repeated_string_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::StringValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        117
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::StringValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::StringValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        117,
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
  pub fn set_repeated_string_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::StringValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        117,
        src);
    }
  }

  // repeated_bytes_wrapper: repeated message google.protobuf.BytesValue
  pub fn repeated_bytes_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::BytesValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        118
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BytesValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bytes_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::BytesValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        118,
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
  pub fn set_repeated_bytes_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::BytesValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        118,
        src);
    }
  }

  // optional_duration: optional message google.protobuf.Duration
  pub fn has_optional_duration(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(119)
    }
  }
  pub fn clear_optional_duration(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        119
      );
    }
  }
  pub fn optional_duration_opt(&self) -> ::std::option::Option<super::google_protobuf_duration_proto::DurationView<'_>> {
    self.has_optional_duration().then(|| self.optional_duration())
  }
  pub fn optional_duration(&self) -> super::google_protobuf_duration_proto::DurationView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(119)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_duration_proto::DurationView::default())
  }
  pub fn optional_duration_mut(&mut self) -> super::google_protobuf_duration_proto::DurationMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         119, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_duration(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_duration_proto::Duration>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        119,
        val
      );
    }
  }

  // optional_timestamp: optional message google.protobuf.Timestamp
  pub fn has_optional_timestamp(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(120)
    }
  }
  pub fn clear_optional_timestamp(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        120
      );
    }
  }
  pub fn optional_timestamp_opt(&self) -> ::std::option::Option<super::google_protobuf_timestamp_proto::TimestampView<'_>> {
    self.has_optional_timestamp().then(|| self.optional_timestamp())
  }
  pub fn optional_timestamp(&self) -> super::google_protobuf_timestamp_proto::TimestampView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(120)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_timestamp_proto::TimestampView::default())
  }
  pub fn optional_timestamp_mut(&mut self) -> super::google_protobuf_timestamp_proto::TimestampMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         120, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_timestamp(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_timestamp_proto::Timestamp>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        120,
        val
      );
    }
  }

  // optional_field_mask: optional message google.protobuf.FieldMask
  pub fn has_optional_field_mask(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(121)
    }
  }
  pub fn clear_optional_field_mask(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        121
      );
    }
  }
  pub fn optional_field_mask_opt(&self) -> ::std::option::Option<super::google_protobuf_field_mask_proto::FieldMaskView<'_>> {
    self.has_optional_field_mask().then(|| self.optional_field_mask())
  }
  pub fn optional_field_mask(&self) -> super::google_protobuf_field_mask_proto::FieldMaskView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(121)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_field_mask_proto::FieldMaskView::default())
  }
  pub fn optional_field_mask_mut(&mut self) -> super::google_protobuf_field_mask_proto::FieldMaskMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         121, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_field_mask(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_field_mask_proto::FieldMask>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        121,
        val
      );
    }
  }

  // optional_struct: optional message google.protobuf.Struct
  pub fn has_optional_struct(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(122)
    }
  }
  pub fn clear_optional_struct(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        122
      );
    }
  }
  pub fn optional_struct_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'_>> {
    self.has_optional_struct().then(|| self.optional_struct())
  }
  pub fn optional_struct(&self) -> super::google_protobuf_struct_proto::StructView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(122)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }
  pub fn optional_struct_mut(&mut self) -> super::google_protobuf_struct_proto::StructMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         122, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_struct(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Struct>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        122,
        val
      );
    }
  }

  // optional_any: optional message google.protobuf.Any
  pub fn has_optional_any(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(123)
    }
  }
  pub fn clear_optional_any(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        123
      );
    }
  }
  pub fn optional_any_opt(&self) -> ::std::option::Option<super::google_protobuf_any_proto::AnyView<'_>> {
    self.has_optional_any().then(|| self.optional_any())
  }
  pub fn optional_any(&self) -> super::google_protobuf_any_proto::AnyView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(123)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_any_proto::AnyView::default())
  }
  pub fn optional_any_mut(&mut self) -> super::google_protobuf_any_proto::AnyMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         123, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_any(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_any_proto::Any>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        123,
        val
      );
    }
  }

  // optional_value: optional message google.protobuf.Value
  pub fn has_optional_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(124)
    }
  }
  pub fn clear_optional_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        124
      );
    }
  }
  pub fn optional_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::ValueView<'_>> {
    self.has_optional_value().then(|| self.optional_value())
  }
  pub fn optional_value(&self) -> super::google_protobuf_struct_proto::ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(124)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ValueView::default())
  }
  pub fn optional_value_mut(&mut self) -> super::google_protobuf_struct_proto::ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         124, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        124,
        val
      );
    }
  }

  // optional_null_value: optional enum google.protobuf.NullValue
  pub fn optional_null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        125, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        125, val.into()
      )
    }
  }

  // optional_empty: optional message google.protobuf.Empty
  pub fn has_optional_empty(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(126)
    }
  }
  pub fn clear_optional_empty(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        126
      );
    }
  }
  pub fn optional_empty_opt(&self) -> ::std::option::Option<super::google_protobuf_empty_proto::EmptyView<'_>> {
    self.has_optional_empty().then(|| self.optional_empty())
  }
  pub fn optional_empty(&self) -> super::google_protobuf_empty_proto::EmptyView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(126)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_empty_proto::EmptyView::default())
  }
  pub fn optional_empty_mut(&mut self) -> super::google_protobuf_empty_proto::EmptyMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         126, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_empty(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_empty_proto::Empty>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        126,
        val
      );
    }
  }

  // repeated_duration: repeated message google.protobuf.Duration
  pub fn repeated_duration(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_duration_proto::Duration> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        127
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_duration_proto::Duration>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_duration_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_duration_proto::Duration> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        127,
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
  pub fn set_repeated_duration(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_duration_proto::Duration>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        127,
        src);
    }
  }

  // repeated_timestamp: repeated message google.protobuf.Timestamp
  pub fn repeated_timestamp(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_timestamp_proto::Timestamp> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        128
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_timestamp_proto::Timestamp>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_timestamp_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_timestamp_proto::Timestamp> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        128,
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
  pub fn set_repeated_timestamp(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_timestamp_proto::Timestamp>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        128,
        src);
    }
  }

  // repeated_fieldmask: repeated message google.protobuf.FieldMask
  pub fn repeated_fieldmask(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_field_mask_proto::FieldMask> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        129
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_field_mask_proto::FieldMask>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fieldmask_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_field_mask_proto::FieldMask> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        129,
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
  pub fn set_repeated_fieldmask(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_field_mask_proto::FieldMask>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        129,
        src);
    }
  }

  // repeated_struct: repeated message google.protobuf.Struct
  pub fn repeated_struct(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Struct> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        134
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Struct>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_struct_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Struct> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        134,
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
  pub fn set_repeated_struct(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Struct>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        134,
        src);
    }
  }

  // repeated_any: repeated message google.protobuf.Any
  pub fn repeated_any(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_any_proto::Any> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        130
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_any_proto::Any>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_any_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_any_proto::Any> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        130,
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
  pub fn set_repeated_any(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_any_proto::Any>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        130,
        src);
    }
  }

  // repeated_value: repeated message google.protobuf.Value
  pub fn repeated_value(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        131
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_value_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        131,
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
  pub fn set_repeated_value(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        131,
        src);
    }
  }

  // repeated_list_value: repeated message google.protobuf.ListValue
  pub fn repeated_list_value(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::ListValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        132
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::ListValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_list_value_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::ListValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        132,
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
  pub fn set_repeated_list_value(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::ListValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        132,
        src);
    }
  }

  // repeated_empty: repeated message google.protobuf.Empty
  pub fn repeated_empty(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_empty_proto::Empty> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        133
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_empty_proto::Empty>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_empty_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_empty_proto::Empty> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        133,
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
  pub fn set_repeated_empty(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_empty_proto::Empty>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        133,
        src);
    }
  }

  // fieldname1: optional int32
  pub fn fieldname1(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        135, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_fieldname1(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        135, val.into()
      )
    }
  }

  // field_name2: optional int32
  pub fn field_name2(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        136, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_name2(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        136, val.into()
      )
    }
  }

  // _field_name3: optional int32
  pub fn _field_name3(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        137, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set__field_name3(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        137, val.into()
      )
    }
  }

  // field__name4_: optional int32
  pub fn field__name4_(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        138, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__name4_(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        138, val.into()
      )
    }
  }

  // field0name5: optional int32
  pub fn field0name5(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        139, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field0name5(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        139, val.into()
      )
    }
  }

  // field_0_name6: optional int32
  pub fn field_0_name6(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        140, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_0_name6(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        140, val.into()
      )
    }
  }

  // fieldName7: optional int32
  pub fn fieldName7(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        141, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_fieldName7(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        141, val.into()
      )
    }
  }

  // FieldName8: optional int32
  pub fn FieldName8(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        142, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FieldName8(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        142, val.into()
      )
    }
  }

  // field_Name9: optional int32
  pub fn field_Name9(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        143, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_Name9(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        143, val.into()
      )
    }
  }

  // Field_Name10: optional int32
  pub fn Field_Name10(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        144, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_Field_Name10(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        144, val.into()
      )
    }
  }

  // FIELD_NAME11: optional int32
  pub fn FIELD_NAME11(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        145, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FIELD_NAME11(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        145, val.into()
      )
    }
  }

  // FIELD_name12: optional int32
  pub fn FIELD_name12(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        146, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FIELD_name12(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        146, val.into()
      )
    }
  }

  // __field_name13: optional int32
  pub fn __field_name13(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        147, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set___field_name13(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        147, val.into()
      )
    }
  }

  // __Field_name14: optional int32
  pub fn __Field_name14(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        148, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set___Field_name14(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        148, val.into()
      )
    }
  }

  // field__name15: optional int32
  pub fn field__name15(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        149, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__name15(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        149, val.into()
      )
    }
  }

  // field__Name16: optional int32
  pub fn field__Name16(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        150, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__Name16(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        150, val.into()
      )
    }
  }

  // field_name17__: optional int32
  pub fn field_name17__(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        151, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_name17__(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        151, val.into()
      )
    }
  }

  // Field_name18__: optional int32
  pub fn Field_name18__(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        152, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_Field_name18__(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        152, val.into()
      )
    }
  }

  pub fn oneof_field(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof<'_> {
    match &self.oneof_field_case() {
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint32 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint32(self.oneof_uint32()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNestedMessage =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNestedMessage(self.oneof_nested_message()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofString =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofString(self.oneof_string()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBytes =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBytes(self.oneof_bytes()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBool =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBool(self.oneof_bool()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint64 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint64(self.oneof_uint64()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofFloat =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofFloat(self.oneof_float()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofDouble =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofDouble(self.oneof_double()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofEnum =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofEnum(self.oneof_enum()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNullValue =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNullValue(self.oneof_null_value()),
      _ => super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn oneof_field_case(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(91);
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::try_from(field_num).unwrap_unchecked()
    }
  }
}

// SAFETY:
// - `TestAllTypesProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for TestAllTypesProto3Mut<'_> {}

// SAFETY:
// - `TestAllTypesProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for TestAllTypesProto3Mut<'_> {}

impl<'msg> ::protobuf::AsView for TestAllTypesProto3Mut<'msg> {
  type Proxied = TestAllTypesProto3;
  fn as_view(&self) -> ::protobuf::View<'_, TestAllTypesProto3> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for TestAllTypesProto3Mut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, TestAllTypesProto3>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for TestAllTypesProto3Mut<'msg> {
  type MutProxied = TestAllTypesProto3;
  fn as_mut(&mut self) -> TestAllTypesProto3Mut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for TestAllTypesProto3Mut<'msg> {
  fn into_mut<'shorter>(self) -> TestAllTypesProto3Mut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl TestAllTypesProto3 {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, TestAllTypesProto3> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> TestAllTypesProto3View<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> TestAllTypesProto3Mut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // optional_int32: optional int32
  pub fn optional_int32(&self) -> i32 {
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
  pub fn set_optional_int32(&mut self, val: i32) {
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

  // optional_int64: optional int64
  pub fn optional_int64(&self) -> i64 {
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
  pub fn set_optional_int64(&mut self, val: i64) {
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

  // optional_uint32: optional uint32
  pub fn optional_uint32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        2, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_uint32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        2, val.into()
      )
    }
  }

  // optional_uint64: optional uint64
  pub fn optional_uint64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        3, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_uint64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        3, val.into()
      )
    }
  }

  // optional_sint32: optional sint32
  pub fn optional_sint32(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        4, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sint32(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        4, val.into()
      )
    }
  }

  // optional_sint64: optional sint64
  pub fn optional_sint64(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        5, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sint64(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        5, val.into()
      )
    }
  }

  // optional_fixed32: optional fixed32
  pub fn optional_fixed32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        6, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_fixed32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        6, val.into()
      )
    }
  }

  // optional_fixed64: optional fixed64
  pub fn optional_fixed64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        7, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_fixed64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        7, val.into()
      )
    }
  }

  // optional_sfixed32: optional sfixed32
  pub fn optional_sfixed32(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        8, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sfixed32(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        8, val.into()
      )
    }
  }

  // optional_sfixed64: optional sfixed64
  pub fn optional_sfixed64(&self) -> i64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i64_at_index(
        9, (0i64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_sfixed64(&mut self, val: i64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i64_at_index(
        9, val.into()
      )
    }
  }

  // optional_float: optional float
  pub fn optional_float(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        10, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_float(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        10, val.into()
      )
    }
  }

  // optional_double: optional double
  pub fn optional_double(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        11, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_double(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        11, val.into()
      )
    }
  }

  // optional_bool: optional bool
  pub fn optional_bool(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        12, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_bool(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        12, val.into()
      )
    }
  }

  // optional_string: optional string
  pub fn optional_string(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        13, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_string(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        13,
        val);
    }
  }

  // optional_bytes: optional bytes
  pub fn optional_bytes(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        14, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_optional_bytes(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        14,
        val);
    }
  }

  // optional_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_optional_nested_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(15)
    }
  }
  pub fn clear_optional_nested_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        15
      );
    }
  }
  pub fn optional_nested_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_>> {
    self.has_optional_nested_message().then(|| self.optional_nested_message())
  }
  pub fn optional_nested_message(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(15)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }
  pub fn optional_nested_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         15, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_nested_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        15,
        val
      );
    }
  }

  // optional_foreign_message: optional message protobuf_test_messages.proto3.ForeignMessage
  pub fn has_optional_foreign_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(16)
    }
  }
  pub fn clear_optional_foreign_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        16
      );
    }
  }
  pub fn optional_foreign_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'_>> {
    self.has_optional_foreign_message().then(|| self.optional_foreign_message())
  }
  pub fn optional_foreign_message(&self) -> super::google_protobuf_test_messages_proto3_proto::ForeignMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(16)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::ForeignMessageView::default())
  }
  pub fn optional_foreign_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::ForeignMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         16, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_foreign_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        16,
        val
      );
    }
  }

  // optional_nested_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn optional_nested_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        17, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_nested_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        17, val.into()
      )
    }
  }

  // optional_foreign_enum: optional enum protobuf_test_messages.proto3.ForeignEnum
  pub fn optional_foreign_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::ForeignEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        18, (super::google_protobuf_test_messages_proto3_proto::ForeignEnum::ForeignFoo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_foreign_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::ForeignEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        18, val.into()
      )
    }
  }

  // optional_aliased_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.AliasedEnum
  pub fn optional_aliased_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        19, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum::AliasFoo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_aliased_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::AliasedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        19, val.into()
      )
    }
  }

  // optional_string_piece: optional string
  pub fn optional_string_piece(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        20, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_string_piece(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        20,
        val);
    }
  }

  // optional_cord: optional string
  pub fn optional_cord(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        21, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_optional_cord(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        21,
        val);
    }
  }

  // recursive_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_recursive_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(22)
    }
  }
  pub fn clear_recursive_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        22
      );
    }
  }
  pub fn recursive_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_>> {
    self.has_recursive_message().then(|| self.recursive_message())
  }
  pub fn recursive_message(&self) -> super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(22)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }
  pub fn recursive_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3Mut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         22, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_recursive_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        22,
        val
      );
    }
  }

  // repeated_int32: repeated int32
  pub fn repeated_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        23
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        23,
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
  pub fn set_repeated_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        23,
        src);
    }
  }

  // repeated_int64: repeated int64
  pub fn repeated_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        24
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        24,
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
  pub fn set_repeated_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        24,
        src);
    }
  }

  // repeated_uint32: repeated uint32
  pub fn repeated_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        25
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        25,
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
  pub fn set_repeated_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        25,
        src);
    }
  }

  // repeated_uint64: repeated uint64
  pub fn repeated_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        26
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        26,
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
  pub fn set_repeated_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        26,
        src);
    }
  }

  // repeated_sint32: repeated sint32
  pub fn repeated_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        27
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        27,
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
  pub fn set_repeated_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        27,
        src);
    }
  }

  // repeated_sint64: repeated sint64
  pub fn repeated_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        28
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        28,
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
  pub fn set_repeated_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        28,
        src);
    }
  }

  // repeated_fixed32: repeated fixed32
  pub fn repeated_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        29
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        29,
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
  pub fn set_repeated_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        29,
        src);
    }
  }

  // repeated_fixed64: repeated fixed64
  pub fn repeated_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        30
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        30,
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
  pub fn set_repeated_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        30,
        src);
    }
  }

  // repeated_sfixed32: repeated sfixed32
  pub fn repeated_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        31
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        31,
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
  pub fn set_repeated_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        31,
        src);
    }
  }

  // repeated_sfixed64: repeated sfixed64
  pub fn repeated_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        32
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        32,
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
  pub fn set_repeated_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        32,
        src);
    }
  }

  // repeated_float: repeated float
  pub fn repeated_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        33
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        33,
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
  pub fn set_repeated_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        33,
        src);
    }
  }

  // repeated_double: repeated double
  pub fn repeated_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        34
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        34,
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
  pub fn set_repeated_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        34,
        src);
    }
  }

  // repeated_bool: repeated bool
  pub fn repeated_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        35
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        35,
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
  pub fn set_repeated_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        35,
        src);
    }
  }

  // repeated_string: repeated string
  pub fn repeated_string(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        36
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        36,
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
  pub fn set_repeated_string(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        36,
        src);
    }
  }

  // repeated_bytes: repeated bytes
  pub fn repeated_bytes(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoBytes> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        37
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoBytes>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bytes_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoBytes> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        37,
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
  pub fn set_repeated_bytes(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoBytes>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        37,
        src);
    }
  }

  // repeated_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn repeated_nested_message(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        38
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_nested_message_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        38,
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
  pub fn set_repeated_nested_message(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        38,
        src);
    }
  }

  // repeated_foreign_message: repeated message protobuf_test_messages.proto3.ForeignMessage
  pub fn repeated_foreign_message(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        39
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_foreign_message_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        39,
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
  pub fn set_repeated_foreign_message(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::ForeignMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        39,
        src);
    }
  }

  // repeated_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn repeated_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        40
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        40,
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
  pub fn set_repeated_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        40,
        src);
    }
  }

  // repeated_foreign_enum: repeated enum protobuf_test_messages.proto3.ForeignEnum
  pub fn repeated_foreign_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        41
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_foreign_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        41,
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
  pub fn set_repeated_foreign_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::ForeignEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        41,
        src);
    }
  }

  // repeated_string_piece: repeated string
  pub fn repeated_string_piece(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        42
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_piece_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        42,
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
  pub fn set_repeated_string_piece(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        42,
        src);
    }
  }

  // repeated_cord: repeated string
  pub fn repeated_cord(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        43
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_cord_mut(&mut self) -> ::protobuf::RepeatedMut<'_, ::protobuf::ProtoString> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        43,
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
  pub fn set_repeated_cord(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        43,
        src);
    }
  }

  // packed_int32: repeated int32
  pub fn packed_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        63
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        63,
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
  pub fn set_packed_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        63,
        src);
    }
  }

  // packed_int64: repeated int64
  pub fn packed_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        64
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        64,
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
  pub fn set_packed_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        64,
        src);
    }
  }

  // packed_uint32: repeated uint32
  pub fn packed_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        65
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        65,
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
  pub fn set_packed_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        65,
        src);
    }
  }

  // packed_uint64: repeated uint64
  pub fn packed_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        66
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        66,
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
  pub fn set_packed_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        66,
        src);
    }
  }

  // packed_sint32: repeated sint32
  pub fn packed_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        67
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        67,
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
  pub fn set_packed_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        67,
        src);
    }
  }

  // packed_sint64: repeated sint64
  pub fn packed_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        68
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        68,
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
  pub fn set_packed_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        68,
        src);
    }
  }

  // packed_fixed32: repeated fixed32
  pub fn packed_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        69
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        69,
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
  pub fn set_packed_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        69,
        src);
    }
  }

  // packed_fixed64: repeated fixed64
  pub fn packed_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        70
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        70,
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
  pub fn set_packed_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        70,
        src);
    }
  }

  // packed_sfixed32: repeated sfixed32
  pub fn packed_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        71
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        71,
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
  pub fn set_packed_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        71,
        src);
    }
  }

  // packed_sfixed64: repeated sfixed64
  pub fn packed_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        72
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        72,
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
  pub fn set_packed_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        72,
        src);
    }
  }

  // packed_float: repeated float
  pub fn packed_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        73
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        73,
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
  pub fn set_packed_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        73,
        src);
    }
  }

  // packed_double: repeated double
  pub fn packed_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        74
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        74,
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
  pub fn set_packed_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        74,
        src);
    }
  }

  // packed_bool: repeated bool
  pub fn packed_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        75
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        75,
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
  pub fn set_packed_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        75,
        src);
    }
  }

  // packed_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn packed_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        76
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn packed_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        76,
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
  pub fn set_packed_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        76,
        src);
    }
  }

  // unpacked_int32: repeated int32
  pub fn unpacked_int32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        77
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_int32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        77,
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
  pub fn set_unpacked_int32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        77,
        src);
    }
  }

  // unpacked_int64: repeated int64
  pub fn unpacked_int64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        78
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_int64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        78,
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
  pub fn set_unpacked_int64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        78,
        src);
    }
  }

  // unpacked_uint32: repeated uint32
  pub fn unpacked_uint32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        79
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_uint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        79,
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
  pub fn set_unpacked_uint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        79,
        src);
    }
  }

  // unpacked_uint64: repeated uint64
  pub fn unpacked_uint64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        80
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_uint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        80,
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
  pub fn set_unpacked_uint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        80,
        src);
    }
  }

  // unpacked_sint32: repeated sint32
  pub fn unpacked_sint32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        81
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sint32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        81,
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
  pub fn set_unpacked_sint32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        81,
        src);
    }
  }

  // unpacked_sint64: repeated sint64
  pub fn unpacked_sint64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        82
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sint64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        82,
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
  pub fn set_unpacked_sint64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        82,
        src);
    }
  }

  // unpacked_fixed32: repeated fixed32
  pub fn unpacked_fixed32(&self) -> ::protobuf::RepeatedView<'_, u32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        83
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_fixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        83,
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
  pub fn set_unpacked_fixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        83,
        src);
    }
  }

  // unpacked_fixed64: repeated fixed64
  pub fn unpacked_fixed64(&self) -> ::protobuf::RepeatedView<'_, u64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        84
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<u64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_fixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, u64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        84,
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
  pub fn set_unpacked_fixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        84,
        src);
    }
  }

  // unpacked_sfixed32: repeated sfixed32
  pub fn unpacked_sfixed32(&self) -> ::protobuf::RepeatedView<'_, i32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        85
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sfixed32_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        85,
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
  pub fn set_unpacked_sfixed32(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        85,
        src);
    }
  }

  // unpacked_sfixed64: repeated sfixed64
  pub fn unpacked_sfixed64(&self) -> ::protobuf::RepeatedView<'_, i64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        86
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<i64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_sfixed64_mut(&mut self) -> ::protobuf::RepeatedMut<'_, i64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        86,
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
  pub fn set_unpacked_sfixed64(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        86,
        src);
    }
  }

  // unpacked_float: repeated float
  pub fn unpacked_float(&self) -> ::protobuf::RepeatedView<'_, f32> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        87
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f32>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_float_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f32> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        87,
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
  pub fn set_unpacked_float(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        87,
        src);
    }
  }

  // unpacked_double: repeated double
  pub fn unpacked_double(&self) -> ::protobuf::RepeatedView<'_, f64> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        88
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<f64>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_double_mut(&mut self) -> ::protobuf::RepeatedMut<'_, f64> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        88,
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
  pub fn set_unpacked_double(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        88,
        src);
    }
  }

  // unpacked_bool: repeated bool
  pub fn unpacked_bool(&self) -> ::protobuf::RepeatedView<'_, bool> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        89
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<bool>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_bool_mut(&mut self) -> ::protobuf::RepeatedMut<'_, bool> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        89,
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
  pub fn set_unpacked_bool(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        89,
        src);
    }
  }

  // unpacked_nested_enum: repeated enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn unpacked_nested_enum(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        90
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn unpacked_nested_enum_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        90,
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
  pub fn set_unpacked_nested_enum(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        90,
        src);
    }
  }

  // map_int32_int32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32Int32Entry
  pub fn map_int32_int32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(44)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_int32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          44, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_int32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        44,
        src);
    }
  }

  // map_int64_int64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt64Int64Entry
  pub fn map_int64_int64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(45)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int64_int64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          45, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int64_int64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        45,
        src);
    }
  }

  // map_uint32_uint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint32Uint32Entry
  pub fn map_uint32_uint32(&self)
    -> ::protobuf::MapView<'_, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(46)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_uint32_uint32_mut(&mut self)
    -> ::protobuf::MapMut<'_, u32, u32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          46, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_uint32_uint32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u32, u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        46,
        src);
    }
  }

  // map_uint64_uint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapUint64Uint64Entry
  pub fn map_uint64_uint64(&self)
    -> ::protobuf::MapView<'_, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(47)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_uint64_uint64_mut(&mut self)
    -> ::protobuf::MapMut<'_, u64, u64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          47, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_uint64_uint64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u64, u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        47,
        src);
    }
  }

  // map_sint32_sint32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint32Sint32Entry
  pub fn map_sint32_sint32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(48)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sint32_sint32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          48, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sint32_sint32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        48,
        src);
    }
  }

  // map_sint64_sint64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSint64Sint64Entry
  pub fn map_sint64_sint64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(49)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sint64_sint64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          49, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sint64_sint64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        49,
        src);
    }
  }

  // map_fixed32_fixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed32Fixed32Entry
  pub fn map_fixed32_fixed32(&self)
    -> ::protobuf::MapView<'_, u32, u32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(50)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u32, u32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_fixed32_fixed32_mut(&mut self)
    -> ::protobuf::MapMut<'_, u32, u32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          50, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_fixed32_fixed32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u32, u32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        50,
        src);
    }
  }

  // map_fixed64_fixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapFixed64Fixed64Entry
  pub fn map_fixed64_fixed64(&self)
    -> ::protobuf::MapView<'_, u64, u64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(51)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<u64, u64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_fixed64_fixed64_mut(&mut self)
    -> ::protobuf::MapMut<'_, u64, u64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          51, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_fixed64_fixed64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<u64, u64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        51,
        src);
    }
  }

  // map_sfixed32_sfixed32: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed32Sfixed32Entry
  pub fn map_sfixed32_sfixed32(&self)
    -> ::protobuf::MapView<'_, i32, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(52)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sfixed32_sfixed32_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          52, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sfixed32_sfixed32(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        52,
        src);
    }
  }

  // map_sfixed64_sfixed64: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapSfixed64Sfixed64Entry
  pub fn map_sfixed64_sfixed64(&self)
    -> ::protobuf::MapView<'_, i64, i64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(53)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i64, i64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_sfixed64_sfixed64_mut(&mut self)
    -> ::protobuf::MapMut<'_, i64, i64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          53, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_sfixed64_sfixed64(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i64, i64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        53,
        src);
    }
  }

  // map_int32_float: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32FloatEntry
  pub fn map_int32_float(&self)
    -> ::protobuf::MapView<'_, i32, f32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(54)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_float_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, f32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          54, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_float(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, f32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        54,
        src);
    }
  }

  // map_int32_double: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapInt32DoubleEntry
  pub fn map_int32_double(&self)
    -> ::protobuf::MapView<'_, i32, f64> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(55)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<i32, f64>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_int32_double_mut(&mut self)
    -> ::protobuf::MapMut<'_, i32, f64> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          55, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_int32_double(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<i32, f64>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        55,
        src);
    }
  }

  // map_bool_bool: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapBoolBoolEntry
  pub fn map_bool_bool(&self)
    -> ::protobuf::MapView<'_, bool, bool> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(56)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<bool, bool>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_bool_bool_mut(&mut self)
    -> ::protobuf::MapMut<'_, bool, bool> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          56, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_bool_bool(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<bool, bool>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        56,
        src);
    }
  }

  // map_string_string: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringStringEntry
  pub fn map_string_string(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(57)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoString>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_string_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoString> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          57, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_string(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoString>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        57,
        src);
    }
  }

  // map_string_bytes: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringBytesEntry
  pub fn map_string_bytes(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, ::protobuf::ProtoBytes> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(58)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, ::protobuf::ProtoBytes>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_bytes_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, ::protobuf::ProtoBytes> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          58, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_bytes(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, ::protobuf::ProtoBytes>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        58,
        src);
    }
  }

  // map_string_nested_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedMessageEntry
  pub fn map_string_nested_message(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(59)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_nested_message_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          59, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_nested_message(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        59,
        src);
    }
  }

  // map_string_foreign_message: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignMessageEntry
  pub fn map_string_foreign_message(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(60)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_foreign_message_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          60, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_foreign_message(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignMessage>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        60,
        src);
    }
  }

  // map_string_nested_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringNestedEnumEntry
  pub fn map_string_nested_enum(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(61)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_nested_enum_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          61, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_nested_enum(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        61,
        src);
    }
  }

  // map_string_foreign_enum: repeated message protobuf_test_messages.proto3.TestAllTypesProto3.MapStringForeignEnumEntry
  pub fn map_string_foreign_enum(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(62)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn map_string_foreign_enum_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          62, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_map_string_foreign_enum(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, super::google_protobuf_test_messages_proto3_proto::ForeignEnum>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        62,
        src);
    }
  }

  // oneof_uint32: optional uint32
  pub fn has_oneof_uint32(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(91)
    }
  }
  pub fn clear_oneof_uint32(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        91
      );
    }
  }
  pub fn oneof_uint32_opt(&self) -> ::std::option::Option<u32> {
    self.has_oneof_uint32().then(|| self.oneof_uint32())
  }
  pub fn oneof_uint32(&self) -> u32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u32_at_index(
        91, (0u32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_uint32(&mut self, val: u32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u32_at_index(
        91, val.into()
      )
    }
  }

  // oneof_nested_message: optional message protobuf_test_messages.proto3.TestAllTypesProto3.NestedMessage
  pub fn has_oneof_nested_message(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(92)
    }
  }
  pub fn clear_oneof_nested_message(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        92
      );
    }
  }
  pub fn oneof_nested_message_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_>> {
    self.has_oneof_nested_message().then(|| self.oneof_nested_message())
  }
  pub fn oneof_nested_message(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(92)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageView::default())
  }
  pub fn oneof_nested_message_mut(&mut self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessageMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         92, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_oneof_nested_message(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        92,
        val
      );
    }
  }

  // oneof_string: optional string
  pub fn has_oneof_string(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(93)
    }
  }
  pub fn clear_oneof_string(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        93
      );
    }
  }
  pub fn oneof_string_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_oneof_string().then(|| self.oneof_string())
  }
  pub fn oneof_string(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        93, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_oneof_string(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        93,
        val);
    }
  }

  // oneof_bytes: optional bytes
  pub fn has_oneof_bytes(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(94)
    }
  }
  pub fn clear_oneof_bytes(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        94
      );
    }
  }
  pub fn oneof_bytes_opt(&self) -> ::std::option::Option<&'_ [u8]> {
    self.has_oneof_bytes().then(|| self.oneof_bytes())
  }
  pub fn oneof_bytes(&self) -> ::protobuf::View<'_, ::protobuf::ProtoBytes> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        94, (b"").into()
      )
    };
    unsafe { str_view.as_ref() }
  }
  pub fn set_oneof_bytes(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoBytes>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_bytes_field(
        ::protobuf::AsMut::as_mut(self).inner,
        94,
        val);
    }
  }

  // oneof_bool: optional bool
  pub fn has_oneof_bool(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(95)
    }
  }
  pub fn clear_oneof_bool(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        95
      );
    }
  }
  pub fn oneof_bool_opt(&self) -> ::std::option::Option<bool> {
    self.has_oneof_bool().then(|| self.oneof_bool())
  }
  pub fn oneof_bool(&self) -> bool {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_bool_at_index(
        95, (false).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_bool(&mut self, val: bool) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_bool_at_index(
        95, val.into()
      )
    }
  }

  // oneof_uint64: optional uint64
  pub fn has_oneof_uint64(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(96)
    }
  }
  pub fn clear_oneof_uint64(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        96
      );
    }
  }
  pub fn oneof_uint64_opt(&self) -> ::std::option::Option<u64> {
    self.has_oneof_uint64().then(|| self.oneof_uint64())
  }
  pub fn oneof_uint64(&self) -> u64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_u64_at_index(
        96, (0u64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_uint64(&mut self, val: u64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_u64_at_index(
        96, val.into()
      )
    }
  }

  // oneof_float: optional float
  pub fn has_oneof_float(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(97)
    }
  }
  pub fn clear_oneof_float(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        97
      );
    }
  }
  pub fn oneof_float_opt(&self) -> ::std::option::Option<f32> {
    self.has_oneof_float().then(|| self.oneof_float())
  }
  pub fn oneof_float(&self) -> f32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f32_at_index(
        97, (0f32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_float(&mut self, val: f32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f32_at_index(
        97, val.into()
      )
    }
  }

  // oneof_double: optional double
  pub fn has_oneof_double(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(98)
    }
  }
  pub fn clear_oneof_double(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        98
      );
    }
  }
  pub fn oneof_double_opt(&self) -> ::std::option::Option<f64> {
    self.has_oneof_double().then(|| self.oneof_double())
  }
  pub fn oneof_double(&self) -> f64 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_f64_at_index(
        98, (0f64).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_double(&mut self, val: f64) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_f64_at_index(
        98, val.into()
      )
    }
  }

  // oneof_enum: optional enum protobuf_test_messages.proto3.TestAllTypesProto3.NestedEnum
  pub fn has_oneof_enum(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(99)
    }
  }
  pub fn clear_oneof_enum(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        99
      );
    }
  }
  pub fn oneof_enum_opt(&self) -> ::std::option::Option<super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum> {
    self.has_oneof_enum().then(|| self.oneof_enum())
  }
  pub fn oneof_enum(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        99, (super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum::Foo).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_enum(&mut self, val: super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        99, val.into()
      )
    }
  }

  // oneof_null_value: optional enum google.protobuf.NullValue
  pub fn has_oneof_null_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(100)
    }
  }
  pub fn clear_oneof_null_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        100
      );
    }
  }
  pub fn oneof_null_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::NullValue> {
    self.has_oneof_null_value().then(|| self.oneof_null_value())
  }
  pub fn oneof_null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        100, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_oneof_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        100, val.into()
      )
    }
  }

  // optional_bool_wrapper: optional message google.protobuf.BoolValue
  pub fn has_optional_bool_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(101)
    }
  }
  pub fn clear_optional_bool_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        101
      );
    }
  }
  pub fn optional_bool_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BoolValueView<'_>> {
    self.has_optional_bool_wrapper().then(|| self.optional_bool_wrapper())
  }
  pub fn optional_bool_wrapper(&self) -> super::google_protobuf_wrappers_proto::BoolValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(101)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BoolValueView::default())
  }
  pub fn optional_bool_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::BoolValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         101, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_bool_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::BoolValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        101,
        val
      );
    }
  }

  // optional_int32_wrapper: optional message google.protobuf.Int32Value
  pub fn has_optional_int32_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(102)
    }
  }
  pub fn clear_optional_int32_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        102
      );
    }
  }
  pub fn optional_int32_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int32ValueView<'_>> {
    self.has_optional_int32_wrapper().then(|| self.optional_int32_wrapper())
  }
  pub fn optional_int32_wrapper(&self) -> super::google_protobuf_wrappers_proto::Int32ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(102)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int32ValueView::default())
  }
  pub fn optional_int32_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::Int32ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         102, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_int32_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::Int32Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        102,
        val
      );
    }
  }

  // optional_int64_wrapper: optional message google.protobuf.Int64Value
  pub fn has_optional_int64_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(103)
    }
  }
  pub fn clear_optional_int64_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        103
      );
    }
  }
  pub fn optional_int64_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::Int64ValueView<'_>> {
    self.has_optional_int64_wrapper().then(|| self.optional_int64_wrapper())
  }
  pub fn optional_int64_wrapper(&self) -> super::google_protobuf_wrappers_proto::Int64ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(103)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::Int64ValueView::default())
  }
  pub fn optional_int64_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::Int64ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         103, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_int64_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::Int64Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        103,
        val
      );
    }
  }

  // optional_uint32_wrapper: optional message google.protobuf.UInt32Value
  pub fn has_optional_uint32_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(104)
    }
  }
  pub fn clear_optional_uint32_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        104
      );
    }
  }
  pub fn optional_uint32_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt32ValueView<'_>> {
    self.has_optional_uint32_wrapper().then(|| self.optional_uint32_wrapper())
  }
  pub fn optional_uint32_wrapper(&self) -> super::google_protobuf_wrappers_proto::UInt32ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(104)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt32ValueView::default())
  }
  pub fn optional_uint32_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::UInt32ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         104, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_uint32_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::UInt32Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        104,
        val
      );
    }
  }

  // optional_uint64_wrapper: optional message google.protobuf.UInt64Value
  pub fn has_optional_uint64_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(105)
    }
  }
  pub fn clear_optional_uint64_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        105
      );
    }
  }
  pub fn optional_uint64_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::UInt64ValueView<'_>> {
    self.has_optional_uint64_wrapper().then(|| self.optional_uint64_wrapper())
  }
  pub fn optional_uint64_wrapper(&self) -> super::google_protobuf_wrappers_proto::UInt64ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(105)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::UInt64ValueView::default())
  }
  pub fn optional_uint64_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::UInt64ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         105, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_uint64_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::UInt64Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        105,
        val
      );
    }
  }

  // optional_float_wrapper: optional message google.protobuf.FloatValue
  pub fn has_optional_float_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(106)
    }
  }
  pub fn clear_optional_float_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        106
      );
    }
  }
  pub fn optional_float_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::FloatValueView<'_>> {
    self.has_optional_float_wrapper().then(|| self.optional_float_wrapper())
  }
  pub fn optional_float_wrapper(&self) -> super::google_protobuf_wrappers_proto::FloatValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(106)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::FloatValueView::default())
  }
  pub fn optional_float_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::FloatValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         106, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_float_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::FloatValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        106,
        val
      );
    }
  }

  // optional_double_wrapper: optional message google.protobuf.DoubleValue
  pub fn has_optional_double_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(107)
    }
  }
  pub fn clear_optional_double_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        107
      );
    }
  }
  pub fn optional_double_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::DoubleValueView<'_>> {
    self.has_optional_double_wrapper().then(|| self.optional_double_wrapper())
  }
  pub fn optional_double_wrapper(&self) -> super::google_protobuf_wrappers_proto::DoubleValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(107)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::DoubleValueView::default())
  }
  pub fn optional_double_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::DoubleValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         107, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_double_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::DoubleValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        107,
        val
      );
    }
  }

  // optional_string_wrapper: optional message google.protobuf.StringValue
  pub fn has_optional_string_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(108)
    }
  }
  pub fn clear_optional_string_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        108
      );
    }
  }
  pub fn optional_string_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::StringValueView<'_>> {
    self.has_optional_string_wrapper().then(|| self.optional_string_wrapper())
  }
  pub fn optional_string_wrapper(&self) -> super::google_protobuf_wrappers_proto::StringValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(108)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::StringValueView::default())
  }
  pub fn optional_string_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::StringValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         108, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_string_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::StringValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        108,
        val
      );
    }
  }

  // optional_bytes_wrapper: optional message google.protobuf.BytesValue
  pub fn has_optional_bytes_wrapper(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(109)
    }
  }
  pub fn clear_optional_bytes_wrapper(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        109
      );
    }
  }
  pub fn optional_bytes_wrapper_opt(&self) -> ::std::option::Option<super::google_protobuf_wrappers_proto::BytesValueView<'_>> {
    self.has_optional_bytes_wrapper().then(|| self.optional_bytes_wrapper())
  }
  pub fn optional_bytes_wrapper(&self) -> super::google_protobuf_wrappers_proto::BytesValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(109)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_wrappers_proto::BytesValueView::default())
  }
  pub fn optional_bytes_wrapper_mut(&mut self) -> super::google_protobuf_wrappers_proto::BytesValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         109, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_bytes_wrapper(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_wrappers_proto::BytesValue>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        109,
        val
      );
    }
  }

  // repeated_bool_wrapper: repeated message google.protobuf.BoolValue
  pub fn repeated_bool_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::BoolValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        110
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BoolValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bool_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::BoolValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        110,
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
  pub fn set_repeated_bool_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::BoolValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        110,
        src);
    }
  }

  // repeated_int32_wrapper: repeated message google.protobuf.Int32Value
  pub fn repeated_int32_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::Int32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        111
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int32_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::Int32Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        111,
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
  pub fn set_repeated_int32_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::Int32Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        111,
        src);
    }
  }

  // repeated_int64_wrapper: repeated message google.protobuf.Int64Value
  pub fn repeated_int64_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::Int64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        112
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::Int64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_int64_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::Int64Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        112,
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
  pub fn set_repeated_int64_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::Int64Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        112,
        src);
    }
  }

  // repeated_uint32_wrapper: repeated message google.protobuf.UInt32Value
  pub fn repeated_uint32_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::UInt32Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        113
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt32Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint32_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::UInt32Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        113,
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
  pub fn set_repeated_uint32_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::UInt32Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        113,
        src);
    }
  }

  // repeated_uint64_wrapper: repeated message google.protobuf.UInt64Value
  pub fn repeated_uint64_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::UInt64Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        114
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::UInt64Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_uint64_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::UInt64Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        114,
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
  pub fn set_repeated_uint64_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::UInt64Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        114,
        src);
    }
  }

  // repeated_float_wrapper: repeated message google.protobuf.FloatValue
  pub fn repeated_float_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::FloatValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        115
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::FloatValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_float_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::FloatValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        115,
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
  pub fn set_repeated_float_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::FloatValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        115,
        src);
    }
  }

  // repeated_double_wrapper: repeated message google.protobuf.DoubleValue
  pub fn repeated_double_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::DoubleValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        116
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::DoubleValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_double_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::DoubleValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        116,
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
  pub fn set_repeated_double_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::DoubleValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        116,
        src);
    }
  }

  // repeated_string_wrapper: repeated message google.protobuf.StringValue
  pub fn repeated_string_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::StringValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        117
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::StringValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_string_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::StringValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        117,
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
  pub fn set_repeated_string_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::StringValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        117,
        src);
    }
  }

  // repeated_bytes_wrapper: repeated message google.protobuf.BytesValue
  pub fn repeated_bytes_wrapper(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_wrappers_proto::BytesValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        118
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_wrappers_proto::BytesValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_bytes_wrapper_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_wrappers_proto::BytesValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        118,
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
  pub fn set_repeated_bytes_wrapper(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_wrappers_proto::BytesValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        118,
        src);
    }
  }

  // optional_duration: optional message google.protobuf.Duration
  pub fn has_optional_duration(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(119)
    }
  }
  pub fn clear_optional_duration(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        119
      );
    }
  }
  pub fn optional_duration_opt(&self) -> ::std::option::Option<super::google_protobuf_duration_proto::DurationView<'_>> {
    self.has_optional_duration().then(|| self.optional_duration())
  }
  pub fn optional_duration(&self) -> super::google_protobuf_duration_proto::DurationView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(119)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_duration_proto::DurationView::default())
  }
  pub fn optional_duration_mut(&mut self) -> super::google_protobuf_duration_proto::DurationMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         119, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_duration(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_duration_proto::Duration>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        119,
        val
      );
    }
  }

  // optional_timestamp: optional message google.protobuf.Timestamp
  pub fn has_optional_timestamp(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(120)
    }
  }
  pub fn clear_optional_timestamp(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        120
      );
    }
  }
  pub fn optional_timestamp_opt(&self) -> ::std::option::Option<super::google_protobuf_timestamp_proto::TimestampView<'_>> {
    self.has_optional_timestamp().then(|| self.optional_timestamp())
  }
  pub fn optional_timestamp(&self) -> super::google_protobuf_timestamp_proto::TimestampView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(120)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_timestamp_proto::TimestampView::default())
  }
  pub fn optional_timestamp_mut(&mut self) -> super::google_protobuf_timestamp_proto::TimestampMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         120, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_timestamp(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_timestamp_proto::Timestamp>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        120,
        val
      );
    }
  }

  // optional_field_mask: optional message google.protobuf.FieldMask
  pub fn has_optional_field_mask(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(121)
    }
  }
  pub fn clear_optional_field_mask(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        121
      );
    }
  }
  pub fn optional_field_mask_opt(&self) -> ::std::option::Option<super::google_protobuf_field_mask_proto::FieldMaskView<'_>> {
    self.has_optional_field_mask().then(|| self.optional_field_mask())
  }
  pub fn optional_field_mask(&self) -> super::google_protobuf_field_mask_proto::FieldMaskView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(121)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_field_mask_proto::FieldMaskView::default())
  }
  pub fn optional_field_mask_mut(&mut self) -> super::google_protobuf_field_mask_proto::FieldMaskMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         121, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_field_mask(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_field_mask_proto::FieldMask>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        121,
        val
      );
    }
  }

  // optional_struct: optional message google.protobuf.Struct
  pub fn has_optional_struct(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(122)
    }
  }
  pub fn clear_optional_struct(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        122
      );
    }
  }
  pub fn optional_struct_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::StructView<'_>> {
    self.has_optional_struct().then(|| self.optional_struct())
  }
  pub fn optional_struct(&self) -> super::google_protobuf_struct_proto::StructView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(122)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::StructView::default())
  }
  pub fn optional_struct_mut(&mut self) -> super::google_protobuf_struct_proto::StructMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         122, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_struct(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Struct>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        122,
        val
      );
    }
  }

  // optional_any: optional message google.protobuf.Any
  pub fn has_optional_any(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(123)
    }
  }
  pub fn clear_optional_any(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        123
      );
    }
  }
  pub fn optional_any_opt(&self) -> ::std::option::Option<super::google_protobuf_any_proto::AnyView<'_>> {
    self.has_optional_any().then(|| self.optional_any())
  }
  pub fn optional_any(&self) -> super::google_protobuf_any_proto::AnyView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(123)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_any_proto::AnyView::default())
  }
  pub fn optional_any_mut(&mut self) -> super::google_protobuf_any_proto::AnyMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         123, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_any(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_any_proto::Any>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        123,
        val
      );
    }
  }

  // optional_value: optional message google.protobuf.Value
  pub fn has_optional_value(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(124)
    }
  }
  pub fn clear_optional_value(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        124
      );
    }
  }
  pub fn optional_value_opt(&self) -> ::std::option::Option<super::google_protobuf_struct_proto::ValueView<'_>> {
    self.has_optional_value().then(|| self.optional_value())
  }
  pub fn optional_value(&self) -> super::google_protobuf_struct_proto::ValueView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(124)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_struct_proto::ValueView::default())
  }
  pub fn optional_value_mut(&mut self) -> super::google_protobuf_struct_proto::ValueMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         124, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_value(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_struct_proto::Value>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        124,
        val
      );
    }
  }

  // optional_null_value: optional enum google.protobuf.NullValue
  pub fn optional_null_value(&self) -> super::google_protobuf_struct_proto::NullValue {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        125, (super::google_protobuf_struct_proto::NullValue::NullValue).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_optional_null_value(&mut self, val: super::google_protobuf_struct_proto::NullValue) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        125, val.into()
      )
    }
  }

  // optional_empty: optional message google.protobuf.Empty
  pub fn has_optional_empty(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(126)
    }
  }
  pub fn clear_optional_empty(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        126
      );
    }
  }
  pub fn optional_empty_opt(&self) -> ::std::option::Option<super::google_protobuf_empty_proto::EmptyView<'_>> {
    self.has_optional_empty().then(|| self.optional_empty())
  }
  pub fn optional_empty(&self) -> super::google_protobuf_empty_proto::EmptyView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(126)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::google_protobuf_empty_proto::EmptyView::default())
  }
  pub fn optional_empty_mut(&mut self) -> super::google_protobuf_empty_proto::EmptyMut<'_> {
     let ptr = unsafe {
       self.inner.ptr_mut().get_or_create_mutable_message_at_index(
         126, self.inner.arena()
       ).unwrap()
     };
     ::protobuf::__internal::runtime::MessageMutInner::from_parent(
         self.as_message_mut_inner(::protobuf::__internal::Private),
         ptr
     ).into()
  }
  pub fn set_optional_empty(&mut self,
    val: impl ::protobuf::IntoProxied<super::google_protobuf_empty_proto::Empty>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        126,
        val
      );
    }
  }

  // repeated_duration: repeated message google.protobuf.Duration
  pub fn repeated_duration(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_duration_proto::Duration> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        127
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_duration_proto::Duration>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_duration_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_duration_proto::Duration> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        127,
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
  pub fn set_repeated_duration(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_duration_proto::Duration>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        127,
        src);
    }
  }

  // repeated_timestamp: repeated message google.protobuf.Timestamp
  pub fn repeated_timestamp(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_timestamp_proto::Timestamp> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        128
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_timestamp_proto::Timestamp>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_timestamp_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_timestamp_proto::Timestamp> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        128,
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
  pub fn set_repeated_timestamp(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_timestamp_proto::Timestamp>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        128,
        src);
    }
  }

  // repeated_fieldmask: repeated message google.protobuf.FieldMask
  pub fn repeated_fieldmask(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_field_mask_proto::FieldMask> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        129
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_field_mask_proto::FieldMask>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_fieldmask_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_field_mask_proto::FieldMask> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        129,
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
  pub fn set_repeated_fieldmask(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_field_mask_proto::FieldMask>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        129,
        src);
    }
  }

  // repeated_struct: repeated message google.protobuf.Struct
  pub fn repeated_struct(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Struct> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        134
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Struct>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_struct_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Struct> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        134,
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
  pub fn set_repeated_struct(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Struct>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        134,
        src);
    }
  }

  // repeated_any: repeated message google.protobuf.Any
  pub fn repeated_any(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_any_proto::Any> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        130
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_any_proto::Any>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_any_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_any_proto::Any> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        130,
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
  pub fn set_repeated_any(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_any_proto::Any>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        130,
        src);
    }
  }

  // repeated_value: repeated message google.protobuf.Value
  pub fn repeated_value(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        131
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::Value>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_value_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::Value> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        131,
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
  pub fn set_repeated_value(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::Value>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        131,
        src);
    }
  }

  // repeated_list_value: repeated message google.protobuf.ListValue
  pub fn repeated_list_value(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_struct_proto::ListValue> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        132
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_struct_proto::ListValue>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_list_value_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_struct_proto::ListValue> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        132,
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
  pub fn set_repeated_list_value(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_struct_proto::ListValue>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        132,
        src);
    }
  }

  // repeated_empty: repeated message google.protobuf.Empty
  pub fn repeated_empty(&self) -> ::protobuf::RepeatedView<'_, super::google_protobuf_empty_proto::Empty> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        133
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<super::google_protobuf_empty_proto::Empty>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }
  pub fn repeated_empty_mut(&mut self) -> ::protobuf::RepeatedMut<'_, super::google_protobuf_empty_proto::Empty> {
    unsafe {
      let raw_array = self.inner.ptr_mut().get_or_create_mutable_array_at_index(
        133,
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
  pub fn set_repeated_empty(&mut self, src: impl ::protobuf::IntoProxied<::protobuf::Repeated<super::google_protobuf_empty_proto::Empty>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_repeated_field(
        ::protobuf::AsMut::as_mut(self).inner,
        133,
        src);
    }
  }

  // fieldname1: optional int32
  pub fn fieldname1(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        135, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_fieldname1(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        135, val.into()
      )
    }
  }

  // field_name2: optional int32
  pub fn field_name2(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        136, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_name2(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        136, val.into()
      )
    }
  }

  // _field_name3: optional int32
  pub fn _field_name3(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        137, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set__field_name3(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        137, val.into()
      )
    }
  }

  // field__name4_: optional int32
  pub fn field__name4_(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        138, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__name4_(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        138, val.into()
      )
    }
  }

  // field0name5: optional int32
  pub fn field0name5(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        139, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field0name5(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        139, val.into()
      )
    }
  }

  // field_0_name6: optional int32
  pub fn field_0_name6(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        140, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_0_name6(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        140, val.into()
      )
    }
  }

  // fieldName7: optional int32
  pub fn fieldName7(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        141, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_fieldName7(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        141, val.into()
      )
    }
  }

  // FieldName8: optional int32
  pub fn FieldName8(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        142, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FieldName8(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        142, val.into()
      )
    }
  }

  // field_Name9: optional int32
  pub fn field_Name9(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        143, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_Name9(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        143, val.into()
      )
    }
  }

  // Field_Name10: optional int32
  pub fn Field_Name10(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        144, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_Field_Name10(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        144, val.into()
      )
    }
  }

  // FIELD_NAME11: optional int32
  pub fn FIELD_NAME11(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        145, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FIELD_NAME11(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        145, val.into()
      )
    }
  }

  // FIELD_name12: optional int32
  pub fn FIELD_name12(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        146, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_FIELD_name12(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        146, val.into()
      )
    }
  }

  // __field_name13: optional int32
  pub fn __field_name13(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        147, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set___field_name13(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        147, val.into()
      )
    }
  }

  // __Field_name14: optional int32
  pub fn __Field_name14(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        148, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set___Field_name14(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        148, val.into()
      )
    }
  }

  // field__name15: optional int32
  pub fn field__name15(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        149, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__name15(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        149, val.into()
      )
    }
  }

  // field__Name16: optional int32
  pub fn field__Name16(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        150, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field__Name16(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        150, val.into()
      )
    }
  }

  // field_name17__: optional int32
  pub fn field_name17__(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        151, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_field_name17__(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        151, val.into()
      )
    }
  }

  // Field_name18__: optional int32
  pub fn Field_name18__(&self) -> i32 {
    unsafe {
      // TODO: b/361751487: This .into() and .try_into() is only
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      // perfectly (and do an unchecked conversion for
      // i32->enum types, since even for closed enums we trust
      // upb to only return one of the named values).
      self.inner.ptr().get_i32_at_index(
        152, (0i32).into()
      ).try_into().unwrap()
    }
  }
  pub fn set_Field_name18__(&mut self, val: i32) {
    unsafe {
      // TODO: b/361751487: This .into() is only here
      // here for the enum<->i32 case, we should avoid it for
      // other primitives where the types naturally match
      //perfectly.
      self.inner.ptr_mut().set_base_field_i32_at_index(
        152, val.into()
      )
    }
  }

  pub fn oneof_field(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof<'_> {
    match &self.oneof_field_case() {
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint32 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint32(self.oneof_uint32()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNestedMessage =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNestedMessage(self.oneof_nested_message()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofString =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofString(self.oneof_string()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBytes =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBytes(self.oneof_bytes()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofBool =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofBool(self.oneof_bool()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofUint64 =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofUint64(self.oneof_uint64()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofFloat =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofFloat(self.oneof_float()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofDouble =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofDouble(self.oneof_double()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofEnum =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofEnum(self.oneof_enum()),
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::OneofNullValue =>
          super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::OneofNullValue(self.oneof_null_value()),
      _ => super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldOneof::not_set(std::marker::PhantomData)
    }
  }

  pub fn oneof_field_case(&self) -> super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase {
    unsafe {
      let field_num = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(
          &self, ::protobuf::__internal::Private)
          .which_oneof_field_number_by_index(91);
      super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::OneofFieldCase::try_from(field_num).unwrap_unchecked()
    }
  }
}  // impl TestAllTypesProto3

impl ::std::ops::Drop for TestAllTypesProto3 {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for TestAllTypesProto3 {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for TestAllTypesProto3 {
  type Proxied = Self;
  fn as_view(&self) -> TestAllTypesProto3View<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for TestAllTypesProto3 {
  type MutProxied = Self;
  fn as_mut(&mut self) -> TestAllTypesProto3Mut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for TestAllTypesProto3 {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$N(P+P)P,P*P-P#P$P%P&P!P P/P1X0Pb33a.P.P.P1X1Xa3c<?=@>A89:;76CETDbGGaBBaETETGGGGGGGGGGGGGGGGGGG<?=@>A89:;76CB<M?M=M@M>MAM8M9M:M;M7M6MCMBMh)31T0/,! ..pa333333333aGGGGGGGGGqa333333.P3bGGGaGGGGeGla(P(P(P(P(P(P(P(P(P(P(P(P(P(P(P(P(P(P^Q!|R!|S!|T!|U!|V!|W!|X!|Y!|Z!");
        super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedMessageEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X3");
        super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$(P3");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init.0, &[super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0,
            <super::google_protobuf_test_messages_proto3_proto::ForeignMessage as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init.0,
            super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0,
            <super::google_protobuf_test_messages_proto3_proto::ForeignMessage as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapInt32Int32Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapInt64Int64Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapUint32Uint32Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapUint64Uint64Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapSint32Sint32Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapSint64Sint64Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapFixed32Fixed32Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapFixed64Fixed64Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapSfixed32Sfixed32Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapSfixed64Sfixed64Entry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapInt32FloatEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapInt32DoubleEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapBoolBoolEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapStringStringEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapStringBytesEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedMessageEntry_msg_init.0,
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapStringForeignMessageEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapStringNestedEnumEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::MapStringForeignEnumEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0,
            <super::google_protobuf_wrappers_proto::BoolValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::Int32Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::Int64Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::UInt32Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::UInt64Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::FloatValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::DoubleValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::StringValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::BytesValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::BoolValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::Int32Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::Int64Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::UInt32Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::UInt64Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::FloatValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::DoubleValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::StringValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_wrappers_proto::BytesValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_duration_proto::Duration as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_timestamp_proto::Timestamp as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_field_mask_proto::FieldMask as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_struct_proto::Struct as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_any_proto::Any as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_struct_proto::Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_empty_proto::Empty as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_duration_proto::Duration as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_timestamp_proto::Timestamp as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_field_mask_proto::FieldMask as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_any_proto::Any as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_struct_proto::Value as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_struct_proto::ListValue as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_empty_proto::Empty as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::google_protobuf_struct_proto::Struct as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedMessageEntry_msg_init.0, &[super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0, &[super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init.0,
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__TestAllTypesProto3_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for TestAllTypesProto3 {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for TestAllTypesProto3 {
  type Msg = TestAllTypesProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<TestAllTypesProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TestAllTypesProto3 {
  type Msg = TestAllTypesProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<TestAllTypesProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for TestAllTypesProto3Mut<'_> {
  type Msg = TestAllTypesProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<TestAllTypesProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TestAllTypesProto3Mut<'_> {
  type Msg = TestAllTypesProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<TestAllTypesProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for TestAllTypesProto3View<'_> {
  type Msg = TestAllTypesProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<TestAllTypesProto3> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for TestAllTypesProto3Mut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod test_all_types_proto3 {// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct NestedMessage {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<NestedMessage>
}

impl ::protobuf::Message for NestedMessage {
  type MessageView<'msg> = NestedMessageView<'msg>;
  type MessageMut<'msg> = NestedMessageMut<'msg>;
}

impl ::std::default::Default for NestedMessage {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for NestedMessage {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `NestedMessage` is `Sync` because it does not implement interior mutability.
//    Neither does `NestedMessageMut`.
unsafe impl ::std::marker::Sync for NestedMessage {}

// SAFETY:
// - `NestedMessage` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for NestedMessage {}

impl ::protobuf::Proxied for NestedMessage {
  type View<'msg> = NestedMessageView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for NestedMessage {}

impl ::protobuf::MutProxied for NestedMessage {
  type Mut<'msg> = NestedMessageMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct NestedMessageView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, NestedMessage>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NestedMessageView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for NestedMessageView<'msg> {
  type Message = NestedMessage;
}

impl ::std::fmt::Debug for NestedMessageView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for NestedMessageView<'_> {
  fn default() -> NestedMessageView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, NestedMessage>> for NestedMessageView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, NestedMessage>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NestedMessageView<'msg> {

  pub fn to_owned(&self) -> NestedMessage {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // a: optional int32
  pub fn a(self) -> i32 {
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

  // corecursive: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_corecursive(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn corecursive_opt(self) -> ::std::option::Option<super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'msg>> {
    self.has_corecursive().then(|| self.corecursive())
  }
  pub fn corecursive(self) -> super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }

}

// SAFETY:
// - `NestedMessageView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for NestedMessageView<'_> {}

// SAFETY:
// - `NestedMessageView` is `Send` because while its alive a `NestedMessageMut` cannot.
// - `NestedMessageView` does not use thread-local data.
unsafe impl ::std::marker::Send for NestedMessageView<'_> {}

impl<'msg> ::protobuf::AsView for NestedMessageView<'msg> {
  type Proxied = NestedMessage;
  fn as_view(&self) -> ::protobuf::View<'msg, NestedMessage> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NestedMessageView<'msg> {
  fn into_view<'shorter>(self) -> NestedMessageView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<NestedMessage> for NestedMessageView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> NestedMessage {
    let mut dst = NestedMessage::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<NestedMessage> for NestedMessageMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> NestedMessage {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for NestedMessage {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NestedMessageView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NestedMessageMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct NestedMessageMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, NestedMessage>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NestedMessageMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for NestedMessageMut<'msg> {
  type Message = NestedMessage;
}

impl ::std::fmt::Debug for NestedMessageMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, NestedMessage>> for NestedMessageMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, NestedMessage>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NestedMessageMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, NestedMessage> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> NestedMessage {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // a: optional int32
  pub fn a(&self) -> i32 {
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
  pub fn set_a(&mut self, val: i32) {
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

  // corecursive: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_corecursive(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_corecursive(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn corecursive_opt(&self) -> ::std::option::Option<super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_>> {
    self.has_corecursive().then(|| self.corecursive())
  }
  pub fn corecursive(&self) -> super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }
  pub fn corecursive_mut(&mut self) -> super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3Mut<'_> {
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
  pub fn set_corecursive(&mut self,
    val: impl ::protobuf::IntoProxied<super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3>) {

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
// - `NestedMessageMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for NestedMessageMut<'_> {}

// SAFETY:
// - `NestedMessageMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for NestedMessageMut<'_> {}

impl<'msg> ::protobuf::AsView for NestedMessageMut<'msg> {
  type Proxied = NestedMessage;
  fn as_view(&self) -> ::protobuf::View<'_, NestedMessage> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NestedMessageMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, NestedMessage>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for NestedMessageMut<'msg> {
  type MutProxied = NestedMessage;
  fn as_mut(&mut self) -> NestedMessageMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for NestedMessageMut<'msg> {
  fn into_mut<'shorter>(self) -> NestedMessageMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl NestedMessage {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, NestedMessage> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> NestedMessageView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> NestedMessageMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // a: optional int32
  pub fn a(&self) -> i32 {
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
  pub fn set_a(&mut self, val: i32) {
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

  // corecursive: optional message protobuf_test_messages.proto3.TestAllTypesProto3
  pub fn has_corecursive(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(1)
    }
  }
  pub fn clear_corecursive(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        1
      );
    }
  }
  pub fn corecursive_opt(&self) -> ::std::option::Option<super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_>> {
    self.has_corecursive().then(|| self.corecursive())
  }
  pub fn corecursive(&self) -> super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(1)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3View::default())
  }
  pub fn corecursive_mut(&mut self) -> super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3Mut<'_> {
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
  pub fn set_corecursive(&mut self,
    val: impl ::protobuf::IntoProxied<super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val
      );
    }
  }

}  // impl NestedMessage

impl ::std::ops::Drop for NestedMessage {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for NestedMessage {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for NestedMessage {
  type Proxied = Self;
  fn as_view(&self) -> NestedMessageView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for NestedMessage {
  type MutProxied = Self;
  fn as_mut(&mut self) -> NestedMessageMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for NestedMessage {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        <super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3 as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table();
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__NestedMessage_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NestedMessage {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NestedMessage {
  type Msg = NestedMessage;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NestedMessage> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NestedMessage {
  type Msg = NestedMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NestedMessage> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NestedMessageMut<'_> {
  type Msg = NestedMessage;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NestedMessage> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NestedMessageMut<'_> {
  type Msg = NestedMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NestedMessage> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NestedMessageView<'_> {
  type Msg = NestedMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NestedMessage> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NestedMessageMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32Int32Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapInt32Int32Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapInt32Int32Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32Int32Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%(P(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32Int32Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32Int32Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt64Int64Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapInt64Int64Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapInt64Int64Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt64Int64Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%+P+P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt64Int64Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt64Int64Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint32Uint32Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapUint32Uint32Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapUint32Uint32Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint32Uint32Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%)P)P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint32Uint32Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint32Uint32Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint64Uint64Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapUint64Uint64Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapUint64Uint64Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint64Uint64Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%,P,P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint64Uint64Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapUint64Uint64Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint32Sint32Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapSint32Sint32Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapSint32Sint32Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint32Sint32Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%*P*P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint32Sint32Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint32Sint32Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint64Sint64Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapSint64Sint64Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapSint64Sint64Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint64Sint64Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%-P-P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint64Sint64Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSint64Sint64Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed32Fixed32Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapFixed32Fixed32Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapFixed32Fixed32Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed32Fixed32Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%#P#P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed32Fixed32Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed32Fixed32Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed64Fixed64Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapFixed64Fixed64Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapFixed64Fixed64Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed64Fixed64Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%$P$P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed64Fixed64Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapFixed64Fixed64Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed32Sfixed32Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapSfixed32Sfixed32Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapSfixed32Sfixed32Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed32Sfixed32Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%%P%P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed32Sfixed32Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed32Sfixed32Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed64Sfixed64Entry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapSfixed64Sfixed64Entry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapSfixed64Sfixed64Entry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed64Sfixed64Entry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%&P&P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed64Sfixed64Entry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapSfixed64Sfixed64Entry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32FloatEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapInt32FloatEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapInt32FloatEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32FloatEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%(P!P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32FloatEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32FloatEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32DoubleEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapInt32DoubleEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapInt32DoubleEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32DoubleEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%(P P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32DoubleEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapInt32DoubleEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapBoolBoolEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapBoolBoolEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapBoolBoolEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapBoolBoolEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%/P/P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapBoolBoolEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapBoolBoolEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringStringEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringStringEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringStringEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringStringEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X1X");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringStringEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringStringEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringBytesEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringBytesEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringBytesEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringBytesEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X0P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringBytesEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringBytesEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedMessageEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringNestedMessageEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringNestedMessageEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        <super::super::google_protobuf_test_messages_proto3_proto::TestAllTypesProto3 as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table();
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedMessageEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignMessageEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringForeignMessageEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringForeignMessageEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignMessageEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X3");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignMessageEntry_msg_init.0, &[<super::super::google_protobuf_test_messages_proto3_proto::ForeignMessage as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignMessageEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedEnumEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringNestedEnumEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringNestedEnumEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedEnumEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X.P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedEnumEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringNestedEnumEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignEnumEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct MapStringForeignEnumEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for MapStringForeignEnumEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignEnumEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X.P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignEnumEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::protobuf_0test_0messages__proto3__TestAllTypesProto3__MapStringForeignEnumEntry_msg_init.0)
      }).0
    }
  }
}
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NestedEnum(i32);

#[allow(non_upper_case_globals)]
impl NestedEnum {
  pub const Foo: NestedEnum = NestedEnum(0);
  pub const Bar: NestedEnum = NestedEnum(1);
  pub const Baz: NestedEnum = NestedEnum(2);
  pub const Neg: NestedEnum = NestedEnum(-1);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "Foo",
      1 => "Bar",
      2 => "Baz",
      -1 => "Neg",
      _ => return None
    })
  }
}

impl ::std::convert::From<NestedEnum> for i32 {
  fn from(val: NestedEnum) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for NestedEnum {
  fn from(val: i32) -> NestedEnum {
    Self(val)
  }
}

impl ::std::default::Default for NestedEnum {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for NestedEnum {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "NestedEnum::{}", constant_name)
    } else {
      write!(f, "NestedEnum::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for NestedEnum {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for NestedEnum {}

impl ::protobuf::Proxied for NestedEnum {
  type View<'a> = NestedEnum;
}

impl ::protobuf::AsView for NestedEnum {
  type Proxied = NestedEnum;

  fn as_view(&self) -> NestedEnum {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NestedEnum {
  fn into_view<'shorter>(self) -> NestedEnum where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for NestedEnum {
  const NAME: &'static str = "NestedEnum";

  fn is_known(value: i32) -> bool {
    matches!(value, 0|1|2|-1)
  }
}

impl ::protobuf::__internal::EntityType for NestedEnum {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AliasedEnum(i32);

#[allow(non_upper_case_globals)]
impl AliasedEnum {
  pub const AliasFoo: AliasedEnum = AliasedEnum(0);
  pub const AliasBar: AliasedEnum = AliasedEnum(1);
  pub const AliasBaz: AliasedEnum = AliasedEnum(2);
  pub const Moo: AliasedEnum = AliasedEnum(2);
  pub const Baz: AliasedEnum = AliasedEnum(2);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "AliasFoo",
      1 => "AliasBar",
      2 => "AliasBaz",
      _ => return None
    })
  }
}

impl ::std::convert::From<AliasedEnum> for i32 {
  fn from(val: AliasedEnum) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for AliasedEnum {
  fn from(val: i32) -> AliasedEnum {
    Self(val)
  }
}

impl ::std::default::Default for AliasedEnum {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for AliasedEnum {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "AliasedEnum::{}", constant_name)
    } else {
      write!(f, "AliasedEnum::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for AliasedEnum {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for AliasedEnum {}

impl ::protobuf::Proxied for AliasedEnum {
  type View<'a> = AliasedEnum;
}

impl ::protobuf::AsView for AliasedEnum {
  type Proxied = AliasedEnum;

  fn as_view(&self) -> AliasedEnum {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for AliasedEnum {
  fn into_view<'shorter>(self) -> AliasedEnum where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for AliasedEnum {
  const NAME: &'static str = "AliasedEnum";

  fn is_known(value: i32) -> bool {
    matches!(value, 0|1|2)
  }
}

impl ::protobuf::__internal::EntityType for AliasedEnum {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}


#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
#[repr(u32)]
pub enum OneofFieldOneof<'msg> {
  OneofUint32(u32) = 111,
  OneofNestedMessage(::protobuf::View<'msg, super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedMessage>) = 112,
  OneofString(&'msg ::protobuf::ProtoStr) = 113,
  OneofBytes(&'msg [u8]) = 114,
  OneofBool(bool) = 115,
  OneofUint64(u64) = 116,
  OneofFloat(f32) = 117,
  OneofDouble(f64) = 118,
  OneofEnum(::protobuf::View<'msg, super::super::google_protobuf_test_messages_proto3_proto::test_all_types_proto3::NestedEnum>) = 119,
  OneofNullValue(::protobuf::View<'msg, super::super::google_protobuf_struct_proto::NullValue>) = 120,

  not_set(std::marker::PhantomData<&'msg ()>) = 0
}
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(dead_code)]
pub enum OneofFieldCase {
  OneofUint32 = 111,
  OneofNestedMessage = 112,
  OneofString = 113,
  OneofBytes = 114,
  OneofBool = 115,
  OneofUint64 = 116,
  OneofFloat = 117,
  OneofDouble = 118,
  OneofEnum = 119,
  OneofNullValue = 120,

  not_set = 0
}

impl OneofFieldCase {
  #[allow(dead_code)]
  pub(crate) fn try_from(v: u32) -> ::std::option::Option<OneofFieldCase> {
    match v {
      0 => Some(OneofFieldCase::not_set),
      111 => Some(OneofFieldCase::OneofUint32),
      112 => Some(OneofFieldCase::OneofNestedMessage),
      113 => Some(OneofFieldCase::OneofString),
      114 => Some(OneofFieldCase::OneofBytes),
      115 => Some(OneofFieldCase::OneofBool),
      116 => Some(OneofFieldCase::OneofUint64),
      117 => Some(OneofFieldCase::OneofFloat),
      118 => Some(OneofFieldCase::OneofDouble),
      119 => Some(OneofFieldCase::OneofEnum),
      120 => Some(OneofFieldCase::OneofNullValue),
      _ => None
    }
  }
}
}  // pub mod test_all_types_proto3


// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__ForeignMessage_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct ForeignMessage {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<ForeignMessage>
}

impl ::protobuf::Message for ForeignMessage {
  type MessageView<'msg> = ForeignMessageView<'msg>;
  type MessageMut<'msg> = ForeignMessageMut<'msg>;
}

impl ::std::default::Default for ForeignMessage {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for ForeignMessage {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `ForeignMessage` is `Sync` because it does not implement interior mutability.
//    Neither does `ForeignMessageMut`.
unsafe impl ::std::marker::Sync for ForeignMessage {}

// SAFETY:
// - `ForeignMessage` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for ForeignMessage {}

impl ::protobuf::Proxied for ForeignMessage {
  type View<'msg> = ForeignMessageView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for ForeignMessage {}

impl ::protobuf::MutProxied for ForeignMessage {
  type Mut<'msg> = ForeignMessageMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct ForeignMessageView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, ForeignMessage>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ForeignMessageView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for ForeignMessageView<'msg> {
  type Message = ForeignMessage;
}

impl ::std::fmt::Debug for ForeignMessageView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for ForeignMessageView<'_> {
  fn default() -> ForeignMessageView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, ForeignMessage>> for ForeignMessageView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, ForeignMessage>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ForeignMessageView<'msg> {

  pub fn to_owned(&self) -> ForeignMessage {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // c: optional int32
  pub fn c(self) -> i32 {
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
// - `ForeignMessageView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for ForeignMessageView<'_> {}

// SAFETY:
// - `ForeignMessageView` is `Send` because while its alive a `ForeignMessageMut` cannot.
// - `ForeignMessageView` does not use thread-local data.
unsafe impl ::std::marker::Send for ForeignMessageView<'_> {}

impl<'msg> ::protobuf::AsView for ForeignMessageView<'msg> {
  type Proxied = ForeignMessage;
  fn as_view(&self) -> ::protobuf::View<'msg, ForeignMessage> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ForeignMessageView<'msg> {
  fn into_view<'shorter>(self) -> ForeignMessageView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<ForeignMessage> for ForeignMessageView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> ForeignMessage {
    let mut dst = ForeignMessage::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<ForeignMessage> for ForeignMessageMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> ForeignMessage {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for ForeignMessage {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ForeignMessageView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for ForeignMessageMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct ForeignMessageMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, ForeignMessage>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for ForeignMessageMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for ForeignMessageMut<'msg> {
  type Message = ForeignMessage;
}

impl ::std::fmt::Debug for ForeignMessageMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, ForeignMessage>> for ForeignMessageMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, ForeignMessage>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> ForeignMessageMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, ForeignMessage> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> ForeignMessage {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // c: optional int32
  pub fn c(&self) -> i32 {
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
  pub fn set_c(&mut self, val: i32) {
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
// - `ForeignMessageMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for ForeignMessageMut<'_> {}

// SAFETY:
// - `ForeignMessageMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for ForeignMessageMut<'_> {}

impl<'msg> ::protobuf::AsView for ForeignMessageMut<'msg> {
  type Proxied = ForeignMessage;
  fn as_view(&self) -> ::protobuf::View<'_, ForeignMessage> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ForeignMessageMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, ForeignMessage>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for ForeignMessageMut<'msg> {
  type MutProxied = ForeignMessage;
  fn as_mut(&mut self) -> ForeignMessageMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for ForeignMessageMut<'msg> {
  fn into_mut<'shorter>(self) -> ForeignMessageMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl ForeignMessage {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, ForeignMessage> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> ForeignMessageView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> ForeignMessageMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // c: optional int32
  pub fn c(&self) -> i32 {
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
  pub fn set_c(&mut self, val: i32) {
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

}  // impl ForeignMessage

impl ::std::ops::Drop for ForeignMessage {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for ForeignMessage {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for ForeignMessage {
  type Proxied = Self;
  fn as_view(&self) -> ForeignMessageView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for ForeignMessage {
  type MutProxied = Self;
  fn as_mut(&mut self) -> ForeignMessageMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for ForeignMessage {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__ForeignMessage_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__ForeignMessage_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__ForeignMessage_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ForeignMessage {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ForeignMessage {
  type Msg = ForeignMessage;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ForeignMessage> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ForeignMessage {
  type Msg = ForeignMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ForeignMessage> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for ForeignMessageMut<'_> {
  type Msg = ForeignMessage;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ForeignMessage> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ForeignMessageMut<'_> {
  type Msg = ForeignMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ForeignMessage> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for ForeignMessageView<'_> {
  type Msg = ForeignMessage;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<ForeignMessage> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for ForeignMessageMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__NullHypothesisProto3_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct NullHypothesisProto3 {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<NullHypothesisProto3>
}

impl ::protobuf::Message for NullHypothesisProto3 {
  type MessageView<'msg> = NullHypothesisProto3View<'msg>;
  type MessageMut<'msg> = NullHypothesisProto3Mut<'msg>;
}

impl ::std::default::Default for NullHypothesisProto3 {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for NullHypothesisProto3 {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `NullHypothesisProto3` is `Sync` because it does not implement interior mutability.
//    Neither does `NullHypothesisProto3Mut`.
unsafe impl ::std::marker::Sync for NullHypothesisProto3 {}

// SAFETY:
// - `NullHypothesisProto3` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for NullHypothesisProto3 {}

impl ::protobuf::Proxied for NullHypothesisProto3 {
  type View<'msg> = NullHypothesisProto3View<'msg>;
}

impl ::protobuf::__internal::SealedInternal for NullHypothesisProto3 {}

impl ::protobuf::MutProxied for NullHypothesisProto3 {
  type Mut<'msg> = NullHypothesisProto3Mut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct NullHypothesisProto3View<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, NullHypothesisProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NullHypothesisProto3View<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for NullHypothesisProto3View<'msg> {
  type Message = NullHypothesisProto3;
}

impl ::std::fmt::Debug for NullHypothesisProto3View<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for NullHypothesisProto3View<'_> {
  fn default() -> NullHypothesisProto3View<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, NullHypothesisProto3>> for NullHypothesisProto3View<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, NullHypothesisProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NullHypothesisProto3View<'msg> {

  pub fn to_owned(&self) -> NullHypothesisProto3 {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

}

// SAFETY:
// - `NullHypothesisProto3View` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for NullHypothesisProto3View<'_> {}

// SAFETY:
// - `NullHypothesisProto3View` is `Send` because while its alive a `NullHypothesisProto3Mut` cannot.
// - `NullHypothesisProto3View` does not use thread-local data.
unsafe impl ::std::marker::Send for NullHypothesisProto3View<'_> {}

impl<'msg> ::protobuf::AsView for NullHypothesisProto3View<'msg> {
  type Proxied = NullHypothesisProto3;
  fn as_view(&self) -> ::protobuf::View<'msg, NullHypothesisProto3> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NullHypothesisProto3View<'msg> {
  fn into_view<'shorter>(self) -> NullHypothesisProto3View<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<NullHypothesisProto3> for NullHypothesisProto3View<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> NullHypothesisProto3 {
    let mut dst = NullHypothesisProto3::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<NullHypothesisProto3> for NullHypothesisProto3Mut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> NullHypothesisProto3 {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for NullHypothesisProto3 {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NullHypothesisProto3View<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for NullHypothesisProto3Mut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct NullHypothesisProto3Mut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, NullHypothesisProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for NullHypothesisProto3Mut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for NullHypothesisProto3Mut<'msg> {
  type Message = NullHypothesisProto3;
}

impl ::std::fmt::Debug for NullHypothesisProto3Mut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, NullHypothesisProto3>> for NullHypothesisProto3Mut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, NullHypothesisProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> NullHypothesisProto3Mut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, NullHypothesisProto3> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> NullHypothesisProto3 {
    ::protobuf::AsView::as_view(self).to_owned()
  }

}

// SAFETY:
// - `NullHypothesisProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for NullHypothesisProto3Mut<'_> {}

// SAFETY:
// - `NullHypothesisProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for NullHypothesisProto3Mut<'_> {}

impl<'msg> ::protobuf::AsView for NullHypothesisProto3Mut<'msg> {
  type Proxied = NullHypothesisProto3;
  fn as_view(&self) -> ::protobuf::View<'_, NullHypothesisProto3> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for NullHypothesisProto3Mut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, NullHypothesisProto3>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for NullHypothesisProto3Mut<'msg> {
  type MutProxied = NullHypothesisProto3;
  fn as_mut(&mut self) -> NullHypothesisProto3Mut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for NullHypothesisProto3Mut<'msg> {
  fn into_mut<'shorter>(self) -> NullHypothesisProto3Mut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl NullHypothesisProto3 {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, NullHypothesisProto3> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> NullHypothesisProto3View<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> NullHypothesisProto3Mut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

}  // impl NullHypothesisProto3

impl ::std::ops::Drop for NullHypothesisProto3 {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for NullHypothesisProto3 {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for NullHypothesisProto3 {
  type Proxied = Self;
  fn as_view(&self) -> NullHypothesisProto3View<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for NullHypothesisProto3 {
  type MutProxied = Self;
  fn as_mut(&mut self) -> NullHypothesisProto3Mut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for NullHypothesisProto3 {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__NullHypothesisProto3_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__NullHypothesisProto3_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__NullHypothesisProto3_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NullHypothesisProto3 {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NullHypothesisProto3 {
  type Msg = NullHypothesisProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NullHypothesisProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NullHypothesisProto3 {
  type Msg = NullHypothesisProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NullHypothesisProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for NullHypothesisProto3Mut<'_> {
  type Msg = NullHypothesisProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NullHypothesisProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NullHypothesisProto3Mut<'_> {
  type Msg = NullHypothesisProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NullHypothesisProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for NullHypothesisProto3View<'_> {
  type Msg = NullHypothesisProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<NullHypothesisProto3> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for NullHypothesisProto3Mut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut protobuf_0test_0messages__proto3__EnumOnlyProto3_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct EnumOnlyProto3 {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<EnumOnlyProto3>
}

impl ::protobuf::Message for EnumOnlyProto3 {
  type MessageView<'msg> = EnumOnlyProto3View<'msg>;
  type MessageMut<'msg> = EnumOnlyProto3Mut<'msg>;
}

impl ::std::default::Default for EnumOnlyProto3 {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for EnumOnlyProto3 {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `EnumOnlyProto3` is `Sync` because it does not implement interior mutability.
//    Neither does `EnumOnlyProto3Mut`.
unsafe impl ::std::marker::Sync for EnumOnlyProto3 {}

// SAFETY:
// - `EnumOnlyProto3` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for EnumOnlyProto3 {}

impl ::protobuf::Proxied for EnumOnlyProto3 {
  type View<'msg> = EnumOnlyProto3View<'msg>;
}

impl ::protobuf::__internal::SealedInternal for EnumOnlyProto3 {}

impl ::protobuf::MutProxied for EnumOnlyProto3 {
  type Mut<'msg> = EnumOnlyProto3Mut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct EnumOnlyProto3View<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, EnumOnlyProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EnumOnlyProto3View<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for EnumOnlyProto3View<'msg> {
  type Message = EnumOnlyProto3;
}

impl ::std::fmt::Debug for EnumOnlyProto3View<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for EnumOnlyProto3View<'_> {
  fn default() -> EnumOnlyProto3View<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, EnumOnlyProto3>> for EnumOnlyProto3View<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, EnumOnlyProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EnumOnlyProto3View<'msg> {

  pub fn to_owned(&self) -> EnumOnlyProto3 {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

}

// SAFETY:
// - `EnumOnlyProto3View` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for EnumOnlyProto3View<'_> {}

// SAFETY:
// - `EnumOnlyProto3View` is `Send` because while its alive a `EnumOnlyProto3Mut` cannot.
// - `EnumOnlyProto3View` does not use thread-local data.
unsafe impl ::std::marker::Send for EnumOnlyProto3View<'_> {}

impl<'msg> ::protobuf::AsView for EnumOnlyProto3View<'msg> {
  type Proxied = EnumOnlyProto3;
  fn as_view(&self) -> ::protobuf::View<'msg, EnumOnlyProto3> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EnumOnlyProto3View<'msg> {
  fn into_view<'shorter>(self) -> EnumOnlyProto3View<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<EnumOnlyProto3> for EnumOnlyProto3View<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> EnumOnlyProto3 {
    let mut dst = EnumOnlyProto3::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<EnumOnlyProto3> for EnumOnlyProto3Mut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> EnumOnlyProto3 {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for EnumOnlyProto3 {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EnumOnlyProto3View<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for EnumOnlyProto3Mut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct EnumOnlyProto3Mut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, EnumOnlyProto3>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for EnumOnlyProto3Mut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for EnumOnlyProto3Mut<'msg> {
  type Message = EnumOnlyProto3;
}

impl ::std::fmt::Debug for EnumOnlyProto3Mut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, EnumOnlyProto3>> for EnumOnlyProto3Mut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, EnumOnlyProto3>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> EnumOnlyProto3Mut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, EnumOnlyProto3> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> EnumOnlyProto3 {
    ::protobuf::AsView::as_view(self).to_owned()
  }

}

// SAFETY:
// - `EnumOnlyProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for EnumOnlyProto3Mut<'_> {}

// SAFETY:
// - `EnumOnlyProto3Mut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for EnumOnlyProto3Mut<'_> {}

impl<'msg> ::protobuf::AsView for EnumOnlyProto3Mut<'msg> {
  type Proxied = EnumOnlyProto3;
  fn as_view(&self) -> ::protobuf::View<'_, EnumOnlyProto3> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for EnumOnlyProto3Mut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, EnumOnlyProto3>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for EnumOnlyProto3Mut<'msg> {
  type MutProxied = EnumOnlyProto3;
  fn as_mut(&mut self) -> EnumOnlyProto3Mut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for EnumOnlyProto3Mut<'msg> {
  fn into_mut<'shorter>(self) -> EnumOnlyProto3Mut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl EnumOnlyProto3 {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, EnumOnlyProto3> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> EnumOnlyProto3View<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> EnumOnlyProto3Mut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

}  // impl EnumOnlyProto3

impl ::std::ops::Drop for EnumOnlyProto3 {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for EnumOnlyProto3 {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for EnumOnlyProto3 {
  type Proxied = Self;
  fn as_view(&self) -> EnumOnlyProto3View<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for EnumOnlyProto3 {
  type MutProxied = Self;
  fn as_mut(&mut self) -> EnumOnlyProto3Mut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for EnumOnlyProto3 {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__EnumOnlyProto3_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$");
        ::protobuf::__internal::runtime::link_mini_table(
            super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__EnumOnlyProto3_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::google_protobuf_test_messages_proto3_proto::protobuf_0test_0messages__proto3__EnumOnlyProto3_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for EnumOnlyProto3 {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for EnumOnlyProto3 {
  type Msg = EnumOnlyProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<EnumOnlyProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EnumOnlyProto3 {
  type Msg = EnumOnlyProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<EnumOnlyProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for EnumOnlyProto3Mut<'_> {
  type Msg = EnumOnlyProto3;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<EnumOnlyProto3> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EnumOnlyProto3Mut<'_> {
  type Msg = EnumOnlyProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<EnumOnlyProto3> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for EnumOnlyProto3View<'_> {
  type Msg = EnumOnlyProto3;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<EnumOnlyProto3> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for EnumOnlyProto3Mut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod enum_only_proto3 {
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bool(i32);

#[allow(non_upper_case_globals)]
impl Bool {
  pub const Kfalse: Bool = Bool(0);
  pub const Ktrue: Bool = Bool(1);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "Kfalse",
      1 => "Ktrue",
      _ => return None
    })
  }
}

impl ::std::convert::From<Bool> for i32 {
  fn from(val: Bool) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for Bool {
  fn from(val: i32) -> Bool {
    Self(val)
  }
}

impl ::std::default::Default for Bool {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for Bool {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "Bool::{}", constant_name)
    } else {
      write!(f, "Bool::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for Bool {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for Bool {}

impl ::protobuf::Proxied for Bool {
  type View<'a> = Bool;
}

impl ::protobuf::AsView for Bool {
  type Proxied = Bool;

  fn as_view(&self) -> Bool {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for Bool {
  fn into_view<'shorter>(self) -> Bool where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for Bool {
  const NAME: &'static str = "Bool";

  fn is_known(value: i32) -> bool {
    matches!(value, 0|1)
  }
}

impl ::protobuf::__internal::EntityType for Bool {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}


}  // pub mod enum_only_proto3


#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ForeignEnum(i32);

#[allow(non_upper_case_globals)]
impl ForeignEnum {
  pub const ForeignFoo: ForeignEnum = ForeignEnum(0);
  pub const ForeignBar: ForeignEnum = ForeignEnum(1);
  pub const ForeignBaz: ForeignEnum = ForeignEnum(2);

  fn constant_name(&self) -> ::std::option::Option<&'static str> {
    #[allow(unreachable_patterns)] // In the case of aliases, just emit them all and let the first one match.
    Some(match self.0 {
      0 => "ForeignFoo",
      1 => "ForeignBar",
      2 => "ForeignBaz",
      _ => return None
    })
  }
}

impl ::std::convert::From<ForeignEnum> for i32 {
  fn from(val: ForeignEnum) -> i32 {
    val.0
  }
}

impl ::std::convert::From<i32> for ForeignEnum {
  fn from(val: i32) -> ForeignEnum {
    Self(val)
  }
}

impl ::std::default::Default for ForeignEnum {
  fn default() -> Self {
    Self(0)
  }
}

impl ::std::fmt::Debug for ForeignEnum {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    if let Some(constant_name) = self.constant_name() {
      write!(f, "ForeignEnum::{}", constant_name)
    } else {
      write!(f, "ForeignEnum::from({})", self.0)
    }
  }
}

impl ::protobuf::IntoProxied<i32> for ForeignEnum {
  fn into_proxied(self, _: ::protobuf::__internal::Private) -> i32 {
    self.0
  }
}

impl ::protobuf::__internal::SealedInternal for ForeignEnum {}

impl ::protobuf::Proxied for ForeignEnum {
  type View<'a> = ForeignEnum;
}

impl ::protobuf::AsView for ForeignEnum {
  type Proxied = ForeignEnum;

  fn as_view(&self) -> ForeignEnum {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for ForeignEnum {
  fn into_view<'shorter>(self) -> ForeignEnum where 'msg: 'shorter {
    self
  }
}

// SAFETY: this is an enum type
unsafe impl ::protobuf::__internal::Enum for ForeignEnum {
  const NAME: &'static str = "ForeignEnum";

  fn is_known(value: i32) -> bool {
    matches!(value, 0|1|2)
  }
}

impl ::protobuf::__internal::EntityType for ForeignEnum {
    type Tag = ::protobuf::__internal::entity_tag::EnumTag;
}


