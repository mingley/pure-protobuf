# Documentation Map and Navigation Architecture

This document fulfills task **DX-01** ("Design task-oriented documentation navigation") in the `pbrs` project. It establishes the master plan for transitioning from an assertion-pinned status inventory into a Diátaxis-aligned, task-oriented documentation suite.

---

## 1. Executive Summary & Scope

The current documentation across `pure-protobuf` mixes user onboarding, design rationale, historical test records, competitive benchmarking, and thousands of exact-string assertions into monolithic files. Most notably, `docs/grpc.md` (2,520 lines), `docs/status.md` (1,214 lines), `docs/architecture.md` (812 lines), and `pbrs-grpc/README.md` (502 lines) contain dense walls of combinatorial permutations (over 2,000 occurrences of the phrase `Distinct from...`), created specifically to satisfy exact-string assertions in `pbrs-grpc/tests/serving.rs`.

Additionally, multiple guides contain stale pre-publication claims (advising users to import crates via Git dependencies until they reach crates.io, despite `pbrs` 0.1.0 and adapter `0.1.0-alpha.1` crates having already been published).

### Upstream and Downstream Dependencies
- **Preceding Work (Wave 0)**: DX-01 analyzes existing assets, catalogues exact prose assertions, documents stale claims, and specifies the target information architecture.
- **Immediate Downstream (DX-02)**: Replace the 1,865 sprawling prose-equality assertions in `pbrs-grpc/tests/serving.rs` with focused API behavior contracts and clean documentation contract tests in `tests/documentation.rs`.
- **Guide Restructuring (DX-03)**: Reorganize existing monolithic documents into concise landing pages (~250 lines) with direct links to focused guides in `docs/guides/`, prune repetitive prose, and update publication claims.
- **Runnable Tutorials (DX-04 – DX-06)**: Extract executable tutorials for all four RPC call shapes (`docs/guides/rpc-shapes.md`), production TLS/mTLS and graceful drain (`docs/guides/production-service.md`), and codegen/migration (`docs/guides/codegen.md`, `docs/guides/migration.md`).

---

## 2. Asset Mapping Across Seven Functional Domains

The repository's documentation is categorized into seven functional domains according to reader intent and lifecycle stage.

| Functional Domain | User Intent | Primary Source Files | Target Location in New Structure |
|---|---|---|---|
| **1. Learn / Tutorials** | Initial onboarding; first working service; understanding the 4 RPC shapes | `README.md`<br>`docs/grpc.md`<br>`pbrs-grpc/README.md`<br>`protobuf-tonic/README.md`<br>`examples/greeter/` | `README.md`<br>`docs/grpc.md` (concise hub)<br>`docs/guides/rpc-shapes.md`<br>`examples/greeter/` |
| **2. How-to Guides** | Solving specific production tasks (TLS, health, codegen, migration) | `docs/grpc.md`<br>`protobuf-tonic/README.md`<br>`README.md` | `docs/guides/codegen.md`<br>`docs/guides/production-service.md`<br>`docs/guides/migration.md`<br>`docs/guides/interceptors.md`<br>`docs/guides/operations.md` |
| **3. API Reference** | Type signatures, method contracts, options, and error variants | Rust source rustdoc<br>`pbrs/src/`<br>`pbrs-grpc/src/`<br>`protobuf-tonic/src/` | `cargo doc --workspace`<br>`docs.rs/pbrs`<br>`docs.rs/pbrs-grpc`<br>`docs.rs/protobuf-tonic` |
| **4. Internals & Design** | Mental model, memory layout, parser architecture, upb comparison | `docs/design.md`<br>`docs/architecture.md`<br>`docs/upb.md`<br>`docs/inventory/` | `docs/architecture.md`<br>`docs/design.md`<br>`docs/upb.md`<br>`docs/guides/comparison.md` |
| **5. Compatibility & Evidence** | Conformance proof, compatibility matrix, project roadmap | `docs/status.md`<br>`docs/ROADMAP.md`<br>`docs/plan/`<br>`TODO.md`<br>`vendor/google/` | `docs/status.md` (clean matrix)<br>`docs/ROADMAP.md`<br>`docs/plan/` |
| **6. Performance** | Microbenchmark numbers, transport throughput, hardware profiles | `docs/benchmarks.md`<br>`bench/`<br>`rpc-bench/`<br>`tonic-bench/` | `docs/benchmarks.md`<br>`rpc-bench/README.md`<br>`tonic-bench/` |
| **7. Operations & Troubleshooting** | Timeouts, error codes, limits, threat model, keepalive, drains | `docs/grpc.md`<br>`docs/architecture.md`<br>`pbrs-grpc/src/status.rs`<br>`pbrs-grpc/src/config.rs` | `docs/guides/operations.md`<br>`docs/grpc.md#limits-and-the-threat-model`<br>`docs/guides/operations.md#status-codes` |

