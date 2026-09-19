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
1. **Server Efficiency (Fixed Load Generator)**: Hold client peer fixed (e.g. `--client-peer=native`), vary `--server-peer=native,tonic,go,cpp`.
2. **Client Efficiency (Fixed Server)**: Hold server peer fixed (e.g. `--server-peer=native`), vary `--client-peer=native,tonic,go,cpp`.
