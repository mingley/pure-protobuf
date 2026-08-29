#!/usr/bin/env bash
# Cloud Agent install for pbrs (pure-protobuf).
#
# Idempotent: safe to run repeatedly and against a warm cache or snapshot.
# Prepares the full development experience: the pinned Rust toolchain, the
# system packages the build and test scripts need, and a warmed build cache.
set -euo pipefail

# The workspace pins rust-version = 1.85, but the base image ships an older
# stable. Pull a current stable toolchain with the components CI uses
# (dtolnay/rust-toolchain@stable + rustfmt, clippy).
rustup toolchain install stable --profile minimal \
  --component rustfmt --component clippy
rustup default stable

# System packages:
#   protobuf-compiler  -> protoc for build.rs, the protoc-gen-pbrs plugin, and
#                         the codegen scripts (scripts/gen.sh, regen-*).
#   cmake + build-essential -> build the official conformance_test_runner and
#                         its bundled protoc (scripts/conformance.sh).
#   libstdc++-14-dev   -> the base image's default `c++` is clang, which selects
#                         the gcc-14 runtime; without its matching dev package
#                         cmake's compiler check fails with `cannot find -lstdc++`.
# The grpc-interop cross-language peer only needs the Go toolchain already in
# the base image.
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  protobuf-compiler cmake g++ build-essential libstdc++-14-dev

# Fetch dependencies and warm the build cache for the whole workspace so the
# first agent command is fast.
cargo build --workspace
