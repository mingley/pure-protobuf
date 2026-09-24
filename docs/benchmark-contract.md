# gRPC and Protobuf Benchmark Contract

**Version:** 1.1.0  
**Status:** Active Contract (Task BM-01)  
**Revision note:** 1.1.0 closes the BM-01 acceptance gaps: network/RTT and core-scaling dimensions (§3.6), overload rule (§3.7), identical handler work (§5.4), and staged execution, claim scope and anti-gaming rules (§10).
**Applies to:** `pure-protobuf` (`pbrs`), `pbrs-grpc`, `rpc-bench`, and comparison suites

---

## 1. Principles and Scope

This document establishes the binding methodology, workload taxonomy, measurement standards, statistical rigor, and acceptance thresholds for all performance claims across `pure-protobuf` and `pbrs-grpc`.

Performance claims in high-throughput RPC systems are frequently distorted by methodological traps:
1. **Coordinated omission**: Closed-loop generators pausing during server stalls, thereby masking true tail latency.
2. **Conflated CPU accounting**: Blurring client and server CPU consumption into an uninterpretable single metric or measuring loopback processes sharing an event loop.
3. **Selective omission**: Dropping failed calls, timeouts, or connection errors from latency distributions to fabricate flattering tails.
4. **Semantic mismatch**: Comparing zero-copy borrowed decoders against fully-allocated owned trees, or comparing cached re-encodes against cold structural generation.
5. **Statistical noise**: Declaring victory based on single-shot or cherry-picked "best of N" runs without uncertainty intervals or adequate sample counts.

This contract forbids these practices. Every optimization and performance gate (BM-01 through BM-13, OP, CP, and SP lanes) must satisfy this contract.

---

## 2. Workload Taxonomy

To prevent confounding serialization costs with transport networking or client scheduling, workloads are categorized into three distinct abstraction tiers.

```
+-----------------------------------------------------------------------------------+
| Tier 3: End-to-End Application RPCs (Mixed-Peer, Cross-Language: Rust, Go, C++)  |
+-----------------------------------------------------------------------------------+
| Tier 2: Transport-Only Workloads (Identical Codec Held Constant: Native vs Tonic) |
+-----------------------------------------------------------------------------------+
| Tier 1: Codec-Only Workloads (In-Memory Serialization: Owned vs View, Mutated)    |
+-----------------------------------------------------------------------------------+
```

### 2.1 Tier 1: Codec-Only Workloads
Evaluates in-memory serialization and deserialization without network or HTTP/2 transport overhead.

#### A. Buffer Ownership Dimension
* **Owned Decoding**: Decodes wire bytes into owned data structures (`String`, `Vec<T>`, boxed sub-messages). All fields are heap-allocated and retain independent lifetime from the input buffer.
* **Borrowed / Zero-Copy View Decoding**: Decodes wire bytes into lightweight views borrowing directly from the input slice (`&'a str`, `&'a [u8]`, view structs). No heap allocations occur for bytes/string payloads.
* *Equivalence Rule*: Owned decoders (`pbrs`, `prost`, `protobuf` v4 upb, `buffa` owned) may only be compared against owned decoders. Borrowed decoders (`buffa decode_view`, `pbrs` zero-copy views) must be benchmarked and reported in a separate dedicated view column.

#### B. Lifecycle and Mutation Dimension
* **Fresh / Cold Encode**: The message struct is instantiated from scratch and populated with data, then serialized to wire bytes. Evaluates standard constructor and field assignment cost.
* **Mutated Encode**: An existing parsed or constructed message has a subset of fields modified (e.g., updating a timestamp or sequence number), followed by serialization. Evaluates dirty-tracking, re-computation of lengths, and cache invalidation overhead.
* **Cached Encode**: An unmodified message is serialized repeatedly, measuring the performance benefit of cached wire-length calculations or pre-encoded packed representations.
* **Parse-Only vs. Parse-and-Touch**:
  * *Parse-Only*: Deserializes the wire buffer into memory.
  * *Parse-and-Touch*: Deserializes wire bytes and recursively accesses every field via accessors/getters to ensure lazy fields or deferred parser tokens are materialized.

