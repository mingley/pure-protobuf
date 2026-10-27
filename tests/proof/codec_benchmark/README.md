# Compare owned codec work

This small diagnostic compares generated pbrs messages with Prost 0.14.4 on
one schema. Both codecs parse the same borrowed byte slice into an owned
result and drop that result inside the measured loop.

The default uses lazy nested fields. Add `--features owned-messages` when
building to select the public `Config::owned_message_fields(true)` profile.
For the instrumented build, use `--features owned-messages,allocations`.
Keep separate binary copies for each profile; the JSON records its selection.
The owned profile constructs children during parsing and keeps smaller size
caches. The lazy profile can reuse original nested wire bytes when encoding.

`parse_owned` measures parsing alone. pbrs can defer nested materialization.
`read_all` also visits every populated field, nested descendant and payload
byte. Read those rows separately. Neither operation measures RPC performance.

Build two binaries so allocation counters do not affect CPU measurements:

```sh
mkdir -p work/codec-proof
cargo build --locked --release --manifest-path tests/proof/codec_benchmark/Cargo.toml
cp target/release/pbrs-codec-proof work/codec-proof/shipping
cargo build --locked --release --features allocations --manifest-path tests/proof/codec_benchmark/Cargo.toml
cp target/release/pbrs-codec-proof work/codec-proof/allocations
python3 tests/proof/codec_benchmark/measure.py \
  --shipping work/codec-proof/shipping \
  --allocations work/codec-proof/allocations \
  --output work/codec-proof/results
```

If `CARGO_TARGET_DIR` is set, copy from that directory instead of `target`.
`protoc` and a stable Rust toolchain must be on `PATH`. Keep the source fixed
while building both binaries, and save its revision and file hashes with the
results.

The driver saves the randomized plan before running five paired processes
per case and operation. Each process records three samples. Both backends
must pass semantic round trips and the same byte-exhaustive checksum. A
fixed map-entry wire order gives every process the same input fingerprint.
These checks cover the eight fixtures; they are not a compatibility proof.

CPU time uses Linux `CLOCK_PROCESS_CPUTIME_ID`; the driver pins all probes to
one permitted CPU. The separate allocation build counts successful Rust
allocation and reallocation requests and requested bytes. It does not measure
RSS, peak memory, retained memory, instructions or kernel copies.

The summary retains every paired ratio, allocation result and percentile
bootstrap interval. Ratios above one mean pbrs used more process CPU. A run
on a shared host remains a diagnostic even if its interval excludes one.
World rankings require broader peers, workloads and controlled hosts.
