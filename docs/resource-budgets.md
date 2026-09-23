# gRPC Resource Budget Model & Memory Bounds Specification

**Task:** RT-05 ("Define a complete resource-budget model")  
**Pinned Standards:** RFC 9113 (HTTP/2), RFC 7541 (HPACK), gRPC over HTTP/2 Wire Specification, gRFC A6 (Client Retries)  
**Work Package:** RT Lane (Reliability & Resource Safety), Wave 1  
**Downstream Implementation:** RT-06 ("Enforce the approved transport byte budget"), RT-07 ("Verify overload fairness and slow-peer isolation")  
**Related Documents:** [docs/retry-contract.md](retry-contract.md), [docs/architecture.md](architecture.md), [docs/grpc.md](grpc.md)  
**Primary Source References:** `pbrs-grpc/src/config.rs`, `pbrs-grpc/src/limits.rs`, `pbrs-grpc/src/stream.rs`, `pbrs-grpc/src/wire.rs`, `pbrs-grpc/src/gzip.rs`, `pbrs-grpc/src/client.rs`, `pbrs-grpc/src/server.rs`

---

## 1. Executive Summary & Problem Statement

In distributed RPC environments, network servers and client connection pools face unpredictable traffic surges, slow clients, hostile connection spam, and oversized or malicious payloads. Without deterministic resource bounds, a gRPC service can exhaust memory (leading to OS OOM-killer termination) or exhaust task scheduling resources long before CPU saturation occurs.

In `pbrs-grpc`:
1. `ServerConfig` and `ChannelConfig` defaults provide safe defaults for individual message sizes (4 MiB) and per-connection stream concurrency (256 streams), but leave global connection counts (`max_concurrent_connections`) and global active RPC counts (`max_concurrent_rpcs`) **unbounded** (`None`).
2. High-throughput defaults—such as 16 MiB connection and stream flow-control windows (`DEFAULT_WINDOW_SIZE`), 1 MiB frame sizes (`DEFAULT_MAX_FRAME_SIZE`), and 1 MiB send buffers (`DEFAULT_MAX_SEND_BUFFER_SIZE`)—allow high single-stream performance, but can multiply into gigabytes of unconstrained memory consumption under overload if active connections and calls are not explicitly bounded.
3. Memory is divided into **transport-owned buffers** (which the gRPC kernel can strictly govern, account for, and reject before commitment) and **application-owned data** (which resides in user handlers, deserialized structs, and retained buffers).

This specification establishes:
- Rigorous mathematical upper-bound formulas for client and server memory consumption.
- A taxonomy strictly separating transport-owned buffers from application memory, detailing the hazards of backing-buffer retention.
- An overload analysis proving why unbounded defaults risk catastrophic process failure and how explicit bounds provide deterministic guarantees.
- The precise admission control and error semantics required for implementation in **RT-06**.
- Four standard, production-ready tuning profiles matching distinct operational environments.

---

## 2. Taxonomy of Buffers: Transport-Owned vs. Application-Owned

To construct a predictive memory model, memory allocations in `pbrs-grpc` are categorized by ownership, allocation lifecycle, and the enforcement mechanism that caps them.

```
┌───────────────────────────────────────────────────────────────────────────────────┐
│                           Total Process Memory (RSS)                              │
├─────────────────────────────────────────┬─────────────────────────────────────────┤
│          Transport-Owned Memory         │         Application-Owned Memory        │
├─────────────────────────────────────────┼─────────────────────────────────────────┤
│ • OS TCP rmem / wmem socket buffers     │ • Deserialized message structs          │
│ • TLS record & session state            │ • Domain business models & heap strings │
│ • HTTP/2 connection state (h2 engine)   │ • Handler async task stacks & locals    │
│ • HPACK dynamic encoder/decoder tables  │ • Application caches & DB query results │
│ • HTTP/2 send buffers & frame queues    │ • Streaming channels (mpsc queues)      │
│ • In-flight FrameReader carry buffers   │ • Retained Bytes slices (buffer holding)│
│ • Bounded decompression inflation arena │                                         │
│ • Transparent retry replay buffers      │                                         │
└─────────────────────────────────────────┴─────────────────────────────────────────┘
```

### 2.1 Detailed Buffer Classification

| Buffer Type | Owner | Typical Size Range | Controlled By / Bounded In | Reclaim Lifecycle |
|---|---|---|---|---|
| **TCP Socket Buffers (`SO_RCVBUF`, `SO_SNDBUF`)** | OS Kernel | 64 KiB – 4 MiB per socket | OS sysctl / TCP autotuning (`tcp_keepalive`, TCP window) | Connection termination |
| **TLS Session Context** | Transport (`rustls`) | 4 KiB – 32 KiB per socket | `ServerTls` / `ClientTls`, cipher suite state | Connection termination |
| **HTTP/2 Connection Driver** | Transport (`h2::server::Connection` / client driver) | 8 KiB – 32 KiB per conn | Tokio task allocation, stream table trees | Connection termination |
| **HPACK Dynamic Tables** | Transport (`h2`) | $2 \times 4096$ octets default | `ServerConfig::header_table_size` (`DEFAULT_HEADER_TABLE_SIZE`) | Connection termination |
| **Connection Send Buffer** | Transport (`h2`) | Up to 1 MiB per conn | `ServerConfig::max_send_buffer_size` (`DEFAULT_MAX_SEND_BUFFER_SIZE`) | Drained on write ACK |
| **Small-DATA Framing Budget** | Transport (`h2`) | 25,600 bytes per conn | `ServerConfig::data_frame_budget` (`DEFAULT_DATA_FRAME_BUDGET`) | Drained on write ACK |
| **Reset Stream Tracking** | Transport (`h2`) | ~1 KiB – 10 KiB per conn | `max_concurrent_reset_streams`, `max_local_error_reset_streams` | Purged after `reset_stream_duration` (1s) |
| **Uncompressed Header Block** | Transport (HPACK decoder) | Up to 16 KiB per RPC | `ServerConfig::max_header_list_size` (`DEFAULT_MAX_HEADER_LIST_SIZE`) | Freed after header parse |
| **Inbound Frame Carry Buffer** | Transport (`wire::FrameReader`) | Up to 4 MiB (chunk-straddling) | `MessageLimits::max_decoding` (`DEFAULT_MAX_DECODING_MESSAGE_SIZE`) | Freed on frame yield |
| **Decompression Expansion Buffer** | Transport (`gzip::decode_limited`) | Up to 4 MiB + 1 byte | `MessageLimits::inflate_budget` (`limits.max_decoding()`) | Freed after protobuf decode |
| **Outbound Frame Serialization** | Transport (`wire::frame_from_msg`) | Serialized payload length | `MessageLimits::max_encoding` / application message size | Dropped after wire enqueue |
| **Transparent Retry Replay Buffer** | Transport / Client (`Call` future) | 1 request frame (`Bytes`) | `encode_msg(&msg)` in `pbrs-grpc/src/client.rs` | Dropped on attempt commit or final status |
| **Streaming Channel Queue** | Transport / Application (`mpsc::channel`) | `buffer` $\times$ message size | `Streaming::channel(buffer)`, `ChannelConfig::stream_buffer` | Dropped as consumer reads |
| **Deserialized Message Struct** | Application (`T: Parse + Default`) | Domain dependent | Rust allocator / struct lifetime | Dropped by application handler |
| **Retained Backing Buffers** | Application (`bytes::Bytes`) | Original chunk size | Application holding sub-slice of transport `Bytes` | Dropped when last `Bytes` clone drops |