#### C. Schemas and Suites
* Canonical Conformance Schema: `google.protobuf.TestAllTypesProto3` (TAT, covering all scalar types, repeated, packed, maps, nested messages, oneofs).
* Small Structs: `Person` / `Address` (`proto/person.proto`), evaluating small repeated strings and inline integers.
* Common Unary Shapes (`proto/codec_cases.proto`): `empty`, `id`, `scalars`, `name_short`, `name_80`, `name_4kib`, `blob_32`, `blob_4kib`, `blob_64kib`, `envelope`, `nest_d4`, `packed_16`, `packed_256`, `tags_4`, `tags_32`, `map_8`, `oneof_ok`, `rpc_mixed`, `rpc_sparse`.
* High-Volume Blobs & Arrays: 1 MiB and 5 MiB raw bytes and packed fixed32/fixed64 arrays.

#### D. Reference Implementations
* `pbrs` (in-tree code generator and runtime)
* `prost` (version 0.13+)
* `protobuf` crate (version 4.35+ / upb C-kernel FFI)
* `buffa` (version 0.9.1+; both owned and view modes)

---

### 2.2 Tier 2: Transport-Only Workloads
Isolates gRPC and HTTP/2 framing, window management, socket multiplexing, and task scheduling by holding the codec constant.

* **Methodology**: Both client and server execute standard `grpc.testing.TestService` procedures. Serialization on both sides is pinned to the exact same `pbrs` codec (via `pbrs-grpc` natively and `protobuf-tonic` via tonic).
* **Isolated Factors**: HTTP/2 frame construction, HPACK compression, flow control window updates, connection multiplexing, asynchronous runtime task transitions, output batching (`OutBatch`), and zero-copy write aggregation.
* **Reference Implementations**:
  * `pbrs-grpc` (native kernel transport)
  * `tonic` (0.14+ over `hyper` and `h2`, using `pbrs` codec)

---

### 2.3 Tier 3: End-to-End Application RPC Workloads
Evaluates real-world client and server stacks across idiomatic language and framework implementations.

#### A. Reference Implementations
* `pbrs-grpc`: Pure Rust native client and server
* `tonic` + `prost`: Standard Rust ecosystem reference
* `grpc-go`: Official Go reference implementation (`google.golang.org/grpc`)
* `grpc-core` (C++): Official C++ reference implementation

#### B. Cross-Peer Evaluation Matrix
To ensure leadership is not an artifact of proprietary client-server optimizations, all four peer combinations must be evaluated:

| Run Configuration | Client Implementation | Server Implementation | Evaluation Focus |
|---|---|---|---|
| **Native Pair** | `pbrs-grpc` | `pbrs-grpc` | Full-stack native system efficiency |
| **Native Client vs Reference** | `pbrs-grpc` | `grpc-go` / `tonic` | Client runtime efficiency and interoperability |
| **Reference Client vs Native** | `grpc-go` / `tonic` | `pbrs-grpc` | Server runtime efficiency and interoperability |
| **Reference Control** | `grpc-go` / `tonic` | `grpc-go` / `tonic` | Baseline reference line |

---

## 3. Scenario Dimensions and Matrix

Workloads must span realistic service operating points across payload sizes, streaming patterns, security layers, and compression.

### 3.1 Payload Dimensions
1. **Empty (0 Bytes)**: Minimal wire payload. Isolates HTTP/2 framing, HPACK header processing, connection concurrency, and event-loop wakeups.
2. **1 KiB**: Typical microservice control and metadata RPC (small JSON/Protobuf envelopes, metadata, IDs, short strings).
3. **64 KiB**: Medium payload representing batched search results, database rows, or catalog documents. Evaluates buffer slicing and frame chunking.
4. **1 MiB**: Large streaming blob or file chunk. Stresses TCP/HTTP/2 flow control windows, memory allocation, and kernel copy efficiency.

