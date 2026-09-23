//! Prelude for the Protobuf Rust API.
//!
//! Same shape as Google protobuf v4: `proto!` plus the common message traits.

pub use crate::{
    AsMut as _, AsView as _, Clear as _, ClearAndParse as _, CopyFrom as _, IntoMut as _,
    IntoView as _, MergeFrom as _, Message as _, Parse as _, ProtoPut as _, Serialize as _,
    TakeFrom as _, proto,
};
