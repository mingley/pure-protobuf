#!/usr/bin/env bash
# Run HTTP/2 negative and conformance test cases against the pbrs-grpc HTTP/2 client.
#
# Covers the official HTTP/2 negative and recovery test cases:
#   goaway                    - Graceful GOAWAY handling and transparent connection migration
#   rst_after_header          - RST_STREAM frame immediately after HEADERS
#   rst_during_data           - RST_STREAM frame in the middle of DATA streaming
#   rst_after_data            - RST_STREAM frame after all DATA without receiving trailers
#   ping                      - PING frame roundtrip and ACK observation
#   max_streams               - SETTINGS_MAX_CONCURRENT_STREAMS enforcement under concurrency
#   data_frame_padding        - Padded DATA frames flow control and decoding without deadlock
#   no_df_padding_sanity_test - Unpadded small DATA frames baseline sanity test
#
# Can run against either:
#   1. An external HTTP/2 test server (via --server-host and --server-port)
#   2. An automatic in-process local simulated HTTP/2 test server (default)
#
# No upstream grpc/grpc files are patched or forked by this harness: the
# local simulated server below is a purpose-built test double speaking the
# documented wire procedures (GOAWAY/RST/PING/SETTINGS/padding), and the
# client adapter asserts the official expected outcomes (both `goaway` calls
# succeed on a proven-new connection; `rst_*` calls fail rather than
# returning fabricated OK). Case names passed to both ends always match.
#
# Usage:
#   ./scripts/grpc-http2-interop.sh [OPTIONS]
#
# Options:
#   --cases=CASE1,CASE2,...    Run only specified test cases
#   --server-host=HOST         External HTTP/2 server host (disables local server)
#   --server-port=PORT         External HTTP/2 server port
#   --port=PORT                Port to bind local simulated server to (default: dynamic)
#   --skip-build               Skip cargo build step
#   --log-dir=DIR              Directory for log files (default: temp directory)
#   --results-json=PATH        Path to write results JSON (default: <log-dir>/results.json)
#   --report-json=PATH         Path to write report JSON (default: <log-dir>/report.json)
#   -h, --help                 Show this help message
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

INTEROP_REPORT="$ROOT/scripts/interop-report.py"

DEFAULT_CASES=(
  goaway
  rst_after_header
  rst_during_data
  rst_after_data
  ping
  max_streams
  data_frame_padding
  no_df_padding_sanity_test
)

CASES=("${DEFAULT_CASES[@]}")
SERVER_HOST=""
SERVER_PORT=""
LOCAL_BIND_PORT=0
SKIP_BUILD=0
LOG_DIR=""
RESULTS_JSON=""
REPORT_JSON=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cases)
      IFS=',' read -r -a CASES <<< "$2"
      shift 2
      ;;
    --cases=*)
      IFS=',' read -r -a CASES <<< "${1#*=}"
      shift
      ;;
    --server-host)
      SERVER_HOST="$2"
      shift 2
      ;;
    --server-host=*)
      SERVER_HOST="${1#*=}"
      shift
      ;;
    --server-port|--port)
      if [[ -n "$SERVER_HOST" ]]; then
        SERVER_PORT="$2"
      else
        LOCAL_BIND_PORT="$2"
      fi
      shift 2
      ;;
    --server-port=*|--port=*)
      val="${1#*=}"
      if [[ -n "$SERVER_HOST" ]]; then
        SERVER_PORT="$val"
      else
        LOCAL_BIND_PORT="$val"
      fi
      shift
      ;;
    --skip-build)
      SKIP_BUILD=1
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
    --results-json)
      RESULTS_JSON="$2"
      shift 2
      ;;
    --results-json=*)
      RESULTS_JSON="${1#*=}"
      shift
      ;;
    --report-json)
      REPORT_JSON="$2"
      shift 2
      ;;
    --report-json=*)
      REPORT_JSON="${1#*=}"
      shift
      ;;
    -h|--help)
      grep '^# ' "$0" | cut -c 3-
      exit 0
      ;;
    *)
      echo "Unknown flag: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$LOG_DIR" ]]; then
  LOG_DIR=$(mktemp -d "/tmp/grpc-http2-interop-logs.XXXXXX")