---

### Detailed Domain Inventory

#### 2.1 Learn / Tutorials
- **Core Protobuf Quickstart (`README.md` lines 86–129)**:
  - Demonstrates generating code via `build.rs`, creating messages, setting fields, serializing to `Vec<u8>`, and parsing back.
  - *Status*: Clean, functional, should remain the primary root README tutorial.
- **Native gRPC Quickstart (`docs/grpc.md` lines 27–121, `pbrs-grpc/README.md` lines 26–52)**:
  - Covers writing `proto/hello.proto`, setting up `build.rs` with `pbrs::codegen::compile_protos`, implementing the generated `Greeter` trait, mounting on `GreeterServer::new(MyGreeter).serve(...)`, and dialing with `GreeterClient::connect(...)`.
  - *Status*: Core code works, but contains stale Git dependency instructions.
- **Tonic Adapter Quickstart (`protobuf-tonic/README.md` lines 50–108)**:
  - Explains configuring `pbrs::codegen::Config::new().emit_tonic_stubs(true)`, implementing `#[tonic::async_trait]` service stubs, and serving via `tonic::transport::Server`.
  - *Status*: Accurate on stub selection, but contains stale Git dependency references.
- **The Four Call Shapes Tutorial (`docs/grpc.md` lines 122–448)**:
  - Explains Unary (`SayHello`), Server-Streaming (`Reading a stream` with `Streaming<HelloReply>`), Client-Streaming (`StreamSender<HelloRequest>`), and Bidirectional Streaming (`bidi_streaming`).
  - *Status*: Comprehensive conceptual explanations, but inline text is intermixed with defensive comparisons. Destination: extract into runnable recipe `docs/guides/rpc-shapes.md` under DX-04.

#### 2.2 How-to Guides (Recipes)
- **Transport Security & mTLS (`docs/grpc.md` lines 650–764)**:
  - Guides configuring `ServerTls`, `ClientTls`, CA pinning (`ClientTls::with_ca`), client identities (`Identity::from_pem`), ALPN (`h2`), and accessing peer certificates via `Rpc::peer_identity`.
  - *Destination*: `docs/guides/production-service.md` (DX-05).
- **Health Checking & Server Reflection (`docs/grpc.md` lines 860–969)**:
  - Explains `HealthServer`, `HealthReporter`, `Check`, `Watch`, `Health::list`, and `ServerReflectionServer`.
  - *Destination*: `docs/guides/operations.md` (OB-04 / DX-05).
- **Code Generation & Multi-file Imports (`README.md` lines 42–84, `docs/grpc.md` lines 55–82)**:
  - Details `compile_protos`, `emit_tonic_stubs`, `emit_kernel_stubs`, `protoc-gen-pbrs`, `PURE_PROTOBUF_STUBS` environment variables, descriptor sets, and include paths.
  - *Destination*: `docs/guides/codegen.md` (DX-06).
- **Tonic & Prost Migration Guide (`protobuf-tonic/README.md`, `docs/upb.md` lines 80–95)**:
  - Explicitly documents that `pbrs` messages implement Google Protobuf v4 traits (`Parse`, `Serialize`), not `prost::Message`. Highlights middleware implications and differences in generated stubs.
  - *Destination*: `docs/guides/migration.md` (DX-06).
- **Multi-Service Routing (`docs/grpc.md` lines 590–649)**:
  - Explains composing multiple services onto one TCP/TLS listener using `Router::new().add_service(...).serve(...)`.
  - *Destination*: `docs/guides/production-service.md`.
- **Local IPC (Unix Domain Sockets & In-Process Pipes) (`docs/grpc.md` lines 790–859)**:
  - Shows `serve_unix`, `connect_unix`, crash recovery via `serve_unix_unlink`, peer credentials from `SO_PEERCRED` on `Rpc::peer_cred`, and memory-backed duplex streaming with `Channel::from_io` / `Server::serve_connection`.
  - *Destination*: `docs/guides/production-service.md`.
- **Interceptors & Request Overlays (`docs/grpc.md` lines 1800–2100)**:
  - Demonstrates client `Outgoing` mutation (`set_user_agent`, `set_timeout`, `set_wait_for_ready`, `set_compress`), server `Rpc` validation, header inspection, and sharing context via extensions.
  - *Destination*: `docs/guides/interceptors.md`.

#### 2.3 API Reference
- **Rustdoc (`cargo doc --workspace --no-deps`)**:
  - The authoritative source for function signatures, struct field definitions, error types, and trait bounds.
- **Core Public Types (`pbrs`)**:
  - `Message`, `Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`, `RepeatedView`, `DynamicMessage`, `codegen::compile_protos`, `codegen::Config`.
