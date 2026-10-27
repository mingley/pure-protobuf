# gRPC resource budgets

Set explicit connection, RPC, message, and transport-byte limits before
deploying a service. This page explains what each limit controls, what remains
application-owned, and how to validate overload and cleanup. Transport limits
do not establish an exact process-memory ceiling.

Related: [retries](retry-contract.md), [architecture](architecture.md),
[gRPC](grpc.md). The implementation is in `pbrs-grpc/src/{config,limits,stream,wire,gzip,client,server}.rs`.
The wire rules follow RFC 9113, RFC 7541, the gRPC HTTP/2 specification and gRFC A6.

## 1. Default limits

Traffic bursts, slow peers and large messages can exhaust memory or task
capacity. Some defaults are bounded; global connection and RPC counts require
explicit limits:

| Fact | Why it matters |
|---|---|
| Inbound message size and per-connection stream defaults are bounded. | The default decoding cap is 4 MiB and the stream cap is 256 per connection. Outbound encoding has no finite default cap. |
| Global connection and active-RPC counts are unbounded by default. | `max_concurrent_connections` and `max_concurrent_rpcs` default to `None`, so operators must choose production ceilings. |
| High-throughput buffers multiply under overload. | 16 MiB connection and stream windows, 1 MiB frames, and 1 MiB send buffers can become gigabytes without explicit connection and call limits. |

---

## 2. Buffer ownership

Budget transport buffers and application data separately. Each has a different
owner, limit and release point.

