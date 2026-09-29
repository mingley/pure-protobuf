#!/usr/bin/env bash
# Run scripts/devloop.sh inside a pinned Linux container.
#
# This gives macOS workers deterministic instruction counts through
# valgrind/callgrind while keeping host artifacts (for example --out
# target/devloop/linux.json) in the checkout. Cargo registry/git data and the
# devloop target directory live in Docker named volumes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

IMAGE="${PBRS_DEVLOOP_LINUX_IMAGE:-pbrs-devloop-linux:rust-1.98-bookworm}"
BASE_IMAGE="${PBRS_DEVLOOP_LINUX_BASE:-rust:1.98-bookworm}"
CARGO_VOLUME="${PBRS_DEVLOOP_LINUX_CARGO_VOLUME:-pbrs-devloop-linux-cargo}"
DEVLOOP_TARGET_VOLUME="${PBRS_DEVLOOP_LINUX_TARGET_VOLUME:-pbrs-devloop-linux-devloop-target}"
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"
HOST_COMMIT="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"

if ! command -v docker >/dev/null 2>&1; then
  echo "devloop-linux: docker is not on PATH" >&2
  exit 1
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  docker build \
    --build-arg "BASE_IMAGE=$BASE_IMAGE" \
    -t "$IMAGE" - <<'DOCKERFILE'
ARG BASE_IMAGE
FROM ${BASE_IMAGE}

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates \
        cmake \
        g++ \
        git \
        gosu \
        make \
        pkg-config \
        protobuf-compiler \
        valgrind \
    && rm -rf /var/lib/apt/lists/*
DOCKERFILE
fi

TTY=()
if [ -t 1 ]; then
  TTY=(-t)
fi

docker run --rm -i "${TTY[@]}" \
  --user root \
  --workdir /work \
  --volume "$ROOT:/work" \
  --volume "$CARGO_VOLUME:/cargo" \
  --volume "$DEVLOOP_TARGET_VOLUME:/work/bench/devloop/target" \
  --env CARGO_HOME=/cargo \
  --env HOME=/cargo \
  --env CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}" \
  --env PBRS_PROTOC_JOBS="${PBRS_PROTOC_JOBS:-3}" \
  --env PBRS_DEVLOOP_COMMIT="$HOST_COMMIT" \
  --env HTTP_PROXY="${HTTP_PROXY:-}" \
  --env HTTPS_PROXY="${HTTPS_PROXY:-}" \
  --env NO_PROXY="${NO_PROXY:-}" \
  --env http_proxy="${http_proxy:-}" \
  --env https_proxy="${https_proxy:-}" \
  --env no_proxy="${no_proxy:-}" \
  "$IMAGE" \
  bash -s -- "$HOST_UID" "$HOST_GID" "$@" <<'INNER'
set -euo pipefail
HOST_UID="$1"
HOST_GID="$2"
shift 2

chown -R "$HOST_UID:$HOST_GID" /cargo /work/bench/devloop/target
gosu "$HOST_UID:$HOST_GID" env HOME=/cargo git -C /tmp config --global --add safe.directory /work
gosu "$HOST_UID:$HOST_GID" env HOME=/cargo git -C /tmp config --global --add safe.directory /work/third_party/protobuf

if [ ! -d /work/third_party/protobuf ]; then
  gosu "$HOST_UID:$HOST_GID" env HOME=/cargo ./scripts/fetch-protobuf.sh
fi

PIN="$(cat /work/vendor/google/PIN)"
SHA="$(cat /work/vendor/google/SHA)"
PROTOC_BUILD=/cargo/pinned-protoc-build
PROTOC="$PROTOC_BUILD/protoc"
STAMP="$PROTOC_BUILD/.pbrs-protoc-pin"
ACTUAL_SHA="$(gosu "$HOST_UID:$HOST_GID" env HOME=/cargo git -C /work/third_party/protobuf rev-parse HEAD)"
if [ "$ACTUAL_SHA" != "$SHA" ]; then
  echo "Pinned protobuf source mismatch: expected $SHA, got $ACTUAL_SHA" >&2
  exit 1
fi
if [ ! -x "$PROTOC" ] || [ ! -f "$STAMP" ] || [ "$(cat "$STAMP")" != "$PIN $SHA" ] ||
   [ "$("$PROTOC" --version)" != "libprotoc ${PIN#v}" ] ||
   [[ "$("$PROTOC" --help)" != *'--rust_out=OUT_DIR'* ]]; then
  gosu "$HOST_UID:$HOST_GID" cmake -S /work/third_party/protobuf -B "$PROTOC_BUILD" \
    -Dprotobuf_BUILD_CONFORMANCE=OFF \
    -Dprotobuf_BUILD_TESTS=OFF
  gosu "$HOST_UID:$HOST_GID" cmake --build "$PROTOC_BUILD" \
    --parallel "${PBRS_PROTOC_JOBS:-3}" --target protoc
fi
printf '%s %s\n' "$PIN" "$SHA" > "$STAMP"
echo "Pinned Linux protoc ready: $PROTOC ($("$PROTOC" --version))"

gosu "$HOST_UID:$HOST_GID" env HOME=/cargo cargo fetch --manifest-path bench/devloop/Cargo.toml

export PATH="$PROTOC_BUILD:$PATH"
exec gosu "$HOST_UID:$HOST_GID" env \
  CARGO_HOME=/cargo \
  HOME=/cargo \
  CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}" \
  PBRS_DEVLOOP_COMMIT="${PBRS_DEVLOOP_COMMIT:-unknown}" \
  PATH="$PATH" \
  ./scripts/devloop.sh "$@"
INNER
