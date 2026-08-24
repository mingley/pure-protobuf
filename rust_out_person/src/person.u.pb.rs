const _: () = ::protobuf::__internal::assert_compatible_gencode_version("4.35.1-release");
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut example__Address_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Address {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Address>
}

impl ::protobuf::Message for Address {
  type MessageView<'msg> = AddressView<'msg>;
  type MessageMut<'msg> = AddressMut<'msg>;
}

impl ::std::default::Default for Address {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Address {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Address` is `Sync` because it does not implement interior mutability.
//    Neither does `AddressMut`.
unsafe impl ::std::marker::Sync for Address {}

// SAFETY:
// - `Address` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Address {}

impl ::protobuf::Proxied for Address {
  type View<'msg> = AddressView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Address {}

impl ::protobuf::MutProxied for Address {
  type Mut<'msg> = AddressMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct AddressView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Address>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for AddressView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for AddressView<'msg> {
  type Message = Address;
}

impl ::std::fmt::Debug for AddressView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for AddressView<'_> {
  fn default() -> AddressView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Address>> for AddressView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Address>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> AddressView<'msg> {

  pub fn to_owned(&self) -> Address {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // city: optional string
  pub fn city(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

}

// SAFETY:
// - `AddressView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for AddressView<'_> {}

// SAFETY:
// - `AddressView` is `Send` because while its alive a `AddressMut` cannot.
// - `AddressView` does not use thread-local data.
unsafe impl ::std::marker::Send for AddressView<'_> {}

impl<'msg> ::protobuf::AsView for AddressView<'msg> {
  type Proxied = Address;
  fn as_view(&self) -> ::protobuf::View<'msg, Address> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for AddressView<'msg> {
  fn into_view<'shorter>(self) -> AddressView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Address> for AddressView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Address {
    let mut dst = Address::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Address> for AddressMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Address {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Address {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for AddressView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for AddressMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct AddressMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Address>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for AddressMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for AddressMut<'msg> {
  type Message = Address;
}

impl ::std::fmt::Debug for AddressMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Address>> for AddressMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Address>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> AddressMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Address> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Address {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // city: optional string
  pub fn city(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_city(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}

// SAFETY:
// - `AddressMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for AddressMut<'_> {}

// SAFETY:
// - `AddressMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for AddressMut<'_> {}

impl<'msg> ::protobuf::AsView for AddressMut<'msg> {
  type Proxied = Address;
  fn as_view(&self) -> ::protobuf::View<'_, Address> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for AddressMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Address>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for AddressMut<'msg> {
  type MutProxied = Address;
  fn as_mut(&mut self) -> AddressMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for AddressMut<'msg> {
  fn into_mut<'shorter>(self) -> AddressMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Address {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Address> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> AddressView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> AddressMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // city: optional string
  pub fn city(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        0, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_city(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        0,
        val);
    }
  }

}  // impl Address

impl ::std::ops::Drop for Address {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Address {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Address {
  type Proxied = Self;
  fn as_view(&self) -> AddressView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Address {
  type MutProxied = Self;
  fn as_mut(&mut self) -> AddressMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Address {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::example__Address_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$M1P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::example__Address_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::example__Address_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Address {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Address {
  type Msg = Address;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Address> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Address {
  type Msg = Address;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Address> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for AddressMut<'_> {
  type Msg = Address;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Address> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for AddressMut<'_> {
  type Msg = Address;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Address> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for AddressView<'_> {
  type Msg = Address;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Address> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for AddressMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}



// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut example__Person_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(non_camel_case_types)]
pub struct Person {
  inner: ::protobuf::__internal::runtime::OwnedMessageInner<Person>
}

impl ::protobuf::Message for Person {
  type MessageView<'msg> = PersonView<'msg>;
  type MessageMut<'msg> = PersonMut<'msg>;
}

impl ::std::default::Default for Person {
  fn default() -> Self {
    Self::new()
  }
}

impl ::std::fmt::Debug for Person {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

// SAFETY:
// - `Person` is `Sync` because it does not implement interior mutability.
//    Neither does `PersonMut`.
unsafe impl ::std::marker::Sync for Person {}

// SAFETY:
// - `Person` is `Send` because it uniquely owns its arena and does
//   not use thread-local data.
unsafe impl ::std::marker::Send for Person {}

impl ::protobuf::Proxied for Person {
  type View<'msg> = PersonView<'msg>;
}

impl ::protobuf::__internal::SealedInternal for Person {}

impl ::protobuf::MutProxied for Person {
  type Mut<'msg> = PersonMut<'msg>;
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct PersonView<'msg> {
  inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Person>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for PersonView<'msg> {}

impl<'msg> ::protobuf::MessageView<'msg> for PersonView<'msg> {
  type Message = Person;
}

impl ::std::fmt::Debug for PersonView<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl ::std::default::Default for PersonView<'_> {
  fn default() -> PersonView<'static> {
    ::protobuf::__internal::runtime::MessageViewInner::default().into()
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageViewInner<'msg, Person>> for PersonView<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageViewInner<'msg, Person>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> PersonView<'msg> {

  pub fn to_owned(&self) -> Person {
    ::protobuf::IntoProxied::into_proxied(*self, ::protobuf::__internal::Private)
  }

  // id: optional int32
  pub fn id(self) -> i32 {
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

  // name: optional string
  pub fn name(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // email: optional string
  pub fn has_email(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn email_opt(self) -> ::std::option::Option<&'msg ::protobuf::ProtoStr> {
    self.has_email().then(|| self.email())
  }
  pub fn email(self) -> ::protobuf::View<'msg, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }

  // tags: repeated string
  pub fn tags(self) -> ::protobuf::RepeatedView<'msg, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        3
      )
    }.map_or_else(
        ::protobuf::__internal::runtime::empty_array::<::protobuf::ProtoString>,
        |raw| unsafe {
          ::protobuf::RepeatedView::from_raw(::protobuf::__internal::Private, raw)
        }
      )
  }

  // scores: repeated message example.Person.ScoresEntry
  pub fn scores(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(4)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

  // address: optional message example.Address
  pub fn has_address(self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn address_opt(self) -> ::std::option::Option<super::AddressView<'msg>> {
    self.has_address().then(|| self.address())
  }
  pub fn address(self) -> super::AddressView<'msg> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::AddressView::default())
  }

  // extras: repeated message example.Person.ExtrasEntry
  pub fn extras(self)
    -> ::protobuf::MapView<'msg, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(6)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }

}

// SAFETY:
// - `PersonView` is `Sync` because it does not support mutation.
unsafe impl ::std::marker::Sync for PersonView<'_> {}

// SAFETY:
// - `PersonView` is `Send` because while its alive a `PersonMut` cannot.
// - `PersonView` does not use thread-local data.
unsafe impl ::std::marker::Send for PersonView<'_> {}

impl<'msg> ::protobuf::AsView for PersonView<'msg> {
  type Proxied = Person;
  fn as_view(&self) -> ::protobuf::View<'msg, Person> {
    *self
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for PersonView<'msg> {
  fn into_view<'shorter>(self) -> PersonView<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

impl<'msg> ::protobuf::IntoProxied<Person> for PersonView<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Person {
    let mut dst = Person::new();
    assert!(unsafe {
      dst.inner.ptr_mut().deep_copy(self.inner.ptr(), dst.inner.arena())
    });
    dst
  }
}

impl<'msg> ::protobuf::IntoProxied<Person> for PersonMut<'msg> {
  fn into_proxied(self, _private: ::protobuf::__internal::Private) -> Person {
    ::protobuf::IntoProxied::into_proxied(::protobuf::IntoView::into_view(self), _private)
  }
}

impl ::protobuf::__internal::EntityType for Person {
    type Tag = ::protobuf::__internal::entity_tag::MessageTag;
}

impl<'msg> ::protobuf::__internal::EntityType for PersonView<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::ViewProxyTag;
}

impl<'msg> ::protobuf::__internal::EntityType for PersonMut<'msg> {
    type Tag = ::protobuf::__internal::entity_tag::MutProxyTag;
}

#[allow(dead_code)]
#[allow(non_camel_case_types)]
pub struct PersonMut<'msg> {
  inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Person>,
}

impl<'msg> ::protobuf::__internal::SealedInternal for PersonMut<'msg> {}

impl<'msg> ::protobuf::MessageMut<'msg> for PersonMut<'msg> {
  type Message = Person;
}

impl ::std::fmt::Debug for PersonMut<'_> {
  fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    write!(f, "{}", ::protobuf::__internal::runtime::debug_string(self))
  }
}

impl<'msg> From<::protobuf::__internal::runtime::MessageMutInner<'msg, Person>> for PersonMut<'msg> {
  fn from(inner: ::protobuf::__internal::runtime::MessageMutInner<'msg, Person>) -> Self {
    Self { inner }
  }
}

#[allow(dead_code)]
impl<'msg> PersonMut<'msg> {

  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private)
    -> ::protobuf::__internal::runtime::MessageMutInner<'msg, Person> {
    self.inner.reborrow()
  }

  pub fn to_owned(&self) -> Person {
    ::protobuf::AsView::as_view(self).to_owned()
  }

  // id: optional int32
  pub fn id(&self) -> i32 {
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
  pub fn set_id(&mut self, val: i32) {
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

  // name: optional string
  pub fn name(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_name(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  // email: optional string
  pub fn has_email(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn clear_email(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        2
      );
    }
  }
  pub fn email_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_email().then(|| self.email())
  }
  pub fn email(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_email(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        3
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
        3,
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
        3,
        src);
    }
  }

  // scores: repeated message example.Person.ScoresEntry
  pub fn scores(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(4)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn scores_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          4, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_scores(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        src);
    }
  }

  // address: optional message example.Address
  pub fn has_address(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn clear_address(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        5
      );
    }
  }
  pub fn address_opt(&self) -> ::std::option::Option<super::AddressView<'_>> {
    self.has_address().then(|| self.address())
  }
  pub fn address(&self) -> super::AddressView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::AddressView::default())
  }
  pub fn address_mut(&mut self) -> super::AddressMut<'_> {
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
  pub fn set_address(&mut self,
    val: impl ::protobuf::IntoProxied<super::Address>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        val
      );
    }
  }

  // extras: repeated message example.Person.ExtrasEntry
  pub fn extras(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(6)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn extras_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          6, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_extras(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        6,
        src);
    }
  }

}