```
┌───────────────────────────────────────────────────────────────────────────────────┐
│                   Service memory (process plus kernel buffers)                    │
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

### 2.1 Buffers and limits

| Buffer Type | Owner | Typical Size Range | Controlled By / Bounded In | Reclaim Lifecycle |
|---|---|---|---|---|
| **TCP Socket Buffers (`SO_RCVBUF`, `SO_SNDBUF`)** | OS Kernel | 64 KiB – 4 MiB per socket | OS sysctl / TCP autotuning (`tcp_keepalive`, TCP window) | Connection termination |
| **TLS Session Context** | Transport (`rustls`) | 4 KiB – 32 KiB per socket | `ServerTls` / `ClientTls`, cipher suite state | Connection termination |
| **HTTP/2 Connection Driver** | Transport (`h2::server::Connection` / client driver) | 8 KiB – 32 KiB per conn | Tokio task allocation, stream table trees | Connection termination |
| **HPACK Dynamic Tables** | Transport (`h2`) | $2 \times 4096$ octets default | `ServerConfig::header_table_size` (`DEFAULT_HEADER_TABLE_SIZE`) | Connection termination |
| **Stream Send Buffer** | Transport (`h2`) | 1 MiB capacity threshold per stream | `ServerConfig::max_send_buffer_size` (`DEFAULT_MAX_SEND_BUFFER_SIZE`) | Capacity returns as bytes are written to the connection |
| **Small-DATA Framing Budget** | Transport (`h2`) | 25,600 bytes per conn | `ServerConfig::data_frame_budget` (`DEFAULT_DATA_FRAME_BUDGET`) | Drained on write ACK |
| **Reset Stream Tracking** | Transport (`h2`) | ~1 KiB – 10 KiB per conn | `max_concurrent_reset_streams`, `max_local_error_reset_streams` | Purged after `reset_stream_duration` (1s) |
| **Uncompressed Header Block** | Transport (HPACK decoder) | Up to 16 KiB per RPC | `ServerConfig::max_header_list_size` (`DEFAULT_MAX_HEADER_LIST_SIZE`) | Freed after header parse |
| **Inbound Frame Carry Buffer** | Transport (`wire::FrameReader`) | Up to 4 MiB (chunk-straddling) | `MessageLimits::max_decoding` (`DEFAULT_MAX_DECODING_MESSAGE_SIZE`) | Freed on frame yield |
| **Decompression Expansion Buffer** | Transport (`compression::read_bounded`) | Output reservation up to the decoding cap, plus an 8 KiB stack chunk and decoder state | `MessageLimits::inflate_budget` (`limits.max_decoding()`) | Output may be retained by the decoded message |
| **Idle Decoded Buffer Cache** | Transport, shared across the process | At most 4 MiB per inbound stream and 16 MiB in total | Fixed internal capacity policy, separate from transport-byte permits | Reused by the next compressed message; released on observed EOF, error, cancellation, deadline or stream drop |
| **Zstd History Window** | Optional Zstd decoder | 8 MiB default logical window cap, plus capacity/block/decoder overhead | `MessageLimits::with_max_zstd_window`; independent of output and frame-count caps | Scratch is reused within a message and released afterward |
| **Compressed Member/Frame Setup** | Compression decoder | At most 1,024 admissions per message by default | `MessageLimits::with_max_compression_frames`; includes empty gzip members and Zstd skippable frames | Count resets for each gRPC message |
| **Outbound Frame Serialization** | Transport (`wire::frame_from_msg`) | Serialized payload length | `MessageLimits::max_encoding` / application message size | Dropped after wire enqueue |
| **Compressed Streaming Scratch** | Transport (`wire::OutBatch`) | Up to 64 KiB per outbound stream; append-capable codecs can retain larger buffers, at most 4 MiB each and 16 MiB process-wide | Independent encode-cache capacity ledger; finite encoding limits govern active storage; outside byte-budget permits | Stream destruction, encode/compression failure or panic; oversize/full-cache buffers are released after encoding |
| **Unconfigured Transparent Retry Replay Buffer** | Client unary/server-streaming executor | One segmented request frame, with no finite default outbound cap | `encode_msg` and existing outbound message limits | Held across a proven-unprocessed replacement; dropped on terminal outcome |
| **Policy Retry Replay Buffer** | Client unary/server-streaming executor | Encoded payload up to the selected finite outbound cap, otherwise 4 MiB, plus five-byte framing | [Service-config §6.3](service-config.md#63-replay-byte-budget-rt-06); shared backing allocations and segment metadata are separate costs | Over-budget calls transfer ownership to the first upload and commit; eligible buffers survive backoff and release on terminal status |
| **Streaming Channel Queue** | Transport / Application (`mpsc::channel`) | `buffer` $\times$ message size | `Streaming::channel(buffer)`, `ChannelConfig::stream_buffer` | Dropped as consumer reads |
| **Deserialized Message Struct** | Application (`T: Parse + Default`) | Domain dependent | Rust allocator / struct lifetime | Dropped by application handler |
| **Retained Backing Buffers** | Application (`bytes::Bytes`) | Original chunk size | Application holding sub-slice of transport `Bytes` | Dropped when last `Bytes` clone drops |

Compressed streams can retain decoded storage only after successful decoding,
when the codec has consumed the input and its backing allocation has one owner.
The Prost adapter supports this for owned fields. Messages that retain shared
`Bytes`, including the default pbrs decoding path, keep their existing ownership
behavior. Unary and uncompressed decoding do not populate this cache.
Cold output up to the decoder's 8 KiB lookahead uses the ordinary codec path;
an existing workspace can still serve later small or empty messages.

Cache admission charges the allocation's full capacity. A full cache or a buffer
above 4 MiB simply disables retention for that message. These limits cover idle
storage between messages; active decoding, codec state and application messages
remain separate costs. Reusing a larger allocation does not raise the next
message's decoded-byte limit. Cancellation through a separate call handle
releases cached storage when the reader observes it or the stream is dropped.

Large encode scratch uses a separate 16 MiB ledger from decoded storage. Only
append-capable codecs enter this cache: Prost and field-wise pbrs messages opt
in through `CodecMessage::ENCODE_APPEND_ONLY`. The codec receives a generic
`BufMut`, which cannot replace, split or retain the transport's allocation.
Custom codecs default to the existing contiguous path, without an additional
temporary buffer or copy. Admission charges full `Vec` capacity after both
serialization and compression succeed; active encoding is uncharged. An error
or unwind discards the active allocation. Cache exhaustion never fails an RPC.
The small 64 KiB allowance, active output, and compression state remain separate
costs; these two cache ledgers are not a process memory cap.

Compressed output starts with at most 4 KiB of payload lookahead. If output
approaches half the known plaintext size, growth anticipates the full size plus
headroom, avoiding an extra doubling for incompressible data's footer. Smaller
output grows normally, so a highly compressed multi-MiB message does not trigger
a plaintext-sized reservation. The growth target is a capacity hint; larger
output remains valid and still grows under the existing message/framing rules.

Outbound gRPC messages can be larger than the configured HTTP/2 send buffer or
the peer's stream window. `wire::send_bytes` queues small frames directly when
the buffer accepts them; otherwise it slices the encoded frame into DATA chunks
no larger than the positive send-buffer limit and currently granted credit.
Only the final chunk carries `END_STREAM` when the caller requests it. The
writer never waits for credit equal to the entire message, which could
otherwise stall behind a smaller buffer. The serialized message itself remains
live while chunks are queued and is separately governed by the outbound
encoding cap; the send-buffer limit alone is not a total-message memory cap.
When a server response producer queues valid messages followed by an error in
the same burst, complete encoded frames are flushed before the non-OK trailers.
A failed encoding rolls back its incomplete frame before earlier replies are
flushed, and the batch's byte permits are released after the flush. A peer
reset can prevent both the flush and trailers from being delivered.

The local `pbrs_grpc::InteropTestService` sample also bounds generated
`Payload.body` at [`DEFAULT_MAX_DECODING_MESSAGE_SIZE`](../pbrs-grpc/src/limits.rs)
(4 MiB), rejecting negative requested sizes with `INVALID_ARGUMENT` and
larger bodies with `RESOURCE_EXHAUSTED` before allocation. This is a
**test-service body-byte policy**, not the transport's serialized-message
limit or an official peer requirement: a `SimpleResponse` or streaming reply
adds protobuf envelope bytes, and the gRPC frame adds five more. A client
requesting the entire body cap must raise its own inbound decoded-message
limit; the official 314,159-byte cases fit the default. `StreamingInputCall`
checks its i32 aggregate instead of saturating. A later invalid output size
becomes non-OK trailers after already queued valid replies, unless a peer
reset prevents delivery. This sample is separate from the benchmark worker
and does not qualify the original upstream negative-case runner.

The public `pbrs_grpc::ByteBudgetTracker` can share an explicit transport-byte
cap through `Server::with_byte_budget_tracker` or
`Channel::with_byte_budget_tracker`. Acquired `BytePermit`s return their bytes
on drop. `allocated()` / `is_quiescent()` expose the current state, and
`peak_allocated()` records the lifetime high-water mark of accounted permits
across clones (including warmup), exact after in-flight acquisitions finish.
Counter overflow rejects explicitly; neither value measures application
allocations or process RSS.
`active_byte_permit_tokens()` separately counts live `BytePermit` handles,
including successful zero-byte acquisitions; `BytePermit::empty()` has no
tracker and is not counted. `peak_active_byte_permit_tokens()` records their
lifetime high-water across tracker clones, exact after acquisitions finish.
Merging two handles on the same tracker reduces the token count by one
without returning bytes. `forget()` removes a live token but deliberately
leaves its bytes charged. Count overflow rejects with `RESOURCE_EXHAUSTED`
and rolls back any newly reserved bytes; the byte admission limit is still
decided solely by allocated bytes. `is_quiescent()` remains a **bytes-only**
check and can be true while a zero-byte token is live. The byte and token
counters are distinct atomics, so concurrent snapshots can briefly observe
different acquisition/release phases. Neither token count is an RPC
semaphore-slot gauge, an RSS measurement, or an interval-specific peak.

### 2.2 The Backing Buffer Retention Hazard

Retaining a small view can keep a large allocation alive:

- Network payloads arrive as reference-counted `bytes::Bytes` chunks.
- When a frame fits in one chunk, `FrameReader` uses `chunk.split_to(frame_len)`
  to share its backing allocation in $O(1)$ time.
- Keeping a shallow copy in a long-lived cache can retain the entire allocation
  (often 16–64 KiB), even for a 16-byte field.
- **For handlers:** Copy a small retained field into independent storage when
  keeping the larger backing allocation would exceed the cache's budget.
  Sharing `Bytes` across threads is supported; crossing a thread boundary does
  not itself require a copy. Choose ownership from lifetime and retained size.

---

## 3. A practical memory model

Treat memory as several separate budgets. A limit on one category does not
bound the others, and a flow-control window is credit rather than an allocation
made when a connection opens.

| Budget | What to count | How to bound or measure it |
|---|---|---|
| Connections | TLS state, HTTP/2 state, HPACK tables, timers, reset tracking, and connection tasks | Set `max_concurrent_connections`; measure memory per connection with the shipping TLS and allocator profile. |
| HTTP/2 streams | Header maps, stream state, queued outbound DATA, and reset/cancellation state | Set `max_concurrent_streams`; multiply per-stream costs by admitted streams across all connections. |
| Active RPCs | Handler/client futures, decoded messages, encoded frames, replay buffers, and stream queues | Set `max_concurrent_rpcs`, finite message limits, and bounded application queues. |
| Accounted transport bytes | Allocations carrying `ByteBudgetTracker` permits | Share a tracker; inspect current and peak allocations after acquisitions settle. Codec scratch, decoder state and allocator overhead are separate costs. |
| Application and allocator memory | Retained messages, shared backing allocations, handler work, caches, task locals, fragmentation | Application budgets plus measured process RSS under sustained mixed traffic. |
| Kernel socket memory | TCP receive/send buffers and connection bookkeeping | Measure separately with platform tools; do not equate it with process RSS. |

### Connection and stream accounting

The current `h2` backend's `max_send_buffer_size` controls send capacity
**per HTTP/2 stream**, not per connection. A connection with many active streams
can therefore queue more than this value in total. It is a backpressure
threshold, not an exact allocation ceiling: a complete serialized message can
remain alive outside the queued chunks, and shared `Bytes` slices can retain
larger allocations. Count serialization and retained backing storage separately.

For an initial estimate, keep these terms distinct:

```text
process memory ≈ base runtime and allocator overhead
               + connection count × measured connection state
               + open stream count × measured stream state
               + live transport buffers and replay buffers
               + idle decode/encode caches and per-stream small scratch
               + application messages, queues, caches, and handler state
               + allocator retention and safety margin
