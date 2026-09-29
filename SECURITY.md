# Security Policy

## Scope

Covered: the published crates `pbrs`, `pbrs-grpc`, and
`protobuf-tonic`, plus `protoc-gen-pbrs` as shipped from the core
package. Out of scope: benchmarks, examples, vendored `third_party/`
peers (report those to their upstreams), and unreleased `main`
snapshots except when the report shows the flaw would ship.

## Reporting a vulnerability

Report privately through GitHub Security Advisories for this
repository. Do not open a public issue for exploitable behavior,
credential exposure, parser crashes from untrusted input, or transport
security bypasses.

Include: affected crate and version, a minimal reproducer (input bytes
or config), observed versus expected behavior, and whether untrusted
network input can reach the flaw. Reports against `main` must name the
SHA; reports against releases must name the name/version.

## Response expectations (proposed)

These targets are proposed and take effect only after the maintenance
owner approves them; until then there is no SLA:

- acknowledge triage within five business days;
- keep the reporter updated through the private advisory thread;
- coordinate fix, release, and disclosure before going public;
- file or request a RUSTSEC advisory for fixed vulnerabilities in
  published versions.

No staffing, timeline, or bounty is promised. See
[the support policy](docs/support-policy.md) for the owner proposal.

## Supported versions

Only the latest published version of each crate receives security
fixes. Alpha and pre-release versions are unsupported except for the
newest alpha of that crate. See [the support
policy](docs/support-policy.md) for the version/MSRV table and the
patch/rollback rules.

## Dependency and supply-chain review

Every `main` and release SHA runs the `dep-audit` CI job
([ci.yml](.github/workflows/ci.yml)), which executes
[scripts/pure-rust-audit.sh](scripts/pure-rust-audit.sh): each
shipping profile (core, native gRPC, tonic adapter, Rust-only
codegen) must resolve to pure-Rust crates with approved licenses, no
C build steps, and no `links` keys outside the allowlist.
[deny.toml](deny.toml) additionally denies yanked advisories and
non-crates.io sources and enforces the license allowlist; each
`ignore` entry records its reason in the file and must be re-justified
when touched.

## Unsafe ownership

`unsafe` is owned by review, not by individuals: the workspace denies
`unsafe_code`, and every `unsafe` block or `unsafe fn` must carry a
`// SAFETY:` proof stating its preconditions, maintained invariants,
and why UB, out-of-bounds access, aliasing violations, and
use-after-free are impossible. Code without a satisfactory rationale
is rejected. The shared invariants live in
[the unsafe-invariants record](docs/unsafe-invariants.md). Miri, ASan,
and LSan runs are scheduled qualification evidence, not proofs; a
claim must cite the exact toolchain and artifact.