- **gRPC Public Types (`pbrs-grpc`)**:
  - Transport & Clients: `Channel`, `ChannelConfig`, `Server`, `ServerConfig`, `Router`, `Incoming`, `ConnectionInfo`, `Target`.
  - RPC Context & Data: `Request`, `Response`, `Rpc`, `Parts`, `Outgoing`, `Streaming`, `StreamSender`, `CallHandle`.
  - Status & Rich Errors: `Status`, `Code`, `ErrorDetails`, `ErrorInfo`, `RetryInfo`, `BadRequest`, `FieldViolation`, `QuotaFailure`, `PreconditionFailure`, `Help`, `LocalizedMessage`.
  - Security: `ClientTls`, `ServerTls`, `Identity`, `Certificate`.
- **Tonic Adapter Public Types (`protobuf-tonic`)**:
  - `ProtobufCodec`, `ProtobufDecoder`, `ProtobufEncoder`.

#### 2.4 Internals & Design
- **Architecture Overview (`docs/architecture.md`)**:
  - Outlines crate boundaries, zero C dependency guarantee, prior-knowledge HTTP/2 accept loop, dispatch tree, framing, and client connection drivers.
  - *Action needed*: Remove the ~444 repetitive test assertion sentences embedded into the prose; preserve pure architectural explanation.
- **Memory Layout & Parser Internals (`docs/design.md`)**:
  - Documents 648-byte `TestAllTypesProto3`, ~19 ns default allocation, zero-allocation empty collections, SSO string handling (<= 23 bytes), lazy slices, and `MsgCold` boxing.
  - *Status*: High quality, concise (81 lines), accurate.
- **Relationship to Google upb (`docs/upb.md`)**:
  - Rigorous architectural comparison of `pbrs` against Google's C-based upb kernel: field-wise Rust structs vs. C arenas, single-pass validation vs. MiniTable decoding, and ABI compatibility boundaries.
  - *Status*: High quality (100 lines), clear tables.
- **Closed Historical Inventories (`docs/inventory/`)**:
  - Records discarded implementation branches and micro-optimizations (e.g., heap copying, flatten merge inner). Kept as historical record.

#### 2.5 Compatibility & Evidence
- **Implementation Status & Feature Boundaries (`docs/status.md`)**:
  - Captures test coverage, conformance runner results (Google conformance runner v35.1: 5,631 binary + JSON, 909 text tests passed), and unsupported features (xDS, Edition 2024, arena views).
  - *Action needed*: Separate the factual compatibility matrix from the 408 "Shipped Distinct notes" assertions.
- **Strategic Roadmap & Scorecards (`docs/ROADMAP.md`)**:
  - Defines work packages GR-01 through GR-12, gating criteria, qualification requirements, and promotion milestones.
- **Granular Execution Plan & Task Definitions (`docs/plan/README.md`, `docs/plan/tasks.json`, `TODO.md`)**:
  - Formal tracking of implementation wave tasks, file scopes, and verification commands.

#### 2.6 Performance
- **Microbenchmarks & Comparison Report (`docs/benchmarks.md`)**:
  - Contains gated and extended benchmark suites comparing `pbrs` against `prost`, Google `protobuf` 4.x (upb), and `buffa`. Details methodology, hardware profile (Apple M4 Pro), payload sizes, and large-payload memcpy behavior.
- **Transport Benchmarks (`rpc-bench`)**:
  - Loopback unary latency and streaming throughput measurements comparing `pbrs-grpc` against `tonic` 0.14 and `grpc-go`.
- **Codec Survey (`tonic-bench`)**:
  - Measures `pbrs` codec performance inside Tonic compared to Prost.

#### 2.7 Operations & Troubleshooting
- **Connection Lifecycle & Draining (`docs/grpc.md` lines 970–1049)**:
  - Server graceful shutdown (`serve_with_shutdown`), draining active streams, connection age/idle limits (`max_connection_age`, `max_connection_idle`), jitter, and `GOAWAY` framing.
- **Timeouts, Deadlines & Cancellation (`docs/grpc.md` lines 500–589)**:
  - Explains timeout propagation (`grpc-timeout`), deadline computation, client cancellation (`CallHandle`, reset stream), and server cancellation detection (`Request::cancelled`).
- **Rich Status Codes & Error Model (`docs/grpc.md` lines 450–499, `pbrs-grpc/src/status.rs`)**:
  - Standard status codes, rich error payload parsing (`google.rpc.Status`), `ErrorDetails` unpacking, and precedence of ASCII headers over binary status details.
- **Limits & Threat Model (`docs/grpc.md` lines 1450–1800, `docs/architecture.md` lines 150–220)**:
  - Deep architectural breakdown of defensive HTTP/2 limits: rapid reset mitigation (CVE-2023-44487 via `max_pending_accept_reset_streams`), CONTINUATION frame floods, protocol-error RST floods (`max_local_error_reset_streams`), small-DATA framing caps (`data_frame_budget`), and header list caps (`max_header_list_size`).
