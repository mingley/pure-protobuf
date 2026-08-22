//! INVENTORY ONLY. Official `protoc --rust_out` (protobuf v35.1, kernel=upb)
//! compiled against `pbrs` remapped as the `protobuf` crate.
//!
//! Not a compatibility shim. Not compiled by `cargo test --workspace`.
//!
//! Generate (official GitHub `protoc-35.1-linux-x86_64.zip`, not protoc-gen-pbrs):
//! ```text
//! protoc --rust_out=DIR --rust_opt=experimental-codegen=enabled,kernel=upb \
//!   -I proto proto/person.proto
//! ```

include!("generated.rs");