Outbound gRPC messages can be larger than the configured HTTP/2 send buffer or
the peer's stream window. `wire::send_bytes` queues small frames directly when
the buffer accepts them; otherwise it slices the encoded frame into DATA chunks
no larger than the positive send-buffer limit and currently granted credit.
Only the final chunk carries `END_STREAM` when the caller requests it. The
writer never waits for credit equal to the entire message, which could
otherwise stall behind a smaller buffer. The serialized message itself remains
live while chunks are queued and is separately governed by the outbound
encoding cap; the send-buffer limit alone is not a total-message memory cap.

The public `pbrs_grpc::ByteBudgetTracker` can share an explicit transport-byte
cap through `Server::with_byte_budget_tracker` or
`Channel::with_byte_budget_tracker`. Acquired `BytePermit`s return their bytes
on drop, and `allocated()` / `is_quiescent()` expose the accounting state.
This tracks only the buffers it owns, not application allocations or process
RSS.

### 2.2 The Backing Buffer Retention Hazard

A critical architectural hazard in zero-copy networking engines is **backing buffer retention**:
- In `pbrs-grpc`, network payloads arrive as reference-counted `bytes::Bytes` chunks sliced from larger I/O buffers.
- When `FrameReader` extracts a frame that fits inside a single incoming chunk, it calls `chunk.split_to(frame_len)`, which performs an $O(1)$ reference-count increment sharing the underlying buffer allocation.
- If application handler logic parses a string or byte slice into a long-lived cache by shallow-copying or retaining a `Bytes` handle (or if custom zero-copy deserialization is employed), **the entire physical allocation (often 16 KiB to 64 KiB) remains anchored in heap memory**, even if the application only references a 16-byte field.
- **Rule for Handlers:** Generated code and user interceptors must clone or copy owned data (`Vec<u8>`, `String`) out of transient request envelopes if the data is stored in caches or transferred across thread boundaries, allowing the underlying transport `Bytes` block to return immediately to the transport allocator.

---

## 3. Mathematical Memory Consumption Models

Let:
- $N_{\text{conn}}$: Total number of active concurrent transport connections.
- $N_{\text{rpc}}$: Total number of active concurrent RPCs currently in flight across all connections.
- $N_{\text{stream\_per\_conn}}$: Max active HTTP/2 streams per connection (bounded by `max_concurrent_streams`, default 256).
- $W_{\text{conn}}$: HTTP/2 connection flow-control window (`initial_connection_window_size`, default 16 MiB).
- $W_{\text{stream}}$: HTTP/2 stream flow-control window (`initial_stream_window_size`, default 16 MiB).
- $S_{\text{send\_buf}}$: HTTP/2 connection send buffer ceiling (`max_send_buffer_size`, default 1 MiB).
- $H_{\text{table}}$: HPACK dynamic header table size (`header_table_size`, default 4,096 bytes).
- $H_{\text{list}}$: Maximum uncompressed header list size (`max_header_list_size`, default 16,384 bytes).
- $M_{\text{decode\_cap}}$: Maximum inbound message decoding size (`max_decoding`, default 4 MiB).
- $M_{\text{encode\_cap}}$: Maximum outbound message encoding size (`max_encoding`, default $\infty$ / application bounded).
- $B_{\text{stream\_queue}}$: Streaming queue depth in messages (`stream_buffer`, default 16).

---

### 3.1 Server Upper-Bound Memory Model

The worst-case theoretical server memory $\text{Memory}_{\text{server}}$ committed to networking, connection state, stream processing, and in-flight RPCs is governed by:

$$\text{Memory}_{\text{server}} = M_{\text{base}} + \sum_{i=1}^{N_{\text{conn}}} M_{\text{conn\_overhead}}^{(i)} + \sum_{j=1}^{N_{\text{rpc}}} M_{\text{rpc\_overhead}}^{(j)} + M_{\text{app\_state}}$$

Where the per-connection and per-RPC terms expand as follows:

#### 1. Per-Connection Overhead ($M_{\text{conn\_overhead}}$)
Every admitted connection commits fixed transport overhead, kernel socket buffers, and connection-level flow control windows:

$$M_{\text{conn\_overhead}} = M_{\text{socket\_tcp}} + M_{\text{tls\_state}} + M_{\text{h2\_conn\_state}} + M_{\text{hpack\_tables}} + M_{\text{send\_buffer}} + M_{\text{data\_frame\_budget}} + M_{\text{reset\_tracking}} + W_{\text{conn\_effective}}$$

- **$M_{\text{socket\_tcp}}$:** Kernel TCP send and receive buffers ($\approx 128 \text{ KiB} \text{ to } 512 \text{ KiB}$).
- **$M_{\text{tls\_state}}$:** Rustls connection state, crypto contexts, and session keys ($\approx 16 \text{ KiB}$).
- **$M_{\text{h2\_conn\_state}}$:** Tokio connection driver task state, ping-pong keepalive timer, stream prioritization trees ($\approx 16 \text{ KiB}$).
- **$M_{\text{hpack\_tables}}$:** HPACK encoder and decoder dynamic tables: $2 \times H_{\text{table}} = 2 \times 4,096 \text{ B} = 8 \text{ KiB}$.
- **$M_{\text{send\_buffer}}$:** Connection-level outbound buffer waiting on peer flow control: $S_{\text{send\_buf}} = 1 \text{ MiB}$.
- **$M_{\text{data\_frame\_budget}}$:** Small DATA framing allocation: $25,600 \text{ B} \approx 25 \text{ KiB}$.
- **$M_{\text{reset\_tracking}}$:** Tracking reset streams: $(N_{\text{reset\_accept}} + N_{\text{reset\_local}} + N_{\text{reset\_concurrent}}) \times 16 \text{ B} \approx 18 \text{ KiB}$.
- **$W_{\text{conn\_effective}}$:** Unconsumed inbound DATA frames buffered across all streams on this connection. Crucially, the HTTP/2 connection window bounds the aggregate unacknowledged DATA frames received across all streams on that connection:
  $$W_{\text{conn\_effective}} \le W_{\text{conn}} = 16 \text{ MiB}$$

