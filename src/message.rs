use crate::error::{ParseError, SerializeError};
use crate::internal::SealedInternal;
use crate::proxied::{AsMut, AsView, IntoMut, IntoView, MutProxied};
use std::fmt::Debug;

/// Operations related to constructing a message.
pub trait Parse: SealedInternal + Sized {
    fn parse(serialized: &[u8]) -> Result<Self, ParseError>;
    fn parse_dont_enforce_required(serialized: &[u8]) -> Result<Self, ParseError>;
}

impl<T> Parse for T
where
    T: Default + ClearAndParse,
{
    #[inline(always)]
    fn parse(serialized: &[u8]) -> Result<Self, ParseError> {
        if serialized.is_empty() && T::EMPTY_PARSE_OK {
            return Ok(Self::default());
        }
        let mut msg = Self::default();
        ClearAndParse::merge_from_bytes(&mut msg, serialized).map(|()| msg)
    }

    #[inline]
    fn parse_dont_enforce_required(serialized: &[u8]) -> Result<Self, ParseError> {
        if serialized.is_empty() {
            return Ok(Self::default());
        }
        let mut msg = Self::default();
        ClearAndParse::merge_from_bytes_dont_enforce_required(&mut msg, serialized).map(|()| msg)
    }
}

/// Operations related to reading a message.
pub trait Serialize: SealedInternal {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError>;
    fn serialized_len(&self) -> usize;
    /// Write the binary payload into `out` without a per-call `Vec`.
    ///
    /// Default implementation serializes to a temporary `Vec` then copies.
    /// Generated messages override this to call `write_to`.
    fn encode(&self, out: &mut impl crate::wire::WireOut) -> Result<(), SerializeError> {
        let bytes = self.serialize()?;
        out.put_slice(&bytes);
        Ok(())
    }
}

pub trait Clear: SealedInternal {
    fn clear(&mut self);
}

pub trait ClearAndParse: SealedInternal {
    const EMPTY_PARSE_OK: bool = false;
    fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), ParseError>;
    fn clear_and_parse_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError>;
    /// Merge `data` into an empty/default message. Used by [`Parse`] to avoid
    /// a redundant `clear()` after `Default`.
    fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), ParseError>;
    fn merge_from_bytes_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.merge_from_bytes(data)
    }
}

pub trait CopyFrom: AsView + SealedInternal {
    fn copy_from(&mut self, src: impl AsView<Proxied = Self::Proxied>);
}

pub trait TakeFrom: AsView + SealedInternal {
    fn take_from(&mut self, src: impl AsMut<MutProxied = Self::Proxied>);
}

pub trait MergeFrom: AsView + SealedInternal {
    fn merge_from(&mut self, src: impl AsView<Proxied = Self::Proxied>);
}

/// The protobuf full name of a generated message (`package.Message`).
///
/// Used to pack [`google.protobuf.Any`](https://protobuf.dev/programming-guides/proto3/#any)
/// without the caller repeating the type URL. Every generated message
/// implements this; `FULL_NAME` is the same associated constant the
/// generated `impl` block already exposes.
pub trait MessageName {
    /// `package.Message` as written in the `.proto`, with no leading dot.
    const FULL_NAME: &'static str;
}

/// Marker implemented only by message types.
pub trait MessageType {}

impl<T> MessageType for T where
    T: crate::internal::EntityType<Tag = crate::internal::entity_tag::MessageTag>
{
}

/// A trait that all owned message types implement.
pub trait Message:
    SealedInternal
    + MessageType
    + MutProxied
    + for<'a> MutProxied<View<'a> = Self::MessageView<'a>, Mut<'a> = Self::MessageMut<'a>>
    + Parse
    + Default
    + Debug
    + Serialize
    + Clear
    + ClearAndParse
    + CopyFrom
    + MergeFrom
    + Send
    + Sync
    + Clone
{
    type MessageView<'msg>: MessageView<'msg, Message = Self>;
    type MessageMut<'msg>: MessageMut<'msg, Message = Self>;

    fn new() -> Self {
        Self::default()
    }
}

/// A trait that all message views implement.
pub trait MessageView<'msg>:
    SealedInternal
    + AsView<Proxied = Self::Message>
    + IntoView<'msg, Proxied = Self::Message>
    + Debug
    + Serialize
    + Send
    + Sync
    + Copy
    + Clone
    + Default
{
    type Message: Message;
}

/// A trait that all message muts implement.
pub trait MessageMut<'msg>:
    SealedInternal
    + AsView<Proxied = Self::Message>
    + IntoView<'msg, Proxied = Self::Message>
    + AsMut<MutProxied = Self::Message>
    + IntoMut<'msg, MutProxied = Self::Message>
    + Debug
    + Serialize
    + Clear
    + ClearAndParse
    + CopyFrom
    + MergeFrom
    + Send
    + Sync
{
    type Message: Message;
}

/// Implemented by generated enum types.
pub trait Enum: Into<i32> + Copy + SealedInternal + 'static {
    const NAME: &'static str;
    fn is_known(value: i32) -> bool;
}

impl<T: crate::internal::Enum> Enum for T {
    const NAME: &'static str = <T as crate::internal::Enum>::NAME;
    fn is_known(value: i32) -> bool {
        <T as crate::internal::Enum>::is_known(value)
    }
}

/// An integer value wasn't known for an enum while converting.
#[derive(Clone, PartialEq, Eq)]
pub struct UnknownEnumValue<T>(i32, std::marker::PhantomData<T>);

impl<T> UnknownEnumValue<T> {
    pub fn new(_private: crate::internal::Private, unknown_value: i32) -> Self {
        Self(unknown_value, std::marker::PhantomData)
    }

    pub fn value(self) -> i32 {
        self.0
    }
}

impl<T> std::fmt::Debug for UnknownEnumValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UnknownEnumValue").field(&self.0).finish()
    }
}

impl<T: Enum> std::fmt::Display for UnknownEnumValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not a known value for {}", self.0, T::NAME)
    }
}

impl<T: Enum> std::error::Error for UnknownEnumValue<T> {}

/// Message equality which may have false-negatives in the face of unknown fields.
pub fn message_eq<T: Serialize>(a: &T, b: &T) -> bool {
    match (a.serialize(), b.serialize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}