// SAFETY:
// - `PersonMut` does not perform any shared mutation.
unsafe impl ::std::marker::Send for PersonMut<'_> {}

// SAFETY:
// - `PersonMut` does not perform any shared mutation.
unsafe impl ::std::marker::Sync for PersonMut<'_> {}

impl<'msg> ::protobuf::AsView for PersonMut<'msg> {
  type Proxied = Person;
  fn as_view(&self) -> ::protobuf::View<'_, Person> {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::IntoView<'msg> for PersonMut<'msg> {
  fn into_view<'shorter>(self) -> ::protobuf::View<'shorter, Person>
  where
      'msg: 'shorter {
    self.inner.as_view().into()
  }
}

impl<'msg> ::protobuf::AsMut for PersonMut<'msg> {
  type MutProxied = Person;
  fn as_mut(&mut self) -> PersonMut<'msg> {
    self.inner.reborrow().into()
  }
}

impl<'msg> ::protobuf::IntoMut<'msg> for PersonMut<'msg> {
  fn into_mut<'shorter>(self) -> PersonMut<'shorter>
  where
      'msg: 'shorter {
    self
  }
}

#[allow(dead_code)]
impl Person {
  pub fn new() -> Self {
    Self { inner: ::protobuf::__internal::runtime::OwnedMessageInner::<Self>::new() }
  }