- **Keepalive & TCP Socket Tuning (`docs/grpc.md` lines 765–789)**:
  - Contrasts TCP OS-level keepalive (`tcp_keepalive_interval`, `tcp_keepalive_retries`) with HTTP/2 protocol PINGs (`keep_alive_interval`, `keep_alive_timeout`), plus `TCP_NODELAY`.

---

## 3. Audit of Exact Prose Assertions in `pbrs-grpc/tests/serving.rs`

A central discovery of DX-01 is that the test suite in `pbrs-grpc/tests/serving.rs` contains a 15,816-line test function:
```rust
fn channel_config_connect_timeout_documents_every_call_shape() { ... } // lines 6732 to 22548
```
This single function asserts **1,865 exact string fragments** across five documentation files:
- `docs/grpc.md` (via variable `guide`): **593 assertions**
- `docs/architecture.md` (via variable `architecture`): **444 assertions**
- `pbrs-grpc/README.md` (via variable `readme`): **415 assertions**
- `docs/status.md` (via variable `status_guide`): **408 assertions**
- `docs/benchmarks.md` (via variable `benches`): **5 assertions**

*(Note: In addition, `examples/greeter/src/lib.rs` contains 33 assertions asserting exact strings in `examples/greeter/README.md`, and `tests/onboarding.rs` contains 2 assertions on `protobuf-tonic/README.md`.)*

### 3.1 Breakdown by Document Section

Every single one of the 1,865 assertions in `serving.rs` maps directly to an exact section in the documentation:

#### A. `docs/grpc.md` (`guide`, 593 assertions)
1. `## Writing a service without codegen`: **126 assertions** (asserts exact behavior of manual `Service::call` dispatch, raw framing, and `Rpc` destruction).
2. `## Interceptors and middleware`: **119 assertions** (asserts `Outgoing` setters, clearers, occupancy predicates, live-socket checks, and shared extensions).
3. `## Metadata`: **102 assertions** (asserts ASCII vs binary `-bin` metadata handling, reserved header rejection, and trailing metadata delivery).
4. `## Compression`: **63 assertions** (asserts gzip negotiation, `accepts_compressed`, `compresses_outbound`, and encoding fallbacks).
5. `## What is not here`: **48 assertions** (asserts explicit omissions like xDS, channelz, dynamic reload, retries).
6. `## Health checks`: **30 assertions** (asserts `Check`, `Watch`, `Health::list`, and subscription cleanup).
7. `## Reflection`: **28 assertions** (asserts symbol lookup, extension query, and reflection stream bounds).
8. `## Limits and the threat model`: **22 assertions** (asserts frame budgets, rapid reset caps, CONTINUATION flood defenses).
9. `#[tokio::main]`: **10 assertions** (asserts server startup semantics and unhandled error bubbling).
10. `## Keepalive`: **8 assertions** (asserts HTTP/2 PING vs TCP socket keepalive).
11. `## TLS`: **8 assertions** (asserts certificate chain extraction, ALPN, and SNI).
12. `## Connection age and idle`: **5 assertions** (asserts GOAWAY timing, age jitter, and idle resets).
13. `## Errors and status codes`: **4 assertions** (asserts `Status` conversion and ASCII precedence).
14. `## Serving several services`: **4 assertions** (asserts `Router` path splitting and unmatched fallback).
15. `## Tuning`: **3 assertions** (asserts buffer allocation and window sizes).
16. `## One-shape proofs`: **3 assertions** (asserts test invariants across transports).
17. `## In-process connections`: **2 assertions** (asserts `from_io` lack of redial and fixed schemes).
18. `# until these crates are on crates.io:`: **2 assertions** (asserts default stub emission phrases).
19. `## Deadlines and cancellation`: **2 assertions** (asserts client RST and `Request::cancelled`).
20. `## Connect timeout`: **2 assertions** (asserts handshake deadlines vs connection refuse).
21. `#[tokio::test]`: **1 assertion**
22. `## Graceful shutdown`: **1 assertion**

#### B. `pbrs-grpc/README.md` (`readme`, 415 assertions)
1. `## Design Invariants & Comparison with Tonic and gRPC-Go`: **330 assertions** (asserts negative comparisons explaining what is NOT present compared to Tonic / gRPC-Go).
2. `## Features & Capabilities`: **85 assertions** (asserts a massive paragraph listing every single builder and error method).

#### C. `docs/status.md` (`status_guide`, 408 assertions)
1. `## Shipped Distinct notes`: **408 assertions** (asserts permutations of "Distinct from..." for every transport and call shape).

