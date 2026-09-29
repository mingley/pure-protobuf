# Official Interoperability and Conformance Case Registry

This directory is the map of every official Protobuf and gRPC test case tracked by `pure-protobuf` and `pbrs-grpc`. Run the main gRPC interoperability matrix with `./scripts/grpc-interop.sh`; run Protobuf conformance and HTTP/2 probes with the commands listed below. These tests do not yet prove the full profile: original HTTP/2 negatives, original server probes, full-duration soaks, backoff, scaling, cloud auth, Application Layer Transport Security (ALTS), Open Request Cost Aggregation (ORCA), xDS, and performance worker cases keep their recorded statuses below.

The registry lives in `cases.json`. It records the upstream source, peer direction, transport, profile, owner task, current disposition, and available evidence for each case.

## Start here

| Need | Command or file | What it proves | Main caveat |
|---|---|---|---|
| Standard gRPC peer interop | `./scripts/grpc-interop.sh` | Native client/server self-test plus the required base-case matrix against pinned `grpc-go`. | Compression is not run against `grpc-go` because that peer ignores compression flags. |
| Protobuf conformance | `./scripts/conformance.sh` | Official Protobuf `v35.1` required and recommended conformance behavior through Edition 2023. | It is separate from gRPC transport interop. |
| Native HTTP/2 server probes | `./scripts/grpc-http2-server-interop.sh` | Local TLS/framing probes plus an attempted original Go probe run against `pbrs-grpc-interop-server`. | Local passes are spec-derived adapters; unavailable original runners are explicit `not_run`, never assumed. |
| C++ compression peer | `./scripts/grpc-interop-cpp.sh` | Both native/C++ directions for 14 baseline cases plus 4 compression cases. | The C++ peer must be the pinned `grpc/grpc` build and digest-recorded binary. |
| Case and evidence registry | `cases.json` | The single source of truth for case status, profile, direction, and evidence. | A local adapter pass cannot relabel an unresolved original upstream procedure. |

## 1. Pinned Upstream Specifications and Tools

All test definitions, procedures, schemas, and runners are pinned to immutable upstream commits.

