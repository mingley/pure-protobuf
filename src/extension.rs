//! Typed native extension identifiers backed by a message's unknown fields.
//!
//! The initial generated-code surface supports selected singular Edition 2024
//! `int32` extensions. It does not implement the official upb extension ABI.

use crate::wire::{MAX_FIELD_NUMBER, UnknownField, UnknownFields};
use std::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
    impl Sealed for i32 {}
}

/// Supported scalar values for native typed extension identifiers.
///
/// This sealed trait currently supports only `i32`. Its wire helpers are used
/// by generated code and do not represent a generic extension codec API.
pub trait ExtensionValue: sealed::Sealed + Copy {
    #[doc(hidden)]
    fn __read(field: &UnknownField, number: u32) -> Option<Self>;
    #[doc(hidden)]
    fn __write(self, number: u32) -> UnknownField;
}

impl ExtensionValue for i32 {
    fn __read(field: &UnknownField, number: u32) -> Option<Self> {
        match field {
            UnknownField::Varint { number: tag, value } if *tag == number => {
                Some(*value as Self)
            }
            _ => None,
        }
    }

    fn __write(self, number: u32) -> UnknownField {
        UnknownField::Varint { number, value: i64::from(self) as u64 }
    }
}

/// A generated extension identifier, bound to its owned message and value type.
///
/// Reads scan existing unknown records without changing their order or
/// encoding. A getter returns the last matching value, or the declared default
/// when absent. Set/clear preserve same-number fields of another wire type.
pub struct Extension<M, V: ExtensionValue> {
    number: u32,
    full_name: &'static str,
    default: V,
    host: PhantomData<fn() -> M>,
}

impl<M, V: ExtensionValue> Copy for Extension<M, V> {}

impl<M, V: ExtensionValue> Clone for Extension<M, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, V: ExtensionValue> Extension<M, V> {
    /// Generated-code construction. Returns `None` for illegal/reserved tags.
    #[doc(hidden)]
    #[must_use]
    pub const fn __new(number: u32, full_name: &'static str, default: V) -> Option<Self> {
        if number == 0 || number > MAX_FIELD_NUMBER || (number >= 19_000 && number <= 19_999) {
            None
        } else {
            Some(Self { number, full_name, default, host: PhantomData })
        }
    }

    /// The protobuf field number.
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// The fully qualified protobuf extension name, without a leading dot.
    #[must_use]
    pub const fn full_name(&self) -> &'static str {
        self.full_name
    }

    /// The descriptor default returned when the extension is absent.
    #[must_use]
    pub const fn default_value(&self) -> V {
        self.default
    }
}

/// Storage access implemented only for generated owned extension hosts.
///
/// Mutable access must invalidate the message's encoded-size cache, including
/// access that ultimately leaves the fields unchanged.
#[doc(hidden)]
pub trait ExtensionHost {
    fn __extension_fields(&self) -> &UnknownFields;
    fn __extension_fields_mut(&mut self) -> &mut UnknownFields;
}

impl<M: ExtensionHost, V: ExtensionValue> Extension<M, V> {
    /// Read the last matching record, or the descriptor default when absent.
    #[must_use]
    pub fn get(&self, message: &M) -> V {
        message
            .__extension_fields()
            .fields
            .iter()
            .rev()
            .find_map(|field| V::__read(field, self.number))
            .unwrap_or(self.default)
    }

    /// Whether a matching scalar record is present, even if its value is zero.
    #[must_use]
    pub fn has(&self, message: &M) -> bool {
        message
            .__extension_fields()
            .fields
            .iter()
            .any(|field| V::__read(field, self.number).is_some())
    }

    /// Replace matching scalar records and append one explicitly present value.
    pub fn set(&self, message: &mut M, value: V) {
        let replacement = value.__write(self.number);
        let fields = &mut message.__extension_fields_mut().fields;
        fields.retain(|field| V::__read(field, self.number).is_none());
        fields.push(replacement);
    }

    /// Remove matching scalar records, preserving other wire types and fields.
    pub fn clear(&self, message: &mut M) {
        message
            .__extension_fields_mut()
            .fields
            .retain(|field| V::__read(field, self.number).is_none());
    }
}
