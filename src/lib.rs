//! Pure-Rust protobuf kernel. Application API matches Google protobuf v4.
//!
//! Not crates.io `protobuf` 4.x (upb/C). Not prost.
//! Official `protoc --rust_out` links against `__internal::runtime`.
//!
//! ```ignore
//! pbrs = { git = "https://github.com/mingley/pure-protobuf" }
//! ```

#![deny(unsafe_op_in_unsafe_fn)]
#![expect(
    missing_docs,
    reason = "the v4 application API is described in the crate docs and README; per-item rustdoc is follow-up"
)]
#![expect(
    unsafe_code,
    reason = "MiniTable ABI, ProtoStr cast, packed memcpy, JSON strtod, generated zeroed Default"
)]
#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::let_underscore_must_use,
    clippy::too_many_lines,
    clippy::mem_forget,
    unreachable_pub,
    reason = "wire parser and generated gencode: checked offsets, specified integer widths, String fmt is infallible"
)]

extern crate self as pbrs;

#[doc(hidden)]
pub use paste as __paste;

pub use crate::dynamic::{
    Cardinality, DescriptorOption, DescriptorPool, DynamicMessage, DynamicMessageMut,
    DynamicMessageView, EnumDescriptor, FieldDescriptor, FieldType, FileDescriptor, MapKeyValue,
    MessageDescriptor, MethodDescriptor, Presence, ServiceDescriptor, Value, RECURSION_LIMIT,
};
pub use crate::error::{ParseError, SerializeError};
pub use crate::map::{Map, MapIter, MapKey, MapMut, MapValue, MapView};
pub use crate::message::{
    message_eq, Clear, ClearAndParse, CopyFrom, Enum, MergeFrom, Message, MessageMut, MessageType,
    MessageView, Parse, Serialize, TakeFrom, UnknownEnumValue,
};
pub use crate::proxied::{
    AsMut, AsView, IntoMut, IntoProxied, IntoView, Mut, MutProxied, Proxied, View,
};
pub use crate::repeated::{ProtoPut, Repeated, RepeatedIter, RepeatedMut, RepeatedView, Singular};
pub use crate::string::{ProtoBytes, ProtoStr, ProtoString, Utf8Error};
pub use crate::wire::{UnknownFields, WireOut};

pub use Singular as ProxiedInRepeated;

pub mod prelude;

#[doc(hidden)]
pub mod __internal {
    pub use crate::internal::{
        assert_compatible_gencode_version, entity_tag, EntityType, Enum, MatcherEq, Private,
        SealedInternal,
    };
    pub use crate::runtime;
}

pub mod codegen;
mod dynamic;
mod error;
pub mod gen_support;
pub mod gencode;
mod generated;
mod internal;
pub mod json;
mod lazy;
mod map;
mod message;
mod packed;
mod proxied;
mod repeated;
pub mod rt;
#[doc(hidden)]
pub mod runtime;
mod string;
pub mod testdata;
pub(crate) mod text;
mod wire;