```

Do not count the same shared backing allocation twice merely because two
`Bytes` handles reference it. Conversely, a small slice can retain an entire
backing allocation. The byte-permit gauge measures what the transport accounts
for; it is not an RSS meter.

The connection receive window limits outstanding, unconsumed HTTP/2 DATA
credit across streams. Once the transport releases that credit, decoded or
application-retained data can still occupy memory while the peer sends more.
Adding one connection window per connection is therefore not a proof of the
total memory bound.

### Independent send buffers and byte budgets

`Server`, `Router`, and `Channel` keep the HTTP/2 send buffer and the aggregate
byte budget separate. `.config(...)` and `.max_send_buffer_size(...)` preserve
an attached tracker, including its shared identity and outstanding permits.
A nondefault send buffer does not create an aggregate budget. Set an aggregate
limit explicitly with `.byte_budget(...)` or `.with_byte_budget_tracker(...)`.

Earlier versions derived an aggregate budget from a nondefault send buffer and
could replace an attached tracker. Code relying on that implicit limit must now
set it explicitly. For example, use `.max_send_buffer_size(4096).byte_budget(131072)`
to allow whole messages larger than the per-stream send buffer while limiting
accounted transport bytes. This byte budget is not a process-memory limit.

[`current-h2-soak.py`](../scripts/current-h2-soak.py) freezes a clean commit,
finite Linux process limits and a bounded workload before executing its test
binary. It records RSS and process high-water separately from exact accounted
transport bytes, byte-permit tokens and observed streaming-call counts. The diagnostic
also samples OS threads, file descriptors and Tokio alive tasks through warmup,
slow readers, overload, cancellation, deadline recovery and drain. See the
[commands, recovery tolerances and limitations](evidence/current-h2-soak.md).
Its result always remains `qualified: false`; QG-06's complete 24-hour campaign
is a separate acceptance requirement.

Current runtime `9d6fb9c4` passes 731 leak-detecting sanitizer tests with one
existing ignore. The clean `deb0d1d6` all-feature workspace passes 3,755 tests,
zero failures and nine existing ignores; only its serving fixture differs from
the runtime. Original failed runs remain recorded in the
[execution follow-up](evidence/grpc-architecture-20261022/README.md).
Twenty-four supervisor offline controls pass. The actual `9d6fb9c4` preview passes
**30.54 seconds and nine drains**, all with client/server byte charges, tokens and
active calls at zero. Each drain records seven file descriptors and one shared
task. Sampled RSS peaks at **18.47 MiB**; the separately observed process memory
high-water mark is **20.05 MiB**. Sampled file descriptors peak at nine.
The preview retains the original limits: 1 GiB address space, 128 descriptors,
4,096 same-UID processes and 60 CPU seconds. The actual 24-hour run started at
02:52 UTC on 2026-10-23; its result is pending. It uses the same memory, descriptor
and process limits, with an 86,430-second CPU allowance. Neither offline
controls nor a short preview proves a full day, total RSS bounds or fairness
under all loads.

### Message and queue accounting

For each simultaneously active RPC, allow for the following lifetimes:

- A message split across DATA chunks can require a carry buffer up to the
  decoding limit.
- Compressed input can keep encoded bytes alive while allocating decompressed
  bytes up to the inflate limit.
- Outbound encoding can keep a whole message frame alive while flow control
  permits only smaller chunks to be queued.
- Unary and server-streaming retries can retain the encoded request across
  attempts. Configured retry policies use the finite payload retention budget
  in [service-config §6.3](service-config.md#63-replay-byte-budget-rt-06).
  Over-budget requests keep their valid first send but commit against replay;
  their upload/logging frame owners release before waiting for a response.
  Unconfigured transparent retry and hedging have no added default retention
  cap. Retry and hedging limits determine how many attempt states coexist.
- A queue bounded to 16 items is not bounded to 16 bytes: count the retained
  size of every item, including any shared backing allocation.
- Handler futures and decoded messages belong to the application budget even
  when the transport has released its permits.

The default inbound decoding cap is 4 MiB. Outbound encoding has no finite
default cap; set `MessageLimits::with_max_encoding` when a deployment needs one.
An encoded protobuf message also includes field tags and lengths, and the gRPC
frame adds five bytes. Budget for the actual encoded size, not only the largest
`bytes` field. A policy replay budget measures the transmitted payload (compressed
when applicable); it does not bound allocator capacity, the full backing
allocation of a shared slice, or memory owned by a binary logging sink.

### Adaptive receive windows

`ServerConfig::adaptive_window(true)` and
`ChannelConfig::adaptive_window(true)` are opt-in. They start at the configured
adaptive initial window and grow stream and connection windows up to
`adaptive_window_max_size` (16 MiB by default). Use that cap when modeling
outstanding receive credit. Fixed 16 MiB windows remain the default.

## 4. Choose and validate deployment limits

The defaults cap HTTP/2 streams at 256 per connection and decoded messages at
4 MiB, but leave total connection and RPC counts uncapped. They are starting
settings, not a production capacity guarantee.

For example:

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(500)
    .max_concurrent_rpcs(1000);
```

