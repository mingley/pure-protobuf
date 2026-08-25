//! tonic `Codec` over **pbrs** (`Parse` / `Serialize`).
//!
//! Not `tonic-prost`. These types cannot implement `prost::Message`.
//! The kernel crate does not depend on tonic.

extern crate self as protobuf_tonic;

use bytes::Buf;
use pbrs::{ClearAndParse, Parse, Serialize};
use std::marker::PhantomData;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::Status;

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
        let n = src.remaining();
        let chunk = src.chunk();
        if chunk.len() >= n {
            let bytes = chunk
                .get(..n)
                .ok_or_else(|| Status::internal("short chunk"))?;
            let item = Parse::parse(bytes).map_err(|e| Status::internal(e.to_string()))?;
            src.advance(n);
            return Ok(Some(item));
        }
        let bytes = src.copy_to_bytes(n);
        Parse::parse(&bytes)
            .map(Some)
            .map_err(|e| Status::internal(e.to_string()))
    }
}

pub mod hello;
