# pbrs-grpc entries for the public grpc_bench harness (SB-17)

This directory adds `pbrs-grpc` server entries to the public `grpc_bench`
harness. It is for maintainers running the external plaintext unary benchmark
matrix; from this directory, run `./run.sh` after `./sync.sh [checkout]`.
Bottom line: failures from peer entries are recorded and skipped, but a `pbrs`
cell failure exits nonzero.

The two server entries mirror tonic: `rust_pbrs_mt_bench` is multi-threaded
with workers from `GRPC_SERVER_CPUS`, and `rust_pbrs_st_bench` is
single-threaded. Both serve plaintext unary `SayHello`, echo the inner `Hello`
message, use the jemalloc allocator, build release with
`opt-level=3 codegen-units=1 lto=true`, and listen on `0.0.0.0:50051`.

## Layout

| Path | Purpose |
|---|---|
| `rust_pbrs_{mt,st}_bench/` | Harness entry dirs (`Dockerfile`, `Cargo.toml` + `Cargo.lock`, `build.rs`, `src/main.rs`). Path dependencies point at `pbrs-src/`, which `sync.sh` snapshots into the checkout. |
| `sync.sh [checkout]` | Clone/verify `LesnyRumcajs/grpc_bench@48b6b95`, copy the entries, and snapshot pure-protobuf HEAD to `pbrs-src/`. Default checkout is `target/grpc-bench`; `GRPC_BENCH_CHECKOUT` overrides it. |
| `run.sh` | Build entry images and run the matrix. It uses the same `ghz` methodology as harness `bench.sh`: plaintext unary, 50 connections, 1000 concurrency, 5s warmup + 20s measured, `ghz` 0.114.0. It adds a CPU-count axis, seeded shuffled entry order per repeat, per-cell reports, and a host record. |
| `<checkout>/results/<ts>-pbrs/` | Result directory with per-cell `<entry>.cpus<N>.rep<R>.report` (`ghz`) plus `.stats` (Docker CPU/memory), `bench.params`, `host.txt`, and `failures.log`. |

`run.sh` knobs: `GRPC_SCENARIOS`, `GRPC_CPUS` (default `1 2 4`),
`GRPC_ENTRIES` (default pbrs + tonic/Thruster/go/java/Vert.x/dotnet/C++ peers),
`GRPC_REPEATS` (default 1), `GRPC_SEED`, and `GRPC_SKIP_BUILD=1`.

On macOS ghz reaches the server via `host.docker.internal`
(`--network=host` cannot work on Docker Desktop); Linux keeps the
harness's `--network=host` + `127.0.0.1`.

Peer build/run failures are recorded and skipped; a pbrs cell
failure exits nonzero. No upstream PR is opened (operator action).

If the pinned (amd64) ghz client caps throughput on an arm64 host,
raise `GRPC_CLIENT_CPUS` until the client stops being the ceiling
and record the deviation; see `docs/evidence/grpc-bench.md`.
