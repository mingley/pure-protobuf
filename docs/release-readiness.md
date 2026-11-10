# Release readiness

The next milestone is a qualified release of all three crates. Use this
checklist to collect the evidence, then follow the [release policy](RELEASE.md).
The [promotion matrix](ROADMAP.md#product-scope-and-promotion) defines the
requirements for each supported profile.

## Candidate versions

These versions are declared at [`713038c7`](https://github.com/mingley/pure-protobuf/commit/713038c7f2d60da41824531a7b81920183c7510b):

| Crate | Candidate | Scope |
|---|---|---|
| `pbrs` | `0.2.1` | Core protobuf |
| `pbrs-grpc` | `0.1.0-alpha.3` | Native gRPC preview |
| `protobuf-tonic` | `0.1.0-alpha.3` | Tonic adapter preview |

Both adapters require `pbrs 0.2.1` at runtime and build time. Recheck the
manifests and registry before tagging. The next coordinated release requires
three new versions; skipping an existing upload does not meet that goal.
Keep adapter alpha warnings until their promotion evidence supports a change.
The [implementation status](status.md) links the latest recorded CI, Miri and
package rehearsal. At `713038c7`, the [hosted dry run](https://github.com/mingley/pure-protobuf/actions/runs/42741860995)
packaged all three crates after its exact-source CI passed. Signed-tag
verification and crates.io authentication were skipped; no crates were
published. Final release evidence must match the release source.

## Release checklist

1. **Freeze the supported profiles and known gaps.** Core has no transport
   requirement. Bounded native deployments may use external discovery and load
   balancing; dynamic endpoint and retry claims have additional gates. The
   tonic adapter needs its own interoperability evidence.
2. **Complete sustained qualification.** Apply
   [GR-11](ROADMAP.md#gr-11-promote-narrowly-then-stabilize-the-public-api):
   24-hour qualification, a 7-day controlled soak, two independent adopter
   workloads, rollback triggers and a rollback rehearsal. Meet the applicable
   [QG-06 and QG-08 requirements](plan/world-class/tasks.json), including finite
   resource and recovery bounds and the security packet with at least
   24 aggregate CPU-hours of fuzzing. Retain failures, seeds and exclusions.
   For native transport workloads, follow the
   [resource campaign commands](resource-campaign.md).
3. **Record promotion and support decisions.** Obtain operator and maintainer
   signoff for each profile. Approve the maintenance owner, response
   expectations, supported versions and upgrade policy. The
   [support policy](support-policy.md) remains proposed. A 1.0 proposal also
   needs API compatibility review and a maintenance commitment.
4. **Pass every required CI job on the final SHA.** The
   [required checks](RELEASE.md#required-ci) include semver, interop,
   conformance, toolchains, dependency audit and isolated package consumers.
   Failed or skipped required jobs block publication. Later commits need
   their own checks.
5. **Rehearse the final packages and provenance.** Verify all three archives,
   the SBOM, `release-context.json`, its CI authority and the
   [artifact attestations](RELEASE.md#provenance-rehearsal-and-verification).
   Local packaging does not replace hosted rehearsal. Recorded checks in
   [status](status.md) apply to the source revisions they name.
6. **Publish through the sole release workflow.** Confirm Trusted Publisher
   settings separately for all three crates. Follow
   [cutting a release](RELEASE.md#cutting-a-release) with an annotated signed
   tag that GitHub marks verified and whose version matches a manifest.
7. **Verify all three uploads.** Confirm each new crates.io version and the
   release artifacts identify the qualified source. Follow
   [same-tag recovery](RELEASE.md#recovery) after a partial upload.

## Native transport memory

The configured `ByteBudgetTracker` limit applies to charged bytes. A
successful native send can release its charge after HTTP/2 queues data, before
the data is written or freed. Native receive buffers are not charged, and a
zero charge does not mean the transport has no retained buffers. See
[resource budgets](resource-budgets.md) for the accounting scope.

A strict aggregate transport-memory bound has not been established for the
native transport. Before claiming one, define its full scope and account for
the retained lifetime of receive buffers, queued and in-flight HTTP/2 data,
shared backing allocations, and relevant connection and stream state. Validate
the bound and recovery on the final source under concurrent slow readers and
writers, queue saturation, cancellation, reset, and shutdown. Retain evidence
under the existing [QG-06](plan/world-class/tasks.json) and
[GR-11](ROADMAP.md#gr-11-promote-narrowly-then-stabilize-the-public-api) gates.
The core protobuf profile has no transport requirement; the profile-specific
gates in the checklist remain in effect.

Known native transport limits include a demonstrated small-response burst
failure under the default client DATA budget, discarded non-final frames that
retain charges, and a reset-discard path that does not charge final DATA. The
[source-pinned witnesses](status.md#messaging-and-protocol-limits) are finite
tests, not rate or total-memory bounds. Validate behavior and any fix on the
release source and retain the resulting evidence.

## Retry and binary-log claims

Opt-in native unary hedging does not yet enforce A6 commitment when ordinary
response headers arrive: another attempt can continue or win. An outcome
becoming ready after the coordinator polls can still race a new send. The
receiver accepts
redundant leading zeros; the pbrs-grpc server emits canonical decimal values.
Client binary logs can include uncommitted retry or hedge attempts, omit an
early rejection trailer, or report a streaming peer's `CANCELLED` status as a
client cancellation. Unary cancellation logging starts after local validation
and concurrency admission; a never-polled call or earlier rejection can omit
the event. See [service-config](service-config.md) and the
[A6/A16 coverage notes](grfc.md).

Do not claim full A6 hedging or pushback conformance, or A16 binary-log
conformance, until the implementation is fixed and final-source tests cover
header commitment, hedge scheduling and pushback, committed versus discarded
attempts, early rejections, and cancellation. These are qualification
requirements for those claims; the profile-specific release gates above still
apply.

## Performance claims

Production promotion needs a benchmark baseline; benchmark victory is not a
GR-11 requirement. Speed claims also need the [benchmark contract](benchmark-contract.md),
named workloads and retained wins, losses and uncertainty. A green perf
workflow validates its configured checks; it does not establish overall
performance leadership. Start with the [results](benchmarks.md) and
[benchmark commands](benchmarks/running.md).
