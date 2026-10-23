# gRPC execution and ownership

The target is less work per RPC across the client, server and generated code,
with complete gRPC behavior and explicit resource ownership. Production readiness
and broad superiority over tonic/prost remain unproved. The current source is
`beb4ecd9` (C11); its [evidence capsule](../evidence/grpc-decode-reuse-20261023/README.md)
records validation, measurements and limits.

## Measure the work before replacing a layer

An RPC pays setup, per-message, per-byte and queueing costs. Isolate the term that
matters for each workload:

| Workload | Costs to examine |
|---|---|
| Small unary | Dispatch, metadata, allocations, locks and wakeups |
| Idle streams with active siblings | Readiness, cancellation registration and scheduling |
| Large compressed messages | Codec work, growth, copies and retained capacity |
| Read-all nested messages | Validation, materialization and reference counts |
| Large generated services | Selected-method state, compile time and code size |

Use the same codec, handler work and endpoint settings for transport comparisons.
Full-interop pbrs handlers and minimal Prost handlers are different workloads.
Keep client and server costs separate. Allocation requests, retained capacity,
RSS, CPU instructions, latency and throughput answer different questions.

## Give each resource an owner

Apply connection, RPC, message and queued-byte admission at their owning boundary.
A per-connection stream cap does not bound work across all connections. Limits
that are unset do not create a finite process budget. Buffers, permits, responder
handles and cancellation registrations must remain owned until their work ends.

Cancellation, reset, panic and drain must destroy the same owned work they stop.
Do not release permits early while a poll or response producer still holds memory.
A dropped task handle must not detach a live RPC. Cancellation callbacks and
waker destruction must run outside registration locks; abandoned waiters must
release their slots. Runtime-specific cancellation work stays on its owner runtime.

## Readiness and handler isolation are different

The connection uses a wake-indexed set of concrete admitted futures. It avoids
polling sleeping handlers on unrelated wakes. Historical independent-peer controls
establish that improvement; their details remain in the
[execution record](../evidence/grpc-architecture-20261022/README.md).

Application polls still run on the connection task. A long synchronous handler,
interceptor, codec or destructor can stop HTTP/2 progress and sibling calls.
A ready queue and a timer cannot preempt that poll.

Compare the current inline policy with owned per-RPC tasks under finite admission.
A separate handler driver per connection can isolate protocol progress with a
spare worker, but its handlers still block one another. Per-RPC tasks can isolate
siblings when workers remain available; a current-thread runtime gains no thread
by spawning another task. Keep permits, reset/drop guards and response ownership
through actual cleanup. Measure handoff costs on small calls and mixed workloads.

## Fix deadline origin and terminal ownership

Source review finds a current gap before a custom `Service` chooses its RPC shape:
there is no outer service timeout, and later shape preparation starts a fresh
relative duration. `Rpc::deadline()` also recomputes now plus the duration. A bidi
shape constructed synchronously can stamp a deadline, then start another relative
wait when first polled later. These are source findings; the proposed independent
peer controls have **not run**, so no new wire failure is claimed as reproduced.

Capture one absolute deadline when complete request headers are accepted, before
interceptors, admission queues or service waits. Every later wait must use its
remaining budget; metadata changes, getters and shape selection cannot extend it.
Reject an already expired call before invoking a new service or handler.

Own the whole cooperative service lifetime and one terminal response transition.
An outer timeout alone can drop `Rpc`'s responder and reset the stream before valid
DEADLINE_EXCEEDED trailers are sent. Preserve the pre-header responder or active
response stream through cancellation, with no second responder or duplicate status.
Test pre-shape waits, delayed first polls, response commitment and same-connection
recovery. This repair is independent of scheduling policy and remains open.

## Reuse decoded storage when ownership returns

C11 adds stream-local reuse of fully consumed, uniquely owned decoded `Vec` backing.
The Prost adapter preserves its input owner through full-range and nested bytes
copies, then the transport checks actual uniqueness. Retained Bytes fields and
default pbrs/custom codecs keep their ownership semantics and may miss the cache;
no retained input is copied or overwritten to manufacture reuse.

