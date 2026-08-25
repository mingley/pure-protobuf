//! gzip for gRPC Compressed-Flag / `grpc-encoding: gzip`.

use crate::status::Status;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// gzip-compress protobuf bytes.
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(payload)
        .map_err(|e| Status::internal(e.to_string()))?;
    enc.finish().map_err(|e| Status::internal(e.to_string()))
}

/// gzip-decompress a Compressed-Flag payload.
pub fn decode(payload: &[u8]) -> Result<Vec<u8>, Status> {
    let mut dec = GzDecoder::new(payload);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(out)
}
