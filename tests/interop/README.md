# Official Interoperability and Conformance Case Registry

This directory contains the authoritative, versioned case registry (`cases.json`) and associated test harnesses for `pure-protobuf` and `pbrs-grpc`.

The case registry records every official upstream test case across Protobuf conformance and gRPC interoperability suites, establishing an honest, transparent, and reproducible accounting of specification sources, peer directions, transports, product profiles, task ownership, and present coverage.

---

## 1. Pinned Upstream Specifications and Tools

All test definitions, procedures, schemas, and runners are pinned to immutable upstream commits:

| Upstream Project | Pin / Commit | Relevant Contracts and Documents |
|---|---|---|
| **`protocolbuffers/protobuf`** | `v35.1`<br>[`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`](https://github.com/protocolbuffers/protobuf/tree/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03) | [Conformance Guide](https://github.com/protocolbuffers/protobuf/blob/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/conformance/README.md), [Rust Shared Tests](https://github.com/protocolbuffers/protobuf/tree/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/rust/test/shared), local `vendor/google/PIN` and `vendor/google/SHA`. |
| **`grpc/grpc`** | [`d1487957db6658bc532b72871775148229836627`](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627) | [Interop Test Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/interop-test-descriptions.md), [HTTP/2 Negative Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/http2-interop-test-descriptions.md), [Connection Backoff Description](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/connection-backoff-interop-test-description.md), [xDS Test Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/xds-test-descriptions.md), [Official Runner](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/run_interop_tests.py), [Performance Framework](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/performance/README.md), `WorkerService`, `BenchmarkService`. |
| **`grpc/grpc-go`** | [`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`](https://github.com/grpc/grpc-go/tree/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef) | [Interop Client](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/interop/client/client.go) and [Interop Server](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/interop/server/server.go) reference implementations. |
| **`grpc/proposal`** | [`6342be729b96478a2897ceb208a8cddcd832a17b`](https://github.com/grpc/proposal/tree/6342be729b96478a2897ceb208a8cddcd832a17b) | [gRFC A6: Client Retries](https://github.com/grpc/proposal/blob/6342be729b96478a2897ceb208a8cddcd832a17b/A6-client-retries.md) (transparent retry, retry commitments, buffering). |

---

## 2. Registry Schema (`cases.json`)

`cases.json` is a machine-readable JSON registry conforming to the following structure:

### Top-Level Metadata
* `version`: Semantic version of the registry format (e.g. `1.0.0`).
* `updated_at`: ISO 8601 date of the last registry reconciliation.
* `title`: Descriptive registry name.
* `description`: Overview of registry scope and purpose.
* `upstream_pins`: Exact commit hashes and canonical documentation paths for all upstream dependencies.
* `disposition_definitions`: Formal definitions of allowed disposition states.
* `suites`: Dictionary mapping suite keys to descriptions.
* `summary`: Aggregate counts by disposition and by suite.
* `cases`: Array of individual case records.

### Case Record Schema
Every entry in `cases` contains:
* `case` *(string)*: Unique case identifier, matching CLI `-test_case` arguments or official suite identifiers (e.g. `empty_unary`, `rst_after_header`).
* `name` *(string)*: Human-readable descriptive name.
* `suite` *(string)*: Suite classification key (see [Test Suites Covered](#3-test-suites-covered)).
* `procedure_source` *(string)*: Exact pinned document path or source runner reference describing the procedure.
* `peer_direction` *(string)*: The direction of RPC execution:
  * `both`: Both client and server implementations are tested against peers.
  * `client_to_server`: Client executes procedure against server.
  * `runner_to_binary`: External conformance test runner communicates via stdin/stdout pipe with binary.
  * `driver_to_worker`: Performance driver orchestrates workers.
  * `internal`: Internal semantic test suite.
* `transport` *(string)*: Protocol and wire transport: `http2_cleartext`, `http2_tls`, `http2_mtls`, `alts`, `pipe`, or `none`.
* `profile` *(string)*: Applicable shipping/qualification profile: `core`, `native`, `tonic`, `full`, `extended`.
* `owner` *(string)*: Authoritative task ID in `docs/plan/tasks.json` responsible for this case.
* `disposition` *(string)*: Current qualification disposition (see [Dispositions](#disposition-rules)).
* `justification` *(string or null)*: Required explanation whenever disposition is `unsupported`, `blocked_external`, or `not_applicable`.
* `present_coverage` *(object)*:
  * `status`: Current status (`passed`, `not_run`, `unsupported`, `blocked_external`, `not_applicable`).
  * `evidence_file`: Path to the script, harness, or test file providing current evidence.
  * `passing_directions`: Array of verified peer directions currently passing.
  * `notes`: Additional technical context, limitations, or pending work.

### Disposition Rules
Every active upstream case has an explicit disposition:
1. `passed`: The case has been executed and verified passing in official test scripts, peer passes, or conformance harness.
2. `not_run`: An active upstream procedure supported or planned for support, but not yet executed in the official harness.
3. `unsupported`: A protocol feature or procedure not yet implemented in the target profile.
4. `blocked_external`: Blocked by external infrastructure, provider identity, or cloud secrets (e.g. GCP metadata server, Google IAM credentials). **Missing credentials must always be classified as `blocked_external`, never `not_applicable`.**
5. `not_applicable`: Explicitly excluded upstream test or kernel internal with approved, documented technical justification (e.g. C/upb internal arena memory layouts).

---

## 3. Test Suites Covered

The 69 registered cases are organized into 12 test suites:

### 1. Standard Interoperability (`standard_interop`, 16 cases)
Baseline gRPC RPC patterns and protocol semantics defined in `doc/interop-test-descriptions.md`:
* **Base unary and streaming**: `empty_unary`, `large_unary`, `client_streaming`, `server_streaming`, `ping_pong`, `empty_stream`.
* **Flow control and lifecycle**: `cancel_after_begin`, `cancel_after_first_response`, `timeout_on_sleeping_server`.
* **Metadata and errors**: `custom_metadata`, `status_code_and_message`, `special_status_message`.
* **Unimplemented handling**: `unimplemented_method`, `unimplemented_service`.
* **Extended runner cases**: `pick_first_unary` (subchannel pick_first validation), `cacheable_unary` (HTTP/2 GET mapping).

### 2. Compression Interoperability (`compression_interop`, 4 cases)
Message-level compression negotiation and framing:
* `client_compressed_unary`, `server_compressed_unary`, `client_compressed_streaming`, `server_compressed_streaming`.
* *Status*: Passes the 18-case self-interop pass in `scripts/grpc-interop.sh`, and both cross-peer directions (36 cells) against the pinned C++ reference peer (`grpc/grpc@d1487957`, v1.84.0) via `scripts/grpc-interop-cpp.sh` in task `IO-05`, with wire compression bits asserted by the reference peer. The native client adds a high-entropy compressed unary leg **after** the three official calls; it does not replace the zero-filled official vector. Cross-language execution against `grpc-go` is skipped because `grpc-go` ignores compression flags.

### 3. HTTP/2 Negative Tests (`http2_negative`, 8 cases)
Adversarial framing, stream cancellation, and connection termination defined in `doc/http2-interop-test-descriptions.md`:
* `rst_after_header`, `rst_after_data`, `rst_during_data`.
* `goaway`, `ping`, `max_streams`.
* `data_frame_padding`, `no_df_padding_sanity_test`.
* *Status*: `scripts/grpc-http2-interop.sh` runs all eight registered
  procedures against a purpose-built local HTTP/2 peer and retains a required
  matrix report. This is a spec-derived adapter, **not** execution of the
  upstream runner binary; the independent-peer qualification in `IO-08`
  remains open. A fake successful client with no peer frames is recorded as
  failed by `test_http2_peer_proof.py`; reports link both client and local-peer
  logs. In-tree hostile tests are complementary.

### 4. Server Probes (`server_probe`, 2 cases)
Official server transport verification probes from `tools/run_tests/run_interop_tests.py`:
* `server_tls_probe`: Verifies ALPN negotiation (`h2`), rejection of invalid ALPN (`http/1.1`), TLS 1.2/1.3 protocol versions and AEAD ciphers, server certificate presentation, and live TLS gRPC RPC.
* `server_framing_probe`: Probes HTTP/2 24-byte connection preface, SETTINGS frame exchange and ACK, rapid reset stream cancellation flood (CVE-2023-44487), small DATA frames flow control, fragmented HEADERS across CONTINUATION frames and CONTINUATION flood protection, bad headers / non-POST HTTP 405 / unsupported media type HTTP 415 rejection, and post-probe server health verification.
* *Status*: `scripts/grpc-http2-server-interop.sh` runs spec-derived local TLS
  and framing probes against `pbrs-grpc-interop-server`, with two required
  result rows and retained logs. It does not invoke the upstream probe binary;
  `IO-09` remains open until that qualification boundary is resolved.

### 5. Connection Backoff (`connection_backoff`, 1 case)
Reconnect backoff, jitter, and retry caps defined in `doc/connection-backoff-interop-test-description.md`:
* `connection_backoff`: Full ~540-second exercise against official C++ `ReconnectService`.
* *Status*: Scheduled in task `FL-05`.

### 6. Soak Testing (`soak`, 2 cases)
Long-running reliability and resource stability from `run_interop_tests.py`:
* `rpc_soak`: Sustained high-iteration RPC loop measuring latency and error budget over a long window.
* `channel_soak`: Repeated channel creation, connection churn, and teardown under load.
* *Status*: Adapters qualified in task `IO-10` (`pbrs-grpc/src/interop_cases.rs`, `pbrs-grpc-interop-client` soak flags, `tests/interop/test_soak.py` 12/12 deterministic tests plus local qualification-scale runs); the full-duration official campaign remains a scheduled operator run.

### 7. Stream Scaling (`scaling`, 1 case)
Concurrent connection scaling under peer stream limits:
* `max_concurrent_streams_connection_scaling`: Subchannel scaling when `SETTINGS_MAX_CONCURRENT_STREAMS` limit is reached.
* *Status*: Scheduled in task `EX-21`.

### 8. Authentication & Credentials (`auth`, 7 cases)
Cloud identity and call credentials from `run_interop_tests.py`:
* `compute_engine_creds`, `jwt_token_creds`, `oauth2_auth_token`, `per_rpc_creds`, `google_default_credentials`, `compute_engine_channel_credentials`.
* `alts_credentials`: Application Layer Transport Security for Google Cloud handshaker.
* *Status*: Cloud auth cases are `blocked_external` pending approved GCP environment (task `EX-17`). ALTS is `unsupported` pending provider review (tasks `EX-18` / `EX-19`).

### 9. ORCA (`orca`, 2 cases)
Open Request Cost Aggregation backend load reporting (gRFC A51):
* `orca_per_rpc`: Per-call backend load metrics in trailing metadata.
* `orca_oob`: Out-of-band load reporting stream over `OpenRcaService`.
* *Status*: Scheduled in tasks `EX-01` and `EX-02`.

### 10. xDS & Load Balancing (`xds_lb`, 12 cases)
Client-side service mesh and dynamic traffic routing from `doc/xds-test-descriptions.md`:
* `round_robin`, `backends_restart`, `circuit_breaking`, `outlier_detection`.
* `xds_ping_pong`, `traffic_splitting`, `path_matching`, `header_matching`, `fault_injection`, `timeout`, `metadata_exchange`, `app_net_security`.
* *Status*: Scheduled in tasks `FL-04` and `EX-07` through `EX-16`.

### 11. Protobuf Conformance (`protobuf_conformance`, 6 cases)
Official Protobuf v35.1 conformance suite and Rust application semantics:
* `protobuf_binary_conformance`: Required binary wire format conformance (5,631 tests).
* `protobuf_json_conformance`: Proto3/edition 2023 JSON mapping conformance.
* `protobuf_text_conformance`: Text format conformance (909 tests).
* `protobuf_enforce_recommended`: Recommended conformance suite passed without `failure_list_rust_upb.txt` skips.
* `rust_shared_application_tests`: In-tree port of `rust/test/shared/` accessors, merge, and serialize in `tests/google_shared.rs` (standalone external runner scheduled in `PB-02`).
* `upb_kernel_internals`: Explicit `not_applicable` with documented justification (C upb arena layout internals do not apply to safe pure-Rust runtime).

### 12. Performance Framework (`performance`, 8 cases)
Official gRPC benchmarking framework from `tools/run_tests/performance/README.md`:
* **WorkerService** control protocol: `worker_service_run_server`, `worker_service_run_client`, `worker_service_core_count`, `worker_service_quit_worker` (tasks `BM-09`, `BM-10`).
* **BenchmarkService** data plane: `benchmark_service_unary`, `benchmark_service_streaming`, `benchmark_service_streaming_from_client`, `benchmark_service_streaming_from_server` (task `BM-08`).

---

## 4. How Cases Are Run

### gRPC Interoperability (`scripts/grpc-interop.sh`)
The standard interop script executes three passes:
1. **Kernel Client $\to$ Kernel Server** (`pbrs-grpc-interop-client` $\to$ `pbrs-grpc-interop-server`):
   Runs all 18 cases (14 base cases + 4 compression cases).
2. **Kernel Client $\to$ Go Server** (`pbrs-grpc-interop-client` $\to$ `google.golang.org/grpc/interop/server`):
   Runs the 14 base cases against official `grpc-go`.
3. **Go Client $\to$ Kernel Server** (`google.golang.org/grpc/interop/client` $\to$ `pbrs-grpc-interop-server`):
   Runs the 14 base cases against official `grpc-go`.

Invocation:
```bash
# Run all passes (requires Go toolchain):
./scripts/grpc-interop.sh

# Run self-interop pass only:
./scripts/grpc-interop.sh --self-only
```

### Protobuf Conformance (`scripts/conformance.sh`)
Builds Google's official `conformance_test_runner` from pinned `v35.1` (`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`) and runs:
1. Required run 1: `--maximum_edition 2023`
2. Required run 2: `--maximum_edition 2023`
3. Recommended run: `--enforce_recommended --maximum_edition 2023`

Invocation:
```bash
./scripts/conformance.sh
```

### Server HTTP/2 Framing and TLS Probes (`scripts/grpc-http2-server-interop.sh`)
The server probe harness verifies native server framing, transport security, and protocol resilience against simulated client probes mapping to official upstream server test probes in `tools/run_tests/run_interop_tests.py`:

#### 1. Probe Contract and Architecture
The script starts two native `pbrs-grpc-interop-server` processes:
* **TLS Instance**: Bound to a dynamic TLS port with `--use_tls=true`, `--tls_cert_file=pbrs-grpc/tests/tls_data/server.crt`, and `--tls_key_file=pbrs-grpc/tests/tls_data/server.key`.
* **Cleartext Instance**: Bound to a dynamic cleartext port for cleartext framing and boundary probes.

Both servers are managed as background jobs with automatic process tracking, startup readiness checks, and clean shutdown traps on exit/interruption.

#### 2. Probe Test Matrix
* **`server_tls_probe` (`http2_tls`)**:
  * *ALPN `h2` Negotiation*: Client connects with `h2` ALPN, asserting `h2` is selected and modern TLS (TLS 1.2 or 1.3 with AEAD cipher) is negotiated.
  * *ALPN Rejection*: Client initiates TLS handshake offering only non-`h2` ALPN (`http/1.1`), asserting that server rejects the connection with a TLS alert (`TLSV1_ALERT_NO_APPLICATION_PROTOCOL`).
  * *Certificate Presentation*: Verifies server presents valid certificate matching identity.
  * *Live TLS RPC*: Executes native `pbrs-grpc-interop-client` against the TLS server with `--use_tls=true --tls_ca_file=... --server_host_override=localhost --test_case=empty_unary`.
* **`server_framing_probe` (`http2_cleartext`)**:
  * *Connection Preface & SETTINGS*: Sends 24-byte client preface and empty SETTINGS frame, receives and validates server SETTINGS frame, and completes two-way SETTINGS ACK handshake.
  * *Rapid Reset Stream Flood*: Dispatches rapid HEADERS followed immediately by RST_STREAM (CANCEL) across 32 streams (CVE-2023-44487 simulation) to verify server rate-limits/mitigates flood without crashing.
  * *Small DATA Frames*: Sends request payload fragmented into tiny 1-byte DATA frames to verify framing boundary parsing and flow control budget enforcement.
  * *CONTINUATION Frame Assembly & Flood*: Verifies multi-frame HEADERS assembly (HEADERS without `END_HEADERS` followed by CONTINUATION with `END_HEADERS`), and tests CONTINUATION flood protection.
  * *Bad Headers & Non-POST Methods*: Asserts non-POST HTTP methods (GET) are rejected with HTTP 405 Method Not Allowed (`allow: POST`), and non-gRPC media types (`application/json`) are rejected with HTTP 415 Unsupported Media Type.
  * *Post-Probe Server Health Verification*: Executes a clean `empty_unary` RPC via `pbrs-grpc-interop-client` to confirm the accept loop and stream dispatcher remained healthy throughout adversarial probes.

#### 3. Prerequisites
* Python 3 standard library (`socket`, `ssl`, `struct`, `subprocess`, `time`). Zero third-party pip dependencies.
* Compiled `pbrs-grpc-interop-server` and `pbrs-grpc-interop-client` binaries (built automatically unless `--skip-build` is provided).
* Throwaway loopback test certificates in `pbrs-grpc/tests/tls_data/`.

#### 4. Invocation
```bash
# Run all server probes (TLS and framing):
./scripts/grpc-http2-server-interop.sh

# Run specific probe:
./scripts/grpc-http2-server-interop.sh --cases=server_tls_probe
./scripts/grpc-http2-server-interop.sh --cases=server_framing_probe

# Skip cargo build and specify custom log directory:
./scripts/grpc-http2-server-interop.sh --skip-build --log-dir=target/interop-logs/custom
```
Execution traces and logs are stored under `target/interop-logs/`, recorded into `results.json`, validated against `cases.json`, and aggregated into `report.json`.
Required-profile gating: validation and aggregation run with `--suite server_probe --profile native --require-matrix`, so a run that omits a probe (e.g. `--cases=server_tls_probe`) or hits an unexpected failure exits non-zero and cannot qualify. Local hostile tests in `pbrs-grpc/tests/hostile.rs` and TLS tests in `pbrs-grpc/tests/tls.rs` are complementary and never substitute for these probe records.
Both HTTP/2 runners require fresh, distinct report paths and fail if any case
cannot be recorded or aggregated; a prior report cannot fill a missing row in
a later run.

---

## 5. Machine-Readable Proof, Reporting, and Aggregation (`scripts/interop-report.py`)

The reporting tool `scripts/interop-report.py` is a standard-library-only Python 3 tool (zero third-party dependencies) that validates, aggregates, and writes machine-readable proof for all interoperability and conformance test runs.

### 5.1 Verification States
The tool models 6 distinct execution and disposition states:
* `passed`: Verified passing in official test scripts, peer passes, or conformance harness.
* `failed`: Test execution failed, exited non-zero, or violated protocol assertions.
* `not_run`: Active upstream test case scheduled for execution, not yet run in the official harness.
* `unsupported`: Protocol feature, RPC pattern, or service not yet implemented in target profile.
* `blocked_external`: Blocked by external infrastructure or credentials (e.g. cloud IAM, GCP metadata service). Never marked `not_applicable`.
* `not_applicable`: Explicitly excluded upstream test or kernel internal with approved, documented technical justification (e.g. C/upb arena memory layouts).

### 5.2 CLI Invocations and Usage

#### 1. Aggregate and Report (Default Mode)
Processes an execution results file, validates against `cases.json`, outputs summary tables (terminal or markdown), and writes `report.json`:
```bash
# Validate, aggregate, print terminal table, and write report.json
python3 scripts/interop-report.py --cases tests/interop/cases.json --results results.json --output report.json

# Output markdown summary table (e.g. for CI job summaries or PR comments)
python3 scripts/interop-report.py --results results.json --format markdown

# Pipe results from stdin
cat results.json | python3 scripts/interop-report.py --results - --format json
```

#### 2. Validate Only
Validates results against matrix definitions, upstream commit pins, and disposition rules without generating reports:
```bash
python3 scripts/interop-report.py validate --cases tests/interop/cases.json --results results.json --require-matrix --require-peers
```

#### 3. Record Individual Case Execution
Appends or updates a single test execution entry in a results file (used by shell harnesses):
```bash
python3 scripts/interop-report.py record \
  --output results.json \
  --case empty_unary \
  --status passed \
  --duration-ms 12.4 \
  --peer grpc-go \
  --direction kernel_client_to_go_server \
  --transport http2_cleartext \
  --stdout-log logs/empty_unary.stdout \
  --stderr-log logs/empty_unary.stderr \
  --exit-code 0 \
  --attempt-count 1
```

### 5.3 CLI Options
* `--cases <path>`: Path to `cases.json` registry (default: `tests/interop/cases.json`).
* `--results <path>`: Path to input execution results JSON (`-` reads from standard input).
* `--output, -o <path>`: Destination path for machine-readable `report.json`.
* `--format {terminal,markdown,json}`: Output display format (default: `terminal`).
* `--suite <name>`: Filter and enforce requirements for a specific suite (e.g. `standard_interop`).
* `--profile <name>`: Filter and enforce requirements for a qualification profile (e.g. `native`).
* `--require-all`: Fail if any case in `cases.json` for the target scope is missing from results.
* `--require-matrix`: Fail if any expected direction from `cases.json` is missing.
* `--require-peers`: Fail if independent peer execution is missing or substituted with self-test.
* `--strict` / `--no-strict`: Enforce strict retry checking (default: `--strict` fails if retries hid initial failure).
* `--no-color`: Disable ANSI color escape codes in terminal output.

### 5.4 Exit Codes
`scripts/interop-report.py` emits deterministic exit codes for automated CI and gating:
* `0` (**Passed / Qualified**): All evaluated cases passed, all matrix requirements met, pins verified, and no failure occurred.
* `1` (**Test Failure**): One or more test cases had `failed` status or non-zero exit codes.
* `2` (**Validation / Matrix Error**): Structural validation failed: missing required cases, duplicate matrix rows, wrong peer pins, wrong procedure sources, empty output, retries hiding first failures, or unauthorized self-test substitution.
* `3` (**Usage / IO Error**): Bad command-line arguments, missing input files, or unreadable JSON syntax.

### 5.5 Input Results Schema (`results.json`)
The input results file is a JSON array or object with a `"results"` array containing:
```json
{
  "peer_pins": {
    "grpc_go": "dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef"
  },
  "results": [
    {
      "case": "cancel_after_begin",
      "status": "passed",
      "duration_ms": 42.5,
      "peer": "pbrs-grpc",
      "direction": "kernel_client_to_kernel_server",
      "transport": "http2_cleartext",
      "stdout_log": "logs/cancel_after_begin.stdout",
      "stderr_log": "logs/cancel_after_begin.stderr",
      "attempt_count": 2,
      "exit_code": 0,
      "first_attempt_status": "failed",
      "attempts": [
        {
          "attempt": 1,
          "status": "failed",
          "duration_ms": 18.0,
          "exit_code": 1,
          "stderr_log": "logs/cancel_after_begin_att1.stderr"
        },
        {
          "attempt": 2,
          "status": "passed",
          "duration_ms": 24.5,
          "exit_code": 0,
          "stdout_log": "logs/cancel_after_begin_att2.stdout"
        }
      ],
      "procedure_source": "grpc/grpc@d1487957db6658bc532b72871775148229836627:doc/interop-test-descriptions.md",
      "notes": "Retried once due to deadline race"
    }
  ]
}
```

### 5.6 Output Report Schema (`report.json`)
The generated `report.json` document links raw logs, exit codes, exact upstream pins, suite aggregates, and profile metrics:
```json
{
  "report_version": "1.0.0",
  "generated_at": "2026-09-18T19:30:00Z",
  "overall_status": "passed",
  "overall_passed": true,
  "failure_reasons": [],
  "target_suite": "standard_interop",
  "target_profile": "native",
  "upstream_pins": {
    "protobuf": { "repository": "protocolbuffers/protobuf", "version": "v35.1", "commit": "35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03" },
    "grpc": { "repository": "grpc/grpc", "commit": "d1487957db6658bc532b72871775148229836627" },
    "grpc_go": { "repository": "grpc/grpc-go", "commit": "dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef" }
  },
  "summary": {
    "total_evaluated": 18,
    "passed": 18,
    "failed": 0,
    "not_run": 0,
    "unsupported": 0,
    "blocked_external": 0,
    "not_applicable": 0,
    "flaky_or_retried": 0,
    "total_duration_ms": 245.8
  },
  "by_suite": {
    "standard_interop": {
      "total_cases": 14,
      "passed": 14,
      "failed": 0,
      "not_run": 0,
      "unsupported": 0,
      "blocked_external": 0,
      "not_applicable": 0,
      "flaky": 0,
      "status": "passed"
    }
  },
  "by_profile": {
    "native": {
      "total_cases": 18,
      "passed": 18,
      "failed": 0,
      "status": "passed"
    }
  },
  "results": [ ... ]
}
```

### 5.7 Fail-Closed Validation Rules
Aggregation strictly fails closed (status: `failed`, exit code 1 or 2) under any of the following conditions:
1. **Empty Output**: If results contains 0 records or empty payload.
2. **Missing Cases**: If any case required for qualification is omitted from results.
3. **Skipped Cases**: If any required case is marked `not_run`.
4. **Duplicate Matrix Rows**: If two records share identical `(case, peer, direction, transport)` coordinates.
5. **Wrong Upstream Pins**: If `peer_pin` does not match pinned upstream commit (e.g. `grpc-go` at `dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`) or if `procedure_source` does not match `cases.json`.
6. **Retries Hiding First Failures**: In strict qualification mode, any test case that passed on a retry after failing attempt 1 is rejected. A flaky first attempt is recorded and cannot satisfy a required qualification gate.
7. **No Self-Test Substitution**: For cases with `peer_direction: "both"`, a self-test (`kernel_client_to_kernel_server`) can never substitute for independent peer passes (`kernel_client_to_go_server`, `go_client_to_kernel_server`).

### 5.8 Integration with GT-04 (Diagnosable & Bounded Runs)
Task **GT-04** upgrades `scripts/grpc-interop.sh` and provides `tests/interop/test_runner.py` to produce granular execution evidence consumed by `scripts/interop-report.py`:
* **Per-Attempt Log Retention**: Rather than discarding stdout/stderr to `/dev/null`, the runner writes individual log files for every attempt (e.g. `logs/<case>_attempt1.stderr`, `logs/<case>_attempt2.stdout`).
* **Attempt Tracking**: Each test case emits a result record specifying `attempt_count` and the detailed `attempts` list, linking exit codes and durations.
* **Flake Transparency**: When transient races occur (e.g. in `cancel_after_begin` or `timeout_on_sleeping_server`), the initial failure is captured. When aggregated with `--strict`, the report explicitly surfaces the flake rather than masking it.
* **Deterministic Gate Pass/Fail**: GT-04 concludes its test passes by calling `scripts/interop-report.py aggregate --results <scratch>/results.json --output <scratch>/report.json`, failing the runner whenever `interop-report.py` returns non-zero.

---

## 6. Consumption by Subsequent Tasks

Subsequent tasks in the execution plan consume `cases.json` as the single source of truth:

* **GT-02 (Machine-Readable Proof & Aggregation)**:
  * Reads `tests/interop/cases.json` to parameterize `scripts/interop-report.py` and `tests/interop/test_report.py`.
  * Verifies that every required matrix row is present, detects duplicate entries, enforces valid disposition transitions, and guarantees that aggregation fails closed if an independent peer pass is missing or substituted with a self-test.
* **GT-03 (Pin the Go Peer and Require It)**:
  * Uses `cases.json` to enforce that all 14 base interop cases are executed in both directions against the pinned `grpc-go` peer (`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`).
  * Enforces that compression cases cite `grpc-go`'s lack of support and that self-only runs are clearly labeled insufficient for qualification.
* **GT-04 (Diagnosable and Bounded Interop Runs)**:
  * Uses case metadata in `cases.json` to assign per-case timeouts, capture distinct attempt logs, track retries, and ensure transient races (e.g. in `cancel_after_begin`) remain visible rather than masked.
* **GT-05 (Require Retained Official Evidence in CI)**:
  * Uploads full interop attempt logs, server logs, and machine-readable `report.json` even on test failure (`if: always()`) in `.github/workflows/ci.yml`.
  * Preserves separate required and recommended conformance outputs (`required_1/`, `required_2/`, `recommended/`) and records structured summary metadata (`summary.json`) with runner pin, runner SHA, git commit, timestamp, and test counts.
  * Ensures caches and stamps incorporate immutable source inputs (`PIN` + `SHA`), and maintains reusable CI gating before release publication in `release.yml`.
* **GT-06 (Detect Upstream and Dependency Drift Separately)**:
  * Adds an opt-in/scheduled read-only compatibility lane that compares reviewed newer case inventories/toolchains against the pinned regression lane.
* **IO-01 through IO-10 (Assertion Audits, TLS, Negative HTTP/2, C++ Peer, Soak)**:
  * Updates `cases.json` dispositions from `not_run` to `passed` as each official adapter and harness is qualified.
* **FL and EX Lanes (Backoff, ORCA, Cloud Auth, xDS)**:
  * Tracks progress and records qualification evidence for advanced fleet and cloud features.
* **QL-04 (Full Official-Gate Inventory Reconciliation)**:
  * Performs final verification against `cases.json` to confirm that every active upstream case has achieved a verified `passed` disposition before any full-profile leadership claim is published.

---

## 7. CI Artifact Retention Contract and Required Profile Pass Criteria

Official evidence of interoperability and conformance is retained in CI pipelines to guarantee that passing runs are verifiable and failures are immediately diagnosable without needing local repros.

### 7.1 Artifact Retention Contract

All test execution in `.github/workflows/ci.yml` must adhere to the following unconditional retention rules:

1. **Unconditional Upload on Failure (`if: always()`)**:
   - Every CI job executing official test suites must upload its outputs, raw logs, and machine-readable reports using `actions/upload-artifact@v4` with `if: always()`.
   - Test runs that fail, crash, time out, or are cancelled must never discard diagnostic traces to `/dev/null` or exit without persisting evidence.

2. **gRPC Interoperability Artifacts (`grpc-interop` job)**:
   - **`target/interop-logs/`** (`grpc-interop-logs`): Full directory hierarchy containing per-attempt stdout/stderr logs for every executed case and direction (`<peer>-<direction>-<case>-attempt<N>.log`), background server lifecycle logs (`server-kernel.log`, `server-go.log`), and raw execution events (`results.json`).
   - **`target/interop-report.json`** (`grpc-interop-report`): Aggregated, machine-readable proof report emitted by `scripts/interop-report.py`. Contains overall pass/fail status, upstream commit pins (`grpc-go` @ `dd51b1c90aaf`), per-suite and per-profile aggregates, individual case durations, attempt counts, exit codes, and failure justifications.

3. **Protobuf Conformance Artifacts (`conformance` job)**:
   - **`target/conformance-out/`** (`conformance-out`): Complete conformance output directory containing separate artifacts for each evaluation pass:
     - `required_1/`: Failure lists, text mismatch files, and full runner log (`runner.log`) for required run 1 (`--maximum_edition 2023`).
     - `required_2/`: Output files and runner log (`runner.log`) for required run 2 (proving deterministic repeatability).
     - `recommended/`: Output files and runner log (`runner.log`) for the recommended pass (`--enforce_recommended --maximum_edition 2023`).
     - `summary.json`: Structured metadata document recording:
       - `runner_pin`: Vendored upstream runner tag (e.g. `v35.1` from `vendor/google/PIN`).
       - `runner_sha`: Pinned upstream commit SHA (e.g. `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` from `vendor/google/SHA`).
       - `git_commit`: The exact `pure-protobuf` repository commit evaluated.
       - `timestamp`: UTC ISO 8601 execution timestamp.
       - `maximum_edition`: Maximum edition tested (`2023`).
       - `test_counts`: Exact test counts for each run (`required_run_1`, `required_run_2`, `recommended`) and total failure counts.
       - `runs`: Detailed status, successes, skipped, expected failures, and unexpected failures per pass.
       - `overall_status`: Verdict (`passed` or `failed`).

4. **Immutable Build Inputs and Cache Stamps**:
   - Runner caches in CI and local build stamps (`target/conformance-build/.pbrs-protobuf-pin`) are keyed by both `vendor/google/PIN` and `vendor/google/SHA`, not just the version tag.
   - Any change to upstream sources forces a clean runner rebuild, preventing stale runner binaries from masking regressions.

---

### 7.2 Required Profile Pass Criteria

For a build or release to qualify, all required profile test suites must meet the following non-negotiable pass criteria:

#### 1. gRPC Interoperability Pass Criteria
* **Zero Failures**: All evaluated cases in the target profile must achieve `status: passed` with exit code `0`.
* **Complete Peer Matrix**:
  * All 14 base interop cases must pass in both peer directions against the pinned `grpc-go` reference peer (`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`):
    1. Kernel Client $\to$ `grpc-go` Server
    2. `grpc-go` Client $\to$ Kernel Server
  * Self-tests (`kernel_client_to_kernel_server`) verify internal consistency but can **never substitute** for independent cross-language peer verification.
* **No Masked Flakes (`--strict`)**:
  * Any case that failed attempt 1 and passed on a retry is flagged as flaky and rejected. A required qualification gate must be 100% first-attempt clean.
* **Strict Assertion Conformance**:
  * Exact upstream protocol behavior must be observed (e.g. `cancel_after_first_response` must terminate with `CANCELLED`, not clean EOF; compression flags must be verified).

#### 2. Protobuf Conformance Pass Criteria
* **Full Required Coverage**:
  * Required run 1 and required run 2 must each pass 100% of official test cases (5,631 successes, 0 skipped, 0 expected failures, 0 unexpected failures) up to Edition 2023.
* **Deterministic Repeatability**:
  * Required run 2 must produce identical results to required run 1. Any non-deterministic field ordering or state leakage between runs fails the gate.
* **Enforce Recommended**:
  * The recommended conformance run must pass cleanly with `--enforce_recommended` without relying on skip lists or known failure lists.

#### 3. Release Publication Gating
* **Reusable CI Gate**: `.github/workflows/release.yml` requires `.github/workflows/ci.yml` via `workflow_call:` before the `publish` job can execute.
* **Fail-Closed Publishing**: A failure in `grpc-interop`, `conformance`, or any other required CI lane permanently halts publication for that SHA. No dry-run or live publish to crates.io can proceed without verified, retained CI evidence.
