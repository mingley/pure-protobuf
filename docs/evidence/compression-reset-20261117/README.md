# Compression decoder reset screen

Pooled decoders were reset when returned, when acquired, and again before the first gzip member. This change resets each cached decoder once, before return. New decoders start clean. Later gzip members still reset their history. The per-thread pool stays bounded at eight decoders per codec.

Source pair:

- Baseline: [`e0d6d092`](https://github.com/mingley/pure-protobuf/commit/e0d6d09230c4e53934be6aa8aa928612bb40733d).
- Candidate: [`2f27343e`](https://github.com/mingley/pure-protobuf/commit/2f27343e23770571c3ab40a15038c6dcb2ed5217).
- [Exact patch](compression-reset.patch), [compiler](rustc.txt), [machine](lscpu.txt), and [binary hashes](binary-sha256.txt).

## Results

These are whole-process component CPU measurements on a shared Ubuntu 24.04 runner. Each case has four alternating ABBA/BAAB blocks, two runs of each binary per block, calibrated iteration counts, identical checked outputs, and one allowed CPU selected with process affinity. A ratio below 1 means the candidate used less CPU.

| Decode case | Median candidate/baseline | Four-block range |
| --- | ---: | ---: |
| gzip, empty message | 0.700 | 0.672–0.704 |
| gzip, 1 KiB noise | 0.500 | 0.494–0.506 |
| zlib, empty message | 0.815 | 0.804–0.819 |
| zlib, 1 KiB noise | 0.673 | 0.666–0.727 |

All twelve cases and every sample are in [results.json](results.json); [summary.txt](summary.txt) includes the duplicate-executable controls. The four 64 KiB/1 MiB decode medians were 0.992–0.999. Roundtrip medians were 0.959 for gzip and 0.964 for zlib; their controls were noisy. The unchanged encode-only controls had medians 0.998 and 0.988.

Duplicate-executable ratios for the 1 KiB decode cases ranged from 0.989–1.026 for gzip and 0.840–1.022 for zlib. Across the whole screen, a 1 MiB zlib control reached 1.533. Large-frame and roundtrip improvements remain unproven. This screen establishes no allocation, RPC, tonic/prost ranking, or dedicated-host qualification result.

## Correctness

The [candidate gate](https://github.com/mingley/pure-protobuf/actions/runs/44034149687) passed 53 compression unit tests and four integration tests on Rust 1.85 and stable, 34 independent tonic gzip interoperability tests on stable, formatting, and strict gRPC Clippy across all targets and features. [Gate metadata](gate-jobs.json) records the exact source and steps.

New tests cover every prefix of a valid compressed message, checksum failures, output caps, empty messages, concatenated gzip members, partial reads, early drop, unwinding, pool saturation, borrowed-pool fallback, and alternating sizes/codecs. Removing reset-on-return makes both early-drop tests fail specifically on dirty decoder state; the [mutant patch](compression-dirty-state-mutant.patch) and [diagnostic](dirty-state-mutant.log) are retained.

## Reproduce

Run the evidence check:

```sh
python3 docs/evidence/compression-reset-20261117/check.py
```

For a new Linux screen, use the compiler in [rustc.txt](rustc.txt) and install `protoc`. Create separate worktrees at the two source SHAs above. Copy [probe.rs](probe.rs) into each worktree as `pbrs-grpc/examples/compression_reset_screen.rs`. Build each with a separate target directory:

```sh
CARGO_TARGET_DIR=/path/to/separate-target cargo build --locked --release -p pbrs-grpc --example compression_reset_screen
```

Pass the two resulting executables to the retained [driver](driver.py):

```sh
python3 docs/evidence/compression-reset-20261117/driver.py /path/to/baseline /path/to/candidate results.json
```

The archived [screen workflow](screen-workflow.yml) and [correctness workflow](gate-workflow.yml) retain the exact commands.

The original [screen run](https://github.com/mingley/pure-protobuf/actions/runs/44034241587), [run metadata](perf-jobs.json), [kernel](uname.txt), source IDs, harness, raw samples, and SHA-256 manifest are retained here. The library source was unchanged during each build; only the identical example was added.
