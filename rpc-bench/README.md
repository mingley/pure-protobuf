# `rpc-bench`: High-Throughput gRPC Benchmark Harness & QPS Worker

`rpc-bench` provides the high-performance benchmark harness and official gRPC `WorkerService` implementation for `pure-protobuf` (`pbrs`) and `pbrs-grpc`.

It strictly adheres to the binding principles in [`docs/benchmark-contract.md`](../docs/benchmark-contract.md):
- **Coordinated Omission Prevention**: Open-loop Poisson and constant-paced arrival schedules track scheduling lag; saturated generator runs are explicitly flagged.
- **Metric Separation**: Client and server CPU seconds, memory RSS, and queue delays are accounted and reported independently.
- **Statistical Fidelity**: Complete HDR latency histograms matching upstream `grpc/support/histogram.c` with exponential bucket distributions.
- **Zero Schema Adaptation**: Consumes and emits official `grpc.testing` protobuf structures over wire gRPC without artificial schema adapters hiding unsupported fields.

---

## 1. Pinned Upstream Specifications and Peers

All official scenarios, driver control protocols, and reference peers are pinned to reviewed upstream releases:

| Component | Upstream Repository / Artifact | Pinned Commit / Version | Description |
|---|---|---|---|
| **Official C++ Peer & Driver** | [`grpc/grpc`](https://github.com/grpc/grpc) | [`d1487957db6658bc532b72871775148229836627`](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627) (v1.84.0) | Official `qps_json_driver` and `qps_worker` |
| **Official Go Reference Peer** | [`google.golang.org/grpc`](https://github.com/grpc/grpc-go) | [`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`](https://github.com/grpc/grpc-go/tree/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef) (v1.85.0-dev) | Official Go benchmark worker (`google.golang.org/grpc/benchmark/worker`) |
| **Native Rust Worker** | `pbrs-rpc-bench` | in-tree (`pure-protobuf`) | Native `WorkerService` and `BenchmarkService` |
| **Official Scenario Definitions** | [`rpc-bench/scenarios/official.json`](scenarios/official.json) | official gRPC `Scenario` schema | Pinned official QPS benchmark scenarios |

---

## 2. Official QPS Benchmark Scenarios (`scenarios/official.json`)

The scenarios file [`rpc-bench/scenarios/official.json`](scenarios/official.json) defines official gRPC QPS scenarios conforming to `grpc.testing.Scenario` and `grpc.testing.Scenarios` from `grpc/testing/control.proto`:

| Scenario Identifier | RPC Type | Payload | Concurrency | Load Model | Description |
|---|---|---|---|---|---|
| `protobuf_unary_ping_pong_empty` | `UNARY` | 0 B req / 0 B resp | 1 channel / 1 RPC | Closed-loop | Baseline unary framing and scheduling overhead |
| `protobuf_unary_ping_pong_1kb` | `UNARY` | 1 KiB req / 1 KiB resp | 1 channel / 1 RPC | Closed-loop | Typical microservice payload round-trip |
| `protobuf_unary_ping_pong_64kb` | `UNARY` | 64 KiB req / 64 KiB resp | 1 channel / 1 RPC | Closed-loop | Medium payload buffer chunking and transport flow |
| `protobuf_streaming_ping_pong_empty` | `STREAMING` | 0 B req / 0 B resp | 1 channel / 1 stream | Closed-loop | Bidi streaming lockstep ping-pong |
| `protobuf_streaming_ping_pong_1kb` | `STREAMING` | 1 KiB req / 1 KiB resp | 1 channel / 1 stream | Closed-loop | Bidi streaming 1 KiB payload message stream |
| `protobuf_unary_qps_unconstrained_100rpcs` | `UNARY` | 1 KiB req / 1 KiB resp | 1 channel / 100 RPCs | Closed-loop | Saturated channel multiplexing and pipeline concurrency |
| `protobuf_unary_poisson_5000qps` | `UNARY` | 1 KiB req / 1 KiB resp | 1 channel / 64 RPCs | Poisson (5,000 QPS) | Open-loop offered load verifying steady-state tail latency |
| `cpp_protobuf_async_unary_ping_pong_insecure` | `UNARY` | 0 B req / 0 B resp | 1 channel / 1 RPC | Closed-loop | Upstream C++ alias for unary ping-pong |
| `cpp_protobuf_async_streaming_ping_pong_insecure` | `STREAMING` | 0 B req / 0 B resp | 1 channel / 1 stream | Closed-loop | Upstream C++ alias for streaming ping-pong |
| `go_protobuf_sync_unary_ping_pong_insecure` | `UNARY` | 0 B req / 0 B resp | 1 channel / 1 RPC | Closed-loop | Upstream Go alias for synchronous unary ping-pong |
| `go_protobuf_sync_streaming_ping_pong_insecure` | `STREAMING` | 0 B req / 0 B resp | 1 channel / 1 stream | Closed-loop | Upstream Go alias for synchronous streaming ping-pong |

---

## 3. Local Reproducible Invocation

The runner script [`scripts/grpc-qps-interop.sh`](../scripts/grpc-qps-interop.sh) orchestrates workers and driver runs with full process isolation and error recovery.

### Quick Start: Dry Run
Inspect the planned execution matrix, scenario configurations, and pinned dependencies without spawning processes:
```bash
./scripts/grpc-qps-interop.sh --dry-run
```

### Fast Smoke Test
Run an official scenario with short warmup and benchmark durations:
```bash
./scripts/grpc-qps-interop.sh --scenario=protobuf_unary_ping_pong_empty --warmup=1 --duration=2
```

### Mixed-Peer Direction Execution
Execute across specific peer directions to evaluate client vs. server efficiency:

```bash
# 1. Native pair (Native Client -> Native Server):
./scripts/grpc-qps-interop.sh --mode=native_pair --scenario=protobuf_unary_ping_pong_empty

# 2. Native client against Reference Go server:
./scripts/grpc-qps-interop.sh --mode=native_client_to_ref_server --scenario=protobuf_unary_ping_pong_empty

# 3. Reference Go client against Native server:
./scripts/grpc-qps-interop.sh --mode=ref_client_to_native_server --scenario=protobuf_unary_ping_pong_empty
```

### Full Matrix Execution
Run the entire suite of official scenarios across all three primary directions:
```bash
./scripts/grpc-qps-interop.sh
```

### Driving with Upstream C++ `qps_json_driver`
After `scripts/grpc-interop-cpp.sh` fetches and builds the pinned C++ interop
peer, build the upstream QPS target from that same checked-out commit:

```bash
cmake --build target/interop-cpp-build --parallel 4 --target qps_json_driver
./scripts/grpc-qps-interop.sh --driver=target/interop-cpp-build/qps_json_driver \
  --mode=all --ref-peer=go --scenario=protobuf_unary_ping_pong_empty --warmup=1 --duration=2
```

The upstream C++ driver accepts only its own `--scenarios_file` and
`--json_file_out` flags. The script writes a single-scenario copy with the
requested timing overrides, and fails if a selected scenario or peer fails.
That C++ version's `--json_file_out` contains **QPS only**, not a complete
`ScenarioResult`; its full reporter output is retained in `*-driver.log`.
The integrated Go driver instead emits the complete wire-compatible
`ScenarioResult` below and rejects unknown scenario options. Neither short
local smoke run establishes the paired, dedicated-host performance gate.

---

## 4. Architecture and Control Flow

```
                     +---------------------------------------+
                     |        Official QPS Driver            |
                     | (qps_json_driver / integrated driver) |
                     +---------------------------------------+
                           /                           \
         RunServer(ServerArgs)                       RunClient(ClientArgs)
         ServerStatus                                ClientStatus
         Mark(reset=true/false)                      Mark(reset=true/false)
                         /                               \
                        v                                 v
         +-----------------------------+   gRPC traffic   +-----------------------------+
         |     Server Worker           | <==============> |     Client Worker           |
         | (rpc-bench worker / go/cpp) |                  | (rpc-bench worker / go/cpp) |
         |   spawns BenchmarkService   |                  |    spawns LoadGenerator     |
         +-----------------------------+                  +-----------------------------+
```

### Key Components

- **`src/worker_server.rs`**: Implements `WorkerService.RunServer` and `CoreCount`. Handles `ServerConfig` setup on ephemeral ports, spawns an isolated benchmark server with graceful shutdown triggers, captures resource snapshots via `resources.rs`, and reports CPU/elapsed deltas on `Mark` signals.
- **`src/worker_client.rs`**: Implements `WorkerService.RunClient`. Connects to target servers, configures `LoadGenerator` under closed-loop or Poisson distributions, tracks latencies in exponential `Histogram` buckets, records status code distributions, and responds with `ClientStatus` on `Mark` requests.
- **`src/benchmark_service.rs`**: Implements `grpc.testing.BenchmarkService` procedures (`UnaryCall`, `StreamingCall`, `StreamingFromClient`, `StreamingFromServer`, `StreamingBothWays`).
- **`src/load.rs`**: High-performance load engine supporting bounded in-flight queuing, open-loop Poisson arrival intervals, and scheduling lag measurements.
- **`src/resources.rs`**: Cross-platform process resource inspection using native OS APIs (`libproc` on macOS, `/proc` on Linux) to record user/system CPU seconds and RSS.

---

## 5. Artifacts and Local Proof

Every benchmark run produces verifiable, immutable evidence in `target/qps-logs/<timestamp>_<pid>/`:

1. **`summary.json`**: Tabular benchmark execution results containing scenario name,
   peer direction, status and the metrics the selected driver actually exports.
   It records the driver's binary path, SHA-256, kind and pinned source revision,
   plus the native worker binary digest, source SHA and dirty-worktree marker.
   Mixed-peer runs also record the reference worker binary digest and pinned
   source; a cached Go worker with the wrong embedded grpc-go module is refused.
   Upstream C++ exports QPS only; its p50/p99 and CPU table cells are `N/A`,
   never inferred from QPS.
2. **`*-result.json`** (integrated Go driver only): Complete raw `grpc.testing.ScenarioResult` protobuf messages serialized to JSON via standard `protojson`. Contains:
   - Full HDR `latencies` histogram data (`bucket`, `min_seen`, `max_seen`, `sum`, `sum_of_squares`, `count`).
   - Server and client `ServerStats` and `ClientStats`.
   - `ScenarioResultSummary` (`qps`, `qps_per_server_core`, `latency_50`..`latency_999`, `server_user_time`, `client_user_time`, etc.).
   - Per-worker exit status and error code distributions (`request_results`).
   The upstream C++ driver's corresponding `*-driver-metrics.json` contains
   only `qps`; it must not be treated as a substitute for these raw metrics.
3. **`*-server.log` & `*-client.log`**: Standard output and error logs from each worker process.
4. **`*-driver.log`**: Driver control stream logs and step-by-step mark transitions.

### No Cloud Deployment Required
In accordance with task BM-11 acceptance criteria:
- All driver control RPCs and data-plane benchmarks execute locally over loopback.
- Native worker stats are consumed without modification or schema trimming.
- No Google Cloud deployment, BigQuery project credentials, or external uploads are required to prove local adapter qualification.
