# pbrs-grpc entries for the public grpc_bench harness (SB-17)

Two servers, mirroring the tonic entries: `rust_pbrs_mt_bench`
(multi-thread, workers = `GRPC_SERVER_CPUS`) and
`rust_pbrs_st_bench` (single-thread). Plaintext unary `SayHello`
echo of the inner `Hello` message, jemalloc allocator, release
`opt-level=3 codegen-units=1 lto=true`, serves `0.0.0.0:50051`.

## Layout

- `rust_pbrs_{mt,st}_bench/`: harness entry dirs (`Dockerfile`,
  `Cargo.toml` + `Cargo.lock`, `build.rs`, `src/main.rs`). Path deps
  point at `pbrs-src/`, a snapshot `sync.sh` drops into the checkout.
- `sync.sh [checkout]`: clone/verify
  `LesnyRumcajs/grpc_bench@48b6b95`, copy the entries, snapshot
  pure-protobuf HEAD to `pbrs-src/`. Default checkout
  `target/grpc-bench` (`GRPC_BENCH_CHECKOUT` overrides).
- `run.sh`: build entry images and run the matrix. Same ghz
  methodology as harness `bench.sh` (plaintext unary, 50 conns,
  1000 concurrency, 5s warmup + 20s measured, ghz 0.114.0), plus a
  CPU-count axis, seeded shuffled entry order per repeat, per-cell
  reports and a host record. Knobs: `GRPC_SCENARIOS`,
  `GRPC_CPUS` (default `1 2 4`), `GRPC_ENTRIES` (default pbrs +
  tonic/Thruster/go/java/Vert.x/dotnet/C++ peers), `GRPC_REPEATS`
  (default 1), `GRPC_SEED`, `GRPC_SKIP_BUILD=1`.
- Results land in `<checkout>/results/<ts>-pbrs/`: per-cell
  `<entry>.cpus<N>.rep<R>.report` (ghz) + `.stats` (docker
  CPU/mem), `bench.params`, `host.txt`, `failures.log`.

On macOS ghz reaches the server via `host.docker.internal`
(`--network=host` cannot work on Docker Desktop); Linux keeps the
harness's `--network=host` + `127.0.0.1`.

Peer build/run failures are recorded and skipped; a pbrs cell
failure exits nonzero. No upstream PR is opened (operator action).