| Upstream project | Pin / commit | Relevant contracts and documents |
|---|---|---|
| **`protocolbuffers/protobuf`** | `v35.1`<br>[`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`](https://github.com/protocolbuffers/protobuf/tree/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03) | [Conformance Guide](https://github.com/protocolbuffers/protobuf/blob/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/conformance/README.md), [Rust Shared Tests](https://github.com/protocolbuffers/protobuf/tree/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/rust/test/shared), local `vendor/google/PIN`, and local `vendor/google/SHA`. |
| **`grpc/grpc`** | [`d1487957db6658bc532b72871775148229836627`](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627) | [Interop Test Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/interop-test-descriptions.md), [HTTP/2 Negative Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/http2-interop-test-descriptions.md), [Connection Backoff Description](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/connection-backoff-interop-test-description.md), [xDS Test Descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/xds-test-descriptions.md), [Official Runner](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/run_interop_tests.py), [Performance Framework](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/performance/README.md), `WorkerService`, and `BenchmarkService`. |
| **`grpc/grpc-go`** | [`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`](https://github.com/grpc/grpc-go/tree/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef) | [Interop Client](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/interop/client/client.go) and [Interop Server](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/interop/server/server.go) reference implementations. |
| **`grpc/proposal`** | [`6342be729b96478a2897ceb208a8cddcd832a17b`](https://github.com/grpc/proposal/tree/6342be729b96478a2897ceb208a8cddcd832a17b) | [gRFC A6: Client Retries](https://github.com/grpc/proposal/blob/6342be729b96478a2897ceb208a8cddcd832a17b/A6-client-retries.md): transparent retry, retry commitments, and buffering. |

## 2. Registry Schema (`cases.json`)

`cases.json` is the machine-readable registry. Keep it aligned with the upstream pins above and with the execution evidence produced by the harnesses.

### Top-Level Metadata

| Field | Meaning |
|---|---|
| `version` | Semantic version of the registry format, such as `1.0.0`. |
| `updated_at` | ISO 8601 date of the last registry reconciliation. |
| `title` | Descriptive registry name. |
| `description` | Registry scope and purpose. |
| `upstream_pins` | Exact commit hashes and canonical documentation paths for all upstream dependencies. |
| `disposition_definitions` | Formal definitions of allowed disposition states. |
| `suites` | Dictionary from suite keys to descriptions. |
| `summary` | Aggregate counts by disposition and by suite. |
| `cases` | Array of individual case records. |

### Case Record Schema

Every `cases` entry has these fields.

| Field | Meaning |
|---|---|
| `case` | Unique case identifier. It matches CLI `-test_case` arguments or official suite identifiers, such as `empty_unary` or `rst_after_header`. |
| `name` | Human-readable case name. |
| `suite` | Suite classification key. See [Test Suites Covered](#3-test-suites-covered). |
| `procedure_source` | Exact pinned document path or source runner reference that describes the procedure. |
| `peer_direction` | Direction of execution: `both`, `client_to_server`, `runner_to_binary`, `driver_to_worker`, or `internal`. |
| `transport` | Protocol and wire transport: `http2_cleartext`, `http2_tls`, `http2_mtls`, `alts`, `pipe`, or `none`. |
| `profile` | Shipping or qualification profile: `core`, `native`, `tonic`, `full`, or `extended`. |
| `owner` | Authoritative task ID in `docs/plan/tasks.json`. |
| `disposition` | Current qualification disposition for the original pinned procedure. See [Dispositions](#disposition-rules). The in-repo [JSON schema](cases.schema.json) lists all six states; `scripts/interop-report.py` checks unique cases and exact summary totals without an extra dependency. |
| `justification` | Required explanation for every non-`passed` disposition. It may be `null` for `passed`. |
| `present_coverage.status` | Current local or original-runner status: `passed`, `failed`, `not_run`, `unsupported`, `blocked_external`, or `not_applicable`. |
| `present_coverage.evidence_scope` | `local_adapter` explicitly marks a local pass that cannot qualify an unresolved original procedure. The field is omitted otherwise. |
| `present_coverage.evidence_file` | Path to the script, harness, or test file providing current evidence. |
| `present_coverage.passing_directions` | Verified peer directions currently passing. |
| `present_coverage.notes` | Extra technical context, limitations, or pending work. |

`peer_direction` values mean:

| Value | Meaning |
|---|---|
| `both` | Both client and server implementations are tested against peers. |
| `client_to_server` | The client executes the procedure against the server. |
| `runner_to_binary` | An external conformance runner talks to a binary over standard input and output. |
| `driver_to_worker` | A performance driver orchestrates workers. |
| `internal` | The case is an internal semantic test suite. |

### Disposition Rules

Every active upstream case has one explicit disposition.

| Disposition | Meaning |
|---|---|
| `passed` | The pinned procedure has passing independent evidence for its required scope. |
| `failed` | The original pinned procedure ran and failed subcases. Diagnose runner drift versus product defects without relabeling a local adapter pass. |
| `not_run` | The active upstream procedure is supported or planned, but it has not yet run in the official harness. |
| `unsupported` | The target profile does not yet implement the protocol feature or procedure. |
| `blocked_external` | External infrastructure, provider identity, or cloud secrets block the case. Missing credentials must always be `blocked_external`, never `not_applicable`. |
| `not_applicable` | The upstream test or kernel internal is explicitly excluded with approved technical justification, such as C/upb arena memory layouts. |

Important status boundaries:

* The eight HTTP/2 client negatives remain `not_run` for the original upstream Twisted/Python-2 runner.
* Those same eight cases have `present_coverage.status=passed` and `evidence_scope=local_adapter` for the separate 8/8 local peer exercise.
* Both server probes are `failed` under the original Go runner: framing 5/6 and TLS 0/3.
* The same server probes also have 2/2 separate local probes.
* Both full-duration soaks are `not_run`, even though their local adapters pass deterministic and qualification-scale tests.
* An original-procedure case cannot be reported `passed` while its registry disposition is unresolved.
* `--spec-adapter` is accepted only for the two named native HTTP/2 suites and their explicit local or external adapter peer identities.
* JSON reports for `--spec-adapter` mark `evidence_scope=spec_derived_adapter` and `qualification.qualified=false`, even when the adapter matrix passes.

## 3. Test Suites Covered

The registry contains 69 cases in 12 suites.

| Suite | Cases | Current summary |
|---|---:|---|
| `standard_interop` | 16 | Base unary, streaming, metadata, status, unimplemented, `pick_first_unary`, and `cacheable_unary` accounting. |
| `compression_interop` | 4 | Passed in self-interop and both directions against the pinned C++ peer. Skipped against `grpc-go`. |
| `http2_negative` | 8 | Local spec-derived adapter passes 8/8; original upstream runner remains `not_run`. |
| `server_probe` | 2 | Local TLS/framing probes pass separately; original Go probes remain failed at framing 5/6 and TLS 0/3. |
| `connection_backoff` | 1 | Scheduled in `FL-05`. |
| `soak` | 2 | Local adapters pass deterministic and qualification-scale tests; original full-duration procedures remain `not_run`. |
| `scaling` | 1 | Scheduled in `EX-21`. |
| `auth` | 7 | Cloud auth is `blocked_external`; ALTS is `unsupported`. |
| `orca` | 2 | Scheduled in `EX-01` and `EX-02`. |
| `xds_lb` | 12 | Scheduled in `FL-04` and `EX-07` through `EX-16`. |
| `protobuf_conformance` | 6 | Official Protobuf conformance and Rust shared application coverage, plus an explicit C/upb internal exclusion. |
| `performance` | 8 | WorkerService control protocol and BenchmarkService data plane cases. |

### 1. Standard Interoperability (`standard_interop`, 16 cases)

These are baseline gRPC remote procedure call (RPC) patterns and protocol semantics from `doc/interop-test-descriptions.md`.

| Group | Cases | What they check |
|---|---|---|
| Base unary and streaming | `empty_unary`, `large_unary`, `client_streaming`, `server_streaming`, `ping_pong`, `empty_stream` | Basic request/response and stream behavior. |
| Flow control and lifecycle | `cancel_after_begin`, `cancel_after_first_response`, `timeout_on_sleeping_server` | Cancellation, deadlines, and lifecycle status. |
| Metadata and errors | `custom_metadata`, `status_code_and_message`, `special_status_message` | Header/trailer metadata and exact status propagation. |
| Unimplemented handling | `unimplemented_method`, `unimplemented_service` | Required `UNIMPLEMENTED` behavior. |
| Extended runner case | `pick_first_unary` | Subchannel `pick_first` validation. |
| Spec-only experimental case | `cacheable_unary` | HTTP/2 GET through a caching proxy. It is omitted from the active runner; its standard gRPC over HTTP/2 disposition is [not applicable](../../docs/cacheable-rpc.md), and the native kernel still requires POST. |

### 2. Compression Interoperability (`compression_interop`, 4 cases)

These cases verify message-level compression negotiation and framing.

| Case | Status and caveat |
|---|---|
| `client_compressed_unary` | Passes in the 18-case self-interop pass and both C++ peer directions. |
| `server_compressed_unary` | Passes in the 18-case self-interop pass and both C++ peer directions. |
| `client_compressed_streaming` | Passes in the 18-case self-interop pass and both C++ peer directions. |
| `server_compressed_streaming` | Passes in the 18-case self-interop pass and both C++ peer directions. |

Full status: `scripts/grpc-interop.sh` passes the 18-case self-interop pass. `scripts/grpc-interop-cpp.sh` passes both cross-peer directions, for 36 cells, against the pinned C++ reference peer (`grpc/grpc@d1487957`, v1.84.0) in task `IO-05`. The reference peer asserts wire compression bits.

The native client adds a high-entropy compressed unary leg after the three official calls. It does not replace the zero-filled official vector. Cross-language execution against `grpc-go` is skipped because `grpc-go` ignores compression flags.

### 3. HTTP/2 Negative Tests (`http2_negative`, 8 cases)

These adversarial framing, stream cancellation, and connection termination cases come from `doc/http2-interop-test-descriptions.md`.

| Case group | Cases | Current status |
|---|---|---|
| Reset behavior | `rst_after_header`, `rst_after_data`, `rst_during_data` | Local adapter passes; original upstream procedure remains `not_run`. |
| Connection behavior | `goaway`, `ping`, `max_streams` | Local adapter passes; original upstream procedure remains `not_run`. |
| Padding behavior | `data_frame_padding`, `no_df_padding_sanity_test` | Local adapter passes; original upstream procedure remains `not_run`. |

`scripts/grpc-http2-interop.sh` runs all eight registered procedures against a purpose-built local HTTP/2 peer and keeps a required matrix report. This is a spec-derived adapter, not execution of the upstream runner binary. The independent-peer qualification in `IO-08` remains open, and the original procedures are registered `not_run`.

A fake successful client with no peer frames is recorded as failed by `test_http2_peer_proof.py`. Reports link both client and local-peer logs. In-tree hostile tests are complementary.

The pinned upstream `grpc/grpc@d1487957` HTTP/2 server imports Twisted and still calls Python 2's `dict.has_key` in `test/http2_test/http2_test_server.py`. Current CI does not have a reviewed, pinned toolchain for that original runner. The local spec-derived peer is valuable regression coverage, but it cannot replace the full-profile result.

### 4. Server Probes (`server_probe`, 2 cases)

<a id="4-server-probes"></a>

These official server transport probes come from `tools/run_tests/run_interop_tests.py`.

| Probe | What it checks | Current status |
|---|---|---|
| `server_tls_probe` | ALPN negotiation for `h2`, rejection of invalid ALPN `http/1.1`, TLS 1.2/1.3 protocol versions and AEAD ciphers, server certificate presentation, and live TLS gRPC RPC. | Local spec-derived probe passes; original Go probe remains failed at TLS 0/3. |
| `server_framing_probe` | HTTP/2 24-byte connection preface, SETTINGS exchange and ACK, rapid reset stream cancellation flood (CVE-2023-44487), small DATA frames flow control, fragmented HEADERS across CONTINUATION frames and CONTINUATION flood protection, bad headers, non-POST HTTP 405, unsupported media type HTTP 415, and post-probe server health. | Local spec-derived probe passes; original Go probe remains failed at framing 5/6. |

`scripts/grpc-http2-server-interop.sh` runs two phases against `pbrs-grpc-interop-server`: spec-derived local TLS and framing probes (peer `local-native-server`), then the original upstream Go probes (peer `grpc-http2-probe`) through the fail-closed [diagnostic driver](../../scripts/grpc-http2-upstream-server-interop.py). The local phase decodes the response `:status` and requires HTTP 405/415, rather than accepting any HEADERS frame. Each phase writes its own required result rows and retained logs.

The upstream phase maps the official runner's separate server modes to the native server one-to-one:

| Official probe mode | Subcases asserted | Native probe row | Transport |
|---|---|---|---|
| `framing` (`-test_case=framing`) | `TestSoonClientShortSettings`, `TestSoonShortPreface`, `TestSoonUnknownFrameType`, `TestSoonClientPrefaceWithStreamId`, `TestSoonSmallMaxFrameSize`, `TestSoonAllSettingsFramesAcked` | `server_framing_probe` | `http2_cleartext` |
| `tls` (`-test_case=tls`) | `TestSoonTLSApplicationProtocol`, `TestSoonTLSMaxVersion`, `TestSoonTLSBadCipherSuites` | `server_tls_probe` | `http2_tls` |

The probe binary is built standard-library-only (`GO111MODULE=off`) from `tools/http2_interop` in the pinned checkout, so no Go module downloads are involved. The framing mode dials a cleartext native server; the TLS mode dials a TLS native server serving the upstream test credentials with ALPN `h2` verified against the upstream test CA for `foo.test.google.fr`. Each row records the Go version, the probe and native binary SHA-256 digests, the native git SHA and dirty flag, and the per-mode raw and server logs.

When the runner prerequisites are unavailable, the upstream phase records both probes as `not_run` with the exact reason and repro steps in the row notes and console output, and the required-profile aggregate fails. Local hostile tests (`pbrs-grpc/tests/hostile.rs`) and TLS tests (`pbrs-grpc/tests/tls.rs`) are complementary and are never recorded as probe results.

Exact upstream prerequisites:

* Host toolchain: Go exactly as pinned in `tests/interop/go/go.mod` (currently 1.25.3), Python 3 standard library only, and a cargo cache warm enough for the diagnostic's `cargo build --offline --locked` debug build of `pbrs-grpc-interop-server`.
* Pinned checkout: a clean `third_party/grpc` at `d1487957db6658bc532b72871775148229836627` with no tracked, untracked, or ignored changes under `tools/http2_interop` or `src/core/tsi/test_creds`. No submodules are needed for the Go probe build.
* Native provenance: a clean HEAD for `src` and `pbrs-grpc`; a dirty tree (or a cached binary via `--skip-rust-build`) is explicitly unqualified.

Reproduce the upstream run from a fresh checkout:

```bash
mkdir -p third_party
git init third_party/grpc
git -C third_party/grpc remote add origin https://github.com/grpc/grpc.git
git -C third_party/grpc fetch --depth 1 origin d1487957db6658bc532b72871775148229836627
git -C third_party/grpc checkout FETCH_HEAD
./scripts/grpc-http2-server-interop.sh --upstream-only
```

Premise check on 2026-09-29 (this host): no `python2`, no Twisted module, no `third_party/grpc`, no `h2spec`/`nghttpd`; Go 1.25.3 present and matching the pin. The original probes are therefore `not_run` here until the pinned checkout above is provided. The Python-2/Twisted runner is the client-negative (IO-08) peer, not the server-probe runner; the server probes need only the Go toolchain plus the pinned checkout.

That harness:

* builds the pinned, standard-library-only Go test binary and native debug server;
* rejects tracked, untracked, or ignored files in the pinned probe and test-credential directories;
* verifies the upstream test certificate authority (CA), server name, and ALPN `h2`;
* retains per-mode raw logs, binary hashes, and JSON under `target/interop-logs/`;
* requires all six framing subcases and all three TLS subcases to pass.

Upstream `TestMain` can return exit **0** even if advisory `TestSoon*` cases fail. A cached native binary selected with `--skip-rust-build` remains explicitly unqualified.

The **2026-09-24 local macOS Go 1.25.3 run** remains framing **5/6** and TLS **0/3**. A separate standard-library socket diagnostic on the cached, unqualified native binary (SHA-256 `a4fa7a289441c8addc956e1e972e4ac8dde1821f0ea419a602260c444a79c251`) received initial SETTINGS, then GOAWAY with code 1 (`PROTOCOL_ERROR`), then EOF for `SETTINGS_MAX_FRAME_SIZE=16383`; a 16384 control received a SETTINGS ACK.

The pinned Go `parseFrame` does not construct `GoAwayFrame`. `TestSoonSmallMaxFrameSize` expects the string `Got goaway frame` even though its helper returns nil after a GOAWAY. Its EOF is therefore not evidence of a missing server GOAWAY, and the original Go result is **not** relabeled as passed.

The TLS cases likewise do not demonstrate a server flaw:

* `h2c` ALPN receives `no_application_protocol`, not the old test's `EOF` or `broken pipe` text.
* The TLS 1.1 case has contradictory Go client MinVersion TLS 1.2 and MaxVersion TLS 1.1, so it errors before connecting.
* The bad-TLS-1.2-cipher test leaves TLS 1.3 enabled.

A verified standard-library TLS handshake with one banned TLS 1.2 cipher (`AES128-SHA`) was rejected when forced to TLS 1.2. The same check negotiated TLS 1.3 AEAD and `h2` when TLS 1.3 was allowed. This does not prove rejection of every weak cipher or acceptance of TLS 1.1.

The original upstream profile and `IO-09` stay open. A reviewed, version-compatible original-probe qualification and a fresh clean native build are still needed.

A read-only upstream audit on 2026-09-24 compared all 15 files under `tools/http2_interop/` at the [pinned source](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627/tools/http2_interop), released [`v1.84.0`](https://github.com/grpc/grpc/tree/3252a89f10d8e92997862167ca7d095ecda85973/tools/http2_interop), and [current-master snapshot](https://github.com/grpc/grpc/tree/88f984bbbd15b0223c95e56d9944357e665b3818/tools/http2_interop). Their blob hashes match. There is **no corrected official runner** in those revisions.

A valid GOAWAY check must do all of this:

* dispatch the existing [`GoAwayFrame` decoder](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/http2_interop/goaway.go#L23-L58) from [`parseFrame`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/http2_interop/http2interop.go#L51-L80);
* reconcile the [helper's nil-on-GOAWAY return](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/http2_interop/s6.5.go#L22-L46) with the [test's required error string](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/http2_interop/s6.5_test.go#L21-L29);
* assert the decoded frame and error code, not EOF.

TLS 1.1 needs compatible client minimum **and** maximum versions under [Go 1.25's TLS checks](https://github.com/golang/go/blob/28622c19591d95c9a83f706f2ed1b303d58da85f/src/crypto/tls/common.go#L1156-L1194). The bad-cipher case must constrain [TLS 1.3 away](https://github.com/golang/go/blob/28622c19591d95c9a83f706f2ed1b303d58da85f/src/crypto/tls/common.go#L692-L701) to test TLS 1.2 suites.

A local patch is an **unofficial adapter**, not an original-runner pass. Seek an upstream-reviewed correction before pinning a new runner. Maintain framing 5/6, TLS 0/3, and the fail-closed full-profile result meanwhile.

### 5. Connection Backoff (`connection_backoff`, 1 case)

`connection_backoff` is the full, approximately 540-second reconnect backoff, jitter, and retry-cap exercise against the official C++ `ReconnectService`. It is defined in `doc/connection-backoff-interop-test-description.md` and scheduled in task `FL-05`.

### 6. Soak Testing (`soak`, 2 cases)

These long-running reliability and resource stability cases come from `run_interop_tests.py`.

| Case | What it checks | Status |
|---|---|---|
| `rpc_soak` | Sustained high-iteration RPC loop, latency, and error budget over a long window. | Original full-duration procedure remains `not_run`. |
| `channel_soak` | Repeated channel creation, connection churn, and teardown under load. | Original full-duration procedure remains `not_run`. |

Adapters are exercised by `pbrs-grpc/src/interop_cases.rs`, the `pbrs-grpc-interop-client` soak flags, and `tests/interop/test_soak.py`. They include 12/12 deterministic tests plus local qualification-scale runs. Their original full-duration procedures remain registered `not_run` for the scheduled operator campaign; `IO-10`'s adapter deliverable is distinct from that campaign.

### 7. Stream Scaling (`scaling`, 1 case)

`max_concurrent_streams_connection_scaling` checks subchannel scaling when the peer reaches its `SETTINGS_MAX_CONCURRENT_STREAMS` limit. It is scheduled in task `EX-21`.

### 8. Authentication & Credentials (`auth`, 7 cases)

These cases come from `run_interop_tests.py`.

| Case group | Cases | Status |
|---|---|---|
| Cloud identity and call credentials | `compute_engine_creds`, `jwt_token_creds`, `oauth2_auth_token`, `per_rpc_creds`, `google_default_credentials`, `compute_engine_channel_credentials` | `blocked_external` pending an approved GCP environment in task `EX-17`. |
| Application Layer Transport Security (ALTS) | `alts_credentials` | `unsupported` per the `EX-18` boundary decision; blocked on leaf cards `EX-18a`/`EX-18b` (see `docs/alts-contract.md`). |

### 9. ORCA (`orca`, 2 cases)

Open Request Cost Aggregation (ORCA) backend load reporting comes from gRFC A51.

| Case | What it checks | Status |
|---|---|---|
| `orca_per_rpc` | Per-call backend load metrics in trailing metadata. | Scheduled in task `EX-01`. |
| `orca_oob` | Out-of-band load reporting stream over `OpenRcaService`. | Scheduled in task `EX-02`. |

### 10. xDS & Load Balancing (`xds_lb`, 12 cases)

These client-side service mesh and dynamic traffic routing cases come from `doc/xds-test-descriptions.md`.

| Group | Cases | Status |
|---|---|---|
| Load balancing and backend behavior | `round_robin`, `backends_restart`, `circuit_breaking`, `outlier_detection` | Scheduled in `FL-04` and `EX-07` through `EX-16`. |
| Routing and policy behavior | `xds_ping_pong`, `traffic_splitting`, `path_matching`, `header_matching`, `fault_injection`, `timeout`, `metadata_exchange`, `app_net_security` | Scheduled in `FL-04` and `EX-07` through `EX-16`. |

### 11. Protobuf Conformance (`protobuf_conformance`, 6 cases)

These cases cover official Protobuf v35.1 conformance and Rust application semantics.

| Case | What it checks | Status note |
|---|---|---|
| `protobuf_binary_conformance` | Required binary wire format conformance. | 5,631 tests. |
| `protobuf_json_conformance` | Proto3 and Edition 2023 JSON mapping conformance. | Tracked in the official conformance suite. |
| `protobuf_text_conformance` | Text format conformance. | 909 tests. |
| `protobuf_enforce_recommended` | Recommended conformance suite with `--enforce_recommended`. | Passed without `failure_list_rust_upb.txt` skips. |
| `rust_shared_application_tests` | In-tree port of `rust/test/shared/` accessors, merge, and serialize in `tests/google_shared.rs`. | Standalone external runner scheduled in `PB-02`. |
| `upb_kernel_internals` | C upb arena layout internals. | Explicit `not_applicable`; they do not apply to the safe pure-Rust runtime. |

### 12. Performance Framework (`performance`, 8 cases)

These cases come from `tools/run_tests/performance/README.md`.

| Area | Cases | Tasks |
|---|---|---|
| `WorkerService` control protocol | `worker_service_run_server`, `worker_service_run_client`, `worker_service_core_count`, `worker_service_quit_worker` | `BM-09`, `BM-10` |
| `BenchmarkService` data plane | `benchmark_service_unary`, `benchmark_service_streaming`, `benchmark_service_streaming_from_client`, `benchmark_service_streaming_from_server` | `BM-08` |

## 4. How Cases Are Run

### gRPC Interoperability (`scripts/grpc-interop.sh`)

The standard interop script executes three passes.

| Pass | Direction | Cases |
|---|---|---|
| Native self-interop | `pbrs-grpc-interop-client` to `pbrs-grpc-interop-server` | All 18 cases: 14 base cases plus 4 compression cases. |
| Native client to Go server | `pbrs-grpc-interop-client` to `google.golang.org/grpc/interop/server` | The 14 base cases against official `grpc-go`. |
| Go client to native server | `google.golang.org/grpc/interop/client` to `pbrs-grpc-interop-server` | The 14 base cases against official `grpc-go`. |

Invocation:

```bash
# Run all passes (requires Go toolchain):
./scripts/grpc-interop.sh

# Run self-interop pass only:
./scripts/grpc-interop.sh --self-only
```

### Protobuf Conformance (`scripts/conformance.sh`)

This script builds Google's official `conformance_test_runner` from pinned `v35.1` (`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`) and runs:

1. Required run 1: `--maximum_edition 2023`
2. Required run 2: `--maximum_edition 2023`
3. Recommended run: `--enforce_recommended --maximum_edition 2023`

Invocation:

```bash
./scripts/conformance.sh
```

### Server HTTP/2 Framing and TLS Probes (`scripts/grpc-http2-server-interop.sh`)

The server probe harness runs in two phases. The local phase verifies native server framing, transport security, and protocol resilience against simulated client probes. The upstream phase executes the official upstream server test probes from `tools/http2_interop` against a fresh native server and records the outcome per probe.

#### 1. Probe Contract and Architecture

The local phase starts two native `pbrs-grpc-interop-server` processes.

| Server | Transport | Important flags |
|---|---|---|
| TLS instance | Dynamic TLS port | `--use_tls=true`, `--tls_cert_file=pbrs-grpc/tests/tls_data/server.crt`, `--tls_key_file=pbrs-grpc/tests/tls_data/server.key` |
| Cleartext instance | Dynamic cleartext port | Cleartext framing and boundary probes. |

Both servers run as background jobs with process tracking, startup readiness checks, and clean shutdown traps on exit or interruption.

The upstream phase delegates execution to `scripts/grpc-http2-upstream-server-interop.py`, which builds the pinned Go probe binary and a native debug server, starts one native server per mode with retained logs, and writes a fail-closed `summary.json`. The shell wrapper maps that summary to one `interop-report.py` row per probe (peer `grpc-http2-probe`, peer pin `d1487957db6658bc532b72871775148229836627`), so the record format matches the local phase and the IO-08 client-negative runner: case, status, duration, peer, direction, transport, suite, profile, stdout/stderr logs, exit code, attempt count, and notes.

#### 2. Probe Test Matrix

| Probe | Checks |
|---|---|
| `server_tls_probe` (`http2_tls`) | ALPN `h2` negotiation; rejection of non-`h2` ALPN (`http/1.1`) with `TLSV1_ALERT_NO_APPLICATION_PROTOCOL`; certificate presentation; live TLS RPC using `pbrs-grpc-interop-client --use_tls=true --tls_ca_file=... --server_host_override=localhost --test_case=empty_unary`. |
| `server_framing_probe` (`http2_cleartext`) | 24-byte client preface; empty SETTINGS frame; server SETTINGS validation; two-way SETTINGS ACK; rapid HEADERS followed by RST_STREAM (CANCEL) across 32 streams for CVE-2023-44487 simulation; 1-byte DATA fragmentation; flow-control budget enforcement; HEADERS plus CONTINUATION assembly; CONTINUATION flood protection; non-POST HTTP 405 with `allow: POST`; `application/json` HTTP 415; post-probe `empty_unary` health check. |

The upstream phase asserts the official subcases mapped in [Server Probes](#4-server-probes): six `TestSoon*` framing subcases for `server_framing_probe` and three `TestSoonTLS*` subcases for `server_tls_probe`.

#### 3. Prerequisites

Local phase:

* Python 3 standard library only: `socket`, `ssl`, `struct`, `subprocess`, and `time`.
* Zero third-party pip dependencies.
* Compiled `pbrs-grpc-interop-server` and `pbrs-grpc-interop-client` binaries. The script builds them unless `--skip-build` is provided.
* Throwaway loopback test certificates in `pbrs-grpc/tests/tls_data/`.

Upstream phase (see [Server Probes](#4-server-probes) for the exact repro):

* Go exactly as pinned in `tests/interop/go/go.mod`.
* Clean `third_party/grpc` checkout at `d1487957db6658bc532b72871775148229836627`.
* Warm cargo cache for the diagnostic's offline locked debug build.

#### 4. Invocation

```bash
# Run both phases (local probes, then original upstream probes):
./scripts/grpc-http2-server-interop.sh

# Run specific probe (both phases record only that probe and fail the
# required matrix, proving a narrowed run cannot qualify):
./scripts/grpc-http2-server-interop.sh --cases=server_tls_probe
./scripts/grpc-http2-server-interop.sh --cases=server_framing_probe

# Local spec-derived phase only (explicitly narrowed scope):
./scripts/grpc-http2-server-interop.sh --skip-upstream

# Original upstream phase only (needs the pinned third_party/grpc checkout):
./scripts/grpc-http2-server-interop.sh --upstream-only

# Skip cargo build and specify custom log directory:
./scripts/grpc-http2-server-interop.sh --skip-build --log-dir=target/interop-logs/custom
```

Execution traces and logs go under `target/interop-logs/`. The local phase records `results.json`, validates against `cases.json` with `--suite server_probe --profile native --spec-adapter --require-matrix`, and aggregates into `report.json`. The upstream phase records `upstream-results.json`, validates without `--spec-adapter` (these rows claim original-procedure execution), and aggregates into `upstream-report.json`. Both phases require fresh, distinct report paths: a prior report cannot fill a missing row in a later run.

Exit status and gating:

* Exit `0` only when every executed phase passed and aggregated cleanly.
* A run that omits a probe, hits an unexpected failure, or finds the upstream runner unavailable exits non-zero and cannot qualify.
* Upstream driver exit `0` maps qualified modes to `passed`; exit `1` maps unqualified modes to `failed` with the failure list in the row notes; exit `2` (missing prerequisites or aborted run) maps both probes to `not_run` with the exact reason and repro steps. A missing or unparseable upstream summary after exit `0`/`1` is recorded `failed`, never passed.
* A future genuine upstream pass still fails validation until the coordinator reconciles the `failed` registry dispositions in `cases.json`: a pass cannot be reported while the registry disposition is unresolved.

Local hostile tests in `pbrs-grpc/tests/hostile.rs` and TLS tests in `pbrs-grpc/tests/tls.rs` are complementary. They never substitute for these probe records.

CI runs the required local gate (`--skip-upstream`) as a blocking step and the original upstream phase (`--upstream-only`) as an advisory step whose `not_run`/`failed` outcome and retained logs stay visible without blocking other lanes while full-profile qualification is open. Both reports are uploaded as artifacts (`grpc-interop-report` and `grpc-interop-upstream-report`).

## 5. Machine-Readable Proof, Reporting, and Aggregation (`scripts/interop-report.py`)

`scripts/interop-report.py` is a standard-library-only Python 3 tool. It validates, aggregates, and writes machine-readable proof for interoperability and conformance runs. It has zero third-party dependencies.

### 5.1 Verification States

| State | Meaning |
|---|---|
| `passed` | Verified passing in official test scripts, peer passes, or conformance harness. |
| `failed` | Test execution failed, exited non-zero, or violated protocol assertions. |
| `not_run` | Active upstream case is scheduled but has not yet run in the official harness. |
| `unsupported` | Protocol feature, RPC pattern, or service is not yet implemented in the target profile. |
| `blocked_external` | External infrastructure or credentials block the case, such as cloud IAM or GCP metadata service. Never mark this `not_applicable`. |
| `not_applicable` | Explicitly excluded upstream test or kernel internal with approved technical justification, such as C/upb arena memory layouts. |

### 5.2 CLI Invocations and Usage

#### 1. Aggregate and Report (Default Mode)

This mode processes execution results, validates them against `cases.json`, prints a terminal or markdown summary, and writes `report.json`.

```bash
# Validate, aggregate, print terminal table, and write report.json
python3 scripts/interop-report.py --cases tests/interop/cases.json --results results.json --output report.json

# Output markdown summary table (e.g. for CI job summaries or PR comments)
python3 scripts/interop-report.py --results results.json --format markdown

# Pipe results from stdin
cat results.json | python3 scripts/interop-report.py --results - --format json
```

#### 2. Validate Only

This mode validates matrix definitions, upstream commit pins, and disposition rules without generating reports.

```bash
python3 scripts/interop-report.py validate --cases tests/interop/cases.json --results results.json --require-matrix --require-peers
```

#### 3. Record Individual Case Execution

Shell harnesses use this mode to append or update one test execution entry in a results file.

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

| Option | Meaning |
|---|---|
| `--cases <path>` | Path to `cases.json`. Default: `tests/interop/cases.json`. |
| `--results <path>` | Path to input execution results JSON. `-` reads from standard input. |
| `--output, -o <path>` | Destination path for machine-readable `report.json`. |
| `--format {terminal,markdown,json}` | Output display format. Default: `terminal`. |
| `--suite <name>` | Filter and enforce requirements for a suite, such as `standard_interop`. |
| `--profile <name>` without `--suite` | Require the entire profile, including unresolved upstream failures and missing original procedures. The `native` profile currently fails qualification. |
| `--suite <name> --profile <name>` | Require a scoped matrix, such as CI's standard-interop direction subset. This is not a full-profile pass. |
| `--require-all` | Fail if any selected case is missing or has an unqualified registry disposition. Applies to `validate` or `aggregate`. |
| `--spec-adapter` | Check only the explicitly scoped native HTTP/2 spec-derived client or server probes, including an explicitly labeled external HTTP/2 peer. A pass is **not** an original upstream or full-profile qualification. |
| `--require-matrix` | Fail if any expected direction from `cases.json` is missing. |
| `--require-peers` | Fail if independent peer execution is missing or substituted with self-test. |
| `--strict` / `--no-strict` | Enforce strict retry checking. Default `--strict` fails if retries hid initial failure. |
| `--no-color` | Disable ANSI color escape codes in terminal output. |

### 5.4 Exit Codes

| Exit code | Meaning |
|---:|---|
| `0` | Passed / qualified: all evaluated cases passed, all matrix requirements were met, pins were verified, and no failure occurred. |
| `1` | Test failure: one or more cases had `failed` status or non-zero exit codes. |
| `2` | Validation / matrix error: missing required cases, duplicate matrix rows, wrong peer pins, wrong procedure sources, empty output, retries hiding first failures, or unauthorized self-test substitution. |
| `3` | Usage / IO error: bad arguments, missing input files, or unreadable JSON syntax. |

### 5.5 Input Results Schema (`results.json`)

Input results may be a JSON array or an object with a `"results"` array.

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

The generated report links raw logs, exit codes, exact upstream pins, suite aggregates, and profile metrics.

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

Aggregation fails closed with status `failed` and exit code 1 or 2 when any rule is violated.

| Rule | Failure condition |
|---|---|
| Empty output | Results contain 0 records or an empty payload. |
| Missing cases | Any case required for qualification is omitted. |
| Skipped cases | Any required case is marked `not_run`. |
| Duplicate matrix rows | Two records share identical `(case, peer, direction, transport)` coordinates. |
| Wrong upstream pins | `peer_pin` does not match the pinned upstream commit, such as `grpc-go` at `dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`, or `procedure_source` does not match `cases.json`. |
| Retries hiding first failures | In strict qualification mode, any case that passed on a retry after failing attempt 1 is rejected. A flaky first attempt is recorded and cannot satisfy a required qualification gate. |
| Self-test substitution | For `peer_direction: "both"` cases, `kernel_client_to_kernel_server` can never substitute for independent peer passes: `kernel_client_to_go_server` and `go_client_to_kernel_server`. |

### 5.8 Integration with GT-04 (Diagnosable & Bounded Runs)

Task **GT-04** upgrades `scripts/grpc-interop.sh` and provides `tests/interop/test_runner.py`. The runner produces granular execution evidence consumed by `scripts/interop-report.py`.

| Capability | Behavior |
|---|---|
| Per-attempt log retention | The runner writes individual stdout/stderr logs for every attempt, such as `logs/<case>_attempt1.stderr` and `logs/<case>_attempt2.stdout`, instead of discarding output to `/dev/null`. |
| Attempt tracking | Each case emits `attempt_count` plus a detailed `attempts` list with exit codes and durations. |
| Flake transparency | Transient races, such as `cancel_after_begin` or `timeout_on_sleeping_server`, keep their initial failure. With `--strict`, the report surfaces the flake instead of masking it. |
| Deterministic gate pass/fail | GT-04 test completion calls `scripts/interop-report.py aggregate --results <scratch>/results.json --output <scratch>/report.json` and fails the runner whenever `interop-report.py` returns non-zero. |

## 6. Consumption by Subsequent Tasks

Later tasks use `cases.json` as the single source of truth.

| Task or lane | How it consumes the registry |
|---|---|
| **GT-02 (Machine-Readable Proof & Aggregation)** | Parameterizes `scripts/interop-report.py` and `tests/interop/test_report.py`; verifies required matrix rows; detects duplicates; enforces valid disposition transitions; guarantees aggregation fails closed when independent peer evidence is missing or replaced with self-test. |
| **GT-03 (Pin the Go Peer and Require It)** | Requires all 14 base interop cases in both directions against pinned `grpc-go` (`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`); enforces that compression cases cite `grpc-go`'s lack of support and that self-only runs are labeled insufficient. |
| **GT-04 (Diagnosable and Bounded Interop Runs)** | Uses case metadata to set timeouts, capture distinct attempt logs, track retries, and keep transient races visible. |
| **GT-05 (Require Retained Official Evidence in CI)** | Uploads full interop attempt logs, server logs, and machine-readable `report.json` even on failure with `if: always()` in `.github/workflows/ci.yml`; preserves required and recommended conformance outputs; records `summary.json` with runner pin, runner SHA, git commit, timestamp, and test counts; keys caches and stamps by immutable source inputs (`PIN` plus `SHA`); keeps reusable CI gating before release publication in `release.yml`. |
| **GT-06 (Detect Upstream and Dependency Drift Separately)** | Adds an opt-in or scheduled read-only compatibility lane that compares reviewed newer case inventories and toolchains against the pinned regression lane. |
| **IO-01 through IO-10 (Assertion Audits, TLS, Negative HTTP/2, C++ Peer, Soak)** | Updates `cases.json` dispositions from `not_run` to `passed` as each official adapter and harness is qualified. |
| **FL and EX lanes (Backoff, ORCA, Cloud Auth, xDS)** | Track progress and qualification evidence for advanced fleet and cloud features. |
| **QL-04 (Full Official-Gate Inventory Reconciliation)** | Performs final verification against `cases.json` so every active upstream case has verified `passed` disposition before any full-profile leadership claim is published. |

## 7. CI Artifact Retention Contract and Required Profile Pass Criteria

CI retains official interoperability and conformance evidence so passing runs are verifiable and failures are diagnosable without a local reproduction.

### 7.1 Artifact Retention Contract

All official test-suite jobs in `.github/workflows/ci.yml` must follow these rules.

| Area | Required behavior |
|---|---|
| Unconditional upload on failure | Every official-suite job uploads outputs, raw logs, and machine-readable reports through `actions/upload-artifact@v4` with `if: always()`. Failed, crashed, timed out, or cancelled runs must not discard diagnostic traces to `/dev/null` or exit without evidence. |
| gRPC interoperability artifacts | The `grpc-interop` job uploads `target/interop-logs/` as `grpc-interop-logs`. The directory contains per-attempt stdout/stderr logs for every executed case and direction (`<peer>-<direction>-<case>-attempt<N>.log`), background server lifecycle logs (`server-kernel.log`, `server-go.log`), and raw execution events (`results.json`). |
| gRPC proof report | The `grpc-interop` job uploads `target/interop-report.json` as `grpc-interop-report`. It is emitted by `scripts/interop-report.py` and includes overall pass/fail status, upstream commit pins (`grpc-go` at `dd51b1c90aaf`), per-suite and per-profile aggregates, individual case durations, attempt counts, exit codes, and failure justifications. |
| Protobuf conformance artifacts | The `conformance` job uploads `target/conformance-out/` as `conformance-out`, with separate outputs for `required_1/`, `required_2/`, `recommended/`, and `summary.json`. |
| Immutable build inputs and cache stamps | CI and local conformance caches, including the local build stamp `target/conformance-build/.pbrs-protobuf-pin`, use both `vendor/google/PIN` and `vendor/google/SHA`, not just the version tag. Any upstream source change forces a clean runner rebuild. |

`target/conformance-out/` contains:

| Path | Contents |
|---|---|
| `required_1/` | Failure lists, text mismatch files, and full runner log (`runner.log`) for required run 1 with `--maximum_edition 2023`. |
| `required_2/` | Output files and runner log (`runner.log`) for required run 2, proving deterministic repeatability. |
| `recommended/` | Output files and runner log (`runner.log`) for the recommended pass with `--enforce_recommended --maximum_edition 2023`. |
| `summary.json` | `runner_pin` such as `v35.1` from `vendor/google/PIN`; `runner_sha` such as `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` from `vendor/google/SHA`; the exact evaluated `pure-protobuf` `git_commit`; UTC ISO 8601 `timestamp`; `maximum_edition` `2023`; exact `test_counts` for `required_run_1`, `required_run_2`, and `recommended`; total failure counts; per-pass `runs` status, successes, skipped, expected failures, and unexpected failures; and `overall_status` as `passed` or `failed`. |

### 7.2 Required Profile Pass Criteria

For a build or release to qualify, all required profile suites must meet these criteria.

#### 1. gRPC Interoperability Pass Criteria

| Requirement | Detail |
|---|---|
| Zero failures | Every evaluated case in the target profile must have `status: passed` and exit code `0`. |
| Complete peer matrix | All 14 base interop cases must pass in both directions against pinned `grpc-go` (`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`): Kernel Client to `grpc-go` Server, and `grpc-go` Client to Kernel Server. |
| No self-test substitution | Self-tests (`kernel_client_to_kernel_server`) verify internal consistency but can never replace independent cross-language peer verification. |
| No masked flakes (`--strict`) | A case that failed attempt 1 and passed on retry is flaky and rejected. A required qualification gate must be 100% first-attempt clean. |
| Strict assertion conformance | Exact upstream protocol behavior must be observed. For example, `cancel_after_first_response` must terminate with `CANCELLED`, not clean EOF, and compression flags must be verified. |

#### 2. Protobuf Conformance Pass Criteria

| Requirement | Detail |
|---|---|
| Full required coverage | Required run 1 and required run 2 must each pass 100% of official tests: 5,631 successes, 0 skipped, 0 expected failures, and 0 unexpected failures, up to Edition 2023. |
| Deterministic repeatability | Required run 2 must match required run 1. Non-deterministic field ordering or state leakage fails the gate. |
| Enforce recommended | The recommended conformance run must pass cleanly with `--enforce_recommended` and without skip lists or known failure lists. |

#### 3. Release Publication Gating

* `.github/workflows/release.yml` requires `.github/workflows/ci.yml` through `workflow_call:` before `publish` can run.
* A failure in `grpc-interop`, `conformance`, or any other required CI lane permanently halts publication for that SHA.
* No dry-run or live crates.io publish can proceed without verified, retained CI evidence.
