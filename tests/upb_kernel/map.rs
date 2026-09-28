//! Map kernel paths (`src/runtime/map.rs`).

use pbrs::runtime::{Arena, InnerMapMut, empty_map};

#[test]
fn empty_map_has_no_entries() {
    let view = empty_map::<pbrs::ProtoString, i32>();
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
}

#[test]
fn inner_map_mut_carries_raw_and_arena() {
    let arena = Arena::new();
    let inner = InnerMapMut::new(std::ptr::null(), &arena);
    assert!(inner.raw.is_null());
}