### 3.2 Call Shapes
All four gRPC communication patterns must be measured:
1. **Unary Call** (`UnaryCall`): Single request message $\rightarrow$ Single response message.
2. **Client Streaming** (`StreamingInputCall` / upload): Continuous client stream of $N$ messages $\rightarrow$ Single summary response.
3. **Server Streaming** (`StreamingOutputCall` / download): Single request message $\rightarrow$ Continuous server stream of $N$ messages.
4. **Bidirectional Streaming** (`FullDuplexCall`):
   * *Ping-Pong*: Lockstep request/response turn-taking over a single stream.
   * *Pipelined Full-Duplex*: Asynchronous interleaved traffic in both directions simultaneously.

### 3.3 Security & Transport Framing
1. **Plaintext (h2c)**: Standard HTTP/2 over cleartext TCP.
2. **TLS 1.3**: Encrypted transport using `rustls` (native) and native TLS stacks in reference peers. Standard cipher suite: `TLS_AES_128_GCM_SHA256`.

### 3.4 Compression
1. **Uncompressed (Identity)**: No compression headers or framing.
2. **Gzip Compression**: Standard gRPC compressed frame flag (byte 0 of frame = 1), default compression level.

### 3.5 Primary vs. Holdout Workload Classification

To prevent overfitting optimizations to a narrow set of synthetic micro-benchmarks, scenarios are divided into **Primary Workloads** (mandatory regression gates) and **Holdout Workloads** (validation suites).

```
+------------------------------------------------------------------------------------+
| Primary Workloads (Gated for Leadership)                                          |
|  - Unary Plaintext: Empty, 1 KiB, 64 KiB                                           |
|  - Unary TLS: 1 KiB                                                                |
|  - Server Streaming: 1 KiB (2,000 msgs)                                            |
|  - Client Streaming: 1 KiB (2,000 msgs)                                            |
|  - Bidi Ping-Pong: Empty (256 pairs)                                               |
+------------------------------------------------------------------------------------+
| Holdout Workloads (Exploratory / Anti-Overfitting Validation)                      |
|  - Large Payloads: 1 MiB Unary, 1 MiB Server Streaming                             |
|  - Compressed Streams: 64 KiB Gzip Unary & Streaming                               |
|  - High Connection Multiplexing: 100+ concurrent streams on 1 connection            |
|  - Churn: High-frequency connection teardown and TLS re-handshakes                 |
|  - Adverse Conditions: Injected packet delay (10ms RTT) and packet drops           |
+------------------------------------------------------------------------------------+
```

### 3.6 Network, RTT and Core Scaling

1. **Loopback is smoke-only**: single-host loopback runs (shared or separate processes) validate harness wiring only. Authoritative cells run client and server on separate hosts over a real network (§8).
2. **Real RTT holdouts**: at least one holdout cell per RPC shape class runs with injected round-trip delay (1 ms and 10 ms profiles via `tc netem` or equivalent), exercising window refill, pipelining and deadline paths that loopback hides.
3. **Core scaling**: leadership hosts use pinned cores; the qualification stage repeats representative primary cells at 1/2/4/8 cores within host limits. Single-core numbers never stand in for multicore scaling.

### 3.7 Overload and Saturation

1. Offered load is stepped past the saturation knee until errors or timeouts appear; the overload cell reports goodput, error/timeout rates, p99 including retained failures, and time to recover after load drops.
2. Overload is a validation axis, not a leadership gate: no throughput-gain threshold applies, but dropped or misclassified failures invalidate the run.

---

## 4. Measurement Methodology: Offered Load & Coordinated Omission

### 4.1 Coordinated Omission Prevention
Closed-loop benchmarking (where client thread $i$ waits for response $k$ before initiating request $k+1$) masks severe server-side latency degradations:
$$\text{If a server stalls for } 10\text{ seconds, a closed-loop client sends } 0\text{ requests during that interval.}$$
The slow period contributes only 1 sample to the latency distribution rather than the thousands of requests that a real production client pool would have generated.