Combining static connection components ($M_{\text{conn\_static}}$):
$$M_{\text{conn\_static}} \approx 300 \text{ KiB} + 8 \text{ KiB} + 1,024 \text{ KiB} + 25 \text{ KiB} + 18 \text{ KiB} \approx 1.375 \text{ MiB}$$
Adding connection window:
$$M_{\text{conn\_overhead}} \le 1.375 \text{ MiB} + W_{\text{conn}}$$

#### 2. Per-RPC Overhead ($M_{\text{rpc\_overhead}}$)
For each admitted and actively processing RPC:

$$M_{\text{rpc\_overhead}} = M_{\text{task\_stack}} + M_{\text{header\_decoded}} + M_{\text{inbound\_carry}} + M_{\text{decompression}} + M_{\text{outbound\_encode}} + M_{\text{stream\_channel}}$$

- **$M_{\text{task\_stack}}$:** Tokio async task allocation (`tokio::spawn`), state machine frame, context map, cancel watch receiver, and semaphore permit ($\approx 4 \text{ KiB}$).
- **$M_{\text{header\_decoded}}$:** Decoded request `Metadata` map: bounded by $H_{\text{list}} = 16 \text{ KiB}$.
- **$M_{\text{inbound\_carry}}$:** `wire::FrameReader` carry buffer when a frame straddles multiple chunks: bounded by $M_{\text{decode\_cap}} = 4 \text{ MiB}$.
- **$M_{\text{decompression}}$:** Memory allocated during gzip inflation in `gzip::decode_limited`: bounded by $M_{\text{decode\_cap}} + 1 \text{ B} = 4 \text{ MiB} + 1 \text{ B}$.
- **$M_{\text{outbound\_encode}}$:** Serialized response frame in `frame_from_msg` / `append_frame`: bounded by $M_{\text{encode\_cap}}$ (or actual message size, typically $\le 4 \text{ MiB}$).
- **$M_{\text{stream\_channel}}$:** For server-streaming or bidi calls using `Streaming::channel(buffer)`: $B_{\text{stream\_queue}} \times S_{\text{item}} \approx 16 \times S_{\text{msg}}$.

#### 3. Full Server Upper-Bound Formula
Combining connection and RPC terms, and accounting for the invariant that unconsumed inbound wire bytes across a connection are capped by $\min(W_{\text{conn}}, \sum_{\text{stream}} W_{\text{stream}})$:

$$\boxed{\text{Memory}_{\text{server}} \le N_{\text{conn}} \times \left( M_{\text{conn\_static}} + S_{\text{send\_buf}} + W_{\text{conn}} \right) + N_{\text{rpc}} \times \left( M_{\text{task}} + H_{\text{list}} + M_{\text{decode\_cap}} + M_{\text{inflate\_cap}} + M_{\text{encode}} \right) + M_{\text{app}}}$$

Plugging in `pbrs-grpc` default parameters:
$$\text{Memory}_{\text{server\_default}} \le N_{\text{conn}} \times \left( 1.375 \text{ MiB} + 16 \text{ MiB} \right) + N_{\text{rpc}} \times \left( 20 \text{ KiB} + 4 \text{ MiB} + 4 \text{ MiB} + M_{\text{encode}} \right) + M_{\text{app}}$$
$$\text{Memory}_{\text{server\_default}} \approx N_{\text{conn}} \times 17.38 \text{ MiB} + N_{\text{rpc}} \times (8.02 \text{ MiB} + M_{\text{encode}}) + M_{\text{app}}$$

---

### 3.2 Client Upper-Bound Memory Model

The client memory consumption model accounts for pooled channel connections, active outgoing calls, call queueing, and the **transparent retry replay buffer**.

$$\text{Memory}_{\text{client}} = M_{\text{client\_base}} + \sum_{k=1}^{N_{\text{channel\_conns}}} M_{\text{conn\_overhead}}^{(k)} + \sum_{m=1}^{N_{\text{client\_rpc}}} M_{\text{call\_overhead}}^{(m)} + M_{\text{client\_app}}$$

#### 1. Per-Connection Component ($N_{\text{channel\_conns}}$)
In `ChannelConfig`, `connections` specifies the active HTTP/2 connection pool size (default 1):
$$M_{\text{client\_conn}} = M_{\text{socket\_tcp}} + M_{\text{tls\_client}} + M_{\text{h2\_driver}} + 2 \times H_{\text{table}} + S_{\text{send\_buf}} + W_{\text{conn}} \approx 1.375 \text{ MiB} + W_{\text{conn}}$$

#### 2. Per-RPC Component ($M_{\text{call\_overhead}}$)
For each in-flight client RPC:
$$M_{\text{call\_overhead}} = M_{\text{call\_future}} + M_{\text{replay\_buffer}} + M_{\text{client\_stream\_queue}} + M_{\text{inbound\_response\_decode}}$$

- **$M_{\text{call\_future}}$:** Pinned async state machine of `Call` future ($\approx 2 \text{ KiB} \text{ to } 8 \text{ KiB}$).
- **$M_{\text{replay\_buffer}}$ (Transparent Retry Buffer):**
  - In `pbrs-grpc/src/client.rs` (`unary` and `server_streaming`), the request is serialized and framed **before** acquiring a stream slot:
    ```rust
    let frame = encode_msg(&msg, compress, wire.limits, wire.gzip_level)?;
    ```
  - This `frame` (`Bytes`) is held in the async task state across the attempt so that if a transparent retry occurs (e.g. on `REFUSED_STREAM` or GOAWAY before data transmission), `frame.clone()` re-sends the payload without re-serialization.
  - Memory committed: $S_{\text{request\_frame}} \le M_{\text{encode\_cap}}$ (up to 4 MiB or configured request size).
  - Held until the attempt receives response headers (committing the call) or fails permanently.
- **$M_{\text{client\_stream\_queue}}$:**
  - For client-streaming and bidi calls: outbound messages are queued into `mpsc::channel(stream_buffer)` (default 16 messages).
  - Memory committed: $B_{\text{stream\_queue}} \times S_{\text{request\_msg}} = 16 \times S_{\text{msg}}$.
