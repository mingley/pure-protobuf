# Release Policy & Publishing

This repository uses automated GitHub Actions workflows to publish crates to [crates.io](https://crates.io) and create GitHub Releases.

## Crate Publishing Overview

The workspace contains:
1. **Core Kernel**: Published as the primary crate (currently `pbrs`).
2. **Adapters & Frameworks**: (`protobuf-tonic`, `pbrs-grpc`) will be published once naming and dependencies are finalized.

## Setup & Publishing Workflows

### 1. First Publish (Manual Dispatch)

For the very first release of a crate name on crates.io, crates.io requires a token with `publish-new` permissions (Trusted Publishing can only be configured *after* a crate already exists on crates.io).

1. Generate a crates.io API token with `publish-new` and `publish-update` scopes at [crates.io/settings/tokens](https://crates.io/settings/tokens).
2. Add it as a GitHub repository secret: **Settings → Secrets and variables → Actions → `CARGO_REGISTRY_TOKEN`**.
3. In GitHub Actions, navigate to the **First publish (crates.io)** workflow.
4. Click **Run workflow**, enter `publish` in the confirmation box, and run the workflow.
5. The workflow verifies packaging, uploads the crate to crates.io, and probes the registry index until it appears live.

### 2. Subsequent Releases (Tag-Driven or Dispatch)

Once the crate exists on crates.io:

#### Recommended: Configure Trusted Publishing (OIDC)
1. Go to `https://crates.io/crates/<crate_name>/settings`.
2. Under **Trusted Publishing**, select **Add a Publisher**.
3. Choose **GitHub**:
   - Owner: `mingley`
   - Repository: `pure-protobuf`
   - Workflow filename: `release.yml`
4. Once configured, future publishes authenticate via short-lived OIDC tokens, making the long-lived repository secret unnecessary.

#### Cutting a Release
1. Update `version` in `Cargo.toml`.
2. Ensure changes are committed on `main`.
3. Push a version tag matching `vX.Y.Z`:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. `.github/workflows/release.yml` triggers automatically:
   - Validates code formatting, clippy lints, and test suite.
   - Verifies the tag matches the crate's `Cargo.toml` version.
   - Authenticates via OIDC Trusted Publishing (falling back to `CARGO_REGISTRY_TOKEN`).
   - Publishes to crates.io with `cargo publish --no-verify`.
   - Generates a GitHub Release with links to crates.io and docs.rs.