**Mandatory Methodology:**
1. **Open-Loop Injection**: Arrival times are predetermined independently of server response times.
   * *Poisson Arrival Process*: Inter-arrival times drawn from an exponential distribution $T_{i+1} = T_i + \text{Exp}(\lambda)$ for realistic burstiness.
   * *Constant-Paced Arrivals*: Fixed intervals $T_{i+1} = T_i + \frac{1}{\lambda}$ for steady-state rate verification.
2. **Schedule-Relative Latency Accounting**:
   $$\text{Reported Latency} = \text{Completion\_Timestamp} - \text{Scheduled\_Dispatch\_Timestamp}$$
   $$\text{Scheduling Lag} = \text{Actual\_Dispatch\_Timestamp} - \text{Scheduled\_Dispatch\_Timestamp}$$
   If the benchmark client experiences scheduling lag exceeding 10% of p50 latency, the load generator is saturated, and the run is automatically invalidated.
3. **No Dropped Failures**:
   * Requests that exceed deadlines, receive error status codes (`RESOURCE_EXHAUSTED`, `UNAVAILABLE`), or time out are **retained** in the latency histogram at $\ge \text{deadline}$.
   * Latency distributions calculated solely over the "successful" subset are forbidden.

---

## 5. Metric Separation: Client vs. Server Efficiency

Client and server efficiency must be instrumented and reported independently. Loopback runs sharing a single process or Tokio runtime are restricted to smoke testing and may never be cited for leadership.

### 5.1 Client Efficiency Metrics
Measured on the dedicated client host/process:
* **Throughput per Client CPU-Second**:
  $$\eta_{\text{client}} = \frac{\text{Successful RPCs}}{\text{Client User CPU Sec} + \text{Client System CPU Sec}}$$
* **Allocation Rate**: Heap bytes allocated per second and allocations per RPC (via jemalloc/mimalloc stats or allocation hooks).
* **Client Resident Set Size (RSS)**: Baseline, steady-state, and peak RSS in MiB.
* **Client Queue Delay**: Time spent in client outbound buffers prior to TCP socket write.

### 5.2 Server Efficiency Metrics
Measured on the dedicated server host/process:
* **Throughput per Server CPU-Second**:
  $$\eta_{\text{server}} = \frac{\text{Successful RPCs}}{\text{Server User CPU Sec} + \text{Server System CPU Sec}}$$
* **Tail Latency at Matched Offered Load**: p50, p90, p95, p99, and p99.9 measured at identical offered QPS ($\lambda$) well below saturation.
* **Server Resident Set Size (RSS)**: Steady-state memory footprint under concurrency.
* **Context Switching & Task Hops**: Voluntary and involuntary context switches per 10,000 RPCs.

### 5.3 Combined Efficiency Metrics
* **Total End-to-End CPU Cost**:
  $$\text{CPU}_{\text{total}} = \frac{\text{Client CPU Sec} + \text{Server CPU Sec}}{\text{Successful RPCs}}$$
* **Goodput**: Delivered application payload megabytes per second (excluding framing/header bytes).

### 5.4 Identical Handler and Validation Work

1. All peers serve identical application semantics: the same `grpc.testing.TestService` / `BenchmarkService` procedures, the same request validation, and the same response construction. Handler CPU is held constant across peers or measured and reported separately; a faster transport must not win by doing less application work.
2. The codec tier (§2.2) pins the identical `pbrs` codec on both ends; the end-to-end tier (§2.3) uses each peer's idiomatic codec but identical message contents and validation rules.

An inbound error on a benchmark bidirectional request stream must surface as a
non-OK response status, not an empty successful stream. Local regressions cover
`StreamingCall` and `StreamingBothWays`; independent official-peer data-plane
validation remains part of BM-08.

The native benchmark worker caps a generated or echoed response payload body
at 4 MiB, using the kernel's default decoded-message size as its local
resource policy. Negative requested sizes fail with `INVALID_ARGUMENT`;
larger bodies and unrepresentable stream aggregates fail with
`RESOURCE_EXHAUSTED`, rather than producing a zero-length or truncated
response. A receiving peer's size limit also counts protobuf overhead.
Comparable official-peer runs must match payload limits explicitly; a
rejected out-of-policy scenario is incomplete, not a performance win.

