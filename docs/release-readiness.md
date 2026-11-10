# Release readiness

The next milestone is a qualified release of all three crates. Use this
checklist to collect the evidence, then follow the [release policy](RELEASE.md).
The [promotion matrix](ROADMAP.md#product-scope-and-promotion) defines the
requirements for each supported profile.

## Candidate versions

These versions are declared at [`d02cf609`](https://github.com/mingley/pure-protobuf/commit/d02cf609ac1b0ccd142fad7790b1d6bc2416936c):

| Crate | Candidate | Scope |
|---|---|---|
| `pbrs` | `0.2.1` | Core protobuf |
| `pbrs-grpc` | `0.1.0-alpha.3` | Native gRPC preview |
| `protobuf-tonic` | `0.1.0-alpha.3` | Tonic adapter preview |

Both adapters require `pbrs 0.2.1` at runtime and build time. Recheck the
manifests and registry before tagging. The next coordinated release requires
three new versions; skipping an existing upload does not meet that goal.
Keep adapter alpha warnings until their promotion evidence supports a change.
The [implementation status](status.md) links the recorded CI and Miri checks.
At `d02cf609`, the [hosted dry run](https://github.com/mingley/pure-protobuf/actions/runs/42882110957)
packaged all three crates after its [exact-source CI](https://github.com/mingley/pure-protobuf/actions/runs/42873027919)
passed. An [independent readback](https://github.com/mingley/pure-protobuf/actions/runs/42882645797)
checked the manifests, selected source files, CycloneDX SBOM, dry-run CI
context and all five attestations. The [artifact](https://github.com/mingley/pure-protobuf/actions/runs/42882110957/artifacts/13794203208)
has ZIP SHA-256 `a1d78ef5a8c65dbd66f34bdf0eaf18784068df347045801f3ebaebd66dd1ebc7`
and expires February 8, 2027. Signed-tag verification and crates.io OIDC
authentication were skipped; no crates were published. These checks apply to
`d02cf609`, not later source revisions.

Earlier [9ac rehearsal](https://github.com/mingley/pure-protobuf/actions/runs/42817437789)
and [readback](https://github.com/mingley/pure-protobuf/actions/runs/42863449492),
[cab0 rehearsal](https://github.com/mingley/pure-protobuf/actions/runs/42805278973)
and [readback](https://github.com/mingley/pure-protobuf/actions/runs/42805764438),
[5ff](https://github.com/mingley/pure-protobuf/actions/runs/42794817426) and
[5fe](https://github.com/mingley/pure-protobuf/actions/runs/42773096505)
remain available. Final release evidence must match the release source.

A [registry check](https://github.com/mingley/pure-protobuf/actions/runs/42863659596)
on November 10, 2026 found published `pbrs` `0.2.0` and published
`pbrs-grpc` and `protobuf-tonic` `0.1.0-alpha.2`. A
[published-package smoke check](https://github.com/mingley/pure-protobuf/actions/runs/42863675030)
generated and ran one unary greeter with those published core and native gRPC
versions on x86-64 Linux, Rust 1.85.0 and 1.99.1, and protoc 3.21.12. It
retains the resolved [1.85](https://github.com/mingley/pure-protobuf/actions/runs/42863675030/artifacts/13786960624)
and [1.99](https://github.com/mingley/pure-protobuf/actions/runs/42863675030/artifacts/13785613505)
lockfiles until February 8, 2027. This one example does not qualify the source
candidates or other workloads.

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
   conformance, i686 transport, toolchains, dependency audit and isolated
   package consumers.
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

At `f73b85d`, [clean i686 checks](https://github.com/mingley/pure-protobuf/actions/runs/42787512926)
failed on Rust 1.85 and 1.99 with default and no-default features because the
unconditional `graviola` dependency rejected that architecture.

At `9acfc0b6`, the built-in TLS provider is selected on x86-64 and AArch64.
On other architectures, its built-in TLS constructors return `UNIMPLEMENTED`
after preliminary checks; provider-dependent key, certificate and verifier
checks do not run. Plaintext and caller-owned rustls configurations remain
available where the target and chosen provider support them. On that revision,
the [required i686 CI cells](https://github.com/mingley/pure-protobuf/actions/runs/42810163563)
passed with Rust 1.85 and stable. They cover plaintext RPC, built-in TLS error
behavior, and caller-configured mTLS and rejection cases; the ring provider is
confined to the test application. The earlier
[prototype check](https://github.com/mingley/pure-protobuf/actions/runs/42789266070)
also exercised plaintext and caller-configured mTLS.

These checks do not establish full platform or deployment qualification. The
core `pbrs` alloc-only profile and i686 alloc-cell Miri evidence do not cover
the gRPC crate. Required checks must pass again on the final release SHA.

## Native transport memory

The configured `ByteBudgetTracker` limit applies to charged bytes. A
successful native send can release its charge after HTTP/2 queues data, before
the data is written or freed. Native receive buffers are not charged, and a
zero charge does not mean the transport has no retained buffers. See
[resource budgets](resource-budgets.md) for the accounting scope.

A [one-hour release-mode preview](https://github.com/mingley/pure-protobuf/actions/runs/42872166447)
on source `d02cf609` completed 1,117 cycles in 3,601.6 seconds (3,600
requested). The shared-host run covered loopback plaintext and TLS with
identity and gzip, up to two active RPCs, 8 MiB client and server byte trackers,
and a 2 MiB message limit. The process had a 1 GiB address-space limit and a
limit of 128 file descriptors. Fault recovery used a separate warmed
plaintext connection to the same server.

The smoke validator passed, but `qualified=false` and the 24-hour soak was
not run. An [independent readback](https://github.com/mingley/pure-protobuf/actions/runs/42884127798)
checked the source and archive digest and reran the validator. The
[retained artifact](https://github.com/mingley/pure-protobuf/actions/runs/42872166447/artifacts/13793279919)
has ZIP SHA-256 `9a6da2696c880f349e7153eb7a6b55c1d1ba1fa47aed2c0071f292654a5d15ee`
and expires February 8, 2027. Independent endpoint attribution, allocator
high-water, kernel socket memory, TLS reset/GOAWAY injection, dedicated-host
performance and other release gates were not covered. This preview does not
establish a memory bound or release qualification.

An [earlier 600-second preview](https://github.com/mingley/pure-protobuf/actions/runs/42803320936)
on source `cab0e580` completed 186 cycles in 601.3 seconds. Its
[independent readback](https://github.com/mingley/pure-protobuf/actions/runs/42805486784)
and [artifact](https://github.com/mingley/pure-protobuf/actions/runs/42803320936/artifacts/13758136824)
retain that unqualified result. An
[earlier 120-second preview](https://github.com/mingley/pure-protobuf/actions/runs/42771958343)
ran on source `5fe70efd`.

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

A [source-pinned witness](status.md#messaging-and-protocol-limits) on 713038c7
showed that the default client budget rejected the 104th parked 9-byte
response. Default clients now have a bounded reserve of up to 1,024 queued
small responses per connection, one per stream. Tests on 3cfc1311 covered
bursts through 1,024, the 1,127/1,128 frame boundary, and 1,024 parked
responses after a Channel redial. An explicit client DATA budget disables
the reserve.

Queued and discarded non-final small DATA still share the configured budget.
Successful reads can credit previous discarded charges in overhead units, but
a saturated budget can reject a new frame before any read. Repeated late final
DATA after a local reset is not charged, and untracked DATA above the local
GOAWAY cutoff bypasses these framing counters. The finite witnesses do not
establish a rate, lifetime-frame or total-memory bound. Validate the final
release source and retain its evidence.

## Retry and binary-log claims

Opt-in native unary hedging does not yet enforce A6 commitment when ordinary
response headers arrive: another attempt can continue or win. An outcome
becoming ready after the coordinator polls can still race a new send. The
receiver accepts
redundant leading zeros; the pbrs-grpc server emits canonical decimal values.
Client binary logs can include uncommitted retry or hedge attempts or omit an
early rejection trailer. A streaming peer or codec `CANCELLED` can be reported
as a client cancellation, and some local streaming cancellations can be missed.
With binary logging enabled, an explicit native `StreamSender::fail` can emit
`Cancel` after the upload loop reaches it. A queued failure bypassed by an
earlier error or final response does not itself emit an event. A Tower input
error does not establish an application cancellation request; existing
status-based inference can still emit an event for `CANCELLED`.

Unary cancellation logging starts after local validation and concurrency
admission; a never-polled call or earlier rejection can omit the event. See
[service-config](service-config.md) and the [A6/A16 coverage notes](grfc.md).

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
