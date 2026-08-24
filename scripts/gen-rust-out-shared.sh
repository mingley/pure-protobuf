#!/usr/bin/env bash
# Generate official rust_out (kernel=upb) and compile rust_out_shared tests.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/rust_out_shared"
exec cargo test -- --test-threads=1 "$@"
