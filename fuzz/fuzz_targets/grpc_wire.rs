#![no_main]

use bytes::{BufMut, Bytes, BytesMut};
use libfuzzer_sys::fuzz_target;
use pbrs_grpc::codec::{self, Frame, HEADER_LEN};
use pbrs_grpc::gzip;
use pbrs_grpc::timeout;
use pbrs_grpc::{Code, MessageLimits, Metadata, Status};

const MAX_FUZZ_INPUT_BYTES: usize = 64 * 1024; // 64 KiB
const MAX_FRAMES_PER_INPUT: usize = 64;

pub fn fuzz_grpc_wire(data: &[u8]) {
    let data = if data.len() > MAX_FUZZ_INPUT_BYTES {
        &data[..MAX_FUZZ_INPUT_BYTES]
    } else {
        data
    };

    // 1. Raw wire frame decoding with bounded limits
    let frame_limit_bytes = 16 * 1024;
    let limits = MessageLimits::unlimited().with_max_decoding(frame_limit_bytes);

    let mut full_buf = BytesMut::from(data);
    let mut full_frames = Vec::new();
    while let Ok(Some(frame)) = codec::pop_limited(&mut full_buf, limits) {
        assert!(
            frame.payload.len() <= frame_limit_bytes,
            "popped frame payload cannot exceed decode limit"
        );

        if frame.compressed {
            // Test bounded decompression on any frame claiming compressed
            match gzip::decode_limited(&frame.payload, limits) {
                Ok(inflated) => {
                    assert!(
                        inflated.len() <= frame_limit_bytes,
                        "decompressed payload cannot exceed decode limit"
                    );
                }
                Err(status) => {
                    assert!(
                        matches!(status.code(), Code::ResourceExhausted | Code::Internal),
                        "decompression must fail cleanly with ResourceExhausted or Internal"
                    );
                }
            }
        }

        full_frames.push(frame);
        if full_frames.len() >= MAX_FRAMES_PER_INPUT {
            break;
        }
    }

    // 2. Chunk fragmentation & coalescing invariance (replay path)
    // Synthesize 1 to 4 message payloads from data to verify fragmentation invariance
    let n_messages = ((data.len() % 4) + 1).min(data.len().max(1));
    let chunk_size = (data.len() / n_messages).max(1);
    let mut expected_frames: Vec<(bool, Bytes)> = Vec::new();
    let mut wire_stream = BytesMut::new();

    for i in 0..n_messages {
        let start = (i * chunk_size).min(data.len());
        let end = if i == n_messages - 1 {
            data.len()
        } else {
            ((i + 1) * chunk_size).min(data.len())
        };
        let raw_payload = &data[start..end];
        let use_gzip = (i % 2 == 1) && !raw_payload.is_empty();

        let (compressed, payload_bytes) = if use_gzip {
            match gzip::encode(raw_payload) {
                Ok(gz) => (true, Bytes::from(gz)),
                Err(_) => (false, Bytes::copy_from_slice(raw_payload)),
            }
        } else {
            (false, Bytes::copy_from_slice(raw_payload))
        };

        if let Ok(encoded_frame) = codec::encode(&payload_bytes, compressed) {
            expected_frames.push((compressed, payload_bytes));
            wire_stream.extend_from_slice(&encoded_frame);
        }
    }

    if !expected_frames.is_empty() {
        let wire_bytes = wire_stream.freeze();

        // Strategy A: byte-by-byte fragmentation
        let mut byte_buf = BytesMut::new();
        let mut byte_frames: Vec<Frame> = Vec::new();
        for &b in wire_bytes.as_ref() {
            byte_buf.put_u8(b);
            while let Ok(Some(frame)) =
                codec::pop_limited(&mut byte_buf, MessageLimits::unlimited())
            {
                byte_frames.push(frame);
            }
        }
        assert_eq!(
            byte_frames.len(),
            expected_frames.len(),
            "byte-by-byte chunking must recover all frames"
        );
        for (got, (expected_comp, expected_payload)) in byte_frames.iter().zip(&expected_frames) {
            assert_eq!(got.compressed, *expected_comp);
            assert_eq!(&got.payload, expected_payload);
        }

        // Strategy B: arbitrary chunk fragmentation (variable chunk sizes 1..=17)
        let mut chunk_buf = BytesMut::new();
        let mut chunk_frames: Vec<Frame> = Vec::new();
        let mut offset = 0;
        let mut step = 1;
        while offset < wire_bytes.len() {
            let take = step.min(wire_bytes.len() - offset);
            chunk_buf.extend_from_slice(&wire_bytes[offset..offset + take]);
            offset += take;
            step = (step % 17) + 1;

            while let Ok(Some(frame)) =
                codec::pop_limited(&mut chunk_buf, MessageLimits::unlimited())
            {
                chunk_frames.push(frame);
            }
        }
        assert_eq!(
            chunk_frames.len(),
            expected_frames.len(),
            "arbitrary chunking must recover all frames"
        );
        for (got, (expected_comp, expected_payload)) in chunk_frames.iter().zip(&expected_frames) {
            assert_eq!(got.compressed, *expected_comp);
            assert_eq!(&got.payload, expected_payload);
        }
    }

    // 3. Decompression expansion & compression bomb bounded budget
    // Direct decompression on arbitrary fuzz input
    let bomb_limit = MessageLimits::unlimited().with_max_decoding(4096);
    match gzip::decode_limited(data, bomb_limit) {
        Ok(inflated) => {
            assert!(
                inflated.len() <= 4096,
                "inflated data cannot exceed budget limit"
            );
        }
        Err(status) => {
            assert!(
                matches!(status.code(), Code::ResourceExhausted | Code::Internal),
                "gzip decompression failure must be ResourceExhausted or Internal"
            );
        }
    }

    // Synthetic compression bomb test: 256 KiB of zeros
    let bomb_raw = vec![0u8; 256 * 1024];
    if let Ok(bomb_gz) = gzip::encode(&bomb_raw) {
        let strict_limit = MessageLimits::unlimited().with_max_decoding(1024);
        let start = std::time::Instant::now();
        let res = gzip::decode_limited(&bomb_gz, strict_limit);
        let elapsed = start.elapsed();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code(), Code::ResourceExhausted);
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "compression bomb must be rejected within time budget"
        );
    }

    // 4. Invalid flags and lengths
    if data.len() >= HEADER_LEN {
        // Corrupt compressed-flag to invalid values (> 1)
        let mut bad_flag_buf = BytesMut::from(data);
        bad_flag_buf[0] = 0x02 | (data[0] % 254 + 2);
        let res = codec::pop_limited(&mut bad_flag_buf, MessageLimits::unlimited());
        assert!(res.is_err(), "invalid compressed-flag must be rejected");

        // Declared length exceeding limit
        let mut oversize_buf = BytesMut::from(data);
        oversize_buf[0] = 0;
        oversize_buf[1..5].copy_from_slice(&(1024u32 * 1024).to_be_bytes());
        let small_limit = MessageLimits::unlimited().with_max_decoding(1024);
        let res = codec::pop_limited(&mut oversize_buf, small_limit);
        assert!(res.is_err(), "oversize message length must be rejected");

        // Declared length u32::MAX
        let mut max_len_buf = BytesMut::from(data);
        max_len_buf[0] = 0;
        max_len_buf[1..5].copy_from_slice(&u32::MAX.to_be_bytes());
        let res = codec::pop_limited(&mut max_len_buf, small_limit);
        assert!(res.is_err(), "u32::MAX message length must be rejected");
    }

    // 5. Metadata and status encoding/decoding boundaries
    if data.len() >= 4 {
        let split_pos = (data[0] as usize) % data.len();
        let (k_bytes, v_bytes) = data.split_at(split_pos);
        if let (Ok(key_str), Ok(val_str)) =
            (std::str::from_utf8(k_bytes), std::str::from_utf8(v_bytes))
        {
            let mut md = Metadata::new();
            match md.insert(key_str, val_str) {
                Ok(()) => {
                    assert!(!key_str.ends_with("-bin"));
                    assert_eq!(md.get(key_str), Some(val_str));
                }
                Err(status) => {
                    assert_eq!(status.code(), Code::InvalidArgument);
                }
            }

            match md.insert_bin(key_str, v_bytes) {
                Ok(()) => {
                    assert!(key_str.ends_with("-bin"));
                    assert_eq!(md.get_bin(key_str).as_deref(), Some(v_bytes));
                }
                Err(status) => {
                    assert_eq!(status.code(), Code::InvalidArgument);
                }
            }

            md.remove(key_str);
            md.remove_bin(key_str);
        }
    }

    // Reserved metadata keys must always be rejected
    let mut md = Metadata::new();
    for reserved in [
        ":status",
        ":path",
        "grpc-status",
        "grpc-message",
        "content-type",
        "te",
        "connection",
        "host",
    ] {
        assert!(md.insert(reserved, "val").is_err());
        assert!(md.set(reserved, "val").is_err());
        assert!(md.insert_bin(reserved, b"val").is_err());
        assert!(md.set_bin(reserved, b"val").is_err());
    }
    for reserved_bin in [":status-bin", "grpc-status-bin", "grpc-status-details-bin"] {
        assert!(md.insert_bin(reserved_bin, b"val").is_err());
        assert!(md.set_bin(reserved_bin, b"val").is_err());
    }

    // Status details with arbitrary bytes
    let code = Code::from_i32(i32::from_le_bytes([
        data.get(0).copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0),
        data.get(2).copied().unwrap_or(0),
        data.get(3).copied().unwrap_or(0),
    ]));
    let msg = String::from_utf8_lossy(data.get(4..20.min(data.len())).unwrap_or_default());
    let mut status = Status::new(code, msg);
    status.set_details(data.to_vec());
    let _ = status.rpc();

    // Timeout header parsing on arbitrary utf8 data
    if let Ok(s) = std::str::from_utf8(data) {
        if let Some(dur) = timeout::parse_timeout(s) {
            let encoded = timeout::encode_timeout(dur);
            let reparsed = timeout::parse_timeout(&encoded);
            assert!(reparsed.is_some(), "reparsed timeout must be valid");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_grpc_wire(data);
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoke_seeded_corpus() {
        let corpus: &[&[u8]] = &[
            b"",
            &[0x00, 0x00, 0x00, 0x00, 0x00], // empty frame
            &[0x00, 0x00, 0x00, 0x00, 0x03, b'f', b'o', b'o'], // single valid frame
            &[0x02, 0x00, 0x00, 0x00, 0x01, 0x00], // invalid flag 2
            &[0x00, 0xff, 0xff, 0xff, 0xff], // u32::MAX length
            &[0x00, 0x00, 0x00, 0x00, 0x05, 0x01, 0x02], // truncated frame
            b"grpc-timeout: 100m",
            b":status: 200",
            b"content-type: application/grpc",
        ];
        for input in corpus {
            fuzz_grpc_wire(input);
        }
    }

    #[test]
    fn test_fragmentation_invariance() {
        let payload1 = b"hello, gRPC world!";
        let payload2 = b"pure-protobuf deterministic streaming";
        let gz1 = gzip::encode(payload1).expect("encode gz");

        let f1 = codec::encode(&gz1, true).expect("f1");
        let f2 = codec::encode(payload2, false).expect("f2");

        let mut stream = BytesMut::new();
        stream.extend_from_slice(&f1);
        stream.extend_from_slice(&f2);
        let wire = stream.freeze();

        // 1-byte chunking
        let mut byte_buf = BytesMut::new();
        let mut frames = Vec::new();
        for &b in wire.as_ref() {
            byte_buf.put_u8(b);
            while let Ok(Some(frame)) =
                codec::pop_limited(&mut byte_buf, MessageLimits::unlimited())
            {
                frames.push(frame);
            }
        }
        assert_eq!(frames.len(), 2);
        assert!(frames[0].compressed);
        assert_eq!(
            gzip::decode_limited(&frames[0].payload, MessageLimits::unlimited()).expect("inflate"),
            payload1
        );
        assert!(!frames[1].compressed);
        assert_eq!(&frames[1].payload[..], payload2);
    }

    #[test]
    fn test_compression_bomb_bounded_time_and_memory() {
        let bomb_raw = vec![0u8; 1024 * 1024]; // 1 MiB zeros
        let bomb_gz = gzip::encode(&bomb_raw).expect("encode");
        let limits = MessageLimits::unlimited().with_max_decoding(1024);

        let start = std::time::Instant::now();
        let res = gzip::decode_limited(&bomb_gz, limits);
        let elapsed = start.elapsed();

        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code(), Code::ResourceExhausted);
        assert!(elapsed < std::time::Duration::from_millis(10));
    }

    #[test]
    fn test_invalid_flags_and_lengths() {
        let mut buf = BytesMut::from(&[0x07, 0x00, 0x00, 0x00, 0x01, 0x00][..]);
        assert!(codec::pop_limited(&mut buf, MessageLimits::unlimited()).is_err());

        let mut oversize = BytesMut::from(&[0x00, 0x00, 0x10, 0x00, 0x00][..]); // 1 MiB
        let limits = MessageLimits::unlimited().with_max_decoding(1024);
        assert_eq!(
            codec::pop_limited(&mut oversize, limits)
                .unwrap_err()
                .code(),
            Code::ResourceExhausted
        );
    }

    #[test]
    fn test_bad_metadata_encodings() {
        let mut md = Metadata::new();
        assert!(md.insert("trace-bin", "ascii").is_err());
        assert!(md.insert_bin("trace", b"bytes").is_err());
        assert!(md.insert(":path", "/service").is_err());
        assert!(md.insert("grpc-status", "0").is_err());
        assert!(md.insert("invalid key with spaces", "val").is_err());
        assert!(md.insert("x-key", "invalid\nvalue").is_err());
    }
}