Those settings cap connections admitted by that server's accept loop and the
concurrent handlers sharing its admission state. They do not cap unrelated
servers in the same process, messages retained by application code, or allocator
memory. The values above illustrate the API; select actual values from your
service's memory and latency budget.

To size a deployment:

1. Set connection, RPC, message, transport-byte, and application-queue limits.
2. Measure peak RSS and accounted bytes under representative TLS, compression,
   streaming, retries, and slow-reader traffic.
3. Exercise overload, cancellation, connection churn, and shutdown. Verify
   explicit rejection and recovery after load falls.
4. Add headroom for allocator retention and application growth before setting
   the container memory limit. Repeat when dependencies or workload shape change.

The [resource-bound tests](../pbrs-grpc/tests/resource_bounds.rs) enforce the
specific fairness and cleanup requirements in §7.2. They do not prove a universal
RSS ceiling. Sustained current-backend soak evidence is still required by the
[roadmap](ROADMAP.md).

---

## 5. Admission Control & Error Semantics (Contract for RT-06)

The RT-06 admission contract requires checks at distinct lifecycle boundaries.
Reject before allocating the memory guarded by a limit. The implementation
and tests below are the current reference for those checks.

```
Incoming Connection (TCP Accept)
       │
       ▼
[ Connection Semaphore? ] ───(Over Limit)───► Close the newly accepted socket
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
       │                                            - Handler does not read the body
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
       └── Yes ─► Check grpc-encoding, then bounded decompression
                       │
                       └──(Exceeds Budget)──► Immediate RESOURCE_EXHAUSTED
                                               (Output reservation ≤ budget)
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
  - **Server:** Connection worker loop in `pbrs-grpc/src/server/connection.rs` (`serve_io`) upon receiving an incoming stream via `conn.poll_accept()`, **before** reading request DATA frames and admitting the handler future.
  - **Client:** Client call entry in `pbrs-grpc/src/client.rs` (`take_rpc_slot()`), **before** acquiring a channel connection slot and before writing request HEADERS.
- **Server Rejection Behavior:**
  - The server immediately responds with an HTTP/2 **Trailers-Only** response (`wire::reject`):
    - `:status = 200`
    - `content-type = application/grpc`
    - `grpc-status = 8` (`Code::ResourceExhausted`)
    - `grpc-message = "too many concurrent RPCs"`
    - `end_stream = true` set directly on the initial `HEADERS` frame.
  - **Reject before reading the request body:**
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

### 5.4 Decompression output limit

`compression::read_bounded` limits gzip, deflate and optional Zstd output to
`limits.inflate_budget()`. It starts with an 8 KiB stack chunk, then reads into
initialized output storage. At a capacity boundary, bounded stack lookahead
measures a short final tail before reserving more heap capacity. Full chunks
keep geometric growth. At the configured limit, a one-byte probe distinguishes
EOF from excess output; an over-limit byte is never appended. Short reads are
not treated as EOF, and later checksums still have to validate.

Output grows after the decoder produces bytes. The buffered input length is
an initial sizing hint capped at 256 KiB. Reservations request no more than
the decoding cap, without trusting a gzip length trailer. Small inputs do not
reserve the full configured maximum.

This bounds decoded output, not total process memory. Include the compressed
input, decoder state, stack chunk, allocator overhead and retained message
backing when sizing a service. Configure finite connection and RPC limits
alongside message limits, and verify recovery under sustained load.

With the optional `zstd` feature, the history window has its own 8 MiB default
cap. A larger advertised window returns `RESOURCE_EXHAUSTED` before its history
buffer is allocated. Set `MessageLimits::with_max_zstd_window` explicitly when
accepting larger windows. `MessageLimits::unlimited()` retains the backend's
100 MiB window cap; lifting the decoded message limit alone leaves the window
cap unchanged. Each standard Zstd frame receives window, content-checksum and
declared-length checks. All legal gzip members or Zstd frames in one gRPC
payload contribute to the same decoded message; deflate rejects bytes after
its single zlib stream.

The default also permits at most **1,024 compressed members/frames per gRPC
message**. Set `MessageLimits::with_max_compression_frames` to choose a lower or
higher count. Gzip empty members and Zstd skippable frames count; deflate uses
one admission. The count is checked before another decoder reset and excess
returns `RESOURCE_EXHAUSTED`. This bounds setup work independently of output
bytes, but does not preempt synchronous decoding inside one admitted frame.
`with_unlimited_decoding` preserves the count; `with_unlimited_compression_frames`
lifts it explicitly, and `unlimited()` removes both byte and count caps.

The tonic adapter validates the complete gzip payload before emitting normalized
uncompressed message views. During that work, encoded views and decoded output
can coexist. Include both, decoder state and the generated message's allocations
in memory measurements; the message cap alone is not a process-memory bound.

### 5.5 Summary Contract Matrix for RT-06

| Limit Exceeded | Detection Point | Incurred Allocation | Network Action | gRPC Status Code |
|---|---|---|---|---|
| **Connection Cap** | `listener.accept()` | 0 bytes | Drop TCP socket immediately | Socket RST / Drop |
| **Server RPC Cap** | `conn.poll_accept()` | 0 bytes body | Send Trailers-Only HEADERS (`end_stream=true`) | `RESOURCE_EXHAUSTED` (8) |
| **Client RPC Cap** | `Call::poll()` / invoke | 0 bytes | Fail client future locally, no wire frames | `RESOURCE_EXHAUSTED` (8) |
| **Inbound Msg Size** | 5-byte frame header | 0 bytes payload | Send Trailers-Only response or stream reset | `RESOURCE_EXHAUSTED` (8) |
| **Outbound Msg Size** | `T::serialized_len()` | 0 bytes frame | Abort serialization, return error | `RESOURCE_EXHAUSTED` (8) |
| **Decompression Overflow** | Decompression loop | Output reservation up to `budget`; decoder and stack costs are separate | Abort inflate, send Trailers-Only error | `RESOURCE_EXHAUSTED` (8) |
| **Compressed Member/Frame Count** | Before the next decoder reset | Already-admitted decoder/input/output state may remain until cleanup | Reject the message | `RESOURCE_EXHAUSTED` (8) |

---

## 6. Example tuning profiles

These are starting points for load testing, not measured deployment presets.
None implies a container RAM recommendation. All snippets assume
`use pbrs_grpc::ServerConfig;` and `use std::time::Duration;`.

| Limit | Edge ingress | Internal service | Bulk transfer | Small footprint |
|---|---:|---:|---:|---:|
| Connections | 5,000 | 200 | 20 | 10 |
| Active RPCs | 10,000 | 2,000 | 100 | 50 |
| HTTP/2 streams per connection | 32 | 256 | 64 | 16 |
| Connection receive window | 256 KiB | 4 MiB | 16 MiB | 64 KiB |
| Stream receive window | 64 KiB | 1 MiB | 16 MiB | 32 KiB |
| Send capacity per stream | 128 KiB | 1 MiB | 4 MiB | 32 KiB |
| Inbound and outbound message cap | 1 MiB | 4 MiB | 32 MiB | 256 KiB |
| Header-list cap | 8 KiB | 16 KiB | 16 KiB | 4 KiB |

### 6.1 Edge ingress

Smaller messages and windows limit each peer's immediate transport demand.
Validate connection churn, handshake timeouts, slow readers, and overload.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(5_000)
    .max_concurrent_rpcs(10_000)
    .max_concurrent_streams(32)
    .initial_connection_window_size(256 * 1024)
    .initial_stream_window_size(64 * 1024)
    .max_send_buffer_size(128 * 1024)
    .max_header_list_size(8 * 1024)
    .max_decoding_message_size(1024 * 1024)
    .max_encoding_message_size(1024 * 1024)
    .connect_timeout(Duration::from_secs(5))
    .max_connection_idle(Duration::from_secs(60))
    .max_connection_age(Duration::from_secs(600));
```