WorkerService `CoreCount` and `RunServer` setup require an observed,
i32-representable system CPU count; a failed probe returns a non-OK status
instead of claiming one core. `RunServer` and `RunClient` require an actual
process resource snapshot at setup and on every `Mark`. Unsupported or failed
capture returns `UNAVAILABLE` (or `INTERNAL` if cleanup also fails), not zero
CPU/RSS or a reused baseline. Initial
zero elapsed/CPU statistics describe a new interval **after** the baseline
was captured. Reset baselines and client histograms advance only after a
successful Mark capture. On capture failure, the worker shuts down and joins
its owned benchmark server (five-second grace, then abort) or cancels,
aborts and joins its owned client generator before reporting the error.
Synthetic tests cover status mapping and the cleanup helpers; they do not
inject a platform capture failure into a live control stream or prove
completion of independently spawned open-loop RPC tasks. These local worker
checks do not qualify BM-09/BM-10 against an independent official driver.

---

## 6. Statistical Rigor and Precision Standards

### 6.1 Repetition and Paired Randomization
* **Minimum Runs**: Minimum 5 independent, paired runs per scenario cell.
* **Randomized Execution Order**: For every comparison between Candidate ($A$) and Baseline ($B$), execution order must be randomized per run (e.g., $A \rightarrow B, B \rightarrow A, A \rightarrow B, \dots$) to neutralize background system thermal throttling, cloud neighbor noise, and CPU frequency scaling.

### 6.2 Duration and Warmup
* **Warmup Phase**: Minimum 15 seconds (or 10,000 RPCs, whichever is greater) to warm caches, branch predictors, and memory arenas. Warmup samples are discarded.
* **Measurement Phase**: Minimum 60 seconds of steady-state execution per run after warmup.

### 6.3 Confidence and Uncertainty Intervals
* Every reported metric (QPS, CPU efficiency, percentiles) must include a **95% confidence interval** calculated using non-parametric bootstrapping (10,000 resamples) or Student's $t$-distribution across the paired runs.
* Point estimates without error margins are rejected.

### 6.4 Sample Size Thresholds for Percentiles
To make statistically sound percentile claims, sample counts must satisfy:
* **p50 / p90**: Minimum 1,000 observations.
* **p95**: Minimum 5,000 observations.
* **p99**: Minimum 10,000 observations.
* **p99.9**: Minimum 1,000,000 observations required. Any claim regarding p99.9 latency with fewer than $10^6$ recorded samples is invalid.

### 6.5 Histogram Fidelity
Latencies must be recorded using high-dynamic-range histograms (e.g., HDR Histogram with 3 significant figures of precision across 1 µs to 60 s range) or exact nanosecond sample vectors. Coarse logarithmic bucketing that rounds tail latencies is forbidden.

---

## 7. Leadership Acceptance Criteria & Budgets

To claim performance leadership over reference implementations (Tonic, grpc-go, C++), the candidate implementation must satisfy the following criteria:

### 7.1 Primary Workload Leadership Thresholds
1. **Efficiency / Throughput Margin**:
   * **$\ge 20\%$ lower CPU per successful RPC** at matched offered load, **OR**
   * **$\ge 20\%$ higher sustainable throughput** (maximum load maintaining error rate $< 0.001\%$ and p99 within SLA).
2. **Tail Latency Guardrail**:
   * **No unexplained $> 5\%$ regression in p99 latency** on any primary workload compared to the reference baseline.
3. **Memory Guardrail**:
   * Peak and steady-state RSS within $+10\%$ of reference implementation, with zero memory growth over a 1-hour soak test.
4. **Correctness Pre-requisite**:
   * Zero unhandled transport panics, zero data corruptions, and 100% pass on applicable official gRPC interoperability tests.

