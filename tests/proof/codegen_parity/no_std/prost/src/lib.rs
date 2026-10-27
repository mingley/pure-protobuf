#![no_std]

extern crate alloc;

/// Type-check the runtime's ordinary serialization API on an alloc-only target.
pub fn encode<M: prost::Message>(message: &M) -> alloc::vec::Vec<u8> {
    message.encode_to_vec()
}
