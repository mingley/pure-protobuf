//! A tonic `Codec` for pbrs messages using `Parse` and `Serialize`.
//!
//! Keeps tonic transport and middleware while using pbrs-generated messages.
//! Code that requires `prost::Message` needs an adapter.

extern crate self as protobuf_tonic;

use bytes::Buf;
use bytes::Bytes;
use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message as ProstMessage;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use tonic::Status;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// tonic [`Codec`] using pbrs [`Serialize`] / [`Parse`] (not `prost::Message`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufCodec<E, D> {
    _e: PhantomData<fn() -> E>,
    _d: PhantomData<fn() -> D>,
}

impl<E, D> Codec for ProtobufCodec<E, D>
where
    E: Serialize + Send + 'static,
    D: Parse + Default + ClearAndParse + Send + 'static,
{
    type Encode = E;
    type Decode = D;
    type Encoder = ProtobufEncoder<E>;
    type Decoder = ProtobufDecoder<D>;

    fn encoder(&mut self) -> Self::Encoder {
        ProtobufEncoder(PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        ProtobufDecoder(PhantomData)
    }
}

/// Encoder half of [`ProtobufCodec`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufEncoder<T>(PhantomData<fn() -> T>);

impl<T: Serialize> Encoder for ProtobufEncoder<T> {
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        Serialize::encode(&item, dst).map_err(|e| Status::internal(e.to_string()))
    }
}

/// Decoder half of [`ProtobufCodec`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufDecoder<T>(PhantomData<fn() -> T>);

impl<T: Parse + Default + ClearAndParse> Decoder for ProtobufDecoder<T> {
    type Item = T;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        // One copy into an owned buffer (tonic's), then shared parse with no
        // pbrs-side copy (PK-09): 2 copies before, 1 after, either branch.
        let n = src.remaining();
        let bytes = src.copy_to_bytes(n);
        Parse::parse_bytes(bytes)
            .map(Some)
            .map_err(|e| Status::internal(e.to_string()))
    }
}

pub mod hello;

/// Failure while converting between wire-compatible prost and pbrs messages.
///
/// Conversion deliberately goes through protobuf wire bytes. This keeps the
/// API schema-agnostic. The encoded buffer is moved into the decoder: neither
/// direction copies that buffer after encoding, and the prost-to-pbrs
/// direction retains it as pbrs' shared backing storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversionError(String);

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ConversionError {}

/// Convert a prost message to its wire-compatible pbrs representation.
///
/// This allocates exactly one wire buffer. [`Bytes::from`] takes ownership of
/// prost's `Vec`, and pbrs parses from that shared buffer without copying it.
pub fn prost_to_pbrs<P, R>(message: &P) -> Result<R, ConversionError>
where
    P: ProstMessage,
    R: Parse,
{
    let mut wire = Vec::with_capacity(message.encoded_len());
    message
        .encode(&mut wire)
        .map_err(|error| ConversionError(error.to_string()))?;
    R::parse_bytes(Bytes::from(wire)).map_err(|error| ConversionError(error.to_string()))
}

/// Convert a pbrs message to its wire-compatible prost representation.
///
/// This allocates one wire buffer for pbrs serialization. Prost consumes the
/// owned buffer directly through its [`bytes::Buf`] decoder interface.
pub fn pbrs_to_prost<P, R>(message: &P) -> Result<R, ConversionError>
where
    P: Serialize,
    R: ProstMessage + Default,
{
    let wire = message
        .serialize()
        .map_err(|error| ConversionError(error.to_string()))?;
    R::decode(Bytes::from(wire)).map_err(|error| ConversionError(error.to_string()))
}