- **$M_{\text{inbound\_response\_decode}}$:**
  - Response frame decoding and decompression: bounded by $M_{\text{decode\_cap}} + M_{\text{inflate\_cap}} \le 4 \text{ MiB} + 4 \text{ MiB} = 8 \text{ MiB}$.

#### 3. Full Client Upper-Bound Formula
$$\boxed{\text{Memory}_{\text{client}} \le N_{\text{conn}} \times \left( 1.375 \text{ MiB} + W_{\text{conn}} \right) + N_{\text{rpc}} \times \left( M_{\text{future}} + S_{\text{replay\_buf}} + B_{\text{queue}} \times S_{\text{msg}} + M_{\text{decode\_cap}} + M_{\text{inflate\_cap}} \right)}$$

---

## 4. Default Configuration Vulnerability & Overload Analysis

### 4.1 Audit of Defaults (`ServerConfig` & `ChannelConfig`)

Inspecting `pbrs-grpc/src/config.rs`:
- `max_concurrent_connections`: **`None` (unbounded)**
- `max_concurrent_rpcs`: **`None` (unbounded)**
- `initial_connection_window_size`: **16 MiB** (`DEFAULT_WINDOW_SIZE`)
- `initial_stream_window_size`: **16 MiB** (`DEFAULT_WINDOW_SIZE`)
- `max_concurrent_streams`: **256** per connection (`DEFAULT_MAX_CONCURRENT_STREAMS`)
- `max_send_buffer_size`: **1 MiB** (`DEFAULT_MAX_SEND_BUFFER_SIZE`)
- `max_decoding`: **4 MiB** (`DEFAULT_MAX_DECODING_MESSAGE_SIZE`)

### 4.2 The Combinatorial Explosion Hazard Under Default Settings

Under default settings, the server relies entirely on the OS file descriptor limit and TCP memory caps to halt incoming connection growth, and relies entirely on client cooperativeness to bound RPC concurrency.

Consider an internal microservice or edge ingress facing a connection spike or slow client attack:
1. **Connection Flooding ($N_{\text{conn}} = 1,000$):**
   - Fixed connection state alone: $1,000 \times 1.375 \text{ MiB} \approx 1.375 \text{ GiB}$.
   - If peers write into their connection windows ($W_{\text{conn}} = 16 \text{ MiB}$):
     $$1,000 \times 16 \text{ MiB} = 16.0 \text{ GiB}$$
2. **Concurrent Stream Overload ($N_{\text{rpc}}$ with 1,000 connections):**
   - Each connection permits up to 256 concurrent streams by HTTP/2 `SETTINGS`.
   - Max concurrent RPCs across the server: $N_{\text{rpc}} = 1,000 \times 256 = 256,000 \text{ RPCs}$.
   - If each RPC transmits an average 64 KiB message:
     $$256,000 \times 64 \text{ KiB} \approx 16.38 \text{ GiB}$$
   - If each RPC inflates a 4 MiB compressed payload:
     $$256,000 \times 8 \text{ MiB} \approx 2.0 \text{ TiB}$$

**Conclusion:** Default settings are optimized for high-throughput single-stream microbenchmarks, but **cannot survive unbounded concurrent load**. Without explicit ceilings on $N_{\text{conn}}$ and $N_{\text{rpc}}$, a traffic spike triggers non-linear memory growth, latency collapse, and unrecoverable process termination.

### 4.3 Deterministic Protection via Explicit Bounds

Setting explicit values for `max_concurrent_connections` and `max_concurrent_rpcs` transforms the unbounded memory equation into a **provable, fixed ceiling**:

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(500)
    .max_concurrent_rpcs(1000);
```

Under this configuration:
- Connections are strictly bounded at $N_{\text{conn}} \le 500$.
- In-flight RPC handler tasks are strictly bounded at $N_{\text{rpc}} \le 1,000$, **regardless of how many streams are opened across the 500 connections**.
- Total transport-committed memory is bounded by:
  $$\text{Memory}_{\text{server\_max}} \le 500 \times (1.375 \text{ MiB} + W_{\text{conn}}) + 1,000 \times (8.02 \text{ MiB}) + M_{\text{app}}$$
- System administrators can calculate the exact maximum resident set size (RSS) needed for container memory limits (`cgroups` limits), ensuring zero risk of unexpected OOM kills.

---

## 5. Admission Control & Error Semantics (Contract for RT-06)

To enforce the approved resource budget model, **RT-06** must implement admission checks at distinct lifecycle boundaries. Every rejection must occur **before** the guarded memory is allocated.

```
Incoming Connection (TCP Accept)
       │
       ▼
[ Connection Semaphore? ] ───(Over Limit)───► Immediate TCP Drop OR HTTP/2 GOAWAY
       │                                       (Zero TLS/h2 memory allocated)
       ▼ (Permit Acquired)
Handshake & HTTP/2 Session Established
       │
       ▼
Incoming Stream (poll_accept / HEADERS)
       │
       ▼
[ RPC Concurrency Semaphore? ] ───(Over Limit)───► Immediate Trailers-Only Response
       │                                            - Status: RESOURCE_EXHAUSTED
       │                                            - end_stream: true
       │                                            - ZERO body bytes read/buffered
       ▼ (Permit Acquired)
Frame Header (5-byte prefix)
       │
       ▼
[ Message Length <= max_decoding? ] ───(Over Limit)───► Immediate RESOURCE_EXHAUSTED
       │                                                 (Zero payload memory allocated)
       ▼ (Length Valid)
Payload Data Received
       │
       ▼
[ Compressed Flag Set? ]
       ├── No ──► Parse Protobuf (T::parse)
       └── Yes ─► Bounded Decompression (GzDecoder.take(budget + 1))
                       │
                       └──(Exceeds Budget)──► Immediate RESOURCE_EXHAUSTED
                                               (Max memory: budget + 1 byte)
