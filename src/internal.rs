//! Encapsulation break for generated/typed messages. Not for application code.

pub trait SealedInternal {}

#[derive(Clone, Copy)]
pub struct Private;

pub(crate) const MAX_MESSAGE_BYTES: u64 = (1 << 31) - 1;
