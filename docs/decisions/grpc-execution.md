# gRPC execution and ownership

Source `af00f348` fixes required-field validation after proto2 message merging;
[its checks](../evidence/grpc-required-merge-20261026/README.md) retain the detecting
failures and consumer-regeneration requirement. Required-reachable validation can
allocate a temporary tree. The earlier transport at `933bf877` retires completed
DATA owners outside backend locks, preserves pre-shape reset ownership and adjusts
batch reservations. Its [dated record](../evidence/grpc-ownership-20261024/README.md)
has 3,840 passing workspace tests and source-bound sanitizer/toolchain results.
Those broader checks do not qualify later source changes.

Unary-server shipping CPU still loses to tonic. Lower allocation and Callgrind
instruction counts describe other costs. A separate ready-set experiment passes
a bounded causal control but has mixed physical CPU results, so it is not adopted.

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

The shipping trace diagnostic finds eventfd notifications and frequent changes
of thread between adjacent TCP operations. Four native-server traces contain
WAIT_BITSET_PRIVATE waits and no WAIT_PRIVATE engine-lock acquisition signature.
Do not retune the engine lock from an aggregate futex count. The separate
[ready-set experiment](../evidence/grpc-ready-set-20261024/README.md) drives its
controlled newly ready RPC in the same parent poll, preserves a sleeping sibling
and passes 87 focused checks. Its 48 captures / 360,000 RPCs have mixed CPU results,
so production adoption is deferred. Already queued notifications and another
ready-set fairness slice remain costs to examine. Notification forwarding needs
its own wake/cancellation correctness model. Tracing affects scheduling.

## Keep one deadline and terminal owner

Capture one absolute deadline when complete request headers are accepted, before
service waits or shape selection. `Rpc::deadline()` reports that fixed value.
Late caps only tighten it; getters, metadata changes and delayed first polls
cannot renew the budget. Check expiry after logging/interceptors and synchronous
service/handler construction before polling the returned future.

The outer owner begins lifecycle accounting before dispatch and keeps the terminal
capability through cooperative cancellation. The application future drops before
the owner, including when never polled. Shared deadline/cancellation state reuses
the existing `CancelFlag` allocation. Retained Request/RequestParts do not keep a
completed response capability alive. Unary accounting uses the writer's actual
committed status, including encoding and byte-admission errors; a later clock read
or payload destructor cannot replace that result.

Native error termination preserves initial headers and the current HTTP/2 frame
length, discards unsent DATA tails and sends one ending. A committed ending stays
committed. gRPC-Web status is BODY DATA: a completed-message boundary can preserve
base64 carry and append status; an unfinished message or trailer body cannot be
repaired by inserting another status, so that path resets the transport. Web
status still needs peer credit and cannot force progress through a blocked socket.

Completed DATA frames now move to a fixed retirement batch declared outside
engine and send-buffer lock guards. Fully consumed owners drop after unlocking;
partial payloads retain their unsent tails and final EOS intent. A full retirement
batch unlocks and destroys owners before staging continues; it does not force IO.
Controls cover normal reclaim, explicit post-flush reclaim, retired-owner unwind,
credit/EOS, reentry and IO error. Generic Buf operations still run under staging
locks; current-frame panic and other queue/EOF owner paths need separate review.

Pre-shape response ownership also retains the selected reset cause through
cancellation and unwind. These repairs do not preempt synchronous application
work or replace handler-isolation measurements.

## Reuse decoded storage when ownership returns

Source `beb4ecd9` introduced stream-local reuse of fully consumed, uniquely owned decoded `Vec` backing.
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

The [original decode reuse measurements](../evidence/grpc-decode-reuse-20261023/README.md)
belong to `beb4ecd9` versus `9d6fb9c4`. The [current benchmark tables](../benchmarks.md)
use `933bf877` against `7c02eb38`, retaining losses and every source cost increase.

## Preserve complete compression validation

Every gzip member or Zstd frame in a gRPC payload contributes to one message.
Validate later checksums, trailers and trailing input too. Deflate is one complete
zlib stream. Bound decoded bytes, frame/member count and Zstd history separately;
empty frames can consume work without consuming output budget. Keep exact-cap
EOF/overflow probes. Reuse output storage without sharing compression history
between gRPC messages or repeating decompression in compatibility adapters.

The gzip encoder now uses a fresh `bufread::GzEncoder` over each message's input
slice and one initialized 8 KiB output chunk. This removes repeated large writer
scratch initialization while preserving level/clamp behavior, complete member
CRC/ISIZE output and writer-error mapping. No compression history crosses message
boundaries. Incompressible/boundary inputs have correctness tests; the endpoint
performance harness uses zero-filled application payloads. Plaintext scratch
retention and compressed reservation remain separate work.

## Generate only the selected work

Router dispatch selects the existing boxed service future synchronously under
the outer owner. It does not retain the complete Rpc inside an initial async
state or add a second box. The private release proof measures Router's driver
at 4,288 → 544 bytes; Single stays 544 bytes, including a large application future.
That is compiler layout evidence, separate from allocation-site or speed claims.
A three-source unchanged-harness control measures **19.76% fewer unary / 7.72%
fewer bidi server requested bytes** versus `40f219fa`; small allocation-count
increases remain. Its 48 captures are separate from the full counter campaigns.

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

Remaining destructor/merge/specification coverage, features, platform/release
qualification, shipping CPU/latency/throughput and an actual final-source
24-hour soak remain open. Older campaigns qualify only their recorded source. The capsule and
[open work](../../TODO.md) retain the next controls; passing a subset does not
establish production readiness or an overall performance ranking.
