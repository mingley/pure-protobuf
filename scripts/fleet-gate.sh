#!/usr/bin/env bash
# Quick pre-push gate for parallel workers pushing directly to main.
#
#   scripts/fleet-gate.sh
#
# Runs the checks that most often break main when several workers push in
# parallel: formatting, strict lib clippy, pbrs-grpc lib/rpc/serving tests
# (serving pins rustdoc sentences), and the documentation contract. It is a
# floor, not a replacement for the area-specific gates in
# docs/plan/world-class/README.md.
set -euo pipefail

cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}"

cargo fmt --check
cargo clippy --lib --all-features -p pbrs -p pbrs-grpc -p protobuf-tonic \
  -p pbrs-grpc-example-greeter -- -D warnings
cargo test -q -p pbrs --lib
cargo test -q -p pbrs-grpc --lib --test rpc --test serving
cargo test -q --test documentation
echo "fleet-gate: ok"
