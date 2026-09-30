# Support Policy (OB-05)

**Status:** proposed 2026-09-29 at `1a86bb57`; maintenance owner and
response expectations are **not approved yet** and take effect only
after approval. Until then this document describes current practice
and proposed targets, not commitments. No staffing, timeline,
automatic version bump, external submission, or publication is
promised.

## Supported versions and MSRV

| Crate | Source manifest version | MSRV | MSRV CI job |
|---|---|---|---|
| `pbrs` | `0.2.0` | 1.85 (`rust-version`) | `msrv-core` |
| `pbrs-grpc` | `0.1.0-alpha.2` | 1.85 (`rust-version`) | `msrv-core` |
| `protobuf-tonic` | `0.1.0-alpha.2` | 1.88 (`rust-version`) | `msrv-tonic` |

The table records source versions; check the registry and release evidence
before treating them as published versions. The proposed support policy is
to support only the latest published version of each crate.
Pre-release versions are supported only as the newest alpha of that
crate. MSRV floors are enforced by the CI jobs above; raising a floor
is a maintainer-level decision and must update the manifest, this
table, and the release notes together.

## Deprecation

The integration surface listed below is stable now, including in the current
`0.x` and alpha releases. Additive changes are allowed. A removal, rename, or
incompatible signature/trait change must first be deprecated in a published
release, remain usable through at least the next published release, and have
migration notes. Removing it then requires a version increment that Cargo
semver treats as breaking. Other public APIs follow normal Cargo semver:
stable (`>=1.0`) deprecations, once 1.0 exists, keep the old API working for
at least one minor version with migration notes before removal; APIs outside
the integration surface may change in a `0.x` minor or alpha, with every
removal called out in the release notes.
Protobuf Edition support is qualified separately (see
[the Edition 2024 contract](edition-2024.md)) and must never be
inferred from crate versions or Rust editions.

## Stable integration surface

The following adopter-facing contracts are covered by the deprecation rule
above and by the `Public API semver` CI job. Paths include their documented
constructors, builder methods, trait methods, and public associated types:

- **TLS injection:** `pbrs_grpc::{Identity, ServerTls, ClientTls,
  TlsHandshakeInfo, PeerIdentity}`, the `serve_tls*` entry points on `Server`,
  `Router`, and `Incoming`, and the `connect_tls*` entry points on `Channel`.
  This stabilizes today's TLS-value injection contract; caller-supplied rustls
  configuration and crypto-provider support remains explicitly out of scope
  until TC-25 lands.
- **Resolution and load balancing:** public items under
  `pbrs_grpc::resolver`, public policies and factory traits under
  `pbrs_grpc::lb`, `Target`, and the resolver/LB configuration methods on
  `Channel` and `ChannelConfig`.
- **Interception:** `Interceptor`, `ResponseInterceptor`,
  `ClientInterceptor`, `Intercepted`, `ServiceExt`, `Outgoing`, and the
  `intercept`/`on_response` entry points on channels, services, and servers.
- **Tower adapters:** `pbrs_grpc::tower_client::{UnaryService,
  ServerStreamingService, ClientStreamingService, BidiService}`,
  `pbrs_grpc::tower_server::{RouterService, TowerBody}`, and their conversion
  entry points.
- **Codec contracts:** `pbrs_grpc::codec::CodecMessage`, the optional
  `pbrs_grpc::codec::prost` adapters, and
  `protobuf_tonic::{ProtobufCodec, ProtobufEncoder, ProtobufDecoder}`.
- **Generated-code runtime:** the `pbrs` traits and types referenced by emitted
  source (`Message`, `Parse`, `Serialize`, `WireOut`, `RawMessage`, proxy,
  repeated/map/string/unknown-field types, and the public `runtime`, `rt`,
  `prelude`, and `gen_support` paths). Generator output and runtime from the
  same supported release line are a compatibility pair; generated source may
  use only public paths in this contract.

The CI comparison covers all public APIs of `pbrs`, `pbrs-grpc`, and
`protobuf-tonic`, which is deliberately stricter than the named minimum. It
compares against the last published releases (`0.2.0`, `0.1.0-alpha.2`, and
`0.1.0-alpha.2`, respectively), so deleting or incompatibly changing any
listed API fails before merge. CI explicitly selects the tool's `minor`
release policy: additive APIs are allowed, and an unchanged alpha version
cannot infer a major release that skips all compatibility checks.
The maintainer approved installing pinned
`cargo-semver-checks` 0.50.0 for this purpose on 2026-09-30; changing that pin
requires renewed review.

## Patch and rollback per crate

- Security and soundness fixes ship as the next patch (or alpha)
  version of the affected crate, alone, without bundling roadmap
  features.
- No backports to older minors except a coordinated, explicitly
  approved exception; the exception names its versions and evidence.
- Yank is reserved for broken or dangerous versions (fails to build,
  unsound, or a security hole with no mitigation). Never yank to
  retry a publish; partial-release recovery is defined in
  [the release policy](RELEASE.md) (Recovery: re-run on the same tag;
  the crates.io probe makes retries idempotent).
- Adapter releases honor the dependency preflight: an adapter must
  require the core version it was qualified against, verified before
  packaging.

## Release evidence requirements

A publish requires the required CI table on the exact release SHA
(see [the release policy](RELEASE.md)): fmt, strict Clippy, the
workspace test suite, gRPC/C++ interop, conformance with retained
reports, MSRV jobs, `macos`, package-consumer isolation (the core
archive excludes unrelated `third_party/`, docs, and tests), and
generated-output checks. Dry-run rehearsal via the existing publisher
is required before any confirmed upload.

## Source and license provenance

- Registry sources are crates.io-only (`unknown-registry` and
  `unknown-git` are denied in [deny.toml](../deny.toml)); no git or
  alternate-registry dependencies ship.
- Licenses are limited to the `deny.toml` allowlist (Apache-2.0, MIT,
  BSD-3-Clause, ISC, Unicode-3.0, Zlib, plus the recorded
  `webpki-roots` exception). The `dep-audit` CI job plus
  [scripts/pure-rust-audit.sh](../scripts/pure-rust-audit.sh) prove
  each shipping profile stays pure-Rust with approved licenses.
- The publisher packs from a disposable checkout of the committed
  SHA, probes `https://crates.io/api/v1/crates/<name>/<version>`
  before uploading, and records all published versions on the same
  release SHA. Only an explicit HTTP 404 means a version is missing.

## Maintenance owner (proposed)

- Publisher: there is exactly one, the `release.yml` workflow
  ([release policy](RELEASE.md)); incremental `main` pushes never
  publish.
- Human owner: to be named by maintainer approval. Proposed response
  targets (no SLA until approved): security-report triage
  acknowledgment within five business days per
  [the security policy](../SECURITY.md); coordinated fix, release,
  and disclosure through the private advisory thread.
