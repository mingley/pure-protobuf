#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

pub type MiniTableExtensionPtr = *const ();

pub type ExtensionRegistryPtr = *const ();
