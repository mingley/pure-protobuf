# Official gRPC C++ Reference Peer Interoperability

This directory documents the pinned C++ gRPC peer used to prove `pbrs-grpc` interoperability, especially wire-level compression behavior. Run all C++ peer cases with `./scripts/grpc-interop-cpp.sh`. This does not replace the Go peer matrix, Protobuf conformance, HTTP/2 negative tests, server probes, or any unresolved full-profile cases in `tests/interop/cases.json`.

The C++ reference peer is the definitive cross-language peer for compression. `grpc-go` covers the baseline unary and streaming cases, but it does not implement the official compression interop assertions.

## 1. Upstream Pin and Specification

All test cases, procedures, and peer behaviors are pinned to the official gRPC repository.

| Specification / artifact | Source / path |
|---|---|
| **Upstream repository** | [`grpc/grpc`](https://github.com/grpc/grpc) |
| **Pinned commit** | [`d1487957db6658bc532b72871775148229836627`](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627) (v1.84.0) |
| **Test descriptions** | [`doc/interop-test-descriptions.md`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/interop-test-descriptions.md) |
| **Client implementation** | [`test/cpp/interop/interop_client.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/interop_client.cc), [`test/cpp/interop/client.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/client.cc) |
| **Server implementation** | [`test/cpp/interop/interop_server.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/interop_server.cc), [`test/cpp/interop/server_helper.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/server_helper.cc) |
| **Proto definitions** | `src/proto/grpc/testing/test.proto`, `messages.proto`, `empty.proto` |

## 2. Why the C++ Reference Peer Is Essential

Use the C++ peer when a test must prove official message compression semantics.

`grpc-go` (`v1.85.0-dev @ dd51b1c90aaf`) is still useful for baseline interop, but it does not cover the compression cases:

* `interop/test_utils.go` ignores `SimpleRequest.expect_compressed` and `SimpleRequest.response_compressed`.
* The `grpc-go` interop client rejects all four compression case names: `client_compressed_unary`, `server_compressed_unary`, `client_compressed_streaming`, and `server_compressed_streaming`.

Therefore, compression qualification needs the C++ reference peer, which implements and asserts the compression specification.

## 3. Toolchain & Dependencies

| Dependency | Requirement |
|---|---|
| CMake | 3.16 or newer. |
| C++ compiler | C++17 compiler: `g++` 9+ or `clang++` 10+. |
| Git | Fetches the pinned repository commit and required submodules. |
| Python 3 | Python 3.8+ standard library only. No pip dependencies. Used for process orchestration and report aggregation. |
| Rust toolchain | Stable Rust for `pbrs-grpc-interop-client` and `pbrs-grpc-interop-server`. |

### Platform Support

| Platform | Supported build path |
|---|---|
| Linux (`x86_64` / `aarch64`) | Native CMake build, or locally cached binaries built from the pinned source. |
| macOS (`arm64` / `x86_64`) | Native CMake build with Apple Clang or Homebrew GCC. |

## 4. Build, Fetch, and Caching Procedures

`scripts/grpc-interop-cpp.sh` discovers, builds, and caches the C++ reference peer binaries: `interop_client` and `interop_server`.

### Automatic Acquisition Flow

The runner tries these paths in order:

1. **Existing binaries**: It looks for `interop_client` and `interop_server` in `$GRPC_INTEROP_CPP_BIN_DIR` (default `target/interop-cpp/`), `target/interop-cpp-build/`, `third_party/grpc/cmake/build/`, or explicit environment-variable paths.
2. **Build from source**: If `cmake` and a C++ compiler are available, it:
   * fetches pinned commit `d1487957db6658bc532b72871775148229836627` with `--depth 1` into `third_party/grpc`;
   * initializes required submodules with `--depth 1`: `abseil-cpp`, `protobuf`, `re2`, `zlib`, and `cares`;
   * configures CMake with `-DgRPC_BUILD_TESTS=ON -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=17`;
   * builds `interop_client` and `interop_server` in parallel;
   * caches output binaries in `target/interop-cpp/`.

### Manual Build Procedure

To build the pinned binaries manually:

```bash
# 1. Clone at the exact pinned commit
mkdir -p third_party/grpc
git init third_party/grpc
git -C third_party/grpc remote add origin https://github.com/grpc/grpc.git
git -C third_party/grpc fetch --depth 1 origin d1487957db6658bc532b72871775148229836627
git -C third_party/grpc checkout FETCH_HEAD
git -C third_party/grpc submodule update --init --recursive --depth 1

# 2. Configure and compile with CMake
cmake -S third_party/grpc -B target/interop-cpp-build \
  -DgRPC_BUILD_TESTS=ON \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CXX_STANDARD=17

cmake --build target/interop-cpp-build --parallel "$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)" \
  --target interop_client interop_server

# 3. Copy binaries to standard cache location
mkdir -p target/interop-cpp
cp target/interop-cpp-build/interop_client target/interop-cpp/
cp target/interop-cpp-build/interop_server target/interop-cpp/
```

### Environment Variable Overrides

| Variable | Meaning |
|---|---|
| `GRPC_INTEROP_CPP_CLIENT` | Explicit path to `interop_client`. |
| `GRPC_INTEROP_CPP_SERVER` | Explicit path to `interop_server`. |
| `GRPC_INTEROP_CPP_BIN_DIR` | Directory containing C++ peer binaries. Default: `target/interop-cpp`. |
| `GRPC_INTEROP_CPP_DOWNLOAD_URL` | Rejected. Unverified binary archives cannot establish the pinned peer's provenance; build from source or use a reviewed local binary with a recorded digest. |
| `GRPC_INTEROP_SKIP_BUILD` | When set to `1`, skips compilation and requires pre-existing binaries. |
| `GRPC_INTEROP_LOG_DIR` | Directory for per-attempt stdout/stderr logs and final `report.json`. |

## 5. Case Matrix & Execution Directions

The C++ harness runs two cross-language peer directions.

| Direction | Meaning |
|---|---|
| `kernel_client_to_cpp_server` | Native client (`pbrs-grpc-interop-client`) calls C++ server (`interop_server`). |
| `cpp_client_to_kernel_server` | C++ client (`interop_client`) calls native server (`pbrs-grpc-interop-server`). |

### The 18 Test Cases

Every run evaluates all 14 baseline specification cases plus all 4 compression cases.

| Suite | Case identifier | Peer directions | What it checks |
|---|---|---|---|
| `standard_interop` | `empty_unary` | Both | Zero-byte request payload and zero-byte response payload. |
| `standard_interop` | `large_unary` | Both | 271,828-byte request and 314,159-byte response; asserts all-zero payload byte patterns. |
| `standard_interop` | `client_streaming` | Both | Client streams 4 request messages totaling 74,922 bytes; server returns the aggregate response size. |
| `standard_interop` | `server_streaming` | Both | Client requests response sizes `[31415, 9, 2653, 58979]`; server streams exact payloads. |
| `standard_interop` | `ping_pong` | Both | Full-duplex bidirectional ping-pong exchange across multiple turns. |
| `standard_interop` | `empty_stream` | Both | Client half-closes a bidirectional stream immediately; verifies no messages before clean close. |
| `standard_interop` | `cancel_after_begin` | Both | Cancels a stream immediately after creation; verifies `CANCELLED` status. |
| `standard_interop` | `cancel_after_first_response` | Both | Cancels a bidirectional stream after the first response; enforces terminal `CANCELLED` code assertion. |
| `standard_interop` | `timeout_on_sleeping_server` | Both | 1ms deadline against a server sleeping 10s; verifies `DEADLINE_EXCEEDED`. |
| `standard_interop` | `custom_metadata` | Both | Echoes initial text header and binary trailer (`-bin`); asserts trailers do not leak into headers. |
| `standard_interop` | `status_code_and_message` | Both | Verifies requested numeric status code and message string. |
| `standard_interop` | `special_status_message` | Both | Verifies Unicode and percent-encoded status messages, including `\t\ntest\r-` and emoji. |
| `standard_interop` | `unimplemented_method` | Both | Calls `/grpc.testing.TestService/UnimplementedCall`; asserts `UNIMPLEMENTED`. |
| `standard_interop` | `unimplemented_service` | Both | Calls a method on missing `/grpc.testing.UnimplementedService`; asserts `UNIMPLEMENTED`. |
| `compression_interop` | `client_compressed_unary` | Both | Unary call with `expect_compressed.value = true`; asserts the request was compressed on the wire. |
| `compression_interop` | `server_compressed_unary` | Both | Unary call with `response_compressed.value = true`; asserts the response was compressed on the wire. |
| `compression_interop` | `client_compressed_streaming` | Both | Streaming call alternates compressed and uncompressed messages; asserts each message flag. |
| `compression_interop` | `server_compressed_streaming` | Both | Streaming call requests compressed responses; asserts all response frames are compressed. |

## 6. Compression Verification Details

In the gRPC wire protocol, message-level compression is signaled in the 5-byte data frame prefix.

```text
+----------------+--------------------------------+
| Compressed (1) |        Message Length (4)      |
|     (0x01)     |         (big-endian u32)       |
+----------------+--------------------------------+
|                Message Payload                  |
|          (compressed bytes, e.g. gzip)          |
+-------------------------------------------------+
```

### Verification Requirements:

| Case | Required behavior |
|---|---|
| `client_compressed_unary` | Client sends `SimpleRequest` with `expect_compressed: {value: true}`. The client sets compressed flag `0x01` and gzip-encodes the payload. The server verifies the flag; if the request is uncompressed, it returns `INVALID_ARGUMENT`. The server response is uncompressed. |
| `server_compressed_unary` | Client sends `SimpleRequest` with `response_compressed: {value: true}`. The server sets compressed flag `0x01` and gzip-compresses the `SimpleResponse` payload. The client asserts that the received frame has the compressed flag and decompresses the payload. |
| `client_compressed_streaming` | Client sends message 1 with `expect_compressed: {value: true}` on the wire compressed. It sends message 2 with `expect_compressed: {value: false}` on the wire uncompressed. The server asserts each individual message's expected compression state. |
| `server_compressed_streaming` | Client requests streaming output with `response_compressed: {value: true}`. The server compresses every streamed response frame. The client asserts every received frame has the compressed flag. |

## 7. Invocation & CLI Usage

```bash
# Run all 18 cases in both directions:
./scripts/grpc-interop-cpp.sh

# Run with existing pre-built binaries (skip build):
./scripts/grpc-interop-cpp.sh --skip-build

# Run specific cases (e.g. compression suite only):
./scripts/grpc-interop-cpp.sh --cases=client_compressed_unary,server_compressed_unary,client_compressed_streaming,server_compressed_streaming

# Run over TLS transport:
./scripts/grpc-interop-cpp.sh --use-tls

# Run self-interop pass only:
./scripts/grpc-interop-cpp.sh --self-only

# Display full help and options:
./scripts/grpc-interop-cpp.sh --help
```

## 8. Process Lifecycle & Diagnostics

| Area | Behavior |
|---|---|
| Startup readiness | Servers are polled by socket connection with a strict `--startup-timeout`. |
| Execution timeout | Client cases run with deadline bounds: `--timeout`, default 15s. Timed-out processes are cleaned up by process group. |
| Process termination | A cleanup trap catches `EXIT`, `INT`, and `TERM`, then terminates all spawned child PIDs with `SIGTERM` followed by `SIGKILL` escalation. |
| Retained evidence | Raw stdout/stderr logs for every attempt are stored in `target/interop-logs/<timestamp>_<pid>/`. |
| Machine-readable reports | Results are recorded in `results.json` and aggregated into `report.json` by `scripts/interop-report.py`. |
| Required-cell gating | Cross-peer runs validate `results.json` per suite: `--suite standard_interop` and `--suite compression_interop`, each scoped to the two C++ directions with `--require-matrix --required-directions`. Missing cases or directions, non-pass records, hidden first-attempt flakes, or wrong peer pins fail qualification. |
| Other peer requirements | Go's separately required directions are checked by `grpc-interop.sh`; one runner cannot substitute for another. |
| Missing binaries | A missing C++ binary fails closed with exit 1 unless `--self-only` is explicitly passed. Self-only output is labeled insufficient for cross-language qualification. |
| Artifact digests | Every cross-peer result record carries pinned source commit `--peer-pin d1487957...` plus a notes field with the exact `interop_client` and `interop_server` SHA-256 digests printed at startup. Each cell is attributable to a specific binary build. |