```

### 5.1 Connection Concurrency Limit (`max_concurrent_connections`)

- **Enforcement Location:** Server accept loop in `pbrs-grpc/src/server.rs` (`accept_loop`) immediately upon `listener.accept()`.
- **Mechanism:** `slots.try_acquire_owned()`.
- **Rejection Behavior:**
  - **Preferred:** Immediate TCP socket drop (`drop(tcp)`). The socket is closed before spawning a handshake task, before allocating TLS session memory, and before initiating HTTP/2 preface processing.
  - **HTTP/2 Active Alternative:** If connection limiting is applied after preface or during connection recycling, the server immediately transmits an HTTP/2 `GOAWAY` frame with error code `NO_ERROR` (graceful drain) or `ENHANCE_YOUR_CALM` (rate/concurrency exceeded) with `last_stream_id = 0`, then terminates the connection.
- **Resource Guarantee:** Zero server heap memory is committed to TLS or HTTP/2 connection tracking for rejected connections.

### 5.2 Process-Wide RPC Concurrency Limit (`max_concurrent_rpcs`)

- **Enforcement Location:**
  - **Server:** Connection worker loop in `pbrs-grpc/src/server.rs` (`serve_io`) upon receiving an incoming stream via `conn.poll_accept()`, **before** reading request DATA frames and before spawning the handler dispatch task.
  - **Client:** Client call entry in `pbrs-grpc/src/client.rs` (`take_rpc_slot()`), **before** acquiring a channel connection slot and before writing request HEADERS.
- **Server Rejection Behavior:**
  - The server immediately responds with an HTTP/2 **Trailers-Only** response (`wire::reject`):
    - `:status = 200`
    - `content-type = application/grpc`
    - `grpc-status = 8` (`Code::ResourceExhausted`)
    - `grpc-message = "too many concurrent RPCs"`
    - `end_stream = true` set directly on the initial `HEADERS` frame.
  - **Infallible Zero-Allocation Invariant:**
    - **No request body data is read or buffered**.
    - No `FrameReader` chunk or carry buffer is allocated or expanded.
    - No decompression or protobuf parsing is attempted.
    - No handler task or async future is spawned.
- **Client Rejection Behavior:**
  - `channel.take_rpc_slot()` fails synchronously returning:
    $$\text{Err}(\text{Status::resource\_exhausted("too many concurrent RPCs")})$$
  - The client transmits zero bytes over the network and opens no HTTP/2 stream.

### 5.3 Inbound & Outbound Message Size Limits (`max_decoding` & `max_encoding`)

- **Inbound Decoding Cap:**
  - Enforced in `limits.check_decode(n)` and `wire::codec::pop_limited`.
  - The 5-byte gRPC framing header `[compressed: u8, len: u32]` is parsed from the wire.
  - If `len > max_decoding`, the frame is rejected immediately with `Code::ResourceExhausted("decoded message length {n} exceeds limit {max}")`.
  - **Invariant:** The payload memory is rejected **before** allocating the destination buffer or reading the payload bytes from the stream.
- **Outbound Encoding Cap:**
  - Enforced in `limits.check_encode(n)` and `wire::frame_from_msg` / `StreamSender::send`.
  - The message's serialized length is computed via `T::serialized_len(&msg)`.
  - If length exceeds `max_encoding`, encoding is refused with `Code::ResourceExhausted`.
  - **Invariant:** No oversized byte buffer is allocated or queued onto the HTTP/2 send stream.

### 5.4 Inbound Gzip Decompression Expansion Budget

- **Enforcement Location:** `pbrs-grpc/src/gzip.rs` (`decode_limited`).
- **Mechanism:**
  - Inbound compressed frames are inflated through:
    ```rust
    GzDecoder::new(payload).take(read_cap).read_to_end(&mut out)
    ```
  - Where `read_cap = budget.saturating_add(1)` and `budget = limits.inflate_budget() = max_decoding`.
- **Rejection Behavior:**
  - If inflated output exceeds `budget`, decompression aborts immediately with `Code::ResourceExhausted("decompressed message exceeds limit {budget}")`.
- **Invariant:**
  - Peak memory allocated during decompression is strictly capped at $\min(\text{actual\_size}, \text{budget} + 1)$.
  - Malicious "gzip bombs" (e.g. tiny 1 KiB payload inflating to 10 GiB of zeroes) are terminated at exactly 4 MiB + 1 byte, completely neutralizing memory exhaustion amplification attacks.

### 5.5 Summary Contract Matrix for RT-06

| Limit Exceeded | Detection Point | Incurred Allocation | Network Action | gRPC Status Code |
|---|---|---|---|---|
| **Connection Cap** | `listener.accept()` | 0 bytes | Drop TCP socket immediately | Socket RST / Drop |
| **Server RPC Cap** | `conn.poll_accept()` | 0 bytes body | Send Trailers-Only HEADERS (`end_stream=true`) | `RESOURCE_EXHAUSTED` (8) |
| **Client RPC Cap** | `Call::poll()` / invoke | 0 bytes | Fail client future locally, no wire frames | `RESOURCE_EXHAUSTED` (8) |
| **Inbound Msg Size** | 5-byte frame header | 0 bytes payload | Send Trailers-Only response or stream reset | `RESOURCE_EXHAUSTED` (8) |
| **Outbound Msg Size** | `T::serialized_len()` | 0 bytes frame | Abort serialization, return error | `RESOURCE_EXHAUSTED` (8) |
| **Gzip Bomb** | Decompression loop | Max `budget + 1` | Abort inflate, send Trailers-Only error | `RESOURCE_EXHAUSTED` (8) |

---

## 6. Recommended Bounded Production Profiles

To assist operators in configuring deterministic memory bounds, this section defines four standardized production profiles.

```
       [ Edge Ingress / Proxy ]       [ Internal Microservice ]     [ High-Throughput Bulk ]
       • 5,000 connections            • 200 connections             • 20 connections
       • 32 streams / conn            • 256 streams / conn          • 64 streams / conn
       • 256 KiB conn window          • 4 MiB conn window           • 16 MiB conn window
       • 64 KiB stream window         • 1 MiB stream window         • 16 MiB stream window
       • 1 MiB max msg                • 4 MiB max msg               • 32 MiB max msg
       ────────────────────────       ─────────────────────────     ─────────────────────────
       Max Transport RAM: ~1.4 GiB    Max Transport RAM: ~1.8 GiB   Max Transport RAM: ~3.5 GiB
```

### 6.1 Profile 1: Edge Ingress / Reverse Proxy Profile

**Operational Context:** Public-facing ingress gateways, mobile/web clients, untrusted networks, high connection churn, Slowloris threats.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(5_000)
    .max_concurrent_rpcs(10_000)
    .max_concurrent_streams(32)
    .initial_connection_window_size(256 * 1024)       // 256 KiB
    .initial_stream_window_size(64 * 1024)            // 64 KiB
    .max_send_buffer_size(128 * 1024)                 // 128 KiB
    .max_header_list_size(8 * 1024)                   // 8 KiB
    .max_decoding_message_size(1024 * 1024)           // 1 MiB
    .max_encoding_message_size(1024 * 1024)           // 1 MiB
    .connect_timeout(Duration::from_secs(5))
    .max_connection_idle(Duration::from_secs(60))
    .max_connection_age(Duration::from_secs(600));
```