### 6.2 Internal service

Tune pool size and concurrency against the latency budget. Coordinate keepalive
with peers and load balancers to avoid needless connection churn.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(200)
    .max_concurrent_rpcs(2_000)
    .max_concurrent_streams(256)
    .initial_connection_window_size(4 * 1024 * 1024)
    .initial_stream_window_size(1024 * 1024)
    .max_send_buffer_size(1024 * 1024)
    .max_header_list_size(16 * 1024)
    .max_decoding_message_size(4 * 1024 * 1024)
    .max_encoding_message_size(4 * 1024 * 1024)
    .keep_alive_interval(Duration::from_secs(30))
    .keep_alive_timeout(Duration::from_secs(10));
```

### 6.3 Bulk transfer

Larger messages need a smaller concurrency budget. Test mixed small and large
RPCs so throughput tuning does not starve latency-sensitive calls.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(20)
    .max_concurrent_rpcs(100)
    .max_concurrent_streams(64)
    .initial_connection_window_size(16 * 1024 * 1024)
    .initial_stream_window_size(16 * 1024 * 1024)
    .max_send_buffer_size(4 * 1024 * 1024)
    .max_decoding_message_size(32 * 1024 * 1024)
    .max_encoding_message_size(32 * 1024 * 1024);
```

Bound response queues with `Streaming::channel(capacity)`; choose capacity
from message size and retained memory. Client request queues use
`ChannelConfig::stream_buffer(capacity)`.