#### D. `docs/architecture.md` (`architecture`, 444 assertions)
1. `### Interceptors`: **238 assertions** (asserts interceptor execution points, context objects, and overlay scopes).
2. `### Status`: **77 assertions** (asserts status error details, trailer formatting, and mapping).
3. `### Client`: **57 assertions** (asserts client pooling, connection limits, and dial options).
4. `### Health and reflection`: **54 assertions** (asserts internal dispatch for health and reflection).
5. `### Dispatch`: **16 assertions** (asserts route resolution, unmounted service handling).
6. `### Accept`: **2 assertions** (asserts accept loop behavior under socket starvation).

#### E. `docs/benchmarks.md` (`benches`, 5 assertions)
1. `### Bidi ping-pong throughput (loopback)`: **2 assertions**
2. `### Bidi ping-pong and upload vs grpc-go (loopback)`: **2 assertions**
3. `### Client-streaming upload throughput (loopback)`: **1 assertion**

---

### 3.2 Thematic Inventory of Assertions

The 1,865 assertions cluster into five primary semantic themes:

1. **Interceptor & Overlay Semantics (~45% of assertions)**:
   - Asserts the presence of `Outgoing::user_agent_is_set`, `wait_for_ready_is_set`, `compress_is_set`.
   - Asserts clearers: `clear_user_agent`, `clear_wait_for_ready`, `clear_compress`, `clear_timeout`.
   - Asserts the distinction between live connection snapshots (`Outgoing::connected`) and wait-for-ready settings.
   - Asserts the exact lifecycle stage where interceptors execute (outbound before stream open vs. server inbound before handler vs. post-response).
2. **Error Handling & Status Details (~35% of assertions)**:
   - Asserts `Status::from_error_details`, `set_error_details`, `set_from_error_details`.
   - Asserts builders: `ErrorInfo::with_reason`, `BadRequest::with_field`, `FieldViolation::with_field`, `QuotaFailure::with_violation`, `PreconditionFailure::with_violation`, `Help::with_link`, `LocalizedMessage::with_locale`.
   - Asserts precedence: ASCII `grpc-status` and `grpc-message` headers take precedence over packed binary `google.rpc.Status` payloads if they disagree.
3. **Negative Invariants / Comparisons with Tonic & gRPC-Go (~12% of assertions)**:
   - Asserts that `pbrs-grpc` lacks `tonic::transport::Server::executor` (uses Tokio runtime directly).
   - Asserts absence of tower middleware layers (`ConcurrencyLimitLayer`, `RateLimitLayer`, `LoadShedLayer`) in the native kernel.
   - Asserts absence of gRPC-Go concepts (`WithDefaultCallOptions`, `WithDefaultServiceConfig`, `EnforcementPolicy`, `NumStreamWorkers`).
4. **Framing Limits & Threat Model (~6% of assertions)**:
   - Asserts specific configuration names: `ServerConfig::max_pending_accept_reset_streams` (20), `max_local_error_reset_streams` (1024), `max_concurrent_reset_streams` (50), `data_frame_budget` (25600), `max_header_list_size` (16 KiB).
   - Asserts defense against CVE-2023-44487 (rapid reset) and CONTINUATION flood attacks.
5. **Transport Equivalence Across 5 Backends (~2% of assertions)**:
   - Asserts that all behaviors are validated across: standard TCP (h2c), TLS, mutual TLS (mTLS), Unix Domain Sockets, and in-process duplex pipes (`from_io`).

---

### 3.3 Safe Migration Protocol for DX-02 and DX-03

Any edit to `docs/grpc.md`, `docs/architecture.md`, `docs/status.md`, or `pbrs-grpc/README.md` that touches asserted text will **instantly break CI** until DX-02 is executed.

#### Protocol for DX-02:
1. **Never delete behavioral tests**: DX-02 must preserve all actual runtime tests covering TLS, interceptors, limits, error codes, and streaming.
2. **Decouple prose matching**: Move documentation presence checks into a dedicated `tests/documentation.rs` file. Replace brittle multi-sentence paragraph assertions with targeted, structural checks (e.g. verifying that a guide contains required security warnings or links to reference manuals).
3. **Verify API assertions programmatically**: Where `serving.rs` asserts that `Outgoing::user_agent_is_set` exists by checking text in a markdown file, assert the actual API behavior in Rust code against the struct instead.

#### Protocol for DX-03:
1. DX-03 must only proceed **after** DX-02 has landed on `main`.
2. Once DX-02 replaces prose checks with API and contract tests, DX-03 can freely excise the 2,012 "Distinct from..." sentences and rewrite guides into concise, human-readable documents.

---

## 4. Stale Publication Claims and Duplicated Prose

### 4.1 Stale Publication Claims

Multiple files currently claim that the crates in this repository are not yet published to crates.io and instruct users to pull dependencies from Git:

1. **`docs/grpc.md` (lines 55–65)**:
   ```toml
   # Cargo.toml
   # until these crates are on crates.io:
   [dependencies]
   pbrs = { git = "https://github.com/mingley/pure-protobuf" }
   pbrs-grpc = { git = "https://github.com/mingley/pure-protobuf" }
   tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

   [build-dependencies]
   pbrs = { git = "https://github.com/mingley/pure-protobuf" }
   ```