- **Per-Connection Static Overhead:** $\approx 300 \text{ KiB} + 8 \text{ KiB} + 128 \text{ KiB} + 256 \text{ KiB} \approx 692 \text{ KiB}$.
- **Max Connection Memory ($N_{\text{conn}} = 5,000$):** $5,000 \times 692 \text{ KiB} \approx 3.30 \text{ GiB}$ (unloaded) / $\approx 600 \text{ MiB}$ (idle TCP).
- **Per-RPC Overhead ($M_{\text{rpc}}$ at 1 MiB cap):** $\approx 10 \text{ KiB} + 1 \text{ MiB} \text{ (decode)} + 1 \text{ MiB} \text{ (inflate)} \approx 2.01 \text{ MiB}$.
- **Max Active RPC Memory ($N_{\text{rpc}} = 10,000$ concurrent limit):**
  - If average payload is 16 KiB: $10,000 \times 32 \text{ KiB} \approx 320 \text{ MiB}$.
  - Absolute theoretical upper bound (all 10,000 RPCs inflight at max 1 MiB message simultaneously): $\approx 1.5 \text{ GiB} \text{ to } 3.0 \text{ GiB}$.
- **Recommended Container Memory Limit:** 4 GiB.

---

### 6.2 Profile 2: Internal Microservice Profile (Standard Production)

**Operational Context:** Service-to-service internal RPC mesh, trusted Kubernetes pods, low-latency requirements, pooled connections.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(200)
    .max_concurrent_rpcs(2_000)
    .max_concurrent_streams(256)
    .initial_connection_window_size(4 * 1024 * 1024)  // 4 MiB
    .initial_stream_window_size(1024 * 1024)          // 1 MiB
    .max_send_buffer_size(1024 * 1024)                // 1 MiB
    .max_header_list_size(16 * 1024)                  // 16 KiB
    .max_decoding_message_size(4 * 1024 * 1024)       // 4 MiB
    .max_encoding_message_size(4 * 1024 * 1024)       // 4 MiB
    .keep_alive_interval(Duration::from_secs(30))
    .keep_alive_timeout(Duration::from_secs(10));
```

- **Per-Connection Static Overhead:** $\approx 300 \text{ KiB} + 8 \text{ KiB} + 1 \text{ MiB} + 4 \text{ MiB} \approx 5.3 \text{ MiB}$.
- **Max Connection Memory ($N_{\text{conn}} = 200$):** $200 \times 5.3 \text{ MiB} \approx 1.06 \text{ GiB}$.
- **Max Active RPC Memory ($N_{\text{rpc}} = 2,000$):**
  - Typical internal RPC payload (32 KiB): $2,000 \times 64 \text{ KiB} \approx 128 \text{ MiB}$.
  - Peak burst capacity: $2,000 \times \text{in-flight buffers} \approx 1.2 \text{ GiB}$.
- **Recommended Container Memory Limit:** 2 GiB to 3 GiB.

---

### 6.3 Profile 3: High-Throughput Bulk / Analytics Profile

**Operational Context:** Big data pipelines, log streaming, distributed dataset replication, batch transfers with large messages.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(20)
    .max_concurrent_rpcs(100)
    .max_concurrent_streams(64)
    .initial_connection_window_size(16 * 1024 * 1024) // 16 MiB
    .initial_stream_window_size(16 * 1024 * 1024)     // 16 MiB
    .max_send_buffer_size(4 * 1024 * 1024)            // 4 MiB
    .max_decoding_message_size(32 * 1024 * 1024)      // 32 MiB
    .max_encoding_message_size(32 * 1024 * 1024)      // 32 MiB
    .stream_buffer_size(32);
```

- **Per-Connection Static Overhead:** $\approx 300 \text{ KiB} + 4 \text{ MiB} + 16 \text{ MiB} \approx 20.3 \text{ MiB}$.
- **Max Connection Memory ($N_{\text{conn}} = 20$):** $20 \times 20.3 \text{ MiB} \approx 406 \text{ MiB}$.
- **Max Active RPC Memory ($N_{\text{rpc}} = 100$ at 32 MiB message cap):**
  - $100 \times (32 \text{ MiB} \text{ decode} + 32 \text{ MiB} \text{ inflate}) \approx 6.4 \text{ GiB}$ peak worst-case.
- **Recommended Container Memory Limit:** 8 GiB.

---

### 6.4 Profile 4: Resource-Constrained / Sidecar Profile

**Operational Context:** Service mesh sidecars, IoT/edge embedded compute, constrained memory environments (<128 MiB RAM budget).

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(10)
    .max_concurrent_rpcs(50)
    .max_concurrent_streams(16)
    .initial_connection_window_size(64 * 1024)        // 64 KiB
    .initial_stream_window_size(32 * 1024)            // 32 KiB
    .max_send_buffer_size(32 * 1024)                  // 32 KiB
    .max_header_list_size(4 * 1024)                   // 4 KiB
    .header_table_size(1024)                          // 1 KiB
    .max_decoding_message_size(256 * 1024)            // 256 KiB
    .max_encoding_message_size(256 * 1024)            // 256 KiB
    .stream_buffer_size(4);
```

- **Per-Connection Static Overhead:** $\approx 100 \text{ KiB} + 32 \text{ KiB} + 64 \text{ KiB} \approx 196 \text{ KiB}$.
- **Max Connection Memory ($N_{\text{conn}} = 10$):** $10 \times 196 \text{ KiB} \approx 2 \text{ MiB}$.
- **Max Active RPC Memory ($N_{\text{rpc}} = 50$ at 256 KiB message cap):** $50 \times 512 \text{ KiB} \approx 25.6 \text{ MiB}$.
- **Total Peak Footprint:** $\le 35 \text{ MiB}$.
- **Recommended Container Memory Limit:** 64 MiB.

---

### 6.5 Comparative Profile Summary

| Metric / Parameter | Default (Unbounded) | Edge Ingress Profile | Internal Microservice | Bulk / Analytics | Sidecar Profile |
|---|---|---|---|---|---|
| **Max Concurrent Connections ($N_{\text{conn}}$)** | $\infty$ (`None`) | 5,000 | 200 | 20 | 10 |
| **Max Concurrent RPCs ($N_{\text{rpc}}$)** | $\infty$ (`None`) | 10,000 | 2,000 | 100 | 50 |
| **Max Streams / Conn** | 256 | 32 | 256 | 64 | 16 |
| **Connection Window ($W_{\text{conn}}$)** | 16 MiB | 256 KiB | 4 MiB | 16 MiB | 64 KiB |
| **Stream Window ($W_{\text{stream}}$)** | 16 MiB | 64 KiB | 1 MiB | 16 MiB | 32 KiB |
| **Send Buffer ($S_{\text{send\_buf}}$)** | 1 MiB | 128 KiB | 1 MiB | 4 MiB | 32 KiB |
| **Max Inbound Message Cap** | 4 MiB | 1 MiB | 4 MiB | 32 MiB | 256 KiB |
| **Max Header List** | 16 KiB | 8 KiB | 16 KiB | 16 KiB | 4 KiB |
| **Peak Transport Bound (Formula)** | $\mathbf{\infty \text{ (Unbounded OOM)}}$ | **$\approx 3.3 \text{ GiB}$** | **$\approx 1.8 \text{ GiB}$** | **$\approx 6.8 \text{ GiB}$** | **$\approx 35 \text{ MiB}$** |
| **Recommended Container RAM** | Unknown / Crash-prone | 4 GiB | 2 GiB – 3 GiB | 8 GiB | 64 MiB |

---

## 7. Task & Concurrency Budget Model

Memory is not the only constrained resource: Tokio runtime worker threads, task queues, and semaphore locks also require explicit budgeting.

```
Incoming Requests
       │
       ▼
