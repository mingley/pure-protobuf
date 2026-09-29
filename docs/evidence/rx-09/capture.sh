#!/usr/bin/env bash
# RX-09 evidence collector; calls the unchanged devloop run-cell entry point.
# Run serially inside the recorded Linux image with /build and /out mounted.
set -euo pipefail

binary="${1:-/build/release/devloop}"
out="${2:-/out}"
mkdir -p "$out/raw" "$out/callgrind"
{
  printf 'source_sha=%s\n' "${PBRS_DEVLOOP_COMMIT:?source SHA required}"
  date -u +started_utc=%Y-%m-%dT%H:%M:%SZ
  uname -a
  rustc --version
  cargo --version
  valgrind --version
  /cargo/pinned-protoc-build/protoc --version
  printf 'online_processors='
  getconf _NPROCESSORS_ONLN
  printf 'TOKIO_WORKER_THREADS=%s\n' "${TOKIO_WORKER_THREADS:-unset}"
  sha256sum "$binary" /cargo/pinned-protoc-build/protoc \
    /work/bench/devloop/Cargo.lock /work/bench/devloop/src/main.rs
} > "$out/provenance.txt"

for cell in rpc.pbrs.unary rpc.tonic.unary rpc.pbrs.server_stream rpc.tonic.server_stream; do
  for repeat in 1 2 3; do
    for iters in 200 400; do
      stem="$cell.repeat$repeat.n$iters"
      printf 'capture %s\n' "$stem"
      valgrind --tool=callgrind --cache-sim=no \
        --callgrind-out-file="$out/callgrind/$stem.out" \
        "$binary" run-cell "$cell" --iters "$iters" --prepare-iters 400 \
        > "$out/raw/$stem.stdout" 2> "$out/raw/$stem.stderr"
    done
  done
done
date -u +finished_utc=%Y-%m-%dT%H:%M:%SZ >> "$out/provenance.txt"
