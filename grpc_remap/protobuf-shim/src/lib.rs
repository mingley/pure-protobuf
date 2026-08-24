//! Crate-name shim: grpc-protobuf `use protobuf::…` against pbrs.
//!
//! pbrs `[lib] name` is `pbrs`. Patching crates.io `protobuf` to the pbrs
//! package leaves rustc `--extern pbrs=…`, so this package is named
//! `protobuf` and re-exports the kernel.

extern crate pbrs;

pub use pbrs::proto;
pub use pbrs::*;