Cold decoded output up to the existing 8 KiB lookahead uses ordinary decoding,
without a workspace Box or retention lease. A warm workspace still serves later
small or empty messages. Logical initialized/read length follows current output
and message limits, independently of a larger physical allocation left by a
previous message. This avoids initializing that whole allocation for modest input.

Idle retained `Vec` capacity is capped at 4 MiB per stream and 16 MiB per process.
Charge actual capacity, including spare storage; a full cache declines retention
without failing the RPC. Active decoding, application messages, codec state and
RSS are separate. Release on DATA EOF, error, deadline, observed cancellation and
Drop. An unpolled reader observes external cancellation only when polled or dropped.
Unary reads and uncompressed decoding do not create this workspace.

Selected source-paired medians for four-message, 1 MiB zero-filled Prost bidi,
gzip level 6, h2c, one connection and one in-flight RPC:

| Native side | Previous source | C11 | Reduction | C11 above fresh tonic |
|---|---:|---:|---:|---:|
| Client | 28.5 MiB/RPC | 19.5 MiB/RPC | 31.5% | 17.1% |
| Server | 27.5 MiB/RPC | 18.5 MiB/RPC | 32.6% | 18.2% |

These are cumulative allocation requests, with source `9d6fb9c4` as the previous
baseline. Values are rounded; ratios use unrounded medians. They do not establish
CPU or wall-time gains. The [benchmark results](../benchmarks.md) retain all losses
and source increases, including separate full-interop routes. Source instruction
changes in the complete selected comparison stay inside
the diagnostic tie band.

## Preserve complete compression validation

Every gzip member or Zstd frame in a gRPC payload contributes to one message.
Validate later checksums, trailers and trailing input too. Deflate is one complete
zlib stream. Bound decoded bytes, frame/member count and Zstd history separately;
empty frames can consume work without consuming output budget. Keep exact-cap
EOF/overflow probes. Reuse output storage without sharing compression history
between gRPC messages or repeating decompression in compatibility adapters.

A current 1 KiB gzip upload-client profile attributes extra work to repeated
initialization of flate2 writer output scratch. Common compressor initialization
is equal in that control. This is allocation-instrumented Callgrind attribution,
not shipping CPU or latency evidence. A safe `bufread::GzEncoder` over the existing
input slice, with a bounded initialized output-copy loop, is an **unimplemented**
experiment. Preserve independent members, CRC/trailer completion and writer errors;
compare compressible, incompressible and boundary inputs before selecting it.
Plaintext encode-scratch retention and compressed-output hints are separate changes.

## Generate only the selected work

Generated native and Prost dispatch select the method before boxing its future,
so a small call does not carry every method's largest state. Preserve conversions
and error behavior while measuring future size, generated bytes and compile cost.

Lazy views suit sparse reads; read-all handlers can validate nested bytes and then
parse them again during materialization. Test one checked merge for singular nested
fields while retaining the current representation. Repeated children already parse
eagerly. Preserve required fields, recursion, merge rules, extensions and unknown
fields, and compare sparse and read-all access. Skipping repeat validation needs a
typed schema-and-limits proof for the span; an arbitrary `Wire` is not that proof.

## Replace a backend for an identified cost

Keep codecs, transport, execution and optional adapters separate. A custom HTTP/2
engine or io_uring backend is justified by measured removable work or a required
ownership/concurrency boundary. Account for commands across tasks, flow control,
TLS, backpressure and compatibility. Follow the [HTTP/2 design](pbrs-h2.md) and
[benchmark contract](../benchmark-contract.md) before making comparative claims.

Correctness, remaining features/spec coverage, platform/release qualification,
shipping CPU/latency/throughput coverage and an actual final-source 24-hour soak
remain open. Older campaigns qualify only their recorded source. The capsule and
[open work](../../TODO.md) retain the next controls; passing a subset does not
establish production readiness or an overall performance ranking.