### 7.2 Codec Leadership Thresholds
* $\ge 15\%$ lower CPU (encode + owned decode) vs `prost` and `protobuf` v4 upb across all common unary shapes.
* Zero-copy view decode must match or exceed `buffa view` within a 5% margin on borrowed string/bytes payloads.

### 7.3 Transport Leadership Thresholds
* Strictly lower p50 and p99 latency on unary calls vs `tonic` under identical hardware and single-stream conditions.
* Streaming throughput parity: $\ge 90\%$ of `tonic` on single-stream loopback; $\ge 20\%$ higher throughput on multi-connection saturated load.

---

## 8. Environmental Controls & Hardware Budgets

All authoritative benchmark runs must record and report their execution profile:

| Parameter | Specification Requirement |
|---|---|
| **Host Architecture** | Dedicated Linux x86_64 and arm64 bare-metal or pinned-core VM instances. No burstable or shared cloud instances. |
| **CPU Configuration** | Core isolation (`isolcpus`), turbo boost disabled, CPU governor set to `performance`, hyper-threading topology recorded. |
| **Network Interface** | Dedicated 10 Gbps+ NICs with documented MTU, ring buffer sizes, and interrupt affinity. |
| **Toolchains & Versions** | Pinned Rust compiler (`rustc --version`), Go version (`go version`), C++ compiler (`clang --version`). |
| **Binary Optimization** | Release build with thin-LTO or fat-LTO, `opt-level = 3`, `codegen-units = 1`. Allocator explicitly specified (`system`, `mimalloc`, `jemalloc`). |
| **Operating System** | Linux kernel version, distribution, and relevant `sysctl` settings (`tcp_rmem`, `tcp_wmem`, `somaxconn`). |

---

## 9. Summary Workflow for Benchmark Execution

```
[Define Scenario in leadership.json]
                 |
                 v
[Pre-Run Sanity Check: Conformance & Interop Gates Pass]
                 |
                 v
[5x Randomized Paired Runs (Candidate vs. Reference)]
  - 15s Warmup (Discarded)
  - 60s Measurement per run
  - Open-Loop Offered Load + Schedule Tracking
                 |
                 v
[Collect Metrics: Client CPU, Server CPU, RSS, HDR Histogram]
                 |
                 v
[Statistical Validation: 95% Confidence Intervals, Sample Counts Checked]
                 |
                 v
[Leadership Audit: >=20% Efficiency Gain, <=5% p99 Regression]
```

---

## 10. Staged Execution, Claim Scope and Anti-Gaming Rules

### 10.1 Affordable staged matrix

Full Cartesian coverage (peers × shapes × payloads × TLS × compression × cores × RTT) is impractical. Runs proceed in stages; each stage gates the next:

| Stage | Name | Cells | Budget |
|---|---|---|---|
| 0 | Smoke | Loopback harness check, one unary cell | Minutes, any host |
| 1 | Primary gates | All `primary` scenarios in `leadership.json`, native pair plus strongest reference per axis | Dedicated hosts, §6 statistics |
| 2 | Holdout validation | All `holdout` scenarios; detects overfitting, no leadership claim required | Same hosts; abbreviated peers allowed on cost grounds when recorded |
| 3 | Qualification | Representative primary cells × 1/2/4/8 cores × real-network RTT profiles | Pinned hosts, full §6 statistics |

Freezing representative cells and pairwise stress cases first is mandatory; expanding a stage requires recording the added cells before rerunning.

### 10.2 Claim scope

Every leadership claim names the exact scenario cells, peer implementations and versions, host hardware, resource budgets and statistical intervals. A codec win cannot satisfy a client or server gate, and a primary-cell win cannot be generalized beyond the measured matrix. Universal "fastest in the world" claims are forbidden; the publishable outcome is leadership on the named matrix, including losses.

### 10.3 No benchmark-specific escape hatches

Implementations must not detect benchmark traffic (by payload pattern, peer identity, port, timing, or scenario ID) to select fast paths unavailable to general traffic. Every optimization exercised by a benchmark cell must apply to equivalent production traffic; benchmark-only tuning disqualifies the run and the claim.
