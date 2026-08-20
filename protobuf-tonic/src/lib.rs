//! tonic `Codec` over **pure-protobuf** (`Parse` / `Serialize`).
//!
//! Not `tonic-prost`. These types cannot implement `prost::Message`.
//! The kernel crate does not depend on tonic.

use bytes::{Buf, BufMut};
use protobuf::{ClearAndParse, Parse, Serialize};
use std::marker::PhantomData;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::Status;

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

#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufEncoder<T>(PhantomData<fn() -> T>);

impl<T: Serialize> Encoder for ProtobufEncoder<T> {
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        let bytes = Serialize::serialize(&item).map_err(|e| Status::internal(e.to_string()))?;
        dst.put_slice(&bytes);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufDecoder<T>(PhantomData<fn() -> T>);

impl<T: Parse + Default + ClearAndParse> Decoder for ProtobufDecoder<T> {
    type Item = T;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let mut buf = vec![0u8; src.remaining()];
        src.copy_to_slice(&mut buf);
        Parse::parse(&buf)
            .map(Some)
            .map_err(|e| Status::internal(e.to_string()))
    }
}

pub mod hello;