### 6.4 Small footprint

Small windows and low concurrency reduce transport demand. Measure allocator
retention and application memory before choosing a process memory limit.

```rust
let config = ServerConfig::new()
    .max_concurrent_connections(10)
    .max_concurrent_rpcs(50)
    .max_concurrent_streams(16)
    .initial_connection_window_size(64 * 1024)
    .initial_stream_window_size(32 * 1024)
    .max_send_buffer_size(32 * 1024)
    .max_header_list_size(4 * 1024)
    .header_table_size(1024)
    .max_decoding_message_size(256 * 1024)
    .max_encoding_message_size(256 * 1024);
```

Set the intended aggregate transport byte budget explicitly with
`Server::byte_budget` or `with_byte_budget_tracker`. Its value can exceed the
per-stream send threshold so whole encoded messages fit. Transport configuration
preserves this tracker regardless of builder order.

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
       │                    ├── Acquired: Admit 1 Dispatch Future
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
2. **RPC Dispatch Futures:**
   - Each admitted RPC has one dispatch future in its connection's wake-indexed set; the current policy does not spawn one Tokio task per handler.
   - Rejected RPCs execute inline on the connection loop without admitting a handler future.
   - The permit `_permit: Option<OwnedSemaphorePermit>` moves into the admitted future:
     ```rust
     admitted.push(async move {
         let _lease = lease;
         let _permit = permit;
         dispatch.dispatch(...).await;
     });
     ```
   - Completion or destruction of the dispatch future drops its permit, returning the slot to the shared semaphore.
   - Its cancellation owner signals on transport task abortion and connection-age teardown too. Normal producer/response lifetimes retain their documented ownership; an early return must not cancel work still owned by the response.
   - Dropping a cancellation waiter removes its slab slot in O(1). Storage follows peak live registrations, not all registrations over the connection's lifetime; waker callbacks run outside the lock.
