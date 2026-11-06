//! Exercise the shipping alloc-only cell on ordinary native and Miri CI.

extern crate alloc;

/// The runtime implementation, compiled directly without changing pbrs features.
#[expect(
    unsafe_code,
    reason = "directly exercise the reviewed shipping cell without another unsafe test implementation"
)]
#[path = "../src/alloc_cell.rs"]
pub mod alloc_cell;

#[path = "fixtures/alloc_cell_phase1.rs"]
mod phase1;
