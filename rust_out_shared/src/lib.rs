//! Official `protoc --rust_out` (kernel=upb) linked as `protobuf` → pbrs.
//!
//! Runs `third_party/protobuf/rust/test/shared/*.rs` as-is. Skipped (docs/status.md):
//! ctype_cord_test, gtest_matchers_test, no_internal_access_test,
//! package_disambiguation_test, extensions_test (edition 2024).
//! Not compiled: proto! `#[cfg(bzl)]` qualified paths (no `--cfg=bzl`).
//! edition2023 `str_view` is ordinary string (upb; cpp VIEW skipped).

#![allow(dead_code, unused, nonstandard_style, unreachable_pub, unused_imports)]
#![allow(clippy::all)]

extern crate pbrs as protobuf;

#[allow(static_mut_refs, unused_mut)]
mod gencode {
    include!(concat!(env!("OUT_DIR"), "/mods.rs"));
}

pub use gencode::*;