3. **Quiescent Draining:**
   - During server shutdown (`drain_tx` / `drain_rx`), the accept loop stops accepting new sockets, closes the listener, and signals connection tasks.
   - In-flight work drains up to `max_connection_age_grace`, then connection teardown drops admitted futures and their permits. Recovery checks verify task, byte and slot cleanup under finite limits.

### 7.2 Bounded Drain & Cancellation Cleanup Model (RT-08)

Shutdown stops admission, lets established calls drain for a configured grace
period, then closes stalled connections. Handshakes select directly on shutdown:

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
- **Client Call Future Drop:** Dropping an in-flight `Call<T>` signals cancellation and releases the client RPC permit it owns. The driver resets an open stream. An unpolled call may never open a wire stream; a completed streaming response retains its own lifetime and resources.
- **Server Stream Reset Handling:** When a client RST or disconnect is received:
  - Inbound readers preserve the terminal reset code; an RST CANCEL maps to CANCELLED. A disconnect may instead be UNAVAILABLE.
  - Outbound stream senders (`drain_to_wire`) abort on `send.poll_reset`, ending dispatch work.
  - Dropping the owning RPC/stream state releases its permits and byte leases. Recovery checks bound cleanup; one call ending does not make a shared budget zero while other calls remain active.
- **Seeded Fault Probe:** A handler activity counter can reach zero one
  scheduler turn before its outer dispatch future drops the server permit. The
  lifecycle harness retries probe admission for at most 300 ms; persistent
  `RESOURCE_EXHAUSTED` still fails. The separate direct call-drop check above
  continues to require immediate client permit reclaim.
