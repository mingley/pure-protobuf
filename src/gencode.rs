//! Field-wise generated TestAllTypes + WKT used by the conformance program.

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

pub use crate::generated::test_messages_edition2023::TestAllTypesEdition2023;
pub use crate::generated::test_messages_edition_unstable::TestAllTypesEditionUnstable;
pub use crate::generated::test_messages_proto2::{TestAllRequiredTypesProto2, TestAllTypesProto2};
pub use crate::generated::test_messages_proto2_editions::{
    TestAllRequiredTypesProto2 as EditionsTestAllRequiredTypesProto2,
    TestAllTypesProto2 as EditionsTestAllTypesProto2,
};
pub use crate::generated::test_messages_proto3::{NestedMessage, TestAllTypesProto3};
pub use crate::generated::test_messages_proto3_editions::TestAllTypesProto3 as EditionsTestAllTypesProto3;

pub use crate::generated::any::Any;
pub use crate::generated::duration::Duration;
pub use crate::generated::empty::Empty;
pub use crate::generated::field_mask::FieldMask;
pub use crate::generated::pb_struct::{ListValue, PbValue, Struct};
pub use crate::generated::timestamp::Timestamp;
pub use crate::generated::wrappers::{
    BoolValue, BytesValue, DoubleValue, FloatValue, Int32Value, Int64Value, StringValue,
    UInt32Value, UInt64Value,
};
