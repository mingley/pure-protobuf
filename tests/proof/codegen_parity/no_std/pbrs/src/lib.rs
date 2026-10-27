#![no_std]

extern crate alloc;

/// Request the corresponding runtime API with every pbrs default feature off.
pub fn encode<M: pbrs::Serialize>(
    message: &M,
) -> Result<alloc::vec::Vec<u8>, pbrs::SerializeError> {
    message.serialize()
}