else
  mkdir -p "$LOG_DIR"
fi

RESULTS_JSON="${RESULTS_JSON:-$LOG_DIR/results.json}"
REPORT_JSON="${REPORT_JSON:-$LOG_DIR/report.json}"

if [[ "$SKIP_BUILD" -ne 1 ]]; then
  echo "== building pbrs-grpc-http2-client =="
  cargo build --release -p pbrs-grpc --bin pbrs-grpc-http2-client
fi

CLIENT_BIN="${GRPC_HTTP2_CLIENT:-$ROOT/target/release/pbrs-grpc-http2-client}"
if [[ ! -x "$CLIENT_BIN" && -x "$ROOT/target/debug/pbrs-grpc-http2-client" ]]; then
  CLIENT_BIN="$ROOT/target/debug/pbrs-grpc-http2-client"
fi

if [[ ! -x "$CLIENT_BIN" ]]; then
  echo "Error: pbrs-grpc-http2-client binary not found at $CLIENT_BIN" >&2
  exit 1
fi

SIMULATED_SERVER_SCRIPT='
import socket, sys, time, struct, threading, select

port = int(sys.argv[1]) if len(sys.argv) > 1 else 0
case = sys.argv[2] if len(sys.argv) > 2 else "goaway"
ready_fifo = sys.argv[3] if len(sys.argv) > 3 else None

def encode_varint(n: int) -> bytes:
    res = bytearray()
    while n >= 0x80:
        res.append((n & 0x7f) | 0x80)
        n >>= 7
    res.append(n & 0x7f)
    return bytes(res)

def hpack_literal_new(name: str, value: str) -> bytes:
    name_b = name.encode("ascii")
    val_b = value.encode("ascii")
    return bytes([0, len(name_b)]) + name_b + bytes([len(val_b)]) + val_b

def h2_frame(type_: int, flags: int, stream_id: int, payload: bytes) -> bytes:
    length = len(payload)
    head = bytes([
        (length >> 16) & 0xff,
        (length >> 8) & 0xff,
        length & 0xff,
        type_,
        flags,
        (stream_id >> 24) & 0x7f,
        (stream_id >> 16) & 0xff,
        (stream_id >> 8) & 0xff,
        stream_id & 0xff,
    ])
    return head + payload

body_bytes = b"\x00" * 314159
payload_msg = b"\x12" + encode_varint(len(body_bytes)) + body_bytes
simple_resp_msg = b"\x0a" + encode_varint(len(payload_msg)) + payload_msg
grpc_msg = b"\x00" + struct.pack(">I", len(simple_resp_msg)) + simple_resp_msg
headers_payload = bytes([0x88]) + hpack_literal_new("content-type", "application/grpc")
trailers_payload = hpack_literal_new("grpc-status", "0")

