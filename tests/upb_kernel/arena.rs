//! Arena allocation and fusion (`src/runtime/arena.rs`).

use pbrs::runtime::Arena;

#[test]
fn new_and_default_allocate() {
    let _ = Arena::new();
    let _ = Arena::default();
}

#[test]
fn fuse_is_idempotent_and_self_safe() {
    let a = Arena::new();
    let b = Arena::new();
    a.fuse(&a);
    a.fuse(&b);
    b.fuse(&a);
}