2. **`pbrs-grpc/README.md` (lines 17–25)**:
   ```toml
   [dependencies]
   # until these crates are on crates.io:
   pbrs = { git = "https://github.com/mingley/pure-protobuf" }
   pbrs-grpc = { git = "https://github.com/mingley/pure-protobuf" }

   [build-dependencies]
   pbrs = { git = "https://github.com/mingley/pure-protobuf" }
   ```
3. **`protobuf-tonic/README.md` (lines 38–48)**:
   ```toml
   [dependencies]
   tonic = { version = "0.14", default-features = false, features = ["transport", "codegen"] }
   pbrs = "0.1"
   protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }
   ```
   Followed by:
   > `**Note on crates.io**: protobuf-tonic currently depends on pbrs by path/git. It will be published to crates.io following the publication of pbrs.`

#### Verified Reality
- `pbrs` **`0.1.0`** is published on crates.io.
- `pbrs-grpc` **`0.1.0-alpha.1`** is published on crates.io.
- `protobuf-tonic` **`0.1.0-alpha.1`** is published on crates.io.
- `.github/workflows/first-publish.yml` is already marked obsolete because all three crates exist on crates.io.
- `docs/RELEASE.md` documents the active tag/dispatch release workflow.

**Action for DX-03**: Replace all Git dependency blocks with standard `Cargo.toml` version specifications:
```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"
```
and for Tonic users:
```toml
[dependencies]
pbrs = "0.1"
protobuf-tonic = "0.1.0-alpha.1"
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen"] }
```

---

### 4.2 Repetitive Comparisons and Prose Duplication

The documentation currently suffers from artificial repetition:

1. **"Distinct from..." combinatorial explosion**:
   - `docs/grpc.md`: **604 occurrences**
   - `docs/status.md`: **501 occurrences**
   - `docs/architecture.md`: **475 occurrences**
   - `pbrs-grpc/README.md`: **432 occurrences**
   - Total: **2,012 occurrences** across four documents.
   Every feature description is followed by an exhaustive list of what it is *not* in Tonic, gRPC-Go, or upb, repeated across several files.
2. **Synthetic file tagging phrases**:
   - Paragraphs repeatedly append phrases like `on this crate README`, `on this status guide`, `on this architecture guide`, solely to make substrings distinct for testing purposes.
3. **Repeated transport qualification**:
   - The phrase `over TLS, mTLS, Unix, and from_io` is repeated verbatim dozens of times across `docs/grpc.md` and `docs/status.md`.
4. **Scattered comparative tables**:
   - Comparisons between `pbrs-grpc`, `tonic`, and `grpc-go` are scattered across prose rather than presented in a clean, unified reference matrix.

**Action for DX-03**:
- Consolidate all architectural and operational differences between `pbrs-grpc`, `tonic`, and `grpc-go` into a single reference document (`docs/guides/comparison.md`).
- Present differences in structured comparison tables rather than repetitive inline prose.

---

## 5. Target Page Hierarchy and Navigation Structure for DX-03

To provide a clean developer experience, DX-03 will adopt a **Hub-and-Spoke Navigation Model**. The main landing guides (`docs/grpc.md`, crate READMEs) will be concise (~250 lines) task hubs that introduce core mental models and provide direct links to standalone, focused recipes in `docs/guides/`.

### 5.1 Proposed Directory Layout

```text
pure-protobuf/
├── README.md                      # Workspace landing page (Core Protobuf focus)
├── Cargo.toml
├── TODO.md                        # Active engineering queue
├── docs/
│   ├── documentation-map.md       # (This file) Architecture and asset map
│   ├── grpc.md                    # Primary gRPC Hub Guide (~250 lines)
│   ├── architecture.md            # Clean system architecture & crate boundaries
│   ├── design.md                  # Memory layout, parser internals, zero-alloc design
│   ├── upb.md                     # Relationship to Google upb & C runtime
│   ├── status.md                  # Compatibility matrix & verified test results
│   ├── benchmarks.md              # Microbenchmarks & transport throughput measurements
│   ├── ROADMAP.md                 # Strategic roadmap & scorecards (GR-01 .. GR-12)
│   ├── RELEASE.md                 # Publishing policy & CI release workflow
│   ├── guides/                    # Task-Oriented How-to Recipes (DX-03 .. DX-06)
│   │   ├── rpc-shapes.md          # 4 RPC call shapes with runnable examples (DX-04)
│   │   ├── production-service.md  # TLS/mTLS, limits, timeouts & graceful drain (DX-05)
│   │   ├── codegen.md             # build.rs, protoc, plugin & multi-file imports (DX-06)
│   │   ├── migration.md           # Migrating from Prost / Tonic / Google upb (DX-06)
│   │   ├── interceptors.md        # Auth, metadata, outgoing overlays & extensions
│   │   ├── operations.md          # Health check, reflection, sockets & keepalive
│   │   └── comparison.md          # Consolidated comparison: pbrs-grpc vs Tonic vs gRPC-Go (DX-03)
│   ├── reference/                 # Detailed Technical Reference
│   │   ├── threat-model.md        # HTTP/2 framing limits & CVE mitigations
│   │   └── status-codes.md        # gRPC status codes & rich ErrorDetails model
│   └── plan/                      # Project execution plan & task cards
│       ├── README.md
│       └── tasks.json
├── pbrs-grpc/
│   └── README.md                  # Crate README (~150-200 lines, links to docs/)
├── protobuf-tonic/
│   └── README.md                  # Tonic adapter guide (~150-200 lines, links to docs/)
└── examples/
    └── greeter/                   # Executable reference microservice
        └── README.md
```

