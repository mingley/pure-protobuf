//! Generated-shaped TestAllTypes wrappers used by the conformance program.
//!
//! Same wrapper + DynamicMessage codec the plugin emits. Accessors for
//! TestAllTypes are produced when the plugin is invoked on the official
//! test_messages protos (see `tests/plugin.rs`).

use crate::dynamic::DescriptorPool;
use std::sync::{Arc, OnceLock};

pub fn conformance_pool() -> Arc<DescriptorPool> {
    static P: OnceLock<Arc<DescriptorPool>> = OnceLock::new();
    P.get_or_init(|| {
        const FDS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/conformance_fds.bin"));
        Arc::new(DescriptorPool::from_file_descriptor_set(FDS).unwrap_or_default())
    })
    .clone()
}

crate::impl_generated_message!(
    TestAllTypesProto3,
    TestAllTypesProto3View,
    TestAllTypesProto3Mut,
    "protobuf_test_messages.proto3.TestAllTypesProto3",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    TestAllTypesProto2,
    TestAllTypesProto2View,
    TestAllTypesProto2Mut,
    "protobuf_test_messages.proto2.TestAllTypesProto2",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    TestAllRequiredTypesProto2,
    TestAllRequiredTypesProto2View,
    TestAllRequiredTypesProto2Mut,
    "protobuf_test_messages.proto2.TestAllRequiredTypesProto2",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    TestAllTypesEdition2023,
    TestAllTypesEdition2023View,
    TestAllTypesEdition2023Mut,
    "protobuf_test_messages.editions.TestAllTypesEdition2023",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    TestAllTypesEditionUnstable,
    TestAllTypesEditionUnstableView,
    TestAllTypesEditionUnstableMut,
    "protobuf_test_messages.edition_unstable.TestAllTypesEditionUnstable",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    EditionsTestAllTypesProto2,
    EditionsTestAllTypesProto2View,
    EditionsTestAllTypesProto2Mut,
    "protobuf_test_messages.editions.proto2.TestAllTypesProto2",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    EditionsTestAllRequiredTypesProto2,
    EditionsTestAllRequiredTypesProto2View,
    EditionsTestAllRequiredTypesProto2Mut,
    "protobuf_test_messages.editions.proto2.TestAllRequiredTypesProto2",
    crate::gencode::conformance_pool()
);
crate::impl_generated_message!(
    EditionsTestAllTypesProto3,
    EditionsTestAllTypesProto3View,
    EditionsTestAllTypesProto3Mut,
    "protobuf_test_messages.editions.proto3.TestAllTypesProto3",
    crate::gencode::conformance_pool()
);
