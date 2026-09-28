//! Upb-kernel check: one submodule per `src/runtime/` area.
//!
//! This is the `upb-kernel` check (`cargo test -p pbrs --test upb_kernel`).
//! UK cards add their kernel tests to the matching file so areas stay
//! disjoint, mirroring the `src/runtime/` split (MX-04).

#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "kernel integration tests drive unsafe entry points directly"
)]

#[path = "upb_kernel/arena.rs"]
mod arena;
#[path = "upb_kernel/array.rs"]
mod array;
#[path = "upb_kernel/decode.rs"]
mod decode;
#[path = "upb_kernel/encode.rs"]
mod encode;
#[path = "upb_kernel/extension.rs"]
mod extension;
#[path = "upb_kernel/layout.rs"]
mod layout;
#[path = "upb_kernel/map.rs"]
mod map;
#[path = "upb_kernel/mini_table.rs"]
mod mini_table;
#[path = "upb_kernel/reflect.rs"]
mod reflect;