/// proto! enables Rust struct-init syntax for protobuf messages.
///
/// ```ignore
/// let msg = proto!(Person {
///     id: 1,
///     name: "ada",
///     address: Address { city: "nyc" },
/// });
/// ```
#[macro_export]
macro_rules! proto {
    ($ty:ident { $($body:tt)* }) => {{
        let mut this = <$ty as ::core::default::Default>::default();
        $crate::proto!(@spread_owned this, [] $($body)*);
        this
    }};
    (@spread_owned $this:ident, [$($fs:tt)*] .. $rest:expr $(,)?) => {
        $crate::MergeFrom::merge_from(&mut $this, $rest);
        $crate::proto!(@owned $this, $($fs)*);
    };
    (@spread_owned $this:ident, [$($fs:tt)*] $t:tt $($rest:tt)*) => {
        $crate::proto!(@spread_owned $this, [$($fs)* $t] $($rest)*);
    };
    (@spread_owned $this:ident, [$($fs:tt)*]) => {
        $crate::proto!(@owned $this, $($fs)*);
    };
    (@spread_mut $this:ident, [$($fs:tt)*] .. $rest:expr $(,)?) => {
        $crate::MergeFrom::merge_from($this, $rest);
        $crate::proto!(@mut $this, $($fs)*);
    };
    (@spread_mut $this:ident, [$($fs:tt)*] $t:tt $($rest:tt)*) => {
        $crate::proto!(@spread_mut $this, [$($fs)* $t] $($rest)*);
    };
    (@spread_mut $this:ident, [$($fs:tt)*]) => {
        $crate::proto!(@mut $this, $($fs)*);
    };
    (@owned $this:ident, ) => {};
    (@owned $this:ident, .. $rest:expr $(,)?) => {
        $crate::MergeFrom::merge_from(&mut $this, $rest);
    };
    (@owned $this:ident, $field:ident : __ { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __n = $this.[<$field _mut>]();
                $crate::proto!(@spread_mut __n, [] $($sub)*);
            }
        }
        $crate::proto!(@owned $this, $($($rest)*)?);
    };
    (@owned $this:ident, $field:ident : $subty:ident { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<set_ $field>]($crate::proto!($subty { $($sub)* }));
        }
        $crate::proto!(@owned $this, $($($rest)*)?);
    };
    (@owned $this:ident, $field:ident : [ $($arr:tt)* ] $(, $($rest:tt)*)?) => {
        $crate::proto!(@arr $this, $field, $($arr)*);
        $crate::proto!(@owned $this, $($($rest)*)?);
    };
    (@owned $this:ident, $field:ident : $val:expr $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<set_ $field>]($val);
        }
        $crate::proto!(@owned $this, $($($rest)*)?);
    };
    (@mut $this:ident, ) => {};
    (@mut $this:ident, .. $rest:expr $(,)?) => {
        $crate::MergeFrom::merge_from($this, $rest);
    };
    (@mut $this:ident, $field:ident : __ { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __n = $this.[<$field _mut>]();
                $crate::proto!(@spread_mut __n, [] $($sub)*);
            }
        }
        $crate::proto!(@mut $this, $($($rest)*)?);
    };
    (@mut $this:ident, $field:ident : $subty:ident { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<set_ $field>]($crate::proto!($subty { $($sub)* }));
        }
        $crate::proto!(@mut $this, $($($rest)*)?);
    };
    (@mut $this:ident, $field:ident : [ $($arr:tt)* ] $(, $($rest:tt)*)?) => {
        $crate::proto!(@arr_mut $this, $field, $($arr)*);
        $crate::proto!(@mut $this, $($($rest)*)?);
    };
    (@mut $this:ident, $field:ident : $val:expr $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<set_ $field>]($val);
        }
        $crate::proto!(@mut $this, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, ) => {};
    (@arr $this:ident, $field:ident, __ { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __r = $this.[<$field _mut>]();
                let mut __e = __r.push_default();
                $crate::proto!(@spread_mut __e, [] $($sub)*);
            }
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, $ty:ident { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().push($crate::proto!($ty { $($sub)* }));
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, ($k:expr, __ { $($sub:tt)* }) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __r = $this.[<$field _mut>]();
                let mut __v = __r.default_value();
                $crate::proto!(@owned __v, $($sub)*);
                __r.insert($k, __v);
            }
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, ($k:expr, $ty:ident { $($sub:tt)* }) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().insert($k, $crate::proto!($ty { $($sub)* }));
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, ($k:expr, $v:expr) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().insert($k, $v);
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr $this:ident, $field:ident, $val:expr $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().proto_put($val);
        }
        $crate::proto!(@arr $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, ) => {};
    (@arr_mut $this:ident, $field:ident, __ { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __r = $this.[<$field _mut>]();
                let mut __e = __r.push_default();
                $crate::proto!(@spread_mut __e, [] $($sub)*);
            }
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, $ty:ident { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().push($crate::proto!($ty { $($sub)* }));
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, ($k:expr, __ { $($sub:tt)* }) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            {
                let mut __r = $this.[<$field _mut>]();
                let mut __v = __r.default_value();
                $crate::proto!(@owned __v, $($sub)*);
                __r.insert($k, __v);
            }
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, ($k:expr, $ty:ident { $($sub:tt)* }) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().insert($k, $crate::proto!($ty { $($sub)* }));
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, ($k:expr, $v:expr) $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().insert($k, $v);
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
    (@arr_mut $this:ident, $field:ident, $val:expr $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $this.[<$field _mut>]().proto_put($val);
        }
        $crate::proto!(@arr_mut $this, $field, $($($rest)*)?);
    };
}
