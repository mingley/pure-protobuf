//! `gzip` coding: re-exported from [`crate::compression`].
//!
//! Kept as its own module so existing `pbrs_grpc::gzip::{encode,
//! decode, ...}` paths keep working after RX-06 moved the registry into
//! [`crate::compression`].

pub use crate::compression::{decode, decode_limited, encode, encode_level};