[ Accept Semaphore: max_concurrent_connections ]
       │
       ├── Acquired: Spawn 1 Tokio Connection Task (serve_io)
       │                    │
       │                    ▼
       │      [ RPC Semaphore: max_concurrent_rpcs ]
       │                    │
       │                    ├── Acquired: Spawn 1 Tokio Dispatch Task (handler)
       │                    │                    │
       │                    │                    ▼
       │                    │             Execute RPC
       │                    │                    │
       │                    │                    ▼
       │                    │             Drop Permit (Release slot)
       │                    │
       │                    └── Over Cap: Send Trailers-Only RESOURCE_EXHAUSTED
       │                                  (NO task spawned)
       │
       └── Over Cap: Drop Socket immediately (NO connection task spawned)
```

### 7.1 Async Task Lifecycle Invariants

1. **Connection Tasks:**
   - Exactly $N_{\text{conn}}$ tasks are spawned on the Tokio runtime for active connections (`serve_io`).
   - When a connection drops or is closed via `GOAWAY`, the connection task completes and drops its `OwnedSemaphorePermit`.
2. **RPC Dispatch Tasks:**
   - Exactly $N_{\text{rpc}}$ tasks are spawned for admitted RPCs (`dispatch.dispatch(...)`).
   - Rejected RPCs (due to `max_concurrent_rpcs` exhaustion) execute inline on the `serve_io` connection loop with zero async task spawning.
   - The permit `_permit: Option<OwnedSemaphorePermit>` is moved into the spawned dispatch task:
     ```rust
     drop(tokio::spawn(async move {
         let _lease = lease;
         let _permit = permit;
         dispatch.dispatch(...).await;
     }));
     ```
   - When the handler future completes, `_permit` is dropped, immediately returning the permit to the shared process-wide semaphore.
3. **Quiescent Draining:**
   - During server shutdown (`drain_tx` / `drain_rx`), the accept loop stops accepting new sockets, closes the listener, and signals connection tasks.
   - In-flight tasks drain up to `max_connection_age_grace` before socket termination, ensuring no lingering tasks or permits leak.

### 7.2 Bounded Drain & Cancellation Cleanup Model (RT-08)

To guarantee that no detached task, stalled peer, unending stream, or slow upload can indefinitely extend server termination, `pbrs-grpc` enforces a deterministic three-phase bounded drain and immediate cancellation cleanup contract:

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│                           Server Shutdown Sequence                               │
├──────────────────────────────────────────────────────────────────────────────────┤
│ Phase 1: Admission Cutoff                                                        │
│ • Shutdown future resolves -> accept loop terminates immediately.                │
│ • Listener is closed; new connection attempts are refused at OS level.           │
├──────────────────────────────────────────────────────────────────────────────────┤
│ Phase 2: GOAWAY & Graceful In-Flight Processing                                  │
│ • Broadcast goaway signal to all live connection tasks (serve_io).               │
│ • Connection driver issues HTTP/2 GOAWAY(2^31 - 1) to refuse new streams.        │
│ • In-flight RPCs continue executing up to grace (max_connection_age_grace).      │
│ • Pending TLS / HTTP/2 handshakes abort immediately via wait_for_drain select.  │
├──────────────────────────────────────────────────────────────────────────────────┤
│ Phase 3: Bounded Force-Close & Quiescence                                        │
│ • If streams finish before grace: connection closes cleanly and drops drain ref. │
│ • If streams stall, block, or never end: force_close timer (now + grace) fires.  │
│ • Socket is dropped; in-flight tasks terminate; permits and buffers quiesce.     │
└──────────────────────────────────────────────────────────────────────────────────┘
```

#### 1. Grace Policy & Drain Timeouts
- **Default Grace Period:** `DEFAULT_MAX_CONNECTION_AGE_GRACE = 10s`, configurable via `Server::max_connection_age_grace(Duration)`.
- **Drain Upper Bound:** Total termination time for established connections is strictly bounded by:
  $$T_{\text{drain}} \le \text{max\_connection\_age\_grace}$$
- **Handshake Stall Bound:** Sockets stalled during TLS accept or HTTP/2 preface handshake select directly on `wait_for_drain(goaway.clone())` and abort immediately upon shutdown initiation. Without shutdown, handshakes are bounded by `handshake_timeout` (default 20s). Stalled handshakes can never postpone process shutdown.
- **Deadline Overlay on Streams:** Unending streams governed by deadlines (`Server::timeout` or client `grpc-timeout`) are aborted with `Status::deadline_exceeded` upon deadline expiration, freeing stream resources prior to or during drain.

#### 2. Cancellation Cleanup & Immediate Permit Reclaim
- **Client Call Future Drop:** Dropping a client `Call<T>` future sends an HTTP/2 `RST_STREAM(CANCEL)` on the wire and drops the held `OwnedSemaphorePermit`. Client concurrency slot count returns to 0 immediately ($O(1)$ permit reclaim), allowing subsequent RPCs to acquire slots without delay.
- **Server Stream Reset Handling:** When a client RST or disconnect is received:
  - Inbound stream readers (`WireStream`) yield `Status::cancelled`.
  - Outbound stream senders (`drain_to_wire`) abort on `send.poll_reset`, terminating the dispatch task.
  - The server's `_permit` and `_lease` are dropped immediately on task exit, releasing concurrency slots and decrementing byte budget allocation to 0.
- **Zero Background Task Leaks:** All auxiliary tasks (ping-pong drivers, response producers, frame writers) monitor stream cancellation channels (`watch::Receiver<bool>`) or select on `poll_reset`, guaranteeing zero detached task leaks.

#### 3. Measurable Overload, Fairness, and Cleanup Bounds (RT-06/RT-07/RT-08 Contract)

