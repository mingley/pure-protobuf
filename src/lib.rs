//! Pure-Rust protobuf kernel. Application API matches Google protobuf v4.
//!
//! Not crates.io `protobuf` 4.x (upb/C). Not prost.
//! Google `protoc --rust_out` will not link. See the crate README and `docs/`.
//!
//! ```ignore
//! pbrs = { git = "https://github.com/mingley/pure-protobuf" }
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

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
pub use crate::wire::UnknownFields;

pub use Singular as ProxiedInRepeated;

pub mod prelude;

#[doc(hidden)]
pub mod __internal {
    pub use crate::internal::{Private, SealedInternal};
}

pub mod codegen;
mod dynamic;
mod error;
pub mod gen_support;
pub mod gencode;
mod generated;
mod internal;
pub(crate) mod json;
mod lazy;
mod map;
mod message;
mod packed;
mod proxied;
mod repeated;
pub mod rt;
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
                let __n = $this.[<$field _mut>]();
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
                let __n = $this.[<$field _mut>]();
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
                let __e = __r.push_default();
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
                let mut __v = ::core::default::Default::default();
                $crate::proto!(@owned __v, $($sub)*);
                $this.[<$field _mut>]().insert($k, __v);
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
                let __e = __r.push_default();
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
                let mut __v = ::core::default::Default::default();
                $crate::proto!(@owned __v, $($sub)*);
                $this.[<$field _mut>]().insert($k, __v);
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
