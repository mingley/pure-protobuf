#!/usr/bin/env bash
# Run the official gRPC interop test cases against the C++ reference peer.
#
# Pinned official C++ peer (grpc/grpc @ d1487957db6658bc532b72871775148229836627 / v1.84.0).
#
# Tests both directions across all 14 base cases PLUS all 4 compression cases:
#   1. Native client (pbrs-grpc-interop-client) -> C++ server (interop_server)
#   2. C++ client (interop_client)             -> Native server (pbrs-grpc-interop-server)
#
# Unlike grpc-go (which ignores compression flags), the C++ reference peer fully
# implements and asserts wire-level compression for unary and streaming RPCs.
# Cross-language execution fails closed unless --self-only is explicitly passed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

INTEROP_REPORT="$ROOT/scripts/interop-report.py"

# Pinned C++ reference peer (matching tests/interop/cases.json):
# commit d1487957db6658bc532b72871775148229836627 (v1.84.0)
CPP_PEER_PIN="d1487957db6658bc532b72871775148229836627"
CPP_PEER_VERSION="v1.84.0 ($CPP_PEER_PIN)"
CPP_PEER_NAME="grpc-cpp"

# Cases that need nothing beyond the base TestService contract (14 cases).
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

# Compression cases (4 cases). Both pbrs-grpc and C++ reference peer implement
# expect_compressed and response_compressed message flags.
COMPRESSION_CASES=(
  client_compressed_unary
  server_compressed_unary
  client_compressed_streaming
  server_compressed_streaming
)

CASES=("${BASE_CASES[@]}" "${COMPRESSION_CASES[@]}")

SELF_ONLY=0
INCLUDE_SELF=0
SKIP_BUILD="${SKIP_BUILD:-${GRPC_INTEROP_SKIP_BUILD:-0}}"
LOG_DIR=""
GRPC_INTEROP_PORT_VAL=""
GRPC_INTEROP_CPP_PORT_VAL=""
CASE_TIMEOUT_SEC="${CASE_TIMEOUT_SEC:-${GRPC_INTEROP_CASE_TIMEOUT:-15}}"
STARTUP_TIMEOUT_SEC="${STARTUP_TIMEOUT_SEC:-${GRPC_INTEROP_STARTUP_TIMEOUT:-5}}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-${GRPC_INTEROP_MAX_ATTEMPTS:-2}}"
OVERALL_FAILED=0
USE_TLS="${USE_TLS:-${GRPC_INTEROP_USE_TLS:-0}}"
TLS_CA_FILE="${TLS_CA_FILE:-${GRPC_INTEROP_TLS_CA_FILE:-}}"
TLS_CERT_FILE="${TLS_CERT_FILE:-${GRPC_INTEROP_TLS_CERT_FILE:-}}"
TLS_KEY_FILE="${TLS_KEY_FILE:-${GRPC_INTEROP_TLS_KEY_FILE:-}}"