  #[doc(hidden)]
  pub fn as_message_mut_inner(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessageMutInner<'_, Person> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner)
  }

  pub fn as_view(&self) -> PersonView<'_> {
    ::protobuf::__internal::runtime::MessageViewInner::view_of_owned(&self.inner).into()
  }

  pub fn as_mut(&mut self) -> PersonMut<'_> {
    ::protobuf::__internal::runtime::MessageMutInner::mut_of_owned(&mut self.inner).into()
  }

  // id: optional int32
  pub fn id(&self) -> i32 {
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
  pub fn set_id(&mut self, val: i32) {
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

  // name: optional string
  pub fn name(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        1, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_name(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        1,
        val);
    }
  }

  // email: optional string
  pub fn has_email(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(2)
    }
  }
  pub fn clear_email(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        2
      );
    }
  }
  pub fn email_opt(&self) -> ::std::option::Option<&'_ ::protobuf::ProtoStr> {
    self.has_email().then(|| self.email())
  }
  pub fn email(&self) -> ::protobuf::View<'_, ::protobuf::ProtoString> {
    let str_view = unsafe {
      self.inner.ptr().get_string_at_index(
        2, (b"").into()
      )
    };
    ::protobuf::ProtoStr::from_utf8_unchecked(unsafe { str_view.as_ref() })
  }
  pub fn set_email(&mut self, val: impl ::protobuf::IntoProxied<::protobuf::ProtoString>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_string_field(
        ::protobuf::AsMut::as_mut(self).inner,
        2,
        val);
    }
  }

  // tags: repeated string
  pub fn tags(&self) -> ::protobuf::RepeatedView<'_, ::protobuf::ProtoString> {
    unsafe {
      self.inner.ptr().get_array_at_index(
        3
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
        3,
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
        3,
        src);
    }
  }

  // scores: repeated message example.Person.ScoresEntry
  pub fn scores(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(4)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn scores_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          4, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_scores(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        4,
        src);
    }
  }

  // address: optional message example.Address
  pub fn has_address(&self) -> bool {
    unsafe {
      self.inner.ptr().has_field_at_index(5)
    }
  }
  pub fn clear_address(&mut self) {
    unsafe {
      self.inner.ptr().clear_field_at_index(
        5
      );
    }
  }
  pub fn address_opt(&self) -> ::std::option::Option<super::AddressView<'_>> {
    self.has_address().then(|| self.address())
  }
  pub fn address(&self) -> super::AddressView<'_> {
    let submsg = unsafe {
      self.inner.ptr().get_message_at_index(5)
    };
    submsg
        .map(|ptr| unsafe { ::protobuf::__internal::runtime::MessageViewInner::wrap(ptr).into() })
       .unwrap_or(super::AddressView::default())
  }
  pub fn address_mut(&mut self) -> super::AddressMut<'_> {
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
  pub fn set_address(&mut self,
    val: impl ::protobuf::IntoProxied<super::Address>) {

    unsafe {
      ::protobuf::__internal::runtime::message_set_sub_message(
        ::protobuf::AsMut::as_mut(self).inner,
        5,
        val
      );
    }
  }

  // extras: repeated message example.Person.ExtrasEntry
  pub fn extras(&self)
    -> ::protobuf::MapView<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      <Self as ::protobuf::__internal::runtime::UpbGetMessagePtr>::get_ptr(&self, ::protobuf::__internal::Private)
        .get_map_at_index(6)
        .map_or_else(
          ::protobuf::__internal::runtime::empty_map::<::protobuf::ProtoString, i32>,
          |raw| ::protobuf::MapView::from_raw(::protobuf::__internal::Private, raw)
        )
    }
  }
  pub fn extras_mut(&mut self)
    -> ::protobuf::MapMut<'_, ::protobuf::ProtoString, i32> {
    unsafe {
      let raw_map = <Self as ::protobuf::__internal::runtime::UpbGetMessagePtrMut>::get_ptr_mut(self, ::protobuf::__internal::Private)
        .get_or_create_mutable_map_at_index(
          6, self.inner.arena()).unwrap();
      let inner = ::protobuf::__internal::runtime::InnerMapMut::new(
        raw_map, self.inner.arena());
      ::protobuf::MapMut::from_inner(::protobuf::__internal::Private, inner)
    }
  }
  pub fn set_extras(
      &mut self,
      src: impl ::protobuf::IntoProxied<::protobuf::Map<::protobuf::ProtoString, i32>>) {
    unsafe {
      ::protobuf::__internal::runtime::message_set_map_field(
        ::protobuf::AsMut::as_mut(self).inner,
        6,
        src);
    }
  }

}  // impl Person

