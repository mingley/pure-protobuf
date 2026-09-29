# SB-24: executable transport cells and claim preflight

Status: delivered implementation and local wiring evidence; SB-24 remains in progress.
The required Go/C++ comparisons, tonic TLS, effective-setting exports and dedicated-host
resource/headroom evidence are incomplete. These results establish no performance win.

## Delivered on 2026-09-29

- `run.py --scenario PATH` validates frozen parameters, cell ids, and every repeat
  permutation before launching processes. It executes all frozen repeats, records
  source SHA-256/effective parameters/order, and keeps separate repeat logs.
  `--cells ID,ID` filters without changing relative order; `--scenario-smoke` retains
  all repeats while using short diagnostic windows. Filtered runs cannot claim a
  complete matrix.
- `rpc-bench load --compression=identity|gzip` supports unary, upload, download,
  and lockstep bidi for native, tonic+pbrs and tonic+prost. `load-server` selects the
  same codec/compression explicitly, supports native verified TLS, and enforces its
  server timeout. Every load response must advertise the selected encoding; silent
  response fallback is a failed call. Payload sizes/counts remain validated.
- The stack runner now uses the existing prost load generator and the matched
  `rpc-bench` prost server. The older `scripts/rpc-bench-matrix.py` adapter remains
  separate and is identified as such in its peer manifest.
- Client cells receive discarded warmup, probes retain separate endpoint resources,
  and missing pinning/resource samples cannot certify reference headroom. Scheduling
  lag over 10% of e2e p50 invalidates a probe even when CPU sampling appears spare.
  Timeout accounting no longer adds timeouts a second time to failed calls.
- `--claim` fails before peer resolution/building: loopback, incomplete runtime
  observations, and unaligned endpoint CPU windows cannot qualify. The claim fairness
  verifier additionally requires complete frozen repeat coverage, explicit codecs,
  runtime setting evidence, distinct hosts, per-probe resources, and headroom.
- The 100k TCP connection point now rejects a single source/destination tuple even
  with unlimited descriptors. Its definition records an example feasible topology:
  four source addresses × 28,232 usable ports = 112,928 connections to one endpoint.
  Source binding/routing must still be implemented and demonstrated.

## Real transport smoke

```sh
PATH=/opt/homebrew/bin:$PATH CARGO_BUILD_JOBS=2 \
  cargo test --locked --manifest-path rpc-bench/Cargo.toml --test worker --bin rpc-bench --quiet
python3 bench/stack-matrix/transport_smoke.py \
  --binary rpc-bench/target/debug/rpc-bench --out-dir target/sb24-wire-smoke
```

Local macOS arm64, debug build, separate processes: **80/80 cells passed**.
The [retained report](sb24-wire-smoke.json) includes the executable SHA-256,
all directions/shapes, per-cell accounting, and captured message flags.
All 720 offered calls completed successfully with zero rejection, failure or timeout.

| Coverage | Cells | Verification |
|---|---:|---|
| Native/tonic+pbrs/tonic+prost, all nine directions, four shapes, identity/gzip | 72 | Bidirectional HTTP/2 DATA capture: all 648 request and 648 response messages per compression mode carried the expected gRPC compressed flag (0 identity, 1 gzip) |
| Native TLS, four shapes, identity/gzip | 8 | CA/server-name verification, decoded payload/count checks, and response encoding assertions inside TLS |

The capture proxy is a diagnostic observer and changes timing. TLS request DATA flags
are encrypted and were not observed by that proxy. The retained binary fingerprint
identifies the measured build; subsequent help/test-only edits do not rewrite it.
The smoke's 150 ms windows and zero-filled 1 KiB payloads are intentionally unsuitable
for efficiency, typical-entropy compression, or tail-latency claims.

## Frozen-order smoke

```sh
python3 bench/stack-matrix/run.py \
  --scenario bench/stack-matrix/scenarios/grpc-bench-echo.json --scenario-smoke \
  --cells server-native-unary-1kib-plain-1cpu,server-tonic-pbrs-unary-1kib-plain-1cpu \
  --out-dir target/sb24-frozen-smoke
```

Executed ten rows in all five frozen orders (native first in repeats 1–4, tonic first
in repeat 5). Warmups: 0.999–1.001 seconds, 5,200/5,200 calls successful. Measurements:
2.000–2.003 seconds, 10,480/10,480 calls successful, zero failed/rejected calls.
All ten rows were correctly **invalid for qualification** because scheduling lag
exceeded the generator threshold. Raw local logs remain under the command's output
directory; the source SHA retained there precedes the scenario's documentation-only
command update. The frozen workload/parameter/order definitions were unchanged.

Logic validation: 36 stack-matrix tests (one unavailable C++ test-data skip),
extended-cell self-test, connection-scale self-test, 89 binary and 93 worker tests,
plus the response-compression negative/CLI regression test passed.

## Remaining acceptance work

SB-24 is not complete. Tonic TLS is not yet implemented in this runner; a provider
configuration using the existing pure-Rust crypto graph is being assessed.
Go/C++ clients still use non-equivalent closed-loop interop soak paths;
official-peer response gzip configuration and runtime effective settings are not
verified. Pipelined bidi remains unsupported. Window, socket, message/stream limits,
TLS version/cipher and compression must come from actual peer observations before
equality can be certified. No dedicated-network headroom, aligned CPU windows,
100k topology, or new saturation-knee/overload campaign was run here.

The existing extended overload runner retains its measured-knee search and recovery
phases; fixed 24k QPS alone remains no evidence of overload. These local changes do
not close SB-22, RT-07, qualification, or independent architecture campaign gates.