show_help() {
  cat << 'EOF'
Usage: scripts/grpc-interop-cpp.sh [OPTIONS]

Official gRPC C++ reference peer interop runner for pbrs-grpc.

Executes official interoperability test cases in both peer directions:
  1. Native client (pbrs-grpc-interop-client) -> C++ server (interop_server)
  2. C++ client (interop_client)             -> Native server (pbrs-grpc-interop-server)

Evaluates all 14 base cases plus the 4 official compression cases:
  Base Cases:
    empty_unary, large_unary, client_streaming, server_streaming, ping_pong,
    empty_stream, cancel_after_begin, cancel_after_first_response,
    timeout_on_sleeping_server, custom_metadata, status_code_and_message,
    special_status_message, unimplemented_method, unimplemented_service
  Compression Cases:
    client_compressed_unary, server_compressed_unary,
    client_compressed_streaming, server_compressed_streaming

Options:
  --help, -h                  Show this help message and exit
  --self-only                 Run only the self-interop pass (kernel client -> kernel server)
  --include-self              Include self-interop pass before running C++ peer passes
  --skip-build                Skip building kernel and C++ binaries (require existing binaries)
  --cases <LIST>              Comma-separated list of cases to execute (default: all 18 cases)
  --cases=<LIST>              Comma-separated list of cases to execute
  --log-dir <DIR>             Directory for execution logs and report.json
  --output-dir <DIR>          Alias for --log-dir
  --port <PORT>               Port for native kernel server (default: dynamically allocated)
  --cpp-port <PORT>           Port for C++ server (default: dynamically allocated)
  --use-tls, --use_tls        Run tests over TLS transport (http2_tls) instead of cleartext
  --tls-ca-file <FILE>        Path to TLS CA certificate file
  --tls-cert-file <FILE>      Path to server TLS certificate file
  --tls-key-file <FILE>       Path to server TLS private key file
  --timeout <SEC>             Per-case deadline in seconds (default: 15)
  --max-attempts <N>          Maximum execution attempts per case (default: 2)

Environment Variables:
  GRPC_INTEROP_CPP_CLIENT     Path to C++ interop_client binary
  GRPC_INTEROP_CPP_SERVER     Path to C++ interop_server binary
  GRPC_INTEROP_CPP_BIN_DIR    Directory containing C++ binaries (default: target/interop-cpp)
  GRPC_INTEROP_KERNEL_CLIENT  Path to native pbrs-grpc-interop-client
  GRPC_INTEROP_KERNEL_SERVER  Path to native pbrs-grpc-interop-server
  GRPC_INTEROP_SKIP_BUILD     If 1, skip binary builds
  GRPC_INTEROP_LOG_DIR        Directory for execution logs and report.json
  GRPC_INTEROP_CASES          Comma-separated case list override
  GRPC_INTEROP_PORT           Native server port
  GRPC_INTEROP_CPP_PORT       C++ server port
  GRPC_INTEROP_CASE_TIMEOUT   Per-case deadline in seconds
  GRPC_INTEROP_STARTUP_TIMEOUT Server startup deadline in seconds (default: 5)
  GRPC_INTEROP_MAX_ATTEMPTS   Maximum attempts per case (default: 2)
  GRPC_INTEROP_USE_TLS        If 1, use TLS transport
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      show_help
      exit 0
      ;;
    --self-only)
      SELF_ONLY=1
      shift
      ;;
    --include-self)
      INCLUDE_SELF=1
      shift
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --cases)
      GRPC_INTEROP_CASES="$2"
      shift 2
      ;;
    --cases=*)
      GRPC_INTEROP_CASES="${1#*=}"
      shift
      ;;
    --log-dir|--output-dir)
      LOG_DIR="$2"
      shift 2
      ;;
    --log-dir=*|--output-dir=*)
      LOG_DIR="${1#*=}"
      shift
      ;;
    --port)
      GRPC_INTEROP_PORT_VAL="$2"
      shift 2
      ;;
    --port=*)
      GRPC_INTEROP_PORT_VAL="${1#*=}"
      shift
      ;;
    --cpp-port|--peer-port)
      GRPC_INTEROP_CPP_PORT_VAL="$2"
      shift 2
      ;;
    --cpp-port=*|--peer-port=*)
      GRPC_INTEROP_CPP_PORT_VAL="${1#*=}"
      shift
      ;;
    --use-tls|--use_tls)
      USE_TLS=1
      shift
      ;;
    --use-tls=*|--use_tls=*)
      val="${1#*=}"
      case "$val" in
        true|1|yes) USE_TLS=1 ;;
        false|0|no) USE_TLS=0 ;;
        *) echo "invalid boolean for --use-tls: $val" >&2; exit 1 ;;
      esac
      shift
      ;;
    --tls-ca-file|--tls_ca_file)
      TLS_CA_FILE="$2"
      shift 2
      ;;
    --tls-ca-file=*|--tls_ca_file=*)
      TLS_CA_FILE="${1#*=}"
      shift
      ;;
    --tls-cert-file|--tls_cert_file)
      TLS_CERT_FILE="$2"
      shift 2
      ;;
    --tls-cert-file=*|--tls_cert_file=*)
      TLS_CERT_FILE="${1#*=}"
      shift
      ;;
    --tls-key-file|--tls_key_file)
      TLS_KEY_FILE="$2"
      shift 2
      ;;
    --tls-key-file=*|--tls_key_file=*)
      TLS_KEY_FILE="${1#*=}"
      shift
      ;;
    --timeout)
      CASE_TIMEOUT_SEC="$2"
      shift 2
      ;;
    --timeout=*)
      CASE_TIMEOUT_SEC="${1#*=}"
      shift
      ;;
    --max-attempts)
      MAX_ATTEMPTS="$2"
      shift 2
      ;;
    --max-attempts=*)
      MAX_ATTEMPTS="${1#*=}"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      echo "Use --help for usage information." >&2
      exit 1
      ;;
  esac