impl ::std::ops::Drop for Person {
  #[inline]
  fn drop(&mut self) {
  }
}

impl ::std::clone::Clone for Person {
  fn clone(&self) -> Self {
    self.as_view().to_owned()
  }
}

impl ::protobuf::AsView for Person {
  type Proxied = Self;
  fn as_view(&self) -> PersonView<'_> {
    self.as_view()
  }
}

impl ::protobuf::AsMut for Person {
  type MutProxied = Self;
  fn as_mut(&mut self) -> PersonMut<'_> {
    self.as_mut()
  }
}

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for Person {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::example__Person_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("$(P1X1TETG3iG");
        ::protobuf::__internal::runtime::link_mini_table(
            super::example__Person_msg_init.0, &[<super::person::ScoresEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::Address as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            <super::person::ExtrasEntry as ::protobuf::__internal::runtime::AssociatedMiniTable>::mini_table(),
            ], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::example__Person_msg_init.0)
      }).0
    }
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetArena for Person {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for Person {
  type Msg = Person;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Person> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for Person {
  type Msg = Person;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Person> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtrMut for PersonMut<'_> {
  type Msg = Person;
  fn get_ptr_mut(&mut self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Person> {
    self.inner.ptr_mut()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for PersonMut<'_> {
  type Msg = Person;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Person> {
    self.inner.ptr()
  }
}
unsafe impl ::protobuf::__internal::runtime::UpbGetMessagePtr for PersonView<'_> {
  type Msg = Person;
  fn get_ptr(&self, _private: ::protobuf::__internal::Private) -> ::protobuf::__internal::runtime::MessagePtr<Person> {
    self.inner.ptr()
  }
}

unsafe impl ::protobuf::__internal::runtime::UpbGetArena for PersonMut<'_> {
  fn get_arena(&mut self, _private: ::protobuf::__internal::Private) -> &::protobuf::__internal::runtime::Arena {
    self.inner.arena()
  }
}

pub mod person {// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut example__Person__ScoresEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct ScoresEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for ScoresEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::person::example__Person__ScoresEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::person::example__Person__ScoresEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::person::example__Person__ScoresEntry_msg_init.0)
      }).0
    }
  }
}
// This variable must not be referenced except by protobuf generated
// code.
pub(crate) static mut example__Person__ExtrasEntry_msg_init: ::protobuf::__internal::runtime::MiniTableInitPtr =
    ::protobuf::__internal::runtime::MiniTableInitPtr(::protobuf::__internal::runtime::MiniTablePtr::dangling());
#[allow(dead_code)]
pub(super) struct ExtrasEntry;

unsafe impl ::protobuf::__internal::runtime::AssociatedMiniTable for ExtrasEntry {
  fn mini_table() -> ::protobuf::__internal::runtime::MiniTablePtr {
    static ONCE_LOCK: ::std::sync::OnceLock<::protobuf::__internal::runtime::MiniTableInitPtr> =
        ::std::sync::OnceLock::new();
    unsafe {
      ONCE_LOCK.get_or_init(|| {
        super::super::person::example__Person__ExtrasEntry_msg_init.0 =
            ::protobuf::__internal::runtime::build_mini_table("%1X(P");
        ::protobuf::__internal::runtime::link_mini_table(
            super::super::person::example__Person__ExtrasEntry_msg_init.0, &[], &[]);
        ::protobuf::__internal::runtime::MiniTableInitPtr(super::super::person::example__Person__ExtrasEntry_msg_init.0)
      }).0
    }
  }
}

}  // pub mod person


