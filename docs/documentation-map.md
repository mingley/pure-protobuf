# Find the right documentation

Start with the task you want to finish. Guides explain how to use the
libraries; reference pages define exact behavior; evidence records what was
measured at a particular revision.

## Build or operate an application

| I want to… | Start here | Runnable code |
|---|---|---|
| Generate and use protobuf messages | [Main quickstart](../README.md#quick-start-install-generate-use) | [Onboarding consumers](../tests/onboarding.rs) |
| Build a native gRPC service | [gRPC guide](grpc.md) | [Greeter](../examples/greeter/README.md) |
| Choose an RPC shape | [Unary and streaming guide](guides/rpc-shapes.md) | [Greeter service](../examples/greeter/src/lib.rs) |
| Configure TLS, limits and shutdown | [Production service guide](guides/production-service.md) | [TLS tests](../pbrs-grpc/tests/tls.rs), [lifecycle tests](../pbrs-grpc/tests/lifecycle.rs) |
| Add authentication or metadata | [Interceptors guide](guides/interceptors.md) | [Greeter](../examples/greeter/src/lib.rs) |
| Add health, reflection and useful errors | [Operations guide](guides/operations.md) | [Health tests](../pbrs-grpc/tests/health.rs), [reflection tests](../pbrs-grpc/tests/reflection.rs) |
| Configure code generation | [Codegen guide](guides/codegen.md) | [Build integration tests](../tests/pbrs_build.rs) |
| Adopt pbrs from tonic/prost | [Migration guide](guides/migration.md) | [Tonic ports](../examples/tonic-ports/README.md) |
| Share a server with axum | [Co-hosting example](../examples/axum-cohost/README.md) | [Example source](../examples/axum-cohost/src/main.rs) |
| Keep tonic and use pbrs messages | [Adapter README](../protobuf-tonic/README.md) | [Adapter tests](../protobuf-tonic/tests/interop.rs) |
| Choose between stacks | [Comparison guide](guides/comparison.md) | [Benchmark guide](benchmarks.md) |
| Handle large payloads | [Ownership and copies](zero-copy.md) | [RPC benchmark](../rpc-bench/README.md) |

For API signatures and trait implementations, run
`cargo doc --workspace --no-deps --open`. Published APIs are also on
[docs.rs/pbrs](https://docs.rs/pbrs),
[docs.rs/pbrs-grpc](https://docs.rs/pbrs-grpc) and
[docs.rs/protobuf-tonic](https://docs.rs/protobuf-tonic).
Match published documentation to the version you use; unreleased source can
have additional features.

## Check support and exact behavior

[Implementation status](status.md) distinguishes source features from
qualification. The [support matrix](../README.md#support-matrix) lists tested
toolchains; [release policy](RELEASE.md), [support policy](support-policy.md)
and [security policy](../SECURITY.md) cover maintenance and publishing.

| Topic | Behavior reference |
|---|---|
| Retry and hedging | [Retry contract](retry-contract.md), [service config](service-config.md) |
| Discovery and traffic policy | [Resolver contract](resolver-contract.md), [xDS boundary](xds-contract.md) |
| Credentials | [Credentials contract](credentials-contract.md), [ALTS boundary](alts-contract.md) |
| Limits and retained memory | [Resource budgets](resource-budgets.md) |
| Generation and upgrades | [Output layout](codegen-layout.md), [compatibility](codegen-compatibility.md), [Edition 2024 subset](edition-2024.md) |
| Protocol inventory | [gRFC matrix](grfc.md), [cacheable RPC behavior](cacheable-rpc.md) |
| Parser safety | [Unsafe invariants](unsafe-invariants.md), [dependency checks](security.md) |

## Understand or improve the implementation

| Question | Read |
|---|---|
| How do the pieces fit? | [Architecture](architecture.md), [runtime design](design.md) |
| How does this differ from upb? | [API and kernel comparison](upb.md) |
| Why was a design chosen? | [Decision index](decisions/README.md) |
| What should I work on next? | [Current queue](../TODO.md), [repository audit](audit-2026-09-29.md) |
| What is the performance strategy? | [World-class gRPC program](plan/world-class/README.md) |
| What must change for existing prost/tonic systems to adopt pbrs? | [Adoption program](plan/adoption/README.md) |
| What are the task and review rules? | [Foundation execution contract](plan/README.md#small-executor-contract) |
| How are product profiles promoted? | [Roadmap](ROADMAP.md) |
| What happened to earlier experiments? | [Closed inventory](inventory/README.md) |

Run `python3 scripts/plan-status.py --ready` for current unclaimed work and
`python3 scripts/plan-status.py --card RX-09` for one task's full contract.
The JSON cards own live dependencies and status; Markdown explains priorities.

## Read or reproduce measurements

Start with the [benchmark guide](benchmarks.md), then use the
[scoreboard](scoreboard.md) and [evidence index](evidence/README.md).
The [benchmark contract](benchmark-contract.md) defines valid comparisons.
The [profiling guide](profiling.md) explains how to identify a limiting path.

A dated evidence note describes its recorded revision and host. It is not an
automatic claim about current source. Closed-loop smoke tests, contended-host
wall time and partial peer matrices must keep their limitations visible.

For protocol evidence, the executable entry points are
[conformance](../scripts/conformance.sh) and
[cross-peer gRPC interop](../scripts/grpc-interop.sh). Their reports and
exclusions support the [implementation status](status.md).

## Keeping documentation useful

Give each page one job: tutorial, task guide, behavior reference, decision,
plan or evidence. Link to the page that owns a fact instead of copying long
inventories. Start with the reader's task, use concrete API names and runnable
commands, and define abbreviations on first use.

Preserve useful inbound anchors when renaming headings. The gRPC guide keeps
explicit anchors for older links. Preserve raw historical measurements and
source pins; explain a newer result rather than silently rewriting an old one.
Leave fixture provenance and protocol contracts precise even when simplifying
their introductory prose.

The documentation checks cover local links, anchors, support caveats, versions,
claim language and snippet syntax. The greeter keeps one structural check for
its error-lifecycle table alongside runtime behavior tests. Syntax checks do
not type-check every recipe; QG-07 closes that gap with compiled examples.

Run these before landing documentation or task changes:

```sh
python3 scripts/plan-lint.py
python3 -B -m unittest tests.test_plan_status tests.test_plan_lint -q
cargo test --locked --test documentation
cargo test --locked -p pbrs-grpc-example-greeter
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
```

The documentation suite needs a working `protoc` for Protobuf snippets.
External URLs are not fetched by its offline link check. Rust example builds,
package consumers and CI provide separate checks beyond snippet parsing.
