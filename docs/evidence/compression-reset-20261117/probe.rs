//! Isolated component screen for compression pool reuse.
use pbrs_grpc::{Codec, MessageLimits};
use std::hint::black_box;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let case = args.get(1).expect("case");
    let n: usize = args.get(2).expect("iterations").parse().expect("integer");
    let codec = if case.starts_with("gzip") { Codec::Gzip } else { Codec::Deflate };
    let length = if case.contains("empty") { 0 } else if case.contains("64k") { 65536 } else if case.contains("1m") { 1048576 } else { 1024 };
    let mut state = 0x4f1b_7c15_5d93_ae81_u64;
    let payload = (0..length).map(|_| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        if case.contains("zero") { 0 } else { state.to_le_bytes()[0] }
    }).collect::<Vec<_>>();
    let encoded = codec.encode(&payload).expect("encode preflight");
    let limits = MessageLimits::new().with_max_decoding(length);
    assert_eq!(codec.decode_limited(&encoded, limits).expect("decode preflight"), payload);
    for _ in 0..16 {
        assert_eq!(codec.decode_limited(&encoded, limits).expect("warm"), payload);
    }
    let mut sum = 0_usize;
    for _ in 0..n {
        let output = if case.contains("roundtrip") {
            let message = codec.encode(black_box(&payload)).expect("encode");
            codec.decode_limited(black_box(&message), limits).expect("decode")
        } else if case.contains("encode-control") {
            codec.encode(black_box(&payload)).expect("encode control")
        } else {
            codec.decode_limited(black_box(&encoded), limits).expect("decode")
        };
        let expected_len = if case.contains("encode-control") { encoded.len() } else { payload.len() };
        assert_eq!(output.len(), expected_len);
        sum = sum.wrapping_add(black_box(output.len()));
        black_box(output);
    }
    println!("{{\"iterations\":{n},\"payload_len\":{},\"encoded_len\":{},\"sum\":{sum}}}", payload.len(), encoded.len());
}
