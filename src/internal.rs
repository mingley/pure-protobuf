//! Encapsulation break for generated/typed messages. Not for application code.

use std::fmt::Debug;

pub trait SealedInternal: Sized {}

#[derive(Clone, Copy, Debug)]
pub struct Private;

pub trait EntityType {
    type Tag;
}

pub mod entity_tag {
    pub struct MessageTag;
    pub struct EnumTag;
    pub struct PrimitiveTag;
    pub struct ViewProxyTag;
    pub struct MutProxyTag;
    pub struct RepeatedTag;
}

impl EntityType for f32 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for f64 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for i32 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for u32 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for i64 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for u64 {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for bool {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for crate::string::ProtoBytes {
    type Tag = entity_tag::PrimitiveTag;
}
impl EntityType for crate::string::ProtoString {
    type Tag = entity_tag::PrimitiveTag;
}

pub trait MatcherEq: SealedInternal + Debug {
    fn matches(&self, o: &Self) -> bool;
}

/// Official rust_out emits `unsafe impl ::protobuf::__internal::Enum`.
///
/// # Safety
/// Implement only for generated protobuf enums. `is_known` must match the
/// schema; closed enums must reject unnamed values.
pub unsafe trait Enum: Into<i32> + Copy + SealedInternal + 'static {
    const NAME: &'static str;
    fn is_known(value: i32) -> bool;
}

/// Official rust_out calls this with `"4.35.1-release"`. This kernel is not
/// crates.io `protobuf` 4.x; accept any gencode version.
pub const fn assert_compatible_gencode_version(_gencode_version: &'static str) {}

pub(crate) const MAX_MESSAGE_BYTES: u64 = (1 << 31) - 1;
