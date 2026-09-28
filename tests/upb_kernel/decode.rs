//! Kernel parse paths (`src/runtime/decode.rs`).
//!
//! Placeholder: decode entry points take kernel-owned message pointers,
//! so decode tests arrive with the UK cards that expose parseable
//! fixtures. They live here, not in neighbouring areas.

#[test]
fn decode_area_is_wired() {
    // `src/runtime/decode.rs` owns parse_into and the ClearAndParse /
    // MergeFrom impls; UK-04..UK-07 extend this file.
}