The table below is the pass/fail contract enforced in `pbrs-grpc/tests/resource_bounds.rs`. Every bound names the side(s) it constrains; "quiescent" always means `byte_budget_allocated() == 0` **and** a probe RPC is not rejected with `RESOURCE_EXHAUSTED` (permits released). RSS is recorded (start/peak per run) for qualification review, not threshold-gated in PR runs.

| # | Bound | Value | Side | Enforced by |
|---|---|---|---|---|
| F-1 | Competing small RPCs under bulk-stream load: success rate | 100% (no starvation) | Server | `test_competing_small_rpcs_progress_under_bulk_stream_load` |
| F-2 | Same workload: admitted-call p99 latency | < 500 ms | Server | `test_competing_small_rpcs_progress_under_bulk_stream_load` |
| F-3 | Same workload: max scheduling queue delay | < 100 ms | Server | `test_competing_small_rpcs_progress_under_bulk_stream_load` |
| F-4 | Fast-client p99 with a slow reader or slow writer peer | < 200 ms | Server | `test_slow_reader_peer_isolation`, `test_slow_writer_peer_isolation` |
| F-5 | Legitimate-call p99 during an RST storm | < 300 ms | Server | `test_reset_storm_isolation_and_competing_progress` |
| F-6 | Idle-peer contention: active-call p99 | < 200 ms | Server | `test_idle_peers_contention_and_fairness` |
| O-1 | Overload: every offered call gets an explicit outcome (success or `RESOURCE_EXHAUSTED` citing the violated limit); no silent buffering, no silent drop | 100% explicit | Client + Server | `test_overload_explicit_status_rejection_no_silent_buffering` |
| O-2 | Overload: admitted-call p99 latency | < 500 ms | Server | `test_overload_explicit_status_rejection_no_silent_buffering` |
| O-3 | Transport byte budget under mixed large/small/compressed load: explicit outcomes only (success or `RESOURCE_EXHAUSTED` citing the budget); held allocations block admission, release re-admits (hard ceiling additionally unit-covered in `limits.rs`) | 100% explicit, block/release | Client + Server | `test_mixed_large_small_compressed_byte_budget` |
| C-1 | Cancellation, encode error, or peer reset returns byte accounting to baseline | `allocated() == 0` | Client + Server | `test_client_cancellation_releases_budget`, `test_encode_error_releases_budget_to_baseline`, `test_reset_storm_isolation_and_competing_progress` |
| C-2 | Dropping `Call` futures across all four shapes releases concurrency permits immediately (next calls succeed) and quiesces buffers | probe succeeds, `allocated() == 0` | Client + Server | `test_cancellation_cleanup_drops_call_futures_and_quiesces` |
| C-3 | Graceful drain with unending streams or blocked uploads terminates within the grace policy and quiesces | `T_drain <= grace` (test window), `allocated() == 0` | Server | `test_graceful_shutdown_bounded_drain_unending_client_stream`, `test_graceful_shutdown_blocked_upload_stream` |
| C-4 | Stalled handshakes never extend termination | drain completes promptly, `allocated() == 0` | Server | `test_graceful_shutdown_handshake_stall_terminates_promptly` |
| C-5 | Deadline-expired unending streams abort, quiesce, and drain fast | `DeadlineExceeded`, `allocated() == 0` | Server | `test_unending_stream_deadline_expiration_and_drain` |

Changing a value above requires a recorded decision **before** rerunning; a test edit that merely relaxes a bound to fit slower code is a contract change, not a fix.

---

## 8. Verification & Links to Downstream Implementation

This specification directly guides implementation and verification in the Reliability (RT) lane:

1. **RT-06 ("Enforce the approved transport byte budget"):**
   - Implements the shared byte-admission and accounting mechanism in `pbrs-grpc/src/limits.rs`, `client.rs`, and `server.rs`.
   - Adopts the error and rejection semantics specified in Section 5 (immediate TCP drop, Trailers-Only `RESOURCE_EXHAUSTED` with `end_stream=true`, zero payload buffering).
   - Validates memory bounds under synthetic hostile tests in `pbrs-grpc/tests/resource_bounds.rs`.
2. **RT-07 ("Verify overload fairness and slow-peer isolation"):**
   - Uses the bounded production profiles defined in Section 6 to benchmark mixed workloads (bulk streams mixed with small unary RPCs, slow readers/writers, and reset storms).
   - Proves that admitted competing calls progress fairly within the budgeted queue delay, without starvation or unbounded RSS growth.
3. **RT-08 ("Prove bounded drain and cancellation cleanup"):**
   - Proves that graceful shutdown bounded by `max_connection_age_grace` terminates cleanly under unending streams, stalled client uploads, and blocked peers without indefinite postponement.
   - Proves that handshake stalls (plain TCP and TLS) do not prolong server drain.
   - Proves that dropping `Call<T>` futures across all 4 call shapes immediately releases concurrency permits and returns byte budget allocations to zero (`is_byte_budget_quiescent() == true`).
   - Validates that expired deadlines on unending streams cleanly abort in-flight handlers and enable immediate shutdown quiescence. Tested in `pbrs-grpc/tests/lifecycle.rs` and `pbrs-grpc/tests/resource_bounds.rs`.

---

## 9. Appendix: Mathematical Notation Reference

| Symbol | Definition | Unit / Type | Default Value |
|---|---|---|---|
| $N_{\text{conn}}$ | Number of concurrent active connections | Integer | Unbounded (`None`) |
| $N_{\text{rpc}}$ | Number of concurrent active RPCs | Integer | Unbounded (`None`) |
| $W_{\text{conn}}$ | HTTP/2 connection-level flow control window | Bytes | 16 MiB ($16,777,216$) |
| $W_{\text{stream}}$ | HTTP/2 stream-level flow control window | Bytes | 16 MiB ($16,777,216$) |
| $S_{\text{send\_buf}}$ | Connection send buffer cap | Bytes | 1 MiB ($1,048,576$) |
| $H_{\text{table}}$ | HPACK dynamic table size | Octets | 4,096 |
| $H_{\text{list}}$ | Maximum uncompressed header list size | Octets | 16,384 |
| $M_{\text{decode\_cap}}$ | Maximum inbound message decoding size | Bytes | 4 MiB ($4,194,304$) |
| $M_{\text{encode\_cap}}$ | Maximum outbound message encoding size | Bytes | Unbounded (`None`) |
| $S_{\text{replay\_buf}}$ | Transparent retry replay buffer size | Bytes | Request payload length |
| $B_{\text{stream\_queue}}$ | Streaming channel queue depth | Messages | 16 |
