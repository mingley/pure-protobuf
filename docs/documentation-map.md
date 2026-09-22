# Documentation map (DX-01)

Design record for task-oriented documentation navigation. Audited at
`0d65607c817fa8eecf6fdd75b8dacd76cfebaf82`; line counts below are
`wc -l` at that revision. The hub-and-spoke layout this card designs
is already landed: the gRPC hub is concise, the seven guides exist,
and markdown prose is no longer pinned by multi-thousand-line
exact-string tests. This file is the maintained navigation record, not
a second copy of the guides. It must stay link- and reference-valid
under `tests/documentation.rs` (section 9).

## 1. Seven-domain content map

| Domain | Reader intent | Canonical page | Supporting pages |
|---|---|---|---|
| Learn / tutorials | First working build and service | [README.md](../README.md) (241 lines) | [gRPC hub](grpc.md) (206 lines), [pbrs-grpc README](../pbrs-grpc/README.md) (142 lines), [protobuf-tonic README](../protobuf-tonic/README.md) (140 lines) |
| How-to guides | Solve one production task | [guides/](guides/rpc-shapes.md) (7 guides, 1,076 lines total) | [rpc-shapes](guides/rpc-shapes.md) (311), [production-service](guides/production-service.md) (201), [operations](guides/operations.md) (184), [codegen](guides/codegen.md) (154), [interceptors](guides/interceptors.md) (121), [migration](guides/migration.md) (83), [comparison](guides/comparison.md) (72) |
| API reference | Signatures, traits, options, errors | rustdoc (`cargo doc --workspace`) | [docs.rs](https://docs.rs/pbrs) for `pbrs`, `pbrs-grpc`, `protobuf-tonic` |
| Internals | Mental model, layout, parser design | [architecture.md](architecture.md) (109 lines) | [design.md](design.md) (81), [upb.md](upb.md) (100), [unsafe-invariants.md](unsafe-invariants.md) (287) |
| Compatibility and evidence | What is supported and proven | [status.md](status.md) (172 lines) | [ROADMAP.md](ROADMAP.md) (415), [plan/README.md](plan/README.md), [TODO.md](../TODO.md), [RELEASE.md](RELEASE.md) (110) |
| Performance | Workload-specific measurements | [benchmarks.md](benchmarks.md) (487 lines) | [benchmark-contract.md](benchmark-contract.md) (299), [resource-budgets.md](resource-budgets.md) (612) |
| Operations and troubleshooting | Run, debug, and bound a service | [operations guide](guides/operations.md) (184 lines) | [retry-contract.md](retry-contract.md) (220), [cacheable-rpc.md](cacheable-rpc.md) (194), [codegen-compatibility.md](codegen-compatibility.md) (210), [codegen-layout.md](codegen-layout.md) (369), [edition-2024.md](edition-2024.md) (403) |

Rule: a journey's canonical page owns the narrative; supporting pages
are linked, not duplicated. New prose goes in exactly one page.

## 2. Canonical page per user journey

Each journey has one canonical page and a link to runnable code.

| Journey | Canonical page | Runnable code |
|---|---|---|
| Use protobuf messages in Rust | [README.md](../README.md) quickstart | [examples/greeter](../examples/greeter/src/main.rs), [tests/typed.rs](../tests/typed.rs) |
| Build a native gRPC service | [gRPC hub](grpc.md) | [examples/greeter](../examples/greeter) (`build.rs`, `proto/`, `src/main.rs`, `src/lib.rs`) |
| Learn one RPC shape | [rpc-shapes guide](guides/rpc-shapes.md) | [examples/greeter](../examples/greeter/src/lib.rs), [pbrs-grpc rpc tests](../pbrs-grpc/tests/rpc.rs) |
| Ship TLS, timeouts, drain | [production-service guide](guides/production-service.md) | [pbrs-grpc tls tests](../pbrs-grpc/tests/tls.rs), [lifecycle tests](../pbrs-grpc/tests/lifecycle.rs) |
| Add auth, metadata, overlays | [interceptors guide](guides/interceptors.md) | [protobuf-tonic interceptor_size tests](../protobuf-tonic/tests/interceptor_size.rs) |
| Operate: health, reflection, errors | [operations guide](guides/operations.md) | [pbrs-grpc health tests](../pbrs-grpc/tests/health.rs), [reflection tests](../pbrs-grpc/tests/reflection.rs) |
| Generate code, migrate from prost/tonic | [codegen guide](guides/codegen.md), [migration guide](guides/migration.md) | [onboarding consumer builds](../tests/onboarding.rs), [protobuf-tonic interop tests](../protobuf-tonic/tests/interop.rs) |
| Compare against tonic / gRPC-Go | [comparison guide](guides/comparison.md) | [bench](../bench), [rpc-bench](../rpc-bench), [tonic-bench](../tonic-bench) |
| Evaluate performance claims | [benchmarks.md](benchmarks.md) | [bench](../bench), [rpc-bench](../rpc-bench), [tonic-bench](../tonic-bench) |
| Check support boundaries | [status.md](status.md) | [conformance script](../scripts/conformance.sh), [interop script](../scripts/grpc-interop.sh) |

## 3. Historical evidence versus current support claims

Current support claims live in [status.md](status.md)
(`Shipped Capabilities and Boundaries`, `Unfinished`) and the
[ROADMAP scorecard](ROADMAP.md). Historical evidence is kept
separate and is never presented as qualification:

- [inventory/](inventory/README.md): closed experiments (heap-copy,
  flatten drafts). Do not merge; do not cite as current behavior.
- [status.md](status.md) `Verified` section: recorded CI /
  conformance / interop numbers at a pinned revision. Historical
  results, not production certification.
- [plan/](plan/README.md): execution cards and dependency order, not
  shipped behavior.
- [vendor/google/](../vendor/google): pinned upstream descriptors
  (conformance FileDescriptorSet), not documentation.

Writer rule: a support claim cites shipped source or a recorded,
pinned run. A historical number cites its revision and stays out of
the guides.

## 4. Existing anchor inventory

[grpc.md](grpc.md) carries 26 explicit HTML anchors that external
and cross-document links may target. They must be preserved or
redirected before any hub rewrite:

```text
quickstart
the-four-call-shapes, reading-a-stream, writing-a-stream, client-streaming
tls, unix-domain-sockets, in-process-connections
deadlines-and-cancellation, connect-timeout, wait-for-ready-and-lazy-connect
graceful-shutdown, connection-age-and-idle, serving-several-services, compression
metadata, interceptors-and-middleware
health-checks, reflection, keepalive, tuning, errors-and-status-codes, testing
writing-a-service-without-codegen
limits-and-the-threat-model
what-is-not-here
```

Heading-slug anchors (for example `support-matrix` in the root
README) are derived from headings; renaming a heading breaks
inbound links the same way. The repo-wide link test in section 9
catches breakage after the fact; this list exists so rewrites
preserve anchors by design.

## 5. Markdown-coupled tests

### 5.1 Exact-prose tests that block text moves

One suite still pins exact markdown prose and must be migrated
before its target text is moved or rewritten:

- [examples/greeter/src/lib.rs](../examples/greeter/src/lib.rs),
  4 test functions, 38 `readme.contains(...)` assertions against
  [examples/greeter/README.md](../examples/greeter/README.md)
  (58 lines):
  - `example_readme_names_from_error_details_on_interceptor_err`
  - `example_readme_names_from_error_details_on_handler_err`
  - `example_readme_names_from_error_details_on_client_interceptor_err`
  - `example_readme_names_from_error_details_on_stream_sender_fail`

Migration protocol (DX-02 follow-up, outside this card): convert
each pinned sentence into either a Rust behavior assertion against
the greeter service or one structural presence check (key type or
warning present), then delete the exact-string assertions. Do not
edit the greeter README's pinned sentences until that lands.

### 5.2 Structural contracts to preserve

These tests couple to markdown deliberately and cheaply; keep
their shape when editing the listed files:

- [tests/onboarding.rs](../tests/onboarding.rs)
  `tonic_readme_selects_stubs_explicitly`: 2 presence checks on
  the tonic README (`emit_tonic_stubs(true)`, `prost::Message`).
- [tests/documentation.rs](../tests/documentation.rs): 13 tests —
  19 critical-caveat presence contracts, repo-wide markdown
  link/anchor validation, this map's `docs/` reference validation,
  and rust/`protoc` syntax checks for guide snippets.

### 5.3 Already migrated

- [pbrs-grpc/tests/serving.rs](../pbrs-grpc/tests/serving.rs):
  zero markdown reads. Its `contains(...)` assertions target Rust
  source (`src/client.rs`, `src/stream.rs`, `src/request.rs`,
  `src/interceptor.rs`, `src/hello.rs`) and TLS fixtures, so the
  main guides can be edited without touching that suite.

## 6. Stale publication claims

One live stale claim remains at this revision, in
[protobuf-tonic/README.md](../protobuf-tonic/README.md):

- Line 35: `protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }`
- Line 41: "`protobuf-tonic` currently depends on `pbrs` by
  path/git. It will be published to crates.io following the
  publication of `pbrs`."

Published reality (workspace manifests): `pbrs` is `0.1.0`,
`pbrs-grpc` is `0.1.0-alpha.1`, and `protobuf-tonic` is
`0.1.0-alpha.1`. The gRPC hub and both other READMEs already use
crates.io version requirements. Replacement text (DX-03 owns the
edit; this card only identifies it):

```toml
[dependencies]
pbrs = "0.1"
protobuf-tonic = "0.1.0-alpha.1"
```

No other `until these crates are on crates.io` or git-dependency
instruction survives outside this map's own quotation above.

## 7. Duplicated prose

- `Distinct from...` permutations: 0 occurrences in
  [grpc.md](grpc.md), [status.md](status.md),
  [architecture.md](architecture.md), [pbrs-grpc
  README](../pbrs-grpc/README.md), [benchmarks.md](benchmarks.md),
  the root README, and the tonic README. The 32 remaining
  occurrences in [examples/greeter/README.md](../examples/greeter/README.md)
  are pinned by the section 5.1 tests; remove prose and tests
  together.
- Install and quickstart blocks are intentionally repeated once
  per crate entry point (root README, gRPC hub, `pbrs-grpc`
  README, tonic README). That is the accepted cost of one
  canonical page per journey; do not factor them into a shared
  include, and do not let a fourth copy grow inside a guide.
- Framework comparisons live only in the [comparison
  guide](guides/comparison.md) plus the summary tables in
  [architecture.md](architecture.md) and the `pbrs-grpc` README.
  New comparisons go in the comparison guide.

## 8. Landing-page specification

The concise landing page this card proposes is landed; this
section records the shape to preserve:

- [grpc.md](grpc.md) (206 lines): what `pbrs-grpc` is,
  crates.io quickstart, four-shape summary, five production
  signpost cards linking into `guides/`, the threat-model
  section, the retry policy, and explicit omissions. Budget:
  stay under ~250 lines; new detail goes in a guide.
- [pbrs-grpc README](../pbrs-grpc/README.md) (142 lines):
  badges, pre-release scope box, install, quickstart, feature
  list, invariant table, documentation links. Budget: stay under
  ~200 lines.
- [status.md](status.md) (172 lines): compatibility matrix and
  recorded evidence. It is not a second tutorial layer; do not
  regrow guides inside it.

## 9. Maintenance contract

Any edit to this map or to a page it references must keep green:

```text
export CARGO_TARGET_DIR=/tmp/pb-target-dx
cargo test --test documentation
```

That suite enforces the map's own validity: every markdown link
resolves to an existing file and anchor, every backticked `docs/`
path in this file exists, and every guide snippet parses. When
adding a page, add its journey row in section 2 and its domain
row in section 1 in the same change.
