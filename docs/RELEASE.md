# Release Policy & Publishing

There is **one** crates.io publisher: [`.github/workflows/release.yml`](../.github/workflows/release.yml).
Incremental pushes to `main` do **not** publish.

[`.github/workflows/release-plz.yml`](../.github/workflows/release-plz.yml) used to
publish on every `main` push when `CRATES_IO_TOKEN` was set. It is disabled.
[`.github/workflows/first-publish.yml`](../.github/workflows/first-publish.yml) is
obsolete (the crates already exist on crates.io) and will not publish.

The publisher uses [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing/).
`rust-lang/crates-io-auth-action` exchanges the workflow's GitHub OIDC identity
for a short-lived `CARGO_REGISTRY_TOKEN`; the repository must not store a
long-lived crates.io API token. The workflow also emits a CycloneDX JSON SBOM,
creates a GitHub build-provenance attestation over the SBOM and packaged crate
archives, retains them as a workflow artifact, and attaches them to tagged
GitHub Releases.

An owner must configure a trusted publisher separately for **each** of `pbrs`,
`protobuf-tonic`, and `pbrs-grpc` in that crate's crates.io settings. Select
GitHub Actions and enter this repository's owner/name and workflow filename
`release.yml` (leave the environment blank unless the workflow is later moved
to a protected GitHub environment). The crates.io configuration is an external
operator prerequisite: source configuration alone cannot enable publication.

## Support and security policy

Supported versions, MSRV, deprecation, per-crate patch/rollback, and
provenance rules live in [the support policy](support-policy.md)
(proposed, pending owner approval). Vulnerability reporting and
response targets live in [the security policy](../SECURITY.md).

## Crates

Versions are read from each crate's `Cargo.toml` at the release SHA. They are
not lockstep and must not be hardcoded as `pbrs/0.1.0`.

| Crate | Role |
|---|---|
| `pbrs` | Core kernel. Publish first. |
| `protobuf-tonic` | tonic adapter. Depends on the `pbrs` version in its manifest. |
| `pbrs-grpc` | Native gRPC kernel. Depends on the `pbrs` version in its manifest. |

`examples/greeter` is `publish = false`.

Current source crates declare Rust language Edition 2024; their existing
`rust-version` floors (1.85 for core/native, 1.88 for tonic) still require
the corresponding MSRV CI jobs. Protocol Buffers Edition 2024 has a separate
[qualified subset](edition-2024.md); full conformance must not be inferred
from the Cargo manifests.

Current manifests (check the files, not this table, before tagging): `pbrs`
`0.2.0`; adapters `0.1.0-alpha.2`. A `v1.0.0` tag does not promote the
adapters. From `0.1.0-alpha.2` on, both adapters build from checked
descriptor sets without `protoc`; the older `0.1.0-alpha.1` archives still
need it.

## Next coordinated all-crate release

The next production-ready release must publish **new versions of all three**
crates: `pbrs`, `protobuf-tonic`, and `pbrs-grpc`. Before tagging, update every
package version and both adapters' `pbrs` dependency requirements, then
validate the lockfile, package contents, unpacked consumers, and exact-SHA
required CI. Rehearse all three packages with the existing dry-run publisher.
Do not trigger the publisher until each crate's applicable qualification and
promotion evidence has been reviewed.

Keep that release **small and cohesive**, not a catch-up bundle of unfinished
roadmap features. Choose modest, compatible version increments independently
per crate, describe only the qualified profile and changes it actually ships,
and keep unsupported or unqualified extensions out of the release claims.
The next coordinated release still requires three **new** versions; later
follow-up releases can be smaller and per-crate through the same workflow,
subject to the adapter dependency preflight.
Neither a green historical benchmark nor a bundled feature count overrides
the applicable safety, interoperability, package-consumer and operator gates.

The [adapter naming decision](decisions/better-tonic.md) keeps `pbrs`,
`pbrs-grpc` and `protobuf-tonic`. New adoption paths use optional features in
those crates. A future rename would need a consumer migration and publisher
update; it is not part of this release plan.

