//! RX-11 isolated decoder diagnostic. Counts allocations like the main devloop
//! harness; no RPC, peer comparison, or leadership claim is inferred from it.

use std::hint::black_box;
use std::sync::atomic::Ordering;
use std::time::Instant;

use pbrs_grpc::{Codec, MessageLimits};

mod allocator;
use allocator::{ALLOCS, ARMED, BYTES, CountingAlloc};

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "usage: compression-devloop gzip|deflate empty|text|random|zeros SIZE ITERS WARMUP"
                .into(),
        );
    }
    let codec = match args[1].as_str() {
        "gzip" => Codec::Gzip,
        "deflate" => Codec::Deflate,
        _ => return Err("unknown codec".into()),
    };
    let size: usize = args[3].parse()?;
    let iters: u64 = args[4].parse()?;
    let warmup: u64 = args[5].parse()?;
    if iters == 0 || size > 4 * 1024 * 1024 {
        return Err("iterations must be positive and size <= 4 MiB".into());
    }
    let mut state = 0x1234_5678_u32;
    let text = b"protobuf payload: id=123 name=grpc repeated fields\n";
    let payload: Vec<u8> = (0..size)
        .map(|i| match args[2].as_str() {
            "text" => text[i % text.len()],
            "zeros" | "empty" => 0,
            "random" => {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            }
            _ => 0,
        })
        .collect();
    if !matches!(args[2].as_str(), "empty" | "text" | "random" | "zeros")
        || (args[2] == "empty" && size != 0)
    {
        return Err("invalid payload shape".into());
    }
    let compressed = codec.encode(&payload)?;
    let limits = MessageLimits::unlimited().with_max_decoding(size);
    // Validate work before timing; both implementations see identical bytes.
    assert_eq!(codec.decode_limited(&compressed, limits)?, payload);
    for _ in 0..warmup {
        black_box(codec.decode_limited(black_box(&compressed), limits)?);
    }
    ALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..iters {
        black_box(codec.decode_limited(black_box(&compressed), limits)?);
    }
    let elapsed = start.elapsed();
    ARMED.store(false, Ordering::Relaxed);
    let fingerprint = compressed.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    });
    println!(
        "{{\"codec\":\"{}\",\"shape\":\"{}\",\"payload_bytes\":{},\"compressed_bytes\":{},\"input_fingerprint\":\"{fingerprint:016x}\",\"iters\":{iters},\"allocs\":{},\"alloc_bytes\":{},\"wall_ns\":{}}}",
        codec.name(),
        args[2],
        size,
        compressed.len(),
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
        elapsed.as_nanos()
    );
    Ok(())
}
