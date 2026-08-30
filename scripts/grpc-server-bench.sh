#!/usr/bin/env bash
# Compare gRPC *server* latency across implementations with one variable.
#
# The same kernel client, the same `grpc.testing.TestService`, the same
# `.proto`, and the same payloads are pointed at two servers in turn: this
# kernel's, and grpc-go's reference interop server. The only thing that differs
# is the server, so the delta is a server delta.
#
# This is not the transport benchmark. `rpc-bench` compares the kernel against
# tonic with both sides under test and is what the process gates use; see
# docs/benchmarks.md. This script answers the narrower question of how the
# kernel's server compares to the most widely deployed gRPC implementation.
# Unary latency is the Xeon table. Ping-pong and upload are extra loopback
# axes; they do not replace that table.
#
# Needs a Go toolchain and network access to fetch google.golang.org/grpc.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

KERNEL_PORT="${GRPC_BENCH_KERNEL_PORT:-10100}"
GO_PORT="${GRPC_BENCH_GO_PORT:-10101}"
ROUNDS="${GRPC_BENCH_ROUNDS:-3}"
PIDS=()

cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

wait_for_port() {
  for _ in $(seq 1 100); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.1
  done
  echo "FAIL: nothing listening on $1" >&2
  return 1
}

echo "== building kernel binaries (release) =="
cargo build --release -p pbrs-grpc \
  --bin pbrs-grpc-interop-server --bin pbrs-grpc-interop-client

if ! command -v go >/dev/null 2>&1; then
  echo "SKIP: no go toolchain" >&2
  exit 0
fi

GO_DIR="$(mktemp -d)"
printf 'module grpcbench\ngo 1.21\n' >"$GO_DIR/go.mod"
echo "== building the Go reference server =="
if ! (cd "$GO_DIR" \
    && go get google.golang.org/grpc/interop/server >/dev/null 2>&1 \
    && go build -o go-interop-server google.golang.org/grpc/interop/server >/dev/null 2>&1); then
  echo "SKIP: could not build google.golang.org/grpc/interop/server" >&2
  exit 0
fi

target/release/pbrs-grpc-interop-server --port "$KERNEL_PORT" >/dev/null 2>&1 &
PIDS+=($!)
"$GO_DIR/go-interop-server" -port "$GO_PORT" -use_tls=false >/dev/null 2>&1 &
PIDS+=($!)
wait_for_port "$KERNEL_PORT"
wait_for_port "$GO_PORT"

echo "== kernel client, two servers, ${ROUNDS} rounds =="
echo "server        empty_p50  empty_p99  large_p50  large_p99  ping_pong_rps  upload_rps"
for round in $(seq 1 "$ROUNDS"); do
  for name in kernel go; do
    if [[ "$name" == kernel ]]; then port="$KERNEL_PORT"; else port="$GO_PORT"; fi
    line="$(target/release/pbrs-grpc-interop-client \
      --server_host 127.0.0.1 --server_port "$port" --bench)"
    # bench empty_p50=.. empty_p99=.. large_p50=.. large_p99=.. ping_pong_rps=.. upload_rps=..
    read -r _ e50 e99 l50 l99 ping upload <<<"$line"
    printf '%-13s %10s %10s %10s %10s %14s %10s   (round %s)\n' \
      "$name" "${e50#*=}" "${e99#*=}" "${l50#*=}" "${l99#*=}" \
      "${ping#*=}" "${upload#*=}" "$round"
  done
done
