#!/usr/bin/env bash
# Run the official gRPC interop test cases against the pbrs-grpc kernel.
#
# Three passes, each one catching a different class of bug:
#
#   kernel client -> kernel server   both halves agree with each other
#   kernel client -> Go server       the kernel client speaks real gRPC
#   Go client     -> kernel server   the kernel server speaks real gRPC
#
# The Go passes need a Go toolchain and network access to fetch
# google.golang.org/grpc. Without either, they are skipped and the script
# still fails on a self-interop regression. Pass --self-only to skip them
# deliberately.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Cases that need nothing beyond the base TestService contract.
BASE_CASES=(
  empty_unary
  large_unary
  client_streaming
  server_streaming
  ping_pong
  empty_stream
  cancel_after_begin
  cancel_after_first_response
  timeout_on_sleeping_server
  custom_metadata
  status_code_and_message
  special_status_message
  unimplemented_method
  unimplemented_service
)

# Cases built on SimpleRequest.expect_compressed and response_compressed.
# grpc-go implements them on neither side: its interop server ignores both
# fields (v1.83.2 interop/test_utils.go, where UnaryCall reads neither), and its
# interop client rejects the case names outright. They therefore only run in the
# self-interop pass, against the one implementation here that does honour them.
COMPRESSION_CASES=(
  client_compressed_unary
  server_compressed_unary
  client_compressed_streaming
  server_compressed_streaming
)

CASES=("${BASE_CASES[@]}" "${COMPRESSION_CASES[@]}")

SELF_ONLY=0
[[ "${1:-}" == "--self-only" ]] && SELF_ONLY=1

PORT="${GRPC_INTEROP_PORT:-10000}"
GO_PORT="${GRPC_INTEROP_GO_PORT:-10001}"
PIDS=()

cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

wait_for_port() {
  local port="$1" name="$2"
  for _ in $(seq 1 100); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.1
  done
  echo "FAIL: $name never listened on $port" >&2
  return 1
}

echo "== building kernel interop binaries =="
cargo build --release -p pbrs-grpc \
  --bin pbrs-grpc-interop-server --bin pbrs-grpc-interop-client

KERNEL_SERVER="target/release/pbrs-grpc-interop-server"
KERNEL_CLIENT="target/release/pbrs-grpc-interop-client"

# Retry once: cancel_after_begin and timeout_on_sleeping_server race a 1 ms
# deadline against connection setup, so a single loaded-machine hiccup is not a
# regression. A real break fails both attempts.
run_case() {
  local client="$1" host="$2" port="$3" case="$4"
  local flag_prefix="$5"
  for _ in 1 2; do
    if "$client" "${flag_prefix}server_host" "$host" "${flag_prefix}server_port" "$port" \
        "${flag_prefix}test_case=$case" ${6:-} >/dev/null 2>&1; then
      return 0
    fi
  done
  return 1
}

run_kernel_client() {
  local host="$1" port="$2" label="$3"
  shift 3
  local cases=("$@")
  local failed=0
  for case in "${cases[@]}"; do
    if run_case "$KERNEL_CLIENT" "$host" "$port" "$case" "--"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      failed=1
    fi
  done
  if [[ $failed -ne 0 ]]; then
    echo "FAIL: kernel client against $label" >&2
    return 1
  fi
  echo "PASS: kernel client against $label (${#cases[@]} cases)"
}

echo "== kernel client -> kernel server =="
"$KERNEL_SERVER" --port "$PORT" &
PIDS+=($!)
wait_for_port "$PORT" "kernel interop server"
run_kernel_client 127.0.0.1 "$PORT" "kernel server" "${CASES[@]}"

if [[ $SELF_ONLY -eq 1 ]]; then
  echo "SKIP: cross-language passes (--self-only)"
  exit 0
fi

if ! command -v go >/dev/null 2>&1; then
  echo "SKIP: cross-language passes (no go toolchain)"
  exit 0
fi

GO_DIR="$(mktemp -d)"
cat >"$GO_DIR/go.mod" <<'EOF'
module grpcinterop

go 1.21
EOF

echo "== fetching Go gRPC interop peer =="
if ! (cd "$GO_DIR" && go get google.golang.org/grpc/interop/client google.golang.org/grpc/interop/server >/dev/null 2>&1); then
  echo "SKIP: cross-language passes (could not fetch google.golang.org/grpc)"
  exit 0
fi
if ! (cd "$GO_DIR" && go build -o go-interop-server google.golang.org/grpc/interop/server \
      && go build -o go-interop-client google.golang.org/grpc/interop/client) >/dev/null 2>&1; then
  echo "SKIP: cross-language passes (could not build the Go peer)"
  exit 0
fi

echo "== kernel client -> Go server =="
"$GO_DIR/go-interop-server" -port "$GO_PORT" -use_tls=false &
PIDS+=($!)
wait_for_port "$GO_PORT" "Go interop server"
run_kernel_client 127.0.0.1 "$GO_PORT" "Go server" "${BASE_CASES[@]}"
for case in "${COMPRESSION_CASES[@]}"; do
  echo "  skip $case (grpc-go does not implement it)"
done

echo "== Go client -> kernel server =="
go_failed=0
for case in "${BASE_CASES[@]}"; do
  if run_case "$GO_DIR/go-interop-client" 127.0.0.1 "$PORT" "$case" "-" "-use_tls=false"; then
    echo "  ok   $case"
  else
    echo "  FAIL $case"
    go_failed=1
  fi
done
for case in "${COMPRESSION_CASES[@]}"; do
  echo "  skip $case (grpc-go does not implement it)"
done
if [[ $go_failed -ne 0 ]]; then
  echo "FAIL: Go client against kernel server" >&2
  exit 1
fi
echo "PASS: Go client against kernel server (${#BASE_CASES[@]} cases)"
