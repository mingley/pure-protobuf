# RPC-Bench Peer Configuration and Launcher Registry

This directory contains peer configuration and launcher metadata for cross-peer and mixed-peer gRPC benchmarking according to `docs/benchmark-contract.md` (task BM-07).

## Registered Peers

| Peer ID | Implementation | Language | Runtime | Default Codec | Upstream Pin |
|---|---|---|---|---|---|
| `native` | `pbrs-grpc` | Rust | tokio + pbrs-grpc | `pbrs` | in-tree (pure-protobuf) |
| `tonic` | `tonic` | Rust | tokio + hyper + h2 | `prost` / `protobuf-tonic` | tonic v0.14.0, prost v0.13.0 |
| `go` | `grpc-go` | Go | go runtime + net/http2 | `google.golang.org/protobuf` | commit `dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef` (v1.85.0-dev) |
| `cpp` | `grpc-core` | C++ | grpc-core C++ event engine | `google::protobuf` (upb/C++) | commit `d1487957db6658bc532b72871775148229836627` (v1.84.0) |

## Matching Configuration (Apples-to-Apples)

All peers must execute under identical, matched network and payload parameters:
- **TCP_NODELAY**: Enabled on both client and server TCP sockets.
- **TLS Cipher**: `TLS_AES_128_GCM_SHA256` (TLS 1.3) when encryption is active.
- **Connections & Concurrency**: 1 connection and 1 concurrency for standard unary/stream scenarios; 4 connections / 16 streams for concurrency scenarios.
- **Payload Dimensions**:
  - `empty_unary`: 0 request bytes, 0 response bytes.
  - `large_unary`: 271,828 request bytes, 314,159 response bytes.
  - `stream`: 1,024 byte payload messages.
  - `ping_pong`: 0 byte payload messages in lockstep (256 pairs).
  - `upload`: 1,024 byte payload messages.

## Execution Matrix Roles

To isolate client vs. server performance:
1. **Server Efficiency (Fixed Load Generator)**: Hold client peer fixed (e.g. `--client-peer=native`), vary `--server-peer=native,tonic-pbrs,tonic-prost,go,cpp`.
2. **Client Efficiency (Fixed Server)**: Hold server peer fixed (e.g. `--server-peer=native`), vary `--client-peer=native,tonic-pbrs,go,cpp`.

## Tonic Codec Roles

The `tonic` transport ships in two codec variants so same-codec transport
deltas and end-to-end reference deltas are never conflated:

| Role | Transport | Codec | Load generator | Use |
|---|---|---|---|---|
| `tonic-pbrs` | tonic 0.14 (`rpc-bench --transport=tonic`) | pbrs | rpc-bench | Same-codec transport comparison vs `native` (client and server) |
| `tonic-prost` | tonic 0.14 (`tonic-interop`) | prost 0.14 | none | End-to-end reference, **server only** |
| `tonic` | legacy alias (deprecated for codec claims) | server prost, client pbrs | rpc-bench | Missing prost server fails; never substitutes another codec |

Rules enforced by `scripts/rpc-bench-matrix.py`:
- Requesting server `tonic-prost` without a built `tonic-interop` binary fails
  the cell (incomplete matrix) instead of silently substituting the pbrs codec.
- Requesting client `tonic-prost` fails the cell explicitly: no prost load
  generator exists. `--client-peer=all` excludes this known-invalid role,
  whereas explicitly requesting it still produces an incomplete matrix.
- Every run records `client_codec`, `server_codec`, `client_workload`
  (`rpc-bench` vs `interop-soak`), and both binary paths, so a soak-driven
  go/cpp client cell is never compared as equivalent to an rpc-bench cell.
- Go/C++ client cells require every requested soak iteration, zero failures and
  a distinct raw latency sample per iteration. Each client process has its own
  CPU accounting and retained stdout/stderr under `target/rpc-bench-logs/`;
  the report links those logs and records the integer-millisecond latency
  resolution. No missing histogram is replaced with an estimated p99.
- `matrix_complete` means requested cells executed, **not** that performance
  leadership or a server ceiling was proved. Quick runs, unmatched interop
  soak workloads, saturated generators, missing resource measurements, or
  scenarios shorter than 60 measured seconds receive an explicit
  `server_ceiling_reason` and are diagnostic only.
- A missing peer (or unsupported role) marks the report `matrix_complete: false`
  with explicit `failed_pairs`/`skipped_pairs`, prints an `[INCOMPLETE MATRIX]`
  banner, and exits 1: a partial table is never a comparison win.
