//! Pure-Rust Protocol Buffers kernel with the Google protobuf v4 **application** API.
//!
//! This is not the crates.io `protobuf` 4.x crate (that runtime is upb/C).
//! This is not prost. Google `protoc --rust_out` gencode will not link here.
//!
//! ```ignore
//! protobuf = { package = "pure-protobuf", git = "https://github.com/mingley/pure-protobuf" }
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

extern crate self as protobuf;

#[doc(hidden)]
pub use paste as __paste;

pub use crate::dynamic::{
    Cardinality, DescriptorPool, DynamicMessage, DynamicMessageMut, DynamicMessageView,
    EnumDescriptor, FieldDescriptor, FieldType, MapKeyValue, MessageDescriptor, MethodDescriptor,
    Presence, ServiceDescriptor, Value, RECURSION_LIMIT,
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
pub use crate::repeated::{Repeated, RepeatedIter, RepeatedMut, RepeatedView, Singular};
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
        let mut __m = <$ty as ::core::default::Default>::default();
        $crate::proto!(@fields __m, $($body)*);
        __m
    }};
    (@fields $m:ident, ) => {};
    (@fields $m:ident, $field:ident : $subty:ident { $($sub:tt)* } $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $m.[<set_ $field>]($crate::proto!($subty { $($sub)* }));
        }
        $crate::proto!(@fields $m, $($($rest)*)?);
    };
    (@fields $m:ident, $field:ident : $val:expr $(, $($rest:tt)*)?) => {
        $crate::__paste::paste! {
            $m.[<set_ $field>]($val);
        }
        $crate::proto!(@fields $m, $($($rest)*)?);
    };
}