The publisher checks both runtime and build-time `pbrs` requirements in each
adapter against the core manifest version and local source path **before**
packaging or contacting crates.io. Mismatched constraints fail both the dry
run and real upload, leaving idempotent partial-release retries intact. This
preflight uses Python 3.11+ standard-library `tomllib`; it does not replace
the separate requirement that all three package versions be new.

The publisher deliberately skips a name/version already present on crates.io
so partial uploads can be retried safely. A successful workflow with unchanged
manifest versions would **not** meet this all-crate release goal. After the
tagged workflow publishes, verify that each new name/version is present on
crates.io and that the workflow recorded all three on the same release SHA.
Adapters remain free to use different versions from the core crate.

## Required CI

`release.yml` calls [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)
on the **same SHA** and will not publish unless every required job succeeds:

| Job | What it runs |
|---|---|
| `semver` | Pinned `cargo-semver-checks` compares all public APIs in `pbrs`, `pbrs-grpc`, and `protobuf-tonic` against their last published releases; a breaking change blocks publication. |
| `test` | fmt, strict Clippy for core targets and all gRPC/tonic/example libraries, fail-closed Python interop/benchmark/publisher and checked Edition 2024/v4 benchmark binding integrity contracts, `cargo test --workspace`, serial standalone rpc-bench/tonic-bench correctness tests (not performance gates), docs `-D warnings` |
| `grpc-interop` | pinned grpc-go and Go toolchain (version from `go.mod`), native directions, eight HTTP/2 negative-case adapters, and server framing/TLS probes; required matrix rows must pass |
| `grpc-interop-cpp` | pinned C++ peer in both directions: 14 standard and 4 compression cases per direction, with binary digests and retained logs |
| `conformance` | `./scripts/conformance.sh`: pinned required twice and recommended, each with separate 5,631 binary/JSON and 909 text assertions in the retained report; then regenerate the adapters' checked descriptor sets with pinned protoc and compare their emitted Rust bytes |
| `msrv-core` | rustc **1.85**: `cargo test -p pbrs --lib` and `cargo test -p pbrs-grpc --lib` |
| `msrv-tonic` | rustc **1.88**: `cargo test -p protobuf-tonic` |
| `macos` | stable, `brew` protoc: `pbrs-grpc` `tcp::tests`, `--test pbrs_build`, `--test onboarding` |
| `package-consumers` | `cargo test --test package_consumer`: assert the core archive excludes unrelated `third_party/`, docs and tests, then unpack all `.crate` archives outside the workspace and build consumers |
| `generated-output` | onboarding `committed_hello_and_wkt_copies_match` and `codegen_stub_flavours_are_explicit` |

A failed or skipped required job blocks publish. Do not treat a previous green
`main` run as sufficient.

Superseded direct `main`/PR CI runs cancel to avoid spending runner time on
outdated SHAs. The reusable CI invoked by `Release` is not cancelled by a later
development push; the publisher still requires every job on its exact SHA.

## Cutting a release

1. Set `version` in the crate manifest(s) you intend to publish. For the
   **next coordinated release**, all three versions must be new and both
   adapters must require that core version. In later per-crate releases,
   unchanged package versions can be skipped only if both adapters' runtime
   and build dependency requirements still match the core manifest. A new
   core version needs aligned source requirements even if an adapter version
   stays put; verify that its *already-published* requirement admits the new
   core, and never claim a skipped adapter gained unpublished source changes.
2. Land that change on `main` (CI must be green; that still does **not**
   publish).
3. Sign an annotated tag on the SHA with `v` plus a version that **matches at least one** crate
   manifest (for example `v0.2.0` for `pbrs` `0.2.0`, or
   `v0.1.0-alpha.2` for an adapter). Push the tag:
   ```bash
   git tag -s v0.2.0 -m 'Release v0.2.0'
   git push origin v0.2.0
   ```
   The workflow rejects lightweight tags, unsigned tags, and signatures that
   GitHub does not mark verified.
4. The tag run re-executes required CI, obtains a short-lived crates.io token
   through OIDC, then runs `./scripts/publish-crates.sh`
   in order `pbrs`, `protobuf-tonic`, `pbrs-grpc`. Already-published
   name/version pairs are skipped. After a new `pbrs` upload it waits for the
   crates.io index before the adapters.