def recv_exact(sock, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            return None
        buf.extend(chunk)
    return bytes(buf)

def read_frame(sock):
    header = recv_exact(sock, 9)
    if not header:
        return None
    length = (header[0] << 16) | (header[1] << 8) | header[2]
    type_ = header[3]
    flags = header[4]
    stream_id = int.from_bytes(header[5:9], "big") & 0x7FFFFFFF
    payload = recv_exact(sock, length) if length > 0 else b""
    return type_, flags, stream_id, payload

conn_count = 0

def handle_conn(conn):
    global conn_count
    conn_count += 1
    my_idx = conn_count
    print(f"NEW_CONNECTION idx={my_idx}", file=sys.stderr, flush=True)
    try:
        preface = recv_exact(conn, 24)
        if not preface:
            return

        if case == "max_streams":
            conn.sendall(h2_frame(4, 0, 0, struct.pack(">HI", 3, 1)))
        else:
            conn.sendall(h2_frame(4, 0, 0, b""))

        conn_window = 65535
        initial_stream_window = 65535
        stream_windows = {}
        outstanding_pings = 0
        active_streams = set()
        max_active = 0
        calls_handled = 0

        def send_ack_or_flow(ftype, fflags, fstream, fpayload):
            nonlocal conn_window, initial_stream_window, outstanding_pings
            if ftype == 4:
                if not (fflags & 1):
                    for i in range(0, len(fpayload), 6):
                        if i + 6 <= len(fpayload):
                            sid, val = struct.unpack(">HI", fpayload[i:i+6])
                            if sid == 4:
                                initial_stream_window = val
                    conn.sendall(h2_frame(4, 1, 0, b""))
            elif ftype == 6:
                if fflags & 1:
                    outstanding_pings -= 1
                else:
                    conn.sendall(h2_frame(6, 1, fstream, fpayload))
            elif ftype == 8:
                inc = struct.unpack(">I", fpayload[:4])[0] & 0x7FFFFFFF
                if fstream == 0:
                    conn_window += inc
                else:
                    stream_windows[fstream] = stream_windows.get(fstream, initial_stream_window) + inc
            elif ftype == 0 and len(fpayload) > 0:
                conn.sendall(h2_frame(8, 0, 0, struct.pack(">I", len(fpayload))))
                conn.sendall(h2_frame(8, 0, fstream, struct.pack(">I", len(fpayload))))

        def drain_incoming():
            while True:
                r, _, _ = select.select([conn], [], [], 0)
                if not r:
                    break
                f = read_frame(conn)
                if not f:
                    break
                send_ack_or_flow(f[0], f[1], f[2], f[3])

        while True:
            f = read_frame(conn)
            if not f:
                break
            ftype, fflags, fstream, fpayload = f
            send_ack_or_flow(ftype, fflags, fstream, fpayload)

            if ftype == 1:
                stream_id = fstream
                stream_windows[stream_id] = stream_windows.get(stream_id, initial_stream_window)

                if case == "goaway":
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    chunk_size = 16384
                    for offset in range(0, len(grpc_msg), chunk_size):
                        drain_incoming()
                        chunk = grpc_msg[offset:offset+chunk_size]
                        conn.sendall(h2_frame(0, 0, stream_id, chunk))
                    conn.sendall(h2_frame(1, 5, stream_id, trailers_payload))
                    if my_idx == 1:
                        goaway_payload = struct.pack(">II", stream_id, 0)
                        conn.sendall(h2_frame(7, 0, 0, goaway_payload))
                        time.sleep(0.1)
                        conn.close()
                        return
                elif case == "rst_after_header":
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    rst_payload = struct.pack(">I", 8)
                    conn.sendall(h2_frame(3, 0, stream_id, rst_payload))
                    conn.close()
                    return
                elif case == "rst_during_data":
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    conn.sendall(h2_frame(0, 0, stream_id, grpc_msg[:1000]))
                    rst_payload = struct.pack(">I", 8)
                    conn.sendall(h2_frame(3, 0, stream_id, rst_payload))
                    conn.close()
                    return
                elif case == "rst_after_data":
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    chunk_size = 16384
                    for offset in range(0, len(grpc_msg), chunk_size):
                        drain_incoming()
                        chunk = grpc_msg[offset:offset+chunk_size]
                        conn.sendall(h2_frame(0, 0, stream_id, chunk))
                    rst_payload = struct.pack(">I", 8)
                    conn.sendall(h2_frame(3, 0, stream_id, rst_payload))
                    conn.close()
                    return
                elif case == "ping":
                    conn.sendall(h2_frame(6, 0, 0, b"\x01" * 8))
                    outstanding_pings += 1
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    conn.sendall(h2_frame(6, 0, 0, b"\x02" * 8))
                    outstanding_pings += 1
                    conn.sendall(h2_frame(6, 0, 0, b"\x03" * 8))
                    outstanding_pings += 1
                    chunk_size = 16384
                    for offset in range(0, len(grpc_msg), chunk_size):
                        drain_incoming()
                        chunk = grpc_msg[offset:offset+chunk_size]
                        conn.sendall(h2_frame(0, 0, stream_id, chunk))
                    conn.sendall(h2_frame(1, 5, stream_id, trailers_payload))
                    conn.sendall(h2_frame(6, 0, 0, b"\x04" * 8))
                    outstanding_pings += 1
                elif case == "max_streams":
                    active_streams.add(stream_id)
                    if len(active_streams) > max_active:
                        max_active = len(active_streams)
                    if len(active_streams) > 1:
                        print(f"SERVER_ASSERTION_FAILED: max concurrent streams violated: active={len(active_streams)}", file=sys.stderr)
                    calls_handled += 1
                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))
                    chunk_size = 16384
                    for offset in range(0, len(grpc_msg), chunk_size):
                        drain_incoming()
                        chunk = grpc_msg[offset:offset+chunk_size]
                        conn.sendall(h2_frame(0, 0, stream_id, chunk))
                    conn.sendall(h2_frame(1, 5, stream_id, trailers_payload))
                    active_streams.discard(stream_id)
                elif case in ("data_frame_padding", "no_df_padding_sanity_test"):
                    is_padded = (case == "data_frame_padding")
                    pad_len = 255 if is_padded else 0
                    chunk_size = 5

                    conn.sendall(h2_frame(1, 4, stream_id, headers_payload))

                    offset = 0
                    while offset < len(grpc_msg):
                        drain_incoming()
                        avail = min(conn_window, stream_windows[stream_id])
                        min_flen = (1 + chunk_size + pad_len) if is_padded else chunk_size
                        if avail < min_flen:
                            f = read_frame(conn)
                            if not f:
                                raise Exception(f"EOF waiting for window update at offset {offset}")
                            send_ack_or_flow(f[0], f[1], f[2], f[3])
                            continue

                        batch = bytearray()
                        while offset < len(grpc_msg):
                            chunk = grpc_msg[offset : offset + chunk_size]
                            flen = (1 + len(chunk) + pad_len) if is_padded else len(chunk)
                            if conn_window < flen or stream_windows[stream_id] < flen:
                                break
                            if is_padded:
                                batch.extend(h2_frame(0, 8, stream_id, bytes([pad_len]) + chunk + (b"\x00" * pad_len)))
                            else:
                                batch.extend(h2_frame(0, 0, stream_id, chunk))
                            conn_window -= flen
                            stream_windows[stream_id] -= flen
                            offset += len(chunk)
                        if batch:
                            conn.sendall(batch)

                    conn.sendall(h2_frame(1, 5, stream_id, trailers_payload))

        if case == "ping":
            if outstanding_pings != 0:
                print(f"SERVER_ASSERTION_FAILED: outstanding pings on disconnect: {outstanding_pings} != 0", file=sys.stderr)
        elif case == "max_streams":
            if max_active > 1 or calls_handled < 11:
                print(f"SERVER_ASSERTION_FAILED: max concurrent streams assertion failed: max_active={max_active}, calls_handled={calls_handled}", file=sys.stderr)
    except Exception as e:
        pass
    finally:
        try:
            conn.close()
        except Exception:
            pass

