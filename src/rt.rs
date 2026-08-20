//! Runtime helpers for generated code. Not a Google upb kernel.

pub use crate::error::{ParseError, SerializeError};
pub use crate::wire::{
    capture_unknown, check_size, decode_tag, decode_varint, decode_zigzag32, decode_zigzag64,
    encode_len_field, encode_tag, encode_varint, encode_zigzag32, encode_zigzag64,
    key_len_value_len, read_fixed32, read_fixed64, read_len_bytes, skip_field, tag_len, varint_len,
    UnknownFields, WIRE_I32, WIRE_I64, WIRE_LEN, WIRE_SGROUP, WIRE_VARINT,
};
