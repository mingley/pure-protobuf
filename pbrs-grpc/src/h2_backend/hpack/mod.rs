mod decoder;
mod encoder;
pub(crate) mod header;
pub(crate) mod huffman;
mod table;

// The registry archive excludes HPACK fixtures. This external-data/fuzz
// suite stays in the provenance snapshot and is not qualified here.
// Ordinary self-contained upstream unit tests remain enabled.
#[cfg(any())]
mod test;

pub use self::decoder::{Decoder, DecoderError, NeedMore};
pub use self::encoder::Encoder;
pub use self::header::{BytesStr, Header};
