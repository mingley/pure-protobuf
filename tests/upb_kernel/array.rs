//! Repeated-field kernel paths (`src/runtime/array.rs`).

use pbrs::runtime::{Arena, InnerRepeatedMut, empty_array};

#[test]
fn empty_array_has_no_items() {
    let view = empty_array::<i32>();
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
}

#[test]
fn inner_repeated_mut_carries_raw_and_arena() {
    let arena = Arena::new();
    let inner = InnerRepeatedMut::new(std::ptr::null(), &arena);
    assert!(inner.raw.is_null());
}
