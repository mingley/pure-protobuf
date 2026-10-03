# Queued RPC deadline failure: deterministic protocol proof

The integrated all-feature gate at source `de0bcdf2` failed
`scenario_d_deadline_expires_waiting_for_slot_returns_deadline_exceeded`:
the second unary reached the service once despite expiring while the peer's
only stream was occupied. The original failed log is retained unchanged.
A later isolated execution of its exact ELF passed; that observation does not
clear the failure or qualify the gate.

The new raw-peer regression reproduces the defect without compiler contention
or wall-clock timing. With Tokio's paused clock, the peer advertises one stream
and holds the first RPC open. A second unary expires after 50 ms. Only after
that error, the peer increases its SETTINGS limit to two. The client then sends
the expired request's HEADERS, complete five-byte gRPC DATA with END_STREAM,
and RST_STREAM(CANCEL), in that order. The request can therefore execute before
the cancellation is processed by a real server. A second regression closes
the occupied first stream with peer RST_STREAM while retaining SETTINGS=1;
it produces the same late HEADERS/DATA/CANCEL sequence. Thus the defect also
occurs when the original peer limit is unchanged. This is a request-admission
defect, rather than an assertion that a particular scheduler run is flaky.

The retained [diagnostic source](deadline-admission/deadline_admission.rs)
is a **red regression**, outside Cargo's default integration-test discovery.
It was originally compiled from `pbrs-grpc/tests/deadline_admission.rs` at proof
commits `bac42c8e` and `a944c9a5`, and relocated with its exact bytes unchanged.
The archived compiler commands and source hashes refer to that original
location. Replaying the commands against the retained source requires changing
only the source path to `docs/evidence/deadline-admission/deadline_admission.rs`.
The original failing `retry_safety` fixture remains unchanged and active.
There is no production fix in this proof. The source base is
`a8545803b90717102fe29c5b48378a74d68d863c`; its client, transport, and original
retry fixture are unchanged from the failing source.

The final two-test probe remains **0 passed / 2 failed**, exit 101, with
unchanged source/library/executable hashes. Its
[raw log](deadline-admission/raw-peer-two-red.log) and
[pins and compiler command](deadline-admission/raw-peer-two-pins.json)
supplement the retained first growth-only proof below.

## Artifact provenance

- [Original all-feature failure](deadline-admission/original-all-features.log),
  SHA256 `acf23894aa2c5ec8db9e2355f061babddebc8c2d49b6a4434420274ecf40760b`.
- Original failing ELF `retry_safety-7c018c5b5d18c52b`, SHA256
  `b0b34c3c808dd6647ae81ede4d4333c0f09ec3106bc6a87d592cbf3c6d4f9819`.
- The proof links the same all-feature library fingerprint used by that ELF:
  `libpbrs_grpc-c5f7b84c09a7121a.rlib`, SHA256
  `69f87c4eccacb0828ed809bc8f251d60e2a4067b0f7bb2d42f0b3f8d2fd42a65`.
- [First deterministic red](deadline-admission/raw-peer-red.log) and
  [guarded red](deadline-admission/raw-peer-red-guarded.log) retain the actual
  HEADERS/DATA/reset frames and failed assertion. The guarded execution checks
  the proof ELF, original ELF, and linked library before and after launching;
  [pins](deadline-admission/raw-peer-pins.json) record exit 101 and no drift.
- [h2 source pins](deadline-admission/h2-source-pins.json) and
  [numbered excerpts](deadline-admission/h2-api-excerpts.txt) retain the exact
  0.4.19 implementation used for the diagnosis.

No Cargo build was required for this red: a small rustc test binary links the
already-built integration gate's immutable library and Tokio artifacts. The
probe makes no performance claim. The existing isolated test's passing result
is retained [separately](deadline-admission/original-elf-single.log).

## Why readiness and cancellation do not prevent this

`h2::SendRequest::clone` clears its per-handle `pending` stream. `poll_ready`
only waits for that handle's already-queued pending stream, so a fresh pool
clone can enqueue an additional request while the connection is at its peer
limit. Native deadline prechecks and biased races occur, but the request has
already entered h2's pending-open queue by the time the response wait expires.

In h2 0.4.19, `send_reset` explicitly retains queued HEADERS and DATA for a
pending-open stream. Sending RST_STREAM as the first frame on an idle stream
would violate HTTP/2, so h2 eventually sends the opening request and then the
reset. Removing that protection or rolling back the dependency is not an
acceptable repair.

## Stable public API feasibility

The available stable h2 API exposes `current_max_send_streams`, which is a
negotiated limit, and per-handle readiness. It exposes no atomic connection-wide
send-count reservation or capacity wait before HEADERS. `poll_capacity` is
DATA credit on a stream already created by `send_request`.

The feature-gated `num_active_streams` is unsuitable as an authoritative send
count: its implementation returns `Store.ids.len()`, including retained reset
streams. h2's actual send count instead decrements only when the stream is
closed, its pending frame queue is empty, and its buffered DATA is empty.
Logical RPC completion, submitted END_STREAM, or wrapper drop does not certify
that transition. `Connection::poll` returning Pending also does not certify
flush completion; it can return Pending from `streams.poll_complete` while
outbound IO or flow control remains blocked.

A native counter or semaphore released solely on logical completion could
therefore recreate this queue under backpressure. A limit checked before
HEADERS also needs to handle SETTINGS shrinking between readiness and
`send_request`. There is no approved production patch or new dependency/API
decision in this proof. The coordinator must review a backend capacity API or
another design that certifies the actual transport transition before shipping.

Any repair must cover dynamic SETTINGS zero/grow/shrink, cancellation of a
pending reservation, connection death and generation isolation, all four RPC
shapes, early responses during upload, and stream/resource-guard drop. The
Tower opaque-body transport uses native `grab` but opens HEADERS itself and
must receive the same protection. No defaults, security reset behavior,
dependencies, benchmark fixtures, or thresholds have changed here.

## Isolated backend component follow-up

The [component evidence pack](deadline-admission/components/README.md) retains
the rejected first prototype, an untested intermediate snapshot, seven passing
tests, a specific mixed-API waker red against that earlier library, and eighteen
passing component tests after the additive-only readiness correction. Complete
source archives, the five-file upstream patch, source/extern/compiler pins,
license, and raw logs are retained. No prototype code is compiled by the
shipping library. These component results do not clear the original integrated
failure or either raw RPC deadline red, and do not qualify native/Tower caller
integration, packaging, all RPC shapes, compression, or performance.
