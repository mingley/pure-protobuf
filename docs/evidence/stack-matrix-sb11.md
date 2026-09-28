# Cross-stack client/server matrix harness (SB-11)

**Status: harness delivered; smoke wired; numbers diagnostic.** Loopback
cells on a shared macOS host (Apple Silicon, 14 CPUs) with no CPU
pinning prove the machinery end to end. Claim-grade numbers need the
primary stage on dedicated pinned Linux hosts per §8 of the
[benchmark contract](../benchmark-contract.md), which this harness
drives but cannot provide.

## What it does

`./scripts/stack-matrix.sh --stage <smoke|primary>`:

- Preflights every required peer binary (pbrs-grpc, tonic-pbrs,
  tonic-prost, grpc-go, grpc-c++), building the scripted ones
  (`--locked` cargo, `-mod=readonly` go) and failing the stage with a
  named peer when one cannot run — never `not_run`.
- Server cells: pins the server under test (taskset 1/2/4 CPUs on
  Linux; explicit unsupported record elsewhere), drives it with one
  independent pinned open-loop Poisson generator (`rpc-bench load`),
  warms up (discarded), then steps offered load geometrically to the
  sustained success QPS at a p99 SLO. A step is valid only with fully
  accounted calls, zero failures/timeouts/rejections, an unsaturated
  generator, and schedule-relative e2e p99 under the SLO.
- Client cells: runs each client under test against one fixed reference
  server at a matched offered rate and rates client CPU per RPC only
  with verified server headroom (< 80% of the pinned budget).
- Reference soak clients (go/cpp) cannot run the open-loop workload:
  their client cells delegate to the existing soak path and are labeled
  `interop-soak`, never SLO-rated or compared as equivalent.
- Matrix: payloads empty/1 KiB/64 KiB, shapes unary/server-stream/bidi
  ping-pong, TLS off and on. Primary adds the 2/4-CPU core-scaling axis
  on unary/1 KiB/plaintext.

## TLS

All cells verify; there is no skip-verify path. Reference servers use
their upstream test credentials; the native generator verifies with the
matching CA and override:

| Server | Server args | CA | Name |
|---|---|---|---|
| native | `--tls-cert/--tls-key` (in-tree `pbrs-grpc/tests/tls_data`) | same dir `ca.crt` | `localhost` |
| go | `-use_tls` (grpc-go module testdata) | module `testdata/ca.pem` | `foo.test.google.fr` |
| cpp | `--use_tls=true` (grpc `test_creds`) | `test_creds/ca.pem` | `foo.test.google.fr` |
| tonic | unsupported (tonic 0.14 TLS pulls a C crypto provider, vs QG-04) | — | — |

Go and C++ TLS were each probed live (native verified load, 13–15k
successes, zero failures); the smoke stage covers native TLS end to end.

## Smoke evidence (2026-09-27)

10/10 cells pass, exit 0 (`docs/evidence/stack-matrix-smoke-report.json`,
harness log `docs/evidence/stack-matrix-smoke.log`). Every server cell
honestly reports `reached max_rate 8000 without a ceiling`: smoke is a
wiring proof with a narrow search, not a server ceiling measurement.

## Layout

- `bench/stack-matrix/cells.py`: cell expansion + validity rules.
- `bench/stack-matrix/slo.py`: SLO search over a probe function.
- `bench/stack-matrix/pin.py`: taskset pinning + affinity readback.
- `bench/stack-matrix/peertls.py`: per-peer TLS material resolution.
- `bench/stack-matrix/run.py`: stage orchestration + `report.json`.
- `rpc-bench/peers/*.json`: `tls` sections mirror the above.
- `tests/interop/test_stack_matrix.py`: 28 unit tests.
- `rpc-bench/tests/load_shapes.rs`: live `load`/server flag + TLS cells.

`rpc-bench load` gained `--shape/--req-bytes/--resp-bytes/--stream-msgs/
--transport/--tls-ca/--tls-server-name`; `rpc-bench server` gained
`--tls-cert/--tls-key` (native only) and now serves TestService +
BenchmarkService on one endpoint; `RpcMetrics` JSON carries e2e/service
latency distributions for the SLO input.
