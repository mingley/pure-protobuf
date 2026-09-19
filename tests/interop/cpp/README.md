# Official gRPC C++ Reference Peer Interoperability

This directory documents the integration, toolchain dependencies, build and caching procedures, test matrix, and wire-level compression verification for the official C++ gRPC reference peer (`grpc/grpc`).

The C++ reference peer serves as the definitive reference implementation for cross-language validation of `pure-protobuf` and `pbrs-grpc`, specifically qualifying protocol areas where other implementations are deficient—most notably, message-level compression negotiation and framing.

---

## 1. Upstream Pin and Specification

All test cases, procedures, and peer behaviors are pinned to the official gRPC repository:

| Specification / Artifact | Source / Path |
|---|---|
| **Upstream Repository** | [`grpc/grpc`](https://github.com/grpc/grpc) |
| **Pinned Commit** | [`d1487957db6658bc532b72871775148229836627`](https://github.com/grpc/grpc/tree/d1487957db6658bc532b72871775148229836627) (v1.84.0) |
| **Test Descriptions** | [`doc/interop-test-descriptions.md`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/interop-test-descriptions.md) |
| **Client Implementation** | [`test/cpp/interop/interop_client.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/interop_client.cc), [`test/cpp/interop/client.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/client.cc) |
| **Server Implementation** | [`test/cpp/interop/interop_server.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/interop_server.cc), [`test/cpp/interop/server_helper.cc`](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/test/cpp/interop/server_helper.cc) |
| **Proto Definitions** | `src/proto/grpc/testing/test.proto`, `messages.proto`, `empty.proto` |

---

## 2. Why the C++ Reference Peer Is Essential

While `grpc-go` provides excellent coverage for baseline unary and streaming RPCs, it completely lacks support for the official compression test cases:
* In `grpc-go` (`v1.85.0-dev @ dd51b1c90aaf`), `interop/test_utils.go` ignores both `SimpleRequest.expect_compressed` and `SimpleRequest.response_compressed` fields.
* Its interop client explicitly rejects the four compression case names (`client_compressed_unary`, `server_compressed_unary`, `client_compressed_streaming`, `server_compressed_streaming`).

Consequently, cross-language qualification of gRPC compression requires the C++ reference peer, which fully implements and asserts the compression specification.

---

## 3. Toolchain & Dependencies

Building and running the C++ reference peer requires:

* **CMake**: 3.16 or newer.
* **C++ Compiler**: C++17 compliant compiler (`g++` 9+ or `clang++` 10+).
* **Git**: Used to fetch the pinned repository commit and required submodules.
* **Python 3**: Python 3.8+ (standard library only, zero pip dependencies) for process orchestration and report aggregation.
* **Rust Toolchain**: Stable Rust (for compiling `pbrs-grpc-interop-client` and `pbrs-grpc-interop-server`).

### Platform Support
* **Linux (x86_64 / aarch64)**: Native CMake build or prebuilt binaries.
* **macOS (arm64 / x86_64)**: Native CMake build (Apple Clang / Homebrew GCC).

---

## 4. Build, Fetch, and Caching Procedures

The test runner `scripts/grpc-interop-cpp.sh` automates the discovery, compilation, and caching of the C++ reference peer binaries (`interop_client` and `interop_server`).

### Automatic Acquisition Flow
1. **Existing Binaries**: Checks if `interop_client` and `interop_server` are already present in `$GRPC_INTEROP_CPP_BIN_DIR` (default: `target/interop-cpp/`), `target/interop-cpp-build/`, `third_party/grpc/cmake/build/`, or configured via environment variables.
2. **Download Artifact**: If `GRPC_INTEROP_CPP_DOWNLOAD_URL` is set, downloads and extracts the pre-built tarball.
3. **Build from Source**: If `cmake` and a C++ compiler are available:
   - Fetches the pinned commit `d1487957db6658bc532b72871775148229836627` with `--depth 1` into `third_party/grpc`.
   - Initializes required submodules with `--depth 1` (`abseil-cpp`, `protobuf`, `re2`, `zlib`, `cares`).
   - Runs CMake configuration with `-DgRPC_BUILD_TESTS=ON -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=17`.
   - Builds targets `interop_client` and `interop_server` in parallel.
   - Caches output binaries into `target/interop-cpp/`.

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
The runner honors the following configuration variables:
* `GRPC_INTEROP_CPP_CLIENT`: Explicit path to `interop_client`.
* `GRPC_INTEROP_CPP_SERVER`: Explicit path to `interop_server`.
* `GRPC_INTEROP_CPP_BIN_DIR`: Directory containing C++ peer binaries (default: `target/interop-cpp`).
* `GRPC_INTEROP_CPP_DOWNLOAD_URL`: URL to download prebuilt C++ binaries archive.
* `GRPC_INTEROP_SKIP_BUILD`: When set to `1`, skips compilation and requires pre-existing binaries.
* `GRPC_INTEROP_LOG_DIR`: Directory for per-attempt stdout/stderr logs and final `report.json`.

---

## 5. Case Matrix & Execution Directions

The C++ interop harness executes across two cross-language peer directions:
1. **`kernel_client_to_cpp_server`**: Native client (`pbrs-grpc-interop-client`) calls C++ server (`interop_server`).
2. **`cpp_client_to_kernel_server`**: C++ client (`interop_client`) calls Native server (`pbrs-grpc-interop-server`).

### The 18 Test Cases
Every run evaluates all 14 baseline specification cases PLUS all 4 compression cases:

| Suite | Case Identifier | Peer Directions | Description |
|---|---|---|---|
| `standard_interop` | `empty_unary` | Both | Zero-byte request payload expecting zero-byte response payload. |
| `standard_interop` | `large_unary` | Both | 271,828 bytes request expecting 314,159 bytes response; asserts all-zero payload byte patterns. |
| `standard_interop` | `client_streaming` | Both | Client streams 4 request messages (74,922 total bytes); server returns aggregated response size. |
| `standard_interop` | `server_streaming` | Both | Client requests sequence of response sizes `[31415, 9, 2653, 58979]`; server streams exact payloads. |
| `standard_interop` | `ping_pong` | Both | Full-duplex bidi ping-pong message exchange across multiple turns. |
| `standard_interop` | `empty_stream` | Both | Half-closes bidi stream immediately; verifies no messages received before clean close. |
| `standard_interop` | `cancel_after_begin` | Both | Cancels stream immediately after creation; verifies CANCELLED status. |
| `standard_interop` | `cancel_after_first_response` | Both | Cancels bidi stream after first response; enforces terminal CANCELLED code assertion. |
| `standard_interop` | `timeout_on_sleeping_server` | Both | 1ms deadline against server sleeping 10s; verifies client observes `DEADLINE_EXCEEDED`. |
| `standard_interop` | `custom_metadata` | Both | Echoes initial text header and binary trailer (`-bin`); asserts trailers do not leak into headers. |
| `standard_interop` | `status_code_and_message` | Both | Verifies requested numeric status code and message string returned by server. |
| `standard_interop` | `special_status_message` | Both | Verifies Unicode and percent-encoded status messages (`\t\ntest\r-`, emoji, etc.). |
| `standard_interop` | `unimplemented_method` | Both | Invokes `/grpc.testing.TestService/UnimplementedCall`; asserts `UNIMPLEMENTED` status. |
| `standard_interop` | `unimplemented_service` | Both | Invokes method on non-existent `/grpc.testing.UnimplementedService`; asserts `UNIMPLEMENTED`. |
| `compression_interop` | `client_compressed_unary` | Both | Unary call with `expect_compressed.value = true`; asserts request was compressed on wire. |
| `compression_interop` | `server_compressed_unary` | Both | Unary call with `response_compressed.value = true`; asserts response was compressed on wire. |
| `compression_interop` | `client_compressed_streaming` | Both | Streaming call alternating compressed and uncompressed messages; asserts each message flag. |
| `compression_interop` | `server_compressed_streaming` | Both | Streaming call requesting compressed responses; asserts all response frames compressed. |

---

## 6. Compression Verification Details

In the gRPC wire protocol, message-level compression is signaled in the 5-byte data frame prefix:
```
+----------------+--------------------------------+
| Compressed (1) |        Message Length (4)      |
|     (0x01)     |         (big-endian u32)       |
+----------------+--------------------------------+
|                Message Payload                  |
|          (compressed bytes, e.g. gzip)          |
+-------------------------------------------------+
```

### Verification Requirements:
1. **`client_compressed_unary`**:
   - Client sends `SimpleRequest` with `expect_compressed: {value: true}`.
   - Client sets the Compressed-Flag bit (`0x01`) and encodes payload with gzip.
   - Server verifies the Compressed-Flag is present. If uncompressed, server returns `INVALID_ARGUMENT`.
   - Server responds with uncompressed `SimpleResponse`.

2. **`server_compressed_unary`**:
   - Client sends `SimpleRequest` with `response_compressed: {value: true}`.
   - Server sets the Compressed-Flag bit (`0x01`) and gzip-compresses the `SimpleResponse` payload.
   - Client asserts that the received frame has the Compressed-Flag bit set and decompresses the payload.

3. **`client_compressed_streaming`**:
   - Client sends message 1 with `expect_compressed: {value: true}` (wire compressed).
   - Client sends message 2 with `expect_compressed: {value: false}` (wire uncompressed).
   - Server asserts the expected compression state of each individual message.

4. **`server_compressed_streaming`**:
   - Client requests streaming output with `response_compressed: {value: true}`.
   - Server compresses every streamed response frame.
   - Client asserts that every received frame has the Compressed-Flag bit set.

---

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

---

## 8. Process Lifecycle & Diagnostics

* **Startup Readiness**: Servers are polled via socket connections with a strict timeout (`--startup-timeout`).
* **Execution Timeout**: Client cases run with deadline bounds (`--timeout`, default 15s) and process group cleanup on expiration.
* **Process Termination**: A comprehensive cleanup trap catches `EXIT`, `INT`, and `TERM`, terminating all spawned child PIDs using `SIGTERM` followed by `SIGKILL` escalation.
* **Retained Evidence**: Raw stdout/stderr logs for every attempt are stored in `target/interop-logs/<timestamp>_<pid>/`.
* **Machine-Readable Reports**: Results are recorded into `results.json` and aggregated into `report.json` via `scripts/interop-report.py`.