---

### 5.2 Specification for Key Landing Pages

#### 1. `docs/grpc.md` (Target: ~250 lines)
The main entry point for native gRPC in `pure-protobuf`. It must not contain thousands of lines of code dumps or test assertions.
- **Header & Overview (~25 lines)**: What `pbrs-grpc` is (pure-Rust, no C, no unsafe kernel, independent of Tonic).
- **Quickstart (~60 lines)**:
  - Concise `Cargo.toml` using published crates.io versions (`pbrs = "0.1"`, `pbrs-grpc = "0.1.0-alpha.1"`).
  - Minimal `build.rs` using `compile_protos`.
  - Minimal `SayHello` implementation and client dial.
- **Mental Model & Core Types (~45 lines)**:
  - Contrast with Tonic: direct prior-knowledge HTTP/2, explicit configuration structs (`ServerConfig`, `ChannelConfig`), unified error handling.
- **The Four Call Shapes Summary (~30 lines)**:
  - Brief description of Unary, Server Streaming, Client Streaming, and Bidi Streaming.
  - Link to `docs/guides/rpc-shapes.md` for full implementation walkthroughs.
- **Production Checklist & Navigation Hub (~90 lines)**:
  - Structured cards/links pointing to:
    - Transport Security & mTLS &rarr; `docs/guides/production-service.md`
    - Code Generation & Custom Stubs &rarr; `docs/guides/codegen.md`
    - Tonic Migration & Trait Boundaries &rarr; `docs/guides/migration.md`
    - Interceptors & Metadata Overlays &rarr; `docs/guides/interceptors.md`
    - Health Checks & Server Reflection &rarr; `docs/guides/operations.md`
    - Threat Model & HTTP/2 Frame Caps &rarr; `docs/grpc.md#limits-and-the-threat-model`
    - Comprehensive Benchmark Results &rarr; `docs/benchmarks.md`
    - Framework Comparisons (Tonic / gRPC-Go) &rarr; `docs/guides/comparison.md`

#### 2. `pbrs-grpc/README.md` (Target: ~150–200 lines)
- Crate summary, crates.io badge, MSRV (1.85).
- Quick installation snippet with crates.io version.
- 5-bullet core feature highlights.
- Clear link to `docs/grpc.md` as the primary working guide.
- Link to `docs/guides/comparison.md` for architectural invariants.

#### 3. `protobuf-tonic/README.md` (Target: ~150–200 lines)
- Crate summary, crates.io badge, MSRV (1.88).
- Explicit trait boundary warning: uses Google Protobuf v4 application traits (`Parse` / `Serialize`), not `prost::Message`.
- Quick installation snippet with crates.io version.
- `build.rs` snippet demonstrating `emit_tonic_stubs(true)`.
- Link to `docs/guides/migration.md` for migrating existing Tonic services.

---

### 5.3 Inbound Anchor Preservation Plan

To ensure external links, cross-document links, and existing documentation tests do not break during DX-03, all major heading anchors currently in `docs/grpc.md` will be preserved either directly in `docs/grpc.md` or as explicitly redirected anchors.

