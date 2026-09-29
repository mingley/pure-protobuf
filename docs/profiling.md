# Profiling kit (SB-14)

Use a profile to explain a measured bottleneck before changing the code.
The script writes timing, allocation, sampled-symbol, and provenance artifacts
under `target/profile/`. These are local diagnostics; the
[benchmark contract](benchmark-contract.md) defines comparative claim evidence.

One command profiles any dev-loop cell, codec or RPC, on Linux or macOS with
whatever sampler is installed:

```sh
./scripts/profile.sh --cell rpc.pbrs.unary
```

After the run, open `meta.json` to check the sampler and completeness,
`cell.json` for per-operation costs, and `top.txt` to see where samples landed.
Compare the same cell on the base and changed revisions with matching build
flags and workload sizes. Keep losses and missing metrics in the report.

The [dev-loop guide](../bench/devloop/README.md) lists the cell families;
the built harness's `devloop list` command is the current inventory.

## Linux dev-loop from macOS

On a contended macOS host, `scripts/devloop.sh` can report exact allocations
but not retired instructions. When Docker Desktop is available, run the same
dev-loop inside an arm64 Linux container:

```sh
./scripts/devloop-linux.sh --cells codec.pbrs.owned_decode --out target/devloop/linux.json
```

The `instructions` metric is the measured-loop instruction count per
operation, not whole-process startup cost. On Linux the parent runs each cell
under `perf` or callgrind at N and 2N measured-loop iterations with identical
preparation, then reports `(instructions_2N - instructions_N) / N`. The
`instruction_method` field in the JSON records the method, such as
`differential_callgrind_2n_minus_n`. Older JSON without that field should be
treated as legacy overhead-inclusive data.

The wrapper builds a small image from pinned `rust:1.98-bookworm` with
`valgrind`, `cmake`, `git`, `g++`, and `protoc`. It mounts the checkout at
`/work`, caches Cargo registry/git data in the `pbrs-devloop-linux-cargo`
Docker volume, and caches `bench/devloop/target` in
`pbrs-devloop-linux-devloop-target`; the pinned Linux `libprotoc 35.1` build is
cached under `/cargo/pinned-protoc-build` in the Cargo volume. The repo's
pinned protobuf source and compiler build are prepared before
`scripts/devloop.sh` runs, so the v4/upb peer uses gencode compatible with
`protobuf = 4.35.1-release` without writing a Linux executable into the host
`target/` directory.

All arguments are forwarded to `scripts/devloop.sh`. Output paths inside the
repository, such as `target/devloop/*.json`, remain on the host checkout. The
container runs the dev-loop as the invoking uid/gid after fixing cache-volume
ownership. Override the image or volumes with
`PBRS_DEVLOOP_LINUX_IMAGE`, `PBRS_DEVLOOP_LINUX_BASE`,
`PBRS_DEVLOOP_LINUX_CARGO_VOLUME`, or `PBRS_DEVLOOP_LINUX_TARGET_VOLUME`.

Output lands in `target/profile/<cell>-<stamp>/`:

| File | Contents |
|---|---|
| `cell.json` | `run-cell` JSON: wall time plus exact heap allocs/bytes from the counting allocator |
| `top.txt` | top-40 sampled symbols with shares (demangled when `rustfilt`/`c++filt` exists) |
| `folded.txt` | folded stacks for `flamegraph.pl`, when the tool emits stacks |
| `profile.*` | raw capture: `profile.samply.json`, `perf.data` + `perf.script`, `sample.txt`, or `dtrace.out` |
| `meta.json` | cell, iters, tool + version, base SHA, file list |

## Tool selection

`--tool auto` (default) picks the first available sampler:

| Tool | Host | Notes |
|---|---|---|
| `samply` | anywhere | `cargo install samply`; view with `samply load profile.samply.json` |
| `perf` | Linux | `perf record -F 997 -g`; needs `perf` + unwind info |
| `sample` | macOS | ships with macOS; no privileges needed for own processes |
| `dtrace` | anywhere | last resort; needs root |

An explicitly requested but missing tool fails and names the install. A partial
capture is labeled in `meta.json`; it is never presented as complete.

## Workload sizing

Unless `--iters` is pinned, the script times a 2000-iteration probe and scales
to ~10 s of sampling, clamped to 10k–20M iterations. That lets the sampler see
steady state instead of startup. `--warmup` (default 1000) matches `run-cell`
semantics. The probe costs one extra run. Pass `--skip-build` to reuse the
current release binary.

## Allocation profiling

Every dev-loop cell runs under a counting `GlobalAlloc`
(`bench/devloop/src/main.rs`). `cell.json` reports exact `allocs` and
`alloc_bytes` for the timed phase only. Setup, teardown, and JSON printing
happen outside the window.

No separate allocator profiler is needed to answer "how many allocations per
op." Use the sampler to answer "where does time go."

## perf-PR workflow

Worker protocol rule 3 requires this evidence on every PK, GN, CL, SV, H2, and
RX optimization PR (template: `.github/pull_request_template.md`):

1. On the base SHA: `devloop run` JSON for the target cells plus
   `./scripts/profile.sh --cell <id>` for the hottest cell.
2. State one hypothesis naming the target categories.
3. Make one coherent change; capture the after-JSON and `compare` it.
4. Attach before/after JSON paths, the `top.txt` delta or flamegraph
   link, the correctness checks run, and the rollback criterion.

Label every number `dev-loop` or `claim-grade` per the
[benchmark contract](benchmark-contract.md); never write "fastest"
without a named matrix.
