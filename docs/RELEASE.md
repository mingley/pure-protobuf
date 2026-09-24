# Release Policy & Publishing

There is **one** crates.io publisher: [`.github/workflows/release.yml`](../.github/workflows/release.yml).
Incremental pushes to `main` do **not** publish.

[`.github/workflows/release-plz.yml`](../.github/workflows/release-plz.yml) used to
publish on every `main` push when `CRATES_IO_TOKEN` was set. It is disabled.
[`.github/workflows/first-publish.yml`](../.github/workflows/first-publish.yml) is
obsolete (the crates already exist on crates.io) and will not publish.

This repository does **not** use crates.io Trusted Publishing. The workflow does
not set `id-token: write` and does not authenticate with OIDC. Uploads use the
repository secret **`CRATES_IO_TOKEN`**, mapped to `CARGO_REGISTRY_TOKEN` for
`cargo publish`.

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
the corresponding MSRV CI jobs. Protobuf Edition 2024 generation is a separate,
unqualified feature and must not be inferred from the Cargo manifests.

Current manifests (check the files, not this table, before tagging): `pbrs`
`0.1.0`; adapters `0.1.0-alpha.1`. A `v1.0.0` tag does not promote the
adapters. The current source builds both adapters from checked descriptor sets
without `protoc`, but the already-published adapter alphas still need it;
that improvement reaches crates.io only when new versions are published.

## Next coordinated all-crate release

The next production-ready release must publish **new versions of all three**
crates: `pbrs`, `protobuf-tonic`, and `pbrs-grpc`. Before tagging, update every
package version and both adapters' `pbrs` dependency requirements, then
validate the lockfile, package contents, unpacked consumers, and exact-SHA
required CI. Rehearse all three packages with the existing dry-run publisher.
Do not trigger the publisher until each crate's applicable qualification and
promotion evidence has been reviewed.

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

1. Set `version` in the crate manifest(s) you intend to publish. Adapters may
   stay at `0.1.0-alpha.1` while `pbrs` moves.
2. Land that change on `main` (CI must be green; that still does **not**
   publish).
3. Tag the SHA with `v` plus a version that **matches at least one** crate
   manifest (for example `v0.1.0` for `pbrs` `0.1.0`, or
   `v0.1.0-alpha.1` for an adapter). Push the tag:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. The tag run re-executes required CI, then `./scripts/publish-crates.sh`
   in order `pbrs`, `protobuf-tonic`, `pbrs-grpc`. Already-published
   name/version pairs are skipped. After a new `pbrs` upload it waits for the
   crates.io index before the adapters.
5. On a successful tag publish it also creates a GitHub Release.

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
  API**, uses a registry token, uploads crates or creates a GitHub Release.
  Uncommitted crate sources fail rather than rehearsing stale code; isolated
  package consumers are checked in CI.
- To upload: set `dry_run` to **false** and type `publish` in `confirm`.
  Anything else fails without publishing.

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

For the coordinated three-crate release, rehearse the existing publisher on a
committed SHA without a registry token or upload:

```bash
DRY_RUN=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target \
  ./scripts/publish-crates.sh
```

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
`tests/package_consumer.rs`. Expected tarball names follow
`<name>-<version>.crate` from the manifests (for example `pbrs-0.1.0.crate`).
