# pbrs-h2 engine design (H2-03)

**Status:** Proposed for maintainer review; no code ships with this document.
**Baseline:** `2598aa12` on 2026-09-29. **Card:** H2-03 (depends on H2-01, H2-02).
**Supersedes:** nothing. **Related:** [H2 engine decision](h2-engine.md) (H2-01
no-go record), [h2 cost attribution](../evidence/h2-costs.md),
[transport seam](../evidence/h202-transport.md),
[resource budgets](../resource-budgets.md).

**Review gate:** the maintainer must approve this design and the crate-placement
recommendation before H2-04 starts. A design note is not production evidence,
and it does not overturn the H2-01 no-go: §2 states the only evidence that can.

## 1. Context and constraints

H2-01 closed as **no-go on starting a custom engine**: on the contended
loopback dev-loop, h2 internals measured 0.8–1.3% self instructions and ~9
allocs / 2.9 KiB per uncompressed RPC — visible but far below the 15% modeled
gain bar, and the scaling probe did not prove h2 blocks a required multi-core
design. H2-03 was nevertheless scheduled as a wave-2 decision draft so the
lane has a buildable design ready if dedicated-host evidence reopens the
question. This document is that design, and nothing else.

Hard constraints inherited from the program:

- **Seam first.** `pbrs-grpc/src/transport/` (H2-02) is the only integration
  surface: 11 traits, h2-backed today, no `h2::` type outside
  `transport/h2.rs`. pbrs-h2 implements the same traits behind a feature flag;
  call sites migrate by swapping the backend import. No behavior change lands
  without H2-15 qualification and the H2-14 default decision.
- **Fallback.** The h2 backend stays available for at least two releases after
  any default switch (program risk table).
- **Dependencies require review.** The engine core must be sans-IO
  with zero mandatory async/TLS dependencies (F7); Tokio support is an
  adapter module. Every new `unsafe` needs a `SAFETY` comment, an
  [unsafe-invariants](../unsafe-invariants.md) entry, and Miri coverage (QG-01).
- **Gates never regress.** F2 (interop), F3 (h2spec + negative tests),
  F6 (24 CPU-hours fuzz per target, Miri), hostile suite: all green before
  H2-14, on one exact candidate SHA (H2-15).
- **Module paths are pre-assigned.** H2-04…H2-13 write scopes name
  `pbrs-h2/src/{frame/,hpack/,conn/{state,flow,bdp,sched,limits},tokio.rs,buf.rs}`.
  This design adopts those paths exactly.

Non-goals: server push (gRPC forbids it), stream prioritization scheduling
(RFC 9113 removed it; PRIORITY frames are validated then ignored), HTTP/1.x
fallback (stays in pbrs-grpc), TLS (stays above the engine via the IO adapter),
any gRPC semantic change (content-type, timeout, compression, status mapping
all stay in pbrs-grpc exactly as today).

## 2. Clearing the 15% gate: measurement protocol, not a claim

This design does **not** assert the gate is cleared. It defines the only two
ways to clear it (from the H2-01 revisit trigger) and the measurement protocol
each requires. H2-04 stays blocked until one of them produces claim-grade
evidence on a dedicated Linux host.

### 2.1 Why the loopback number cannot settle it

In `h2-costs.md`, Tokio self-IR is 54–59% of the loopback cells: the
closed-loop harness's scheduler/wait overhead dwarfs every transport share, so
h2's 0.8–1.3% is a share of a denominator dominated by harness wait, not a
ceiling on removable cost. The gate needs a denominator that looks like
production: open-loop load, real network (or at least separate processes with
pinned CPUs), matched windows/Nagle/runtime (SB-01/SB-11), and per-RPC CPU
attribution that separates scheduler wait from transport work.

### 2.2 Gate path A: modeled removable cost ≥ 15% on primary C/D cells

Primary cells are C2/C8 (client) and D2/D4 (server). The protocol:

1. SB-11 matrix on a dedicated host, pbrs-grpc vs required peers, with
   callgrind IR/RPC, DHAT allocs/RPC, and lock-wait counters (h2 upstream
   instrumentation from the H2-01 patch list, item 2/5).
2. Attribute CPU/RPC to: (a) h2 stream-store/frame-queue locks,
   (b) h2 HPACK/header encode, (c) h2 frame IO/wakeups, (d) h2 allocs,
   (e) pbrs-grpc above the seam, (f) runtime wait.
3. The engine may claim only (a)–(d) minus its own modeled cost; the claim
   clears the gate if the removable sum is ≥ 15% of non-wait CPU/RPC on both
   a client-primary and a server-primary cell.

Modeled-gain budget this design must fill (hypotheses to measure, not facts):

| # | Removable cost (H2-01 evidence) | Design mechanism | Assumed magnitude |
|---|---|---|---|
| 1 | h2 stream-store + frame-queue mutexes (2/conn; unquantified, noisy) | Single-owner connection: no locks on any hot path (§4.1) | Must-measure under many-stream contention; the largest unknown |
| 2 | `HeaderBlock::into_encoding` largest h2 symbol (0.24% self-IR) + per-header churn | Pre-encoded per-method header templates (H2-10, §4.3) | Must-measure; fixed gRPC header sets encode as one memcpy |
| 3 | ~9 h2 allocs / 2.9 KiB per RPC | Zero-alloc steady-state RPC: slab stream table, reused buffers (§4.4, §4.7) | Bounded by allocator cost share on the claim host |
| 4 | Frame-queue wakeups (8–15 ctx-switch/RPC proxy, noisy) | Single task drives connection; batched vectored flush (§4.7) | Must-measure with Linux wakeup counters |
| 5 | DATA copies across chunk boundaries | Zero-copy DATA path over caller buffers (§4.7) | Visible on C4/streaming cells first |