5. It packages all three crates, generates a CycloneDX JSON SBOM, and requests
   a GitHub build-provenance attestation for those artifacts. On a successful
   tag publish it creates a GitHub Release containing the `.crate` archives and
   SBOM. The attestation remains independently verifiable through GitHub's
   artifact-attestation API and CLI.

Publishers are serialized (`concurrency: crates-io-publish`) so two tags cannot
upload at once.

### Manual dispatch (rehearsal or confirmed upload)

**Actions → Release → Run workflow**:

- `dry_run` defaults to **true**: the publisher packs each crate with
  `cargo package --no-verify --offline` in a disposable checkout of the
  committed SHA. Adapters resolve the matching local `pbrs` there without
  rewriting the source lockfile; the patch does not change packaged manifests
  or real uploads. A fresh runner first fetches cached dependencies (network
  required), but the rehearsal never queries the crates.io **version-status
  API**, uses a registry token, uploads crates or creates a GitHub Release. It
  does generate and retain the same SBOM, `.crate` archives, and provenance
  attestation as the publish path, making dispatch the release rehearsal.
  Uncommitted crate sources fail rather than rehearsing stale code; isolated
  package consumers are checked in CI.
- To upload, dispatch the workflow **on a signed `v*` tag**, set `dry_run` to
  **false**, and type `publish` in `confirm`. Anything else fails without
  publishing. This preserves signed-tag provenance during partial retries.

### Provenance rehearsal and verification

Run a dry dispatch from the candidate commit. After it succeeds, download the
`release-provenance-<sha>` workflow artifact and confirm that it contains three
`.crate` archives and `pure-protobuf-<sha>.cdx.json`. Verify any downloaded
file against GitHub's attestation service with:

```bash
gh attestation verify pbrs-<version>.crate --repo mingley/pure-protobuf
gh attestation verify pure-protobuf-<sha>.cdx.json --repo mingley/pure-protobuf
```

The attestation binds each artifact digest to the workflow identity and source
SHA. It does not sign the Git tag; tag signature verification is the separate,
fail-closed check before an upload.

## Recovery

If `pbrs` reached crates.io and an adapter failed (index lag, token, network):

1. Do **not** yank `pbrs` or bump versions just to retry.
2. Re-run the same `release.yml` on the same tag (or dispatch with
   `dry_run=false` and `confirm=publish` on that SHA).
3. The script probes `https://crates.io/api/v1/crates/<name>/<version>` from
   the manifests. Versions already on the index succeed without
   `cargo publish`. Missing versions are published and waited on.
   Only an explicit HTTP 404 means a version is missing: network failures,
   rate limits and other HTTP responses stop the publisher before any upload.

`cargo publish` of a version that already exists would error; the probe makes
the retry idempotent.

## Local packing check

For a fast local packaging-only check, rehearse on a committed SHA without a
registry token or upload:

```bash
DRY_RUN=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target \
  ./scripts/publish-crates.sh
```

This local command does not have a GitHub OIDC identity and therefore cannot
produce the hosted attestation. Use the dry workflow dispatch for the required
provenance rehearsal.

It packs `pbrs`, `protobuf-tonic`, and `pbrs-grpc` in a disposable checkout,
reusing the root Cargo target. This does not query crates.io or verify that
the manifest versions are new. Individual packaging checks from a clean tree
are also available:

```bash
cargo publish -p pbrs --dry-run
cargo publish -p protobuf-tonic --dry-run
cargo publish -p pbrs-grpc --dry-run
```

Adapter dry-runs may warn or skip verify when the in-tree `pbrs` version is
not on crates.io yet. Isolated consumers of the unpacked `.crate` are
`tests/package_consumer.rs`; it packs all three crates in one `cargo package`
call so adapters resolve the unpublished core without rewriting `Cargo.lock`.
A core change that adapters rely on (for example a new feature) therefore
needs a new core version: an already-published version wins over the local
copy. Expected tarball names follow
`<name>-<version>.crate` from the manifests (for example `pbrs-0.2.0.crate`).