s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", port))
actual_port = s.getsockname()[1]
s.listen(10)

if ready_fifo:
    with open(ready_fifo, "w") as f:
        f.write(f"READY:{actual_port}\n")
else:
    print(f"READY:{actual_port}", flush=True)

while True:
    try:
        conn, _ = s.accept()
        t = threading.Thread(target=handle_conn, args=(conn,), daemon=True)
        t.start()
    except Exception:
        break
'

echo "== running HTTP/2 interop cases (${#CASES[@]} cases) =="
OVERALL_FAILED=0
PASSED_COUNT=0
FAILED_COUNT=0

for case in "${CASES[@]}"; do
  log_file="$LOG_DIR/${case}.log"
  host="$SERVER_HOST"
  port="$SERVER_PORT"
  server_pid=""

  if [[ -z "$host" || -z "$port" ]]; then
    # Start local simulated server for this case
    host="127.0.0.1"
    server_fifo=$(mktemp -u "/tmp/http2-server-fifo.XXXXXX")
    mkfifo "$server_fifo"
    server_log="$LOG_DIR/${case}_server.log"

    python3 -c "$SIMULATED_SERVER_SCRIPT" "$LOCAL_BIND_PORT" "$case" "$server_fifo" > "$server_log" 2>&1 &
    server_pid=$!

    ready_line=""
    if read -r -t 5 ready_line < "$server_fifo"; then
      port="${ready_line#READY:}"
    else
      echo "  FAIL $case (server failed to initialize)"
      kill -9 "$server_pid" 2>/dev/null || true
      rm -f "$server_fifo"
      OVERALL_FAILED=1
      FAILED_COUNT=$((FAILED_COUNT + 1))
      continue
    fi
    rm -f "$server_fifo"
  fi

  # Execute client
  start_time=$(python3 -c 'import time; print(time.perf_counter())')
  client_exit=0
  if "$CLIENT_BIN" --server_host="$host" --server_port="$port" --test_case="$case" > "$log_file" 2>&1; then
    client_exit=0
  else
    client_exit=$?
  fi
  end_time=$(python3 -c 'import time; print(time.perf_counter())')
  dur_ms=$(python3 -c "print(round(($end_time - $start_time) * 1000, 2))")

  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi

  server_assert_failed=0
  if [[ -f "$LOG_DIR/${case}_server.log" ]] && grep -q "SERVER_ASSERTION_FAILED" "$LOG_DIR/${case}_server.log"; then
    server_assert_failed=1
  fi

  # Local-mode cross-check for `goaway`: the peer must have accepted at
  # least two connections, proving the client migrated after GOAWAY instead
  # of reusing the drained connection. External-peer runs have no server
  # log; there the client-side reconnect assertion is the proof.
  goaway_conn_failed=0
  if [[ "$case" == "goaway" && -f "$LOG_DIR/${case}_server.log" ]]; then
    conn_seen=$(grep -c "NEW_CONNECTION" "$LOG_DIR/${case}_server.log" || true)
    if [[ "$conn_seen" -lt 2 ]]; then
      goaway_conn_failed=1
    fi
  fi

  if [[ $client_exit -eq 0 && $server_assert_failed -eq 0 && $goaway_conn_failed -eq 0 ]]; then
    echo "  ok   $case (${dur_ms}ms)"
    PASSED_COUNT=$((PASSED_COUNT + 1))
    status="passed"
  else
    echo "  FAIL $case (exit code $client_exit, ${dur_ms}ms)"
    if [[ $server_assert_failed -ne 0 ]]; then
      grep "SERVER_ASSERTION_FAILED" "$LOG_DIR/${case}_server.log" | sed 's/^/       /'
    fi
    if [[ $goaway_conn_failed -ne 0 ]]; then
      echo "goaway: server accepted $conn_seen connection(s), need >= 2 to prove migration" | sed 's/^/       /'
    fi
    sed 's/^/       /' "$log_file"
    OVERALL_FAILED=1
    FAILED_COUNT=$((FAILED_COUNT + 1))
    status="failed"
  fi

  if [[ -f "$INTEROP_REPORT" ]]; then
    python3 "$INTEROP_REPORT" record \
      --output "$RESULTS_JSON" \
      --case "$case" \
      --status "$status" \
      --duration-ms "$dur_ms" \
      --peer "pbrs-grpc" \
      --direction "client_to_server" \
      --transport "http2_cleartext" \
      --suite "http2_negative" \
      --profile "native" \
      --stdout-log "$log_file" \
      --stderr-log "$log_file" \
      --exit-code "$client_exit" \
      --attempt-count 1 >/dev/null || true
  fi
done

echo "=================================================="
echo "HTTP/2 interop suite summary: $PASSED_COUNT passed, $FAILED_COUNT failed"
echo "Logs saved to $LOG_DIR"
echo "=================================================="

if [[ -f "$RESULTS_JSON" && -f "$INTEROP_REPORT" ]]; then
  echo "== validating interop results =="
  python3 "$INTEROP_REPORT" validate --results "$RESULTS_JSON" --suite http2_negative --profile native --require-matrix || OVERALL_FAILED=1

  echo "== aggregating interop results =="
  python3 "$INTEROP_REPORT" aggregate --results "$RESULTS_JSON" --output "$REPORT_JSON" --suite http2_negative --profile native --require-matrix || OVERALL_FAILED=1
fi

if [[ $OVERALL_FAILED -ne 0 ]]; then
  exit 1
fi
exit 0