done

if [[ $USE_TLS -eq 1 ]]; then
  DEFAULT_TLS_DIR="$ROOT/pbrs-grpc/tests/tls_data"
  TLS_CA_FILE="${TLS_CA_FILE:-$DEFAULT_TLS_DIR/ca.crt}"
  TLS_CERT_FILE="${TLS_CERT_FILE:-$DEFAULT_TLS_DIR/server.crt}"
  TLS_KEY_FILE="${TLS_KEY_FILE:-$DEFAULT_TLS_DIR/server.key}"
  TRANSPORT="http2_tls"
else
  TRANSPORT="http2_cleartext"
fi

if [[ -n "${GRPC_INTEROP_CASES:-}" ]]; then
  IFS=', ' read -r -a CASES <<< "$GRPC_INTEROP_CASES"
fi

TIMESTAMP_PID="$(date +%Y%m%d_%H%M%S)_$$"
LOG_DIR="${LOG_DIR:-${GRPC_INTEROP_LOG_DIR:-$ROOT/target/interop-logs/$TIMESTAMP_PID}}"
mkdir -p "$LOG_DIR"
RESULTS_JSON="$LOG_DIR/results.json"
REPORT_JSON="$LOG_DIR/report.json"

find_free_port() {
  python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

PORT="${GRPC_INTEROP_PORT_VAL:-${GRPC_INTEROP_PORT:-}}"
if [[ -z "$PORT" ]]; then
  PORT="$(find_free_port)"
fi

CPP_PORT="${GRPC_INTEROP_CPP_PORT_VAL:-${GRPC_INTEROP_CPP_PORT:-}}"
if [[ -z "$CPP_PORT" ]]; then
  CPP_PORT="$(find_free_port)"
  while [[ "$CPP_PORT" == "$PORT" ]]; do
    CPP_PORT="$(find_free_port)"
  done
fi

TRACKED_PIDS=()
SCRATCH_PATHS=()

cleanup() {
  local exit_status=$?
  for pid in "${TRACKED_PIDS[@]:-}"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      for _ in {1..10}; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.05
      done
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
      fi
      wait "$pid" 2>/dev/null || true
    fi
  done
  for p in "${SCRATCH_PATHS[@]:-}"; do
    if [[ -e "$p" ]]; then
      rm -rf "$p"
    fi
  done
  if [[ -f "${RESULTS_JSON:-}" && ! -f "${REPORT_JSON:-}" ]]; then
    python3 "$INTEROP_REPORT" aggregate --results "$RESULTS_JSON" --output "$REPORT_JSON" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

stop_server() {
  local pid="$1"
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in {1..10}; do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -9 "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
  fi
}

start_server() {
  local name="$1"
  local port="$2"
  local log_file="$3"
  shift 3
  local server_cmd=("$@")

  # 1. Check if port is already occupied before starting server
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    exec 3<&- 3>&-
    echo "FAIL: port $port is already occupied before starting $name" >&2
    echo "Port $port occupied" >> "$log_file"
    return 1
  fi

  # 2. Start server in background with stdout/stderr redirected to log_file
  "${server_cmd[@]}" > "$log_file" 2>&1 &
  local pid=$!
  TRACKED_PIDS+=($pid)

  # 3. Wait for server to listen on port with startup deadline
  local deadline=$((SECONDS + STARTUP_TIMEOUT_SEC))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "FAIL: $name (pid $pid) died during startup" >&2
      if [[ -f "$log_file" ]]; then
        cat "$log_file" >&2
      fi
      return 1
    fi
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.05
  done
  echo "FAIL: $name (pid $pid) never listened on $port within ${STARTUP_TIMEOUT_SEC}s" >&2
  if [[ -f "$log_file" ]]; then
    cat "$log_file" >&2
  fi
  return 1
}

record_server_failure() {
  local case="$1" peer="$2" direction="$3" reason="$4" log_file="$5"
  local case_log="$LOG_DIR/${peer}-${direction}-${case}-attempt1.log"
  echo "$reason" > "$case_log"
  if [[ -f "$log_file" ]]; then
    cat "$log_file" >> "$case_log"
  fi
  local record_args=(
    python3 "$INTEROP_REPORT" record
    --output "$RESULTS_JSON"
    --case "$case"
    --status failed
    --duration-ms 0.0
    --peer "$peer"
    --direction "$direction"
    --transport "$TRANSPORT"
    --stdout-log "$case_log"
    --stderr-log "$case_log"
    --exit-code 1
    --attempt-count 1
    --notes "$reason"
  )
  if [[ "$peer" == "$CPP_PEER_NAME" ]]; then
    record_args+=(--peer-pin "$CPP_PEER_PIN")
  fi
  "${record_args[@]}" >/dev/null
}

run_attempt() {
  local deadline_sec="$1"
  local log_path="$2"
  shift 2
  python3 -c '
import subprocess, sys, time, os, signal

deadline_sec = float(sys.argv[1])
log_path = sys.argv[2]
cmd = sys.argv[3:]

start = time.perf_counter()
exit_code = 1

with open(log_path, "wb") as log_file:
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True
        )
        try:
            exit_code = proc.wait(timeout=deadline_sec)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            exit_code = 124
            log_file.write(f"\nCommand timed out after {deadline_sec}s\n".encode("utf-8"))
    except Exception as e:
        log_file.write(f"\nExecution error: {e}\n".encode("utf-8"))
        exit_code = 1

end = time.perf_counter()
duration_ms = (end - start) * 1000.0
print(f"{duration_ms:.2f} {exit_code}")
' "$deadline_sec" "$log_path" "$@"
}

run_case() {
  local client="$1"
  local host="$2"
  local port="$3"
  local case="$4"
  local peer="$5"
  local direction="$6"
  local flag_prefix="$7"
  shift 7
  local extra_args=("$@")

  local max_attempts="${MAX_ATTEMPTS:-2}"
  local attempt=1
  local first_attempt_status=""
  local first_duration_ms=0
  local total_duration_ms=0
  local last_log=""
  local last_exit_code=0
  local last_status="failed"

  local client_cmd=("$client" "${flag_prefix}server_host" "$host" "${flag_prefix}server_port" "$port" "${flag_prefix}test_case=$case")
  if [[ ${#extra_args[@]} -gt 0 ]]; then
    client_cmd+=("${extra_args[@]}")
  fi

  while (( attempt <= max_attempts )); do
    local log_file="$LOG_DIR/${peer}-${direction}-${case}-attempt${attempt}.log"
    last_log="$log_file"

    local result
    result=$(run_attempt "$CASE_TIMEOUT_SEC" "$log_file" "${client_cmd[@]}")
    local duration_ms exit_code
    read -r duration_ms exit_code <<< "$result"

    total_duration_ms=$(python3 -c "print(round($total_duration_ms + $duration_ms, 2))")
    last_exit_code=$exit_code

    if [[ $exit_code -eq 0 ]]; then
      last_status="passed"
      break
    else
      last_status="failed"
      if [[ $attempt -eq 1 ]]; then
        first_attempt_status="failed"
        first_duration_ms=$duration_ms
        local rec1_args=(
          python3 "$INTEROP_REPORT" record
          --output "$RESULTS_JSON"
          --case "$case"
          --status failed
          --duration-ms "$duration_ms"
          --peer "$peer"
          --direction "$direction"
          --transport "$TRANSPORT"
          --stdout-log "$log_file"
          --stderr-log "$log_file"
          --exit-code "$exit_code"
          --attempt-count 1
        )
        if [[ "$peer" == "$CPP_PEER_NAME" ]]; then
          rec1_args+=(--peer-pin "$CPP_PEER_PIN")
        fi
        "${rec1_args[@]}" >/dev/null
      fi
    fi

    attempt=$((attempt + 1))
  done

  local final_attempts=$attempt
  if (( final_attempts > max_attempts )); then
    final_attempts=$max_attempts
  fi

  local record_args=(
    python3 "$INTEROP_REPORT" record
    --output "$RESULTS_JSON"
    --case "$case"
    --status "$last_status"
    --duration-ms "$total_duration_ms"
    --peer "$peer"
    --direction "$direction"
    --transport "$TRANSPORT"
    --stdout-log "$last_log"
    --stderr-log "$last_log"
    --exit-code "$last_exit_code"
    --attempt-count "$final_attempts"
  )
  if [[ -n "$first_attempt_status" && $final_attempts -gt 1 ]]; then
    record_args+=(--first-attempt-status "$first_attempt_status")
  fi
  if [[ "$peer" == "$CPP_PEER_NAME" ]]; then
    record_args+=(--peer-pin "$CPP_PEER_PIN")
  fi

  "${record_args[@]}" >/dev/null

  if [[ "$last_status" == "passed" ]]; then
    return 0
  else
    return 1
  fi
}

run_kernel_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5"
  shift 5
  local cases=("$@")
  local pass_failed=0
  local client_extra=()
  if [[ $USE_TLS -eq 1 ]]; then
    client_extra=(
      "--use_tls=true"
      "--tls_ca_file=$TLS_CA_FILE"
      "--server_host_override=localhost"
    )
  fi
  for case in "${cases[@]}"; do
    if run_case "$KERNEL_CLIENT" "$host" "$port" "$case" "$peer" "$direction" "--" "${client_extra[@]}"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: native client against $label" >&2
    return 1
  fi
  echo "PASS: native client against $label (${#cases[@]} cases)"
}

run_cpp_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5"
  shift 5
  local cases=("$@")
  local pass_failed=0
  local client_extra=()
  if [[ $USE_TLS -eq 1 ]]; then
    client_extra=(
      "--use_tls=true"
      "--use_test_ca=true"
      "--server_host_override=localhost"
    )
  else
    client_extra=("--use_tls=false")
  fi
  for case in "${cases[@]}"; do
    if run_case "$CPP_CLIENT" "$host" "$port" "$case" "$peer" "$direction" "--" "${client_extra[@]}"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: C++ client against $label" >&2
    return 1
  fi
  echo "PASS: C++ client against $label (${#cases[@]} cases)"
}

# =========================================================================
# Step 1: Build native kernel binaries (if not skipped)
# =========================================================================
if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "== building kernel interop binaries =="
  cargo build --release -p pbrs-grpc \
    --bin pbrs-grpc-interop-server --bin pbrs-grpc-interop-client
fi

KERNEL_SERVER="${GRPC_INTEROP_KERNEL_SERVER:-$ROOT/target/release/pbrs-grpc-interop-server}"
KERNEL_CLIENT="${GRPC_INTEROP_KERNEL_CLIENT:-$ROOT/target/release/pbrs-grpc-interop-client}"

KERNEL_SERVER_ARGS=(--port "$PORT")
if [[ $USE_TLS -eq 1 ]]; then
  KERNEL_SERVER_ARGS+=(
    --use_tls=true
    --tls_cert_file "$TLS_CERT_FILE"
    --tls_key_file "$TLS_KEY_FILE"
  )
fi

# =========================================================================
# Self-interop pass (optional or self-only)
# =========================================================================
if [[ $SELF_ONLY -eq 1 || $INCLUDE_SELF -eq 1 ]]; then
  echo "== kernel client -> kernel server =="
  SERVER_PID=""
  if ! start_server "kernel server" "$PORT" "$LOG_DIR/server-kernel.log" "$KERNEL_SERVER" "${KERNEL_SERVER_ARGS[@]}"; then
    echo "FAIL: kernel server failed to start on port $PORT" >&2
    OVERALL_FAILED=1
    for case in "${CASES[@]}"; do
      record_server_failure "$case" "pbrs-grpc" "kernel_client_to_kernel_server" "kernel server failed to start on port $PORT" "$LOG_DIR/server-kernel.log"
    done
  else
    SERVER_PID="${TRACKED_PIDS[-1]}"
    run_kernel_client 127.0.0.1 "$PORT" "pbrs-grpc" "kernel_client_to_kernel_server" "kernel server" "${CASES[@]}" || OVERALL_FAILED=1
    stop_server "$SERVER_PID"
  fi

  if [[ $SELF_ONLY -eq 1 ]]; then
    echo "SKIP: C++ cross-language passes (--self-only requested)"
    echo "WARNING: self-only output is insufficient for official cross-language qualification"

    echo "== aggregating interop results =="
    AGGREGATE_EXIT=0
    python3 "$INTEROP_REPORT" aggregate \
      --results "$RESULTS_JSON" \
      --output "$REPORT_JSON" || AGGREGATE_EXIT=$?

    if [[ $OVERALL_FAILED -ne 0 || $AGGREGATE_EXIT -ne 0 ]]; then
      echo "FAIL: self-interop qualification failed" >&2
      exit 1
    fi
    echo "PASS: kernel client against kernel server passed successfully"
    exit 0
  fi
fi

# =========================================================================
# Step 2: Ensure C++ Reference Peer Binaries
# =========================================================================
CPP_BIN_DIR="${GRPC_INTEROP_CPP_BIN_DIR:-$ROOT/target/interop-cpp}"
CPP_SERVER="${GRPC_INTEROP_CPP_SERVER:-$CPP_BIN_DIR/interop_server}"
CPP_CLIENT="${GRPC_INTEROP_CPP_CLIENT:-$CPP_BIN_DIR/interop_client}"

# Check alternate locations if not set
if [[ ! -x "$CPP_CLIENT" && -x "$ROOT/target/interop-cpp-build/interop_client" ]]; then
  CPP_CLIENT="$ROOT/target/interop-cpp-build/interop_client"
fi
if [[ ! -x "$CPP_SERVER" && -x "$ROOT/target/interop-cpp-build/interop_server" ]]; then
  CPP_SERVER="$ROOT/target/interop-cpp-build/interop_server"
fi
if [[ ! -x "$CPP_CLIENT" && -x "$ROOT/third_party/grpc/cmake/build/interop_client" ]]; then
  CPP_CLIENT="$ROOT/third_party/grpc/cmake/build/interop_client"
fi
if [[ ! -x "$CPP_SERVER" && -x "$ROOT/third_party/grpc/cmake/build/interop_server" ]]; then
  CPP_SERVER="$ROOT/third_party/grpc/cmake/build/interop_server"
fi

ensure_cpp_peer() {
  if [[ -x "$CPP_CLIENT" && -x "$CPP_SERVER" ]]; then
    return 0
  fi

  if [[ "$SKIP_BUILD" == "1" ]]; then
    echo "FAIL: C++ peer binaries missing ($CPP_CLIENT, $CPP_SERVER) and build skipped (--skip-build)" >&2
    return 1
  fi

  # Attempt download from URL if provided
  if [[ -n "${GRPC_INTEROP_CPP_DOWNLOAD_URL:-}" ]]; then
    echo "== downloading C++ gRPC interop peer from $GRPC_INTEROP_CPP_DOWNLOAD_URL =="
    mkdir -p "$CPP_BIN_DIR"
    curl -fsSL "$GRPC_INTEROP_CPP_DOWNLOAD_URL" | tar -xz -C "$CPP_BIN_DIR"
    if [[ -x "$CPP_CLIENT" && -x "$CPP_SERVER" ]]; then
      return 0
    fi
  fi

  # Attempt build from source using cmake
  if command -v cmake >/dev/null 2>&1 && (command -v g++ >/dev/null 2>&1 || command -v clang++ >/dev/null 2>&1); then
    echo "== building C++ gRPC interop peer ($CPP_PEER_VERSION) =="
    mkdir -p "$CPP_BIN_DIR"
    local grpc_src="${GRPC_INTEROP_CPP_SRC_DIR:-$ROOT/third_party/grpc}"
    local build_dir="${GRPC_INTEROP_CPP_BUILD_DIR:-$ROOT/target/interop-cpp-build}"

    if [[ ! -d "$grpc_src/.git" && ! -f "$grpc_src/CMakeLists.txt" ]]; then
      echo "== fetching grpc/grpc @ $CPP_PEER_PIN =="
      mkdir -p "$grpc_src"
      git init "$grpc_src"
      git -C "$grpc_src" remote add origin https://github.com/grpc/grpc.git 2>/dev/null || true
      git -C "$grpc_src" fetch --depth 1 origin "$CPP_PEER_PIN"
      git -C "$grpc_src" checkout FETCH_HEAD
      git -C "$grpc_src" submodule update --init --recursive --depth 1
    fi

    echo "== configuring C++ interop targets with cmake =="
    cmake -S "$grpc_src" -B "$build_dir" \
      -DgRPC_BUILD_TESTS=ON \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_CXX_STANDARD=17

    local parallel="${NPROC:-$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)}"
    echo "== compiling interop_client and interop_server =="
    cmake --build "$build_dir" --parallel "$parallel" --target interop_client interop_server

    cp "$build_dir/interop_client" "$CPP_CLIENT"
    cp "$build_dir/interop_server" "$CPP_SERVER"
    chmod +x "$CPP_CLIENT" "$CPP_SERVER"
    return 0
  fi

  echo "FAIL: C++ gRPC reference peer binaries missing ($CPP_CLIENT, $CPP_SERVER)" >&2
  echo "Please set GRPC_INTEROP_CPP_CLIENT and GRPC_INTEROP_CPP_SERVER, or install cmake and g++ to build from source." >&2
  return 1
}

if ! ensure_cpp_peer; then
  exit 1
fi

CPP_SERVER_ARGS=(--port "$CPP_PORT")
if [[ $USE_TLS -eq 1 ]]; then
  CPP_SERVER_ARGS+=(--use_tls=true)
else
  CPP_SERVER_ARGS+=(--use_tls=false)
fi

# =========================================================================
# Pass 1: Native client (pbrs-grpc-interop-client) -> C++ server (interop_server)
# =========================================================================
echo "== native client -> C++ server =="
CPP_SERVER_PID=""
if ! start_server "C++ server" "$CPP_PORT" "$LOG_DIR/server-cpp.log" "$CPP_SERVER" "${CPP_SERVER_ARGS[@]}"; then
  echo "FAIL: C++ server failed to start on port $CPP_PORT" >&2
  OVERALL_FAILED=1
  for case in "${CASES[@]}"; do
    record_server_failure "$case" "$CPP_PEER_NAME" "kernel_client_to_cpp_server" "C++ server failed to start on port $CPP_PORT" "$LOG_DIR/server-cpp.log"
  done
else
  CPP_SERVER_PID="${TRACKED_PIDS[-1]}"
  run_kernel_client 127.0.0.1 "$CPP_PORT" "$CPP_PEER_NAME" "kernel_client_to_cpp_server" "C++ server" "${CASES[@]}" || OVERALL_FAILED=1
  stop_server "$CPP_SERVER_PID"
fi

# =========================================================================
# Pass 2: C++ client (interop_client) -> Native server (pbrs-grpc-interop-server)
# =========================================================================
echo "== C++ client -> native server =="
KERNEL_SERVER_PID=""
if ! start_server "kernel server" "$PORT" "$LOG_DIR/server-kernel.log" "$KERNEL_SERVER" "${KERNEL_SERVER_ARGS[@]}"; then
  echo "FAIL: kernel server failed to start on port $PORT" >&2
  OVERALL_FAILED=1
  for case in "${CASES[@]}"; do
    record_server_failure "$case" "$CPP_PEER_NAME" "cpp_client_to_kernel_server" "kernel server failed to start on port $PORT" "$LOG_DIR/server-kernel.log"
  done
else
  KERNEL_SERVER_PID="${TRACKED_PIDS[-1]}"
  run_cpp_client 127.0.0.1 "$PORT" "$CPP_PEER_NAME" "cpp_client_to_kernel_server" "kernel server" "${CASES[@]}" || OVERALL_FAILED=1
  stop_server "$KERNEL_SERVER_PID"
fi

# =========================================================================
# Step 3: Aggregate Results
# =========================================================================
echo "== aggregating C++ interop results =="
AGGREGATE_EXIT=0
python3 "$INTEROP_REPORT" aggregate \
  --results "$RESULTS_JSON" \
  --output "$REPORT_JSON" || AGGREGATE_EXIT=$?

if [[ $OVERALL_FAILED -ne 0 || $AGGREGATE_EXIT -ne 0 ]]; then
  echo "FAIL: C++ interop qualification failed" >&2
  exit 1
fi

echo "PASS: all C++ interop passes completed successfully"
exit 0
