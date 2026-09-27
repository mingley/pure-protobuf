# Official QPS scenarios (SB-10) — diagnostic results

**Status: diagnostic, not claim-grade.** Mostly closed-loop scenarios,
5 s warmup / 30 s runs on a shared macOS host (Apple Silicon, 14 CPUs),
driven by the pinned upstream C++ `qps_json_driver`. SB-21 creates the
contract-compliant variants (dedicated hosts, open-loop, full latency
accounting). QPS numbers below prove interop and plausibility, not
leadership.

## Pins

| Component | Pin |
|---|---|
| C++ driver + worker (`qps_json_driver`, `qps_worker`) | grpc/grpc@d1487957 (v1.84.0), built from `third_party/grpc` |
| Go worker | google.golang.org/grpc@dd51b1c90aaf (v1.85.0-dev) |
| Native worker | in-tree `rpc-bench` at the recorded source SHA per run |

Per-run `summary.json` records driver/worker binary SHA-256 digests,
source pins, and the native source SHA + dirty-worktree marker.

## Thread controls

The 7 `protobuf_*` scenarios request `async_server_threads: 1` and
`async_client_threads: 1`. The native worker honors these as explicit
Tokio worker counts (dedicated 1-worker runtimes, echoed to the worker
log); previously they were rejected, so no official scenario could run.
`SYNC_CLIENT` / `SYNC_SERVER` stay explicitly unsupported: the two
`go_*_sync_*` scenarios fail with the native worker rejecting the setup
before bind/dial (`INVALID_ARGUMENT: unsupported client/server_type`),
which the C++ driver reports as a missing initial status (upstream
`driver.cc` treats that as fatal). Async-only operation is inherent to
the Tokio worker; SYNC support is not planned.

The C++ server worker requires grpc's helper port server
(`third_party/grpc/tools/run_tests/start_port_server.py`) to allocate
benchmark ports; without it the C++ worker never yields initial status.
The runs below started it first. Native and Go workers bind ephemeral
ports themselves and do not need it.

## Results (QPS per the C++ driver; p50/p99/CPU are N/A upstream)

2026-09-26, 5 s warmup / 30 s runs, warmup excluded. **45/45 async PASS**
(9 per run x 5 pairings). `FAIL*` = `go_*_sync_*`: expected, the native
worker rejects SYNC setup with INVALID_ARGUMENT before bind/dial (async
Tokio worker; SYNC not planned). The two `-cpp-*` columns fix the C++
peer to `--client-pinned-core=3 --server-pinned-core=4`; the rest pin
the C++ driver the same way.

| scenario | native-pair | cpp-n2c | cpp-c2n | go-n2c | go-c2n |
|---|---|---|---|---|---|
| pb.unary_ping_pong_empty | 16990.8 | 9909.7 | 11415.4 | 12811.1 | 12576.4 |
| pb.unary_ping_pong_1kb | 15092.6 | 10154.8 | 10703.6 | 12607.0 | 11135.0 |
| pb.unary_ping_pong_64kb | 6816.4 | 5347.9 | 6300.2 | 4561.3 | 3952.2 |
| pb.streaming_ping_pong_empty | 13343.2 | 9786.0 | 13942.7 | 11738.9 | 14982.4 |
| pb.streaming_ping_pong_1kb | 13236.8 | 9568.0 | 13728.5 | 11775.4 | 2979.4 |
| pb.unary_qps_unconstrained_100rpcs | 36729.9 | 30013.2 | 4711.1 | 7674.7 | 4423.0 |
| pb.unary_poisson_5000qps | 5012.9 | 5013.1 | 522.8 | 5013.2 | 11058.2 |
| cpp_pb.async_unary_ping_pong | 12790.0 | 11963.1 | 11977.8 | 2268.9 | 2179.9 |
| cpp_pb.async_streaming_ping_pong | 12832.1 | 10196.2 | 13945.5 | 4525.3 | 2753.3 |

Out-of-line cells (cpp-c2n poisson 522.8, go-c2n streaming_1kb 2979.4,
go-c2n poisson 11058.2 vs the 5000 offered load) are shared-host noise
under closed-loop driving, not regressions: per-scenario logs are
retained alongside the matrices for SB-21 follow-up.

## Retained artifacts

Per run directory (`docs/evidence/qps-sb10/<run>/`):

- `summary.json`: per-scenario status + QPS + pins/digests.
- `<scenario>.scenario.json`: the exact scenario definition the driver ran.
- `<scenario>-<direction>-driver-metrics.json`: C++ driver QPS output.
- `run<N>-harness.log`: per-run harness transcript with the results
  matrix and FAIL diagnostics.

Per-scenario worker logs (including the honored thread-control echo
lines) live under `target/qps-logs/sb10-*/` in the tree that produced
this commit and are not checked in; rerun the commands below to
reproduce them.

Reproduce (port server first, then one invocation per direction):

```sh
cd third_party/grpc && python3 tools/run_tests/start_port_server.py
./scripts/grpc-qps-interop.sh --driver target/interop-cpp/qps_json_driver --mode native_pair
./scripts/grpc-qps-interop.sh --driver target/interop-cpp/qps_json_driver --ref-peer cpp --mode native_client_to_ref_server
./scripts/grpc-qps-interop.sh --driver target/interop-cpp/qps_json_driver --ref-peer cpp --mode ref_client_to_native_server
./scripts/grpc-qps-interop.sh --driver target/interop-cpp/qps_json_driver --ref-peer go --mode native_client_to_ref_server
./scripts/grpc-qps-interop.sh --driver target/interop-cpp/qps_json_driver --ref-peer go --mode ref_client_to_native_server
```
