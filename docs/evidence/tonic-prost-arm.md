# TC-24 tonic+prost comparison arm (2026-09-29)

**Status: arm delivered; diagnostic numbers, mixed result.** `rpc-bench
--transport=tonic` grew a `--codec=pbrs|prost` flag (default `pbrs`) plus a
`prost-server` subcommand, so tonic+pbrs and tonic+prost run from the same
harness with matched settings. Five large-payload Bytes cells were recorded
against the SB-13 tonic+pbrs baseline. No threshold was applied: each cell
below reports both arms win-or-loss as measured.

## Premise check: stack-matrix does not cover this

`bench/stack-matrix/` has `tonic-pbrs` and `tonic-prost` server peers, but
three gaps keep it from answering TC-24:

1. **No large-message cells.** `cells.py::PAYLOADS` tops out at 64 KiB
   (`empty`/`1kib`/`64kib`); the SB-13 large-payload cells are 64 KiB /
   1 MiB / 4 MiB / 8 MiB (`docs/evidence/large-payload-baseline/rpc.json`).
2. **No recorded comparison.** The only checked-in matrix result is the
   smoke report (`docs/evidence/stack-matrix-smoke-report.json`): 10/10
   cells at `empty` payload, declared a wiring proof, not numbers. No
   primary-stage run exists.
3. **Asymmetric harness.** `tonic-prost` there is server-only
   (`cells.py::NO_GENERATOR`): a separate `tonic-interop` binary driven by
   the native generator. TC-24 requires both arms from the same harness
   with matched settings, which is what was built here.

## What was built

- `rpc-bench/build.rs`: `tonic_prost_build::configure()` compiles
  `proto/grpc/testing/test.proto` (same file as the pbrs
  `emit_tonic_stubs` call above it) into `OUT_DIR/grpc.testing.rs`:
  prost messages plus generated `test_service_client` /
  `test_service_server` stubs. Requires `protoc` at build time, like the
  existing bench crates (`tonic-bench`, `bench/devloop`,
  `tests/interop/tonic`).
- `rpc-bench/Cargo.toml` (+ lock): `prost = "0.14"`,
  `tonic-prost = "0.14"`, build-dep `tonic-prost-build = "0.14"`.
  Lock resolves to prost/prost-build 0.14.4, tonic-prost/
  tonic-prost-build 0.14.6 — the same pins as
  `tests/interop/tonic/Cargo.lock`. Bench-only (`publish = false`); no
  shipping manifest touched (QG-04).
- `rpc-bench/src/main.rs`:
  - `prost_gen` module alongside `tonic_gen`;
  - `LoadCodec` (`--codec=pbrs|prost`, default `pbrs`); `--codec=prost`
    requires `--transport=tonic` (native is pbrs-only) and is rejected
    otherwise;
  - `ProstInterop` + `serve_prost_tonic`: prost-codec TestService
    mirroring `process::TonicInterop` method for method, served through
    the same `process::fair_tonic_server()` + `NodelayIncoming` +
    `--max-message-size` path as the pbrs arm;
  - `run_load_prost`: prost-codec twin of `run_load_tonic` for all four
    load shapes; channel construction is shared
    (`process::tonic_channel` / `fair_tonic_endpoint`), so transport
    settings match exactly and only the codec differs;
  - `prost-server` subcommand (separate process, `READY port=...`
    line, `--port/--timeout-secs/--max-message-size` mirroring `server`);
    `load --codec=prost` without `--server_addr` loops back against an
    in-process prost server, while every other combination keeps the
    existing native loopback (existing `load_shapes` behavior unchanged).
- Tests (in `main.rs`, in scope): `--codec` parse/validation tests, an
  in-process prost unary roundtrip (0/1 KiB/64 KiB/1 MiB + 8x1 MiB
  client-streaming aggregate), and a prost-client-vs-pbrs-native-server
  wire-compat check proving both arms measure the same RPC.

## Results (same session, alternating arms)

Method: SB-13 settings — separate server process per arm, closed loop,
`--max-message-size 16777216` both sides, 8 s unary / 10 s
client_stream, release binary. Host: Apple M4 Pro, 14 CPUs, macOS 26.7,
rustc 1.98.1 (shared host: diagnostic, not claim-grade). Arms alternated
per cell. Every run: 0 failures, 0 timeouts, 0 queue overflows. `MB/s`
counts req+resp payload bytes x QPS, as in SB-13.

| cell | SB-13 tonic+pbrs (QPS / p50 / p99 ms) | tonic+pbrs fresh | tonic+prost fresh | per-cell |
|---|---|---|---|---|
| unary 64 KiB | 9453 / 0.42 / 0.52 | 3464 / 0.91 / 6.06 (454 MB/s) | 3323 / 1.02 / 5.36 (436 MB/s) | pbrs +4% QPS |
| unary 1 MiB | 1053 / 3.78 / 4.29 | 793 / 4.20 / 10.59 (1662 MB/s) | 934 / 4.22 / 5.38 (1958 MB/s) | **prost +18% QPS** (see repeat) |
| unary 4 MiB | 202 / 19.06 / 25.98 | 172 / 22.48 / 29.87 (1443 MB/s) | 157 / 26.26 / 33.08 (1316 MB/s) | pbrs +10% QPS |
| unary 8 MiB | 98 / 41.10 / 47.21 | 79.8 / 49.11 / 64.71 (1339 MB/s) | 66.8 / 51.10 / 111.39 (1121 MB/s) | pbrs +19% QPS |
| client_stream 8x1 MiB | 225 / 17.64 / 22.36 | 160.6 / 24.77 / 35.49 (1347 MB/s up) | 118.8 / 32.92 / 50.40 (996 MB/s up) | pbrs +35% QPS |

The 1 MiB prost win did **not** reproduce stably: two further alternating
pairs on the same host gave pbrs 418 / prost 480 QPS, then pbrs 791 /
prost 670 QPS. Run-to-run swing (~2x) dominates the arm delta there, so
the honest verdict is *mixed / no stable winner at 1 MiB on this host*,
pbrs ahead in the other four single-run cells.

## Reproduce

```sh
cargo build --locked --manifest-path rpc-bench/Cargo.toml --release
B=rpc-bench/target/release/rpc-bench
# arm A (pbrs): $B server --transport=tonic --port=0 --max-message-size 16777216
# arm B (prost): $B prost-server --port=0 --max-message-size 16777216
# then, per arm (P = READY port, N in 65536 1048576 4194304 8388608):
$B load --server_addr=127.0.0.1:P --transport=tonic --codec=<pbrs|prost> \
  --shape=unary --req-bytes=N --resp-bytes=N --duration-secs=8 \
  --max-message-size=16777216
# client_stream: --shape=client_stream --stream-msgs=8 --req-bytes=1048576 \
#   --duration-secs=10
```

## Limitations

- Single shared macOS host; p99s show scheduling noise (e.g. 6 ms p99 at
  64 KiB). Claim-grade numbers need pinned Linux hosts per the benchmark
  contract, as with SB-13 itself.
- Fresh absolute QPS is well below SB-13's (e.g. 3464 vs 9453 at 64 KiB)
  — different host load, not an arm regression: the comparison that
  matters is same-session pbrs-vs-prost, which is what the table reports.
- One run per cell except 1 MiB unary (three paired runs). No statistics
  beyond that; no threshold was applied to any cell.
- `prost-server` serves TestService only (no BenchmarkService, no TLS —
  same tonic limits as `server --transport=tonic`).
- Raw per-run JSONs are scratch (`/tmp/tc24-sweep/`); the table above is
  the recorded evidence.