### 2.3 Gate path B: h2 blocks a required design

If RX-02 (thread-per-core) or the many-stream scaling target (D8) proves that
h2's two-mutex connection design prevents the required architecture and the
limitation cannot be fixed upstream, path B clears the gate regardless of the
15% number. This design keeps path B open by making single-ownership and
adapter-swappable IO (Tokio now, io_uring later) structural, not optional.

## 3. Architecture overview

One connection is one sans-IO state machine, owned by exactly one task. The
engine never touches the network, the clock, or the allocator on hot paths:
bytes in, bytes out, explicit wakeups.

```text
                         pbrs-h2 crate
+-------------------------------------------------------------+
|  sans-IO core (no tokio, no time, no IO)                    |
|                                                             |
|  wire bytes --> frame::Decoder --> conn::Connection        |
|                                  (owns stream table, HPACK  |
|                                   decoder, flow windows,    |
|                                   flood budgets)            |
|                                      |  events (headers,    |
|                                      v  data, trailers,     |
|                                  call-site handles          |
|                                  (SendStream/RecvStream)    |
|                                      |  send calls          |
|                                      v                      |
|                                  sched::Writer --> frame    |
|                                  (batched, vectored)        |
|                                                             |
|  adapters: tokio::Driver (H2-06) | iouring (RX-03, later)   |
+-------------------------------------------------------------+
|  pbrs-grpc/src/transport/pbrs_h2.rs: seam trait impls       |
+-------------------------------------------------------------+
```

Core rules:

- **Single owner.** `conn::Connection` is `!Sync` by construction (holds
  `PhantomData<*const ()>` or `Cell`-based internals where needed; exact
  mechanism is H2-05's choice). All stream handles are IDs or short borrows
  from the connection task — never `Arc<Mutex<…>>`. Cross-task communication
  (RPC tasks ↔ connection task) goes through the existing seam handle shapes,
  which the pbrs_h2 backend implements with channels owned by the driver task.
- **Sans-IO boundary.** The core exposes `poll`-free methods:
  `receive(bytes) -> Events`, `poll_transmit(buf) -> Action`,
  `inject_time(now)`, `wants_write()`. All timers (PING RTT, reset retention,
  flood windows) consume injected time. All IO (read/write/flush, TLS, TCP
  tuning) lives in adapters. This makes the core deterministically testable
  and fuzzable without a runtime.
- **Bounded everything.** Every queue, table, budget, and buffer has a
  configured cap; exceeding a cap degrades to RST or GOAWAY, never to
  unbounded growth. Caps default to today's `ServerConfig`/`ChannelConfig`
  values (§7).
- **gRPC specialization is explicit.** Generic HTTP/2 stays correct (h2spec
  must pass), but fast paths assume: no push, no priority scheduling, small
  fixed header sets, DATA-heavy streams, trailers-only error responses. Each
  fast path names its fallback in §4.

## 4. Module design

### 4.1 `conn::Connection`: the state machine (`conn/mod.rs`, `conn/state.rs`)

One struct owns, per connection: the frame decoder, HPACK decoder, stream
table, connection flow window, HPACK encoder, transmit scheduler, flood
budgets, ping slots, GOAWAY state, and SETTINGS (local + peer). Client and
server share the machine; a `Side` const generic (or enum, H2-05's choice)
selects preface shape, stream-ID parity, and accept-vs-open behavior.

Event model: `receive()` ingests newly read bytes and returns a small inline
`Events` iterator (headers / data / trailers / reset / goaway / settings-ack
/ ping-ack). Sending is by handle calls that enqueue into the scheduler and
return `WouldBlock`-style backpressure when windows or the send buffer are
exhausted; the driver flushes with `poll_transmit`. No internal task, no
wakers inside the core — the adapter translates `wants_write()` and event
availability into waker registrations at the seam boundary.

Stream states follow RFC 9113 §5.1 exactly: idle, reserved (unused for gRPC
but representable for PUSH_PROMISE rejection paths), open, half-closed
(local/remote), closed, plus engine-internal `reset-retained` for the
error-reporting retention window (today's `reset_stream_duration`).

### 4.2 Frame codec (`frame/`)

Zero-copy frame header parsing over caller-supplied buffers; payloads are
borrowed slices or `Bytes` cuts, never copied to parse. The decoder is a
resumeable pull parser: `need(n)` / `frame(frame)` states, tolerant of
arbitrary TCP segmentation (the hostile
`regression_chunk_fragmentation_preserves_message_stream` property becomes a
codec unit test over all split points). The encoder writes frame headers
directly into the transmit buffer; padding is never generated (gRPC has no
use for it) but received padding is validated (length checks, strip).

Frame-size enforcement: peer frames above our `max_frame_size` are a
connection error `FRAME_SIZE_ERROR` (RFC 9113 §4.2); control-frame fixed
lengths are checked per type (§6).

### 4.3 HPACK with gRPC header templates (`hpack/`, H2-04b + H2-10)

Two layers:

- `hpack::Codec`: a complete, spec-exact RFC 7541 encoder/decoder with
  dynamic tables sized by SETTINGS, integer/Huffman coding, and
  `COMPRESSION_ERROR` on any decoding failure. Fuzzed standalone
  (`h2_hpack.rs` target). Decoder tracks uncompressed header-list bytes
  against `max_header_list_size` *during* decoding and aborts mid-block —
  oversize is a stream error (RST), not a connection kill, unless the
  CONTINUATION-count cap trips (see §7: preserves the h2
  `too_many_continuations` → `ENHANCE_YOUR_CALM` behavior).
- `hpack::templates` (H2-10): per-method pre-encoded header blocks. gRPC
  request/response headers are near-constant per method (`:method`,
  `:scheme`, `:path`, `content-type`, `te`, plus small metadata deltas).
  Codegen (`native_stubs.rs`) emits the static block bytes per method; at
  send time the engine memcpys the template and appends only varying fields
  (authority on first use, custom metadata, timeout). Encoder dynamic-table
  updates stay correct: templates are emitted as literals without indexing
  plus static-table indexed fields, so they never disturb table state; an
  adaptive mode may index stable entries later (measure first).

Inbound fast path: static-table indexed fields decode via a 61-entry jump
table; literal names hash to interned pseudo-header slots. No `HashMap` on
the hot path; unknown headers go through the general path.

### 4.4 Stream table without locks (`conn/state.rs`)

A slab (`Vec<StreamSlot>` + free list) indexed by stream ID, pre-sized to
`max_concurrent_streams` (default 256). Each slot is fixed-size (~128–256
bytes target, H2-13's budget): state enum, send/receive windows, header-block
accumulator handle, reset info, waker slots for the two halves. No per-stream
allocation in steady state: opening a stream pops a free slot; closing
returns it. Stream IDs above the peer's used maximum are rejected without
allocation; the table never grows past its cap — excess streams get
`REFUSED_STREAM` (preserves transparent-retry behavior).

Handles handed to RPC tasks are `(ConnectionHandle, StreamId)` pairs where
the handle is a channel to the owner task (MPSC, bounded). This is the one
allowed cross-task hop and it replaces h2's mutex pair: all connection
mutation happens on the owner task. Waker overhead is one atomic registration
per pending direction, same as h2's, but with no lock acquisition on the
wake path.

### 4.5 Flow control with BDP estimation (`conn/flow.rs`, `conn/bdp.rs`)

Stream and connection windows exactly per RFC 9113 §5.2: 16-bit…31-bit
accounting, `WINDOW_UPDATE` on consumption, `FLOW_CONTROL_ERROR` on overflow.
Defaults stay 16 MiB / 16 MiB.

`conn/bdp.rs` ports `pbrs-grpc/src/bdp.rs` (hyper's BDP-ping shape) into
sans-IO: the estimator consumes `(bytes_received, rtt_sample)` inputs and
emits target windows; RTT samples come from the single outstanding PING slot,
which the driver shares between keepalive and BDP exactly as today (one
`PingPong` owner per connection). Time is injected, so BDP logic is unit
testable with scripted clocks. Adaptive mode stays opt-in with the same
`initial_window`/`max_window` config; fixed mode is byte-identical policy to
today.

Receive-window release (`release_capacity`) is O(1): credit accumulates per
stream and flushes as coalesced `WINDOW_UPDATE`s on the next transmit —
no per-byte frames, no per-release wakeup.

### 4.6 SETTINGS / PING / GOAWAY / RST / CONTINUATION handling

- **SETTINGS** (`conn/mod.rs`): validate per §6 (length multiple of 6,
  stream 0, value ranges); apply `HEADER_TABLE_SIZE` (resize encoder tables),
  `MAX_CONCURRENT_STREAMS` (shrink/expand accept budget),
  `INITIAL_WINDOW_SIZE` (adjust all live stream send windows with overflow
  → `FLOW_CONTROL_ERROR`), `MAX_FRAME_SIZE` (clamp transmit framing),
  `MAX_HEADER_LIST_SIZE` (advisory inbound cap, we enforce our own anyway);
  unknown IDs ignored; `ENABLE_PUSH` must be 0/1 (else `PROTOCOL_ERROR`),
  value 1 ignored (we never push). ACK every non-ACK SETTINGS immediately,
  coalesced into the next flush. Empty SETTINGS is legal (common) — the
  flood defense is rate-based (§7), not content-based.
- **PING** (`conn/mod.rs`): non-ACK PING on stream 0 with 8-byte opaque data
  is ACKed with identical data, coalesced (multiple pending pings ACK in one
  flush); ACKs route to the single ping slot (keepalive/BDP waiter). Any
  violation (length ≠ 8, stream ≠ 0) is a connection error. Inbound PING
  rate is limited (§7) — this closes the gap noted in `config.rs`: today
  inbound client PINGs are never GOAWAY'd.
- **GOAWAY** (`conn/mod.rs`): on send (`graceful_shutdown`): emit GOAWAY
  `NO_ERROR` with last-processed-stream-id; in-flight streams run to
  completion; new stream opens fail locally. On receive: mark connection
  draining, fail opens above last-stream-id with retryable errors so the
  client transparent-retry path (REFUSED_STREAM/GOAWAY-before-commit)
  replays on a fresh connection. Error-code GOAWAYs fail affected streams
  with the mapped status. Debug data is capped (default 256 bytes) and never
  logged verbatim (OB-03 masking rules stay in pbrs-grpc).
- **RST_STREAM** (`conn/state.rs`): validate (length 4, stream ≠ 0, stream
  not idle — else connection error `PROTOCOL_ERROR`); apply to stream state
  (wake both halves with the reset code); received RSTs on half-closed
  (remote) are absorbed. Locally generated RSTs (refusals, protocol errors,
  cancellations) flow through the local-reset flood budget (§7).
- **CONTINUATION** (`conn/mod.rs` + `hpack`): HEADERS/PUSH_PROMISE without
  `END_HEADERS` enters continuation-expected state for that stream; any
  other frame type meanwhile is a connection error `PROTOCOL_ERROR`;
  CONTINUATION for another stream is likewise an error. Fragments append to
  a capped accumulator (cap = `max_header_list_size`); exceeding it mid-block
  is a stream error (RST) and exceeding the CONTINUATION *count* cap is
  GOAWAY `ENHANCE_YOUR_CALM` (preserves h2 `too_many_continuations`).

PRIORITY frames are validated (length 5, stream ≠ 0) and otherwise ignored
(no scheduling effect — RFC 9113 removed prioritization). Self-dependency is
a stream error `PROTOCOL_ERROR` (RST on that stream), preserving the current
h2-derived behavior that the `protocol_error_rst_flood` hostile test relies
on, and counting toward the local-reset flood budget. PUSH_PROMISE receipt is
always a connection error `PROTOCOL_ERROR` (neither side pushes).

### 4.7 Write scheduling with vectored IO (`conn/sched.rs`, `buf.rs`)

The scheduler owns one transmit buffer (`buf.rs`, default 16 KiB + 1 MiB
send-buffer cap honoring `max_send_buffer_size`) and a ready queue of streams
with pending DATA. Policy: control frames (SETTINGS ACK, PING ACK, RST,
WINDOW_UPDATE, GOAWAY) always precede DATA; HEADERS precede DATA on the same
stream; DATA streams drain round-robin in stream-ID order (deterministic,
starvation-free, no priority tree). Small writes coalesce: HEADERS + first
DATA + trailers-only responses for a unary RPC flush as a single `writev`.

Backpressure: `send_data` fails fast when stream or connection window or the
send-buffer cap is exhausted; `poll_capacity` waits for window growth. This
preserves the `wire::send_bytes` chunking contract (never wait for whole
message credit; only the final chunk carries `END_STREAM`).

The adapter flushes with vectored writes (`writev` / `write_buf` chains) and
registers `wants_write` for writability. Steady-state unary response target:
one syscall.

### 4.8 IO adapters: Tokio first, io_uring later

`tokio::Driver` (H2-06) binds the sans-IO core to `AsyncRead`/`AsyncWrite`:
read pump → `receive()` → dispatch events to waiting RPC tasks; send calls
from RPC tasks → owner-task channel → `poll_transmit` → write pump. The
driver task IS the single owner. TLS (rustls), TCP tuning (NODELAY,
keepalives), UDS, and handshake/preface timeouts stay in pbrs-grpc's accept
and dial paths exactly as today — the adapter receives an already-handshaked
IO object, same as the h2 backend does now.

io_uring (`RX-03`) needs no engine change: a second adapter implements the
same read-pump/write-pump contract over ring buffers. The design requirement
on the core is only that `receive`/`poll_transmit` never assume `&mut`
exclusivity of the caller's IO buffers beyond the call — satisfied by the
borrowed-slice codec (§4.2).

The pbrs-grpc seam binding (`transport/pbrs_h2.rs`, H2-06's `transport/`
write scope) implements the 11 H2-02 traits against the driver. Seam changes
are forbidden unless the maintainer approves: if a trait cannot be honored
efficiently (candidates: `poll_capacity` semantics, `PingPong` sharing), H2-06
reports back and the design is amended — it does not silently diverge.

## 5. RFC 9113 MUST inventory → module + test

Working inventory compiled by hand from RFC 9113 for this design. H2-04/H2-07
must machine-verify it against the RFC text (MUST/MUST NOT grep per section)
and extend `scripts/h2spec.sh` coverage mapping; any gap found there amends
this table, not the code's behavior.

Notation: module paths are relative to `pbrs-h2/src/`. Tests: `unit` =
engine unit test, `h2spec` = numbered h2spec case family, `neg` = hostile or
negative-interop test (existing `pbrs-grpc/tests/hostile.rs` case or its
pbrs-h2 successor), `fuzz` = structure-aware fuzz assertion.

| ID | Requirement (paraphrase) | Module | Test |
|---|---|---|---|
| 9113-3.4-1 | Client MUST send connection preface first; server MUST treat anything else as connection error `PROTOCOL_ERROR` | `tokio` (server accept), `frame` | unit, h2spec 3.5 |
| 9113-3.4-2 | Server MUST send SETTINGS as its first frame | `conn`, `tokio` | unit, h2spec 3.5 |
| 9113-3.5-1 | Client preface SETTINGS MUST be the first frame after the magic | `frame`, `conn` | unit, h2spec 3.5 |
| 9113-4.1-1 | Reserved frame bit MUST be ignored on receipt | `frame` | unit, fuzz |
| 9113-4.2-1 | Receiver MUST treat a frame larger than `max_frame_size` as connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 4.2 |
| 9113-4.3-1 | Header block fragments MUST arrive as a contiguous sequence; any other frame type while CONTINUATION is expected MUST be a connection error `PROTOCOL_ERROR` | `conn` | unit, h2spec 4.3, neg `continuation_flood` variant |
| 9113-4.3-2 | Header decoding failure MUST be a connection error `COMPRESSION_ERROR` | `hpack`, `conn` | unit, h2spec 4.3, fuzz |
| 9113-5.1-1 | Frames other than HEADERS/PRIORITY on an idle stream MUST be a connection error `PROTOCOL_ERROR` | `conn/state` | unit, h2spec 5.1 |
| 9113-5.1-2 | Client-initiated streams MUST use odd IDs, server-initiated even; MUST be numerically increasing per side; violations MUST be connection error `PROTOCOL_ERROR` | `conn/state` | unit, h2spec 5.1 |
| 9113-5.1-3 | A stream MUST NOT be reused after close; frames on closed streams (except PRIORITY/WINDOW_UPDATE/RST) MUST be stream error `STREAM_CLOSED` | `conn/state` | unit, h2spec 5.1 |
| 9113-5.1-4 | Frames after END_STREAM (except WINDOW_UPDATE/PRIORITY/RST) MUST be a connection error of type `STREAM_CLOSED` | `conn/state` | unit, h2spec 5.1 |
| 9113-5.2-1 | Sender MUST NOT send DATA exceeding stream or connection window | `conn/flow`, `conn/sched` | unit, neg interop |
| 9113-5.2-2 | Receiver MUST treat a window increment past 2³¹−1 as `FLOW_CONTROL_ERROR` (connection or stream scope) | `conn/flow` | unit, h2spec 6.9 |
| 9113-5.4-1 | Unknown frame types MUST be ignored | `frame` | unit, h2spec 5.5, fuzz |
| 9113-5.4-2 | Connection error: endpoint SHOULD send GOAWAY first (design upgrades to MUST except when the connection is already dead) | `conn` | unit, h2spec (all error cases) |
| 9113-6.1-1 | DATA with stream ID 0 MUST be connection error `PROTOCOL_ERROR` | `frame`, `conn/state` | unit, h2spec 6.1 |
| 9113-6.1-2 | DATA on a non-open/non-half-closed(local) stream MUST be stream error `STREAM_CLOSED` | `conn/state` | unit, h2spec 6.1 |
| 9113-6.1-3 | DATA padding length byte MUST NOT exceed payload length, else connection error `PROTOCOL_ERROR` | `frame` | unit, fuzz |
| 9113-6.2-1 | HEADERS with stream ID 0 MUST be connection error `PROTOCOL_ERROR` | `frame`, `conn/state` | unit, h2spec 6.2 |
| 9113-6.2-2 | HEADERS padding rules as DATA | `frame` | unit, fuzz |
| 9113-6.3-1 | PRIORITY length ≠ 5 MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.2 |
| 9113-6.3-2 | PRIORITY with stream ID 0 MUST be connection error `PROTOCOL_ERROR` | `frame` | unit, h2spec 6.2 |
| 9113-6.3-3 | PRIORITY self-dependency MUST be stream error `PROTOCOL_ERROR` (RST on that stream; counted to the local-reset budget) | `conn/state`, `conn/limits` | unit, neg `protocol_error_rst_flood` |
| 9113-6.4-1 | RST_STREAM length ≠ 4 MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.4 |
| 9113-6.4-2 | RST_STREAM with stream ID 0 MUST be connection error `PROTOCOL_ERROR` | `frame` | unit, h2spec 6.4 |
| 9113-6.4-3 | RST_STREAM on an idle stream MUST be connection error `PROTOCOL_ERROR` | `conn/state` | unit, h2spec 6.4 |
| 9113-6.5-1 | SETTINGS length not a multiple of 6 MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.5 |
| 9113-6.5-2 | SETTINGS on a non-zero stream MUST be connection error `PROTOCOL_ERROR` | `frame` | unit, h2spec 6.5 |
| 9113-6.5-3 | SETTINGS with ACK set and non-empty payload MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.5 |
| 9113-6.5-4 | Unknown SETTINGS parameters MUST be ignored | `conn` | unit, h2spec 6.5 |
| 9113-6.5-5 | `ENABLE_PUSH` ≠ 0/1 MUST be connection error `PROTOCOL_ERROR` | `conn` | unit, h2spec 6.5 |
| 9113-6.5-6 | `INITIAL_WINDOW_SIZE` > 2³¹−1 MUST be connection error `FLOW_CONTROL_ERROR`; applying it MUST adjust live stream windows, overflow → `FLOW_CONTROL_ERROR` | `conn`, `conn/flow` | unit, h2spec 6.5 |
| 9113-6.5-7 | `MAX_FRAME_SIZE` outside 2¹⁴…2²⁴−1 MUST be connection error `PROTOCOL_ERROR` | `conn` | unit, h2spec 6.5 |
| 9113-6.6-1 | Receipt of PUSH_PROMISE MUST be connection error `PROTOCOL_ERROR` (neither side pushes) | `conn` | unit, h2spec 6.6 |
| 9113-6.7-1 | PING length ≠ 8 MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.7 |
| 9113-6.7-2 | PING on a non-zero stream MUST be connection error `PROTOCOL_ERROR` | `frame` | unit, h2spec 6.7 |
| 9113-6.7-3 | Receiver MUST ACK every non-ACK PING with identical opaque data | `conn` | unit, h2spec 6.7, keepalive tests |
| 9113-6.8-1 | GOAWAY on a non-zero stream MUST be connection error `PROTOCOL_ERROR` | `frame` | unit, h2spec 6.8 |
| 9113-6.8-2 | After GOAWAY, streams above last-stream-id MUST be treated as unprocessed (retryable) | `conn`, seam binding | unit, retry-safety tests |
| 9113-6.9-1 | WINDOW_UPDATE length ≠ 4 MUST be connection error `FRAME_SIZE_ERROR` | `frame` | unit, h2spec 6.9 |
| 9113-6.9-2 | WINDOW_UPDATE with zero increment MUST be stream error `PROTOCOL_ERROR` (stream scope) or connection error `PROTOCOL_ERROR` (connection scope) | `conn/flow` | unit, h2spec 6.9 |
| 9113-6.10-1 | CONTINUATION for a stream with no pending header block MUST be connection error `PROTOCOL_ERROR` | `conn` | unit, h2spec 6.10 |
| 9113-8.x-1 | Request/response validation the engine owns: `:method`/`:scheme`/`:path` presence is enforced above the engine (pbrs-grpc, unchanged); the engine MUST surface decoded pseudo-headers verbatim including duplicates | `hpack`, seam binding | unit, hostile content-type tests |
| 9113-9.x-1 | TLS peers: engine is TLS-agnostic; the adapter MUST NOT start the HTTP/2 session until the TLS handshake completes (unchanged pbrs-grpc behavior) | `tokio` adapter | tls tests |

## 6. RFC 7541 MUST inventory → module + test

Same verification rule as §5: H2-04b machine-checks this table against the
RFC text.

| ID | Requirement (paraphrase) | Module | Test |
|---|---|---|---|
| 7541-4.2-1 | Encoder MUST NOT emit a dynamic-table size larger than the protocol maximum (`SETTINGS_HEADER_TABLE_SIZE`); decoder MUST treat a larger signaled size as decoding error → `COMPRESSION_ERROR` | `hpack` | unit, h2spec 4.2, fuzz |
| 7541-4.2-2 | Dynamic-table size update MUST be honored by evicting entries (oldest first) until within the new bound | `hpack` | unit |
| 7541-5.1-1 | Integer overflow past the implementation limit MUST be a decoding error → `COMPRESSION_ERROR` (bound: reject prefix continuations beyond 5 bytes / 2³¹) | `hpack` | unit, fuzz |
| 7541-5.2-1 | Invalid Huffman coding MUST be a decoding error → `COMPRESSION_ERROR` | `hpack` | unit, fuzz |
| 7541-6.1-1 | Indexed field with index 0 or above static+dynamic size MUST be a decoding error → `COMPRESSION_ERROR` | `hpack` | unit, h2spec 4.2, fuzz |
| 7541-6.3-1 | Dynamic-table size update MUST appear only at the start of a header block; elsewhere it MUST be a decoding error → `COMPRESSION_ERROR` | `hpack` | unit, h2spec 4.2 |
| 7541-7.x-1 | Sensitive fields (`authorization`, `cookie`, `grpc-*` credentials-carrying metadata) MUST be emitted never-indexed by our encoder | `hpack`, `hpack/templates` | unit |
| 7541-size-1 | Entry size accounting MUST be name+value+32 octets; eviction MUST proceed until the table fits | `hpack` | unit, fuzz differential vs h2 |

## 7. Security limits: every existing defense and its new home

All defaults below equal today's `ServerConfig`/`ChannelConfig` values; any
change needs maintainer approval (worker protocol §9: threshold changes
escalate). New defenses (ping/settings rate limits) are marked NEW — they
close gaps the design may not silently introduce.

| # | Existing defense (current location) | New location | Behavior preserved |
|---|---|---|---|
| L1 | Rapid reset: `max_pending_accept_reset_streams` (20) — remotely-reset streams queued before accept; h2 GOAWAYs past the cap | `conn/limits.rs` accept-queue counter | Same cap, same GOAWAY `ENHANCE_YOUR_CALM`; test: hostile `rst_flood_beyond_pending_reset_cap` (migrated) |
| L2 | Protocol-error RST flood: `max_local_error_reset_streams` (1024) — locally generated RSTs per window | `conn/limits.rs` local-reset budget | Same cap; test: hostile `protocol_error_rst_flood` |
| L3 | Reset retention: `max_concurrent_reset_streams` (50) + `reset_stream_duration` (1 s) | `conn/state.rs` reset ledger, injected time | Same bounds; unit tests on retention/expiry |
| L4 | Header list size: `max_header_list_size` (16 KiB) | `hpack` decoder accounting | Oversize → stream RST (RPC fails, server healthy); test: hostile `metadata_beyond_the_header_list_cap` |
| L5 | CONTINUATION flood: h2 `too_many_continuations` → GOAWAY `ENHANCE_YOUR_CALM` | `conn/limits.rs` continuation counter per header block | Same GOAWAY; test: hostile `continuation_flood` |
| L6 | Small-DATA flood: `data_frame_budget` (25,600 B) → `ENHANCE_YOUR_CALM` | `conn/limits.rs` DATA-frame cost budget | Same cap and GOAWAY; test: hostile `small_data_flood` |
| L7 | NEW: PING flood limiter (today: none — `config.rs` documents inbound PINGs are never GOAWAY'd) | `conn/limits.rs` token bucket (default: 2× keepalive rate + burst, exact values need approval) | PINGs past the rate are dropped then GOAWAY `ENHANCE_YOUR_CALM`; new hostile test required (H2-05c) |
| L8 | NEW: SETTINGS flood limiter (empty-SETTINGS abuse, CVE-2019-9512-class) | `conn/limits.rs` token bucket | Same treatment as L7; new hostile test required |
| L9 | Empty-frame abuse: zero-length DATA without END_STREAM, empty CONTINUATION, zero-increment WINDOW_UPDATE | `conn/limits.rs`: zero-length DATA costs against L6 at full frame cost; empty CONTINUATION counts to L5; zero-increment WINDOW_UPDATE is a spec error (§5: 9113-6.9-2) | No free work for empty frames; fuzz + unit tests |
| L10 | `max_concurrent_streams` (256) | `conn/state.rs` slab cap | Excess → `REFUSED_STREAM` (transparent retry preserved); load tests |
| L11 | `max_send_buffer_size` (1 MiB) | `conn/sched.rs` transmit cap | Backpressure via `poll_capacity`; fairness tests (RT-07) |
| L12 | `max_frame_size` (1 MiB) | `frame` decoder | Oversize → `FRAME_SIZE_ERROR`; §5: 9113-4.2-1 |
| L13 | HPACK bomb (dynamic-table / header-block memory) | `hpack`: table bound by SETTINGS (default 4 KiB ×2) + L4 accounting during decode | Bounded before allocation; fuzz + hostile |
| L14 | Unfinished header blocks (HEADERS without END_HEADERS, peer goes quiet) | `conn`: capped accumulator + no accept; server idle/age timeouts (unchanged, pbrs-grpc) reap the connection | Test: hostile `unfinished_headers_do_not_take_the_accept_loop_down` |
| L15 | Keepalive: PING interval/timeout, dead-peer detection | Single ping slot in `conn`; policy in driver/seam (bdp.rs shape preserved) | Existing keepalive + `adaptive_window.rs` tests |
| L16 | BDP/adaptive windows (opt-in, 16 MiB cap) | `conn/bdp.rs` + `conn/flow.rs` | Same caps; `adaptive_window.rs` tests |
| L17 | GOAWAY drain (`graceful_shutdown`, connection age/idle) | `conn` GOAWAY emit + seam binding; policy stays in pbrs-grpc | Existing lifecycle/drain tests |
| L18 | Preface/handshake timeouts, TLS timeouts, TCP drop on connection-cap | Adapters + pbrs-grpc accept path (NOT the engine — sans-IO has no sockets) | Existing lifecycle/tls tests, unchanged |

Stays above the engine, byte-for-byte behavior unchanged: gRPC content-type /
`:path` / method validation, `grpc-timeout`, message size caps, gzip bomb
budget, compression-flag/encoding validation, status mapping, retry/hedging,
authz, OTel, channelz, reflection. The hostile tests covering those (roughly
two thirds of `hostile.rs`) must pass unmodified against the new backend —
that is the H2-06/H2-08 acceptance bar.

## 8. Performance targets per scoreboard cell

Targets are relative to the h2 backend on the same host, same shape, matched
windows (SB-01 fairness). Dev-loop targets gate H2-06…H2-13 merges;
claim-grade margins follow contract §7 (≥20% CPU/throughput or no p99
regression > 5%) and are decided by SB-22, not here. Cells the engine cannot
move are marked `—` (inherits pbrs-grpc work).

| Cell | Target (vs h2 backend, same host) | Driven by |
|---|---|---|
| C1 unary latency p50/p99, 1 in flight | No regression; p99 within 5% | Single-owner wake path, no extra hop |
| C2 unary QPS/client core at saturation | ≥15% higher (gate path A needs this on a primary cell) | §2.2 budget items 1–4 |
| C3 streaming msgs/s/client core | ≥20% higher | Zero per-message alloc, batched flush |
| C4 large-message throughput | ≥10% higher; zero DATA copies | Zero-copy DATA path (§4.7) |
| C5 instructions/allocs per RPC | 0 engine allocs on steady-state RPC; IR/RPC down ≥10% on dev-loop | Slab table, templates, reused buffers |
| C6 memory per channel/stream | Per-stream ≤ 256 B engine state; per-conn static ≤ 64 KiB at 256 streams | H2-13 budget |
| C7 time to first RPC (cold TLS) | No regression (handshake path unchanged) | — |
| C8 CPU/RPC at matched load | ≥15% lower | Same as C2, measured at fixed offered load |
| D1 server latency at fixed load | p99 within 5% (no regression) | — |
| D2 sustainable QPS/core within p99 SLO | ≥15% higher | §2.2 budget items 1–4 |
| D3 streaming/core | ≥20% higher | Same as C3 |
| D4 CPU/allocs per RPC | Same as C5 | Same as C5 |
| D5 memory per conn/stream | Same as C6 | H2-13 budget |
| D6 accept + TLS handshake rate | ≥10% higher | Single-writev preface+SETTINGS+window burst |
| D7 overload goodput/fairness | Bounded O(1)-amortized work per hostile frame; flood GOAWAY within cap+1 frames; accept loop serves during floods | `conn/limits.rs` (§7) |
| D8 multi-core scaling 1→N | ≥90% efficiency at 4 cores multi-connection; single connection never contends (no locks) | Single ownership (§4.1, §4.4) |
| E1 grpc_bench-style unary | Tracks C2/D2 wins | — |
| E2 official WorkerService scenarios | Pass + tracks C2/D2 wins | F2 gate |
| E3 TLS enabled | Tracks C2/D2 wins; kTLS-ready zero-copy (RX-05 may use it) | Adapter keeps TLS above engine |
| E4 compression enabled | No engine regression (gzip dominates; engine must not add copies) | Zero-copy DATA path |
| E5 real-network RTT | BDP windows match-or-beat h2+bdp.rs throughput at 1–100 ms RTT | `conn/bdp.rs` (§4.5) |
| F1 conformance/upstream Rust tests | Green, unchanged | Engine is below the codec |
| F2 interop all pinned peers | Pass unmodified | H2-08 |
| F3 h2spec + HTTP/2 negative | h2spec 100% pass (generic + HTTP/2 connection sections); negative interop pass | H2-07 |
| F4 gRFC coverage | Unchanged | — |
| F5 xDS interop | Unchanged | — |
| F6 fuzz/Miri | 24 CPU-hours per target (`h2_frame`, `h2_hpack`, `h2_diff`); Miri clean on `unsafe` (target: zero `unsafe` in core) | H2-09, QG-01 |
| F7 pure-Rust graph | Core has zero non-std dependencies; adapters add only tokio/rustls (already in tree) | QG-04 audit |

Failure rule: any primary-cell regression > 1% dev-loop (2% RPC cells) or any
F-gate red blocks merge — same rule as every optimization card.

## 9. Crate placement recommendation (NEEDS MAINTAINER APPROVAL)

**Recommended: new workspace crate `pbrs-h2`** (sibling of `pbrs-grpc`),
`v0.1.0`, no public API stability promise until H2-14. Final placement is a
maintainer decision — this section is the proposal, not the verdict.

Why a new crate over `pbrs-grpc/src/engine/`:

- The sans-IO core must build with **zero dependencies** (not even tokio) for
  F7 purity, standalone fuzzing, and future no-std/embedded reuse; a
  `pbrs-grpc` module cannot express that boundary.
- Independent fuzz targets (`h2_frame`, `h2_hpack`, `h2_diff`) and Miri runs
  attach to the crate without dragging the gRPC stack into the harness.
- The transport seam (`transport/pbrs_h2.rs`) then depends on `pbrs-h2` like
  it depends on `h2` today — symmetric backends, symmetric fallback, and the
  feature flag (`--features pbrs-h2-backend`, exact name needs approval)
  swaps one import.

Why not a separate repository: the engine and seam evolve in lockstep through
H2-15; cross-repo version skew would slow every H2 card. Revisit after H2-14
if external users want it.

Acknowledged cost: the release rule ("three qualified crates ship together")
gains a fourth crate to qualify; H2-04's `Cargo.toml` + workspace wiring must
update `RELEASE.md` qualification and the QG-04 audit. If the maintainer
rejects a new crate, the fallback is `pbrs-grpc/src/engine/` with a
dependency firewall enforced by an import linter — strictly worse, but
workable.

## 10. Implementation sequence (maps to existing cards)

No new cards proposed; the sequence below fits the current H2 write scopes:

1. H2-04: crate skeleton + `frame/` + `h2_frame` fuzz target. Entry: this
   design approved + QG-01 (Miri policy).
2. H2-04b: `hpack/` + templates groundwork + `h2_hpack` fuzz target.
3. H2-05: `conn/state.rs` + `conn/mod.rs` state machine, sans-IO event model.
4. H2-05b: `conn/flow.rs` windows; H2-05c: `conn/limits.rs` (§7 table).
   Parallel after H2-05.
5. H2-06: `tokio.rs` driver + `transport/pbrs_h2.rs` seam binding behind the
   feature flag. First end-to-end: existing native + hostile + interop suites
   against the new backend.
6. H2-07/H2-08/H2-09: h2spec CI, negative interop, differential fuzzing.
7. H2-10: codegen templates (`native_stubs.rs` + `hpack/templates.rs`).
8. H2-11/H2-12/H2-13: scheduler policy, BDP, memory budgets.
9. H2-15: single-SHA qualification → H2-14 default decision.

Each step merges only with its accept items green; the feature flag keeps
every intermediate state shippable with the h2 backend.

## 11. Open questions for maintainer

1. **Crate placement (§9):** approve new workspace crate `pbrs-h2`, or keep
   the engine inside `pbrs-grpc`? If a new crate: approve the feature-flag
   name and the fourth-crate release-qualification cost?
2. **Gate timing:** H2-01 says H2-03/H2-04 stay blocked until dedicated-host
   evidence crosses the bar. Does approving this design unblock H2-04
   immediately, or only after gate path A/B evidence lands (and who runs
   that measurement — SB-11/SB-22 operators)?
3. **New flood-defense defaults (L7/L8):** approve adding PING-rate and
   SETTINGS-rate GOAWAY defenses the current stack lacks? Proposed starting
   point: token bucket sized at 2× the configured keepalive rate + small
   burst for PING, and 60 SETTINGS/min + burst 10 for SETTINGS — or should
   H2-05c match grpc-go/http2 defaults instead?
4. **PRIORITY self-dependency:** this design keeps h2's behavior (stream
   error + RST, counted to the local-reset budget) rather than silently
   ignoring PRIORITY. Confirm, or prefer pure-ignore per RFC 9113 §5.3?
5. **DATA-after-END_STREAM:** h2 0.4 reports connection `PROTOCOL_ERROR`;
   the RFC says stream error `STREAM_CLOSED`. This design follows the RFC
   (row 9113-5.1-4). Confirm the deliberate divergence from h2 behavior?
6. **Zero-`unsafe` goal:** the design targets zero `unsafe` in the sans-IO
   core (F6 row). Is that a hard requirement, or may H2-13 use `unsafe`
   for the slab/buffer fast paths with the usual `SAFETY` + Miri bar?
7. **io_uring adapter ownership:** confirm the io_uring adapter stays an
   RX-03 experiment behind the runtime seam and is not an H2-14 qualifier.

## 12. Limitations of this document

- The RFC MUST inventories (§5, §6) are hand-compiled working lists, not
  machine-verified; H2-04/H2-04b/H2-07 must verify them against the RFC
  sources and amend the tables.
- The §2.2 gain budget is hypotheses with assumed magnitudes. Only gate path
  A/B evidence on a dedicated host can promote them to a build decision.
- Performance targets (§8) are design goals pegged to the h2 backend on
  matched hardware; no number here is measured or claim-grade.
- io_uring/kTLS/thread-per-core interactions are sketched only far enough to
  keep the core adapter-agnostic; RX-02/RX-03/RX-05 own those designs.
- This draft was written without running any build or test (card checks: []).