| Existing Anchor in `docs/grpc.md` | Resolution in DX-03 Landing Page | Target Guide Link |
|---|---|---|
| `#quickstart` | Retained directly in `docs/grpc.md` | — |
| `#the-four-call-shapes` | Retained as summary section | `docs/guides/rpc-shapes.md` |
| `#reading-a-stream` | Summary + link | `docs/guides/rpc-shapes.md#reading-a-stream` |
| `#writing-a-stream` | Summary + link | `docs/guides/rpc-shapes.md#writing-a-stream` |
| `#client-streaming` | Summary + link | `docs/guides/rpc-shapes.md#client-streaming` |
| `#metadata` | Summary + link | `docs/guides/interceptors.md#metadata` |
| `#errors-and-status-codes` | Summary + link | `docs/guides/operations.md#status-codes` |
| `#deadlines-and-cancellation` | Summary + link | `docs/guides/production-service.md#deadlines` |
| `#wait-for-ready-and-lazy-connect` | Summary + link | `docs/guides/production-service.md#wait-for-ready` |
| `#connect-timeout` | Summary + link | `docs/guides/production-service.md#timeouts` |
| `#serving-several-services` | Summary + link | `docs/guides/production-service.md#router` |
| `#tls` | Summary + link | `docs/guides/production-service.md#tls` |
| `#keepalive` | Summary + link | `docs/guides/operations.md#keepalive` |
| `#unix-domain-sockets` | Summary + link | `docs/guides/production-service.md#unix-sockets` |
| `#in-process-connections` | Summary + link | `docs/guides/production-service.md#in-process` |
| `#health-checks` | Summary + link | `docs/guides/operations.md#health-checks` |
| `#reflection` | Summary + link | `docs/guides/operations.md#reflection` |
| `#graceful-shutdown` | Summary + link | `docs/guides/production-service.md#graceful-shutdown` |
| `#connection-age-and-idle` | Summary + link | `docs/guides/production-service.md#connection-age` |
| `#compression` | Summary + link | `docs/guides/production-service.md#compression` |
| `#limits-and-the-threat-model` | Retained directly in `docs/grpc.md` | `docs/grpc.md#limits-and-the-threat-model` |
| `#tuning` | Summary + link | `docs/guides/operations.md#tuning` |
| `#interceptors-and-middleware` | Summary + link | `docs/guides/interceptors.md` |
| `#testing` | Summary + link | `docs/guides/operations.md#testing` |
| `#writing-a-service-without-codegen` | Retained as advanced section or guide | `docs/guides/codegen.md#manual-service` |
| `#what-is-not-here` | Retained as summary section | `docs/guides/comparison.md#omissions` |

---

## 6. Execution Roadmap for DX-02 and DX-03

### Phase 1: DX-02 Execution ("Replace sprawling prose-equality tests with contracts")
1. **Target**: `pbrs-grpc/tests/serving.rs` and new `tests/documentation.rs`.
2. **Action**:
   - Delete the 1,865 `guide.contains(...)`, `readme.contains(...)`, `status_guide.contains(...)`, and `architecture.contains(...)` assertions inside `fn channel_config_connect_timeout_documents_every_call_shape`.
   - Verify that all behavioral tests (exercising the actual server, client, TLS, Unix sockets, interceptors, error details, and timeouts) remain completely intact.
   - In `tests/documentation.rs`, implement focused structural tests:
     - Verify required links exist between landing pages and recipes.
     - Verify required safety caveats (e.g. `prost::Message` incompatibility in `protobuf-tonic/README.md`) are present.
     - Verify published crate version strings match package manifests.
3. **Validation**: Run `cargo test -p pbrs-grpc --test serving` and verify that the test passes without depending on exact prose strings.

### Phase 2: DX-03 Execution ("Reorganize guides and remove stale duplication")
1. **Target**: `README.md`, `docs/grpc.md`, `docs/status.md`, `docs/architecture.md`, `pbrs-grpc/README.md`, `protobuf-tonic/README.md`, and new `docs/guides/`.
2. **Action**:
   - Rewrite `docs/grpc.md` to match the ~250-line hub specification.
   - Strip the 2,012 "Distinct from..." sentences and repetitive "on this crate README" tags from all markdown files.
   - Replace all `# until these crates are on crates.io:` Git dependency snippets with clean crates.io dependencies.
   - Create `docs/guides/comparison.md` containing structured comparison tables against Tonic and gRPC-Go.
   - Create the initial recipe files in `docs/guides/` matching the planned structure.
3. **Validation**:
   - Run `cargo test --workspace` to ensure all documentation and onboarding tests pass.
   - Validate that all markdown links resolve correctly.

### Phase 3: DX-07 Execution ("Continuously check published documentation contracts")
1. **Target**: `tests/documentation.rs`, `.github/workflows/ci.yml`, and `docs/documentation-map.md`.
2. **Action**:
   - Enhance `tests/documentation.rs` with comprehensive contract validation:
     - All internal markdown links and anchor targets across `docs/` and `README.md` resolve to valid files and heading slugs/HTML anchors.
     - All referenced guides and anchors in `docs/documentation-map.md` resolve to existing files and valid anchors.
     - All fenced Rust and Protobuf code snippets in guides are syntactically valid (validated via `rustfmt` and `protoc`).
     - Critical architecture, retry, security/TLS, and license caveats remain intact.
   - Add a dedicated fast documentation contract check step in `.github/workflows/ci.yml` running on pull requests and pushes.
3. **Validation**:
   - Run `cargo test --test documentation` to ensure all link, anchor, snippet, and caveat contracts pass.
