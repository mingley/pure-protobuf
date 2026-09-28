//! Message memory layout (`src/runtime/layout.rs`).

use pbrs::runtime::{MiniTablePtr, PtrAndLen, StringView};

#[test]
fn empty_string_view_refers_to_nothing() {
    let view = StringView::empty();
    assert_eq!(unsafe { view.as_ref() }, b"");
}

#[test]
fn string_view_round_trips_bytes() {
    let view = StringView::from(b"abc".as_slice());
    assert_eq!(unsafe { view.as_ref() }, b"abc");
}

#[test]
fn dangling_table_and_alias_shape() {
    assert!(MiniTablePtr::dangling().0.is_null());
    assert_eq!(
        std::mem::size_of::<PtrAndLen>(),
        std::mem::size_of::<StringView>()
    );
}
