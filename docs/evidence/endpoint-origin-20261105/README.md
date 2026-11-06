# Endpoint origin cost

This capture checks the cost of adding `Endpoint::origin(Uri)` to native request routing. The recorded run compared locally committed `dc2261c8efe6fa99bf6c3583fa642f09ee7249cf` source with the same source plus ten native file changes. Both exports have identical measurement-only layout getters. The candidate changes no dependencies, lockfile, or shipping unsafe code.

For reproduction, [portable-baseline.json](portable-baseline.json) binds all 407 selected baseline Git inputs to the public `0ae8a43d3d08617c249c84a3f0d30f614494d70e` commit. Each selected file has the same SHA-256 in both commits and in the original capture. This does not assert that the entire commits match. Original capture records keep their `dc2261c8` baseline unchanged; the portable checker requires the public commit and verifies every selected file. Preparation reads `0ae8a43d` and records that reconstruction choice separately, so the unpublished local commit is unnecessary.

All 72 measured processes passed the request, response, route, and checksum checks. The paired CPU medians were slightly higher for the candidate. The spread includes both wins and losses, so this capture establishes neither a speed improvement nor zero overhead.

| Matched operation | Median paired CPU change | Individual pair range | Allocation calls per RPC, both versions |
| --- | ---: | ---: | ---: |
| Stock unary, no origin | +0.42% | −3.53% to +6.29% | 11 |
| Unary with a 5-second deadline, no origin | +0.38% | −2.03% to +5.43% | 25 |
| Unary with an equivalent HTTP origin | +0.16% | −2.33% to +12.54% | 11 |

The private `ChannelLocal` grows from 176 to 184 bytes on this x86_64 build. The public `Channel` handle remains 16 bytes. Across the separate allocation observations, requested allocation bytes per completed RPC fall by 8 bytes in each mode; allocation call counts are unchanged. These counters cover the combined client and server. They do not measure retained memory or RSS.

The CPU driver binary grows from 4,662,016 to 4,671,760 bytes. This is one linked diagnostic executable, not a measurement of every application using the crate.

## Work measured

One current-thread Tokio runtime runs a native client and echo server through a 64 KiB in-memory duplex stream. Each RPC sends and receives an 8-byte string: 10 protobuf payload bytes and a 5-byte gRPC frame prefix in each direction. The server checks method path, authority, and scheme, and the client checks every response. Connection setup, warmup, teardown, and reporting are outside the timed window.

Each mode has nine randomized baseline/candidate pairs, with 20,000 completed RPCs and 1,000 warmup RPCs per process. The collector pins each process to the same allowed CPU and records process CPU time and wall time. The table uses the median of within-pair candidate/baseline CPU ratios; it is not the ratio of two independently calculated medians. All pair values remain in [results.json](results.json).

The HTTP-origin comparison sends the same scheme and authority. The baseline uses its existing `Channel::origin("benchmark.local")`; the candidate uses `Endpoint::origin("http://benchmark.local/ignored?discard=1")`. The ignored path and query do not change the method path. Allocation observations use separate binaries, with three pairs of 128 measured and 128 warmup RPCs per mode. Their CPU times are excluded from the table.

This is a shared-host unary diagnostic with default runtime features, Rust 1.99.1, LLVM 23.1.1, release optimization, thin LTO, and one codegen unit. It does not measure network or TLS costs, streaming throughput, custom-scheme performance, tail latency, production capacity, or a tonic/Prost ranking. The public [endpoint origin tests](../../../pbrs-grpc/tests/endpoint_origin.rs) exercise those origin semantics separately; see the [transport guide](../../guides/transport-controls.md) for supported behavior.

## Check the capture

Run from the repository root:

```sh
python3 docs/evidence/endpoint-origin-20261105/check.py
```

The checker verifies artifact hashes, all 407 public Git baseline inputs, source overlays, four successful builds, twelve prechecks, all 72 recorded process results, layouts, and the reconstructed summaries. It compiles nothing and runs no new benchmark.

- [source-overlays.patch](source-overlays.patch) shows the ten native source changes. `source-overlays.tar.gz` contains only those files for reproduction.
- [source-manifest.json](source-manifest.json) binds the selected baseline Git inputs, candidate overlays, exported sources, and initial driver inputs by SHA-256. Both driver harnesses were identically refreshed before building; the build records bind the exact [main.rs](main.rs) compiled for this capture.
- [portable-baseline.json](portable-baseline.json) records selected-input equivalence with public `0ae8a43d`. [portable-replay-validation.json](portable-replay-validation.json) retains a successful default checker and source-only preparation in a fresh shallow Git containing `0ae8a43d` with the local `dc2261c8` object absent. The earlier failed source-only replay record remains unchanged in `replay-validation.json`.
- `raw.tar.gz` retains original collector scripts, source/build manifests, compiler output, every raw result, and the failed attempts. It contains no executable binaries or full source exports.
- [manifest.json](manifest.json) hashes the capsule files. These are local source and output bindings, not filesystem-read traces or hosted provenance attestations.

The first offline metadata attempt needed an uncached, unused `r-efi` target package. Metadata was then filtered to the actual x86_64 Linux target. A later candidate build hit the 600 MiB logical target-size guard and was stopped. Its exact size at the stop was not retained. After freezing baseline binaries outside the target, a supported `cargo clean -p pbrs -p pbrs-grpc --release` removed only the isolated package artifacts, and both candidate builds passed. The successful target ended at 445,078,810 logical bytes. The 1 GiB free-space floor and all failed-attempt records remain in the raw archive.

## Reproduce

The scripts require Python 3.11+, Linux x86_64 CPU affinity and process clocks, Git access to the public `0ae8a43d` commit, a Rust toolchain, `protoc`, and cached registry dependencies for offline builds. They honor `CARGO_HOME` and `RUSTUP_HOME`; this repository's local toolchain directories are used when present. Start with a new output directory:

```sh
python3 docs/evidence/endpoint-origin-20261105/prepare.py --out /tmp/pbrs-origin-replay
python3 docs/evidence/endpoint-origin-20261105/build.py --out /tmp/pbrs-origin-replay
# Stop other local builds and benchmarks before collecting timings.
python3 docs/evidence/endpoint-origin-20261105/measure.py --out /tmp/pbrs-origin-replay
```

The output stays under that directory. Source preparation checks the frozen input hashes. Builds use one job, offline locked resolution, a 600 MiB sampled logical target guard, and a 1 GiB free-space floor. The replay builder performs the supported package cleanup between variants proactively; the original guard failure remains in this capture. The portable scripts add the public baseline alias and change output-path and resource-cleanup plumbing; the exact scripts used for the recorded run are in `raw.tar.gz` under `original-scripts/`.

Inspect the new `measurements/summary.json` together with all raw pairs. A repeat can have different results on a shared host; do not promote its best pair to a performance claim.
