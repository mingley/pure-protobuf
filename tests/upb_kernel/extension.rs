//! Extension registry ABI (`src/runtime/extension.rs`).

use pbrs::runtime::{ExtensionRegistryPtr, MiniTableExtensionPtr};

#[test]
fn extension_pointers_are_thin() {
    assert_eq!(
        std::mem::size_of::<MiniTableExtensionPtr>(),
        std::mem::size_of::<*const ()>()
    );
    assert_eq!(
        std::mem::size_of::<ExtensionRegistryPtr>(),
        std::mem::size_of::<*const ()>()
    );
}
