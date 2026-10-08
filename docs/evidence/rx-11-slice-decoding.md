# RX-11: decode compressed frames from slices

The gzip and deflate decoders now read the existing frame slice directly.
Previously, flate2's `read` wrapper allocated a 32 KiB input buffer and
copied compressed bytes into it. Using its `bufread` wrapper removes that
buffer. The decompressor, output reservation, message limits, and status
handling stay the same.

## Measurements

Dev-loop run on Linux x86_64, 2026-10-05. Baseline:
`452bfad7`. The candidate is the two decoder-import changes and comments
retained in `source/runtime.patch` inside the
[artifact archive](rx-11-slice-decoding/records.tar.gz).

The [diagnostic](../../bench/devloop/compression/README.md) measures decoding
only. Rust 1.88.0, flate2 1.1.10, miniz_oxide 0.9.1, and Valgrind 3.24.0
were fixed across both binaries. Each of 16 cells has three randomized paired
repeats with N/2N instruction accounting and matching input fingerprints.
The archive includes both measured binaries, source snapshots, lockfile,
raw child outputs, and full Callgrind graphs.

Every cell saves **one allocation and 32,768 requested bytes per decode**.
Instruction changes:

| Input | gzip | deflate |
|---|---:|---:|
| Empty | -21.17% | -21.18% |
| Text, 1 KiB | -20.39% | -20.38% |
| Random bytes, 1 KiB | -25.09% | -25.02% |
| Zeros, 1 KiB | -20.89% | -21.01% |
| Text, 64 KiB | -9.42% | -8.87% |
| Random bytes, 64 KiB | -5.86% | -5.79% |
| Zeros, 1 MiB | -0.88% | -0.81% |
| Random bytes, 1 MiB | -3.84% | -3.80% |

Allocation count falls 11–50% across these cells. The large zero-filled rows
have small instruction gains because decompression dominates their cost.
The baseline and candidate profiles retain the call graphs behind the result.

Callgrind wall time is secondary: the random 1 MiB deflate row rose 7.22%
despite fewer instructions. Compilation ran concurrently on this host.
Dedicated-host timing and RPC latency remain unmeasured.

## Compatibility checks

The buffered-wrapper oracle passed before and after the change. It compares
decoded bytes and status codes for every truncation and a bit mutation at
every byte of a valid stream, concatenated members, trailing garbage, and
five size-limit boundaries for both codecs.

- Native library/RPC/compression/hostile/message-size suites: 588 passed,
  one existing ignored test.
- TLS suite, run serially: 26 passed, including gzip on all four RPC shapes
  over TLS and mTLS.
- Mixed-tonic suites: 41 passed, including generated streaming services and
  gzip in both transport directions.
- Public documentation checks: 25 passed.
- Diagnostic allocator: one ordinary test and one Miri test passed
  (`nightly-2025-06-01`).
- Shipping dependency audit and plan checks passed.
- Workspace all-target/all-feature Clippy, diagnostic Clippy, and
  warning-strict workspace rustdoc passed.

The earlier documentation run failed on the old prose patterns and scanned
the local toolchain cache. The final run checks the shorter descriptions of
the same TLS behavior and excludes the scratch `work/` directory. Both logs
are retained.

Verify the archive and recompute each cell from raw observations:

```sh
python3 docs/evidence/rx-11-slice-decoding/check.py
```

RX-11 remains in progress. Per-side RPC costs, tonic performance comparisons,
Go/C++ gzip qualification, and the full transport matrix still need runs.
Rollback consists of restoring the two `flate2::read` imports.
