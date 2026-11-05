# Moving owned byte buffers

The runtime now takes a moved `Vec<u8>` when inserting a repeated bytes field
or map value. Singular bytes setters also take owned `Cow<[u8]>` and
`Box<[u8]>` buffers. These paths previously copied the payload. Borrowed
inputs still copy, and empty moved vectors release their capacity.

This record counts requested Rust allocations. It compares the former
conversion expressions with calls to the updated runtime in the same
instrumented program. It does not build a complete previous crate or measure
CPU time, throughput, retained memory or RPC performance.

## Results

Three processes produced the same 60 rows each. The inputs cover lengths
0, 1, 16, 4,096 and 1,048,576 bytes, exact and spare vector capacity, owned
and borrowed `Cow`, and boxed slices. Payload creation and boxing happen
before counting starts. Nonempty moved buffers preserve their payload
pointer, and aliases remain readable after dropping the original value.

For a 4,096-byte payload:

| Input | Former conversion calls / bytes | Updated conversion calls / bytes | Updated first clone calls / bytes |
|---|---:|---:|---:|
| Exact-capacity `Vec` or owned `Cow` | 1 / 4,096 | 0 / 0 | 1 / 24 |
| Spare-capacity `Vec` or owned `Cow` | 1 / 4,096 | 1 / 24 | 0 / 0 |
| `Box<[u8]>` | 1 / 4,096 | 0 / 0 | 1 / 24 |
| Borrowed `Cow` | 1 / 4,096 | 1 / 4,096 | 1 / 24 |

The former nonempty paths also request a 24-byte header on first clone.
`as_shared` is measured **after** that clone and requests no additional
allocation in these rows. Empty conversions, clones and sharing request no
allocation, including empty vectors with spare capacity.

Taking a vector retains its spare capacity. For 1- and 16-byte payloads with
spare capacity, the updated conversion requests a 24-byte header rather than
the former 1- or 16-byte payload allocation. Those conversion-byte rows
increase. The counters do not measure allocator overhead or retained RSS.
The [zero-copy guide](../../zero-copy.md) explains how to copy a short slice
when retaining a much larger allocation would be wasteful.

## Source and test scope

The source is `1c6ec8ca` plus the three retained runtime overlays in
`raw/overlay`: byte ownership, empty-vector normalization and the final
outlined map equality helper. The build uses `pbrs` without default features,
the development profile without debug information, Rust 1.99.1 and bytes 1.12.1
on Linux x86_64. No generation, conformance or RPC code runs in this profile.

The complete run retains identical before/after inventories for 1,311
selected source inputs. The compiler depfile identifies 32 bound inputs,
including the package manifest, lockfile, build script and lints. The checker
reconstructs these from Git and the retained overlays. This is compiler
source-dependency evidence, not a filesystem read trace for the build script
or toolchain. Compiler and library artifact hashes are recorded; executable
binaries and the toolchain are not included.

The collector rebuilt the runtime-only library for this capture. Its
before/after inventory covers the accepted runtime source, including the
final map equality helper.

The pointer-transfer regression test also ran on the former `LazyBytes`
conversion and failed at the expected pointer comparison. Its captured
source hash matches `raw/old-path-lazy.rs`. Other agents edited unrelated
inputs during that test, so its wrapper reports that the broad input
inventory changed. It is an old-path copying control, not a validation of
the full concurrent tree. Final source needs separate integration checks.

## Check and reproduce

Verify all 180 rows, raw hashes, compiler metadata, source overlays and the
old-path control. The checkout needs Git history containing `1c6ec8ca`:

```bash
python3 docs/evidence/byte-ownership-20261105/check.py
```

Run the retained allocation program against the current checkout from the
repository root:

```bash
mkdir -p work/byte-ownership-rerun
CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p pbrs --lib --no-default-features
rustc --edition=2024 docs/evidence/byte-ownership-20261105/raw/conversions.rs \
  --extern pbrs=target/debug/libpbrs.rlib -L dependency=target/debug/deps \
  -o work/byte-ownership-rerun/conversions
work/byte-ownership-rerun/conversions
```

If setting `CARGO_TARGET_DIR`, adjust the `--extern` and `-L` paths to that
directory. `raw/run.py` retains the original managed-workspace collector and
its commands. Results from a later checkout or toolchain have their own
source and allocation-layout scope.