- **Task Cleanup:** Upload/response drivers use cancellation and transport-reset wakeups. Handler scheduling is wake-indexed; cancellation parking is tied to the owning runtime. Test task-count and byte-budget recovery under finite admission limits rather than infer a universal no-leak guarantee from these mechanisms.

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

The separate-process `rpc-bench fairness` report adds numeric client/server
`byte_budget_active_byte_permit_tokens_lifetime_peak` and
`byte_budget_active_byte_permit_tokens_post_drain` fields under
`per_endpoint`. Each process reads its own tracker; lifetime peaks include
warmup and the client's post-drain probe. These are exact token counts after
acquisitions complete, not counts inferred by dividing sampled bytes by
message size. The existing `active_permits_peak` and
`active_permits_post_drain` fields remain `null`: they refer to unavailable
RPC-slot semaphore occupancy. Sampled byte peaks remain lower bounds, and
full listener/transport queue delay, other fairness cells and dedicated-host
paired reference runs remain unqualified.

---

## 8. Implementation and checks

The implementation cards and checks are:

1. **RT-06 ("Enforce the approved transport byte budget"):**
   - Implements the shared byte-admission and accounting mechanism in `pbrs-grpc/src/limits.rs`, `client.rs`, and `server.rs`.
   - Adopts the error and rejection semantics specified in Section 5 (immediate TCP drop, Trailers-Only `RESOURCE_EXHAUSTED` with `end_stream=true`, zero payload buffering).
   - Validates memory bounds under synthetic hostile tests in `pbrs-grpc/tests/resource_bounds.rs`.
2. **RT-07 ("Verify overload fairness and slow-peer isolation"):**
   - Uses the bounded production profiles defined in Section 6 to benchmark mixed workloads (bulk streams mixed with small unary RPCs, slow readers/writers, and reset storms).
   - Checks admitted-call progress and the queue-delay bounds above; RSS is recorded separately.
3. **RT-08 ("Prove bounded drain and cancellation cleanup"):**
   - Checks drain under unending streams, stalled uploads and blocked peers against `max_connection_age_grace`.
   - Checks that plain TCP and TLS handshake stalls do not prolong drain.
   - Checks immediate client-permit release on `Call<T>` drop across all four call shapes and eventual byte-budget recovery.
   - Checks deadline expiry and handler cleanup in `pbrs-grpc/tests/lifecycle.rs` and `pbrs-grpc/tests/resource_bounds.rs`.

---

## 9. Appendix: Mathematical Notation Reference

| Symbol | Definition | Unit / Type | Default Value |
|---|---|---|---|
| $N_{\text{conn}}$ | Number of concurrent active connections | Integer | Unbounded (`None`) |
| $N_{\text{rpc}}$ | Number of concurrent active RPCs | Integer | Unbounded (`None`) |
| $W_{\text{conn}}$ | HTTP/2 connection-level flow control window | Bytes | 16 MiB ($16,777,216$) |
| $W_{\text{stream}}$ | HTTP/2 stream-level flow control window | Bytes | 16 MiB ($16,777,216$) |
| $S_{\text{send\_buf}}$ | Send-capacity threshold per HTTP/2 stream | Bytes | 1 MiB ($1,048,576$) |
| $H_{\text{table}}$ | HPACK dynamic table size | Octets | 4,096 |
| $H_{\text{list}}$ | Maximum uncompressed header list size | Octets | 16,384 |
| $M_{\text{decode\_cap}}$ | Maximum inbound message decoding size | Bytes | 4 MiB ($4,194,304$) |
| $M_{\text{encode\_cap}}$ | Maximum outbound message encoding size | Bytes | Unbounded (`None`) |
| $S_{\text{replay\_buf}}$ | Transparent retry replay buffer size | Bytes | Request payload length |
| $B_{\text{stream\_queue}}$ | Streaming channel queue depth | Messages | 16 |
