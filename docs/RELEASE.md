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

Current manifests (check the files, not this table, before tagging): `pbrs`
`0.1.0`; adapters `0.1.0-alpha.1`. A `v1.0.0` tag does not promote the
adapters.

## Required CI

`release.yml` calls [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)
on the **same SHA** and will not publish unless every required job succeeds:

| Job | What it runs |
|---|---|
| `test` | fmt, clippy `-D warnings`, `cargo test --workspace`, docs `-D warnings` |
| `grpc-interop` | `./scripts/grpc-interop.sh` |
| `conformance` | `./scripts/conformance.sh` |
| `msrv-core` | rustc **1.85**: `cargo test -p pbrs --lib` and `cargo test -p pbrs-grpc --lib` |
| `msrv-tonic` | rustc **1.88**: `cargo test -p protobuf-tonic` |
| `macos` | stable, `brew` protoc: `pbrs-grpc` `tcp::tests`, `--test pbrs_build`, `--test onboarding` |
| `package-consumers` | `cargo test --test package_consumer` (unpack `.crate` outside the workspace) |
| `generated-output` | onboarding `committed_hello_and_wkt_copies_match` and `codegen_stub_flavours_are_explicit` |

A failed or skipped required job blocks publish. Do not treat a previous green
`main` run as sufficient.

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

- `dry_run` defaults to **true**: `cargo publish --dry-run` only. No token,
  no crates.io upload, no GitHub Release.
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

`cargo publish` of a version that already exists would error; the probe makes
the retry idempotent.

## Local packing check

From a clean tree:

```bash
cargo publish -p pbrs --dry-run
cargo publish -p protobuf-tonic --dry-run
cargo publish -p pbrs-grpc --dry-run
```

Adapter dry-runs may warn or skip verify when the in-tree `pbrs` version is
not on crates.io yet. Isolated consumers of the unpacked `.crate` are
`tests/package_consumer.rs`. Expected tarball names follow
`<name>-<version>.crate` from the manifests (for example `pbrs-0.1.0.crate`).
